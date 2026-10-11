//! A virtual mouse+keyboard through the Linux `uinput` subsystem. Absolute
//! pointer coordinates target the full compositor layout bounding box.
use anyhow::{Context, Result, bail, ensure};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::time::{SystemTime, UNIX_EPOCH};

// linux/uinput.h and linux/input-event-codes.h constants.
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;
const REL_WHEEL: u16 = 0x08;
const REL_HWHEEL: u16 = 0x06;
const REL_WHEEL_HI_RES: u16 = 0x0b;
const REL_HWHEEL_HI_RES: u16 = 0x0c;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const UI_SET_EVBIT: u64 = 0x4004_5564;
const UI_SET_KEYBIT: u64 = 0x4004_5565;
const UI_SET_RELBIT: u64 = 0x4004_5566;
const UI_SET_ABSBIT: u64 = 0x4004_5567;
const UI_DEV_CREATE: u64 = 0x5501;
const UI_DEV_DESTROY: u64 = 0x5502;
const ABS_MAX: i32 = 1 << 20;

#[repr(C)]
struct AbsSetup {
    code: u16,
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}
#[repr(C)]
struct UinputUserDev {
    name: [u8; 80],
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    ff_effects_max: u32,
    absmax: [i32; 64],
    absmin: [i32; 64],
    absfuzz: [i32; 64],
    absflat: [i32; 64],
}

pub(crate) struct Uinput {
    file: File,
    abs: bool,
}

fn ioctl(fd: i32, request: u64, arg: u64) -> Result<()> {
    let r = unsafe { libc::ioctl(fd, request as libc::c_ulong, arg) };
    ensure!(r >= 0, "uinput ioctl 失败: {}", std::io::Error::last_os_error());
    Ok(())
}

impl Uinput {
    pub(crate) fn open() -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .context(
                "无法打开 /dev/uinput（需要将当前用户加入 input 组或配置 udev 规则，详见下文）",
            )?;
        let fd = file.as_raw_fd();
        ioctl(fd, UI_SET_EVBIT, u64::from(EV_KEY))?;
        ioctl(fd, UI_SET_EVBIT, u64::from(EV_REL))?;
        ioctl(fd, UI_SET_EVBIT, u64::from(EV_ABS))?;
        ioctl(fd, UI_SET_EVBIT, u64::from(EV_SYN))?;
        // All keyboard keys (1..=248) and the mouse button range.
        for key in 1u64..=248 {
            ioctl(fd, UI_SET_KEYBIT, key)?;
        }
        for button in 0x110u64..=0x117 {
            ioctl(fd, UI_SET_KEYBIT, button)?;
        }
        for rel in [REL_WHEEL, REL_HWHEEL, REL_WHEEL_HI_RES, REL_HWHEEL_HI_RES, 0x00, 0x01] {
            ioctl(fd, UI_SET_RELBIT, u64::from(rel))?;
        }
        ioctl(fd, UI_SET_ABSBIT, u64::from(ABS_X))?;
        ioctl(fd, UI_SET_ABSBIT, u64::from(ABS_Y))?;

        let mut dev: UinputUserDev = unsafe { std::mem::zeroed() };
        let name = b"OpenUUYC Virtual Input";
        dev.name[..name.len()].copy_from_slice(name);
        dev.bustype = 0x03; // BUS_USB
        dev.vendor = 0x1d6b;
        dev.product = 0x0104;
        dev.version = 1;
        dev.absmax[ABS_X as usize] = ABS_MAX;
        dev.absmax[ABS_Y as usize] = ABS_MAX;
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&dev as *const UinputUserDev).cast::<u8>(),
                std::mem::size_of::<UinputUserDev>(),
            )
        };
        (&file).write_all(bytes).context("写入 uinput 设备描述失败")?;
        ioctl(fd, UI_DEV_CREATE, 0).context("创建 uinput 设备失败")?;
        // Give udev a moment to apply permissions to the new event node.
        std::thread::sleep(std::time::Duration::from_millis(40));
        let _ = AbsSetup {
            code: 0,
            value: 0,
            minimum: 0,
            maximum: 0,
            fuzz: 0,
            flat: 0,
            resolution: 0,
        };
        Ok(Self { file, abs: true })
    }

    fn emit(&mut self, kind: u16, code: u16, value: i32) -> Result<()> {
        // struct input_event { timeval time; u16 type; u16 code; s32 value; }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let mut buf = [0u8; 24];
        buf[0..8].copy_from_slice(&(now.as_secs() as i64).to_ne_bytes());
        buf[8..16].copy_from_slice(&(i64::from(now.subsec_micros())).to_ne_bytes());
        buf[16..18].copy_from_slice(&kind.to_ne_bytes());
        buf[18..20].copy_from_slice(&code.to_ne_bytes());
        buf[20..24].copy_from_slice(&value.to_ne_bytes());
        (&self.file).write_all(&buf).context("写入输入事件失败")
    }
    fn sync(&mut self) -> Result<()> {
        self.emit(EV_SYN, SYN_REPORT, 0)
    }

    pub(crate) fn key(&mut self, code: u16, down: bool) -> Result<()> {
        self.emit(EV_KEY, code, i32::from(down))?;
        self.sync()
    }
    pub(crate) fn button(&mut self, code: u16, down: bool) -> Result<()> {
        self.key(code, down)
    }
    /// Absolute position as a fraction (0.0..=1.0) of the whole layout.
    pub(crate) fn move_abs(&mut self, fx: f64, fy: f64) -> Result<()> {
        let clamp = |v: f64| (v.clamp(0.0, 1.0) * f64::from(ABS_MAX)).round() as i32;
        self.emit(EV_ABS, ABS_X, clamp(fx))?;
        self.emit(EV_ABS, ABS_Y, clamp(fy))?;
        self.sync()
    }
    pub(crate) fn move_rel(&mut self, dx: i32, dy: i32) -> Result<()> {
        if dx != 0 {
            self.emit(EV_REL, 0x00, dx)?;
        }
        if dy != 0 {
            self.emit(EV_REL, 0x01, dy)?;
        }
        self.sync()
    }
    pub(crate) fn wheel(&mut self, horizontal: bool, detents: i32, hi_res: i32) -> Result<()> {
        let (lo, hi) = if horizontal {
            (REL_HWHEEL, REL_HWHEEL_HI_RES)
        } else {
            (REL_WHEEL, REL_WHEEL_HI_RES)
        };
        if hi_res != 0 {
            self.emit(EV_REL, hi, hi_res)?;
        }
        if detents != 0 {
            self.emit(EV_REL, lo, detents)?;
        }
        self.sync()
    }
}

impl Drop for Uinput {
    fn drop(&mut self) {
        let _ = ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY, 0);
    }
}

/// Create and immediately destroy a device, to check permissions at startup.
pub(crate) fn self_test() -> Result<()> {
    let _dev = Uinput::open()?;
    Ok(())
}

/// True when /dev/uinput is at least present.
pub(crate) fn present() -> bool {
    std::path::Path::new("/dev/uinput").exists()
}
