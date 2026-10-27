//! The kernel side of the stylus daemon: reading and grabbing the
//! touchscreen (evdev) and the virtual pointer it drives (uinput). Plain
//! ioctls on Linux; the decisions are in [`super::stylus`].

use super::stylus::{AbsRange, Button, DeviceInfo, Output, RawEvent, VIRTUAL_NAME};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;
const SYN_DROPPED: u16 = 3;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_TOUCH: u16 = 0x14a;
const INPUT_PROP_DIRECT: u16 = 0x01;
const BUS_VIRTUAL: u16 = 0x06;
const KEY_MAX: usize = 0x2ff;
const ABS_MAX: usize = 0x3f;
const REL_MAX: usize = 0x0f;
const INPUT_PROP_MAX: usize = 0x1f;

/// Range of the virtual pointer's axes; X maps it onto the whole screen.
pub const VIRTUAL_RANGE: i32 = 32767;

// _IOC from <asm-generic/ioctl.h> (arm64 and x86 use the generic layout).
const fn ioc(dir: u64, ty: u8, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((ty as u64) << 8) | nr
}
const READ: u64 = 2;
const WRITE: u64 = 1;
const fn eviocgbit(ev: u16, len: usize) -> u64 {
    ioc(READ, b'E', 0x20 + ev as u64, len)
}
const fn eviocgabs(axis: u16) -> u64 {
    ioc(READ, b'E', 0x40 + axis as u64, std::mem::size_of::<AbsInfo>())
}
const EVIOCGID: u64 = ioc(READ, b'E', 0x02, 8);
const EVIOCGRAB: u64 = ioc(WRITE, b'E', 0x90, std::mem::size_of::<libc::c_int>());
const fn eviocgname(len: usize) -> u64 {
    ioc(READ, b'E', 0x06, len)
}
const fn eviocgprop(len: usize) -> u64 {
    ioc(READ, b'E', 0x09, len)
}
const UI_SET_EVBIT: u64 = ioc(WRITE, b'U', 100, std::mem::size_of::<libc::c_int>());
const UI_SET_KEYBIT: u64 = ioc(WRITE, b'U', 101, std::mem::size_of::<libc::c_int>());
const UI_SET_ABSBIT: u64 = ioc(WRITE, b'U', 103, std::mem::size_of::<libc::c_int>());
const UI_DEV_SETUP: u64 = ioc(WRITE, b'U', 3, std::mem::size_of::<UinputSetup>());
const UI_ABS_SETUP: u64 = ioc(WRITE, b'U', 4, std::mem::size_of::<UinputAbsSetup>());
const UI_DEV_CREATE: u64 = ioc(0, b'U', 1, 0);
const UI_DEV_DESTROY: u64 = ioc(0, b'U', 2, 0);

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    absinfo: AbsInfo,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

fn ioctl_ptr<T>(file: &File, request: u64, arg: *mut T) -> io::Result<i32> {
    // SAFETY: `arg` points to a live value of the size encoded in `request`.
    let r = unsafe { libc::ioctl(file.as_raw_fd(), request as _, arg) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

fn ioctl_int(file: &File, request: u64, arg: libc::c_int) -> io::Result<()> {
    // SAFETY: integer-argument ioctls (EVIOCGRAB, UI_SET_*BIT, UI_DEV_*).
    let r = unsafe { libc::ioctl(file.as_raw_fd(), request as _, arg) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn bits(file: &File, ev: u16, max: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; max / 8 + 1];
    ioctl_ptr(file, eviocgbit(ev, buf.len()), buf.as_mut_ptr())?;
    Ok(buf)
}

fn has(bits: &[u8], code: u16) -> bool {
    bits.get(code as usize / 8).is_some_and(|b| b & (1 << (code % 8)) != 0)
}

fn absinfo(file: &File, axis: u16) -> io::Result<AbsInfo> {
    let mut info = AbsInfo::default();
    ioctl_ptr(file, eviocgabs(axis), &mut info)?;
    Ok(info)
}

/// An opened input device with what the daemon needs to know about it.
pub struct EvdevDevice {
    file: File,
    pub info: DeviceInfo,
    pub x: AbsRange,
    pub y: AbsRange,
    touch_key: u16,
    grabbed: bool,
}

fn probe_file(path: &Path, file: File) -> io::Result<EvdevDevice> {
    let mut name = [0u8; 256];
    let name = match ioctl_ptr(&file, eviocgname(name.len()), name.as_mut_ptr()) {
        Ok(_) => String::from_utf8_lossy(name.split(|b| *b == 0).next().unwrap_or_default()).into_owned(),
        Err(_) => String::new(),
    };
    let mut id = InputId { bustype: 0, vendor: 0, product: 0, version: 0 };
    let _ = ioctl_ptr(&file, EVIOCGID, &mut id);
    let types = bits(&file, 0, 0x1f)?;
    let keys = if has(&types, EV_KEY) { bits(&file, EV_KEY, KEY_MAX)? } else { Vec::new() };
    let abs = if has(&types, EV_ABS) { bits(&file, EV_ABS, ABS_MAX)? } else { Vec::new() };
    let rel = if has(&types, EV_REL) { bits(&file, EV_REL, REL_MAX)? } else { Vec::new() };
    let mut props = vec![0u8; INPUT_PROP_MAX / 8 + 1];
    let _ = ioctl_ptr(&file, eviocgprop(props.len()), props.as_mut_ptr());
    let (abs_x, abs_y) = if has(&abs, ABS_X) && has(&abs, ABS_Y) {
        (Some(ABS_X), Some(ABS_Y))
    } else if has(&abs, ABS_MT_POSITION_X) && has(&abs, ABS_MT_POSITION_Y) {
        (Some(ABS_MT_POSITION_X), Some(ABS_MT_POSITION_Y))
    } else {
        (None, None)
    };
    let range = |axis: Option<u16>| -> io::Result<AbsRange> {
        match axis {
            Some(a) => absinfo(&file, a).map(|i| AbsRange { min: i.minimum, max: i.maximum }),
            None => Ok(AbsRange { min: 0, max: 1 }),
        }
    };
    let (x, y) = (range(abs_x)?, range(abs_y)?);
    let info = DeviceInfo {
        path: path.to_string_lossy().into_owned(),
        name,
        vendor: id.vendor,
        product: id.product,
        abs_x: abs_x.is_some(),
        abs_y: abs_y.is_some(),
        btn_touch: has(&keys, BTN_TOUCH),
        btn_left: has(&keys, BTN_LEFT),
        rel: rel.iter().any(|b| *b != 0),
        direct: has(&props, INPUT_PROP_DIRECT),
    };
    let touch_key = if info.btn_touch { BTN_TOUCH } else { BTN_LEFT };
    Ok(EvdevDevice { file, info, x, y, touch_key, grabbed: false })
}

impl EvdevDevice {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)?;
        probe_file(path, file)
    }

    pub fn fd(&self) -> i32 {
        self.file.as_raw_fd()
    }

    /// Take the device away from everyone else (the X server), so its
    /// events arrive only through the virtual pointer. Released on drop and
    /// by the kernel when the process exits.
    pub fn grab(&mut self) -> io::Result<()> {
        ioctl_int(&self.file, EVIOCGRAB, 1)?;
        self.grabbed = true;
        Ok(())
    }

    /// Everything readable now, decoded. `Ok(empty)` when nothing is
    /// pending; ENODEV when the device was unplugged.
    pub fn read(&mut self) -> io::Result<Vec<RawEvent>> {
        const SIZE: usize = std::mem::size_of::<InputEvent>();
        let mut buf = [0u8; SIZE * 64];
        let n = match self.file.read(&mut buf) {
            Ok(0) => return Err(io::Error::from_raw_os_error(libc::ENODEV)),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for chunk in buf[..n - n % SIZE].chunks_exact(SIZE) {
            // SAFETY: the kernel writes whole `struct input_event`s.
            let ev: InputEvent = unsafe { std::ptr::read_unaligned(chunk.as_ptr() as *const InputEvent) };
            let decoded = match (ev.kind, ev.code) {
                (EV_ABS, ABS_X | ABS_MT_POSITION_X) => Some(RawEvent::X(ev.value)),
                (EV_ABS, ABS_Y | ABS_MT_POSITION_Y) => Some(RawEvent::Y(ev.value)),
                (EV_KEY, code) if code == self.touch_key => Some(RawEvent::Touch(ev.value != 0)),
                (EV_SYN, SYN_REPORT) => Some(RawEvent::Sync),
                (EV_SYN, SYN_DROPPED) => Some(RawEvent::Dropped),
                _ => None,
            };
            out.extend(decoded);
        }
        Ok(out)
    }
}

impl Drop for EvdevDevice {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = ioctl_int(&self.file, EVIOCGRAB, 0);
        }
    }
}

/// Every /dev/input/event* this process can open, described.
pub fn list_devices() -> Vec<DeviceInfo> {
    let mut paths: Vec<_> = std::fs::read_dir("/dev/input")
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("event"))).collect())
        .unwrap_or_default();
    paths.sort_by_key(|p| p.file_name().and_then(|n| n.to_str()?.trim_start_matches("event").parse::<u32>().ok()));
    paths.iter().filter_map(|p| EvdevDevice::open(p).ok().map(|d| d.info.clone())).collect()
}

/// The absolute pointer the daemon drives: left, middle and right buttons
/// and X/Y over [`VIRTUAL_RANGE`], like a virtual machine's tablet. Not a
/// touchscreen (no BTN_TOUCH, no direct property), so the X server treats
/// its buttons as mouse buttons, right click included.
pub struct VirtualPointer {
    file: File,
    screen: (u32, u32),
}

impl VirtualPointer {
    pub fn create(screen: (u32, u32)) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open("/dev/uinput")?;
        for ev in [EV_SYN, EV_KEY, EV_ABS] {
            ioctl_int(&file, UI_SET_EVBIT, ev as libc::c_int)?;
        }
        for key in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE] {
            ioctl_int(&file, UI_SET_KEYBIT, key as libc::c_int)?;
        }
        for axis in [ABS_X, ABS_Y] {
            ioctl_int(&file, UI_SET_ABSBIT, axis as libc::c_int)?;
            let mut setup =
                UinputAbsSetup { code: axis, absinfo: AbsInfo { maximum: VIRTUAL_RANGE, ..Default::default() } };
            ioctl_ptr(&file, UI_ABS_SETUP, &mut setup)?;
        }
        let mut setup = UinputSetup {
            id: InputId { bustype: BUS_VIRTUAL, vendor: 0, product: 0, version: 1 },
            name: [0; 80],
            ff_effects_max: 0,
        };
        setup.name[..VIRTUAL_NAME.len()].copy_from_slice(VIRTUAL_NAME.as_bytes());
        ioctl_ptr(&file, UI_DEV_SETUP, &mut setup)?;
        ioctl_int(&file, UI_DEV_CREATE, 0)?;
        Ok(Self { file, screen })
    }

    pub fn set_screen(&mut self, screen: (u32, u32)) {
        self.screen = screen;
    }

    pub fn emit(&mut self, out: &[Output]) -> io::Result<()> {
        let scale = |v: f64, size: u32| ((v / (size.max(2) - 1) as f64) * VIRTUAL_RANGE as f64).round() as i32;
        let mut events = Vec::with_capacity(out.len() * 2);
        for o in out {
            match *o {
                Output::Move { x, y } => {
                    events.push((EV_ABS, ABS_X, scale(x, self.screen.0)));
                    events.push((EV_ABS, ABS_Y, scale(y, self.screen.1)));
                }
                Output::Button { button, pressed } => {
                    let code = match button {
                        Button::Left => BTN_LEFT,
                        Button::Right => BTN_RIGHT,
                    };
                    events.push((EV_KEY, code, pressed as i32));
                }
                Output::Sync => events.push((EV_SYN, SYN_REPORT, 0)),
            }
        }
        let mut bytes = Vec::with_capacity(events.len() * std::mem::size_of::<InputEvent>());
        for (kind, code, value) in events {
            let ev = InputEvent { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, kind, code, value };
            // SAFETY: plain-old-data struct written as the kernel reads it.
            let raw = unsafe {
                std::slice::from_raw_parts(&ev as *const InputEvent as *const u8, std::mem::size_of::<InputEvent>())
            };
            bytes.extend_from_slice(raw);
        }
        self.file.write_all(&bytes)
    }
}

impl Drop for VirtualPointer {
    fn drop(&mut self) {
        let _ = ioctl_int(&self.file, UI_DEV_DESTROY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_the_kernel_headers() {
        // Values from <linux/input.h> / <linux/uinput.h> on 64-bit Linux.
        assert_eq!(EVIOCGRAB, 0x4004_4590);
        assert_eq!(EVIOCGID, 0x8008_4502);
        assert_eq!(eviocgname(256), 0x8100_4506);
        assert_eq!(eviocgabs(ABS_X), 0x8018_4540);
        assert_eq!(eviocgbit(EV_KEY, 96), 0x8060_4521);
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_DESTROY, 0x5502);
        assert_eq!(UI_DEV_SETUP, 0x405c_5503);
        assert_eq!(UI_ABS_SETUP, 0x401c_5504);
        assert_eq!(std::mem::size_of::<InputEvent>(), 24);
    }

    #[test]
    fn bit_lookup() {
        let b = [0b0000_0010u8, 0, 0b1000_0000];
        assert!(has(&b, 1));
        assert!(!has(&b, 0));
        assert!(has(&b, 23));
        assert!(!has(&b, 200));
    }
}
