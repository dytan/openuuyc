//! Linux device facts used for the device identity and diagnostics.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Hardware {
    pub name: String,
    pub system_uuid: String,
    pub machine_guid: String,
    pub os: String,
    pub base_board: String,
    pub cpu: String,
    pub video: Vec<String>,
    pub mac: String,
    pub memory: i64,
    pub screen: String,
    pub errors: Vec<String>,
}

fn read_trimmed(path: &str) -> Result<String> {
    Ok(std::fs::read_to_string(path)
        .with_context(|| format!("读取 {path} 失败"))?
        .trim()
        .to_owned())
}

pub(crate) fn computer_name() -> Result<String> {
    read_trimmed("/proc/sys/kernel/hostname")
        .or_else(|_| read_trimmed("/etc/hostname"))
        .context("读取主机名失败")
}

/// Human readable OS string (`PRETTY_NAME` from os-release plus kernel version).
pub(crate) fn os_name() -> String {
    let pretty = ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| "Linux".to_owned());
    match read_trimmed("/proc/sys/kernel/osrelease") {
        Ok(kernel) if !kernel.is_empty() => format!("{pretty} (Linux {kernel})"),
        _ => pretty,
    }
}

fn cpu_name() -> Result<String> {
    let text = std::fs::read_to_string("/proc/cpuinfo")?;
    text.lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "model name").then(|| value.trim().to_owned())
        })
        .context("cpuinfo 无 model name")
}

fn memory_mib() -> Result<i64> {
    let text = std::fs::read_to_string("/proc/meminfo")?;
    let kib: i64 = text
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|v| v.split_whitespace().next())
        .context("meminfo 无 MemTotal")?
        .parse()?;
    Ok(kib / 1024)
}

fn primary_mac() -> Result<String> {
    let mut names: Vec<_> = std::fs::read_dir("/sys/class/net")?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "lo")
        .collect();
    names.sort();
    // Prefer physical devices (they have a `device` link) over virtual bridges.
    names.sort_by_key(|name| !std::path::Path::new(&format!("/sys/class/net/{name}/device")).exists());
    for name in names {
        if let Ok(mac) = read_trimmed(&format!("/sys/class/net/{name}/address"))
            && !mac.is_empty()
            && mac != "00:00:00:00:00:00"
        {
            return Ok(mac.to_uppercase().replace(':', "-"));
        }
    }
    anyhow::bail!("未找到网卡地址")
}

fn video_names() -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir("/sys/class/drm")?.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        let vendor = std::fs::read_to_string(device.join("vendor")).unwrap_or_default();
        let id = std::fs::read_to_string(device.join("device")).unwrap_or_default();
        let driver = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        names.push(format!("{} {} ({driver})", vendor.trim(), id.trim()).trim().to_owned());
    }
    Ok(names)
}

impl Hardware {
    pub fn read() -> Result<Self> {
        let mut value = Self::default();
        let mut field = |label: &str, result: Result<String>, errors: &mut Vec<String>| match result {
            Ok(v) if !v.is_empty() => v,
            Ok(_) => {
                errors.push(format!("{label}：系统未提供"));
                String::new()
            }
            Err(e) => {
                errors.push(format!("{label}：{e:#}"));
                String::new()
            }
        };
        let mut errors = Vec::new();
        value.name = field("计算机名", computer_name(), &mut errors);
        // machine-id is world-readable and stable per installation.
        value.machine_guid = field(
            "机器标识",
            read_trimmed("/etc/machine-id").or_else(|_| read_trimmed("/var/lib/dbus/machine-id")),
            &mut errors,
        );
        // product_uuid is root-only on most distros; fall back to machine-id.
        value.system_uuid = read_trimmed("/sys/class/dmi/id/product_uuid")
            .unwrap_or_else(|_| value.machine_guid.clone());
        value.cpu = field("处理器", cpu_name(), &mut errors);
        value.base_board = field(
            "主板",
            read_trimmed("/sys/class/dmi/id/board_name").map(|board| {
                let vendor = read_trimmed("/sys/class/dmi/id/board_vendor").unwrap_or_default();
                format!("{vendor} {board}").trim().to_owned()
            }),
            &mut errors,
        );
        value.os = os_name();
        value.mac = field("网卡地址", primary_mac(), &mut errors);
        match video_names() {
            Ok(names) => value.video = names,
            Err(e) => errors.push(format!("显卡：{e:#}")),
        }
        match memory_mib() {
            Ok(memory) => value.memory = memory,
            Err(e) => errors.push(format!("内存：{e:#}")),
        }
        value.screen = crate::platform::display::local_display_dimensions()
            .first()
            .map(|(w, h)| format!("{w}x{h}"))
            .unwrap_or_default();
        value.errors = errors;
        ensure!(
            !value.name.is_empty() && !value.machine_guid.is_empty(),
            "无法读取 Linux 设备标识"
        );
        Ok(value)
    }
}
