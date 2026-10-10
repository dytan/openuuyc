//! XDG user directories for the file browser roots.
use std::path::PathBuf;

pub(crate) fn known_folders() -> Vec<(PathBuf, &'static str, &'static str)> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let user_dirs = home
        .as_ref()
        .map(|home| home.join(".config/user-dirs.dirs"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let lookup = |key: &str, fallback: &str| -> Option<PathBuf> {
        let home = home.as_ref()?;
        let value = user_dirs.lines().find_map(|line| {
            let rest = line.trim().strip_prefix(key)?.strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_owned())
        });
        let path = match value {
            Some(value) => PathBuf::from(value.replace("$HOME", &home.to_string_lossy())),
            None => home.join(fallback),
        };
        path.is_dir().then_some(path)
    };
    [
        ("XDG_DOCUMENTS_DIR", "Documents", "文档", "document"),
        ("XDG_DESKTOP_DIR", "Desktop", "桌面", "desktop"),
        ("XDG_PICTURES_DIR", "Pictures", "图片", "picture"),
        ("XDG_VIDEOS_DIR", "Videos", "视频", "video"),
        ("XDG_MUSIC_DIR", "Music", "音乐", "music"),
        ("XDG_DOWNLOAD_DIR", "Downloads", "下载", "download"),
    ]
    .into_iter()
    .filter_map(|(key, fallback, name, icon)| Some((lookup(key, fallback)?, name, icon)))
    .collect()
}
