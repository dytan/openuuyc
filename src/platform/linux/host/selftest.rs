//! Offline self-test of the Linux controlled-side pipeline: enumerate outputs,
//! capture one frame, encode it with the software H.264 encoder, decode it back,
//! and probe /dev/uinput. A real controller connection still needs a peer.
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

use crate::platform::linux::capture::{self, Desktop};
use crate::platform::linux::host::input;

pub fn run(bench: Option<u32>) -> Result<String> {
    let mut report = String::new();
    macro_rules! line {
        ($($arg:tt)*) => {{ report.push_str(&format!($($arg)*)); report.push('\n'); }};
    }

    line!("OpenUUYC Linux 被控自检");
    line!("会话类型: {:?}", capture::kind());
    if let Some(reason) = capture::unavailable_reason() {
        line!("采集不可用: {reason}");
    }

    let screens = capture::screens().context("枚举显示器失败")?;
    line!("显示器数量: {}", screens.len());
    for s in &screens {
        line!(
            "  [{}] {} {}x{} @{}Hz primary={} scale={:?} identity={:?}",
            s.id, s.device_name, s.width, s.height, s.fps, s.primary, s.dpi_scale, s.identity
        );
    }
    let screen = screens
        .iter()
        .find(|s| s.primary)
        .or_else(|| screens.first())
        .cloned()
        .context("没有可采集的显示器")?;

    let mut desktop = Desktop::open_selected(&screen).context("打开采集源失败")?;
    line!("采集后端: {}", desktop.backend_name());

    let mut frame = None;
    let started = Instant::now();
    for _ in 0..30 {
        if let Some(f) = desktop
            .next(100, 2, false, false, (1920, 1080))
            .context("采集帧失败")?
        {
            frame = Some(f);
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            break;
        }
    }
    let frame = frame.context("5 秒内未取得画面")?;
    line!(
        "采集成功: {}x{}, {} 字节, 用时 {} ms",
        frame.width,
        frame.height,
        frame.texture.len(),
        started.elapsed().as_millis()
    );

    // Encode the captured frame.
    use crate::features::host::format::{self, Format, QualityTarget, Rate};
    let rate = Rate {
        target: 8_000_000,
        peak: 8_000_000,
        fps: 30,
        quality: 2,
        quality_target: QualityTarget {
            bitrate: 8_000_000,
            fps: 30,
        },
    };
    let mut encoder = crate::platform::linux::encoder::Encoder::software_format(
        &(),
        (frame.width, frame.height),
        Format::AVC,
        rate,
    )
    .context("创建软件编码器失败")?;
    let encode_started = Instant::now();
    let packets = encoder
        .encode(&frame.texture, 0, true)
        .context("编码失败")?;
    let total: usize = packets.iter().map(|p| p.data.len()).sum();
    line!(
        "编码成功: {} 个包, {} 字节, 用时 {} ms, 实现={}",
        packets.len(),
        total,
        encode_started.elapsed().as_millis(),
        encoder.implementation()
    );
    let _ = format::Backend::Software;

    // Decode the bitstream back.
    let mut decoder = openuuyc_codec::decoder::Decoder::new(openuuyc_codec::Codec::H264, &[])
        .context("创建软件解码器失败")?;
    let mut decoded = 0;
    for packet in &packets {
        decoded += decoder.push(&packet.data, 0).context("解码失败")?.len();
    }
    line!("解码成功: 还原 {decoded} 帧");

    // uinput availability and creation.
    if input::present() {
        match input::self_test() {
            Ok(()) => line!("uinput: 可创建虚拟输入设备（键鼠注入可用）"),
            Err(e) => line!("uinput: /dev/uinput 存在但无法创建设备: {e:#}"),
        }
    } else {
        line!("uinput: /dev/uinput 不存在（加载 uinput 内核模块）");
    }

    // Optional steady-state benchmark: capture -> scale/convert -> encode,
    // reporting per-stage p50/p95 and the achievable (CPU-bound) frame rate.
    if let Some(frames) = bench {
        let frames = frames.clamp(1, 100_000);
        line!("");
        line!("基准：{frames} 帧（采集→缩放→编码）");
        let mut grab_ms = Vec::with_capacity(frames as usize);
        let mut scale_ms = Vec::with_capacity(frames as usize);
        let mut encode_ms = Vec::with_capacity(frames as usize);
        let mut frame_ms = Vec::with_capacity(frames as usize);
        let mut bytes_total = 0usize;
        // Reuse one encoder sized to the capture output.
        let mut bench_encoder = crate::platform::linux::encoder::Encoder::software_format(
            &(),
            (frame.width, frame.height),
            Format::AVC,
            rate,
        )
        .context("创建基准编码器失败")?;
        let mut implementation = bench_encoder.implementation();
        let bench_started = Instant::now();
        let mut captured_frames = 0u32;
        for i in 0..frames {
            let loop_started = Instant::now();
            let Some((f, grab, scale)) = desktop
                .next_timed(100, 2, false, false, (1920, 1080))
                .context("基准采集失败")?
            else {
                continue;
            };
            // A size change (first frame settles the real capture size) needs a
            // fresh encoder; rebuild once then continue timing.
            if (f.width, f.height) != (frame.width, frame.height) && i == 0 {
                bench_encoder = crate::platform::linux::encoder::Encoder::software_format(
                    &(), (f.width, f.height), Format::AVC, rate,
                ).context("重建基准编码器失败")?;
                implementation = bench_encoder.implementation();
            }
            let enc_started = Instant::now();
            // Periodic IDR so we measure realistic steady-state P-frames too.
            let force = i % 60 == 0;
            let packets = bench_encoder
                .encode(&f.texture, (i as i64) * 10_000, force)
                .context("基准编码失败")?;
            let enc = enc_started.elapsed();
            bytes_total += packets.iter().map(|p| p.data.len()).sum::<usize>();
            grab_ms.push(grab.as_secs_f64() * 1000.0);
            scale_ms.push(scale.as_secs_f64() * 1000.0);
            encode_ms.push(enc.as_secs_f64() * 1000.0);
            frame_ms.push(loop_started.elapsed().as_secs_f64() * 1000.0);
            captured_frames += 1;
        }
        let elapsed = bench_started.elapsed();
        let pct = |v: &mut Vec<f64>, p: f64| -> f64 {
            if v.is_empty() { return 0.0; }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
            v[idx.min(v.len() - 1)]
        };
        let mut g = grab_ms.clone();
        let mut sc = scale_ms.clone();
        let mut en = encode_ms.clone();
        let mut fr = frame_ms.clone();
        line!("  采集帧数: {captured_frames}  码流: {} KB  实现={implementation}", bytes_total / 1024);
        line!("  screencopy 抓帧: p50 {:.1} ms  p95 {:.1} ms", pct(&mut g, 0.50), pct(&mut g, 0.95));
        line!("  缩放/转换:      p50 {:.1} ms  p95 {:.1} ms", pct(&mut sc, 0.50), pct(&mut sc, 0.95));
        line!("  编码:           p50 {:.1} ms  p95 {:.1} ms", pct(&mut en, 0.50), pct(&mut en, 0.95));
        line!("  单帧串行合计:   p50 {:.1} ms  p95 {:.1} ms", pct(&mut fr, 0.50), pct(&mut fr, 0.95));
        let fps = captured_frames as f64 / elapsed.as_secs_f64();
        line!("  串行可达帧率:   {:.1} fps（{} 帧用时 {} ms）", fps, captured_frames, elapsed.as_millis());
        let serial_p50 = pct(&mut frame_ms.clone(), 0.50);
        if serial_p50 > 0.0 {
            line!("  若采集/编码分线程并行，上限约 {:.1} fps",
                1000.0 / pct(&mut grab_ms.clone(), 0.50).max(pct(&mut encode_ms.clone(), 0.50)).max(0.1));
        }

        // VA-API path: encode N frames on the GPU and decode them back with the
        // software and VA-API decoders to confirm the bitstream is conformant.
        line!("");
        match crate::platform::linux::encoder::Encoder::hardware_format(
            &(),
            (frame.width, frame.height),
            Format::AVC,
            rate,
            crate::features::host::format::Color::Sdr,
        ) {
            Err(e) => {
                line!("VA-API 编码不可用（将使用软件编码）：{e:#}");
            }
            Ok(mut va) => {
                line!("VA-API 基准：{frames} 帧（采集→转换/上传→GPU编码→解码校验），实现={}", va.implementation());
                let mut va_grab = Vec::new();
                let mut va_scale = Vec::new();
                let mut va_enc = Vec::new();
                let mut va_bytes = 0usize;
                let mut units: Vec<Vec<u8>> = Vec::new();
                let mut va_frames = 0u32;
                let started = Instant::now();
                for i in 0..frames {
                    let Some((f, grab, scale)) = desktop
                        .next_timed(100, 2, false, false, (1920, 1080))
                        .context("VA-API 基准采集失败")?
                    else {
                        continue;
                    };
                    if (f.width, f.height) != (frame.width, frame.height) {
                        continue;
                    }
                    let enc_started = Instant::now();
                    let force = i % 60 == 0;
                    let packets = va
                        .encode_cancellable(
                            &f.texture,
                            (i as i64) * 10_000,
                            force,
                            &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                        )
                        .context("VA-API 基准编码失败")?;
                    va_enc.push(enc_started.elapsed().as_secs_f64() * 1000.0);
                    va_grab.push(grab.as_secs_f64() * 1000.0);
                    va_scale.push(scale.as_secs_f64() * 1000.0);
                    for p in &packets {
                        va_bytes += p.data.len();
                        if units.len() < 240 {
                            units.push(p.data.clone());
                        }
                    }
                    va_frames += 1;
                }
                let elapsed = started.elapsed();
                // Decode validation (software decoder: strict, authoritative).
                let mut sw = openuuyc_codec::decoder::Decoder::new(openuuyc_codec::Codec::H264, &[])
                    .context("创建软件解码器失败")?;
                let mut sw_errors = 0u32;
                let mut sw_decoded = 0usize;
                for u in &units {
                    match sw.push(u, 0) {
                        Ok(frames) => sw_decoded += frames.len(),
                        Err(_) => sw_errors += 1,
                    }
                }
                if std::env::var_os("OPENUUYC_VAAPI_DUMP").is_some() {
                    for (idx, u) in units.iter().take(2).enumerate() {
                        let _ = std::fs::write(format!("/tmp/va_unit{idx}.h264"), u);
                        let mut types = Vec::new();
                        let mut i = 0usize;
                        while i + 4 < u.len() {
                            let sc4 = u[i] == 0 && u[i+1] == 0 && u[i+2] == 0 && u[i+3] == 1;
                            let sc3 = u[i] == 0 && u[i+1] == 0 && u[i+2] == 1;
                            if sc4 { types.push(u[i+4] & 0x1f); i += 4; continue; }
                            if sc3 { types.push(u[i+3] & 0x1f); i += 3; continue; }
                            i += 1;
                        }
                        line!("  unit{idx} len={} 首字节={:02x?} NAL类型={:?}", u.len(), &u[..u.len().min(8)], types);
                    }
                }
                line!("  码流: {} KB", va_bytes / 1024);
                line!("  screencopy 抓帧: p50 {:.1} ms  p95 {:.1} ms", pct(&mut va_grab.clone(), 0.50), pct(&mut va_grab.clone(), 0.95));
                line!("  缩放:            p50 {:.1} ms  p95 {:.1} ms", pct(&mut va_scale.clone(), 0.50), pct(&mut va_scale.clone(), 0.95));
                line!("  转换+上传+编码:  p50 {:.1} ms  p95 {:.1} ms", pct(&mut va_enc.clone(), 0.50), pct(&mut va_enc.clone(), 0.95));
                let fps = va_frames as f64 / elapsed.as_secs_f64();
                line!("  串行可达帧率:    {:.1} fps（{} 帧用时 {} ms）", fps, va_frames, elapsed.as_millis());
                line!("  若采集/编码分线程并行，上限约 {:.1} fps",
                    1000.0 / pct(&mut va_grab.clone(), 0.50).max(pct(&mut va_enc.clone(), 0.50)).max(0.1));
                line!("  软件解码校验: 还原 {sw_decoded} 帧, 解码错误 {sw_errors} 个");
                match crate::platform::linux::decoder::vaapi_decode_roundtrip(frame.width, frame.height, &units) {
                    Ok(n) => line!("  VA-API 解码校验: 还原 {n} 帧"),
                    Err(e) => line!("  VA-API 解码校验: 不可用（{e:#}）"),
                }
                if sw_errors == 0 && sw_decoded > 0 {
                    line!("  ✓ VA-API 码流解码干净，可安全启用硬件编码");
                } else {
                    line!("  ✗ VA-API 码流解码存在问题，请勿启用硬件编码");
                }
            }
        }
    }

    line!("自检完成。真实被控仍需对端控制器连接测试。");
    Ok(report)
}
