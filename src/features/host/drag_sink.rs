//! Linux host end of the negotiated (`OUDD`) drag channel.
//!
//! A controller that advertises `openuuyc_drag_drop` uses this instead of the
//! clipboard OleDrop action once the host says hello. The account feature map
//! only opens that action for a peer at version 4.20.0 or newer, and this
//! build reports engine `1.0.0-rc.2`, so without the hello a Windows controller
//! refuses the gesture and the files never leave the viewer. Dropped files are
//! written under the user's Downloads directory — the same folder file-transfer
//! Send uses. Nothing is placed at the pointer, and drag-out is not offered.

use crate::{
    features::clipboard::{self, Clipboard},
    protocol::drag_drop::{self as wire, Packet, Payload},
};
use anyhow::{Result, anyhow};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

pub(super) struct Frame {
    pub bulk: bool,
    pub data: Vec<u8>,
}

type DropResult = std::result::Result<crate::features::file_transfer::import::Report, String>;

struct Live {
    clipboard: Clipboard,
    sender: tokio::task::JoinHandle<()>,
}

struct Drag {
    id: u64,
    buffered: Vec<Vec<u8>>,
    committed: bool,
    finished: bool,
    started: Instant,
    live: Option<Live>,
}

pub(super) struct Sink {
    token: u64,
    ready: bool,
    acked: bool,
    advertised: Option<bool>,
    hello_at: Option<Instant>,
    sequence: u64,
    drag: Option<Drag>,
    outgoing: Vec<Frame>,
    clip_tx: mpsc::Sender<Vec<u8>>,
    done_tx: mpsc::Sender<(u64, DropResult)>,
}

impl Sink {
    pub(super) fn new(
        token: u64,
        clip_tx: mpsc::Sender<Vec<u8>>,
        done_tx: mpsc::Sender<(u64, DropResult)>,
    ) -> Self {
        Self {
            token,
            ready: false,
            acked: false,
            advertised: None,
            hello_at: None,
            sequence: 0,
            drag: None,
            outgoing: Vec::new(),
            clip_tx,
            done_tx,
        }
    }

    /// `ready` is the whole gate: the controller asked for native drag, a
    /// session is being viewed, file clipboard and file access are on, and
    /// the FILE channel is open.
    pub(super) fn set_ready(&mut self, ready: bool) {
        self.expire();
        if self.ready && !ready {
            if self.advertised == Some(true) {
                self.queue(0, 0, false, Payload::Available(false));
                self.advertised = Some(false);
                tracing::info!(enabled = false, "linux host native file drag");
            }
            if self.drag.as_ref().is_some_and(|drag| !drag.finished) {
                self.finish(3, String::new());
            }
            self.acked = false;
            self.hello_at = None;
            self.ready = false;
            return;
        }
        self.ready = ready;
        if !ready {
            return;
        }
        let due = self
            .hello_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1));
        if self.acked || !due {
            return;
        }
        if self.advertised != Some(true) {
            tracing::info!(enabled = true, "linux host native file drag");
        }
        self.queue(0, 0, false, Payload::Hello(1));
        self.queue(0, 0, false, Payload::Available(true));
        self.advertised = Some(true);
        self.hello_at = Some(Instant::now());
    }

    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<()> {
        let Some(packet) = wire::decode(bytes)? else {
            return Ok(());
        };
        if packet.token != self.token {
            return Ok(());
        }
        match packet.payload {
            Some(Payload::HelloAck(_)) => self.acked = true,
            Some(Payload::Begin(begin)) if self.ready => {
                self.acked = true;
                if self.drag.as_ref().is_some_and(|drag| !drag.finished) {
                    self.finish(3, String::new());
                }
                if packet.drag == 0 || begin.point.is_none() {
                    return Ok(());
                }
                self.drag = Some(Drag {
                    id: packet.drag,
                    buffered: Vec::new(),
                    committed: false,
                    finished: false,
                    started: Instant::now(),
                    live: None,
                });
                if begin.reverse {
                    self.finish(4, "Linux 被控端不提供拖出".into());
                    return Ok(());
                }
                let seq = self.next_seq();
                self.queue(
                    packet.drag,
                    seq,
                    false,
                    Payload::Feedback(wire::Feedback {
                        ready: true,
                        effect: wire::COPY,
                    }),
                );
                tracing::info!(drag = packet.drag, "linux host accepted file drag");
            }
            Some(Payload::Data(data)) => {
                let Some(drag) = self.drag.as_mut() else {
                    return Ok(());
                };
                if drag.finished {
                    return Ok(());
                }
                if drag.committed {
                    if let Err(error) = self.feed(data) {
                        self.finish(4, error.to_string());
                    }
                } else {
                    drag.buffered.push(data);
                }
            }
            Some(Payload::Commit(_)) => {
                let Some(drag) = self.drag.as_mut() else {
                    return Ok(());
                };
                if drag.finished || drag.committed {
                    return Ok(());
                }
                drag.committed = true;
                drag.started = Instant::now();
                let buffered = std::mem::take(&mut drag.buffered);
                if let Err(error) = self.start_import(buffered) {
                    self.finish(4, error.to_string());
                }
            }
            Some(Payload::Cancel(_)) => self.finish(3, String::new()),
            _ => {}
        }
        Ok(())
    }

    pub(super) fn wrap_clipboard(&mut self, bytes: Vec<u8>) {
        let Some(drag) = self.drag.as_ref() else {
            return;
        };
        if drag.finished || !drag.committed {
            return;
        }
        let id = drag.id;
        let bulk = clipboard::carries_file_bytes(&bytes).unwrap_or(false);
        let seq = self.next_seq();
        match encode(self.token, id, seq, Payload::Data(bytes)) {
            Ok(data) => self.outgoing.push(Frame { bulk, data }),
            Err(error) => self.finish(4, error.to_string()),
        }
    }

    pub(super) fn on_done(&mut self, id: u64, result: DropResult) {
        if !self
            .drag
            .as_ref()
            .is_some_and(|drag| drag.id == id && !drag.finished)
        {
            return;
        }
        match result {
            Ok(report) if report.saved > 0 => {
                tracing::info!(
                    drag = id,
                    saved = report.saved,
                    total = report.total,
                    first = %report.first,
                    "linux host saved dragged files"
                );
                self.finish(2, String::new());
            }
            Ok(_) => self.finish(4, "没有文件被保存".into()),
            Err(error) => self.finish(4, error),
        }
    }

    pub(super) fn drain(&mut self) -> Vec<Frame> {
        std::mem::take(&mut self.outgoing)
    }

    fn expire(&mut self) {
        let timed_out = self.drag.as_ref().is_some_and(|drag| {
            drag.committed && !drag.finished && drag.started.elapsed() > Duration::from_secs(180)
        });
        if timed_out {
            self.finish(4, "文件拖放超时".into());
        }
    }

    fn start_import(&mut self, buffered: Vec<Vec<u8>>) -> Result<()> {
        let id = self
            .drag
            .as_ref()
            .map(|drag| drag.id)
            .ok_or_else(|| anyhow!("拖放已结束"))?;
        let clipboard = Clipboard::new();
        clipboard.platform(1);
        clipboard.host_role();
        clipboard.arm_receiver()?;
        let done_tx = self.done_tx.clone();
        clipboard.on_file_drop(Arc::new(move |result| {
            let _ = done_tx.try_send((id, result));
        }));
        let clip_tx = self.clip_tx.clone();
        let outgoing = clipboard.clone();
        let sender = tokio::spawn(outgoing.sender(move |_epoch, packet| {
            let clip_tx = clip_tx.clone();
            async move {
                clip_tx
                    .send(packet)
                    .await
                    .map_err(|_| anyhow!("拖放剪贴板发送队列已关闭"))
            }
        }));
        if let Some(drag) = self.drag.as_mut() {
            drag.live = Some(Live { clipboard: clipboard.clone(), sender });
        }
        for message in buffered {
            clipboard.receive(&Clipboard::note_file_drop(&message))?;
        }
        Ok(())
    }

    fn feed(&mut self, message: Vec<u8>) -> Result<()> {
        let live = self
            .drag
            .as_ref()
            .and_then(|drag| drag.live.as_ref())
            .ok_or_else(|| anyhow!("拖放接收尚未开始"))?;
        live.clipboard
            .receive(&Clipboard::note_file_drop(&message))?;
        Ok(())
    }

    fn finish(&mut self, stage: u32, message: String) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if drag.finished {
            return;
        }
        let id = drag.id;
        drag.finished = true;
        if let Some(live) = drag.live.take() {
            live.clipboard.suspend();
            live.sender.abort();
        }
        let message: String = message.chars().take(240).collect();
        let seq = self.next_seq();
        self.queue(
            id,
            seq,
            false,
            Payload::Finished(wire::Finished {
                stage,
                message,
                bytes_read: 0,
            }),
        );
    }

    fn next_seq(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1).max(1);
        self.sequence
    }

    fn queue(&mut self, drag: u64, sequence: u64, bulk: bool, payload: Payload) {
        match encode(self.token, drag, sequence, payload) {
            Ok(data) => self.outgoing.push(Frame { bulk, data }),
            Err(error) => tracing::warn!(%error, "拖放消息编码失败"),
        }
    }
}

fn encode(token: u64, drag: u64, sequence: u64, payload: Payload) -> Result<Vec<u8>> {
    wire::encode(Packet {
        token,
        capabilities: 0,
        drag,
        sequence,
        payload: Some(payload),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::drag_drop::{Begin, Point};

    fn sink() -> (Sink, mpsc::Receiver<Vec<u8>>, mpsc::Receiver<(u64, DropResult)>) {
        let (clip_tx, clip_rx) = mpsc::channel(32);
        let (done_tx, done_rx) = mpsc::channel(4);
        (Sink::new(7, clip_tx, done_tx), clip_rx, done_rx)
    }

    fn encode_at(drag: u64, sequence: u64, payload: Payload) -> Vec<u8> {
        encode(7, drag, sequence, payload).unwrap()
    }

    fn payloads(frames: &[Frame]) -> Vec<Payload> {
        frames
            .iter()
            .map(|frame| wire::decode(&frame.data).unwrap().unwrap().payload.unwrap())
            .collect()
    }

    #[test]
    fn hello_then_begin_offers_copy() {
        let (mut sink, _clip, _done) = sink();
        sink.set_ready(false);
        assert!(sink.drain().is_empty(), "idle host stays quiet");
        sink.set_ready(true);
        let hello = payloads(&sink.drain());
        assert!(
            matches!(hello.as_slice(), [Payload::Hello(1), Payload::Available(true)]),
            "{hello:?}"
        );
        sink.push(&encode_at(0, 0, Payload::HelloAck(1))).unwrap();
        let begin = encode_at(
            9,
            1,
            Payload::Begin(Begin {
                point: Some(Point {
                    screen: 0,
                    x: 0.4,
                    y: 0.6,
                }),
                reverse: false,
                preserve: false,
            }),
        );
        sink.push(&begin).unwrap();
        let answer = payloads(&sink.drain());
        assert!(
            matches!(
                answer.as_slice(),
                [Payload::Feedback(feedback)]
                    if feedback.ready && feedback.effect == wire::COPY
            ),
            "{answer:?}"
        );
        sink.push(&encode_at(9, 2, Payload::Cancel(true))).unwrap();
        let done = payloads(&sink.drain());
        assert!(
            matches!(done.as_slice(), [Payload::Finished(finished)] if finished.stage == 3),
            "{done:?}"
        );
    }

    #[test]
    fn reverse_drag_is_refused() {
        let (mut sink, _clip, _done) = sink();
        sink.set_ready(true);
        sink.drain();
        sink.push(&encode_at(
            3,
            1,
            Payload::Begin(Begin {
                point: Some(Point {
                    screen: 0,
                    x: 0.2,
                    y: 0.2,
                }),
                reverse: true,
                preserve: false,
            }),
        ))
        .unwrap();
        let answer = payloads(&sink.drain());
        assert!(
            matches!(answer.as_slice(), [Payload::Finished(finished)] if finished.stage == 4),
            "{answer:?}"
        );
    }

    #[tokio::test]
    async fn commit_writes_the_file_into_downloads() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _lock = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "openuuyc-drag-dst-{}",
            std::process::id()
        ));
        let source_dir = std::env::temp_dir().join(format!(
            "openuuyc-drag-src-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&source_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&source_dir).unwrap();
        let previous = std::env::var_os("XDG_DOWNLOAD_DIR");
        unsafe { std::env::set_var("XDG_DOWNLOAD_DIR", &dir) };
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => unsafe { std::env::set_var("XDG_DOWNLOAD_DIR", value) },
                    None => unsafe { std::env::remove_var("XDG_DOWNLOAD_DIR") },
                }
            }
        }
        let _restore = Restore(previous);

        let source = source_dir.join("src-note.txt");
        let body = b"dragged-from-controller";
        std::fs::write(&source, body).unwrap();

        let (controller, _offers) =
            Clipboard::isolated_files(Arc::new(|| true));
        controller.platform(1);
        controller.arm_receiver().unwrap();
        let (cout_tx, mut cout_rx) = mpsc::channel(64);
        tokio::spawn(controller.clone().sender(move |_epoch, packet| {
            let cout_tx = cout_tx.clone();
            async move {
                cout_tx
                    .send(packet)
                    .await
                    .map_err(|_| anyhow!("controller clipboard closed"))
            }
        }));
        controller
            .publish_files(vec![source.clone()])
            .await
            .expect("publish");

        let (mut sink, mut clip_rx, mut done_rx) = sink();
        sink.set_ready(true);
        sink.drain();
        sink.push(&encode_at(0, 0, Payload::HelloAck(1))).unwrap();
        sink.push(&encode_at(
            11,
            1,
            Payload::Begin(Begin {
                point: Some(Point {
                    screen: 0,
                    x: 0.5,
                    y: 0.5,
                }),
                reverse: false,
                preserve: false,
            }),
        ))
        .unwrap();
        sink.drain();
        sink.push(&encode_at(
            11,
            2,
            Payload::Commit(Point {
                screen: 0,
                x: 0.5,
                y: 0.5,
            }),
        ))
        .unwrap();

        let saved = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                tokio::select! {
                    packet = cout_rx.recv() => {
                        let Some(packet) = packet else { break None; };
                        let seq = sink.next_seq();
                        sink.push(&encode_at(11, seq, Payload::Data(packet))).unwrap();
                    }
                    packet = clip_rx.recv() => {
                        let Some(packet) = packet else { break None; };
                        sink.wrap_clipboard(packet);
                        for frame in sink.drain() {
                            let decoded = wire::decode(&frame.data).unwrap().unwrap();
                            let Some(Payload::Data(inner)) = decoded.payload else { continue; };
                            controller.receive(&inner).unwrap();
                        }
                    }
                    done = done_rx.recv() => {
                        let Some((id, result)) = done else { break None; };
                        sink.on_done(id, result);
                        break Some(sink.drain());
                    }
                }
            }
        })
        .await
        .expect("drag import timed out");

        let frames = saved.expect("import channel closed");
        let answer = payloads(&frames);
        assert!(
            matches!(answer.as_slice(), [Payload::Finished(finished)] if finished.stage == 2),
            "{answer:?}"
        );
        let landed = std::fs::read(dir.join("src-note.txt")).expect("file in Downloads");
        assert_eq!(landed, body);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&source_dir);
    }
}
