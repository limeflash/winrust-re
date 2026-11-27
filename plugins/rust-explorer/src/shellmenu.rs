//! The bar's menu for files, apps and folders, rust-explorer's side (the protocol and its reasons are in
//! `winrust_common::filemenu`). A right click builds the shell's own `IContextMenu` for the selection, reads its items, and offers
//! them to the bar ([`offer`]); the bar draws the menu and sends back what was chosen ([`act`]). The menu stays alive here
//! while the bar shows it: Send to, Open with and the submenus of other programs are filled only when they are opened, and what
//! is chosen runs here, as the desktop user and with a window of ours as its owner.
//!
//! The shell's menu is never touched on the desktop's UI thread: a handler of Windows or of a program may wait for a process that
//! is not there (Pin to taskbar talks to explorer.exe, which does not run beside this shell) and never come back. Each menu lives
//! on a worker of its own (an STA thread with a message loop: the `IContextMenu`, its `HMENU`, the submenu fills and
//! `InvokeCommand` all run there; the UI thread talks to it through channels and events). The UI thread waits for the built list
//! for [`BUDGET`], pumping messages, and not a moment longer; if Windows has not answered, the bar gets our own rows at once
//! ([`own_items`]: Open, Run as administrator, Cut, Copy, Delete, Properties; the bar adds Copy as path, Rename, Open file
//! location and the pins itself) and "Show more options" waits for the worker in the background ([`MORE_SUB`]). A worker that
//! never returns is given up (its thread leaks, nothing else), and what was slow is remembered by kind for the session.
//!
//! `false` from [`offer`] (no bar, or one that does not answer) leaves the caller to show the old menu.
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap},
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering::Relaxed},
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT, WAIT_EVENT, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM},
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
            Threading::{CreateEventW, SetEvent},
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};
use winrust_common::filemenu::{self, Act, MORE_SUB, Request, ShellItem, SubReply, fl, menu, rq};

/// How long the UI thread waits for the shell's list before it shows our own rows.
const BUDGET: Duration = Duration::from_millis(700);

/// The command numbers of our own items (the shell's are 1..0x7FFF).
mod syn {
    pub const OPEN: u32 = 0x8001;
    pub const RUNAS: u32 = 0x8002;
    pub const PROPS: u32 = 0x8003;
    pub const DELETE: u32 = 0x8004;
    pub const CUT: u32 = 0x8005;
    pub const COPY: u32 = 0x8006;
}

/// The timer that drains the worker's answers, and how often (the worker reads a filling submenu again as often, for [`READS`]
/// reads: about 0.7 s, Send to is whole in a few hundred ms).
pub(crate) const TIMER: usize = 0x5746;
const EVERY: u32 = 120;
const READS: u8 = 6;

/// Whether to tell the bar about a read of `now` items, given the count it was last told (`None`: nothing yet). An empty
/// read is not worth telling while the submenu may still fill; on the last read it ends the bar's waiting.
fn worth_telling(told: Option<usize>, now: usize, last: bool) -> bool {
    (now > 0 && told != Some(now)) || (last && told.is_none())
}

// ---------------------------------------------------------------------------------------------------------------------
// events and waiting

/// A Win32 auto-reset event: it wakes a thread that waits for messages as well, which a channel cannot.
struct Signal(HANDLE);

// SAFETY: an event handle may be set and waited on from any thread.
unsafe impl Send for Signal {}
unsafe impl Sync for Signal {}

impl Signal {
    fn new() -> Self {
        // SAFETY: an unnamed event; a failure leaves a null handle, which `wait` copes with by polling.
        Self(unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.unwrap_or_default())
    }

    fn set(&self) {
        // SAFETY: our own event.
        if !self.0.is_invalid() { unsafe { let _ = SetEvent(self.0); } }
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        // SAFETY: our own event, closed once.
        if !self.0.is_invalid() { unsafe { let _ = CloseHandle(self.0); } }
    }
}

/// Sends this thread's pending messages on their way. `true` = a WM_QUIT was met (it is posted again for the loop above).
fn pump() -> bool {
    let mut m = MSG::default();
    // SAFETY: the standard message pump of the calling thread.
    unsafe {
        while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
            if m.message == WM_QUIT {
                PostQuitMessage(m.wParam.0 as i32);
                return true;
            }
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
    false
}

/// Waits for `sig` or `ms` (u32::MAX: for ever), sending the thread's messages on their way meanwhile. `true` = the signal was set.
fn wait(sig: &Signal, ms: u32) -> bool {
    if sig.0.is_invalid() {
        std::thread::sleep(Duration::from_millis(u64::from(ms.min(10))));
        pump();
        return false;
    }
    // SAFETY: waits on our own event and the thread's input.
    let r = unsafe { MsgWaitForMultipleObjects(Some(&[sig.0]), false, ms, QS_ALLINPUT) };
    if r == WAIT_OBJECT_0 {
        true
    } else if r == WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
        pump();
        false
    } else {
        if r != WAIT_TIMEOUT { std::thread::sleep(Duration::from_millis(5)); }
        false
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the worker: everything that touches the shell's menu

/// What lives on the worker: the shell's menu and what is done with it. Made on the worker, never leaves it.
trait Backend {
    /// The items of the submenu with this handle, now that it is being opened (`None`: not worth telling yet).
    fn fill(&mut self, handle: u32) -> Option<Vec<ShellItem>>;
    /// One more read of every submenu being watched: what changed since the bar was told.
    fn reread(&mut self) -> Vec<(u32, Vec<ShellItem>)>;
    fn watching(&self) -> bool;
    /// Runs item `id` of the shell's menu.
    fn run(&mut self, h: HWND, id: u32);
}

/// The shell's menu for one selection: the `IContextMenu`, its popup menu, the submenus handed out as lazy handles.
struct Session {
    cm: IContextMenu,
    cm2: Option<IContextMenu2>,
    menu: HMENU,
    /// The submenus handed out as [`filemenu::Sub::Lazy`], by handle - 1: the menu and its position in its parent.
    subs: Vec<(*mut c_void, u32)>,
    /// Submenus the bar opened that Windows may still be filling (Send to fills from a worker): read again for a while.
    watch: Vec<Watch>,
    /// What the menu is for: the verbs that are ours to run ([`special`]) need the files.
    paths: Vec<PathBuf>,
}

/// A submenu being read again: what the bar was last told about it and how many times it has been read since.
struct Watch {
    handle: u32,
    told: Option<usize>,
    ticks: u8,
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the menu is ours; the context menu objects go with the struct.
        unsafe { let _ = DestroyMenu(self.menu); }
    }
}

/// What reads a menu: the verb of a command, and the "about to open" message of a submenu.
struct Src<'a> {
    cm: &'a IContextMenu,
    cm2: Option<&'a IContextMenu2>,
    subs: &'a mut Vec<(*mut c_void, u32)>,
}

impl menu::Source for Src<'_> {
    fn verb(&mut self, id: u32) -> String {
        let at = id.saturating_sub(1) as usize;
        let mut buf = [0u16; 128];
        // SAFETY: the buffer is as long as the call is told; a command without a verb answers an error.
        let ok = unsafe { self.cm.GetCommandString(at, GCS_VERBW, None, PSTR(buf.as_mut_ptr().cast()), buf.len() as u32) };
        if ok.is_ok() {
            return String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
        }
        // some handlers answer only the ANSI question
        let mut a = [0u8; 128];
        // SAFETY: as above.
        let ok = unsafe { self.cm.GetCommandString(at, GCS_VERBA, None, PSTR(a.as_mut_ptr()), a.len() as u32) };
        if ok.is_err() { return String::new(); }
        String::from_utf8_lossy(&a[..a.iter().position(|&c| c == 0).unwrap_or(a.len())]).into_owned()
    }
    fn opening(&mut self, m: *mut c_void, at: u32) {
        // SAFETY: WM_INITMENUPOPUP (the submenu, its position) is how the shell is told a submenu is about to be shown.
        if let Some(c2) = self.cm2 { unsafe { let _ = c2.HandleMenuMsg(WM_INITMENUPOPUP, WPARAM(m as usize), LPARAM(at as isize)); } }
    }
    fn keep(&mut self, m: *mut c_void, at: u32) -> u32 {
        self.subs.push((m, at));
        self.subs.len() as u32
    }
}

impl Session {
    fn src(&mut self) -> (Src<'_>, HMENU) {
        (Src { cm: &self.cm, cm2: self.cm2.as_ref(), subs: &mut self.subs }, self.menu)
    }

    fn tidy(mut items: Vec<ShellItem>) -> Vec<ShellItem> {
        // our own "Compress to ZIP" is the one: Windows' entry in Send to is left out, as in the old menu (other zip tools stay)
        if let Some(zip) = crate::sh::system_zip_name() { items.retain(|i| !i.label.eq_ignore_ascii_case(&zip)); }
        items
    }
}

impl Backend for Session {
    fn fill(&mut self, handle: u32) -> Option<Vec<ShellItem>> {
        let &(m, at) = handle.checked_sub(1).and_then(|i| self.subs.get(i as usize))?;
        let (mut src, _) = self.src();
        // SAFETY: `m` is a submenu of the menu this session owns.
        let items = Self::tidy(unsafe { menu::fill(m, at, &mut src) });
        let tell = worth_telling(None, items.len(), false);
        self.watch.retain(|w| w.handle != handle);
        self.watch.push(Watch { handle, told: tell.then_some(items.len()), ticks: 0 });
        tell.then_some(items)
    }

    fn reread(&mut self) -> Vec<(u32, Vec<ShellItem>)> {
        let mut out = Vec::new();
        for i in 0..self.watch.len() {
            let (handle, told, ticks) = (self.watch[i].handle, self.watch[i].told, self.watch[i].ticks + 1);
            self.watch[i].ticks = ticks;
            let Some(&(m, _)) = handle.checked_sub(1).and_then(|h| self.subs.get(h as usize)) else { continue };
            let (mut src, _) = self.src();
            // SAFETY: as in `fill`; the shell has been told already, this only reads what it has put there since.
            let items = Self::tidy(unsafe { menu::read(m, &mut src) });
            if worth_telling(told, items.len(), ticks >= READS) {
                self.watch[i].told = Some(items.len());
                out.push((handle, items));
            }
        }
        self.watch.retain(|w| w.ticks < READS);
        out
    }

    fn watching(&self) -> bool {
        !self.watch.is_empty()
    }

    fn run(&mut self, h: HWND, id: u32) {
        // some verbs Windows runs through explorer.exe, which does not run beside this shell: they are done here
        let verb = {
            let (mut src, _) = self.src();
            menu::Source::verb(&mut src, id)
        };
        if special(&verb, &self.paths, &mut set_wallpaper) {
            return;
        }
        let mut pt = POINT::default();
        // SAFETY: plain calls; the verb is the command's offset, which is how the shell numbers its items.
        unsafe {
            let _ = GetCursorPos(&mut pt);
            // ptInvoke: without it dialogs the shell opens ("Open with" > choose another app) land off screen
            let ci = CMINVOKECOMMANDINFOEX {
                cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                fMask: CMIC_MASK_PTINVOKE,
                hwnd: h,
                lpVerb: PCSTR(id.saturating_sub(1) as usize as *const u8),
                nShow: SW_SHOWNORMAL.0,
                ptInvoke: pt,
                ..Default::default()
            };
            let before = crate::sh::top_windows();
            let _ = self.cm.InvokeCommand(&ci as *const _ as *const CMINVOKECOMMANDINFO);
            crate::sh::keep_on_screen(before);
        }
    }
}

/// The commands of the shell that rust-explorer does itself, by verb (whichever row or submenu of the menu they come from): `true` =
/// done. "Set as desktop background" goes through `IDesktopWallpaper`, which lives in explorer.exe; the file's own path is what
/// Windows' older `SPI_SETDESKWALLPAPER` takes, and it is right whether or not explorer.exe runs. Only the first of several
/// files is used.
fn special(verb: &str, paths: &[PathBuf], set_wallpaper: &mut dyn FnMut(&Path)) -> bool {
    if verb.eq_ignore_ascii_case("setdesktopwallpaper") {
        if let Some(p) = paths.first() { set_wallpaper(p); }
        return true;
    }
    false
}

/// Makes `path` the desktop picture (the style stays as it is) and tells everybody: our own desktop reloads on that broadcast.
fn set_wallpaper(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a NUL-terminated path that outlives the call (SPI_SETDESKWALLPAPER only reads it).
    unsafe { let _ = SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(w.as_ptr() as *mut c_void), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE); }
}

/// The shell's menu for `paths` and its top level items. Runs on the worker.
fn build_session(h: HWND, paths: &[PathBuf]) -> Option<(Box<dyn Backend>, Vec<ShellItem>)> {
    let cm: IContextMenu = crate::sh::ui(h, paths).ok()?;
    // SAFETY: a fresh menu for the shell to fill, owned by the session.
    let menu_h = unsafe { CreatePopupMenu().ok()? };
    // SAFETY: as above; ids 1..0x7FFF are what the shell numbers its commands in.
    if unsafe { cm.QueryContextMenu(menu_h, 0, 1, 0x7FFF, CMF_NORMAL) }.is_err() {
        // SAFETY: the menu is ours.
        unsafe { let _ = DestroyMenu(menu_h); }
        return None;
    }
    let cm2 = cm.cast::<IContextMenu2>().ok();
    let mut s = Session { cm, cm2, menu: menu_h, subs: Vec::new(), watch: Vec::new(), paths: paths.to_vec() };
    let (mut src, m) = s.src();
    // SAFETY: the menu was just filled by the shell and is ours.
    let items = unsafe { menu::read(m.0, &mut src) };
    Some((Box::new(s), items))
}

enum Cmd {
    Fill(u32),
    /// Run item `id` of the shell's menu with the window (`isize`) as its owner, then tell that window (`u32`: the menu's
    /// session) that the folder may have changed.
    Run(isize, u32, u32),
    /// Let go of the menu; the worker ends.
    Drop,
}

enum Reply {
    /// The shell's list (`None`: it has no menu for this).
    Built(Option<Vec<ShellItem>>),
    Sub(u32, Vec<ShellItem>),
}

/// Workers that are still building their menu: a shell that does not answer must not pile threads up without end.
static UNBUILT: AtomicU32 = AtomicU32::new(0);
const MAX_UNBUILT: u32 = 4;

/// The UI thread's end of a worker.
struct Worker {
    cmd: Sender<Cmd>,
    cmd_sig: Arc<Signal>,
    rep: Receiver<Reply>,
    rep_sig: Arc<Signal>,
}

impl Worker {
    fn send(&self, c: Cmd) {
        let _ = self.cmd.send(c);
        self.cmd_sig.set();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // a worker that is busy (or stuck) in the shell takes it when it is back (or never)
        self.send(Cmd::Drop);
    }
}

/// Starts a worker that makes its backend with `build` (on the worker).
fn spawn<B>(build: B) -> Worker
where
    B: FnOnce() -> Option<(Box<dyn Backend>, Vec<ShellItem>)> + Send + 'static,
{
    let (cmd, crx) = channel::<Cmd>();
    let (rtx, rep) = channel::<Reply>();
    let (cmd_sig, rep_sig) = (Arc::new(Signal::new()), Arc::new(Signal::new()));
    let (cs, rs) = (Arc::clone(&cmd_sig), Arc::clone(&rep_sig));
    UNBUILT.fetch_add(1, Relaxed);
    let started = std::thread::Builder::new().name("shell-menu".into()).spawn(move || {
        // SAFETY: an apartment of its own for the shell's objects, which live and die on this thread.
        unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); }
        let built = build();
        UNBUILT.fetch_sub(1, Relaxed);
        let (backend, items) = match built {
            Some((b, i)) => (Some(b), Some(i)),
            None => (None, None),
        };
        let _ = rtx.send(Reply::Built(items));
        rs.set();
        if let Some(mut be) = backend {
            let every = Duration::from_millis(u64::from(EVERY));
            let mut next = Instant::now() + every;
            'life: loop {
                let ms = if be.watching() { next.saturating_duration_since(Instant::now()).as_millis() as u32 } else { u32::MAX };
                wait(&cs, ms);
                while let Ok(c) = crx.try_recv() {
                    match c {
                        Cmd::Fill(h) => {
                            if let Some(items) = be.fill(h) {
                                let _ = rtx.send(Reply::Sub(h, items));
                                rs.set();
                            }
                            next = Instant::now() + every;
                        }
                        Cmd::Run(hw, id, session) => {
                            be.run(HWND(hw as *mut c_void), id);
                            let (wp, lp) = Act::Item(0).encode(session);
                            // SAFETY: a plain message to the desktop's window: it reloads its list
                            unsafe { let _ = PostMessageW(Some(HWND(hw as *mut c_void)), filemenu::act_msg(), WPARAM(wp), LPARAM(lp)); }
                        }
                        Cmd::Drop => break 'life,
                    }
                }
                if be.watching() && Instant::now() >= next {
                    for (h, items) in be.reread() {
                        let _ = rtx.send(Reply::Sub(h, items));
                        rs.set();
                    }
                    next = Instant::now() + every;
                }
            }
            drop(be);
        }
        // SAFETY: balances the call above.
        unsafe { CoUninitialize() };
    });
    if started.is_err() {
        UNBUILT.fetch_sub(1, Relaxed);
    }
    Worker { cmd, cmd_sig, rep, rep_sig }
}

/// How the wait for the shell's list ended.
#[derive(Debug)]
enum Built {
    Ready(Vec<ShellItem>),
    /// The shell has no menu for this (or the worker is gone).
    Failed,
    /// Not in time.
    Slow,
}

/// Waits at most `budget` for the worker's list, sending the thread's messages on their way meanwhile.
fn await_built(w: &Worker, budget: Duration) -> Built {
    let end = Instant::now() + budget;
    loop {
        match w.rep.try_recv() {
            Ok(Reply::Built(Some(items))) => return Built::Ready(items),
            Ok(Reply::Built(None)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => return Built::Failed,
            Ok(Reply::Sub(..)) => continue,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Built::Slow;
        }
        wait(&w.rep_sig, left.as_millis().max(1) as u32);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// what was slow

/// The kinds of selection the shell was slow for: never answered (`None`), or answered late (`Some(when)`, forgiven after a while).
static SLOW: Mutex<Option<HashMap<String, Option<Instant>>>> = Mutex::new(None);
const FORGIVE: Duration = Duration::from_secs(120);

/// What the selection is, for remembering: the extensions of the files and "<dir>" for folders. (A shortcut is a `.lnk` whatever it
/// points at: the shell's handlers go by the shortcut's own kind first.)
fn kind_of(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| if p.is_dir() { "<dir>".to_string() } else { p.extension().map_or(String::new(), |e| format!(".{}", e.to_string_lossy().to_ascii_lowercase())) })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("+")
}

fn with_slow<T>(f: impl FnOnce(&mut HashMap<String, Option<Instant>>) -> T) -> T {
    let mut g = SLOW.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(g.get_or_insert_with(HashMap::new))
}

fn mark_slow(kind: &str) {
    with_slow(|m| _ = m.entry(kind.to_string()).or_insert(None));
}

/// The shell did answer, late: skipped for a while yet.
fn note_answered(kind: &str) {
    with_slow(|m| _ = m.insert(kind.to_string(), Some(Instant::now())));
}

fn known_slow(kind: &str) -> bool {
    with_slow(|m| match m.get(kind) {
        None => false,
        Some(None) => true,
        Some(Some(t)) if t.elapsed() < FORGIVE => true,
        Some(Some(_)) => {
            m.remove(kind);
            false
        }
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// our own rows

/// A program Windows can run as administrator.
fn runnable(p: &Path) -> bool {
    p.extension().is_some_and(|e| ["exe", "lnk", "bat", "cmd", "msi", "com"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The rows rust-explorer makes without asking the shell, by the verbs the bar maps onto the design's rows; the bar adds the ones
/// that need nothing from here (Copy as path, Rename, Open file location, the pins).
fn own_items(paths: &[PathBuf]) -> Vec<ShellItem> {
    let one = paths.len() == 1;
    let mut v = Vec::new();
    if one {
        let mut open = ShellItem::new(syn::OPEN, "open", "Open");
        open.flags = fl::DEFAULT;
        v.push(open);
        if runnable(&paths[0]) { v.push(ShellItem::new(syn::RUNAS, "runas", "Run as administrator")); }
    }
    v.push(ShellItem::new(syn::CUT, "cut", "Cut"));
    v.push(ShellItem::new(syn::COPY, "copy", "Copy"));
    v.push(ShellItem::new(syn::DELETE, "delete", "Delete"));
    if one { v.push(ShellItem::new(syn::PROPS, "properties", "Properties")); }
    v
}

/// The items and flags of the request for how the wait ended.
fn choose(built: Built, paths: &[PathBuf]) -> (Vec<ShellItem>, u32) {
    match built {
        Built::Ready(items) => (items, 0),
        Built::Failed | Built::Slow => (own_items(paths), rq::SLOW),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the menu the bar is showing now

/// Where the shell's list stands for a menu that was shown with our own rows.
enum Late {
    /// The shell's list was there in time: nothing is waited for.
    No,
    Waiting,
    Ready(Vec<ShellItem>),
    /// It has no list, or there is no worker for it.
    Failed,
}

struct Live {
    id: u32,
    dir: PathBuf,
    paths: Vec<PathBuf>,
    kind: String,
    worker: Option<Worker>,
    late: Late,
}

thread_local! {
    static SESSION: RefCell<Option<Live>> = const { RefCell::new(None) };
    static OFFERING: Cell<bool> = const { Cell::new(false) };
}
static NEXT: AtomicU32 = AtomicU32::new(1);

impl Live {
    /// What the worker has said since: (handle, items) for the bar.
    fn drain(&mut self) -> Vec<(u32, Vec<ShellItem>)> {
        let mut out = Vec::new();
        let Some(w) = &self.worker else { return out };
        while let Ok(r) = w.rep.try_recv() {
            match r {
                Reply::Built(items) => {
                    if !matches!(self.late, Late::Waiting) { continue; }
                    match items {
                        Some(items) => {
                            note_answered(&self.kind);
                            self.late = Late::Ready(items.clone());
                            out.push((MORE_SUB, items));
                        }
                        None => {
                            self.late = Late::Failed;
                            out.push((MORE_SUB, Vec::new()));
                        }
                    }
                }
                Reply::Sub(h, items) => out.push((h, items)),
            }
        }
        out
    }

    /// The bar opens submenu `n`: the items now (`None`: they come later, by [`Live::drain`]).
    fn fill(&mut self, n: u32) -> Option<Vec<ShellItem>> {
        if n == MORE_SUB {
            return match &self.late {
                Late::Ready(items) => Some(items.clone()),
                Late::Waiting => None,
                Late::No | Late::Failed => Some(Vec::new()),
            };
        }
        match &self.worker {
            Some(w) => {
                w.send(Cmd::Fill(n));
                None
            }
            None => Some(Vec::new()),
        }
    }
}

/// Asks the bar to show the menu for `paths` (all in `dir`) at the pointer, or at `key_at` when it was the keyboard that opened
/// it. `true` = the bar has it: everything else happens in [`act`]. Never waits for the shell longer than [`BUDGET`].
pub(crate) fn offer(hw: *mut c_void, dir: &Path, paths: &[PathBuf], key_at: Option<(i32, i32)>) -> bool {
    if paths.is_empty() || winrust_common::deskmenu::bar().is_null() { return false; }
    // a second right click while the first is being waited for (messages are pumped then) has nothing to add
    if OFFERING.replace(true) { return true; }
    let r = offer_now(hw, dir, paths, key_at);
    OFFERING.set(false);
    r
}

fn offer_now(hw: *mut c_void, dir: &Path, paths: &[PathBuf], key_at: Option<(i32, i32)>) -> bool {
    let kind = kind_of(paths);
    let skip = known_slow(&kind);
    let worker = if skip && UNBUILT.load(Relaxed) >= MAX_UNBUILT {
        None
    } else {
        let (h, ps) = (hw as isize, paths.to_vec());
        Some(spawn(move || build_session(HWND(h as *mut c_void), &ps)))
    };
    let built = match &worker {
        None => Built::Slow,
        Some(w) => await_built(w, if skip { Duration::ZERO } else { BUDGET }),
    };
    if matches!(built, Built::Slow) && !skip { mark_slow(&kind); }
    let late = match (&built, &worker) {
        (Built::Ready(_), _) => Late::No,
        (Built::Slow, Some(_)) => Late::Waiting,
        _ => Late::Failed,
    };
    let (items, mut flags) = choose(built, paths);
    let mut pt = POINT::default();
    // SAFETY: plain call.
    unsafe { let _ = GetCursorPos(&mut pt); }
    let at = key_at.unwrap_or((pt.x, pt.y));
    let one = paths.len() == 1;
    if crate::desk() { flags |= rq::DESKTOP; } else { flags |= rq::TAB; }
    if one { flags |= rq::CAN_RENAME; }
    if one && paths[0].is_dir() && crate::ui::pins().contains(&paths[0]) { flags |= rq::PINNED; }
    let id = NEXT.fetch_add(1, Relaxed);
    let req = Request { session: id, at, flags, paths: paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(), items };
    // kept before the bar is asked: its first message back can come while it is still answering
    SESSION.set(Some(Live { id, dir: dir.to_path_buf(), paths: paths.to_vec(), kind, worker, late }));
    // SAFETY: a timer of the window that is handling the click: it drains what the worker says
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(hw, TIMER, EVERY, None) };
    if filemenu::ask(hw, &req) { return true; }
    SESSION.with_borrow_mut(|c| if c.as_ref().is_some_and(|s| s.id == id) { *c = None; });
    false
}

/// Where the menu opens when the keyboard asked: the lower middle of the first selected row, in screen pixels (the mockup's
/// `x = middle, y = bottom - 10`).
pub(crate) fn row_at(list: *mut c_void) -> Option<(i32, i32)> {
    use windows_sys::Win32::{Foundation::RECT, Graphics::Gdi::ClientToScreen, UI::Controls::{LVM_GETITEMRECT, LVM_GETNEXTITEM, LVNI_SELECTED}};
    let i = crate::put(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
    if i < 0 { return None; }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 }; // LVIR_BOUNDS = 0 is asked for in `left`
    // SAFETY: the list view fills the rectangle; the point is converted with the list's own handle.
    unsafe {
        crate::put(list, LVM_GETITEMRECT, i as usize, &mut r as *mut RECT as isize);
        let mut pt = windows_sys::Win32::Foundation::POINT { x: (r.left + r.right) / 2, y: r.bottom - crate::ui::sc(10) };
        if ClientToScreen(list, &mut pt) == 0 { return None; }
        Some((pt.x, pt.y))
    }
}

fn set_clipboard_text(h: HWND, text: &str) {
    use windows::Win32::{Foundation::HANDLE, System::{DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData}, Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock}}};
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the standard way to put text on the clipboard: a moveable block the clipboard owns after SetClipboardData.
    unsafe {
        if OpenClipboard(Some(h)).is_err() { return; }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(mem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(mem);
                let _ = SetClipboardData(13, Some(HANDLE(mem.0))); // CF_UNICODETEXT
            }
        }
        let _ = CloseClipboard();
    }
}

/// "Copy as path": one quoted path a line, as Windows does.
fn copy_paths(h: HWND, paths: &[PathBuf]) {
    set_clipboard_text(h, &paths.iter().map(|p| format!("\"{}\"", p.display())).collect::<Vec<_>>().join("\r\n"));
}

/// [`copy_paths`] for the menu of our own that is shown when there is no bar.
pub(crate) fn copy_paths_text(h: *mut c_void, paths: &[PathBuf]) {
    copy_paths(HWND(h), paths);
}

fn terminal(dir: &Path) {
    use std::{os::windows::process::CommandExt, process::Command};
    // Windows Terminal when it is installed (its alias lives in WindowsApps), else PowerShell in a console of its own
    if Command::new("wt.exe").arg("-d").arg(dir).spawn().is_err() {
        let _ = Command::new("cmd.exe").args(["/c", "start", "powershell.exe", "-NoExit"]).current_dir(dir).creation_flags(0x0800_0000).spawn();
    }
}

/// The timer ([`TIMER`]): tells the bar what the worker has said since (the shell's list that came late, a submenu that filled).
pub(crate) fn tick(hw: *mut c_void) {
    let live = SESSION.with_borrow_mut(|c| c.as_mut().map(|s| (s.id, s.drain())));
    let Some((session, news)) = live else {
        // SAFETY: the timer set in `offer_now`.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(hw, TIMER) };
        return;
    };
    for (handle, items) in news { let _ = filemenu::send_sub(hw, &SubReply { session, handle, items }); }
}

/// One of our own rows was chosen: nothing here asks the shell for a menu.
fn own(hw: *mut c_void, s: &Live, id: u32) {
    match id {
        // SAFETY: the same call a double click makes.
        syn::OPEN => for p in &s.paths { unsafe { crate::activate(hw, p.clone(), p.is_dir()) } },
        syn::RUNAS => if let [p] = s.paths.as_slice() {
            let (verb, file, dir) = (crate::w("runas"), crate::w(&p.to_string_lossy()), p.parent().map(|d| crate::w(&d.to_string_lossy())));
            // SAFETY: NUL-terminated strings that outlive the call.
            unsafe { windows_sys::Win32::UI::Shell::ShellExecuteW(hw, verb.as_ptr(), file.as_ptr(), std::ptr::null(), dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()), windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL) };
        },
        syn::PROPS => if let [p] = s.paths.as_slice() { crate::properties(p) },
        syn::DELETE => {
            // the shell's own confirmation and undo, as the Delete key has them
            let _ = crate::sh::fileop(hw, crate::sh::Op::Delete, &s.paths, &s.dir);
            crate::reload(hw);
        }
        syn::CUT | syn::COPY => _ = crate::sh::clip_set(hw, &s.paths, id == syn::CUT),
        _ => {}
    }
}

/// What the bar chose. `lp` is the menu's session: an answer for a menu that has been replaced is ignored.
pub(crate) fn act(hw: *mut c_void, wp: usize, lp: isize) {
    let h = HWND(hw);
    let Some((a, session)) = Act::decode(wp, lp) else { return };
    // the worker has run a command: the folder may have changed (item 0 is never the bar's: a separator has no number)
    if a == Act::Item(0) {
        crate::reload(hw);
        return;
    }
    if let Act::Fill(n) = a {
        let reply = SESSION.with_borrow_mut(|c| c.as_mut().filter(|s| s.id == session).map(|s| s.fill(n)));
        match reply {
            // a menu that is gone: an empty answer ends the bar's spinner
            None => _ = filemenu::send_sub(hw, &SubReply { session, handle: n, items: Vec::new() }),
            Some(Some(items)) => _ = filemenu::send_sub(hw, &SubReply { session, handle: n, items }),
            Some(None) => {}
        }
        return;
    }
    // Everything else ends the menu: the session is taken out first, so a modal dialog's message loop (Delete asks) cannot meet it
    let Some(s) = SESSION.with_borrow_mut(|c| if c.as_ref().is_some_and(|s| s.id == session) { c.take() } else { None }) else { return };
    match a {
        Act::Item(id) if id >= syn::OPEN => own(hw, &s, id),
        Act::Item(id) => {
            // the shell's own: run on its worker (which tells the window to reload when it is done); the menu is let go after
            if let Some(w) = &s.worker { w.send(Cmd::Run(hw as isize, id, session)); }
        }
        Act::Rename => unsafe {
            let (list, ..) = crate::selected();
            let i = crate::put(list, windows_sys::Win32::UI::Controls::LVM_GETNEXTITEM, usize::MAX, windows_sys::Win32::UI::Controls::LVNI_SELECTED as isize);
            if i >= 0 {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(list);
                crate::put(list, windows_sys::Win32::UI::Controls::LVM_EDITLABELW, i as usize, 0);
            }
        },
        Act::Zip => {
            let base = if s.paths.len() == 1 { s.paths[0].file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { s.dir.file_name().map_or("Archive".into(), |n| n.to_string_lossy().into_owned()) };
            let dest = crate::jobs::free_name(&s.dir, &format!("{base}.zip"), false);
            crate::jobs::zip(hw as isize, s.paths.clone(), dest);
        }
        Act::Extract => _ = crate::jobs::extract(hw as isize, s.paths.clone(), s.dir.clone()),
        Act::CopyPath => copy_paths(h, &s.paths),
        Act::NewTab => {
            if let [p] = s.paths.as_slice() {
                if crate::desk() { let _ = std::process::Command::new(std::env::current_exe().unwrap_or_default()).arg(p).spawn(); } else { crate::tab_open(p.clone()); }
            }
        }
        Act::Terminal => {
            if let [p] = s.paths.as_slice() { terminal(p); }
        }
        Act::TogglePin => {
            if let [p] = s.paths.as_slice() { crate::ui::toggle_pin(p); }
        }
        Act::Reveal => {
            if let [p] = s.paths.as_slice() { winrust_common::reveal(&p.to_string_lossy()); }
        }
        Act::Done | Act::Fill(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_submenu_that_is_still_filling_is_told_when_it_has_something_and_when_it_changes() {
        assert!(!worth_telling(None, 0, false), "empty and may still fill: wait");
        assert!(worth_telling(None, 0, true), "empty on the last read: the bar stops waiting");
        assert!(worth_telling(None, 2, false));
        assert!(!worth_telling(Some(2), 2, false), "nothing new");
        assert!(worth_telling(Some(2), 5, false), "Send to grew");
        assert!(!worth_telling(Some(5), 5, true));
        assert!(!worth_telling(Some(5), 0, true), "an empty read after items is not news");
    }

    #[test]
    fn set_as_desktop_background_is_done_here_by_its_verb_and_nothing_else_is() {
        let mut set = Vec::new();
        let paths = [PathBuf::from(r"C:\pics.png"), PathBuf::from(r"C:\pics.png")];
        assert!(special("setdesktopwallpaper", &paths, &mut |p| set.push(p.to_path_buf())));
        assert_eq!(set, [PathBuf::from(r"C:\pics.png")], "the first image only");
        assert!(special("SetDesktopWallpaper", &paths, &mut |p| set.push(p.to_path_buf())), "by any case");
        assert_eq!(set.len(), 2);
        for other in ["open", "properties", "delete", "", "rotate90"] {
            assert!(!special(other, &paths, &mut |_| panic!("{other} must not set the wallpaper")), "{other} is the shell's");
        }
        assert!(special("setdesktopwallpaper", &[], &mut |_| panic!("no files, nothing to set")), "still ours: the shell would only fail");
    }

    /// A shell that answers every submenu with one item.
    struct Quiet;
    impl Backend for Quiet {
        fn fill(&mut self, h: u32) -> Option<Vec<ShellItem>> { Some(vec![ShellItem::new(h, "", "Item")]) }
        fn reread(&mut self) -> Vec<(u32, Vec<ShellItem>)> { Vec::new() }
        fn watching(&self) -> bool { false }
        fn run(&mut self, _: HWND, _: u32) {}
    }

    #[test]
    fn a_shell_that_never_answers_costs_the_ui_its_budget_and_not_a_moment_more_and_our_rows_are_shown() {
        // never returns, like QueryContextMenu in a handler that waits for explorer.exe
        let w = spawn(|| loop { std::thread::park(); });
        let t = Instant::now();
        let built = await_built(&w, Duration::from_millis(300));
        let took = t.elapsed();
        assert!(matches!(built, Built::Slow), "{built:?}");
        assert!(took >= Duration::from_millis(250) && took < Duration::from_millis(900), "{took:?}");
        let paths = [PathBuf::from(r"C:\Users\x\Desktop\Duet.lnk")];
        let (items, flags) = choose(built, &paths);
        assert!(flags & rq::SLOW != 0);
        let verbs: Vec<&str> = items.iter().map(|i| i.verb.as_str()).collect();
        assert_eq!(verbs, ["open", "runas", "cut", "copy", "delete", "properties"]);
        assert!(items.iter().all(|i| i.id >= syn::OPEN), "our numbers do not meet the shell's 1..0x7FFF");
        assert!(items[0].is_default());
        // several items: no Open, no Properties
        let several = own_items(&[PathBuf::from("a.txt"), PathBuf::from("b.txt")]);
        assert_eq!(several.iter().map(|i| i.verb.as_str()).collect::<Vec<_>>(), ["cut", "copy", "delete"]);
        // a text file cannot be run as administrator
        assert!(!own_items(&[PathBuf::from("a.txt")]).iter().any(|i| i.verb == "runas"));
        // the stuck worker is given up: dropping it neither waits nor panics
        drop(w);
    }

    #[test]
    fn a_shell_that_has_no_menu_is_not_waited_for_either() {
        let w = spawn(|| None);
        let t = Instant::now();
        assert!(matches!(await_built(&w, Duration::from_secs(5)), Built::Failed));
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_list_that_comes_late_is_handed_to_the_bar_as_the_more_submenu() {
        let w = spawn(|| {
            std::thread::sleep(Duration::from_millis(400));
            Some((Box::new(Quiet) as Box<dyn Backend>, vec![ShellItem::new(7, "open", "Open")]))
        });
        assert!(matches!(await_built(&w, Duration::from_millis(50)), Built::Slow));
        let mut live = Live { id: 1, dir: PathBuf::new(), paths: vec![], kind: ".test-late".into(), worker: Some(w), late: Late::Waiting };
        assert_eq!(live.fill(MORE_SUB), None, "not there yet: the bar spins");
        let end = Instant::now() + Duration::from_secs(5);
        let mut news = Vec::new();
        while news.is_empty() && Instant::now() < end {
            wait(&live.worker.as_ref().unwrap().rep_sig, 50);
            news = live.drain();
        }
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].0, MORE_SUB);
        assert_eq!(news[0].1[0].label, "Open");
        assert_eq!(live.fill(MORE_SUB).map(|v| v.len()), Some(1), "asked again, it is answered at once");
        // a submenu is filled by the worker and comes back through the same drain
        assert_eq!(live.fill(3), None);
        let mut got = Vec::new();
        while got.is_empty() && Instant::now() < end {
            wait(&live.worker.as_ref().unwrap().rep_sig, 50);
            got = live.drain();
        }
        assert_eq!(got[0].0, 3);
        with_slow(|m| _ = m.remove(".test-late"));
    }

    #[test]
    fn a_kind_the_shell_was_slow_for_is_remembered_and_one_that_answered_late_is_forgiven_after_a_while() {
        assert_eq!(kind_of(&[PathBuf::from(r"C:\a\Duet.LNK"), PathBuf::from(r"C:\a\b.lnk")]), ".lnk");
        assert_eq!(kind_of(&[PathBuf::from(r"C:\a\x.exe"), PathBuf::from(r"C:\a\y.txt")]), ".exe+.txt");
        let k = ".test-kind-a";
        assert!(!known_slow(k));
        mark_slow(k);
        assert!(known_slow(k), "never answered: skipped from now on");
        let j = ".test-kind-b";
        note_answered(j);
        assert!(known_slow(j));
        with_slow(|m| _ = m.insert(j.into(), Some(Instant::now() - FORGIVE - Duration::from_secs(1))));
        assert!(!known_slow(j), "forgiven");
        with_slow(|m| _ = m.remove(k));
    }

    /// The menu of a real file in the temp folder, only built and read (nothing is invoked), on a worker as the desktop does: the
    /// shell's top level has Open as the default item and Properties, by their verbs, and every submenu is a handle until it is
    /// asked for. The test never waits longer than 30 s: a shell that hangs fails it instead of hanging the run.
    #[test]
    fn a_real_menu_of_a_temp_file_is_read_and_nothing_is_run() {
        crate::sh::init();
        let d = std::env::temp_dir().join(format!("wr-filemenu-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("note.txt");
        std::fs::write(&f, b"x").unwrap();
        let ps = vec![f.clone()];
        let w = spawn(move || build_session(HWND::default(), &ps));
        let built = await_built(&w, Duration::from_secs(30));
        let _ = std::fs::remove_dir_all(&d);
        let Built::Ready(items) = built else { panic!("a menu in 30 s: {built:?}") };
        let verbs: Vec<&str> = items.iter().map(|i| i.verb.as_str()).collect();
        assert!(items.iter().any(|i| i.verb == "open" && i.is_default() && i.id != 0), "{verbs:?}");
        assert!(verbs.contains(&"properties") && verbs.contains(&"copy") && verbs.contains(&"cut") && verbs.contains(&"delete"), "{verbs:?}");
        assert!(items.iter().filter(|i| i.has_sub()).all(|i| matches!(i.sub, filemenu::Sub::Lazy(n) if n >= 1)), "submenus are handles");
    }
}
