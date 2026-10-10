//! Physical IPv4 inventory, change polling, and interface-bound WoL UDP on Linux.
//!
//! The controller uses this to send magic packets from a LAN interface; host-side
//! NIC configuration lives in `setup` and is Windows-only.
pub(crate) mod setup;
use crate::features::host::wol::packet::{LanInfo, mask, valid_ip};
use anyhow::{Context, Result, ensure};
use std::{
    ffi::CStr,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Interface {
    pub index: u32,
    pub luid: u64,
    pub name: String,
    pub mac: [u8; 6],
    pub ip: Ipv4Addr,
    pub prefix: u8,
    pub default_metric: Option<u64>,
}

impl Interface {
    pub fn registration(&self) -> LanInfo {
        LanInfo {
            mac: self
                .mac
                .iter()
                .map(|v| format!("{v:02X}"))
                .collect::<Vec<_>>()
                .join(":"),
            inner_ip: self.ip.to_string(),
            subnet_mask: mask(self.prefix).to_string(),
        }
    }
}

fn sys(name: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/net/{name}/{file}"))
        .ok()
        .map(|v| v.trim().to_owned())
}

fn mac(name: &str) -> Option<[u8; 6]> {
    let text = sys(name, "address")?;
    let bytes = text
        .split(':')
        .map(|v| u8::from_str_radix(v, 16).ok())
        .collect::<Option<Vec<_>>>()?;
    bytes.try_into().ok()
}

/// Physical (non-virtual) Ethernet/Wi-Fi devices only; bridges, tunnels, veths,
/// docker and loopback interfaces have no `device` link in sysfs.
fn physical(name: &str) -> bool {
    std::path::Path::new(&format!("/sys/class/net/{name}/device")).exists()
        && sys(name, "type").as_deref() == Some("1")
        && sys(name, "operstate").as_deref() == Some("up")
}

/// Default-route metric per interface from `/proc/net/route`.
fn default_metrics() -> std::collections::HashMap<String, u64> {
    let mut result = std::collections::HashMap::new();
    let Ok(text) = std::fs::read_to_string("/proc/net/route") else {
        return result;
    };
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 || fields[1] != "00000000" || fields[7] != "00000000" {
            continue;
        }
        let Ok(metric) = fields[6].parse::<u64>() else {
            continue;
        };
        result
            .entry(fields[0].to_owned())
            .and_modify(|v: &mut u64| *v = (*v).min(metric))
            .or_insert(metric);
    }
    result
}

pub(crate) fn inventory() -> Result<Vec<Interface>> {
    let metrics = default_metrics();
    let mut result = Vec::new();
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        ensure!(
            libc::getifaddrs(&mut list) == 0,
            "无法读取网卡地址: {}",
            std::io::Error::last_os_error()
        );
        struct List(*mut libc::ifaddrs);
        impl Drop for List {
            fn drop(&mut self) {
                unsafe { libc::freeifaddrs(self.0) }
            }
        }
        let _list = List(list);
        let mut p = list;
        while !p.is_null() {
            let a = &*p;
            p = a.ifa_next;
            if a.ifa_addr.is_null() || a.ifa_netmask.is_null() {
                continue;
            }
            if i32::from((*a.ifa_addr).sa_family) != libc::AF_INET {
                continue;
            }
            let name = CStr::from_ptr(a.ifa_name).to_string_lossy().into_owned();
            if !physical(&name) {
                continue;
            }
            let Some(mac) = mac(&name) else { continue };
            if mac == [0; 6] || mac[0] & 1 != 0 {
                continue;
            }
            let addr = &*a.ifa_addr.cast::<libc::sockaddr_in>();
            let netmask = &*a.ifa_netmask.cast::<libc::sockaddr_in>();
            let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
            let prefix = u32::from_be(netmask.sin_addr.s_addr).count_ones() as u8;
            if !valid_ip(ip) || !(1..=30).contains(&prefix) {
                continue;
            }
            let index = libc::if_nametoindex(a.ifa_name);
            result.push(Interface {
                index,
                luid: u64::from(index),
                default_metric: metrics.get(&name).copied(),
                name,
                mac,
                ip,
                prefix,
            });
        }
    }
    result.sort_by_key(|v| (v.default_metric.unwrap_or(u64::MAX), v.index, v.ip));
    Ok(result)
}

#[derive(Default)]
pub(crate) struct Changes {
    pub revision: AtomicU64,
    pub notify: tokio::sync::Notify,
}

impl Changes {
    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        self.notify.notify_one();
    }
}

/// Polls the inventory (Linux has no portable callback without netlink plumbing).
pub(crate) struct Watcher {
    stop: Arc<AtomicBool>,
}

impl Watcher {
    pub fn new(changes: Arc<Changes>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("wol-network-watch".into())
            .spawn(move || {
                let mut last = inventory().ok();
                while !flag.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_secs(3));
                    let now = inventory().ok();
                    if now != last {
                        last = now;
                        changes.changed();
                    }
                }
            })
            .context("启动网络变化监视")?;
        Ok(Self { stop })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[derive(Default)]
pub(crate) struct Sent {
    pub attempted: u32,
    pub sent: u32,
}

pub(crate) fn send(
    interface: &Interface,
    packet: &[u8; 102],
    destinations: &[SocketAddrV4],
    valid: impl Fn() -> bool,
) -> Result<Sent> {
    ensure!(valid(), "唤醒请求已失效");
    let socket = UdpSocket::bind(SocketAddrV4::new(interface.ip, 0)).context("绑定唤醒源网卡")?;
    socket.set_broadcast(true)?;
    socket.set_write_timeout(Some(Duration::from_millis(500)))?;
    let mut result = Sent::default();
    for destination in destinations {
        ensure!(valid(), "唤醒请求已取消或网络已改变");
        result.attempted += 1;
        match socket.send_to(packet, *destination) {
            Ok(n) if n == packet.len() => result.sent += 1,
            Ok(_) => tracing::warn!(port = destination.port(), "WoL UDP write was incomplete"),
            Err(error) => tracing::debug!(
                port = destination.port(),
                code = error.raw_os_error(),
                "WoL UDP send failed"
            ),
        }
    }
    ensure!(result.sent > 0, "所有唤醒端口均发送失败");
    Ok(result)
}
