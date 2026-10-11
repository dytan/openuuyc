//! Linux capture/encoder worker: CPU frames from the Linux capture backend
//! into the software encoder. Mirrors the Windows worker minus GPU devices
//! and cross-adapter transfer.
use super::super::Lease;
use super::super::VideoConfig;
use super::super::capture;
use super::super::encoder;
use super::super::lock;
use super::super::output_size;
use super::Published;
use anyhow::Context as _;
use anyhow::Result;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[path = "capture_worker/metrics.rs"]
mod metrics;
#[path = "capture_worker/feed.rs"]
mod feed;

pub(super) fn capture_loop(
    mut screen: capture::Screen,
    handle: Lease,
    cancel: CancellationToken,
    encode_cancel: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    business_ready: Arc<AtomicBool>,
    config: Arc<Mutex<VideoConfig>>,
    keyframe: Arc<AtomicBool>,
    sender: mpsc::Sender<encoder::Encoded>,
    started: Instant,
    transport: crate::features::host::transport::Transport,
    negotiated: Arc<crate::features::host::format::Negotiated>,
    publication: tokio::sync::watch::Sender<Published>,
    pointer: Arc<Mutex<Option<crate::platform::windows::cursor_shape::Snapshot>>>,
) -> Result<()> {
    let _activity = crate::platform::windows::host_service::activity::Work::new();
    let _runtime = encoder::Runtime::new()?;
    // Capture runs on its own thread (`feed`) so grab+scale overlaps encode.
    let mut feed: Option<feed::Feed> = None;
    let mut backend_name: &'static str = "Linux";
    let mut last_hdr_available = false;
    let mut encoder = None;
    let mut current = None;
    let mut next = Instant::now();
    let mut paced_fps = 0;
    let mut cached = None::<capture::Frame>;
    let mut last_frame = None::<Instant>;
    let mut failed = std::collections::HashSet::new();
    let mut encode_errors = 0u32;
    let mut requested = None;
    let mut automatic = None::<crate::features::host::parameters::AutoQuality>;
    let mut initial_auto = true;
    let mut admission = crate::features::host::parameters::WindowAdmission::default();
    let mut generation = 0;
    let mut frame_metadata = std::collections::BTreeMap::new();
    let mut last_diagnostics = None;
    let mut metrics = metrics::Metrics::default();

    while !cancel.is_cancelled() && handle.requested() {
        let iteration_started = Instant::now();
        let mut wanted = *lock(&config);
        if !negotiated.permits_format(wanted.format) {
            let previous = wanted.revision;
            let chroma = wanted.format.chroma;
            let hdr = wanted.color.is_hdr();
            negotiated.apply(
                &mut wanted,
                None,
                chroma,
                hdr,
                (screen.width, screen.height),
            )?;
            let mut active = lock(&config);
            if active.revision != previous {
                continue;
            }
            wanted.revision = previous.wrapping_add(1);
            *active = wanted;
        }

        let requested_hdr = wanted.color.is_hdr();
        if feed.is_none() && transport.media_ready() && wanted.sending && wanted.capturing {
            // The authorized track is already negotiated. Start the capture-ahead
            // thread while ICE/DTLS is still connecting; the gate below still
            // forbids frame acquisition/encoding/sending until transport is up.
            // Nothing is kept warm outside this track's lease/cancellation scope.
            let opened_at = Instant::now();
            let connecting = !connected.load(Ordering::Acquire);
            feed = Some(feed::Feed::start(
                screen.clone(),
                config.clone(),
                handle.clone(),
                cancel.clone(),
            ));
            tracing::info!(
                elapsed_ms = opened_at.elapsed().as_millis(),
                connecting,
                "host capture feed started"
            );
            // Native setup may overlap incoming CaptureSetting changes. Read
            // the current revision before configuring or encoding any frame.
            continue;
        }
        if feed.is_some() {
            // T CCDCA0/CF3450 gate HDR on the actual source, not only the
            // requested setting. Keep the request so same-source HDR recovery
            // can re-enable the negotiated format without rewriting user intent.
            negotiated.apply_source(
                &mut wanted,
                (screen.width, screen.height),
                last_hdr_available,
            )?;
        }
        let settings = (
            wanted.revision,
            wanted.quality,
            wanted.auto_quality,
            wanted.bitrate,
            wanted.fps,
        );
        let changed = requested != Some(settings);
        if changed {
            automatic = (wanted.quality == 5).then(|| {
                crate::features::host::parameters::AutoQuality::new(
                    wanted.auto_quality,
                    wanted.maximum_quality,
                )
            });
            initial_auto = true;
            requested = Some(settings);
        }
        let candidate = negotiated.candidate(&wanted, (screen.width,screen.height),
            screen.render_adapter.unwrap_or(screen.adapter), &failed)
            .context("本会话的协商编码候选已全部失败")?;
        let hardware =
            candidate.capability.backend != crate::features::host::format::Backend::Software;
        let candidate_id = (
            candidate.capability.adapter,
            candidate.capability.backend,
            candidate.capability.format,
        );
        wanted.format = candidate.capability.format;
        if wanted.format.depth < 10 {
            wanted.color = crate::features::host::format::Color::Sdr;
        }
        wanted.maximum = candidate.maximum_for(wanted.requested_maximum);
        wanted.fps = wanted.fps.min(candidate.fps).min(screen.fps).max(1);
        metrics.report(wanted.fps, screen.adapter, candidate.capability.adapter);
        let period = Duration::from_secs_f64(1.0 / f64::from(wanted.fps));
        if paced_fps != wanted.fps {
            paced_fps = wanted.fps;
            next = Instant::now();
        }
        let maximum_quality = negotiated.maximum_quality(
            wanted.format,
            (screen.width, screen.height),
            wanted.maximum,
        );
        let budget_size =
            crate::media::geometry::fit_size(screen.width, screen.height, wanted.maximum);
        if let Some(auto) = automatic.as_mut() {
            auto.limit(maximum_quality);
            let sample = transport.automatic_rates();
            if wanted.sending
                && wanted.capturing
                && connected.load(Ordering::Acquire)
                && auto
                    .observe(
                        Instant::now(),
                        wanted.format,
                        budget_size,
                        wanted.fps,
                        last_frame.is_some_and(|at| at.elapsed() < Duration::from_secs(1)),
                        sample,
                    )
                    .is_some()
            {
                initial_auto = false;
            }
            wanted.quality = auto.quality();
        } else if wanted.quality != 6 {
            // The fallback encoder's real size limit also limits the fixed
            // quality budget and QoS label, not only the GPU texture size.
            wanted.quality = wanted.quality.min(maximum_quality);
        }
        transport.fps(wanted.fps);
        let bounds = if automatic.is_some() {
            crate::features::host::parameters::automatic(
                wanted.format,
                wanted.quality,
                budget_size,
                wanted.fps,
                initial_auto,
            )
        } else {
            crate::features::host::parameters::fixed(
                wanted.format,
                wanted.quality,
                wanted.bitrate,
                budget_size,
                wanted.fps,
            )
        };
        let quality_target = crate::features::host::format::QualityTarget {
            bitrate: bounds.maximum,
            fps: wanted.fps,
        };
        if wanted.sending && wanted.capturing {
            transport.quality(automatic.is_some(), wanted.quality, changed);
            transport.configure(bounds);
        } else {
            transport.pause();
        }
        if !connected.load(Ordering::Acquire)
            || !business_ready.load(Ordering::Acquire)
            || !transport.media_ready()
            || !wanted.sending
            || !wanted.capturing
        {
            if !wanted.capturing {
                if last_diagnostics.take().is_some() {
                    handle.video(None);
                }
                feed = None;
                encoder = None;
                current = None;
                publication.send_if_modified(|state| {
                    if state.capturing || state.visible {
                        state.capturing = false;
                        state.visible = false;
                        true
                    } else {
                        false
                    }
                });
            }
            cached = None;
            last_frame = None;
            keyframe.store(true, Ordering::Release);
            next = Instant::now();
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        if Instant::now() < next {
            std::thread::sleep(
                next.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(20)),
            );
            continue;
        }
        if sender.capacity() == 0 {
            metrics.backpressure_waits += 1;
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        let feed_ref = match feed.as_mut() {
            Some(feed_ref) => feed_ref,
            None => {
                feed = Some(feed::Feed::start(
                    screen.clone(),
                    config.clone(),
                    handle.clone(),
                    cancel.clone(),
                ));
                continue;
            }
        };
        if let Some(fatal) = feed_ref.fatal() {
            anyhow::bail!(fatal);
        }
        let wait_ms = ((1000.0 / f64::from(wanted.fps)).round() as u32).max(1);
        // Advance the planned deadline, not the (usually late) wake-up time.
        // Otherwise sleep/dispatch overhead is added to every frame period.
        // A whole missed period resets the clock instead of replaying a backlog.
        let loop_now = Instant::now();
        next = if loop_now.saturating_duration_since(next) >= period {
            loop_now + period
        } else {
            next + period
        };
        // The producer already grabbed the newest frame while we encoded the
        // previous one, so this normally returns immediately with a fresh frame.
        let snapshot = feed_ref.wait(wait_ms);
        let captured = match &snapshot {
            Some(snap) => {
                // Record the real grab+scale cost so throughput logs stay honest.
                metrics.capture.record(snap.grab + snap.scale);
                *lock(&pointer) = snap.cursor.clone();
                if generation != snap.generation {
                    generation = snap.generation;
                    encoder = None;
                    cached = None;
                    current = None;
                    last_frame = None;
                    keyframe.store(true, Ordering::Release);
                }
                screen = snap.screen.clone();
                backend_name = snap.backend_name;
                last_hdr_available = snap.hdr_available;
                snap.available
            }
            None => {
                // No fresh frame within the period: fall back to the cached one.
                metrics.backpressure_waits += 1;
                true
            }
        };
        // A selected source remains attached during temporary capture loss.
        // Removing its track here can make the receiver unsubscribe and prevent
        // the next capture attempt. Visibility has its own permission report.
        publication.send_if_modified(|state| {
            let changed = state.screen != screen || !state.capturing;
            if state.screen != screen {
                state.screen.clone_from(&screen);
            }
            state.capturing = true;
            changed
        });
        handle.source(&screen, backend_name);
        if wanted.color.is_hdr()
            != (requested_hdr && last_hdr_available && wanted.format.depth == 10)
        {
            // Capture recovery or source refresh changed the effective format.
            // Rebuild preprocessing/encoder before admitting this frame.
            cached = None;
            continue;
        }
        if !captured {
            cached = None;
            publication.send_if_modified(|state| {
                if state.visible {
                    state.visible = false;
                    true
                } else {
                    false
                }
            });
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let frame = match snapshot.and_then(|snap| snap.frame) {
            Some(frame) => {
                metrics.new_frames += u64::from(frame.is_new);
                cached = Some(frame.clone());
                frame
            }
            None => {
                metrics.empty_captures += 1;
                let Some(mut frame) = cached.clone() else {
                    continue;
                };
                if last_frame.is_some_and(|at| {
                    at.elapsed() < Duration::from_secs_f64(1.0 / f64::from(wanted.fps.min(30)))
                }) {
                    continue;
                }
                frame.captured = Instant::now();
                frame.is_new = false;
                frame
            }
        };
        let size = output_size(frame.width, frame.height, wanted.quality);
        let device = ();
        if current.is_none_or(|(old_size, old_candidate)| {
            old_size != size || old_candidate != (candidate_id, wanted.color)
        }) {
            // Release before replacement to avoid consuming a second driver
            // session solely for a size/candidate transition (T C2D840).
            drop(encoder.take());
            let created = if hardware {
                encoder::Encoder::hardware_format(
                    &device,
                    size,
                    wanted.format,
                    crate::features::host::format::Rate {
                        target: bounds.initial.max(300_000),
                        peak: bounds.initial.max(300_000),
                        fps: wanted.fps,
                        quality: wanted.quality,
                        quality_target,
                    },
                    wanted.color,
                )
            } else {
                encoder::Encoder::software_format(
                    &device,
                    size,
                    wanted.format,
                    crate::features::host::format::Rate {
                        target: bounds.initial.max(300_000),
                        peak: bounds.initial.max(300_000),
                        fps: wanted.fps,
                        quality: wanted.quality,
                        quality_target,
                    },
                )
            };
            encoder = match created {
                Ok(created) => {
                    handle.encoder(
                        created.implementation(),
                        created.maximum_size(),
                        &screen,
                        backend_name,
                    );
                    Some(created)
                }
                Err(error) => {
                    tracing::warn!(%error,?candidate_id,"host encoder initialization failed; disabling this candidate");
                    failed.insert(candidate_id);
                    encoder = None;
                    cached = None;
                    current = None;
                    continue;
                }
            };
            current = Some((size, (candidate_id, wanted.color)));
            frame_metadata.clear();
            encode_errors = 0;
            keyframe.store(true, Ordering::Release);
        }
        let active_encoder = encoder.as_mut().context("缺少编码器")?;
        let peak = transport.media_rate().min(bounds.maximum);
        // A zero/tiny allocation is a transport pause, not an encoder failure.
        // Do not fabricate bandwidth by clamping it back to a quality floor.
        let codec_minimum = crate::features::host::parameters::MIN_ENCODER_RATE;
        if peak < codec_minimum {
            continue;
        }
        let (media_rate, discard_frame) =
            admission.next(peak, codec_minimum, transport.cwnd_ratio());
        if discard_frame {
            metrics.admission_drops += 1;
            continue;
        }
        let encoded = (|| -> Result<_> {
            let rate_started = Instant::now();
            let reconfigured = active_encoder.configure_rate(crate::features::host::format::Rate {
                target: media_rate,
                peak,
                fps: wanted.fps,
                quality: wanted.quality,
                quality_target,
            });
            metrics.rate.record(rate_started.elapsed());
            if reconfigured? {
                keyframe.store(true, Ordering::Release);
            }
            // Network requests are rate-limited at their RTCP entry. Explicit
            // quality/source controls and failed inputs must not wait 600ms.
            let force = keyframe.swap(false, Ordering::AcqRel);
            let encode_started = Instant::now();
            let timestamp = (frame.captured.duration_since(started).as_nanos() / 100) as i64;
            frame_metadata.insert(
                timestamp,
                (
                    frame.captured,
                    encode_started,
                    frame.is_new,
                    frame.hdr_metadata,
                ),
            );
            while frame_metadata.len() > 2048 {
                frame_metadata.pop_first();
            }
            let encoded = active_encoder.encode_cancellable(
                &frame.texture,
                timestamp,
                force,
                &encode_cancel,
            );
            metrics.encode.record(encode_started.elapsed());
            let encoded = encoded?;
            for output in &encoded {
                if let Some(actual) = crate::media::video_format::parse_stream_format(
                    output.format.codec.media(),
                    &output.data,
                ) {
                    anyhow::ensure!(
                        actual.chroma_format_idc == output.format.chroma
                            && actual.bit_depth_luma == output.format.depth
                            && actual.bit_depth_chroma == output.format.depth
                            && (actual.visible_width, actual.visible_height) == size,
                        "编码器实际输出偏离协商格式"
                    );
                }
            }
            Ok((encoded, force, Instant::now()))
        })();
        metrics.work.record(iteration_started.elapsed());
        if cancel.is_cancelled() || !handle.requested() {
            break;
        }
        let (encoded, force, encode_finished) = match encoded {
            Ok(encoded) => {
                encode_errors = 0;
                encoded
            }
            Err(error) => {
                encode_errors += 1;
                tracing::warn!(%error,hardware,consecutive=encode_errors,"host encoding failed");
                keyframe.store(true, Ordering::Release);
                next = Instant::now() + Duration::from_secs_f64(1.0 / f64::from(wanted.fps));
                if encode_errors >= 10
                    || error.downcast_ref::<encoder::SwitchCandidate>().is_some()
                {
                    // T C2C610: disable the failed candidate. Do not endlessly
                    // reopen that same encoder after its consecutive failures.
                    failed.insert(candidate_id);
                    encoder = None;
                    current = None;
                    cached = None;
                    last_frame = None;
                }
                continue;
            }
        };
        metrics.encoded_frames += encoded.len() as u64;
        if force && !encoded.iter().any(|frame| frame.keyframe) {
            keyframe.store(true, Ordering::Release);
        }
        last_frame = Some(frame.captured);
        if !encoded.is_empty() {
            let diagnostics = super::super::ActiveEncoding {
                preferred_gpu: negotiated.preference().0,
                preferred_codec: negotiated.preference().1,
                selection: negotiated.selection_reason(&candidate.capability, !failed.is_empty()),
                backend: candidate.capability.backend,
                adapter: candidate.capability.adapter,
                format: wanted.format,
                size,
                fps: wanted.fps,
                target_bps: media_rate,
            };
            if last_diagnostics != Some(diagnostics) {
                handle.video(Some(diagnostics));
                last_diagnostics = Some(diagnostics);
            }
            let mut active = lock(&config);
            if active.revision == wanted.revision {
                active.reported_quality = wanted.quality;
            }
            let state = Published {
                screen: screen.clone(),
                capturing: true,
                visible: true,
                quality: wanted.quality,
                fps: wanted.fps,
                format: wanted.format,
                budget_size,
                encoder: Some(candidate.capability.backend),
                capture: backend_name.into(),
            };
            publication.send_if_modified(|old| {
                if *old != state {
                    *old = state;
                    true
                } else {
                    false
                }
            });
        }
        for mut frame in encoded {
            if let Some((captured, input_started, is_new, metadata)) =
                frame_metadata.remove(&frame.timestamp_100ns)
            {
                frame.is_new = is_new;
                frame.color.hdr_metadata = if frame.color.transfer == 16 {
                    metadata
                } else {
                    None
                };
                frame.timing = Some(encoder::FrameTiming {
                    captured,
                    encode_started: input_started,
                    encode_finished,
                });
            }
            // No B frames: older metadata cannot belong to a later output.
            while frame_metadata
                .first_key_value()
                .is_some_and(|(&timestamp, _)| timestamp < frame.timestamp_100ns)
            {
                frame_metadata.pop_first();
            }
            sender.blocking_send(frame).context("画面发送队列已关闭")?;
        }
    }
    Ok(())
}
