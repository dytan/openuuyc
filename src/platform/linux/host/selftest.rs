//! Offline self-test of the Linux controlled-side pipeline: enumerate outputs,
//! capture one frame, encode it with the software H.264 encoder, decode it back,
//! and probe /dev/uinput. A real controller connection still needs a peer.
use anyhow::{Context, Result};
use std::time::{Duration, Instant};

use crate::platform::linux::capture::{self, Desktop};
use crate::platform::linux::host::input;

pub fn run() -> Result<String> {
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

    line!("自检完成。真实被控仍需对端控制器连接测试。");
    Ok(report)
}
