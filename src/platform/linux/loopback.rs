//! Host desktop-audio capture on Linux: the default (or chosen) sink's monitor
//! source, read through `parec` (PulseAudio or PipeWire's pulse server) as
//! 48 kHz stereo float. Same block contract as the Windows WASAPI loopback.
use crate::media::audio::sender::{BLOCK, RATE};
use anyhow::{Context, Result, ensure};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

pub(crate) type Samples = Arc<dyn Fn([f32; BLOCK * 2], Instant) + Send + Sync>;

pub(crate) struct Endpoint {
    pub id: String,
    name: String,
}
impl Endpoint {
    pub fn name(&self) -> String {
        self.name.clone()
    }
}
#[derive(Default)]
pub(crate) struct Change {
    pub refresh: bool,
    pub rebuild: bool,
}
pub(crate) struct Devices;

const DEFAULT: &str = "@DEFAULT_MONITOR@";

fn which(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(tool).is_file())
    })
}

impl Devices {
    pub fn new() -> Result<Self> {
        ensure!(which("parec"), "缺少 parec（请安装 libpulse / pipewire-pulse）");
        Ok(Self)
    }
    pub fn changes(&self, _selected: Option<&str>) -> Change {
        Change::default()
    }
    pub fn endpoint(&self, selected: Option<&str>) -> Result<Option<Endpoint>> {
        if let Some(id) = selected
            && let Some(found) = self.list()?.into_iter().find(|e| e.id == id)
        {
            return Ok(Some(found));
        }
        Ok(Some(Endpoint {
            id: DEFAULT.into(),
            name: "默认播放设备".into(),
        }))
    }
    pub fn list(&self) -> Result<Vec<Endpoint>> {
        let output = Command::new("pactl")
            .args(["list", "short", "sinks"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .context("无法运行 pactl")?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split('\t').nth(1))
            .map(|sink| Endpoint {
                id: format!("{sink}.monitor"),
                name: sink.to_owned(),
            })
            .collect())
    }
}

pub(crate) struct Capture {
    child: Child,
    producing: Arc<AtomicBool>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Capture {
    pub fn producing(&self) -> bool {
        self.producing.load(Ordering::Acquire)
    }
    pub fn poll(&mut self) -> Result<()> {
        if let Some(status) = self.child.try_wait()? {
            anyhow::bail!("parec 已退出：{status}");
        }
        Ok(())
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(crate) fn open(endpoint: &Endpoint, samples: Samples) -> Result<Capture> {
    let mut child = Command::new("parec")
        .arg(format!("--device={}", endpoint.id))
        .args([
            "--format=float32le",
            &format!("--rate={RATE}"),
            "--channels=2",
            "--latency-msec=20",
            "--raw",
            "--client-name=OpenUUYC",
            "--stream-name=desktop-audio",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动 parec")?;
    let mut stdout = child.stdout.take().context("parec 无输出管道")?;
    let producing = Arc::new(AtomicBool::new(false));
    let flag = producing.clone();
    let reader = std::thread::Builder::new()
        .name("host-audio-parec".into())
        .spawn(move || {
            let mut bytes = vec![0u8; BLOCK * 2 * 4];
            loop {
                if stdout.read_exact(&mut bytes).is_err() {
                    break;
                }
                let mut block = [0f32; BLOCK * 2];
                for (sample, chunk) in block.iter_mut().zip(bytes.chunks_exact(4)) {
                    *sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                }
                flag.store(true, Ordering::Release);
                samples(block, Instant::now());
            }
        })
        .context("无法启动音频读取线程")?;
    Ok(Capture {
        child,
        producing,
        reader: Some(reader),
    })
}
