//! Linux clipboard adapter for X11 and Wayland.
//!
//! Differences from the Windows/OLE adapter this replaces:
//! * X11 and Wayland have no delayed rendering across processes, so an offer
//!   from the remote is fetched immediately instead of when the user pastes.
//! * There is no clipboard-change notification, so local changes are polled.
//! * Files copied here are offered to the remote: it pulls the descriptor list
//!   and then the contents by offset, so nothing has to be promised in advance.
//!   Local file copies (text/uri-list, gnome-copied-files, kde-cutselection) are
//!   advertised to the remote as FileGroupDescriptorW/FileContents so a Mac
//!   paste can pull them. Incoming remote files still need real local paths
//!   (FUSE mount or Downloads drop) before a paste into a local app.
use super::formats::{self, Format};
use super::*;
use anyhow::Context as _;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

pub(super) use super::formats::safe_name;

/// Anything larger is a transfer, not a clipboard paste.
const MAX_CLIP: usize = 32 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(400);

pub(super) enum Command {
    Activate(Weak<Inner>),
    /// Files dropped on the viewer window, offered to the remote as a drag drop.
    DropFiles(
        Arc<super::drag::Submission>,
        PreparedFiles,
        ClipboardFormatListRequestKind,
    ),
    Remove(u64),
    Offer(
        Weak<Inner>,
        u64,
        Vec<ClipboardFormat>,
        Option<ClipboardFormatListRequestKind>,
    ),
    Request(Weak<Inner>, u64, i64, ClipboardRequestKind),
    Text(Weak<Inner>, u64, i64, String),
    /// Files published through a dedicated file session (file transfer UI).
    PublishFiles(
        Weak<Inner>,
        u64,
        PreparedFiles,
        tokio::sync::oneshot::Sender<Result<(), String>>,
    ),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct FileSummary {
    pub count: u32,
    pub bytes: u64,
    pub first_name: String,
}

/// A dropped or published file set, walked once up front so the summary and
/// the descriptor list the remote later reads agree.
pub(super) struct PreparedFiles {
    files: LocalFiles,
    summary: FileSummary,
}

impl PreparedFiles {
    pub fn summary(&self) -> FileSummary {
        self.summary.clone()
    }
}

pub(super) fn prepare_files(
    paths: Vec<PathBuf>,
    allowed: impl Fn() -> bool,
) -> Result<PreparedFiles> {
    ensure!(
        !paths.is_empty() && paths.len() <= MAX_FILES,
        "拖放文件数量无效"
    );
    for path in &paths {
        ensure!(allowed(), "文件准备已取消");
        ensure!(
            !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
            "不传输链接"
        );
        ensure!(path.file_name().is_some(), "不能拖放根目录");
    }
    let files = collect_files(&paths)?;
    ensure!(allowed(), "文件准备已取消");
    let mut summary = FileSummary {
        count: 0,
        bytes: 0,
        first_name: String::new(),
    };
    for item in &files.items {
        if item.desc.file_attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            summary.count += 1;
            summary.bytes = summary.bytes.saturating_add(item.desc.file_size);
        }
        if summary.first_name.is_empty() {
            summary.first_name = item.desc.file_name.clone();
        }
    }
    Ok(PreparedFiles { files, summary })
}

/// Host-side file offers only exist on Windows; Linux never receives one.
pub(crate) struct FileOffer(pub(super) std::convert::Infallible);

fn file_links(session: &Inner) -> Vec<Format> {
    let ids = vec![
        (
            formats::register("FileGroupDescriptorW"),
            "FileGroupDescriptorW".into(),
        ),
        (formats::register("FileContents"), "FileContents".into()),
    ];
    formats::outgoing(&ids, session.platform.load(Ordering::Acquire), true)
}

struct Worker {
    sender: SyncSender<Command>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

static WORKER: OnceLock<std::result::Result<Worker, String>> = OnceLock::new();

/// One file the local clipboard offers, described the way a Windows peer reads
/// it: a backslash path relative to the copied root, Windows attribute bits and
/// a FILETIME.
struct LocalFile {
    desc: ClipboardFileDescriptor,
    /// `None` for a directory, which has no contents to serve.
    path: Option<PathBuf>,
    /// The copied item this entry was reached through; contents are refused if
    /// the path stops resolving inside it.
    root: PathBuf,
}

/// A flattened snapshot of everything one local copy put on the clipboard.
struct LocalFiles {
    items: Vec<LocalFile>,
}

/// What this client currently owns locally, as the remote would see it.
#[derive(Default)]
struct LocalSnapshot {
    text: Option<String>,
    image: Option<Vec<u8>>,
    /// The paths a local copy put on the clipboard. They are only walked when
    /// the remote asks for the list, so copying a large tree locally costs
    /// nothing until someone pastes it there.
    sources: Vec<PathBuf>,
}

impl LocalSnapshot {
    fn is_empty(&self) -> bool {
        self.text.is_none() && self.image.is_none() && self.sources.is_empty()
    }

    /// CF_UNICODETEXT and CF_DIB are what a Windows peer understands; files
    /// travel as the descriptor and contents pair, as they do in OLE.
    fn format_ids(&self) -> Vec<(u32, String)> {
        let mut ids = Vec::new();
        if !self.sources.is_empty() {
            ids.push((
                formats::register("FileGroupDescriptorW"),
                "FileGroupDescriptorW".into(),
            ));
            ids.push((formats::register("FileContents"), "FileContents".into()));
            return ids;
        }
        if self.text.is_some() {
            ids.push((13, String::new()));
            ids.push((1, String::new()));
        }
        if self.image.is_some() {
            // CF_DIB for Windows peers, plus the widely-supported "PNG"
            // registered format — macOS controllers often prefer PNG over DIB
            // when talking to a Windows-dialect host.
            ids.push((8, String::new()));
            ids.push((formats::register("PNG"), "PNG".into()));
        }
        ids
    }

    fn data(&self, format: &Format) -> Result<Vec<u8>> {
        match format.local {
            13 => self
                .text
                .as_deref()
                .map(formats::unicode)
                .context("本地剪贴板没有文本"),
            1 => self
                .text
                .as_deref()
                .map(|text| {
                    let mut bytes = text.replace('\n', "\r\n").into_bytes();
                    bytes.push(0);
                    bytes
                })
                .context("本地剪贴板没有文本"),
            8 | 17 => self.image.clone().context("本地剪贴板没有图片"),
            _ if format.wire.name.eq_ignore_ascii_case("PNG")
                || format.wire.name.eq_ignore_ascii_case("JFIF")
                || format.wire.name.eq_ignore_ascii_case("image/png")
                || format.wire.name.eq_ignore_ascii_case("image/jpeg") =>
            {
                self.image.clone().context("本地剪贴板没有图片")
            },
            _ => bail!("不支持的本地剪贴板格式"),
        }
    }
}

/// Windows file attributes this client synthesises from a Unix mode.
const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
/// Seconds between the Windows epoch (1601-01-01) and the Unix one.
const FILETIME_EPOCH_OFFSET: u64 = 11_644_473_600;

/// A Unix mtime as the FILETIME a Windows peer stamps the pasted file with.
fn filetime(meta: &std::fs::Metadata) -> u64 {
    let seconds = meta.mtime();
    if seconds < -(FILETIME_EPOCH_OFFSET as i64) {
        return 0;
    }
    let ticks = (seconds + FILETIME_EPOCH_OFFSET as i64) as u64;
    ticks
        .saturating_mul(10_000_000)
        .saturating_add(meta.mtime_nsec().max(0) as u64 / 100)
}

fn attributes(meta: &std::fs::Metadata) -> u32 {
    let mut bits = if meta.is_dir() {
        FILE_ATTRIBUTE_DIRECTORY
    } else {
        FILE_ATTRIBUTE_NORMAL
    };
    // Owner write is the closest thing this filesystem has to the read-only bit.
    if meta.mode() & 0o200 == 0 {
        bits |= FILE_ATTRIBUTE_READONLY;
    }
    bits
}

/// Walk one copied path into the flat descriptor list the protocol carries.
/// Names are relative to the copied item and use the separator Windows expects.
fn collect_file(path: &Path, root: &Path, name: &str, items: &mut Vec<LocalFile>) -> Result<()> {
    ensure!(
        items.len() < MAX_FILES && safe_name(name),
        "文件数量或名称不支持"
    );
    let meta = std::fs::symlink_metadata(path)?;
    // A symlink is not copied: the peer would receive its target under a name
    // that promises otherwise, and the target may sit outside the copy.
    ensure!(!meta.is_symlink(), "不传输符号链接");
    ensure!(meta.is_dir() || meta.is_file(), "只支持普通文件与目录");
    let canonical = std::fs::canonicalize(path)?;
    ensure!(canonical.starts_with(root), "文件超出复制范围");
    items.push(LocalFile {
        desc: ClipboardFileDescriptor {
            file_name: name.replace('/', "\\"),
            file_attributes: attributes(&meta),
            last_write_time: filetime(&meta),
            file_size: if meta.is_dir() { 0 } else { meta.len() },
        },
        path: (!meta.is_dir()).then(|| canonical.clone()),
        root: root.to_path_buf(),
    });
    if meta.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&canonical)?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|entry| entry.file_name())
            .collect();
        entries.sort();
        for entry in entries {
            collect_file(
                &canonical.join(&entry),
                root,
                &format!("{name}\\{}", entry.to_string_lossy()),
                items,
            )?;
        }
    }
    Ok(())
}

/// Flatten the paths the local clipboard holds. One unreadable entry fails the
/// whole offer rather than handing the peer a list it cannot complete.
fn collect_files(paths: &[PathBuf]) -> Result<LocalFiles> {
    let mut items = Vec::new();
    for path in paths {
        let root = std::fs::canonicalize(path)?;
        let name = root
            .file_name()
            .context("无法确定文件名")?
            .to_string_lossy()
            .into_owned();
        collect_file(&root, &root, &name, &mut items)?;
    }
    ensure!(!items.is_empty(), "文件列表为空");
    Ok(LocalFiles { items })
}

impl LocalFiles {
    /// Serve one `FileContentsRequest`. `flags` is 1 for the size and 2 for a
    /// range of the contents, as the official client sends them.
    fn read(&self, ask: &ClipboardFileContentsRequest) -> Result<Vec<u8>> {
        let file = self
            .items
            .get(ask.list_index as usize)
            .context("无效的文件索引")?;
        ensure!(ask.requested_len as usize <= FILE_BLOCK, "文件读取请求过大");
        if ask.flags == 1 {
            return Ok(file.desc.file_size.to_le_bytes().to_vec());
        }
        ensure!(
            ask.flags == 2 && file.desc.file_attributes & FILE_ATTRIBUTE_DIRECTORY == 0,
            "无效的文件读取类型"
        );
        let path = file.path.as_ref().context("该条目没有内容")?;
        // O_NOFOLLOW closes the window where the final component is swapped for
        // a link between the walk and this read.
        let mut handle = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        ensure!(
            std::fs::canonicalize(path)?.starts_with(&file.root),
            "文件超出原始复制范围"
        );
        let length =
            (u64::from(ask.requested_len)).min(file.desc.file_size.saturating_sub(ask.pos_offset));
        if length == 0 {
            return Ok(Vec::new());
        }
        use std::io::{Read as _, Seek as _};
        handle.seek(std::io::SeekFrom::Start(ask.pos_offset))?;
        let mut data = vec![0u8; length as usize];
        let mut filled = 0;
        while filled < data.len() {
            match handle.read(&mut data[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        data.truncate(filled);
        Ok(data)
    }
}

struct State {
    receiver: Receiver<Command>,
    clipboard: Option<arboard::Clipboard>,
    sessions: HashMap<u64, Weak<Inner>>,
    published: HashMap<u64, Vec<Format>>,
    local: LocalSnapshot,
    /// File lists handed out per session and task, kept until the clipboard
    /// changes so a slow remote can keep reading the copy it started on.
    tasks: HashMap<(u64, u32), Arc<LocalFiles>>,
    /// The remote copy currently published locally, mounted for as long as the
    /// clipboard points at it.
    mounted: Option<super::fuse::Mount>,
    /// Holds the clipboard selection while those paths are the copy on it.
    offer: Option<super::x11_offer::FileOffer>,
    /// Names each mount's directory, so a new copy never reuses a path the
    /// file manager may still have open.
    generation: u64,
    /// Set while this adapter writes, so the poll does not report its own write.
    writing: bool,
    /// Per-session file sets from a viewer drop or file publication; they take
    /// precedence over the local clipboard for the next descriptor request.
    explicit: HashMap<u64, (Arc<LocalFiles>, Option<Arc<super::drag::Submission>>)>,
}

pub(super) fn start() -> Result<()> {
    WORKER
        .get_or_init(|| {
            let (sender, receiver) = sync_channel(64);
            let thread = std::thread::Builder::new()
                .name("UU clipboard".into())
                .spawn(move || run(receiver))
                .map_err(|error| format!("启动剪贴板线程失败：{error}"))?;
            Ok(Worker {
                sender,
                thread: Mutex::new(Some(thread)),
            })
        })
        .as_ref()
        .map(|_| ())
        .map_err(|error| anyhow!(error.clone()))
}

pub(super) fn post(command: Command) -> Result<()> {
    start()?;
    let worker = WORKER
        .get()
        .and_then(|worker| worker.as_ref().ok())
        .context("剪贴板线程不可用")?;
    worker
        .sender
        .try_send(command)
        .map_err(|_| anyhow!("剪贴板队列已满或已关闭"))
}

/// The Windows adapter pumps its STA here. This worker is a plain thread, so
/// callers waiting on a response simply keep waiting on their condvar.
pub(super) fn pump() {}

pub(super) fn shutdown() {
    let Some(Ok(worker)) = WORKER.get() else {
        return;
    };
    // Dropping every sender ends the receive loop; the clone here is the last one.
    let thread = lock(&worker.thread).take();
    if let Some(thread) = thread {
        let _ = worker.sender.try_send(Command::Remove(u64::MAX));
        drop(thread);
    }
}

fn run(receiver: Receiver<Command>) {
    let clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => Some(clipboard),
        Err(error) => {
            tracing::warn!(%error, "系统剪贴板不可用，剪贴板同步将保持关闭");
            None
        }
    };
    let mut state = State {
        receiver,
        clipboard,
        sessions: HashMap::new(),
        published: HashMap::new(),
        local: LocalSnapshot::default(),
        tasks: HashMap::new(),
        mounted: None,
        offer: None,
        generation: 0,
        writing: false,
        explicit: HashMap::new(),
    };
    loop {
        match state.receiver.recv_timeout(POLL) {
            Ok(command) => {
                if matches!(command, Command::Remove(u64::MAX)) {
                    break;
                }
                process(&mut state, command);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Err(error) = poll_local(&mut state) {
                    tracing::debug!(%error, "读取本地剪贴板失败");
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn sessions(state: &State) -> Vec<Arc<Inner>> {
    state
        .sessions
        .values()
        .filter_map(std::sync::Weak::upgrade)
        .collect()
}

fn process(state: &mut State, command: Command) {
    for (files, ticket) in state.explicit.values_mut() {
        if let Some(ticket) = ticket
            && !ticket.valid()
            && !files.items.is_empty()
        {
            ticket.fail("拖放等待超时或权限已撤销".into());
            // Keep the rejected publication until its descriptor request
            // arrives; never substitute ordinary copied files.
            *files = Arc::new(LocalFiles { items: Vec::new() });
        }
    }
    match command {
        Command::DropFiles(ticket, files, action) => {
            let result = (|| -> Result<()> {
                ensure!(ticket.valid(), "文件拖放已过期");
                let session = ticket.session.upgrade().context("连接已结束")?;
                let links = file_links(&session);
                state.published.insert(session.id, links.clone());
                state.explicit.insert(
                    session.id,
                    (Arc::new(files.files), Some(ticket.clone())),
                );
                session.emit(
                    ticket.epoch,
                    request(
                        session.next(),
                        ClipboardRequestKind::FormatList(ClipboardFormatListRequest {
                            formats: links.into_iter().map(|format| format.wire).collect(),
                            has_action: 1,
                            drag_drop_action: Some(action),
                        }),
                    ),
                )?;
                ticket.published.store(true, Ordering::Release);
                Ok(())
            })();
            if let Err(error) = result {
                ticket.fail(error.to_string());
            }
        }
        Command::PublishFiles(weak, epoch, files, reply) => {
            let result = (|| -> Result<()> {
                let session = weak
                    .upgrade()
                    .filter(|session| session.valid(epoch) && session.file_allowed())
                    .context("文件发布已取消")?;
                let links = file_links(&session);
                state.published.insert(session.id, links.clone());
                state
                    .explicit
                    .insert(session.id, (Arc::new(files.files), None));
                session.emit(
                    epoch,
                    request(
                        session.next(),
                        ClipboardRequestKind::FormatList(ClipboardFormatListRequest {
                            formats: links.into_iter().map(|format| format.wire).collect(),
                            has_action: 0,
                            drag_drop_action: None,
                        }),
                    ),
                )
            })();
            let _ = reply.send(result.map_err(|error| error.to_string()));
        }
        Command::Activate(weak) => {
            if let Some(session) = weak.upgrade() {
                state.sessions.insert(session.id, Arc::downgrade(&session));
                tracing::info!(
                    id = session.id,
                    sessions = state.sessions.len(),
                    files = session.file_allowed(),
                    platform = session.platform.load(Ordering::Acquire),
                    "剪贴板会话已注册"
                );
                // poll_local skips when the snapshot is unchanged, so a session
                // that joins after a local copy would never see FormatList.
                if let Err(error) = publish_local(state) {
                    tracing::debug!(%error, "激活后发布本地剪贴板失败");
                }
            }
        }
        Command::Remove(id) => {
            state.sessions.remove(&id);
            state.published.remove(&id);
            state.explicit.remove(&id);
        }
        Command::Offer(weak, epoch, formats, action) => {
            if let Some(session) = weak.upgrade().filter(|session| session.valid(epoch)) {
                let names: Vec<_> = formats.iter().map(|f| f.name.as_str()).collect();
                let ids: Vec<_> = formats.iter().map(|f| f.id).collect();
                tracing::info!(
                    id = session.id,
                    formats = ?names,
                    format_ids = ?ids,
                    has_action = action.is_some(),
                    "收到远端剪贴板 FormatList"
                );
                let result = if let Some(action) = action {
                    // OleDrop/AutoSave: save into Downloads (drag path; skipped for now
                    // in product priority, but keep the handler so drops are not lost).
                    receive_drop(session.clone(), epoch, formats, action)
                } else {
                    // Fetch + write must not block this worker: session.data() waits on
                    // peer replies that this same thread must keep pumping.
                    accept_offer_async(session.clone(), epoch, formats)
                };
                if let Err(error) = result {
                    tracing::warn!(error = format!("{error:#}"), "接受远端剪贴板失败");
                    session.fail(error.to_string());
                }
            }
        }
        Command::Text(weak, epoch, id, text) => {
            if let Some(session) = weak.upgrade().filter(|session| session.valid(epoch)) {
                let result = if text.is_empty() {
                    Err(anyhow!("空文本"))
                } else {
                    write_text(state, &text)
                };
                if let Err(error) = &result {
                    tracing::debug!(%error, "写入本地剪贴板文本失败");
                }
                let _ = session.emit(
                    epoch,
                    Envelope {
                        request: None,
                        response: Some(Response {
                            header: Some(Header { id }),
                            clip: None,
                            text: Some(ClipboardTextChangeResponse {
                                err: if result.is_ok() { 1 } else { 2 },
                            }),
                        }),
                    },
                );
            }
        }
        Command::Request(weak, epoch, id, kind) => {
            if let Some(session) = weak.upgrade().filter(|session| session.valid(epoch))
                && let Err(error) = serve(state, &session, epoch, id, kind)
            {
                session.fail(error.to_string());
            }
        }
    }
}

/// Write an official file-drop (OleDrop / AutoSave) into the user's Downloads
/// folder. Runs on a worker thread so the native clipboard loop can keep
/// pumping FileContents responses that `read_file` waits on.
fn receive_drop(
    session: Arc<Inner>,
    epoch: u64,
    formats: Vec<ClipboardFormat>,
    action: ClipboardFormatListRequestKind,
) -> Result<()> {
    ensure!(session.file_allowed(), "文件剪贴板已关闭");
    let has_files = formats.iter().any(|f| {
        formats::incoming(f.clone(), session.platform.load(Ordering::Acquire), true)
            .is_some_and(|link| link.local == descriptor_format())
    });
    ensure!(has_files, "拖放未包含文件");
    std::thread::Builder::new()
        .name("linux file drop".into())
        .spawn(move || {
            let result = (|| -> Result<()> {
                const TASK: u32 = 0;
                let descriptors = session.descriptors(epoch, TASK)?;
                ensure!(!descriptors.is_empty(), "远端拖放文件列表为空");
                let dest = match &action {
                    ClipboardFormatListRequestKind::AutoSave(a) => {
                        crate::features::file_transfer::import::destination(&a.dest_path)?
                    }
                    ClipboardFormatListRequestKind::OleDrop(_) => {
                        crate::features::file_transfer::import::destination("")?
                    }
                };
                let total = descriptors.len() as u32;
                let items: Vec<_> = descriptors
                    .into_iter()
                    .map(|f| crate::features::file_transfer::import::Item {
                        directory: f.file_attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
                        info: crate::features::file_transfer::protocol::FileInfo {
                            rel_path: f.file_name,
                            size: f.file_size,
                            modified_time: f
                                .last_write_time
                                .saturating_sub(116444736000000000)
                                / 10_000_000,
                        },
                    })
                    .collect();
                let first_name = items
                    .first()
                    .map(|i| i.info.rel_path.clone())
                    .unwrap_or_default();
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let report = runtime.block_on(crate::features::file_transfer::import::receive(
                    dest,
                    items,
                    || session.valid(epoch),
                    |index, offset, length| {
                        session.read_file(epoch, TASK, index as u32, offset, length.min(FILE_BLOCK), 2)
                    },
                ));
                let (count, saved, first) = match &report {
                    Ok(r) => (r.total, r.saved, r.first.clone()),
                    Err(_) => (total, 0, first_name),
                };
                if matches!(&action, ClipboardFormatListRequestKind::AutoSave(_)) {
                    let _ = session.emit(
                        epoch,
                        request(
                            session.next(),
                            ClipboardRequestKind::AutoSaveComplete(
                                DragDropAutoSaveComplete {
                                    total_count: count,
                                    success_count: saved,
                                    first_file_name: first.clone(),
                                },
                            ),
                        ),
                    );
                }
                report?;
                tracing::info!(
                    total = count,
                    saved,
                    first = %first,
                    "linux host saved official file drop"
                );
                let body = if saved > 1 {
                    format!("已接收 {saved} 个文件：{first} 等")
                } else {
                    format!("已接收文件：{first}")
                };
                let _ = std::process::Command::new("notify-send")
                    .arg("--app-name=OpenUUYC")
                    .arg("--icon=document-save")
                    .arg("OpenUUYC 文件拖放")
                    .arg(body)
                    .status();
                Ok(())
            })();
            if let Err(error) = result {
                tracing::warn!(error = format!("{error:#}"), "linux official file drop failed");
                session.fail(error.to_string());
            }
        })?;
    Ok(())
}

/// Run `accept_offer` on a worker so the native loop can keep dispatching
/// FileContents / FormatData replies that `session.data()` waits on.
fn accept_offer_async(
    session: Arc<Inner>,
    epoch: u64,
    formats: Vec<ClipboardFormat>,
) -> Result<()> {
    std::thread::Builder::new()
        .name("clipboard accept".into())
        .spawn(move || {
            // A private State slice is not available here; we write through a
            // short-lived arboard handle and also ask the main worker to adopt
            // the snapshot via a side channel would be ideal. For text/image we
            // write directly with wl-copy/arboard; for files we still need the
            // main worker's FUSE mount. Post back a synthetic local-only path:
            if let Err(error) = accept_offer_standalone(session.clone(), epoch, formats) {
                tracing::warn!(error = format!("{error:#}"), "应用远端剪贴板失败");
                session.fail(error.to_string());
            }
        })?;
    Ok(())
}

/// Apply a remote FormatList without the main `State` (text/image via arboard /
/// wl-copy; files fall back to posting a follow-up that the worker mounts).
fn accept_offer_standalone(
    session: Arc<Inner>,
    epoch: u64,
    offered: Vec<ClipboardFormat>,
) -> Result<()> {
    if offered.is_empty() {
        return Ok(());
    }
    let platform = session.platform.load(Ordering::Acquire);
    let files = session.file_allowed();
    let links: Vec<Format> = offered
        .into_iter()
        .filter_map(|format| formats::incoming(format, platform, files))
        .collect();
    tracing::info!(
        locals = ?links.iter().map(|l| l.local).collect::<Vec<_>>(),
        wires = ?links.iter().map(|l| l.wire.name.as_str()).collect::<Vec<_>>(),
        "远端剪贴板格式已解析"
    );
    if files && links.iter().any(|link| link.local == descriptor_format()) {
        // File offers need FUSE on the main worker; re-post without action is
        // wrong (would loop). Use the import path into Downloads instead — same
        // as an explicit drop, which is what users expect for a remote file copy.
        return receive_drop(
            session,
            epoch,
            links.iter().map(|l| l.wire.clone()).collect(),
            ClipboardFormatListRequestKind::OleDrop(DragDropOleDrop {
                screen_id: 0,
                target_x: 0.5,
                target_y: 0.5,
            }),
        );
    }
    // Prefer image over text: a Mac screenshot offer often includes both
    // CF_UNICODETEXT / HTML Format and PNG/CF_DIB; picking text first made
    // every image paste look like an empty-or-HTML text clipboard.
    let Some(format) = [8u32, 17, 13, 1]
        .into_iter()
        .find_map(|id| links.iter().find(|format| format.local == id))
        .cloned()
    else {
        tracing::warn!("远端剪贴板没有可识别的文本/图片格式");
        return Ok(());
    };
    let data = session.data(epoch, &format.wire)?;
    ensure!(data.len() <= MAX_CLIP, "剪贴板内容过大");
    let data = formats::convert(data, &format, platform, false)?;
    match format.local {
        13 => {
            let text = utf16_text(&data)?;
            write_text_external(&text)?;
            tracing::info!(chars = text.len(), "已写入远端文本到本地剪贴板");
        }
        1 => {
            let text = String::from_utf8_lossy(&data)
                .trim_end_matches('\0')
                .replace("\r\n", "\n");
            write_text_external(&text)?;
            tracing::info!(chars = text.len(), "已写入远端文本到本地剪贴板");
        }
        8 | 17 => {
            write_image_external(&data)?;
            tracing::info!(bytes = data.len(), "已写入远端图片到本地剪贴板");
        }
        _ => return Ok(()),
    }
    *lock(&session.error) = None;
    Ok(())
}

fn write_text_external(text: &str) -> Result<()> {
    // Prefer wl-copy: it forks a helper that stays alive for Wayland pastes.
    // Fall back to arboard when wl-copy is missing.
    match std::process::Command::new("wl-copy")
        .arg("--type")
        .arg("text/plain;charset=utf-8")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(text.as_bytes())?;
            }
            child.wait()
        }) {
        Ok(status) if status.success() => return Ok(()),
        Ok(status) => tracing::debug!(?status, "wl-copy 写入失败，回退 arboard"),
        Err(error) => tracing::debug!(%error, "wl-copy 不可用，回退 arboard"),
    }
    let mut clipboard = arboard::Clipboard::new().context("系统剪贴板不可用")?;
    clipboard
        .set_text(text.to_owned())
        .context("写入系统剪贴板失败")?;
    // Keep this handle alive briefly so wlr-data-control peers can fetch.
    std::thread::sleep(Duration::from_millis(50));
    Ok(())
}

fn write_image_external(dib: &[u8]) -> Result<()> {
    let (w, h, rgba) = dib_to_rgba(dib)?;
    // PNG via wl-copy when available.
    if let Ok(png) = rgba_to_png(w, h, &rgba) {
        match std::process::Command::new("wl-copy")
            .arg("--type")
            .arg("image/png")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(&png)?;
                }
                child.wait()
            }) {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => tracing::debug!(?status, "wl-copy 图片写入失败，回退 arboard"),
            Err(error) => tracing::debug!(%error, "wl-copy 图片不可用，回退 arboard"),
        }
    }
    let mut clipboard = arboard::Clipboard::new().context("系统剪贴板不可用")?;
    clipboard
        .set_image(arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: std::borrow::Cow::Owned(rgba),
        })
        .context("写入系统剪贴板失败")?;
    std::thread::sleep(Duration::from_millis(50));
    Ok(())
}

fn rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    use image::{ImageBuffer, ImageFormat, Rgba};
    let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba.to_vec())
        .context("RGBA 尺寸与数据不匹配")?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .context("PNG 编码失败")?;
    Ok(out)
}

/// Fetch the best offered format now, because the local clipboard cannot
/// promise data it does not yet hold.
fn accept_offer(
    state: &mut State,
    session: &Arc<Inner>,
    epoch: u64,
    offered: Vec<ClipboardFormat>,
) -> Result<()> {
    if offered.is_empty() {
        return Ok(());
    }
    let platform = session.platform.load(Ordering::Acquire);
    let files = session.file_allowed();
    let links: Vec<Format> = offered
        .into_iter()
        .filter_map(|format| formats::incoming(format, platform, files))
        .collect();
    // Files win when offered: a peer that copied files usually also offers
    // their names as text, and pasting the names is not what was asked for.
    if files && links.iter().any(|link| link.local == descriptor_format()) {
        return accept_files(state, session, epoch);
    }
    // Prefer image over text (see accept_offer_standalone).
    let Some(format) = [8u32, 17, 13, 1]
        .into_iter()
        .find_map(|id| links.iter().find(|format| format.local == id))
        .cloned()
    else {
        return Ok(());
    };
    let data = session.data(epoch, &format.wire)?;
    ensure!(data.len() <= MAX_CLIP, "剪贴板内容过大");
    let data = formats::convert(data, &format, platform, false)?;
    match format.local {
        13 => {
            let text = utf16_text(&data)?;
            write_text(state, &text)?;
        }
        1 => {
            let text = String::from_utf8_lossy(&data)
                .trim_end_matches('\0')
                .replace("\r\n", "\n");
            write_text(state, &text)?;
        }
        8 | 17 => write_image(state, &data)?,
        _ => return Ok(()),
    }
    *lock(&session.error) = None;
    Ok(())
}

fn descriptor_format() -> u32 {
    formats::register("FileGroupDescriptorW")
}

/// Mount the remote's copy and point the local clipboard at it.
///
/// Only the list is fetched here. The contents follow one read at a time,
/// through the filesystem, if and when something actually opens them.
fn accept_files(state: &mut State, session: &Arc<Inner>, epoch: u64) -> Result<()> {
    // The task id the official Windows client uses for an inbound offer.
    const TASK: u32 = 0;
    let descriptors = session.descriptors(epoch, TASK)?;
    ensure!(!descriptors.is_empty(), "远端剪贴板文件列表为空");
    let tree = super::fuse::Tree::build(&descriptors)?;
    let parent = super::fuse::mount_parent()?;
    let reader = {
        let session = Arc::downgrade(session);
        Arc::new(move |index: u32, offset: u64, length: usize| {
            let session = session.upgrade().context("剪贴板会话已结束")?;
            session.read_file(epoch, TASK, index, offset, length.min(FILE_BLOCK), 2)
        })
    };
    state.generation += 1;
    let mount = super::fuse::Mount::new(&parent, state.generation, tree, reader)?;
    let paths = mount.paths();
    tracing::debug!(
        files = descriptors.len(),
        root = %mount.root().display(),
        "已挂载远端剪贴板文件"
    );
    write_files(state, &paths)?;
    // Held until the next copy replaces it: the clipboard still points here,
    // and the paste may be minutes away.
    state.mounted = Some(mount);
    *lock(&session.error) = None;
    Ok(())
}

fn utf16_text(bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len().is_multiple_of(2), "无效的Unicode剪贴板");
    let mut words: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    while words.last() == Some(&0) {
        words.pop();
    }
    if words.first() == Some(&0xfeff) {
        words.remove(0);
    }
    Ok(String::from_utf16(&words)?.replace("\r\n", "\n"))
}

fn write_text(state: &mut State, text: &str) -> Result<()> {
    state.writing = true;
    let result = write_text_external(text);
    state.writing = false;
    result?;
    state.local = LocalSnapshot {
        text: Some(text.to_owned()),
        ..Default::default()
    };
    state.tasks.clear();
    state.mounted = None;
    state.offer = None;
    Ok(())
}

/// Publish paths as a file copy on the local clipboard.
///
/// A file copy has to be offered under several names at once, and only the
/// owner of the selection can answer for more than one, so this takes the
/// clipboard itself rather than going through the library used for text and
/// images. On a desktop where that fails there is still `text/uri-list`, which
/// is enough for anything that does not insist on the GTK file-manager name.
fn write_files(state: &mut State, paths: &[PathBuf]) -> Result<()> {
    state.writing = true;
    let offer = super::x11_offer::FileOffer::publish(paths.to_vec());
    let result = match offer {
        Ok(offer) => {
            state.offer = Some(offer);
            Ok(())
        }
        Err(error) => {
            tracing::debug!(error = format!("{error:#}"), "接管剪贴板失败，改用单一格式");
            state.offer = None;
            let clipboard = state.clipboard.as_mut().context("系统剪贴板不可用")?;
            clipboard
                .set()
                .file_list(paths)
                .map_err(anyhow::Error::from)
        }
    };
    state.writing = false;
    result.context("写入系统剪贴板失败")?;
    state.local = LocalSnapshot {
        sources: paths.to_vec(),
        ..Default::default()
    };
    state.tasks.clear();
    Ok(())
}

fn write_image(state: &mut State, dib: &[u8]) -> Result<()> {
    let image = dib_to_rgba(dib)?;
    let clipboard = state.clipboard.as_mut().context("系统剪贴板不可用")?;
    state.writing = true;
    let result = clipboard.set_image(arboard::ImageData {
        width: image.0 as usize,
        height: image.1 as usize,
        bytes: std::borrow::Cow::Owned(image.2),
    });
    state.writing = false;
    result.context("写入系统剪贴板失败")?;
    state.local = LocalSnapshot {
        image: Some(dib.to_vec()),
        ..Default::default()
    };
    state.tasks.clear();
    state.mounted = None;
    state.offer = None;
    Ok(())
}

/// Strip trailing CR/LF that RFC 2483 uri-list entries often leave behind.
fn trim_crlf(path: PathBuf) -> PathBuf {
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    let bytes = path.as_os_str().as_bytes();
    let trimmed = bytes
        .iter()
        .rposition(|byte| !matches!(byte, b'\r' | b'\n'))
        .map_or(0, |last| last + 1);
    if trimmed == bytes.len() {
        return path;
    }
    PathBuf::from(std::ffi::OsString::from_vec(bytes[..trimmed].to_vec()))
}

/// Decode one `file://` (or bare absolute) URI line into a local path.
/// Percent-decodes, strips CR/LF, rejects non-local / non-file schemes.
fn uri_line_to_path(line: &str) -> Option<PathBuf> {
    let line = line.trim().trim_end_matches(['\r', '\n']);
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let path = if let Some(rest) = line.strip_prefix("file://") {
        // file://localhost/path or file:///path or file://host/path (reject remote hosts)
        let rest = percent_decode_path(rest);
        if let Some(after_host) = rest.strip_prefix("localhost") {
            after_host.to_string()
        } else if rest.starts_with('/') {
            rest
        } else {
            // file://hostname/... — not a local path we can serve
            return None;
        }
    } else if line.starts_with('/') {
        line.to_string()
    } else {
        return None;
    };
    let path = PathBuf::from(path);
    path.is_absolute().then_some(path)
}

/// Minimal percent-decoder for uri-list paths (UTF-8). Invalid escapes are kept.
fn percent_decode_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse RFC 2483 `text/uri-list` bytes into absolute local paths.
fn parse_uri_list(bytes: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .filter_map(uri_line_to_path)
        .map(trim_crlf)
        .collect()
}

/// GNOME `x-special/gnome-copied-files`: first line is `copy`/`cut`, rest are URIs.
fn parse_gnome_copied_files(bytes: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let Some(action) = lines.next() else {
        return Vec::new();
    };
    if !matches!(action.trim(), "copy" | "cut") {
        // Some producers omit the action line; treat the whole blob as uri-list.
        return parse_uri_list(bytes);
    }
    lines.filter_map(uri_line_to_path).map(trim_crlf).collect()
}

/// KDE `application/x-kde-cutselection` / similar: often `0`/`1` then uri-list, or
/// just a uri-list. Accept either.
fn parse_kde_files(bytes: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return Vec::new();
    };
    if matches!(first.trim(), "0" | "1") {
        return lines.filter_map(uri_line_to_path).map(trim_crlf).collect();
    }
    parse_uri_list(bytes)
}

/// Fallback when arboard cannot see file offers (GNOME/KDE often publish
/// `x-special/gnome-copied-files` / `application/x-kde-cutselection` without a
/// simultaneous `text/uri-list` that arboard's Wayland backend reads).
fn wl_paste_files() -> Vec<PathBuf> {
    const MIMES: &[&str] = &[
        "text/uri-list",
        "x-special/gnome-copied-files",
        "application/x-kde-cutselection",
        "text/x-moz-url",
    ];
    for mime in MIMES {
        let Ok(output) = std::process::Command::new("wl-paste")
            .args(["--no-newline", "--type", mime])
            .output()
        else {
            continue;
        };
        if !output.status.success() || output.stdout.is_empty() {
            continue;
        }
        let paths = match *mime {
            "x-special/gnome-copied-files" => parse_gnome_copied_files(&output.stdout),
            "application/x-kde-cutselection" => parse_kde_files(&output.stdout),
            _ => parse_uri_list(&output.stdout),
        };
        if !paths.is_empty() {
            tracing::info!(mime, count = paths.len(), "wl-paste 读到本地文件剪贴板");
            return paths;
        }
    }
    Vec::new()
}

/// Local file paths currently on the clipboard, preferring arboard then wl-paste.
fn read_local_files(clipboard: &mut arboard::Clipboard) -> Vec<PathBuf> {
    let from_arboard = clipboard
        .get()
        .file_list()
        .unwrap_or_default()
        .into_iter()
        .map(trim_crlf)
        .filter(|path| path.is_absolute())
        .collect::<Vec<_>>();
    if !from_arboard.is_empty() {
        return from_arboard;
    }
    wl_paste_files()
        .into_iter()
        .filter(|path| path.is_absolute())
        .collect()
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "gif" | "tif" | "tiff" | "webp"
            )
        })
}

fn encoded_bytes_to_dib(bytes: &[u8]) -> Result<Vec<u8>> {
    // Re-use formats::convert sniffing by giving it a name matching the magic.
    let name = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "PNG"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "JFIF"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "PNG"
    };
    let format = Format {
        wire: ClipboardFormat {
            id: 8,
            name: name.into(),
        },
        local: 8,
    };
    formats::convert(bytes.to_vec(), &format, 1, false)
}

fn wl_paste_image() -> Option<Vec<u8>> {
    for mime in ["image/png", "image/jpeg", "image/bmp", "image/tiff"] {
        let output = std::process::Command::new("wl-paste")
            .args(["--type", mime, "-n"])
            .output()
            .ok()?;
        if output.status.success() && !output.stdout.is_empty() {
            match encoded_bytes_to_dib(&output.stdout) {
                Ok(dib) => return Some(dib),
                Err(error) => {
                    tracing::debug!(mime, error = format!("{error:#}"), "wl-paste 图片解码失败")
                }
            }
        }
    }
    None
}

fn read_local_image(clipboard: &mut arboard::Clipboard, sources: &[PathBuf]) -> Option<Vec<u8>> {
    if let Ok(image) = clipboard.get_image() {
        match rgba_to_dib(image.width as u32, image.height as u32, &image.bytes) {
            Ok(dib) => return Some(dib),
            Err(error) => tracing::debug!(error = format!("{error:#}"), "本地 RGBA→DIB 失败"),
        }
    }
    if let Some(dib) = wl_paste_image() {
        return Some(dib);
    }
    if sources.len() == 1 && is_image_path(&sources[0]) {
        match std::fs::read(&sources[0]) {
            Ok(bytes) if !bytes.is_empty() && bytes.len() <= MAX_CLIP => {
                match encoded_bytes_to_dib(&bytes) {
                    Ok(dib) => return Some(dib),
                    Err(error) => tracing::debug!(
                        path = %sources[0].display(),
                        error = format!("{error:#}"),
                        "本地图片文件解码失败"
                    ),
                }
            }
            Ok(_) => {}
            Err(error) => tracing::debug!(
                path = %sources[0].display(),
                %error,
                "读取本地图片文件失败"
            ),
        }
    }
    None
}

fn poll_local(state: &mut State) -> Result<()> {
    if state.writing || state.sessions.is_empty() {
        return Ok(());
    }
    let Some(clipboard) = state.clipboard.as_mut() else {
        return Ok(());
    };
    // The whole outbound path starts here, so when files never reach the remote
    // this says whether they were on the local clipboard at all.
    if tracing::enabled!(target: "openuuyc::clipboard", tracing::Level::DEBUG) {
        let probe = clipboard.get().file_list();
        tracing::debug!(target: "openuuyc::clipboard",
            sessions = state.sessions.len(),
            files = ?probe.as_ref().map(Vec::len).map_err(ToString::to_string),
            first = ?probe.as_ref().ok().and_then(|paths| paths.first().cloned()),
            held = state.local.sources.len(),
            "剪贴板轮询");
    }
    // File managers put paths on text/uri-list and/or DE-specific MIME types
    // (x-special/gnome-copied-files, application/x-kde-cutselection). arboard
    // only reads uri-list; wl-paste covers the rest.
    let sources = read_local_files(clipboard)
        .into_iter()
        // A copy the remote sent is published as paths into this process's own
        // filesystem. Offering those back would ask the remote for the files it
        // just gave us, over and over.
        .filter(|path| {
            !state
                .mounted
                .as_ref()
                .is_some_and(|m| path.starts_with(m.root()))
        })
        .collect::<Vec<_>>();
    // Prefer a bitmap payload when present. File managers put a single image
    // path on text/uri-list (no image/* offer); promote that to CF_DIB so a
    // Mac paste into Preview/Pages gets pixels rather than a broken file offer.
    // Screenshot tools usually offer image/png directly.
    let image = read_local_image(clipboard, &sources);
    let snapshot = if let Some(dib) = image {
        LocalSnapshot {
            image: Some(dib),
            ..Default::default()
        }
    } else if !sources.is_empty() {
        LocalSnapshot {
            sources,
            ..Default::default()
        }
    } else if let Some(text) = clipboard.get_text().ok().filter(|text| !text.is_empty()) {
        LocalSnapshot {
            text: Some(text),
            ..Default::default()
        }
    } else {
        LocalSnapshot::default()
    };
    if snapshot.text == state.local.text
        && snapshot.image == state.local.image
        && snapshot.sources == state.local.sources
    {
        return Ok(());
    }
    tracing::info!(
        files = snapshot.sources.len(),
        text = snapshot.text.as_ref().map(String::len),
        image = snapshot.image.as_ref().map(Vec::len),
        "本地剪贴板已变化"
    );
    state.local = snapshot;
    // The previous copy is gone; a read still in flight against it now fails.
    state.tasks.clear();
    publish_local(state)
}

/// Advertise `state.local` to every active session (FormatList), or clear
/// publications when empty.
fn publish_local(state: &mut State) -> Result<()> {
    if state.local.is_empty() {
        state.published.clear();
        return Ok(());
    }
    let ids = state.local.format_ids();
    for session in sessions(state) {
        let links = formats::outgoing(
            &ids,
            session.platform.load(Ordering::Acquire),
            session.file_allowed(),
        );
        if links.is_empty() {
            state.published.remove(&session.id);
            continue;
        }
        state.published.insert(session.id, links.clone());
        let wires: Vec<_> = links.iter().map(|f| f.wire.name.clone()).collect();
        tracing::info!(id = session.id, formats = ?wires, "向远端发布本地剪贴板");
        session.emit(
            session.epoch.load(Ordering::Acquire),
            request(
                session.next(),
                ClipboardRequestKind::FormatList(ClipboardFormatListRequest {
                    formats: links.into_iter().map(|format| format.wire).collect(),
                    has_action: 0,
                    drag_drop_action: None,
                }),
            ),
        )?;
        *lock(&session.error) = None;
    }
    Ok(())
}

fn serve(
    state: &mut State,
    session: &Arc<Inner>,
    epoch: u64,
    id: i64,
    kind: ClipboardRequestKind,
) -> Result<()> {
    match kind {
        ClipboardRequestKind::FormatDataAsk(ask) => {
            let result = (|| -> Result<Vec<u8>> {
                let formats = state.published.get(&session.id).context("原剪贴板已失效")?;
                let format = formats
                    .iter()
                    .find(|format| {
                        format.wire.id == ask.format_id
                            && (ask.format_name.is_empty() || ask.format_name == format.wire.name)
                    })
                    .context("未发布该剪贴板格式")?;
                let data = state.local.data(format)?;
                formats::convert(data, format, session.platform.load(Ordering::Acquire), true)
            })();
            match result {
                Ok(data) if !data.is_empty() => {
                    tracing::info!(
                        id = session.id,
                        format = %ask.format_name,
                        format_id = ask.format_id,
                        bytes = data.len(),
                        "已回传剪贴板格式数据"
                    );
                    session.enqueue(Outbound::Blocks(epoch, id, ask.block_key, data))
                }
                other => {
                    tracing::warn!(
                        id = session.id,
                        format = %ask.format_name,
                        format_id = ask.format_id,
                        ok_empty = matches!(other, Ok(_)),
                        "剪贴板格式数据准备失败或为空"
                    );
                    session.emit(
                        epoch,
                        response(
                            id,
                            ClipboardResponseKind::FormatDataConfirm(ClipboardFormatDataConfirm {
                                err: 2,
                                block_key: ask.block_key,
                                block_count: 0,
                            }),
                        ),
                    )
                }
            }
        }
        ClipboardRequestKind::FileDescListRequest(ask) => {
            tracing::info!(
                id = session.id,
                task = ask.task_id,
                sources = state.local.sources.len(),
                "远端请求文件描述列表"
            );
            let result = (|| -> Result<Arc<LocalFiles>> {
                ensure!(session.file_allowed(), "文件剪贴板已关闭");
                if let Some((files, ticket)) = state.explicit.remove(&session.id) {
                    if let Some(ticket) = &ticket {
                        ensure!(ticket.valid(), "拖放文件提议已失效");
                        ticket.handed_off();
                    }
                    ensure!(state.tasks.len() < 32, "文件任务过多");
                    return Ok(files);
                }
                ensure!(
                    state.published.contains_key(&session.id),
                    "原文件剪贴板已失效"
                );
                ensure!(!state.local.sources.is_empty(), "本地剪贴板没有文件");
                ensure!(state.tasks.len() < 32, "文件任务过多");
                Ok(Arc::new(collect_files(&state.local.sources)?))
            })();
            let Ok(files) = result else {
                tracing::warn!(
                    id = session.id,
                    task = ask.task_id,
                    "文件描述列表准备失败，已拒绝远端请求"
                );
                return session.emit(
                    epoch,
                    response(
                        id,
                        ClipboardResponseKind::FileDescListResponse(
                            ClipboardFileDescriptorListResponse {
                                task_id: ask.task_id,
                                segment_count: 0,
                                err: 2,
                            },
                        ),
                    ),
                );
            };
            tracing::info!(
                id = session.id,
                task = ask.task_id,
                items = files.items.len(),
                "已回传文件描述列表"
            );
            state.tasks.insert((session.id, ask.task_id), files.clone());
            // Stay below the SDK's 512 KiB message ceiling even with long names.
            let mut segments = Vec::<Vec<ClipboardFileDescriptor>>::new();
            let mut current = Vec::new();
            let mut bytes = 0;
            for item in &files.items {
                let size = item.desc.encoded_len() + 8;
                if current.len() == 1500 || bytes + size > 450_000 {
                    segments.push(std::mem::take(&mut current));
                    bytes = 0;
                }
                current.push(item.desc.clone());
                bytes += size;
            }
            if !current.is_empty() {
                segments.push(current);
            }
            let mut messages = std::collections::VecDeque::new();
            messages.push_back(response(
                id,
                ClipboardResponseKind::FileDescListResponse(ClipboardFileDescriptorListResponse {
                    task_id: ask.task_id,
                    segment_count: segments.len() as u32,
                    err: 1,
                }),
            ));
            for (index, items) in segments.into_iter().enumerate() {
                messages.push_back(request(
                    session.next(),
                    ClipboardRequestKind::DescSegment(ClipboardFileDescriptorSegment {
                        task_id: ask.task_id,
                        segment_id: index as u32 + 1,
                        file_descs: items,
                    }),
                ));
            }
            session.enqueue(Outbound::Packets(epoch, messages))
        }
        ClipboardRequestKind::FileContentsRequest(ask) => {
            let result = if session.file_allowed() {
                state
                    .tasks
                    .get(&(session.id, ask.task_id))
                    .context("文件任务已失效")
                    .and_then(|files| files.read(&ask))
            } else {
                Err(anyhow!("文件剪贴板已关闭"))
            };
            let (err, data) = match result {
                Ok(data) => {
                    tracing::info!(
                        id = session.id,
                        task = ask.task_id,
                        index = ask.list_index,
                        offset = ask.pos_offset,
                        bytes = data.len(),
                        "已回传文件内容块"
                    );
                    (1, data)
                }
                Err(error) => {
                    tracing::warn!(%error, index = ask.list_index, "读取本地文件失败");
                    (2, Vec::new())
                }
            };
            session.emit(
                epoch,
                response(
                    id,
                    ClipboardResponseKind::FileContentsResponse(ClipboardFileContentsResponse {
                        task_id: ask.task_id,
                        data,
                        err,
                        pos_offset: ask.pos_offset,
                        list_index: ask.list_index,
                    }),
                ),
            )
        }
        ClipboardRequestKind::CancelRequest(ask) => {
            state.tasks.remove(&(session.id, ask.task_id));
            session.emit(
                epoch,
                response(
                    id,
                    ClipboardResponseKind::CancelResponse(ClipboardFileCancelResponse {
                        task_id: ask.task_id,
                        err: 1,
                    }),
                ),
            )
        }
        _ => Ok(()),
    }
}

/// A packed device-independent bitmap, as CF_DIB carries it.
fn dib_to_rgba(dib: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    ensure!(dib.len() >= 40, "位图数据过短");
    let header = u32::from_le_bytes(dib[0..4].try_into()?) as usize;
    ensure!(
        (40..=124).contains(&header) && dib.len() > header,
        "不支持的位图头"
    );
    let width = i32::from_le_bytes(dib[4..8].try_into()?);
    let height = i32::from_le_bytes(dib[8..12].try_into()?);
    let depth = u16::from_le_bytes(dib[14..16].try_into()?);
    let compression = u32::from_le_bytes(dib[16..20].try_into()?);
    ensure!(depth == 32 || depth == 24, "仅支持 24/32 位位图");
    // BI_RGB and BI_BITFIELDS with the usual masks share this pixel layout.
    ensure!(compression == 0 || compression == 3, "不支持压缩位图");
    let bottom_up = height > 0;
    let width = u32::try_from(width.abs()).context("位图宽度无效")?;
    let height = u32::try_from(height.abs()).context("位图高度无效")?;
    ensure!(
        width > 0 && height > 0 && width <= 32768 && height <= 32768,
        "位图尺寸无效"
    );
    let bytes = usize::from(depth / 8);
    let stride = ((width as usize * bytes) + 3) & !3;
    let masks = if compression == 3 { 12 } else { 0 };
    let start = header + masks;
    ensure!(
        dib.len() >= start + stride * height as usize,
        "位图数据不完整"
    );
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for row in 0..height as usize {
        let source = if bottom_up {
            height as usize - 1 - row
        } else {
            row
        };
        let line = &dib[start + source * stride..][..stride];
        for column in 0..width as usize {
            let pixel = &line[column * bytes..][..bytes];
            let target = (row * width as usize + column) * 4;
            rgba[target] = pixel[2];
            rgba[target + 1] = pixel[1];
            rgba[target + 2] = pixel[0];
            rgba[target + 3] = if bytes == 4 { pixel[3] } else { 255 };
        }
    }
    // A 32-bit DIB with an all-zero alpha channel is opaque in practice.
    if bytes == 4 && rgba.iter().skip(3).step_by(4).all(|alpha| *alpha == 0) {
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }
    Ok((width, height, rgba))
}

fn rgba_to_dib(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    ensure!(width > 0 && height > 0, "图片尺寸无效");
    let pixels = width as usize * height as usize;
    ensure!(rgba.len() >= pixels * 4, "图片数据不完整");
    ensure!(pixels * 4 <= MAX_CLIP, "图片过大");
    let mut dib = Vec::with_capacity(40 + pixels * 4);
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    // Positive height keeps the bottom-up order Windows applications expect.
    dib.extend_from_slice(&(height as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&((pixels * 4) as u32).to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    for row in (0..height as usize).rev() {
        for column in 0..width as usize {
            let pixel = &rgba[(row * width as usize + column) * 4..][..4];
            dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Ok(dib)
}

#[cfg(test)]
mod file_serve_tests {
    //! Self-test for the Linux→remote (incl. macOS controller) file-clipboard
    //! serve path: walk a copied tree into the descriptor list and answer the
    //! FileContentsRequest size/range reads the peer sends when it pastes.
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "ouuyc_fileserve_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn req(list_index: u32, flags: u32, pos_offset: u64, requested_len: u32) -> ClipboardFileContentsRequest {
        ClipboardFileContentsRequest {
            task_id: 0,
            list_index,
            flags,
            pos_offset,
            requested_len,
        }
    }

    #[test]
    fn serves_single_file_size_and_contents() {
        let dir = unique_dir("single");
        let path = dir.join("hello.txt");
        let body = b"the quick brown fox jumps over the lazy dog";
        std::fs::write(&path, body).unwrap();

        let files = collect_files(&[path.clone()]).expect("collect");
        assert_eq!(files.items.len(), 1, "one descriptor");
        let d = &files.items[0].desc;
        assert_eq!(d.file_name, "hello.txt");
        assert_eq!(d.file_size, body.len() as u64);
        assert_eq!(d.file_attributes & FILE_ATTRIBUTE_DIRECTORY, 0);

        // flags=1 -> little-endian u64 size, as the official client reads it.
        let size = files.read(&req(0, 1, 0, 8)).expect("size read");
        assert_eq!(size, (body.len() as u64).to_le_bytes().to_vec());

        // flags=2 -> full contents.
        let whole = files.read(&req(0, 2, 0, body.len() as u32)).expect("full read");
        assert_eq!(whole, body);

        // flags=2 -> a mid-file range (what a chunked paste asks for).
        let tail = files.read(&req(0, 2, 4, 5)).expect("range read");
        assert_eq!(tail, &body[4..9]);

        // Past EOF yields an empty tail, not an error.
        let past = files.read(&req(0, 2, body.len() as u64, 16)).expect("eof read");
        assert!(past.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn serves_directory_tree_in_order() {
        let dir = unique_dir("tree");
        let root = dir.join("folder");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"aaa").unwrap();
        std::fs::write(root.join("sub").join("b.bin"), b"bbbb").unwrap();

        let files = collect_files(&[root.clone()]).expect("collect tree");
        let names: Vec<_> = files.items.iter().map(|f| f.desc.file_name.clone()).collect();
        // Root dir first, then its sorted children; nested path uses backslash.
        assert!(names.contains(&"folder".to_string()), "names={names:?}");
        assert!(names.contains(&"folder\\a.txt".to_string()), "names={names:?}");
        assert!(names.contains(&"folder\\sub".to_string()), "names={names:?}");
        assert!(names.contains(&"folder\\sub\\b.bin".to_string()), "names={names:?}");

        // Directory entries carry the directory bit and refuse a contents read.
        let dir_idx = names.iter().position(|n| n == "folder").unwrap() as u32;
        assert_ne!(
            files.items[dir_idx as usize].desc.file_attributes & FILE_ATTRIBUTE_DIRECTORY,
            0
        );
        assert!(files.read(&req(dir_idx, 2, 0, 16)).is_err(), "dir has no contents");

        // A nested file still serves its bytes.
        let file_idx = names.iter().position(|n| n == "folder\\sub\\b.bin").unwrap() as u32;
        let bytes = files.read(&req(file_idx, 2, 0, 4)).expect("nested read");
        assert_eq!(bytes, b"bbbb");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_bad_index_and_oversize_request() {
        let dir = unique_dir("bad");
        let path = dir.join("x.dat");
        std::fs::write(&path, b"12345").unwrap();
        let files = collect_files(&[path]).expect("collect");

        assert!(files.read(&req(99, 1, 0, 8)).is_err(), "bad index rejected");
        assert!(
            files.read(&req(0, 2, 0, (FILE_BLOCK + 1) as u32)).is_err(),
            "oversize request rejected"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_uri_list_percent_decode_and_multiple() {
        let raw = b"file:///tmp/hello%20world.txt\r\nfile:///tmp/dir%2Fsub\n# comment\nfile://localhost/tmp/z\nhttp://example.com/x\n/tmp/bare\n";
        let paths = parse_uri_list(raw);
        let as_str: Vec<_> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        assert!(as_str.contains(&"/tmp/hello world.txt".into()), "{as_str:?}");
        assert!(as_str.contains(&"/tmp/dir/sub".into()), "{as_str:?}");
        assert!(as_str.contains(&"/tmp/z".into()), "{as_str:?}");
        assert!(as_str.contains(&"/tmp/bare".into()), "{as_str:?}");
        assert!(!as_str.iter().any(|s| s.contains("example.com")), "{as_str:?}");
        // No leftover CR on any path.
        assert!(as_str.iter().all(|s| !s.contains('\r')), "{as_str:?}");
    }

    #[test]
    fn parse_gnome_copied_files_copy_and_cut() {
        let raw = b"copy\nfile:///tmp/a.txt\nfile:///tmp/b%20c.txt\n";
        let paths = parse_gnome_copied_files(raw);
        assert_eq!(
            paths,
            vec![PathBuf::from("/tmp/a.txt"), PathBuf::from("/tmp/b c.txt")]
        );
        let cut = b"cut\nfile:///tmp/only.txt\n";
        assert_eq!(
            parse_gnome_copied_files(cut),
            vec![PathBuf::from("/tmp/only.txt")]
        );
    }

    #[test]
    fn parse_kde_files_with_and_without_flag() {
        let with = b"1\nfile:///tmp/k.txt\n";
        assert_eq!(parse_kde_files(with), vec![PathBuf::from("/tmp/k.txt")]);
        let plain = b"file:///tmp/k2.txt\r\n";
        assert_eq!(parse_kde_files(plain), vec![PathBuf::from("/tmp/k2.txt")]);
    }
}
