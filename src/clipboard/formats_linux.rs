//! Linux clipboard format helpers (stub).
//! TODO(linux): map MIME types (text/plain, image/png, text/uri-list) to wire formats.
use crate::clipboard::protocol::ClipboardFormat;

#[derive(Clone)]
pub struct Format {
    pub wire: ClipboardFormat,
    pub local: u32,
}

pub fn register(_name: &str) -> u32 {
    0
}

pub fn name(id: u32) -> String {
    format!("linux-format-{id}")
}

pub fn file_format(_id: u32, name: &str) -> bool {
    name.eq_ignore_ascii_case("text/uri-list")
}

pub fn supported(id: u32, name: &str, files: bool) -> bool {
    let _ = (id, files);
    name.eq_ignore_ascii_case("text/plain")
        || name.eq_ignore_ascii_case("image/png")
        || file_format(id, name)
}
