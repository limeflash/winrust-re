//! The live window's side of the machine: [`run`] does what the window asks of Windows (the shell's file operations, the bar's menu,
//! a drag, the clipboard, the copier), the places of the sidebar, the picture worker, and the little that the window host cannot do for
//! an app: messages that are not the host's (the bar's answers to the menu, the menu's timer) and the work that must run once the
//! host has finished with the window's own call.
//!
//! Anything that may run a modal loop is queued ([`defer`]) and runs after the host's handling of the current message, with nothing
//! of the window borrowed: the host's event path then works as usual while the shell's progress dialog or a drag is up.
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ffi::c_void,
    path::PathBuf,
    sync::{OnceLock, mpsc::Sender},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, ClientToScreen, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{ChangeWindowMessageFilterEx, IsIconic, MSGFLT_ALLOW, PostMessageW, SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow, WM_APP, WM_APPCOMMAND, WM_LBUTTONUP, WM_NCDESTROY, WM_TIMER},
    },
};
use winrust_common::{deskmenu, filemenu, t, tf};

use super::{
    backend::{DriveInfo, Do, Place, Places},
    look::{DriveKind, IconKind, Special},
};

pub const MSG_THUMB: u32 = WM_APP + 2;
pub const MSG_TICK: u32 = WM_APP + 9;
pub const MSG_RELOAD: u32 = WM_APP + 10;
pub const MSG_DONE: u32 = WM_APP + 11;
pub const MSG_PLACES: u32 = WM_APP + 12;
pub const MSG_MENU_OWN: u32 = WM_APP + 13;
pub const MSG_DRAG_DONE: u32 = WM_APP + 14;
pub const MSG_RENAME: u32 = WM_APP + 16;
pub const MSG_OPEN: u32 = WM_APP + 17;
pub const MSG_NEWTAB: u32 = WM_APP + 18;
pub const MSG_REGISTER: u32 = WM_APP + 15;
pub const MSG_BACK: u32 = WM_APP + 19;
pub const MSG_FORWARD: u32 = WM_APP + 20;
/// Posted to wake the subclass so that it runs what was queued.
const MSG_WAKE: u32 = WM_APP + 60;

/// How many pictures the worker has brought to a window (what the self-test looks at).
static PICTURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[must_use]
pub fn pictures() -> usize {
    PICTURES.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn count_picture() {
    PICTURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// A picture a worker made.
pub struct ThumbMsg {
    pub era: u32,
    /// The picture is the file's icon (a program's own), not a thumbnail of what is in it.
    pub icon: bool,
    pub path: PathBuf,
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

fn hw(owner: isize) -> HWND {
    owner as HWND
}

// ---------------------------------------------------------------------------------------------------------------------
// the window's subclass and the queue

type Job = Box<dyn FnOnce()>;

thread_local! {
    static QUEUE: RefCell<VecDeque<Job>> = const { RefCell::new(VecDeque::new()) };
    static OWNER: Cell<isize> = const { Cell::new(0) };
    static DRAINING: Cell<bool> = const { Cell::new(false) };
}

/// Queues `f` to run on the window's thread once the host has handled the message it is in.
pub fn defer(f: impl FnOnce() + 'static) {
    QUEUE.with_borrow_mut(|q| q.push_back(Box::new(f)));
    let h = OWNER.get();
    if h != 0 {
        // SAFETY: a message to our own window wakes the subclass, which runs the queue.
        unsafe { PostMessageW(hw(h), MSG_WAKE, 0, 0) };
    }
}

fn drain() {
    if DRAINING.replace(true) {
        return;
    }
    while let Some(j) = QUEUE.with_borrow_mut(VecDeque::pop_front) {
        j();
    }
    DRAINING.set(false);
}

/// What the file manager's window does for the rows of the bar's menu that act on the window: it is told with messages.
struct WinHost;

fn boxed(p: PathBuf) -> isize {
    Box::into_raw(Box::new(p)) as isize
}

impl crate::shellmenu::Win for WinHost {
    fn reload(&self, h: *mut c_void) {
        // SAFETY: a message to the window.
        unsafe { PostMessageW(h, MSG_RELOAD, 0, 0) };
    }

    fn rename(&self, h: *mut c_void) {
        // SAFETY: a message to the window.
        unsafe { PostMessageW(h, MSG_RENAME, 0, 0) };
    }

    fn open(&self, h: *mut c_void, p: PathBuf) {
        // SAFETY: the box is taken back by `App::message` for this message.
        unsafe { PostMessageW(h, MSG_OPEN, 0, boxed(p)) };
    }

    fn new_tab(&self, h: *mut c_void, p: PathBuf) {
        // SAFETY: the box is taken back by `App::message` for this message.
        unsafe { PostMessageW(h, MSG_NEWTAB, 0, boxed(p)) };
    }

    fn toggle_pin(&self, h: *mut c_void, p: PathBuf) {
        crate::ui::toggle_pin(&p);
        // SAFETY: a message to the window.
        unsafe { PostMessageW(h, MSG_PLACES, 0, 0) };
    }
}

unsafe extern "system" fn subclass(h: HWND, m: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    // what the bar's menu answered: the host knows no registered message
    if m == filemenu::act_msg() {
        crate::shellmenu::act_with(&WinHost, h, wp, lp);
        drain();
        return 0;
    }
    // the timer that drains what the shell's menu worker says (`shellmenu::offer` set it on this window)
    if m == WM_TIMER && wp == crate::shellmenu::TIMER {
        crate::shellmenu::tick(h);
        return 0;
    }
    // the mouse's back and forward buttons (and the keyboard's browser keys) arrive as APPCOMMAND_BROWSER_BACKWARD / _FORWARD
    if m == WM_APPCOMMAND {
        match (lp >> 16) & 0x0FFF {
            1 => {
                // SAFETY: a message to our own window.
                unsafe { PostMessageW(h, MSG_BACK, 0, 0) };
                return 1;
            }
            2 => {
                // SAFETY: a message to our own window.
                unsafe { PostMessageW(h, MSG_FORWARD, 0, 0) };
                return 1;
            }
            _ => {}
        }
    }
    if m == WM_NCDESTROY {
        // SAFETY: removes the subclass this function was installed as.
        unsafe { RemoveWindowSubclass(h, Some(subclass), 1) };
    }
    // SAFETY: the next procedure of the chain: the host's.
    let r = unsafe { DefSubclassProc(h, m, wp, lp) };
    drain();
    r
}

/// Called once the window exists: the subclass, and the permission for the bar (which may run elevated) to answer the menu.
pub fn attach(hwnd: isize) {
    OWNER.set(hwnd);
    // SAFETY: our own window; the subclass lives as long as it.
    unsafe {
        SetWindowSubclass(hw(hwnd), Some(subclass), 1, 0);
        ChangeWindowMessageFilterEx(hw(hwnd), filemenu::act_msg(), MSGFLT_ALLOW, std::ptr::null_mut());
    }
}

/// Brings the window forward ("Show in folder" from another app).
pub fn raise(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    // SAFETY: plain calls on our own window.
    unsafe {
        if IsIconic(hw(hwnd)) != 0 {
            ShowWindow(hw(hwnd), SW_RESTORE);
        }
        SetForegroundWindow(hw(hwnd));
    }
}

/// Whether the bar is there to draw the menu of the shell's items.
pub fn bar() -> bool {
    !deskmenu::bar().is_null()
}

// ---------------------------------------------------------------------------------------------------------------------
// doing what was asked

/// Screen pixels of a point of the window's client area in DIPs.
fn screen_of(owner: isize, (x, y): (f32, f32)) -> (i32, i32) {
    // SAFETY: plain queries of our own window.
    unsafe {
        let s = windows_sys::Win32::UI::HiDpi::GetDpiForWindow(hw(owner)) as f32 / 96.0;
        let mut p = POINT { x: (x * s).round() as i32, y: (y * s).round() as i32 };
        ClientToScreen(hw(owner), &mut p);
        (p.x, p.y)
    }
}

fn post(owner: isize, msg: u32) {
    winrust_ui::window::post(owner, msg, 0, 0);
}

pub fn run(owner: isize, d: Do) {
    let h = hw(owner);
    match d {
        Do::Open(p) => crate::open_file(h, p),
        Do::RunAs(p) => crate::run_as(h, &p),
        Do::Clip { paths, cut } => _ = crate::sh::clip_set(h, &paths, cut),
        Do::Paste(dir) => defer(move || {
            crate::paste(hw(owner), &dir);
            post(owner, MSG_RELOAD);
        }),
        Do::Delete { paths, dir } => defer(move || {
            // the shell's own confirmation and undo
            let _ = crate::sh::fileop(hw(owner), crate::sh::Op::Delete, &paths, &dir);
            post(owner, MSG_RELOAD);
        }),
        Do::Props(p) => crate::properties(&p),
        Do::ItemMenu { dir, paths, at } => defer(move || {
            let at = at.map(|p| screen_of(owner, p));
            if !crate::shellmenu::offer(hw(owner), &dir, &paths, at) {
                // no bar, or it does not answer: the window shows its own menu
                post(owner, MSG_MENU_OWN);
            }
        }),
        Do::Drag(paths) => defer(move || {
            match crate::real_paths(&paths) {
                Ok(real) if !real.is_empty() => {
                    let _ = crate::sh::drag(hw(owner), &real);
                }
                Ok(_) => {}
                Err(e) => crate::warn(hw(owner), &e),
            }
            // the drag took the mouse button's release: the window is told, away from anything it has
            // SAFETY: a message to our own window, sent with no borrow of the host held.
            unsafe { SendMessageW(hw(owner), WM_LBUTTONUP, 0, -1 as LPARAM) };
            post(owner, MSG_DRAG_DONE);
            post(owner, MSG_RELOAD);
        }),
        Do::DropOn { paths, to, copy } => defer(move || {
            let _ = crate::sh::fileop(hw(owner), if copy { crate::sh::Op::Copy } else { crate::sh::Op::Move }, &paths, &to);
            post(owner, MSG_RELOAD);
        }),
        Do::Zip { dir, paths } => {
            let base = if paths.len() == 1 { paths[0].file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { dir.file_name().map_or("Archive".into(), |n| n.to_string_lossy().into_owned()) };
            let dest = crate::jobs::free_name(&dir, &format!("{base}.zip"), false);
            crate::jobs::zip(owner, paths, dest);
        }
        Do::Extract { dir, paths } => {
            if crate::zipx::split_zip(&dir).is_some() {
                // out of an archive being looked into: next to the archive, in a new folder named after it
                if let Some((arc, names)) = crate::zip_names(&paths)
                    && let (Some(parent), Some(stem)) = (arc.parent(), arc.file_stem())
                {
                    let out = crate::jobs::free_name(parent, &stem.to_string_lossy(), true);
                    std::thread::spawn(move || _ = crate::zipx::extract_some(&arc, &names, &out));
                }
            } else {
                let _ = crate::jobs::extract(owner, paths, dir);
            }
        }
        Do::CopyPaths(paths) => crate::shellmenu::copy_paths_text(h, &paths),
        Do::Pin(p) => {
            crate::ui::toggle_pin(&p);
        }
        Do::Terminal(dir) => crate::shellmenu::terminal(&dir),
        Do::DiskUsage(dir) => {
            // wiztree takes the drive letter and asks for admin rights itself
            let drive: String = dir.to_string_lossy().chars().take(2).collect();
            if let Some(exe) = crate::tool("wiztree") {
                let _ = std::process::Command::new(exe).arg(drive).spawn();
            }
        }
        Do::Sizes { dir, names } => crate::folder_sizes(h, dir, names),
        Do::Thumb { path, icon, size } => {
            let _ = thumbs_tx(owner).send(ThumbReq { era: crate::GEN.load(std::sync::atomic::Ordering::Relaxed), path, icon, size });
        }
        Do::Eject(_) => {}
        Do::JobCancel => crate::jobs::cancel(),
        Do::Navigated(dir) => crate::shellwin::navigated(&dir),
        Do::Start => {
            attach(owner);
            post(owner, MSG_REGISTER);
        }
        Do::Register => crate::shellwin::register(owner),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the places of the sidebar

/// Home, the user's known folders, the folders pinned to Quick access, and the drives with how full they are.
pub fn places() -> Places {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos};
    let mut quick = Vec::new();
    let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default());
    if home.is_dir() {
        quick.push(Place { title: t!("Главная", "Home", ja: "ホーム", pt: "Início", zh: "主页").into(), path: home, icon: IconKind::Home, pinned: false });
    }
    let known = [
        (t!("Рабочий стол", "Desktop", ja: "デスクトップ", pt: "Área de Trabalho", zh: "桌面"), &FOLDERID_Desktop, Special::Desktop),
        (t!("Загрузки", "Downloads", ja: "ダウンロード", pt: "Downloads", zh: "下载"), &FOLDERID_Downloads, Special::Downloads),
        (t!("Документы", "Documents", ja: "ドキュメント", pt: "Documentos", zh: "文档"), &FOLDERID_Documents, Special::Documents),
        (t!("Изображения", "Pictures", ja: "ピクチャ", pt: "Imagens", zh: "图片"), &FOLDERID_Pictures, Special::Pictures),
        (t!("Музыка", "Music", ja: "ミュージック", pt: "Música", zh: "音乐"), &FOLDERID_Music, Special::Music),
        (t!("Видео", "Videos", ja: "ビデオ", pt: "Vídeos", zh: "视频"), &FOLDERID_Videos, Special::Videos),
    ];
    for (title, id, special) in known {
        if let Some(p) = crate::sh::known_dir(id).filter(|p| p.is_dir()) {
            quick.push(Place { title: title.into(), path: p, icon: IconKind::Folder(Some(special)), pinned: false });
        }
    }
    for p in crate::ui::pins() {
        if p.is_dir() {
            let title = p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
            quick.push(Place { title, path: p, icon: IconKind::Folder(None), pinned: true });
        }
    }
    Places { quick, drives: drives() }
}

fn drives() -> Vec<DriveInfo> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    let mut out = Vec::new();
    // SAFETY: plain queries with NUL-terminated names and buffers of the sizes given.
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let root = format!("{letter}:\\");
            let rootw = crate::w(&root);
            let ty = GetDriveTypeW(rootw.as_ptr());
            if ty == 4 {
                // a mapped network drive: listed without label and size, they would stall the window while a share is down
                out.push(DriveInfo {
                    root: PathBuf::from(root),
                    label: tf!("Сетевой диск ({}:)", "Network Drive ({}:)", ja: "ネットワーク ドライブ ({}:)", pt: "Unidade de rede ({}:)", zh: "网络驱动器（{}:）", letter),
                    kind: DriveKind::Net,
                    fs: String::new(),
                    total: 0,
                    free: 0,
                    removable: false,
                });
                continue;
            }
            if !matches!(ty, 2 | 3) {
                continue;
            }
            let (mut label, mut fs) = ([0u16; 64], [0u16; 32]);
            let ok = GetVolumeInformationW(rootw.as_ptr(), label.as_mut_ptr(), 64, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), 32);
            let (name, fs) = if ok != 0 { (crate::wstr(label.as_ptr()), crate::wstr(fs.as_ptr())) } else { (String::new(), String::new()) };
            let (mut free, mut total) = (0u64, 0u64);
            let cap = GetDiskFreeSpaceExW(rootw.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) != 0 && total > 0;
            let removable = ty == 2;
            let base = if !name.is_empty() {
                name
            } else if removable {
                t!("USB-накопитель", "USB Drive", ja: "USB ドライブ", pt: "Unidade USB", zh: "USB 驱动器").into()
            } else {
                t!("Локальный диск", "Local Disk", ja: "ローカル ディスク", pt: "Disco local", zh: "本地磁盘").into()
            };
            out.push(DriveInfo {
                root: PathBuf::from(root),
                label: format!("{base} ({letter}:)"),
                kind: if removable { DriveKind::Usb } else { DriveKind::Ssd },
                fs,
                total: if cap { total } else { 0 },
                free: if cap { free } else { 0 },
                removable,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// the picture worker

struct ThumbReq {
    era: u32,
    path: PathBuf,
    icon: bool,
    size: i32,
}

fn thumbs_tx(owner: isize) -> &'static Sender<ThumbReq> {
    static TX: OnceLock<Sender<ThumbReq>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<ThumbReq>();
        std::thread::spawn(move || {
            use std::os::windows::ffi::OsStrExt;
            use windows::{
                Win32::{
                    Foundation::SIZE,
                    System::Com::{COINIT_MULTITHREADED, CoInitializeEx},
                    UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY, SIIGBF_THUMBNAILONLY},
                },
                core::PCWSTR,
            };
            // SAFETY: COM for this worker thread; the shell's image factory is asked about one path at a time.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                for r in rx {
                    if r.era != crate::GEN.load(std::sync::atomic::Ordering::Relaxed) {
                        continue;
                    }
                    let wide: Vec<u16> = r.path.as_os_str().encode_wide().chain([0]).collect();
                    let Ok(f) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(PCWSTR(wide.as_ptr()), None) else { continue };
                    let flags = SIIGBF_BIGGERSIZEOK | if r.icon { SIIGBF_ICONONLY } else { SIIGBF_THUMBNAILONLY };
                    let Ok(bmp) = f.GetImage(SIZE { cx: r.size, cy: r.size }, flags) else { continue };
                    let got = pixels_of(bmp.0);
                    DeleteObject(bmp.0);
                    let Some((w, h, px)) = got else { continue };
                    let m = Box::new(ThumbMsg { era: r.era, icon: r.icon, path: r.path, w, h, px });
                    PostMessageW(hw(owner), MSG_THUMB, Box::into_raw(m) as usize, 0);
                }
            }
        });
        tx
    })
}

/// The pixels of a 32-bit bitmap as premultiplied BGRA (a thumbnail with no alpha at all is opaque).
unsafe fn pixels_of(bmp: *mut c_void) -> Option<(i32, i32, Vec<u32>)> {
    // SAFETY: the bitmap is read through GetDIBits into a buffer of exactly width * height pixels.
    unsafe {
        let mut bm: BITMAP = std::mem::zeroed();
        if GetObjectW(bmp, size_of::<BITMAP>() as i32, (&raw mut bm).cast()) == 0 || bm.bmWidth <= 0 || bm.bmHeight == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
        let mut px = vec![0u32; (w * h) as usize];
        let dc = GetDC(std::ptr::null_mut());
        let n = GetDIBits(dc, bmp, 0, h as u32, px.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
        ReleaseDC(std::ptr::null_mut(), dc);
        if n == 0 {
            return None;
        }
        if px.iter().all(|p| p >> 24 == 0) {
            px.iter_mut().for_each(|p| *p |= 0xff00_0000);
        } else if px.iter().any(|p| {
            let a = p >> 24;
            (p >> 16 & 255) > a || (p >> 8 & 255) > a || (p & 255) > a
        }) {
            // straight alpha: premultiplied for Direct2D
            for p in &mut px {
                let a = *p >> 24;
                let m = |s: u32| ((*p >> s & 255) * a + 127) / 255;
                *p = a << 24 | m(16) << 16 | m(8) << 8 | m(0);
            }
        }
        Some((w, h, px))
    }
}
