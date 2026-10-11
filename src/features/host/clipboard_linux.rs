//! In-process Linux host clipboard.
//!
//! Windows runs clipboard I/O in a user-session agent behind a named pipe. On
//! Linux the GUI already owns the session, so we drive `features::clipboard`
//! directly: TEXT-channel packets feed the protocol, and an OleDrop/AutoSave
//! FormatList is turned into a write under the user's Downloads folder by
//! `native_linux` (see `Command::Offer`).
use crate::features::clipboard::Clipboard;
use crate::protocol::peer_platform::PeerPlatform;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use webrtc::data_channel::RTCDataChannel;

pub(crate) mod agent {
    /// Linux hosts run inside the interactive user session.
    pub(crate) fn desktop_available() -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub files: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            files: true,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Status {
    pub active: bool,
    pub files: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub dragging: bool,
}

struct Shared {
    generation: u64,
    channel: Option<u16>,
    sender: Option<mpsc::Sender<Vec<u8>>>,
    receiver: Option<mpsc::Receiver<Vec<u8>>>,
    official_drop: Option<crate::account::feature_ability::FeatureCatalog>,
}

#[derive(Clone)]
pub(crate) struct Receiver(Arc<Mutex<Shared>>);

impl Default for Receiver {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(Shared {
            generation: 0,
            channel: None,
            sender: None,
            receiver: None,
            official_drop: None,
        })))
    }
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Receiver {
    pub fn official_drop(&self, catalog: Option<crate::account::feature_ability::FeatureCatalog>) {
        lock(&self.0).official_drop = catalog;
    }
    pub fn native(&self, _enabled: bool) {}
    pub fn bind_file(&self, _channel: &Arc<RTCDataChannel>) {}
    pub fn close_file(&self, _channel: &Arc<RTCDataChannel>) {}
    pub fn receive_file(&self, _channel: &Arc<RTCDataChannel>, _bytes: &[u8]) -> Result<()> {
        // Native OLE drag FILE channel is Windows-only; official drop uses TEXT.
        Ok(())
    }
    pub fn bind(&self, channel: u16) -> u64 {
        let (sender, receiver) = mpsc::channel(32);
        let mut state = lock(&self.0);
        state.generation = state.generation.wrapping_add(1);
        state.channel = Some(channel);
        state.sender = Some(sender);
        state.receiver = Some(receiver);
        state.generation
    }
    pub fn receive(&self, generation: u64, bytes: &[u8]) -> Result<bool> {
        ensure!(bytes.len() < 524288, "剪贴板消息过大");
        if !crate::protocol::drag_drop::is_packet(bytes)
            && !crate::features::clipboard::is_message(bytes)?
        {
            return Ok(false);
        }
        let state = lock(&self.0);
        if state.generation != generation {
            return Ok(true);
        }
        if let Some(sender) = state.sender.as_ref() {
            sender
                .try_send(bytes.to_vec())
                .map_err(|_| anyhow::anyhow!("剪贴板接收队列已满或关闭"))?;
        }
        Ok(true)
    }
}

pub(super) fn snapshot(clipboard: &Clipboard) -> Status {
    let state = clipboard.snapshot();
    Status {
        active: state.active,
        files: state.active && state.files,
        error: state.error,
        dragging: clipboard.dragging(),
    }
}

pub(super) async fn run(
    receiver: Receiver,
    mut routes: tokio::sync::watch::Receiver<super::peer::ReportRoutes>,
    lease: super::Lease,
    connected: Arc<AtomicBool>,
    cancel: CancellationToken,
    platform: PeerPlatform,
    _input: super::input::Receiver,
    _screens: impl Fn() -> (Vec<crate::media::capture::Screen>, bool) + Send + Sync + 'static,
) {
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    while !cancel.is_cancelled() && lease.requested() {
        let channel = routes
            .borrow()
            .text
            .as_ref()
            .and_then(std::sync::Weak::upgrade);
        if let Some(channel) = channel {
            if let Err(error) = binding(
                &receiver,
                &mut routes,
                &lease,
                &connected,
                &cancel,
                platform,
                channel,
            )
            .await
            {
                tracing::warn!(%error, "host clipboard binding stopped");
                lease.clipboard_status(Status {
                    error: Some(error.to_string()),
                    ..Default::default()
                });
                // Drain until the TEXT route changes so we do not replay a
                // half-processed drop against a new channel.
                loop {
                    if cancel.is_cancelled() || !lease.requested() {
                        return;
                    }
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        changed = routes.changed() => {
                            if changed.is_err() { return; }
                            break;
                        }
                        _ = tick.tick() => {}
                    }
                }
            }
        } else {
            tokio::select! {
                _ = cancel.cancelled() => return,
                changed = routes.changed() => {
                    if changed.is_err() { return; }
                }
                _ = tick.tick() => {}
            }
        }
    }
}

async fn binding(
    receiver: &Receiver,
    routes: &mut tokio::sync::watch::Receiver<super::peer::ReportRoutes>,
    lease: &super::Lease,
    connected: &Arc<AtomicBool>,
    cancel: &CancellationToken,
    platform: PeerPlatform,
    channel: Arc<RTCDataChannel>,
) -> Result<()> {
    let (generation, mut rx) = {
        let mut state = lock(&receiver.0);
        ensure!(state.channel == Some(channel.id()), "剪贴板通道已被替换");
        (
            state.generation,
            state
                .receiver
                .take()
                .ok_or_else(|| anyhow::anyhow!("剪贴板通道已关闭"))?,
        )
    };
    let stop = cancel.child_token();
    let _guard = stop.clone().drop_guard();

    let clipboard = Clipboard::guarded(Some(Arc::new(agent::desktop_available)));
    // The host registers with the account as a Windows device (platform 1) so it
    // is controllable and advertises remote_clipboard_v5. Every controller,
    // including macOS, therefore speaks the Windows clipboard-format dialect on
    // the wire (CF ids such as CF_UNICODETEXT=13 with an empty name, "HTML
    // Format", CF_DIB) rather than Mac UTIs. Translate with the Windows dialect
    // regardless of the controller's real platform; using the controller's
    // platform (e.g. 4 = Mac) made incoming formats unrecognised and silently
    // dropped the paste in both directions.
    let _ = platform;
    clipboard.platform(1);
    clipboard.host_role();

    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(32);
    let sender_future = clipboard.sender(move |_epoch, packet| {
        let out_tx = out_tx.clone();
        async move {
            out_tx
                .send(packet)
                .await
                .map_err(|_| anyhow::anyhow!("剪贴板发送队列已关闭"))
        }
    });
    let sender_task = tokio::spawn(sender_future);

    let mut permission: Option<bool> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let result = async {
        loop {
            if stop.is_cancelled() || !lease.requested() {
                break;
            }
            if !routes
                .borrow()
                .text
                .as_ref()
                .is_some_and(|c| c.ptr_eq(&Arc::downgrade(&channel)))
            {
                break;
            }
            if lock(&receiver.0).generation != generation {
                break;
            }

            let settings = lease.clipboard_settings();
            let level = routes.borrow().clipboard;
            let ready = settings.enabled && level >= 1 && connected.load(Ordering::Acquire);
            let files = settings.files && level >= 2 && ready;
            clipboard.set_enabled(ready)?;
            clipboard.set_files(settings.files);
            clipboard.policy(ready, settings.files);

            if permission != Some(files) {
                channel
                    .send_text_bytes(&bytes::Bytes::from(
                        crate::features::stream_control::publisher::clipboard_permission(files),
                    ))
                    .await?;
                tracing::info!(files, ready, "linux host clipboard permission");
                permission = Some(files);
            }

            let snap = clipboard.snapshot();
            lease.clipboard_status(Status {
                active: ready,
                files,
                error: snap.error,
                dragging: clipboard.dragging(),
            });

            tokio::select! {
                _ = stop.cancelled() => break,
                changed = routes.changed() => {
                    if changed.is_err() { break; }
                }
                _ = tick.tick() => {}
                message = rx.recv() => {
                    let Some(message) = message else { break; };
                    if crate::protocol::drag_drop::is_packet(&message) {
                        // Native drag is Windows-only; ignore.
                        continue;
                    }
                    tracing::debug!(bytes = message.len(), "host clipboard packet");
                    if let Err(error) = clipboard.receive(&message) {
                        tracing::warn!(error = format!("{error:#}"), "处理远端剪贴板消息失败");
                    }
                }
                packet = out_rx.recv() => {
                    let Some(packet) = packet else { break; };
                    channel
                        .send_text_bytes(&bytes::Bytes::from(packet))
                        .await?;
                }
            }
        }
        Ok(())
    }
    .await;

    clipboard.suspend();
    sender_task.abort();
    let _ = sender_task.await;
    result
}
