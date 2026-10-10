//! Remote-cursor snapshot types shared with the (Windows-only) host capture path.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub hotspot: [u32; 2],
    pub kind: i32,
    pub png: String,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Snapshot {
    pub showing: bool,
    pub x: i32,
    pub y: i32,
    pub image: Option<Image>,
}
