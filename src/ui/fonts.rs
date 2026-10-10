//! The complete egui fonts, compressed at build time and shared by all windows.
use egui::{FontData, FontDefinitions, FontFamily, FontTweak};
use std::{
    io::Read,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

pub(crate) fn definitions() -> FontDefinitions {
    static FONTS: LazyLock<FontDefinitions> = LazyLock::new(|| {
        let mut fonts = FontDefinitions::empty();
        // Keep egui 0.36.1's exact font set, fallback order and emoji metrics.
        // Font sources and licenses come from epaint_default_fonts of that version.
        macro_rules! font {
            ($name:literal, $scale:expr) => {{
                let mut bytes = Vec::new();
                flate2::read::ZlibDecoder::new(
                    include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".zlib")).as_slice(),
                )
                .read_to_end(&mut bytes)
                .expect("invalid bundled font");
                fonts.font_data.insert(
                    $name.into(),
                    Arc::new(FontData::from_owned(bytes).tweak(FontTweak {
                        scale: $scale,
                        ..Default::default()
                    })),
                );
            }};
        }
        font!("Hack", 1.0);
        font!("Ubuntu-Light", 1.0);
        font!("NotoEmoji-Regular", 0.81);
        font!("emoji-icon-font", 0.90);
        fonts.families.insert(
            FontFamily::Monospace,
            [
                "Hack",
                "Ubuntu-Light",
                "NotoEmoji-Regular",
                "emoji-icon-font",
            ]
            .map(String::from)
            .into(),
        );
        fonts.families.insert(
            FontFamily::Proportional,
            ["Ubuntu-Light", "NotoEmoji-Regular", "emoji-icon-font"]
                .map(String::from)
                .into(),
        );
        fonts
    });
    FONTS.clone()
}

pub(crate) fn install(ctx: &egui::Context) {
    let mut fonts = definitions();
    let system = system_cjk_candidates()
        .into_iter()
        .find_map(|(path, index)| std::fs::read(&path).ok().map(|bytes| (path, index, bytes)));
    if let Some((path, index, bytes)) = system {
        let name = "system-cjk".to_owned();
        let mut data = FontData::from_owned(bytes);
        data.index = index;
        fonts.font_data.insert(name.clone(), Arc::new(data));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
        tracing::debug!(path = %path.display(), index, "installed system CJK font");
    } else {
        tracing::warn!("no system CJK font found; non-Latin labels may be unavailable");
    }
    ctx.set_fonts(fonts);
    ctx.request_repaint();
}

/// `(path, face_index)`; the face index selects a face inside `.ttc` collections.
fn system_cjk_candidates() -> Vec<(PathBuf, u32)> {
    #[cfg(windows)]
    {
        let directory = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        ["msyh.ttc", "msyhbd.ttc", "simhei.ttf", "simsun.ttc"]
            .into_iter()
            .map(|name| (directory.join(name), 0))
            .collect()
    }
    #[cfg(not(windows))]
    {
        // Noto Sans/Serif CJK *.ttc face order: 0=JP 1=KR 2=SC 3=TC 4=HK.
        // Face 2 is Simplified Chinese.
        const NOTO_SC: u32 = 2;
        [
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Regular.otf",
            "/usr/share/fonts/opentype/source-han-sans/SourceHanSans-Regular.otf",
            "/usr/share/fonts/noto/NotoSansSC-Regular.otf",
            "/usr/share/fonts/truetype/arphic/uming.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        ]
        .into_iter()
        .map(|path| {
            let noto = path.contains("NotoSansCJK-Regular.ttc")
                || path.contains("NotoSerifCJK-Regular.ttc");
            (PathBuf::from(path), if noto { NOTO_SC } else { 0 })
        })
        .collect()
    }
}
