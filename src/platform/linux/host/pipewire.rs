//! PipeWire video consumer for the ScreenCast portal, loaded at run time.
//!
//! `libpipewire-0.3.so.0` is opened with dlopen rather than linked: desktops
//! that offer the portal ship the library, but its headers track newer
//! releases than this build wants to require. The entry points used here have
//! been stable since PipeWire 0.3.
//!
//! Frames are requested as shared memory (MemPtr or MemFd) and copied to
//! tightly packed BGRA on PipeWire's loop thread. KWin does not mark its
//! shared-memory buffers mappable, so a MemFd with no data pointer is mapped
//! here for the duration of one dequeued buffer. DMA-BUF is intentionally not
//! imported: negotiation never asks for it, and a buffer that arrives as
//! DMA-BUF anyway is reported instead of being misread.

use anyhow::{Context, Result, anyhow, bail, ensure};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[repr(C)]
struct SpaList {
    next: *mut c_void,
    prev: *mut c_void,
}
#[repr(C)]
struct SpaCallbacks {
    funcs: *const c_void,
    data: *mut c_void,
}
#[repr(C)]
struct SpaHook {
    link: SpaList,
    cb: SpaCallbacks,
    removed: Option<unsafe extern "C" fn(*mut SpaHook)>,
    private: *mut c_void,
}
type Callback = Option<unsafe extern "C" fn(*mut c_void)>;
#[repr(C)]
struct StreamEvents {
    version: u32,
    destroy: Callback,
    state_changed: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char)>,
    control_info: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
    io_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32)>,
    param_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
    add_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    remove_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    process: Callback,
    drained: Callback,
    command: Option<unsafe extern "C" fn(*mut c_void, *const c_void)>,
    trigger_done: Callback,
}
#[repr(C)]
struct PwBuffer {
    buffer: *mut SpaBuffer,
    user_data: *mut c_void,
    size: u64,
}
#[repr(C)]
struct SpaBuffer {
    n_metas: u32,
    n_datas: u32,
    metas: *mut SpaMeta,
    datas: *mut SpaData,
}
#[repr(C)]
struct SpaMeta {
    kind: u32,
    size: u32,
    data: *mut c_void,
}
#[repr(C)]
struct SpaData {
    kind: u32,
    flags: u32,
    fd: i64,
    mapoffset: u32,
    maxsize: u32,
    data: *mut c_void,
    chunk: *mut SpaChunk,
}
#[repr(C)]
struct SpaChunk {
    offset: u32,
    size: u32,
    stride: i32,
    flags: i32,
}
#[repr(C)]
struct SpaMetaRegion {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

const STREAM_EVENTS_VERSION: u32 = 2;
const DIRECTION_INPUT: u32 = 0;
const FLAG_AUTOCONNECT: u32 = 1 << 0;
/// `PW_STREAM_FLAG_MAP_BUFFERS`: map buffers the producer marked mappable.
const FLAG_MAP_BUFFERS: u32 = 1 << 2;
const STATE_ERROR: c_int = -1;
const STATE_UNCONNECTED: c_int = 0;
const STATE_STREAMING: c_int = 3;

const TYPE_ID: u32 = 3;
const TYPE_INT: u32 = 4;
const TYPE_LONG: u32 = 5;
const TYPE_RECTANGLE: u32 = 10;
const TYPE_FRACTION: u32 = 11;
const TYPE_OBJECT: u32 = 15;
const TYPE_CHOICE: u32 = 19;
const OBJECT_FORMAT: u32 = 0x40003;
const OBJECT_PARAM_BUFFERS: u32 = 0x40004;
const OBJECT_PARAM_META: u32 = 0x40005;
const PARAM_ENUM_FORMAT: u32 = 3;
const PARAM_FORMAT: u32 = 4;
const PARAM_BUFFERS: u32 = 5;
const PARAM_META: u32 = 6;
const FORMAT_MEDIA_TYPE: u32 = 1;
const FORMAT_MEDIA_SUBTYPE: u32 = 2;
const FORMAT_VIDEO_FORMAT: u32 = 0x20001;
const FORMAT_VIDEO_MODIFIER: u32 = 0x20002;
const FORMAT_VIDEO_SIZE: u32 = 0x20003;
const FORMAT_VIDEO_FRAMERATE: u32 = 0x20004;
const MEDIA_TYPE_VIDEO: u32 = 2;
const MEDIA_SUBTYPE_RAW: u32 = 1;
const VIDEO_RGBX: u32 = 7;
const VIDEO_BGRX: u32 = 8;
const VIDEO_RGBA: u32 = 11;
const VIDEO_BGRA: u32 = 12;
const BUFFERS_DATA_TYPE: u32 = 6;
const META_TYPE: u32 = 1;
const META_SIZE: u32 = 2;
const META_VIDEO_CROP: u32 = 2;
const DATA_MEM_PTR: u32 = 1;
const DATA_MEM_FD: u32 = 2;
const DATA_MEM_DMABUF: u32 = 3;
const CHOICE_RANGE: u32 = 1;
const CHOICE_ENUM: u32 = 3;
const CHOICE_FLAGS: u32 = 4;

/// Shared-memory buffer types this fallback can read.
const SHM_BUFFER_MASK: u32 = (1 << DATA_MEM_PTR) | (1 << DATA_MEM_FD);

struct Api {
    _library: libloading::Library,
    thread_loop_new: unsafe extern "C" fn(*const c_char, *const c_void) -> *mut c_void,
    thread_loop_get_loop: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    thread_loop_start: unsafe extern "C" fn(*mut c_void) -> c_int,
    thread_loop_stop: unsafe extern "C" fn(*mut c_void),
    thread_loop_destroy: unsafe extern "C" fn(*mut c_void),
    thread_loop_lock: unsafe extern "C" fn(*mut c_void),
    thread_loop_unlock: unsafe extern "C" fn(*mut c_void),
    context_new: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void,
    context_destroy: unsafe extern "C" fn(*mut c_void),
    context_connect_fd: unsafe extern "C" fn(*mut c_void, c_int, *mut c_void, usize) -> *mut c_void,
    core_disconnect: unsafe extern "C" fn(*mut c_void) -> c_int,
    properties_new: unsafe extern "C" fn(*const c_char, ...) -> *mut c_void,
    stream_new: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void) -> *mut c_void,
    stream_add_listener:
        unsafe extern "C" fn(*mut c_void, *mut SpaHook, *const StreamEvents, *mut c_void),
    stream_connect:
        unsafe extern "C" fn(*mut c_void, u32, u32, u32, *mut *const c_void, u32) -> c_int,
    stream_update_params: unsafe extern "C" fn(*mut c_void, *mut *const c_void, u32) -> c_int,
    stream_dequeue_buffer: unsafe extern "C" fn(*mut c_void) -> *mut PwBuffer,
    stream_queue_buffer: unsafe extern "C" fn(*mut c_void, *mut PwBuffer) -> c_int,
    stream_disconnect: unsafe extern "C" fn(*mut c_void) -> c_int,
    stream_destroy: unsafe extern "C" fn(*mut c_void),
}

fn api() -> Result<&'static Api> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| load().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

fn load() -> Result<Api> {
    // SAFETY: the system PipeWire 0.3 client library; each symbol is copied
    // out with its C signature and the library is kept alive in `Api`.
    unsafe {
        let library = libloading::Library::new("libpipewire-0.3.so.0")
            .context("没有找到 PipeWire（libpipewire-0.3.so.0）")?;
        macro_rules! symbol {
            ($name:literal) => {
                *library
                    .get(concat!($name, "\0").as_bytes())
                    .with_context(|| format!("PipeWire 缺少 {}", $name))?
            };
        }
        let init: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char) = symbol!("pw_init");
        init(std::ptr::null_mut(), std::ptr::null_mut());
        Ok(Api {
            thread_loop_new: symbol!("pw_thread_loop_new"),
            thread_loop_get_loop: symbol!("pw_thread_loop_get_loop"),
            thread_loop_start: symbol!("pw_thread_loop_start"),
            thread_loop_stop: symbol!("pw_thread_loop_stop"),
            thread_loop_destroy: symbol!("pw_thread_loop_destroy"),
            thread_loop_lock: symbol!("pw_thread_loop_lock"),
            thread_loop_unlock: symbol!("pw_thread_loop_unlock"),
            context_new: symbol!("pw_context_new"),
            context_destroy: symbol!("pw_context_destroy"),
            context_connect_fd: symbol!("pw_context_connect_fd"),
            core_disconnect: symbol!("pw_core_disconnect"),
            properties_new: symbol!("pw_properties_new"),
            stream_new: symbol!("pw_stream_new"),
            stream_add_listener: symbol!("pw_stream_add_listener"),
            stream_connect: symbol!("pw_stream_connect"),
            stream_update_params: symbol!("pw_stream_update_params"),
            stream_dequeue_buffer: symbol!("pw_stream_dequeue_buffer"),
            stream_queue_buffer: symbol!("pw_stream_queue_buffer"),
            stream_disconnect: symbol!("pw_stream_disconnect"),
            stream_destroy: symbol!("pw_stream_destroy"),
            _library: library,
        })
    }
}

/// Builds one SPA pod. Every pod is `{u32 size, u32 type}` then its body,
/// padded to 8 bytes, which is the layout `spa_pod_builder` produces.
#[derive(Default)]
struct Pod(Vec<u8>);
impl Pod {
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_ne_bytes());
    }
    fn pad(&mut self) {
        while self.0.len() % 8 != 0 {
            self.0.push(0);
        }
    }
    fn value(&mut self, kind: u32, body: &[u32]) {
        self.u32((body.len() * 4) as u32);
        self.u32(kind);
        for word in body {
            self.u32(*word);
        }
        self.pad();
    }
    /// A choice of `values`, each `child_words` 32-bit words of `child_type`.
    fn choice(&mut self, choice: u32, child_type: u32, child_words: usize, values: &[u32]) {
        self.u32((16 + values.len() * 4) as u32);
        self.u32(TYPE_CHOICE);
        self.u32(choice);
        self.u32(0);
        self.u32((child_words * 4) as u32);
        self.u32(child_type);
        for word in values {
            self.u32(*word);
        }
        self.pad();
    }
    fn object(kind: u32, id: u32, props: impl FnOnce(&mut Self)) -> Self {
        let mut pod = Self::default();
        pod.u32(0);
        pod.u32(TYPE_OBJECT);
        pod.u32(kind);
        pod.u32(id);
        props(&mut pod);
        let size = (pod.0.len() - 8) as u32;
        pod.0[..4].copy_from_slice(&size.to_ne_bytes());
        pod
    }
    fn key(&mut self, key: u32) {
        self.u32(key);
        self.u32(0);
    }
    fn aligned(&self) -> Vec<u64> {
        let mut words = vec![0u64; self.0.len().div_ceil(8)];
        // SAFETY: `words` has at least as many bytes as the pod.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.0.as_ptr(),
                words.as_mut_ptr().cast::<u8>(),
                self.0.len(),
            );
        }
        words
    }
}

/// Shared-memory format offer. The DRM modifier key is omitted on purpose:
/// its presence is what asks the portal for DMA-BUF.
fn enum_format_shm() -> Pod {
    Pod::object(OBJECT_FORMAT, PARAM_ENUM_FORMAT, |pod| {
        pod.key(FORMAT_MEDIA_TYPE);
        pod.value(TYPE_ID, &[MEDIA_TYPE_VIDEO]);
        pod.key(FORMAT_MEDIA_SUBTYPE);
        pod.value(TYPE_ID, &[MEDIA_SUBTYPE_RAW]);
        pod.key(FORMAT_VIDEO_FORMAT);
        // The first value is the preferred one and is repeated in the list,
        // which is the SPA enum-choice shape.
        pod.choice(
            CHOICE_ENUM,
            TYPE_ID,
            1,
            &[VIDEO_BGRX, VIDEO_BGRX, VIDEO_BGRA, VIDEO_RGBX, VIDEO_RGBA],
        );
        pod.key(FORMAT_VIDEO_SIZE);
        pod.choice(
            CHOICE_RANGE,
            TYPE_RECTANGLE,
            2,
            &[1920, 1080, 1, 1, 16384, 16384],
        );
        pod.key(FORMAT_VIDEO_FRAMERATE);
        pod.choice(CHOICE_RANGE, TYPE_FRACTION, 2, &[60, 1, 0, 1, 1000, 1]);
    })
}

fn buffers() -> Pod {
    Pod::object(OBJECT_PARAM_BUFFERS, PARAM_BUFFERS, |pod| {
        pod.key(BUFFERS_DATA_TYPE);
        pod.choice(CHOICE_FLAGS, TYPE_INT, 1, &[SHM_BUFFER_MASK]);
    })
}

fn crop_meta() -> Pod {
    Pod::object(OBJECT_PARAM_META, PARAM_META, |pod| {
        pod.key(META_TYPE);
        pod.value(TYPE_ID, &[META_VIDEO_CROP]);
        pod.key(META_SIZE);
        pod.value(TYPE_INT, &[size_of::<SpaMetaRegion>() as u32]);
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NegotiatedFormat {
    format: u32,
    width: u32,
    height: u32,
    modifier: Option<u64>,
}

/// # Safety
/// `pod` must point at a valid SPA pod.
unsafe fn parse_format(pod: *const c_void) -> Option<NegotiatedFormat> {
    let read =
        |offset: usize| unsafe { pod.cast::<u8>().add(offset).cast::<u32>().read_unaligned() };
    let read64 =
        |offset: usize| unsafe { pod.cast::<u8>().add(offset).cast::<u64>().read_unaligned() };
    let size = read(0) as usize;
    if read(4) != TYPE_OBJECT || size < 8 {
        return None;
    }
    let end = 8 + size;
    let mut offset = 16;
    let (mut format, mut width, mut height) = (None, None, None);
    let mut modifier = None;
    while offset + 16 <= end {
        let key = read(offset);
        let value_size = read(offset + 8) as usize;
        let mut value_type = read(offset + 12);
        let mut value = offset + 16;
        if value_type == TYPE_CHOICE {
            value_type = read(value + 12);
            value += 16;
        }
        match (key, value_type) {
            (FORMAT_VIDEO_FORMAT, TYPE_ID) => format = Some(read(value)),
            (FORMAT_VIDEO_MODIFIER, TYPE_LONG) => modifier = Some(read64(value)),
            (FORMAT_VIDEO_SIZE, TYPE_RECTANGLE) => {
                width = Some(read(value));
                height = Some(read(value + 4));
            }
            _ => {}
        }
        offset += 16 + value_size.next_multiple_of(8);
    }
    Some(NegotiatedFormat {
        format: format?,
        width: width?,
        height: height?,
        modifier,
    })
}

/// Tightly packed BGRA (`width * height * 4`).
#[derive(Debug)]
pub(super) struct Picture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Default)]
struct State {
    picture: Option<Picture>,
    sequence: u64,
    format: Option<NegotiatedFormat>,
    streaming: bool,
    error: Option<String>,
    buffers_sent: bool,
    delivered: u64,
    unreadable: Option<String>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

struct Callbacks {
    api: &'static Api,
    stream: *mut c_void,
    shared: Arc<Shared>,
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|error| error.into_inner())
}

unsafe extern "C" fn on_state_changed(
    data: *mut c_void,
    _old: c_int,
    state: c_int,
    error: *const c_char,
) {
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    let mut guard = lock(&callbacks.shared.state);
    match state {
        STATE_STREAMING => {
            if !guard.streaming {
                tracing::info!("PipeWire screencast streaming");
            }
            guard.streaming = true;
        }
        STATE_ERROR => {
            let text = if error.is_null() {
                "未知错误".to_owned()
            } else {
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            };
            guard.error = Some(format!("PipeWire 画面流出错：{text}"));
        }
        STATE_UNCONNECTED if guard.streaming => {
            guard.error = Some("屏幕共享已结束".into());
        }
        _ => {}
    }
    drop(guard);
    callbacks.shared.changed.notify_all();
}

unsafe extern "C" fn on_param_changed(data: *mut c_void, id: u32, param: *const c_void) {
    if param.is_null() || id != PARAM_FORMAT {
        return;
    }
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    let format = unsafe { parse_format(param) };
    let mut state = lock(&callbacks.shared.state);
    state.format = format;
    if state.buffers_sent {
        return;
    }
    state.buffers_sent = true;
    drop(state);
    let buffers = buffers().aligned();
    let meta = crop_meta().aligned();
    let mut params = [
        buffers.as_ptr().cast::<c_void>(),
        meta.as_ptr().cast::<c_void>(),
    ];
    unsafe {
        (callbacks.api.stream_update_params)(
            callbacks.stream,
            params.as_mut_ptr(),
            params.len() as u32,
        )
    };
    tracing::info!(?format, "PipeWire screencast format negotiated");
}

unsafe extern "C" fn on_process(data: *mut c_void) {
    let callbacks = unsafe { &*data.cast::<Callbacks>() };
    let api = callbacks.api;
    let mut newest: *mut PwBuffer = std::ptr::null_mut();
    loop {
        let buffer = unsafe { (api.stream_dequeue_buffer)(callbacks.stream) };
        if buffer.is_null() {
            break;
        }
        if !newest.is_null() {
            unsafe { (api.stream_queue_buffer)(callbacks.stream, newest) };
        }
        newest = buffer;
    }
    if newest.is_null() {
        return;
    }
    let format = lock(&callbacks.shared.state).format;
    let picture = match format {
        Some(format) => unsafe { read_picture((*newest).buffer, format) },
        None => Err("画面格式还没协商".into()),
    };
    let kind = unsafe {
        let buffer = (*newest).buffer;
        (!buffer.is_null() && (*buffer).n_datas > 0 && !(*buffer).datas.is_null())
            .then(|| (*(*buffer).datas).kind)
    };
    unsafe { (api.stream_queue_buffer)(callbacks.stream, newest) };
    let mut state = lock(&callbacks.shared.state);
    state.delivered += 1;
    if state.delivered == 1 {
        tracing::info!(
            data_type = kind,
            readable = picture.is_ok(),
            "PipeWire first buffer delivered"
        );
    }
    match picture {
        Ok(picture) => {
            state.picture = Some(picture);
            state.sequence += 1;
            drop(state);
            callbacks.shared.changed.notify_all();
        }
        Err(reason) => {
            if state.unreadable.is_none() {
                tracing::warn!(data_type = kind, %reason, "PipeWire buffer held no readable picture");
            }
            state.unreadable = Some(match kind {
                Some(kind) => format!("数据类型 {kind}：{reason}"),
                None => reason,
            });
        }
    }
}

/// Copy one dequeued buffer to packed BGRA, or say why it held none.
///
/// # Safety
/// `buffer` must be a buffer the stream just dequeued, and it must stay
/// dequeued for the duration of this call (the MemFd map is dropped here).
unsafe fn read_picture(
    buffer: *mut SpaBuffer,
    negotiated: NegotiatedFormat,
) -> std::result::Result<Picture, String> {
    if buffer.is_null() {
        return Err("缓冲为空".into());
    }
    let buffer = unsafe { &*buffer };
    if buffer.n_datas == 0 || buffer.datas.is_null() {
        return Err("缓冲里没有数据块".into());
    }
    let data = unsafe { &*buffer.datas };
    if data.kind == DATA_MEM_DMABUF {
        return Err(
            "合成器提供了 DMA-BUF 缓冲。当前 ScreenCast 回退只读取共享内存（MemPtr/MemFd），不导入 DMA-BUF"
                .into(),
        );
    }
    if data.chunk.is_null() {
        return Err("缓冲没有 chunk".into());
    }
    // PipeWire maps a buffer only when its producer marks it mappable. KWin's
    // shared-memory buffers are not marked, so they arrive as a bare MemFd.
    let mapped = if !data.data.is_null() {
        None
    } else if data.kind == DATA_MEM_FD && data.fd >= 0 {
        Some(MemFdMap::new(data.fd as i32, data.mapoffset, data.maxsize)?)
    } else {
        return Err("缓冲没有映射到内存，也不是能自己映射的 MemFd".into());
    };
    let source: &[u8] = if let Some(mapped) = &mapped {
        mapped.bytes()
    } else {
        unsafe { std::slice::from_raw_parts(data.data.cast::<u8>(), data.maxsize as usize) }
    };
    let chunk = unsafe { &*data.chunk };
    let stride = if chunk.stride > 0 {
        chunk.stride as usize
    } else {
        negotiated.width as usize * 4
    };
    let base = (chunk.offset % data.maxsize.max(1)) as usize;
    copy_picture_pixels(
        buffer,
        source,
        base,
        stride,
        negotiated.format,
        negotiated.width,
        negotiated.height,
    )
    .ok_or_else(|| {
        format!(
            "缓冲放不下 {}×{} 的画面（行宽 {stride}，偏移 {base}，大小 {}）",
            negotiated.width,
            negotiated.height,
            source.len()
        )
    })
}

/// A read-only mapping of one MemFd buffer.
struct MemFdMap {
    pointer: *mut c_void,
    length: usize,
    /// `mmap` wants a page-aligned offset; `mapoffset` need not be one.
    start: usize,
    size: usize,
}

impl MemFdMap {
    fn new(fd: i32, offset: u32, size: u32) -> std::result::Result<Self, String> {
        if size == 0 {
            return Err("MemFd 缓冲大小为 0".into());
        }
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        let aligned = u64::from(offset) / page * page;
        let start = (u64::from(offset) - aligned) as usize;
        let length = start + size as usize;
        // SAFETY: read-only shared map of a descriptor PipeWire keeps open
        // while the buffer is dequeued. Unmapped on drop, before the buffer
        // is queued back.
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ,
                libc::MAP_SHARED,
                fd,
                aligned as libc::off_t,
            )
        };
        if pointer == libc::MAP_FAILED {
            return Err(format!(
                "映射 MemFd 缓冲失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self {
            pointer,
            length,
            start,
            size: size as usize,
        })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: the mapping covers `start + size` bytes for the life of `self`.
        unsafe { std::slice::from_raw_parts(self.pointer.cast::<u8>().add(self.start), self.size) }
    }
}

impl Drop for MemFdMap {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.pointer, self.length) };
    }
}

fn copy_picture_pixels(
    buffer: &SpaBuffer,
    source: &[u8],
    base: usize,
    stride: usize,
    format: u32,
    width: u32,
    height: u32,
) -> Option<Picture> {
    let (x, y, w, h) = picture_crop(buffer, width, height);
    let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
    let needed = base + (y + h).saturating_sub(1) * stride + (x + w) * 4;
    if w == 0 || h == 0 || needed > source.len() {
        return None;
    }
    let swap = matches!(format, VIDEO_RGBX | VIDEO_RGBA);
    let mut pixels = vec![0u8; w * h * 4];
    if !swap {
        for row in 0..h {
            let start = base + (y + row) * stride + x * 4;
            pixels[row * w * 4..(row + 1) * w * 4].copy_from_slice(&source[start..start + w * 4]);
        }
    } else {
        for row in 0..h {
            let start = base + (y + row) * stride + x * 4;
            let source_row = &source[start..start + w * 4];
            let target = &mut pixels[row * w * 4..(row + 1) * w * 4];
            for (texel, out) in source_row.chunks_exact(4).zip(target.chunks_exact_mut(4)) {
                out[0] = texel[2];
                out[1] = texel[1];
                out[2] = texel[0];
                out[3] = texel[3];
            }
        }
    }
    Some(Picture {
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

fn picture_crop(buffer: &SpaBuffer, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let (mut x, mut y, mut w, mut h) = (0usize, 0usize, width as usize, height as usize);
    if buffer.n_metas == 0 || buffer.metas.is_null() {
        return (0, 0, width, height);
    }
    for index in 0..buffer.n_metas as usize {
        let meta = unsafe { &*buffer.metas.add(index) };
        if meta.kind == META_VIDEO_CROP
            && meta.size as usize >= size_of::<SpaMetaRegion>()
            && !meta.data.is_null()
        {
            let region = unsafe { &*meta.data.cast::<SpaMetaRegion>() };
            if region.width > 0 && region.height > 0 {
                x = region.x.max(0) as usize;
                y = region.y.max(0) as usize;
                w = (region.width as usize).min(width as usize - x.min(width as usize));
                h = (region.height as usize).min(height as usize - y.min(height as usize));
            }
        }
    }
    (x as u32, y as u32, w as u32, h as u32)
}

pub(super) struct Stream {
    api: &'static Api,
    thread_loop: *mut c_void,
    context: *mut c_void,
    core: *mut c_void,
    stream: *mut c_void,
    _hook: Box<SpaHook>,
    _events: Box<StreamEvents>,
    callbacks: *mut Callbacks,
    shared: Arc<Shared>,
    delivered: u64,
}

// SAFETY: the raw handles are used on PipeWire's thread or under its loop
// lock, and `next` only waits on the mutex. The value can move between
// threads; it is not shared.
unsafe impl Send for Stream {}

impl Stream {
    pub fn connect(fd: std::os::fd::OwnedFd, node: u32) -> Result<Self> {
        use std::os::fd::IntoRawFd;
        let api = api()?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        let name = CString::new("openuuyc-capture")?;
        // SAFETY: documented PipeWire setup order. Every handle is checked
        // and torn down in reverse by `Drop`.
        unsafe {
            let thread_loop = (api.thread_loop_new)(name.as_ptr(), std::ptr::null());
            ensure!(!thread_loop.is_null(), "无法创建 PipeWire 线程");
            let context = (api.context_new)(
                (api.thread_loop_get_loop)(thread_loop),
                std::ptr::null_mut(),
                0,
            );
            if context.is_null() {
                (api.thread_loop_destroy)(thread_loop);
                bail!("无法创建 PipeWire 上下文");
            }
            let mut this = Self {
                api,
                thread_loop,
                context,
                core: std::ptr::null_mut(),
                stream: std::ptr::null_mut(),
                _hook: Box::new(std::mem::zeroed()),
                _events: Box::new(StreamEvents {
                    version: STREAM_EVENTS_VERSION,
                    destroy: None,
                    state_changed: Some(on_state_changed),
                    control_info: None,
                    io_changed: None,
                    param_changed: Some(on_param_changed),
                    add_buffer: None,
                    remove_buffer: None,
                    process: Some(on_process),
                    drained: None,
                    command: None,
                    trigger_done: None,
                }),
                callbacks: std::ptr::null_mut(),
                shared: shared.clone(),
                delivered: 0,
            };
            if (api.thread_loop_start)(thread_loop) < 0 {
                (api.context_destroy)(context);
                (api.thread_loop_destroy)(thread_loop);
                bail!("无法启动 PipeWire 线程");
            }
            (api.thread_loop_lock)(thread_loop);
            let result = (|| -> Result<()> {
                // `connect_fd` takes ownership of the descriptor.
                this.core =
                    (api.context_connect_fd)(context, fd.into_raw_fd(), std::ptr::null_mut(), 0);
                ensure!(!this.core.is_null(), "无法连接屏幕共享的 PipeWire 远端");
                let key = |text: &str| CString::new(text).unwrap();
                let (media_type, video, category, capture, role, screen) = (
                    key("media.type"),
                    key("Video"),
                    key("media.category"),
                    key("Capture"),
                    key("media.role"),
                    key("Screen"),
                );
                let props = (api.properties_new)(
                    media_type.as_ptr(),
                    video.as_ptr(),
                    category.as_ptr(),
                    capture.as_ptr(),
                    role.as_ptr(),
                    screen.as_ptr(),
                    std::ptr::null::<c_char>(),
                );
                this.stream = (api.stream_new)(this.core, name.as_ptr(), props);
                ensure!(!this.stream.is_null(), "无法创建 PipeWire 画面流");
                this.callbacks = Box::into_raw(Box::new(Callbacks {
                    api,
                    stream: this.stream,
                    shared,
                }));
                (api.stream_add_listener)(
                    this.stream,
                    &mut *this._hook,
                    &*this._events,
                    this.callbacks.cast(),
                );
                let format = enum_format_shm().aligned();
                let mut params = [format.as_ptr().cast::<c_void>()];
                let connected = (api.stream_connect)(
                    this.stream,
                    DIRECTION_INPUT,
                    node,
                    FLAG_AUTOCONNECT | FLAG_MAP_BUFFERS,
                    params.as_mut_ptr(),
                    params.len() as u32,
                );
                ensure!(connected >= 0, "连接 PipeWire 画面流失败：{connected}");
                Ok(())
            })();
            (api.thread_loop_unlock)(thread_loop);
            result.map(|()| this)
        }
    }

    pub fn stage(&self) -> String {
        let state = lock(&self.shared.state);
        stage(
            state.streaming,
            state.format.map(|format| (format.width, format.height)),
            state.delivered,
            state.unreadable.clone(),
        )
    }

    /// The next picture newer than the last one returned, or `None` on timeout.
    pub fn next(&mut self, timeout: Duration) -> Result<Option<Picture>> {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.shared.state);
        loop {
            if let Some(error) = &state.error {
                bail!("{error}");
            }
            if state.sequence != self.delivered
                && let Some(picture) = state.picture.take()
            {
                self.delivered = state.sequence;
                return Ok(Some(picture));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
    }
}

fn stage(
    streaming: bool,
    format: Option<(u32, u32)>,
    delivered: u64,
    unreadable: Option<String>,
) -> String {
    match (streaming, format, delivered, unreadable) {
        (false, None, _, _) => "门户的画面流没有开始传输".into(),
        (true, None, _, _) => "门户的画面流已开始，但没有协商出画面格式".into(),
        (_, Some((width, height)), 0, _) => format!(
            "画面格式已协商（{width}×{height}），但合成器还没送来画面；有的桌面在画面静止时不发帧"
        ),
        (_, Some(_), delivered, Some(reason)) => {
            format!("合成器送来了 {delivered} 个缓冲，但读不出画面（{reason}）")
        }
        (_, Some(_), delivered, None) => format!("合成器送来了 {delivered} 个缓冲，画面还在处理"),
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let api = self.api;
        // SAFETY: reverse of setup. The loop is stopped before callback data
        // it could still enter is freed.
        unsafe {
            (api.thread_loop_lock)(self.thread_loop);
            if !self.stream.is_null() {
                (api.stream_disconnect)(self.stream);
                (api.stream_destroy)(self.stream);
            }
            if !self.core.is_null() {
                (api.core_disconnect)(self.core);
            }
            (api.thread_loop_unlock)(self.thread_loop);
            (api.thread_loop_stop)(self.thread_loop);
            (api.context_destroy)(self.context);
            (api.thread_loop_destroy)(self.thread_loop);
            if !self.callbacks.is_null() {
                drop(Box::from_raw(self.callbacks));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_names_where_the_first_picture_stopped() {
        assert_eq!(stage(false, None, 0, None), "门户的画面流没有开始传输");
        assert!(stage(true, None, 0, None).contains("没有协商出画面格式"));
        assert!(stage(true, Some((1920, 1080)), 0, None).contains("1920×1080"));
        assert!(
            stage(
                true,
                Some((1920, 1080)),
                3,
                Some("数据类型 2：缓冲为空".into())
            )
            .contains("数据类型 2：缓冲为空")
        );
    }

    #[test]
    fn a_fixed_format_parses_back_and_shm_offer_has_no_modifier() {
        let pod = Pod::object(OBJECT_FORMAT, PARAM_FORMAT, |pod| {
            pod.key(FORMAT_MEDIA_TYPE);
            pod.value(TYPE_ID, &[MEDIA_TYPE_VIDEO]);
            pod.key(FORMAT_VIDEO_FORMAT);
            pod.value(TYPE_ID, &[VIDEO_BGRX]);
            pod.key(FORMAT_VIDEO_SIZE);
            pod.value(TYPE_RECTANGLE, &[2560, 1440]);
        })
        .aligned();
        assert_eq!(
            unsafe { parse_format(pod.as_ptr().cast()) },
            Some(NegotiatedFormat {
                format: VIDEO_BGRX,
                width: 2560,
                height: 1440,
                modifier: None
            })
        );
        let offer = enum_format_shm().aligned();
        let bytes = offer.len() * 8;
        let size = unsafe { offer.as_ptr().cast::<u32>().read() } as usize;
        assert!(size + 8 <= bytes);
        assert_eq!((size + 8) % 8, 0);
        let parsed = unsafe { parse_format(offer.as_ptr().cast()) }.expect("shm offer");
        assert_eq!(parsed.modifier, None);
        assert_eq!(parsed.format, VIDEO_BGRX);
    }

    #[test]
    fn buffers_offer_memptr_and_memfd_only() {
        let pod = buffers().aligned();
        let words: Vec<u32> = (0..14)
            .map(|index| unsafe { pod.as_ptr().cast::<u32>().add(index).read() })
            .collect();
        assert_eq!(words[1], TYPE_OBJECT);
        assert_eq!(words[2], OBJECT_PARAM_BUFFERS);
        assert_eq!(words[3], PARAM_BUFFERS);
        assert_eq!(words[4], BUFFERS_DATA_TYPE);
        assert_eq!(words[7], TYPE_CHOICE);
        assert_eq!(words[8], CHOICE_FLAGS);
        assert_eq!(words[11], TYPE_INT);
        assert_eq!(words[12], SHM_BUFFER_MASK);
        assert_eq!(SHM_BUFFER_MASK & (1 << DATA_MEM_DMABUF), 0);
    }

    /// KWin hands over shared-memory buffers PipeWire did not map: a MemFd
    /// with no data pointer, and a `mapoffset` that is not page aligned.
    #[test]
    fn an_unmapped_memfd_buffer_is_mapped_and_read() {
        use std::io::{Seek as _, SeekFrom, Write as _};
        use std::os::fd::{AsRawFd as _, FromRawFd as _};
        let fd = unsafe { libc::memfd_create(c"openuuyc-test".as_ptr(), 0) };
        assert!(fd >= 0, "memfd_create");
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let offset = 100u32;
        let pixels = [1u8, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
        file.seek(SeekFrom::Start(u64::from(offset))).expect("seek");
        file.write_all(&pixels).expect("write");
        let mut chunk = SpaChunk {
            offset: 0,
            size: pixels.len() as u32,
            stride: 8,
            flags: 0,
        };
        let mut data = SpaData {
            kind: DATA_MEM_FD,
            flags: 0,
            fd: i64::from(file.as_raw_fd()),
            mapoffset: offset,
            maxsize: pixels.len() as u32,
            data: std::ptr::null_mut(),
            chunk: &mut chunk,
        };
        let mut buffer = SpaBuffer {
            n_metas: 0,
            n_datas: 1,
            metas: std::ptr::null_mut(),
            datas: &mut data,
        };
        let format = NegotiatedFormat {
            format: VIDEO_BGRX,
            width: 2,
            height: 2,
            modifier: None,
        };
        let picture = unsafe { read_picture(&mut buffer, format) }.expect("picture");
        assert_eq!((picture.width, picture.height), (2, 2));
        assert_eq!(picture.pixels, pixels);
        unsafe { (*buffer.datas).maxsize = 8 };
        let error = unsafe { read_picture(&mut buffer, format) }.expect_err("too small");
        assert!(error.contains("放不下"), "{error}");
    }

    #[test]
    fn a_dmabuf_buffer_is_rejected() {
        let mut chunk = SpaChunk {
            offset: 0,
            size: 0,
            stride: 16,
            flags: 0,
        };
        let mut data = SpaData {
            kind: DATA_MEM_DMABUF,
            flags: 0,
            fd: -1,
            mapoffset: 0,
            maxsize: 0,
            data: std::ptr::null_mut(),
            chunk: &mut chunk,
        };
        let mut buffer = SpaBuffer {
            n_metas: 0,
            n_datas: 1,
            metas: std::ptr::null_mut(),
            datas: &mut data,
        };
        let format = NegotiatedFormat {
            format: VIDEO_BGRA,
            width: 2,
            height: 2,
            modifier: Some(0),
        };
        let error = unsafe { read_picture(&mut buffer, format) }.expect_err("dmabuf");
        assert!(error.contains("DMA-BUF"), "{error}");
    }

    #[test]
    fn a_crop_keeps_the_inner_pixel_and_swaps_rgba() {
        let mut region = SpaMetaRegion {
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        };
        let mut meta = SpaMeta {
            kind: META_VIDEO_CROP,
            size: size_of::<SpaMetaRegion>() as u32,
            data: (&mut region as *mut SpaMetaRegion).cast(),
        };
        let buffer = SpaBuffer {
            n_metas: 1,
            n_datas: 0,
            metas: &mut meta,
            datas: std::ptr::null_mut(),
        };
        let source = [
            0, 0, 0, 0, 0, 0, 0, 0, 9, 9, 9, 9, 0, 0, 0, 0, 10, 20, 30, 255,
        ];
        let picture = copy_picture_pixels(&buffer, &source, 0, 12, VIDEO_RGBA, 2, 2).unwrap();
        assert_eq!((picture.width, picture.height), (1, 1));
        assert_eq!(picture.pixels, vec![30, 20, 10, 255]);
        assert!(copy_picture_pixels(&buffer, &source[..19], 0, 12, VIDEO_RGBA, 2, 2).is_none());
    }
}
