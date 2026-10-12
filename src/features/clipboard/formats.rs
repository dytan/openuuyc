use super::*;
use std::io::Cursor;
#[cfg(windows)]
use windows::{
    Win32::System::DataExchange::{GetClipboardFormatNameW, RegisterClipboardFormatW},
    core::PCWSTR,
};
#[derive(Clone)]
pub(super) struct Format {
    pub wire: ClipboardFormat,
    pub local: u32,
}
#[cfg(windows)]
pub(super) fn register(name: &str) -> u32 {
    let s: Vec<u16> = name.encode_utf16().chain([0]).collect();
    unsafe { RegisterClipboardFormatW(PCWSTR(s.as_ptr())) }
}
#[cfg(windows)]
pub(super) fn name(id: u32) -> String {
    let mut s = [0u16; 1024];
    let n = unsafe { GetClipboardFormatNameW(id, &mut s) };
    String::from_utf16_lossy(&s[..n.max(0) as usize])
}

/// The wire protocol speaks Windows clipboard-format ids, so a process-local
/// registry stands in for the system one: the same name always maps to the same
/// id within this client, starting at CF_PRIVATEFIRST like Windows does.
#[cfg(not(windows))]
fn registry() -> &'static Mutex<(HashMap<String, u32>, HashMap<u32, String>)> {
    static REGISTRY: std::sync::OnceLock<Mutex<(HashMap<String, u32>, HashMap<u32, String>)>> =
        std::sync::OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}

#[cfg(not(windows))]
pub(super) fn register(name: &str) -> u32 {
    if name.is_empty() {
        return 0;
    }
    let mut registry = lock(registry());
    if let Some(id) = registry.0.get(name) {
        return *id;
    }
    let id = 0xc000 + registry.0.len() as u32;
    if id > 0xffff {
        return 0;
    }
    registry.0.insert(name.to_owned(), id);
    registry.1.insert(id, name.to_owned());
    id
}

#[cfg(not(windows))]
pub(super) fn name(id: u32) -> String {
    if let Some(name) = standard_name(id) {
        return name.to_owned();
    }
    lock(registry()).1.get(&id).cloned().unwrap_or_default()
}

/// The predefined CF_* formats this client can name without the system.
#[cfg(not(windows))]
const fn standard_name(id: u32) -> Option<&'static str> {
    Some(match id {
        1 => "CF_TEXT",
        2 => "CF_BITMAP",
        3 => "CF_METAFILEPICT",
        4 => "CF_SYLK",
        5 => "CF_DIF",
        6 => "CF_TIFF",
        7 => "CF_OEMTEXT",
        8 => "CF_DIB",
        9 => "CF_PALETTE",
        10 => "CF_PENDATA",
        11 => "CF_RIFF",
        12 => "CF_WAVE",
        13 => "CF_UNICODETEXT",
        14 => "CF_ENHMETAFILE",
        15 => "CF_HDROP",
        16 => "CF_LOCALE",
        17 => "CF_DIBV5",
        _ => return None,
    })
}

/// File names arriving from a Windows peer must stay valid Windows names, even
/// when this client writes them on a filesystem that would accept more.
pub(super) fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.encode_utf16().count() < 260
        && !s.contains(['\0', ':'])
        && !s.starts_with(['\\', '/'])
        && s.split(['\\', '/']).all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with(['.', ' '])
                && !p.chars().any(|c| c < ' ' || "<>\"|?*".contains(c))
                && !matches!(
                    p.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
}
pub(super) fn file_format(id: u32, name: &str) -> bool {
    id == 15
        || [
            "FileGroupDescriptorW",
            "FileGroupDescriptor",
            "FileContents",
            "FileName",
            "FileNameW",
            "Shell IDList Array",
            "Preferred DropEffect",
            "DropDescription",
            "Shell Object Offsets",
            "DataObjectAttributes",
            "DataObjectAttributesRequiringElevation",
            "public.file-url",
        ]
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name))
}
pub(super) fn supported(id: u32, name: &str, files: bool) -> bool {
    if name.eq_ignore_ascii_case("OpenUUYC.Clipboard.Remote")
        || name.eq_ignore_ascii_case("DataObject")
        || name.eq_ignore_ascii_case("Ole Private Data")
    {
        return false;
    }
    if file_format(id, name) {
        return files
            && (matches!(
                name,
                "FileGroupDescriptorW" | "FileContents" | "public.file-url"
            ) || name.eq_ignore_ascii_case("Preferred DropEffect"));
    }
    matches!(
        id,
        1 | 4 | 5 | 6 | 7 | 8 | 10 | 11 | 12 | 13 | 14 | 16 | 17 | 0x81 | 0x8e
    ) || (id >= 0xc000 && !name.is_empty())
}
/// Windows-registered / MIME names a peer may use for image payloads instead of
/// (or in addition to) CF_DIB. Map them onto local CF_DIB so the Linux host can
/// decode them the same way as a native DIB offer.
fn image_format_name(name: &str) -> bool {
    matches!(
        name,
        "PNG"
            | "JFIF"
            | "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/bmp"
            | "image/tiff"
            | "public.png"
            | "public.jpeg"
            | "public.jpeg-image"
            | "public.tiff"
            | "CF_DIB"
            | "CF_DIBV5"
    )
}

pub(super) fn incoming(f: ClipboardFormat, platform: i32, files: bool) -> Option<Format> {
    let local = if platform == 4 {
        match f.name.as_str() {
            "public.utf8-plain-text" | "public.utf16-plain-text" | "public.plain-text" => 13,
            // Mac pasteboard often offers PNG/JPEG rather than TIFF.
            "public.tiff" | "public.png" | "public.jpeg" | "public.jpeg-image" => 8,
            "public.html" => register("HTML Format"),
            "public.file-url" if files => register("FileGroupDescriptorW"),
            _ => return None,
        }
    } else if f.name.is_empty() {
        f.id
    } else if image_format_name(&f.name) {
        // Mac speaking the Windows dialect often advertises "PNG" / "JFIF"
        // registered formats (and sometimes MIME names) rather than CF_DIB.
        if f.name.eq_ignore_ascii_case("CF_DIBV5") || f.id == 17 {
            17
        } else {
            8
        }
    } else {
        register(&f.name)
    };
    if local == 0 || (!supported(local, &name(local), files) && platform != 4 && !image_format_name(&f.name)) {
        return None;
    }
    Some(Format { wire: f, local })
}
pub(super) fn outgoing(ids: &[(u32, String)], platform: i32, files: bool) -> Vec<Format> {
    let mut out = Vec::new();
    for (id, n) in ids {
        if !supported(*id, n, files) {
            continue;
        }
        if platform != 4 {
            out.push(Format {
                wire: ClipboardFormat {
                    id: *id,
                    name: n.clone(),
                },
                local: *id,
            });
            continue;
        }
        let names: &[&str] = match *id {
            13 => &["public.utf8-plain-text", "public.utf16-plain-text"],
            // Prefer PNG for macOS peers; keep TIFF as a fallback UTI.
            8 => &["public.png", "public.tiff"],
            _ if n == "HTML Format" => &["public.html"],
            _ if n == "FileGroupDescriptorW" => &["public.file-url"],
            _ => &[],
        };
        for n in names {
            out.push(Format {
                wire: ClipboardFormat {
                    id: out.len() as u32 + 1,
                    name: (*n).into(),
                },
                local: *id,
            });
        }
    }
    out
}
fn utf16(bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() % 2 == 0, "无效的Unicode剪贴板");
    let mut words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    while words.last() == Some(&0) {
        words.pop();
    }
    if words.first() == Some(&0xfeff) {
        words.remove(0);
    }
    Ok(String::from_utf16(&words)?)
}
pub(super) fn unicode(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}
fn field(bytes: &[u8], key: &str) -> Option<usize> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    head.lines()
        .find_map(|l| l.strip_prefix(key)?.trim().parse().ok())
}
fn windows_lines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}
fn encoded_image_to_dib(data: Vec<u8>, kind: image::ImageFormat) -> Result<Vec<u8>> {
    let mut reader = image::ImageReader::with_format(Cursor::new(data), kind);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DATA as u64);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let decoded = reader.decode()?;
    let rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    // Build a 32-bit bottom-up BI_RGB DIB (no file header) matching CF_DIB.
    let pixels = (w as usize).saturating_mul(h as usize);
    ensure!(pixels.saturating_mul(4) <= MAX_DATA, "图片过大");
    let mut dib = Vec::with_capacity(40 + pixels * 4);
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(w as i32).to_le_bytes());
    dib.extend_from_slice(&(h as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&((pixels * 4) as u32).to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    for row in (0..h as usize).rev() {
        for column in 0..w as usize {
            let pixel = &rgba.as_raw()[(row * w as usize + column) * 4..][..4];
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Ok(dib)
}

fn sniff_image_format(data: &[u8], name: &str) -> Option<image::ImageFormat> {
    if data.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'])
        || name.eq_ignore_ascii_case("PNG")
        || name.eq_ignore_ascii_case("image/png")
        || name == "public.png"
    {
        return Some(image::ImageFormat::Png);
    }
    if data.starts_with(&[0xff, 0xd8, 0xff])
        || name.eq_ignore_ascii_case("JFIF")
        || name.eq_ignore_ascii_case("image/jpeg")
        || name.eq_ignore_ascii_case("image/jpg")
        || name == "public.jpeg"
        || name == "public.jpeg-image"
    {
        return Some(image::ImageFormat::Jpeg);
    }
    if (data.starts_with(b"BM") && data.len() > 14)
        || name.eq_ignore_ascii_case("image/bmp")
    {
        return Some(image::ImageFormat::Bmp);
    }
    if name.eq_ignore_ascii_case("image/tiff") || name == "public.tiff" {
        return Some(image::ImageFormat::Tiff);
    }
    None
}

pub(super) fn convert(data: Vec<u8>, f: &Format, platform: i32, outbound: bool) -> Result<Vec<u8>> {
    // Windows-dialect peers (incl. macOS controllers talking to our Windows-
    // spoofed Linux host) may ship PNG/JFIF payloads under a registered name.
    // Decode those to CF_DIB on the way in; encode DIB→PNG/JPEG on the way out
    // when we advertised a registered image name alongside CF_DIB.
    if platform != 4 {
        if let Some(kind) = sniff_image_format(&data, &f.wire.name) {
            if outbound {
                // Local image cache is always a packed CF_DIB.
                if matches!(
                    kind,
                    image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Tiff
                ) {
                    let bmp = dib_to_bmp(&data)?;
                    let mut reader =
                        image::ImageReader::with_format(Cursor::new(bmp), image::ImageFormat::Bmp);
                    let mut limits = image::Limits::default();
                    limits.max_alloc = Some(MAX_DATA as u64);
                    limits.max_image_width = Some(32768);
                    limits.max_image_height = Some(32768);
                    reader.limits(limits);
                    let image = reader.decode()?;
                    let mut out = Cursor::new(Vec::new());
                    image.write_to(&mut out, kind)?;
                    let encoded = out.into_inner();
                    ensure!(encoded.len() <= MAX_DATA, "转换后的剪贴板内容过大");
                    return Ok(encoded);
                }
                return Ok(data);
            }
            // Inbound: BM-prefixed is a BMP file; strip to DIB. Raw CF_DIB
            // (header size 40..124) is left alone. Encoded PNG/JPEG → DIB.
            if kind == image::ImageFormat::Bmp && data.starts_with(b"BM") && data.len() >= 14 {
                return Ok(data[14..].to_vec());
            }
            if kind == image::ImageFormat::Bmp {
                return Ok(data);
            }
            return encoded_image_to_dib(data, kind);
        }
        return Ok(data);
    }
    let out = match f.wire.name.as_str() {
        "public.utf8-plain-text" | "public.plain-text" => {
            if outbound {
                utf16(&data)?.replace("\r\n", "\n").into_bytes()
            } else {
                unicode(&windows_lines(
                    std::str::from_utf8(&data)?
                        .trim_end_matches('\0')
                        .trim_start_matches('\u{feff}'),
                ))
            }
        }
        "public.utf16-plain-text" => {
            if outbound {
                let s = utf16(&data)?.replace("\r\n", "\n");
                s.encode_utf16().flat_map(u16::to_le_bytes).collect()
            } else {
                unicode(&windows_lines(&utf16(&data)?))
            }
        }
        "public.html" => {
            if outbound {
                let start =
                    field(&data, "StartHTML:").ok_or_else(|| anyhow!("HTML剪贴板缺少起始位置"))?;
                let end =
                    field(&data, "EndHTML:").ok_or_else(|| anyhow!("HTML剪贴板缺少结束位置"))?;
                ensure!(start <= end && end <= data.len(), "无效的HTML剪贴板范围");
                data[start..end].to_vec()
            } else {
                let html = std::str::from_utf8(&data)?.trim_end_matches('\0');
                let before = "<html><body><!--StartFragment-->";
                let after = "<!--EndFragment--></body></html>";
                let header = |a, b, c, d| {
                    format!(
                        "Version:1.0\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n"
                    )
                };
                let len = header(0, 0, 0, 0).len();
                let mut result = header(
                    len,
                    len + before.len() + html.len() + after.len(),
                    len + before.len(),
                    len + before.len() + html.len(),
                )
                .into_bytes();
                result.extend_from_slice(before.as_bytes());
                result.extend_from_slice(html.as_bytes());
                result.extend_from_slice(after.as_bytes());
                result.push(0);
                result
            }
        }
        "public.tiff" | "public.png" | "public.jpeg" | "public.jpeg-image" => {
            let wire = f.wire.name.as_str();
            let (bytes, kind) = if outbound {
                (dib_to_bmp(&data)?, image::ImageFormat::Bmp)
            } else {
                let kind = match wire {
                    "public.png" => image::ImageFormat::Png,
                    "public.jpeg" | "public.jpeg-image" => image::ImageFormat::Jpeg,
                    _ => image::ImageFormat::Tiff,
                };
                (data, kind)
            };
            let mut reader = image::ImageReader::with_format(Cursor::new(bytes), kind);
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(MAX_DATA as u64);
            limits.max_image_width = Some(32768);
            limits.max_image_height = Some(32768);
            reader.limits(limits);
            let image = reader.decode()?;
            let mut out = Cursor::new(Vec::new());
            let outbound_fmt = match wire {
                "public.png" => image::ImageFormat::Png,
                "public.jpeg" | "public.jpeg-image" => image::ImageFormat::Jpeg,
                _ => image::ImageFormat::Tiff,
            };
            image.write_to(
                &mut out,
                if outbound {
                    outbound_fmt
                } else {
                    image::ImageFormat::Bmp
                },
            )?;
            let data = out.into_inner();
            if outbound {
                data
            } else {
                ensure!(data.len() >= 14, "无效的位图输出");
                data[14..].to_vec()
            }
        }
        _ => data,
    };
    ensure!(out.len() <= MAX_DATA, "转换后的剪贴板内容过大");
    Ok(out)
}
fn dib_to_bmp(d: &[u8]) -> Result<Vec<u8>> {
    ensure!(d.len() >= 40, "无效的DIB");
    let u32at = |i| u32::from_le_bytes(d[i..i + 4].try_into().unwrap());
    let header = u32at(0) as usize;
    ensure!(
        matches!(header, 40 | 52 | 56 | 108 | 124) && header <= d.len(),
        "不支持的DIB头"
    );
    let bits = u16::from_le_bytes([d[14], d[15]]);
    let compression = u32at(16);
    let used = u32at(32) as usize;
    let palette = if used != 0 {
        used
    } else if bits <= 8 {
        1usize << bits
    } else {
        0
    };
    let masks = if header == 40 && compression == 3 {
        12
    } else if header == 40 && compression == 6 {
        16
    } else {
        0
    };
    let offset = header
        .checked_add(masks)
        .and_then(|v| v.checked_add(palette.checked_mul(4)?))
        .ok_or_else(|| anyhow!("DIB尺寸溢出"))?;
    ensure!(offset <= d.len(), "无效的DIB像素偏移");
    let mut out = Vec::with_capacity(14 + d.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + d.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&((14 + offset) as u32).to_le_bytes());
    out.extend_from_slice(d);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_utf8_text_roundtrip() {
        let format = Format {
            wire: ClipboardFormat {
                id: 1,
                name: "public.utf8-plain-text".into(),
            },
            local: 13,
        };
        let local = unicode("hello\nmac");
        let wire = convert(local.clone(), &format, 4, true).unwrap();
        assert_eq!(wire, b"hello\nmac");
        let back = convert(wire, &format, 4, false).unwrap();
        assert_eq!(utf16(&back).unwrap().replace("\r\n", "\n"), "hello\nmac");
    }

    #[test]
    fn mac_outgoing_text_utis() {
        let out = outgoing(&[(13, String::new())], 4, false);
        let names: Vec<_> = out.iter().map(|f| f.wire.name.as_str()).collect();
        assert!(names.contains(&"public.utf8-plain-text"));
        assert!(names.contains(&"public.utf16-plain-text"));
    }

    #[test]
    fn windows_dialect_png_name_maps_to_dib() {
        let format = incoming(
            ClipboardFormat {
                id: 0,
                name: "PNG".into(),
            },
            1,
            false,
        )
        .expect("PNG should map");
        assert_eq!(format.local, 8);
    }

    #[test]
    fn windows_dialect_png_bytes_convert_to_dib() {
        // 1x1 red PNG
        let png: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82];
        let format = Format {
            wire: ClipboardFormat {
                id: 0,
                name: "PNG".into(),
            },
            local: 8,
        };
        let dib = convert(png.to_vec(), &format, 1, false).unwrap();
        assert!(dib.len() >= 40);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 1);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 1);
    }
}
