//! Linux clipboard adapter stub.
//! TODO(linux): integrate wl-clipboard / X11 clipboard (e.g. arboard) for text/images/files.
use crate::clipboard::{
    Inner,
    protocol::{ClipboardFormat, ClipboardRequestKind},
};
use anyhow::Result;
use std::sync::Weak;

pub enum Command {
    Activate(Weak<Inner>),
    Remove(u64),
    Offer(Weak<Inner>, u64, Vec<ClipboardFormat>),
    Request(Weak<Inner>, u64, i64, ClipboardRequestKind),
    Text(Weak<Inner>, u64, i64, String),
}

pub fn start() -> Result<()> {
    tracing::warn!(target: "openuuyc::clipboard", "TODO(linux): native clipboard sync not implemented");
    Ok(())
}

pub fn post(command: Command) -> Result<()> {
    match command {
        Command::Remove(_) => Ok(()),
        other => {
            let _ = other;
            Ok(())
        }
    }
}

pub fn pump() {}

pub fn shutdown() {}

pub fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 255
        && !s.contains(['/', '\\', '\0'])
        && s != "."
        && s != ".."
}
