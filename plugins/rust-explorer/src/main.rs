// rust-explorer [path]  — file manager window (Explorer replacement).
// Virtual list (LVS_OWNERDATA, only visible rows are asked for text/icon), system icons, tabs,
// IFileOperation (recycle bin / undo / elevation prompts), real shell context menu with submenus, OLE drag and drop.
// Keys: Enter open, Backspace up, F2 rename, Del recycle, Ctrl+C/X/V copy/cut/paste, Ctrl+N new folder,
// Ctrl+T/W/Tab tabs, Ctrl+M thumbnails, Ctrl+L/F4 address bar, Ctrl+F or "? terms" in the bar = instant search through the rs service, right click / Menu key = shell context menu.
// Look: topbar's palette, painted tabs / toolbar / address pill and status line (src/ui.rs), dark title bar and list.
// Clipboard is the system one (Ctrl+C/X/V interoperate with Explorer). Also: Alt+Left/Right back/forward, Alt+Up up,
// Alt+Enter properties, F5 refresh, Ctrl+H hidden files, click a column header to sort, right click on empty space = folder menu,
// middle click on a folder = open it in a new tab.
// shortcuts: std::fs listing (not FindFirstFileEx large fetch), icons are extension-based (exe/lnk/ico/url/scr
// per file) and synchronous (thumbnails: one background thread, 96px, no disk cache), internal copy/cut clipboard, silent rename errors,
// search blocks the UI while it waits for the service (no async), the service pipe is admin-only so run as admin,
// drops always land in the current folder (not on a folder row), tabs remember only the folder (no selection/scroll).
// upgrade: each of those one at a time; then register as the folder-open handler.
// rust-explorer --desktop  - the desktop for rs-shell: borderless bottom-most color-keyed window with the Desktop folders
//   (user + public) as big icons over the wallpaper it paints itself (without explorer nothing else does): reads the BMP
//   that rs-shell converts into %LOCALAPPDATA%\rs-shell\wallpaper.bmp, "fill" style, scaled once at start.
//   shortcuts: wallpaper is loaded once (changing it needs a restart), icon labels are always white, no background menu /
//   drag-to-position / auto-arrange, items from the public desktop can't be renamed.
#![windows_subsystem = "windows"]
use std::{cell::RefCell, env, fs, os::windows::fs::MetadataExt, path::{Path, PathBuf}, ptr, sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed}};
use winrust_common::{deskmenu::{self, Cmd}, t, tf};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::{GetStockObject, InvalidateRect, ScreenToClient}, System::LibraryLoader::GetModuleHandleW, UI::{Controls::*, Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_CONTROL}, Shell::*, WindowsAndMessaging::*}};

mod desktop;
use desktop::*;

mod zipx; // our own ZIP engine (deflate, inflate, container)
mod jobs; // our own copier, ZIP packer, ZIP/7z unpacker (background, progress in the status line)
mod shellwin; // registers the window as a shell window: "Show in folder" from other apps selects the file
mod menu; // the folder's own menu, painted in our colours
mod shellmenu; // the bar's menu for files and folders: the shell's items offered to it, what it chooses run here
mod ui; // the look: palette, painted tabs / toolbar / address pill / status line, dark theme

static DESKTOP: AtomicBool = AtomicBool::new(false);
fn desk() -> bool { DESKTOP.load(Relaxed) }
/// Where the file manager window was when it last closed: %APPDATA%\rust-explorer\window.txt, `max|normal left top right bottom`
/// (the restored rectangle, so un-maximizing goes back to the size it had).
fn place_file() -> PathBuf {
    PathBuf::from(env::var_os("APPDATA").unwrap_or_default()).join(r"rust-explorer\window.txt")
}
fn parse_place(s: &str) -> Option<(bool, RECT)> {
    let mut it = s.split_whitespace();
    let max = match it.next()? { "max" => true, "normal" => false, _ => return None };
    let n: Vec<i32> = it.map(str::parse).collect::<Result<_, _>>().ok()?;
    let [left, top, right, bottom] = n[..] else { return None };
    (right - left >= 200 && bottom - top >= 150).then_some((max, RECT { left, top, right, bottom })) // junk or a collapsed window: default size
}
fn save_place(h: HWND) {
    let mut wp: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
    wp.length = size_of::<WINDOWPLACEMENT>() as u32;
    if unsafe { GetWindowPlacement(h, &mut wp) } == 0 { return; }
    let r = wp.rcNormalPosition;
    let f = place_file();
    let _ = fs::create_dir_all(f.parent().unwrap_or(&f));
    let _ = fs::write(f, format!("{} {} {} {} {}", if wp.showCmd == SW_SHOWMAXIMIZED as u32 { "max" } else { "normal" }, r.left, r.top, r.right, r.bottom));
}
/// The saved place, if it is still on a monitor (one may have been unplugged since).
fn saved_place() -> Option<(bool, RECT)> {
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
    let (max, r) = parse_place(&fs::read_to_string(place_file()).ok()?)?;
    (!unsafe { MonitorFromRect(&r, MONITOR_DEFAULTTONULL) }.is_null()).then_some((max, r))
}
const THUMB: i32 = 96;
const WM_THUMB: u32 = WM_APP + 2; // worker -> UI: wp = Box<(era, row, HBITMAP)>
static GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0); // bumped on every new listing; stale thumbnails are dropped
static TX: std::sync::OnceLock<std::sync::mpsc::Sender<(u32, usize, PathBuf)>> = std::sync::OnceLock::new();
const WM_GO: u32 = WM_APP + 1; // address bar: Enter (wp 13) / Esc (wp 27)
static EDIT_OLD: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static LABEL_OLD: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0); // the rename box (only one exists at a time)

struct Row { name: String, dir: bool, size: u64, time: u64, icon: i32, thumb: i32, hdr: bool } // time: FILETIME ticks; thumb: -2 not asked, -1 asked, >=0 image-list index

/// Back / forward trail of visited folders (one for the whole window, not per tab).
#[derive(Default)]
struct Hist { v: Vec<PathBuf>, pos: usize }
impl Hist {
    fn visit(&mut self, p: PathBuf) {
        if self.v.get(self.pos) == Some(&p) { return; }
        self.v.truncate(self.pos + usize::from(!self.v.is_empty())); // a new visit drops the forward trail
        self.v.push(p);
        self.pos = self.v.len() - 1;
    }
    fn back(&mut self) -> Option<PathBuf> { if self.pos > 0 { self.pos -= 1; Some(self.v[self.pos].clone()) } else { None } }
    fn forward(&mut self) -> Option<PathBuf> { if self.pos + 1 < self.v.len() { self.pos += 1; Some(self.v[self.pos].clone()) } else { None } }
}

struct State { list: HWND, tab: HWND, edit: HWND, dir: PathBuf, rows: Vec<Row>, thumbs: bool, himl: isize, tabs: Vec<PathBuf>, active: usize, hist: Hist }
thread_local! {
    static ST: RefCell<State> = RefCell::new(State { list: 0 as _, tab: 0 as _, edit: 0 as _, dir: PathBuf::new(), rows: vec![], thumbs: false, himl: 0, tabs: vec![], active: 0, hist: Hist { v: vec![], pos: 0 } });
}
static SORT_COL: AtomicUsize = AtomicUsize::new(0); // 0 name, 1 size, 2 modified
static SORT_ASC: AtomicBool = AtomicBool::new(true);
static SHOW_HIDDEN: AtomicBool = AtomicBool::new(false); // Ctrl+H
static NAV: AtomicBool = AtomicBool::new(false); // loading from the back / forward trail: do not record it again
const WM_NAV: u32 = WM_APP + 3; // wp: 1 back, 2 forward, 3 properties, 4 up (sent from the message loop for Alt+key)
const WM_DIRSIZE: u32 = WM_APP + 7; // folder-size worker -> UI: wp = Box<(era, folder name, bytes)> (#147)
const WM_WALL: u32 = WM_APP + 8; // desktop: rs-shell made the new wallpaper BMP, paint it
const WM_JOB: u32 = WM_APP + 5; // jobs.rs worker finished: wp = Box<Option<String>> (the error, if any)

fn row(name: String, m: Option<&fs::Metadata>) -> Row {
    Row { name, dir: m.is_some_and(|m| m.is_dir()), size: m.map_or(0, |m| m.len()), time: m.map_or(0, |m| m.last_write_time()), icon: -1, thumb: -2, hdr: false }
}

/// Folders first (unless "Folders first" is off in the folder's menu, #162); inside each group by the column (0 name, 1 size, 2 modified), ties by name.
fn sort_rows(rows: &mut [Row], col: usize, asc: bool) {
    let key = |r: &Row| r.name.rsplit('\\').next().unwrap_or("").to_lowercase();
    let mixed = !desk() && mixed_marker().exists();
    rows.sort_by(|a, b| {
        let o = match col { 1 => a.time.cmp(&b.time), 2 => type_text(&a.name, a.dir).cmp(&type_text(&b.name, b.dir)), 3 => a.size.cmp(&b.size), _ => key(a).cmp(&key(b)) };
        let dirs = if mixed { std::cmp::Ordering::Equal } else { (!a.dir).cmp(&!b.dir) };
        dirs.then(if asc { o } else { o.reverse() }).then_with(|| key(a).cmp(&key(b)))
    });
    // grouped by date: the groups are contiguous (the stable sort keeps the order above inside each one)
    if group_on() { bucket_refresh(); rows.sort_by_cached_key(|r| bucket(r.time)); }
}

// ---------- grouping by date ("Group by date" in the folder's menu): Today, Yesterday, Earlier this week, ... ----------

const DAY: i64 = 864_000_000_000; // FILETIME ticks per day
/// The date headings of "Group by date", in bucket order: shown above the rows of each group.
fn bucket_name(i: usize) -> &'static str {
    [
        t!("Сегодня", "Today", ja: "今日", pt: "Hoje", zh: "今天"),
        t!("Вчера", "Yesterday", ja: "昨日", pt: "Ontem", zh: "昨天"),
        t!("Ранее на этой неделе", "Earlier this week", ja: "今週", pt: "No início desta semana", zh: "本周早些时候"),
        t!("На прошлой неделе", "Last week", ja: "先週", pt: "Semana passada", zh: "上周"),
        t!("Ранее в этом месяце", "Earlier this month", ja: "今月", pt: "No início deste mês", zh: "本月早些时候"),
        t!("В прошлом месяце", "Last month", ja: "先月", pt: "Mês passado", zh: "上个月"),
        t!("Ранее в этом году", "Earlier this year", ja: "今年", pt: "No início deste ano", zh: "今年早些时候"),
        t!("Давно", "A long time ago", ja: "ずっと前", pt: "Há muito tempo", zh: "很久以前"),
    ][i]
}

thread_local! {
    static BCTX: std::cell::Cell<(i64, i64)> = const { std::cell::Cell::new((0, 0)) }; // (today's local day number, local - UTC in ticks)
}

fn mixed_marker() -> PathBuf { PathBuf::from(env::var_os("APPDATA").unwrap_or_default()).join("rust-explorer").join("folders-mixed") }
fn group_marker() -> PathBuf { PathBuf::from(env::var_os("APPDATA").unwrap_or_default()).join(r"rust-explorer\group-by-date") }
static THUMBS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false); // thumbnail view: no headings
fn group_on() -> bool { !desk() && !THUMBS.load(Relaxed) && group_marker().exists() }

/// Takes "now" and the time zone offset once, so bucket() is plain arithmetic while a folder is sorted and painted.
fn bucket_refresh() {
    use windows_sys::Win32::{Foundation::FILETIME, Storage::FileSystem::FileTimeToLocalFileTime};
    let now = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() / 100) as i64) + 116_444_736_000_000_000;
    let (utc, mut local) = (FILETIME { dwLowDateTime: now as u32, dwHighDateTime: (now >> 32) as u32 }, FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 });
    let off = if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } != 0 { ((local.dwHighDateTime as i64) << 32 | local.dwLowDateTime as i64) - now } else { 0 };
    BCTX.set(((now + off) / DAY, off));
}

/// Index into BUCKETS of a FILETIME (UTC ticks).
fn bucket(time: u64) -> i32 {
    let (today, off) = BCTX.get();
    match today - (time as i64 + off) / DAY {
        i64::MIN..=0 => 0,
        1 => 1,
        2..=6 => 2,
        7..=13 => 3,
        14..=29 => 4,
        30..=59 => 5,
        60..=364 => 6,
        _ => 7,
    }
}

/// A heading row ("Today", ...) before the first row of each date group. Its name starts with '?', which no file can have.
fn add_headers(rows: &mut Vec<Row>) {
    if !group_on() { return; }
    bucket_refresh();
    let mut out = Vec::with_capacity(rows.len() + 8);
    let mut last = -1;
    for r in rows.drain(..) {
        let b = bucket(r.time);
        if b != last {
            last = b;
            out.push(Row { name: format!("?{}", bucket_name(b as usize)), dir: false, size: 0, time: 0, icon: -1, thumb: -1, hdr: true });
        }
        out.push(r);
    }
    *rows = out;
}

/// Explorer's "Type" column in short form: "File folder", "TXT file", "File".
fn type_text(name: &str, dir: bool) -> String {
    if dir { return t!("Папка с файлами", "File folder", ja: "ファイル フォルダー", pt: "Pasta de arquivos", zh: "文件夹").into(); }
    let file = name.rsplit('\\').next().unwrap_or(name);
    match file.rsplit_once('.') { Some((stem, e)) if !stem.is_empty() && !e.is_empty() => tf!("Файл {}", "{} file", ja: "{} ファイル", pt: "Arquivo {}", zh: "{} 文件", e.to_uppercase()), _ => t!("Файл", "File", ja: "ファイル", pt: "Arquivo", zh: "文件").into() }
}

/// Explorer shows file sizes in KB; the bigger ones read better in MB / GB.
fn size_text(b: u64) -> String {
    const MB: u64 = 1 << 20;
    if b < MB { format!("{} KB", b.div_ceil(1024)) } else if b < 1 << 30 { format!("{:.1} MB", b as f64 / MB as f64) } else { format!("{:.2} GB", b as f64 / (1u64 << 30) as f64) }
}

fn date_text(ft: u64) -> String {
    use windows_sys::Win32::{Foundation::{FILETIME, SYSTEMTIME}, System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime}};
    if ft == 0 { return String::new(); }
    unsafe {
        let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
        let (mut u, mut l): (SYSTEMTIME, SYSTEMTIME) = (std::mem::zeroed(), std::mem::zeroed());
        if FileTimeToSystemTime(&f, &mut u) == 0 || SystemTimeToTzSpecificLocalTime(ptr::null(), &u, &mut l) == 0 { return String::new(); }
        format!("{:04}-{:02}-{:02} {:02}:{:02}", l.wYear, l.wMonth, l.wDay, l.wHour, l.wMinute)
    }
}

/// Our own tools installed by the launcher (photoview, wiztree): the exe in Program Files, None when not installed.
fn tool(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env::var_os("ProgramFiles")?).join(name).join(format!("{name}.exe"));
    p.is_file().then_some(p)
}

/// Same list as photoview's EXTS.
fn is_pic(p: &std::path::Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| ["jpg", "jpeg", "jfif", "png", "bmp", "gif", "tif", "tiff", "webp", "heic", "avif"].contains(&e.to_ascii_lowercase().as_str()))
}

fn properties(path: &PathBuf) {
    let (f, v) = (w(&path.to_string_lossy()), w("properties"));
    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = 0x0C; // SEE_MASK_INVOKEIDLIST
    sei.lpVerb = v.as_ptr(); sei.lpFile = f.as_ptr(); sei.nShow = SW_SHOW;
    unsafe { ShellExecuteExW(&mut sei) };
}

/// New folder / new empty file with a free name ("New folder", "New folder (2)", ...).
fn new_entry(dir: &PathBuf, base: &str, folder: bool) {
    if zipx::split_zip(dir).is_some() { return; }
    let (stem, ext) = match base.rsplit_once('.') { Some((s, e)) if !folder => (s.to_string(), format!(".{e}")), _ => (base.to_string(), String::new()) };
    let path = (1..).map(|n| dir.join(if n == 1 { base.to_string() } else { format!("{stem} ({n}){ext}") })).find(|p| !p.exists());
    if let Some(p) = path { let _ = if folder { fs::create_dir(p) } else { fs::File::create(p).map(drop) }; }
}

/// Pastes what is on the system clipboard (files copied or cut here or in any other file manager).
/// Copies and moves to another drive run in our own copier (jobs.rs: parallel on SSDs, unbuffered for big files); moves on the
/// same drive (a rename), shell items, and folders that need elevation go through the shell.
fn paste(h: HWND, dir: &PathBuf) {
    if zipx::split_zip(dir).is_some() { return; } // an archive is read-only here
    let Some((paths, cut)) = sh::clip_get() else { return };
    let fast = jobs::can_fast_copy(&paths, dir) && !(cut && paths.iter().all(|p| jobs::same_volume(p, dir))) && jobs::writable(dir);
    if fast && !jobs::busy() {
        jobs::copy(h as isize, paths, dir.clone(), cut);
    } else {
        let _ = sh::fileop(h, if cut { sh::Op::Move } else { sh::Op::Copy }, &paths, dir);
    }
    if cut { sh::clip_clear(); }
}

const MENU_ZIP: u32 = 0x9001;
const MENU_EXTRACT: u32 = 0x9002;
const MENU_PIN: u32 = 0x9003;
const MENU_RENAME: u32 = 0x9004;
const MENU_OPEN: u32 = 0x9005;
const MENU_RUNAS: u32 = 0x9006;
const MENU_COPY_PATH: u32 = 0x9007;
const MENU_PROPS: u32 = 0x9008;
const MENU_DELETE: u32 = 0x9009;

/// True while the window shows the inside of a .zip: read-only, its rows are no files on disk.
fn in_zip() -> bool { zipx::split_zip(&cur_dir()).is_some() }

fn warn(h: HWND, text: &str) {
    unsafe { MessageBoxW(h, w(text).as_ptr(), w("WinRust Explorer").as_ptr(), MB_ICONWARNING) };
}

/// (archive, archive paths) when every one of `paths` is something inside one and the same .zip.
fn zip_names(paths: &[PathBuf]) -> Option<(PathBuf, Vec<String>)> {
    let mut arc = None;
    let mut names = vec![];
    for p in paths {
        let (a, inner) = zipx::split_zip(p).filter(|(_, i)| !i.is_empty())?;
        if arc.get_or_insert_with(|| a.clone()) != &a { return None; }
        names.push(inner);
    }
    Some((arc?, names))
}

/// Things from an archive as real files in a temp folder: the clipboard, drag and "open with" need files on disk. One path per name.
/// Folders of earlier days are cleaned up here.
fn materialize(arc: &Path, names: &[String]) -> Result<Vec<PathBuf>, String> {
    static N: AtomicUsize = AtomicUsize::new(0);
    let root = env::temp_dir().join("WinRust-zip");
    for e in fs::read_dir(&root).into_iter().flatten().flatten() {
        if e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a > std::time::Duration::from_secs(86_400)) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
    let out = root.join(format!("{}-{}", std::process::id(), N.fetch_add(1, Relaxed)));
    zipx::extract_some(arc, names, &out)?;
    Ok(names.iter().map(|n| out.join(n.rsplit('/').next().unwrap_or(n))).collect())
}

/// `paths` as files on disk: unchanged, or taken out of the archive they are in first.
fn real_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    match zip_names(paths) {
        Some((arc, names)) => materialize(&arc, &names),
        None => Ok(paths.to_vec()),
    }
}

/// Enter / double click on a row.
unsafe fn activate(h: HWND, p: PathBuf, is_dir: bool) {
    // a file inside a zip: a real copy in the temp folder first
    let p = match zip_names(std::slice::from_ref(&p)) {
        Some((arc, names)) if !is_dir => match materialize(&arc, &names) {
            Ok(mut v) => v.remove(0),
            Err(e) => return warn(h, &e),
        },
        _ => p,
    };
    if is_dir && desk() { let _ = std::process::Command::new(env::current_exe().unwrap_or_default()).arg(p).spawn(); }
    else if is_dir { load(h, p) }
    else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) && p.is_file() { load(h, p) } // a zip opens like a folder; "Extract here" is in its menu
    else if let Some(exe) = tool("photoview").filter(|_| is_pic(&p)) { let _ = std::process::Command::new(exe).arg(&p).spawn(); }
    else {
        // like Explorer: the exe starts in its own folder (games look for files next to it)
        let dir = p.parent().map(|d| w(&d.to_string_lossy()));
        unsafe { ShellExecuteW(h, ptr::null(), w(&p.to_string_lossy()).as_ptr(), ptr::null(), dir.as_ref().map_or(ptr::null(), |d| d.as_ptr()), SW_SHOWNORMAL) };
    }
}

/// The menu for rows inside a zip. They are no shell items, so this is a small menu of our own.
fn zip_menu(h: HWND, paths: &[PathBuf]) {
    let id = unsafe {
        let m = CreatePopupMenu();
        let items = [
            (1usize, t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开")),
            (2, t!("Копировать", "Copy", ja: "コピー", pt: "Copiar", zh: "复制")),
            (3, t!("Извлечь сюда", "Extract here", ja: "ここに展開", pt: "Extrair aqui", zh: "解压到此处")),
        ];
        for (i, lbl) in items { AppendMenuW(m, MF_STRING, i, w(lbl).as_ptr()); }
        let mut pt: POINT = std::mem::zeroed();
        GetCursorPos(&mut pt);
        let id = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, h, ptr::null());
        DestroyMenu(m);
        id
    };
    let Some((arc, names)) = zip_names(paths) else { return };
    match id {
        1 => if let [p] = paths {
            let is_dir = ST.with_borrow(|st| st.rows.iter().any(|r| r.dir && st.dir.join(&r.name) == *p));
            unsafe { activate(h, p.clone(), is_dir) };
        },
        2 => match materialize(&arc, &names) {
            Ok(v) => { let _ = sh::clip_set(h, &v, false); }
            Err(e) => warn(h, &e),
        },
        3 => {
            // next to the archive, in a new folder named after it; no progress yet (shortcut: fire and forget)
            let (Some(parent), Some(stem)) = (arc.parent(), arc.file_stem()) else { return };
            let out = jobs::free_name(parent, &stem.to_string_lossy(), true);
            std::thread::spawn(move || _ = zipx::extract_some(&arc, &names, &out));
        }
        _ => {}
    }
}

/// The shell menu for the selection, with our "Compress to ZIP" / "Extract here" on top.
fn item_menu(h: HWND, dir: &Path, paths: &[PathBuf], key: bool) {
    if zipx::split_zip(dir).is_some() { return zip_menu(h, paths); }
    // the bar draws it (and runs nothing here until it is chosen from); from the keyboard it opens at the selected row
    if shellmenu::offer(h, dir, paths, if key { shellmenu::row_at(selected().0) } else { None }) { return; }
    // the bar is not there (or does not answer): a small menu of our own rows. Windows' own menu (7-Zip, Send to...) is not built here:
    // asking the shell for it on this thread can hang the whole desktop, and `shellmenu::offer` is where it is asked, on a worker.
    let mut extra: Vec<(u32, &str)> = vec![];
    let one = paths.len() == 1;
    if one { extra.push((MENU_OPEN, t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开"))); }
    if one && paths[0].extension().is_some_and(|e| ["exe", "lnk", "bat", "cmd", "msi"].iter().any(|x| e.eq_ignore_ascii_case(x))) {
        extra.push((MENU_RUNAS, t!("Запуск от имени администратора", "Run as administrator", ja: "管理者として実行", pt: "Executar como administrador", zh: "以管理员身份运行")));
    }
    if paths.iter().all(|p| p.is_file() && jobs::is_archive(p)) {
        extra.push((MENU_EXTRACT, t!("Извлечь сюда", "Extract here", ja: "ここに展開", pt: "Extrair aqui", zh: "解压到此处")));
    }
    if !jobs::busy() { extra.push((MENU_ZIP, t!("Сжать в ZIP", "Compress to ZIP", ja: "ZIP に圧縮", pt: "Compactar em ZIP", zh: "压缩为 ZIP"))); }
    extra.push((MENU_COPY_PATH, t!("Копировать как путь", "Copy as path", ja: "パスのコピー", pt: "Copiar como caminho", zh: "复制文件地址")));
    // the shell menu has no Rename here (it needs the shell's own folder view to answer it): the same edit box as F2
    if one { extra.push((MENU_RENAME, t!("Переименовать", "Rename", ja: "名前の変更", pt: "Renomear", zh: "重命名"))); }
    if let [p] = paths { if p.is_dir() { extra.push((MENU_PIN, if ui::pins().contains(p) { t!("Открепить от быстрого доступа", "Unpin from Quick access", ja: "クイック アクセスからピン留めを解除", pt: "Desafixar do Acesso rápido", zh: "从快速访问取消固定") } else { t!("Закрепить в быстром доступе", "Pin to Quick access", ja: "クイック アクセスにピン留め", pt: "Fixar no Acesso rápido", zh: "固定到快速访问") })); } }
    if !in_zip() { extra.push((MENU_DELETE, t!("Удалить", "Delete", ja: "削除", pt: "Excluir", zh: "删除"))); }
    if one { extra.push((MENU_PROPS, t!("Свойства", "Properties", ja: "プロパティ", pt: "Propriedades", zh: "属性"))); }
    match sh::own_menu(h, &extra) {
        Some(MENU_OPEN) => unsafe { activate(h, paths[0].clone(), paths[0].is_dir()) },
        Some(MENU_RUNAS) => unsafe {
            let dir = paths[0].parent().map(|d| w(&d.to_string_lossy()));
            ShellExecuteW(h, w("runas").as_ptr(), w(&paths[0].to_string_lossy()).as_ptr(), ptr::null(), dir.as_ref().map_or(ptr::null(), |d| d.as_ptr()), SW_SHOWNORMAL);
        },
        Some(MENU_COPY_PATH) => shellmenu::copy_paths_text(h, paths),
        Some(MENU_PROPS) => properties(&paths[0]),
        Some(MENU_DELETE) => { let _ = sh::fileop(h, sh::Op::Delete, paths, &dir.to_path_buf()); reload(h); }
        Some(MENU_PIN) => ui::toggle_pin(&paths[0]),
        Some(MENU_RENAME) => unsafe {
            let (list, ..) = selected();
            let i = put(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
            if i >= 0 {
                SetFocus(list);
                put(list, LVM_EDITLABELW, i as usize, 0);
            }
        },
        Some(MENU_ZIP) => {
            let base = if paths.len() == 1 { paths[0].file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { dir.file_name().map_or("Archive".into(), |n| n.to_string_lossy().into_owned()) };
            let dest = jobs::free_name(dir, &format!("{base}.zip"), false);
            jobs::zip(h as isize, paths.to_vec(), dest);
        }
        Some(MENU_EXTRACT) => _ = jobs::extract(h as isize, paths.to_vec(), dir.to_path_buf()),
        _ => reload(h),
    }
}

/// Right click on empty space: the folder's own menu. On the desktop the bar draws the design's menu when it runs (`desktop::ask_bar`);
/// this one is the desktop's own, for when it does not.
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn bg_menu(h: HWND) {
    if desk() && desktop::ask_bar() { return; }
    let m = CreatePopupMenu();
    menu::item(m, 1, "\u{E8F4}", t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹"), false);
    menu::item(m, 2, "\u{E8A5}", t!("Новый текстовый файл", "New text file", ja: "新しいテキスト ドキュメント", pt: "Novo arquivo de texto", zh: "新建文本文件"), false);
    menu::item(m, 3, "\u{E77F}", t!("Вставить", "Paste", ja: "貼り付け", pt: "Colar", zh: "粘贴"), false);
    menu::sep(m);
    if !desk() {
        let sort = CreatePopupMenu();
        let (col, asc) = (SORT_COL.load(Relaxed), SORT_ASC.load(Relaxed));
        // columns of sort_rows: 0 name, 1 modified, 2 type, 3 size
        let cols = [
            (0usize, "\u{E8D2}", t!("По имени", "Name", ja: "名前", pt: "Nome", zh: "名称")),
            (1, "\u{E787}", t!("По дате изменения", "Date modified", ja: "更新日時", pt: "Data de modificação", zh: "修改日期")),
            (2, "\u{E7C3}", t!("По типу", "Type", ja: "種類", pt: "Tipo", zh: "类型")),
            (3, "\u{E9D9}", t!("По размеру", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小")),
        ];
        for (c, glyph, lbl) in cols { menu::item(sort, 20 + c, glyph, lbl, col == c); }
        menu::sep(sort);
        menu::item(sort, 30, "\u{E74A}", t!("По возрастанию", "Ascending", ja: "昇順", pt: "Crescente", zh: "升序"), asc);
        menu::item(sort, 31, "\u{E74B}", t!("По убыванию", "Descending", ja: "降順", pt: "Decrescente", zh: "降序"), !asc);
        menu::sep(sort);
        menu::item(sort, 14, "\u{E8B7}", t!("Папки сверху", "Folders first", ja: "フォルダーを先頭に表示", pt: "Pastas primeiro", zh: "文件夹优先"), !mixed_marker().exists());
        menu::submenu(m, "\u{E8CB}", t!("Сортировка", "Sort by", ja: "並べ替え", pt: "Classificar por", zh: "排序方式"), sort);
        menu::item(m, 13, "\u{E81C}", t!("Группировать по дате", "Group by date", ja: "日付でグループ化", pt: "Agrupar por data", zh: "按日期分组"), group_on());
    } else {
        menu::item(m, 8, "\u{E8D2}", t!("Упорядочить значки по имени", "Sort icons by name", ja: "名前順にアイコンを並べ替え", pt: "Classificar ícones por nome", zh: "按名称排列图标"), false);
        menu::item(m, 9, "\u{E787}", t!("Упорядочить значки по дате", "Sort icons by date", ja: "日付順にアイコンを並べ替え", pt: "Classificar ícones por data", zh: "按日期排列图标"), false);
        menu::item(m, 10, "\u{E9D9}", t!("Упорядочить значки по размеру", "Sort icons by size", ja: "サイズ順にアイコンを並べ替え", pt: "Classificar ícones por tamanho", zh: "按大小排列图标"), false);
        menu::item(m, 11, "\u{E80A}", t!("Выровнять значки", "Arrange icons", ja: "アイコンの整列", pt: "Alinhar ícones", zh: "排列图标"), false);
        let lbl = if view().size == deskmenu::IconSize::Small { t!("Крупные значки", "Large icons", ja: "大きいアイコン", pt: "Ícones grandes", zh: "大图标") } else { t!("Мелкие значки", "Small icons", ja: "小さいアイコン", pt: "Ícones pequenos", zh: "小图标") };
        menu::item(m, 12, "\u{E8A9}", lbl, false);
        let lbl = if icons_hidden() { t!("Показать значки рабочего стола", "Show desktop icons", ja: "デスクトップ アイコンを表示", pt: "Mostrar ícones da área de trabalho", zh: "显示桌面图标") } else { t!("Скрыть значки рабочего стола", "Hide desktop icons", ja: "デスクトップ アイコンを非表示", pt: "Ocultar ícones da área de trabalho", zh: "隐藏桌面图标") };
        menu::item(m, 7, "\u{E7B3}", lbl, false);
    }
    menu::sep(m);
    menu::item(m, 4, "\u{E72C}", t!("Обновить", "Refresh", ja: "最新の情報に更新", pt: "Atualizar", zh: "刷新"), false);
    let disk = tool("wiztree");
    if disk.is_some() { menu::item(m, 6, "\u{EDA2}", t!("Занятое место (WinRust Disk)", "Disk usage (WinRust Disk)", ja: "ディスク使用量 (WinRust Disk)", pt: "Uso do disco (WinRust Disk)", zh: "磁盘占用（WinRust Disk）"), false); }
    menu::item(m, 5, "\u{E946}", t!("Свойства", "Properties", ja: "プロパティ", pt: "Propriedades", zh: "属性"), false);
    let id = menu::track(h, m);
    let dir = cur_dir();
    match id {
        7 => set_icons_hidden(!icons_hidden()),
        20..=23 => SORT_COL.store(id as usize - 20, Relaxed), // the reload below sorts again
        30 | 31 => SORT_ASC.store(id == 30, Relaxed),
        13 => {
            let k = group_marker();
            if group_on() { let _ = fs::remove_file(&k); } else { if let Some(d) = k.parent() { let _ = fs::create_dir_all(d); } let _ = fs::write(&k, "1"); }
        }
        14 => {
            let k = mixed_marker();
            if k.exists() { let _ = fs::remove_file(&k); } else { if let Some(d) = k.parent() { let _ = fs::create_dir_all(d); } let _ = fs::write(&k, "1"); }
        }
        12 => { // the grid changes with the size: icons are laid out again
            set_view(view().toggled_small());
            apply_icon_size(ST.with_borrow(|st| st.list));
            let _ = fs::remove_file(desk::pos_file());
        }
        8..=11 => { // the saved cells are dropped: reload lays every icon out again, down the columns in sort order
            if id != 11 { let s = deskmenu::Sort::ALL[id as usize - 8]; set_view(View { sort: s, ..view() }); SORT_COL.store(sort_col(s), Relaxed); SORT_ASC.store(true, Relaxed); }
            let _ = fs::remove_file(desk::pos_file());
        }
        6 => { // wiztree takes the drive letter and asks for admin rights itself
            let drive: String = dir.to_string_lossy().chars().take(2).collect();
            if let Some(exe) = disk { let _ = std::process::Command::new(exe).arg(drive).spawn(); }
        }
        1 => new_entry(&dir, t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹"), true),
        2 => new_entry(&dir, t!("Новый текстовый документ.txt", "New text document.txt", ja: "新しいテキスト ドキュメント.txt", pt: "Novo documento de texto.txt", zh: "新建文本文档.txt"), false),
        3 => paste(h, &dir),
        5 => properties(&dir),
        _ => {}
    }
    if id != 0 { reload(h); }
}

/// Command line -> (folder to open, file to select there). Understands `/select,"C:\a\b.txt"` (what browsers and other
/// apps pass for "show in folder"), a plain folder, or a plain file path (its folder, file selected).
fn start_target() -> (PathBuf, Option<String>) {
    let mut a = env::args().skip(1);
    let mut p = a.next().unwrap_or_default();
    if p.to_lowercase().starts_with("/select,") || p.to_lowercase().starts_with("--select") {
        p = p.get(8..).unwrap_or("").trim_start_matches(',').to_string();
        if p.is_empty() { p = a.next().unwrap_or_default(); }
    }
    let p = PathBuf::from(p.trim_matches('"'));
    if p.as_os_str().is_empty() { return (PathBuf::from("C:\\"), None); }
    match (p.is_file(), p.parent()) {
        (true, Some(d)) => (d.to_path_buf(), p.file_name().map(|n| n.to_string_lossy().into_owned())),
        _ => (p, None),
    }
}

/// Another app asked the shell to show `p` in a folder window (SHOpenFolderAndSelectItems): open its folder here, select it, come to front.
fn shell_select(h: isize, p: PathBuf) {
    let h = h as HWND;
    let (Some(dir), Some(name)) = (p.parent(), p.file_name()) else { return };
    if !cur_dir().as_os_str().eq_ignore_ascii_case(dir.as_os_str()) { load(h, dir.to_path_buf()); }
    select_row(&name.to_string_lossy());
    unsafe {
        if IsIconic(h) != 0 { ShowWindow(h, SW_RESTORE); }
        SetForegroundWindow(h);
    }
}

/// Selects and scrolls to the row called `name` in the current folder.
fn select_row(name: &str) {
    let (list, i) = ST.with_borrow(|st| (st.list, st.rows.iter().position(|r| r.name.eq_ignore_ascii_case(name))));
    let Some(i) = i else { return };
    unsafe {
        let mut it: LVITEMW = std::mem::zeroed();
        it.stateMask = LVIS_SELECTED | LVIS_FOCUSED;
        put(list, LVM_SETITEMSTATE, usize::MAX, &it as *const _ as isize); // the shell selects it alone
        it.state = LVIS_SELECTED | LVIS_FOCUSED;
        put(list, LVM_SETITEMSTATE, i, &it as *const _ as isize);
        put(list, LVM_ENSUREVISIBLE, i, 0);
    }
}

/// Back / forward along the visited-folder trail.
fn nav(h: HWND, back: bool) {
    let to = ST.with_borrow_mut(|st| if back { st.hist.back() } else { st.hist.forward() });
    if let Some(p) = to {
        NAV.store(true, Relaxed);
        load(h, p);
        NAV.store(false, Relaxed);
    }
}

fn w(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }

fn put(h: HWND, msg: u32, wp: usize, lp: isize) -> isize { unsafe { SendMessageW(h, msg, wp, lp) } }

fn wstr(p: *const u16) -> String {
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 { n += 1; }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) })
}

pub fn cur_dir() -> PathBuf { ST.with_borrow(|st| st.dir.clone()) }

/// System image-list index. Extension-based (no disk access) except for files that carry their own icon.
fn icon_of(p: &PathBuf, dir: bool) -> i32 {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let own = !dir && matches!(ext.as_str(), "exe" | "lnk" | "ico" | "url" | "scr");
    let mut fi: SHFILEINFOW = unsafe { std::mem::zeroed() };
    let attr = if dir { 0x10 } else { 0x80 }; // DIRECTORY / NORMAL
    let flags = SHGFI_SYSICONINDEX | SHGFI_SMALLICON | if own { 0 } else { SHGFI_USEFILEATTRIBUTES };
    unsafe { SHGetFileInfoW(w(&p.to_string_lossy()).as_ptr(), attr, &mut fi, size_of::<SHFILEINFOW>() as u32, flags) };
    fi.iIcon
}

/// The tabs are painted by the chrome window from State.tabs: "setting a tab's text" is just a repaint.
fn tab_text(tab: HWND, _i: usize, _text: &str, _insert: bool) {
    if !tab.is_null() { unsafe { InvalidateRect(tab, ptr::null(), 0) }; }
}

/// The text of a desktop row. Rows from the public desktop carry absolute paths: only the file name is shown, and shortcuts
/// lose their ".lnk" / ".url" like in Explorer. Other windows show the row as it is.
fn shown_name(name: &str, desktop: bool) -> String {
    if !desktop { return name.to_string(); }
    let file = std::path::Path::new(name).file_name().map_or_else(|| name.to_string(), |n| n.to_string_lossy().into_owned());
    let lower = file.to_lowercase();
    if lower.ends_with(".lnk") || lower.ends_with(".url") { file[..file.len() - 4].to_string() } else { file }
}

/// Type-ahead of the virtual list (LVN_ODFINDITEM): the first row at or after `start` whose name begins with `typed`, wrapping
/// round to the top. Case is ignored; `name_at(i)` is None for the date-group heading rows, which are never a match. A list
/// with LVS_OWNERDATA asks us because it has no items of its own to search: unanswered, it always lands on the first row.
fn find_row(len: usize, start: usize, typed: &str, name_at: impl Fn(usize) -> Option<String>) -> Option<usize> {
    let typed = typed.to_lowercase();
    if typed.is_empty() || len == 0 { return None; }
    (0..len).map(|k| (start.min(len) + k) % len).find(|&i| name_at(i).is_some_and(|n| n.to_lowercase().starts_with(&typed)))
}

fn label(dir: &PathBuf) -> String {
    dir.file_name().map_or_else(|| dir.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// Rows of `dir`, folders first. Desktop mode merges the public desktop (absolute names) and hides hidden files.
/// None = `dir` itself is unreadable.
fn rows_of(dir: &PathBuf) -> Option<Vec<Row>> {
    if !desk() {
        if let Some((arc, inner)) = zipx::split_zip(dir) { // inside a .zip: its entries are the rows
            let mut rows: Vec<Row> = zipx::list_dir(&arc, &inner).ok()?.into_iter().map(|i| Row { name: i.name, dir: i.dir, size: i.size, time: zipx::dos_to_filetime(i.time), icon: -1, thumb: -2, hdr: false }).collect();
            sort_rows(&mut rows, SORT_COL.load(Relaxed), SORT_ASC.load(Relaxed));
            add_headers(&mut rows);
            return Some(rows);
        }
    }
    let mut dirs = vec![dir.clone()];
    if desk() { dirs.extend(desktop::desktop_dirs().into_iter().skip(1)); }
    let mut rows = vec![];
    for (n, d) in dirs.iter().enumerate() {
        let Ok(rd) = fs::read_dir(d) else { if n == 0 { return None } else { continue } };
        for e in rd.flatten() {
            let m = e.metadata().ok();
            if (desk() || !SHOW_HIDDEN.load(Relaxed)) && m.as_ref().is_some_and(|m| m.file_attributes() & 2 != 0) { continue; } // FILE_ATTRIBUTE_HIDDEN
            let name = if n == 0 { e.file_name().to_string_lossy().into_owned() } else { e.path().to_string_lossy().into_owned() };
            rows.push(row(name, m.as_ref()));
        }
    }
    sort_rows(&mut rows, SORT_COL.load(Relaxed), SORT_ASC.load(Relaxed));
    add_headers(&mut rows);
    Some(rows)
}

/// Loads `dir` into the virtual list and the active tab. Unreadable dir (access denied, gone) leaves the view unchanged.
/// Never call with ST borrowed: the list sends GETDISPINFO back into the window proc.
fn load(h: HWND, dir: PathBuf) {
    let Some(rows) = rows_of(&dir) else { return };
    if !desk() { unsafe { SetWindowTextW(h, w(&dir.to_string_lossy()).as_ptr()) }; }
    let (list, tab, edit, active, n) = ST.with_borrow_mut(|st| {
        if let Some(t) = st.tabs.get_mut(st.active) { *t = dir.clone(); }
        st.dir = dir.clone(); st.rows = rows;
        if !NAV.load(Relaxed) { st.hist.visit(dir.clone()); }
        (st.list, st.tab, st.edit, st.active, st.rows.len())
    });
    unsafe { SetWindowTextW(edit, w(&dir.to_string_lossy()).as_ptr()) };
    shellwin::navigated(&dir);
    ui::set_search(None); // a folder again: the pill shows its path
    tab_text(tab, active, &label(&dir), false);
    ui::nav_dirty(); // the pane highlights where we are
    reset_thumbs();
    put(list, LVM_DELETEALLITEMS, 0, 0);
    if desk() { desk::fill(list, n); } else { put(list, LVM_SETITEMCOUNT, n, 0); }
    folder_sizes(h, dir, ST.with_borrow(|st| st.rows.iter().filter(|r| r.dir && !r.hdr).map(|r| r.name.clone()).collect()));
}

/// Asks the rust-search service of drive `d` (see plugins/rust-search): terms in, one path per line out.
/// None = no service for that drive or no access (its pipe is admin-only unless started with --allow).
fn ask(d: char, q: &str) -> Option<String> {
    use std::{fs::OpenOptions, io::{Read, Write}};
    let mut p = OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\rs-search-{d}")).ok()?;
    p.write_all(q.as_bytes()).ok()?;
    let (mut out, mut chunk) = (Vec::new(), [0u8; 1 << 16]);
    // the server ends its reply by hanging up, which surfaces as a read error: treat any error as EOF
    while let Ok(n @ 1..) = p.read(&mut chunk) { out.extend_from_slice(&chunk[..n]); }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Fills the list with search hits from every drive's service. Rows hold absolute paths, so joining them
/// onto the current dir still yields the right file. Blocks the UI for the duration of the query (fast).
/// Never call with ST borrowed.
fn search(h: HWND, q: &str) {
    let mut rows = vec![];
    let mut more = String::new();
    let mut indexed = false; // some drive had a rust-search service
    for d in 'A'..='Z' {
        let Some(reply) = ask(d, q) else { continue };
        indexed = true;
        for line in reply.lines() {
            if line.starts_with("... ") { more = format!(", {}", line.trim_start_matches("... ")); continue; }
            let m = fs::metadata(line).ok();
            rows.push(row(line.to_string(), m.as_ref()));
        }
    }
    if !indexed {
        // no rust-search installed: look through the current folder and below instead
        let (local, cut, flipped) = local_search(&cur_dir(), q);
        if flipped { more = tf!(" для «{}»", " for \u{201C}{}\u{201D}", ja: "「{}」として", pt: " para \u{201C}{}\u{201D}", zh: "（按「{}」）", flip_layout(q)); } // typed in the wrong layout: shown what was meant
        rows = local;
        if cut { more += t!(", остановлено: уточните запрос", ", stopped early: narrow the search", ja: "、中断: 検索範囲を狭めてください", pt: ", interrompido: refine a pesquisa", zh: "，已提前停止：请缩小搜索范围"); }
    }
    let n = rows.len();
    let where_ = if indexed { "" } else { t!(" в этой папке", " in this folder", ja: "（このフォルダー内）", pt: " nesta pasta", zh: "（仅此文件夹）") };
    let title = if n == 0 {
        tf!("поиск: {} - ничего не найдено{}{}", "search: {} - nothing{}{}", ja: "検索: {} - 見つかりません{}{}", pt: "pesquisa: {} - nada encontrado{}{}", zh: "搜索：{} - 无结果{}{}", q, where_, "")
    } else {
        tf!("поиск: {} - совпадений: {}{}{}", "search: {} - {} hits{}{}", ja: "検索: {} - {} 件{}{}", pt: "pesquisa: {} - {} resultados{}{}", zh: "搜索：{} - {} 个结果{}{}", q, n, where_, more)
    };
    ui::set_search(Some(q.to_string()));
    unsafe { SetWindowTextW(h, w(&title).as_ptr()) };
    let list = ST.with_borrow_mut(|st| { st.rows = rows; st.list });
    reset_thumbs();
    put(list, LVM_DELETEALLITEMS, 0, 0);
    put(list, LVM_SETITEMCOUNT, n, 0);
}

/// The same keys typed in the other layout (QWERTY <-> ЙЦУКЕН): "сфкпщ" -> "cargo", "ljrevtyns" -> "документы".
fn flip_layout(q: &str) -> String {
    let en: Vec<char> = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>~".chars().collect();
    let ru: Vec<char> = "йцукенгшщзхъфывапролджэячсмитьбюёЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮЁ".chars().collect();
    q.chars().map(|c| en.iter().position(|&x| x == c).map(|i| ru[i]).or_else(|| ru.iter().position(|&x| x == c).map(|i| en[i])).unwrap_or(c)).collect()
}

/// Files and folders under `dir` whose name contains `q` (any case), as rows with absolute names. Walks breadth-first and
/// stops after LOCAL_MAX hits or LOCAL_MS so a search from C:\ cannot hang the window. The same walk also matches `q` typed
/// in the other keyboard layout; those hits are used only when `q` itself found nothing.
/// Returns (rows, stopped early, rows are for the other layout).
/// shortcut: blocks the UI while it walks (up to LOCAL_MS); move to a thread if deeper searches are wanted.
fn local_search(dir: &Path, q: &str) -> (Vec<Row>, bool, bool) {
    const LOCAL_MAX: usize = 500;
    const LOCAL_MS: u128 = 1500;
    let (q, t0) = (q.to_lowercase(), std::time::Instant::now());
    let alt = flip_layout(&q);
    let (mut out, mut other, mut queue) = (vec![], vec![], std::collections::VecDeque::from([dir.to_path_buf()]));
    let mut cut = false;
    'walk: while let Some(d) = queue.pop_front() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            if out.len() >= LOCAL_MAX || t0.elapsed().as_millis() > LOCAL_MS { cut = true; break 'walk; }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && !ft.is_symlink() { queue.push_back(e.path()); } // no junction loops
            let name = e.file_name().to_string_lossy().to_lowercase();
            let hit = |v: &mut Vec<Row>| v.push(row(e.path().to_string_lossy().into_owned(), e.metadata().ok().as_ref()));
            if name.contains(&q) { hit(&mut out); } else if alt != q && other.len() < LOCAL_MAX && name.contains(&alt) { hit(&mut other); }
        }
    }
    if out.is_empty() && !other.is_empty() { (other, cut, true) } else { (out, cut, false) }
}

/// Drops all thumbnails and invalidates in-flight ones; call whenever the rows are replaced.
fn reset_thumbs() {
    GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let himl = ST.with_borrow(|st| st.himl);
    unsafe { ImageList_Remove(himl as _, -1) };
}

/// Bytes under `p`, not following junctions or symlinks (they point elsewhere, often back up the tree); stops when `stale`.
fn tree_size(p: &std::path::Path, stale: &dyn Fn() -> bool) -> u64 {
    const REPARSE: u32 = 0x400; // FILE_ATTRIBUTE_REPARSE_POINT
    let Ok(rd) = fs::read_dir(p) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        if stale() { return n; }
        let Ok(m) = e.metadata() else { continue };
        if m.file_attributes() & REPARSE != 0 { continue; }
        n += if m.is_dir() { tree_size(&e.path(), stale) } else { m.len() };
    }
    n
}

/// Fills the Size column of the folders in `dir` in the background, one folder at a time; a new listing stops it.
/// shortcut: walks with read_dir (no MFT, no cache); a drive root is skipped because that walk is the whole disk.
fn folder_sizes(h: HWND, dir: PathBuf, names: Vec<String>) {
    if desk() || dir.parent().is_none() || names.is_empty() { return; }
    let (era, hw) = (GEN.load(Relaxed), h as usize);
    std::thread::spawn(move || {
        let stale = || GEN.load(Relaxed) != era;
        for name in names {
            let n = tree_size(&dir.join(&name), &stale);
            if stale() { return; }
            unsafe { PostMessageW(hw as HWND, WM_DIRSIZE, Box::into_raw(Box::new((era, name, n))) as usize, 0) };
        }
    });
}

fn reload(h: HWND) { load(h, cur_dir()); }

/// (list, current dir, full paths of the selected rows)
fn selected() -> (HWND, PathBuf, Vec<PathBuf>) {
    ST.with_borrow(|st| {
        let (mut sel, mut i) = (vec![], -1isize);
        loop {
            i = put(st.list, LVM_GETNEXTITEM, i as usize, LVNI_SELECTED as isize);
            let Some(r) = usize::try_from(i).ok().and_then(|i| st.rows.get(i)) else { break };
            if !r.hdr { sel.push(st.dir.join(&r.name)); }
        }
        (st.list, st.dir.clone(), sel)
    })
}

fn col(list: HWND, i: i32, text: &str, width: i32, right: bool) {
    let mut t = w(text);
    let mut c: LVCOLUMNW = unsafe { std::mem::zeroed() };
    c.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_FMT; c.pszText = t.as_mut_ptr(); c.cx = width;
    c.fmt = if right { LVCFMT_RIGHT } else { LVCFMT_LEFT };
    put(list, LVM_INSERTCOLUMNW, i as usize, &c as *const _ as isize);
}

/// Opens a new tab on the current folder.
fn tab_new() {
    let tab = ST.with_borrow_mut(|st| { st.tabs.push(st.dir.clone()); st.active = st.tabs.len() - 1; st.tab });
    tab_text(tab, 0, "", true);
}

/// Opens `dir` in a new tab, staying on the current one (middle click on a folder row).
fn tab_open(dir: PathBuf) {
    let tab = ST.with_borrow_mut(|st| { st.tabs.push(dir); st.tab });
    tab_text(tab, 0, "", true); // insert: the bar repaints with the new tab, the listing stays
}

fn tab_select(h: HWND, i: usize) {
    let dir = ST.with_borrow_mut(|st| { st.active = i; st.tabs[i].clone() });
    load(h, dir);
}

fn tab_close(h: HWND) { let a = ST.with_borrow(|st| st.active); tab_close_at(h, a); }

/// Closes tab `i` (the last one stays). The listing changes only when the active tab was the one closed.
fn tab_close_at(h: HWND, i: usize) {
    let (tab, was_active) = ST.with_borrow_mut(|st| {
        if st.tabs.len() < 2 || i >= st.tabs.len() { return (st.tab, None); }
        st.tabs.remove(i);
        let before = st.active;
        st.active = if i < before { before - 1 } else { before.min(st.tabs.len() - 1) };
        (st.tab, Some(i == before))
    });
    match was_active {
        Some(true) => { let a = ST.with_borrow(|st| st.active); tab_select(h, a); }
        Some(false) => tab_text(tab, 0, "", false),
        None => {}
    }
}

fn toggle_view(h: HWND) {
    let (list, on) = ST.with_borrow_mut(|st| { st.thumbs = !st.thumbs; (st.list, st.thumbs) });
    THUMBS.store(on, Relaxed);
    ui::row_height(list, !on);
    put(list, LVM_SETVIEW, if on { LV_VIEW_ICON } else { LV_VIEW_DETAILS } as usize, 0);
    reload(h);
}

/// Address box in search mode ("? terms").
fn search_focus() {
    let edit = ST.with_borrow(|st| st.edit);
    unsafe {
        SetWindowTextW(edit, w("? ").as_ptr());
        SetFocus(edit);
    }
    put(edit, EM_SETSEL, 2, 2);
}

/// Shell integration through COM (windows crate): file operations, context menu, drag and drop.
mod sh {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::PathBuf};
    use windows::{core::*, Win32::{Foundation::*, System::{Com::*, DataExchange::RegisterClipboardFormatW, Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock}, Ole::*, SystemServices::MODIFIERKEYS_FLAGS}, UI::{Shell::{Common::ITEMIDLIST, *}, WindowsAndMessaging::*}}};

    #[derive(Clone, Copy)]
    pub enum Op { Copy, Move, Delete }

    /// A known folder where Windows says it is (it may be moved to another drive).
    pub fn known_dir(id: &GUID) -> Option<PathBuf> {
        unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const c_void));
            s.map(PathBuf::from)
        }
    }

    pub fn init() { unsafe { let _ = OleInitialize(None); } }

    fn wide(p: &PathBuf) -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() }

    /// Copy / move into `to`, or recycle. Must not be called while ST is borrowed (the shell pumps messages).
    pub fn fileop(h: *mut c_void, op: Op, from: &[PathBuf], to: &PathBuf) -> Result<()> {
        unsafe {
            let fo: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
            fo.SetOwnerWindow(HWND(h))?;
            fo.SetOperationFlags(if matches!(op, Op::Delete) { FOF_ALLOWUNDO } else { FOF_RENAMEONCOLLISION })?;
            let item = |p: &PathBuf| SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide(p).as_ptr()), None);
            let dest = if matches!(op, Op::Delete) { None } else { Some(item(to)?) };
            for p in from {
                let it = item(p)?;
                match (op, &dest) {
                    (Op::Copy, Some(d)) => fo.CopyItem(&it, d, PCWSTR::null(), None)?,
                    (Op::Move, Some(d)) => fo.MoveItem(&it, d, PCWSTR::null(), None)?,
                    _ => fo.DeleteItem(&it, None)?,
                }
            }
            fo.PerformOperations()
        }
    }

    /// Background thumbnail loader: GetImage for each (era, row, path) asked for, result posted to the main window.
    pub fn start_thumbs(main: isize) -> std::sync::mpsc::Sender<(u32, usize, PathBuf)> {
        let (tx, rx) = std::sync::mpsc::channel::<(u32, usize, PathBuf)>();
        std::thread::spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            for (era, row, path) in rx {
                if era != super::GEN.load(std::sync::atomic::Ordering::Relaxed) { continue; }
                let Ok(f) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(PCWSTR(wide(&path).as_ptr()), None) else { continue };
                let Ok(bmp) = f.GetImage(SIZE { cx: super::THUMB, cy: super::THUMB }, SIIGBF_BIGGERSIZEOK) else { continue };
                let padded = pad(bmp.0);
                windows_sys::Win32::Graphics::Gdi::DeleteObject(bmp.0 as _);
                if padded.is_null() { continue; }
                let b = Box::new((era, row, padded as isize));
                let _ = PostMessageW(Some(HWND(main as *mut c_void)), super::WM_THUMB, WPARAM(Box::into_raw(b) as usize), LPARAM(0));
            }
        });
        tx
    }

    /// Centers `src` (any size up to THUMB, 32-bit with alpha) on a transparent THUMB x THUMB bitmap: ImageList_Add needs exact sizes.
    unsafe fn pad(src: *mut c_void) -> *mut c_void {
        use windows_sys::Win32::Graphics::Gdi::*;
        unsafe {
            let t = super::THUMB;
            let mut bm: BITMAP = std::mem::zeroed();
            GetObjectW(src as _, size_of::<BITMAP>() as i32, &mut bm as *mut _ as _);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: t, biHeight: -t, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
            let mut bits = std::ptr::null_mut();
            let dst = CreateDIBSection(std::ptr::null_mut(), &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if dst.is_null() { return std::ptr::null_mut(); }
            let (dc, sdc) = (CreateCompatibleDC(std::ptr::null_mut()), CreateCompatibleDC(std::ptr::null_mut()));
            let (o1, o2) = (SelectObject(dc, dst), SelectObject(sdc, src as _));
            BitBlt(dc, (t - bm.bmWidth) / 2, (t - bm.bmHeight) / 2, bm.bmWidth, bm.bmHeight, sdc, 0, 0, SRCCOPY);
            SelectObject(dc, o1); SelectObject(sdc, o2);
            DeleteDC(dc); DeleteDC(sdc);
            dst
        }
    }

    /// Puts `paths` on the system clipboard like Explorer: the shell's own data object plus the "Preferred DropEffect"
    /// marker (copy or cut), so Ctrl+V works in a real Explorer window and in other file managers.
    pub fn clip_set(h: *mut c_void, paths: &[PathBuf], cut: bool) -> Result<()> {
        unsafe {
            let data: IDataObject = ui(HWND(h), paths)?;
            let fmt = FORMATETC { cfFormat: RegisterClipboardFormatW(w!("Preferred DropEffect")) as u16, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0 as u32, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            let hg = GlobalAlloc(GMEM_MOVEABLE, 4)?;
            *(GlobalLock(hg) as *mut u32) = if cut { DROPEFFECT_MOVE.0 } else { DROPEFFECT_COPY.0 };
            let _ = GlobalUnlock(hg);
            let med = STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: hg }, pUnkForRelease: std::mem::ManuallyDrop::new(None) };
            data.SetData(&fmt, &med, true)?; // the data object owns the memory from here
            OleSetClipboard(&data)
        }
    }

    /// What is on the system clipboard as files: (paths, cut?). None = no files there.
    pub fn clip_get() -> Option<(Vec<PathBuf>, bool)> {
        unsafe {
            let o = OleGetClipboard().ok()?;
            let paths = hdrop_paths(&o).ok().filter(|p| !p.is_empty())?;
            let fmt = FORMATETC { cfFormat: RegisterClipboardFormatW(w!("Preferred DropEffect")) as u16, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0 as u32, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            let cut = o.GetData(&fmt).ok().is_some_and(|mut m| {
                let p = GlobalLock(m.u.hGlobal) as *const u32;
                let v = if p.is_null() { 0 } else { *p };
                let _ = GlobalUnlock(m.u.hGlobal);
                ReleaseStgMedium(&mut m);
                v & DROPEFFECT_MOVE.0 != 0
            });
            Some((paths, cut))
        }
    }

    pub fn clip_clear() { unsafe { let _ = OleSetClipboard(None); } }

    struct Pidls(Vec<*mut ITEMIDLIST>);
    impl Drop for Pidls { fn drop(&mut self) { for p in &self.0 { unsafe { CoTaskMemFree(Some(*p as *const c_void)) }; } } }

    /// A shell UI object (IContextMenu, IDataObject...) for items that all live in one folder.
    pub(crate) fn ui<T: Interface>(h: HWND, paths: &[PathBuf]) -> Result<T> {
        unsafe {
            let mut abs = Pidls(vec![]);
            for p in paths {
                let mut pidl = std::ptr::null_mut();
                SHParseDisplayName(PCWSTR(wide(p).as_ptr()), None, &mut pidl, 0, None)?;
                abs.0.push(pidl);
            }
            let parent: IShellFolder = SHBindToParent(abs.0[0], None)?;
            let kids: Vec<*const ITEMIDLIST> = abs.0.iter().map(|&p| ILFindLastID(p) as *const _).collect();
            parent.GetUIObjectOf(h, &kids, None)
        }
    }

    /// The name Windows shows for its own "Send to > Compressed (zipped) folder": the stem of the .ZFSendToTarget file in SendTo.
    pub(crate) fn system_zip_name() -> Option<String> {
        let dir = PathBuf::from(std::env::var_os("APPDATA")?).join(r"Microsoft\Windows\SendTo");
        let f = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ZFSendToTarget")))?;
        f.file_stem().map(|s| s.to_string_lossy().into_owned())
    }

    unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
        unsafe { (*(lp.0 as *mut Vec<isize>)).push(h.0 as isize); }
        TRUE
    }

    pub(crate) fn top_windows() -> Vec<isize> {
        let mut v: Vec<isize> = vec![];
        unsafe { let _ = EnumWindows(Some(collect), LPARAM(&mut v as *mut _ as isize)); }
        v
    }

    /// Dialogs the shell opens for a command ("Open with" > choose another app, Properties) can land partly off screen on some
    /// setups. For a few seconds after the command, any new window that sticks out of its monitor is moved back inside.
    pub(crate) fn keep_on_screen(before: Vec<isize>) {
        std::thread::spawn(move || unsafe {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                for h in top_windows().into_iter().filter(|h| !before.contains(h)) {
                    let h = HWND(h as *mut c_void);
                    let mut r = RECT::default();
                    if !IsWindowVisible(h).as_bool() || IsZoomed(h).as_bool() || GetWindowRect(h, &mut r).is_err() || r.right <= r.left { continue; }
                    let mut mi = windows::Win32::Graphics::Gdi::MONITORINFO { cbSize: size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32, ..Default::default() };
                    let mon = windows::Win32::Graphics::Gdi::MonitorFromWindow(h, windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST);
                    if !windows::Win32::Graphics::Gdi::GetMonitorInfoW(mon, &mut mi).as_bool() { continue; }
                    let a = mi.rcWork;
                    if r.right - r.left > a.right - a.left || r.bottom - r.top > a.bottom - a.top { continue; } // maximized-like: not a stray dialog
                    let x = r.left.min(a.right - (r.right - r.left)).max(a.left);
                    let y = r.top.min(a.bottom - (r.bottom - r.top)).max(a.top);
                    if (x, y) != (r.left, r.top) {
                        let _ = SetWindowPos(h, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                }
            }
        });
    }

    /// A small popup of our own rows (`extra`: id, text), at the pointer: the id picked, None when nothing was. It never asks the
    /// shell for a context menu, which can hang (see `shellmenu`).
    pub fn own_menu(h: *mut c_void, extra: &[(u32, &str)]) -> Option<u32> {
        unsafe {
            let hwnd = HWND(h);
            let menu = CreatePopupMenu().ok()?;
            for (id, text) in extra {
                let t: Vec<u16> = text.encode_utf16().chain([0]).collect();
                let _ = AppendMenuW(menu, MF_STRING, *id as usize, PCWSTR(t.as_ptr()));
            }
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let id = TrackPopupMenuEx(menu, TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0, pt.x, pt.y, hwnd, None).0;
            let _ = DestroyMenu(menu);
            u32::try_from(id).ok().filter(|i| *i > 0)
        }
    }

    /// Starts an OLE drag of `paths` (blocks until dropped).
    pub fn drag(h: *mut c_void, paths: &[PathBuf]) -> Result<()> {
        unsafe {
            let hwnd = HWND(h);
            let data: IDataObject = ui(hwnd, paths)?;
            SHDoDragDrop(Some(hwnd), &data, None, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK)?;
            Ok(())
        }
    }

    fn hdrop_paths(o: &IDataObject) -> Result<Vec<PathBuf>> {
        unsafe {
            let fmt = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0 as u32, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            let mut med = o.GetData(&fmt)?;
            let hd = HDROP(med.u.hGlobal.0);
            let mut out = vec![];
            for i in 0..DragQueryFileW(hd, u32::MAX, None) {
                let mut buf = vec![0u16; DragQueryFileW(hd, i, None) as usize + 1];
                let n = DragQueryFileW(hd, i, Some(&mut buf)) as usize;
                out.push(PathBuf::from(String::from_utf16_lossy(&buf[..n])));
            }
            ReleaseStgMedium(&mut med);
            Ok(out)
        }
    }

    #[implement(IDropTarget)]
    struct DropT { main: isize }

    fn effect(k: MODIFIERKEYS_FLAGS) -> DROPEFFECT { if k.0 & 8 != 0 { DROPEFFECT_COPY } else { DROPEFFECT_MOVE } } // 8 = MK_CONTROL

    impl IDropTarget_Impl for DropT_Impl {
        fn DragEnter(&self, _o: Ref<'_, IDataObject>, k: MODIFIERKEYS_FLAGS, _pt: &POINTL, e: *mut DROPEFFECT) -> Result<()> { unsafe { *e = if super::in_zip() { DROPEFFECT_NONE } else { effect(k) } }; Ok(()) }
        fn DragOver(&self, k: MODIFIERKEYS_FLAGS, _pt: &POINTL, e: *mut DROPEFFECT) -> Result<()> { unsafe { *e = if super::in_zip() { DROPEFFECT_NONE } else { effect(k) } }; Ok(()) }
        fn DragLeave(&self) -> Result<()> { Ok(()) }
        fn Drop(&self, o: Ref<'_, IDataObject>, k: MODIFIERKEYS_FLAGS, pt: &POINTL, e: *mut DROPEFFECT) -> Result<()> {
            if super::desk::dragging() {
                // desktop icons dropped back on the desktop: a move of the icons, not of the files (the dragger finishes it)
                super::desk::dropped_here((pt.x, pt.y));
                unsafe { *e = DROPEFFECT_NONE };
                return Ok(());
            }
            if super::in_zip() { // nothing can be dropped into an archive here
                unsafe { *e = DROPEFFECT_NONE };
                return Ok(());
            }
            let paths = hdrop_paths(o.ok()?)?;
            let to = super::cur_dir();
            // like Explorer: same drive moves, other drive copies, Ctrl forces copy
            let copy = k.0 & 8 != 0 || paths.first().and_then(|p| p.components().next()) != to.components().next();
            unsafe { *e = if copy { DROPEFFECT_COPY } else { DROPEFFECT_MOVE } };
            let _ = fileop(self.main as *mut c_void, if copy { Op::Copy } else { Op::Move }, &paths, &to);
            unsafe { let _ = PostMessageW(Some(HWND(self.main as *mut c_void)), WM_APP, WPARAM(0), LPARAM(0)); }
            Ok(())
        }
    }

    pub fn register_drop(list: *mut c_void, main: isize) -> Result<()> {
        let t: IDropTarget = DropT { main }.into();
        unsafe { RegisterDragDrop(HWND(list), &t) }
    }
}


/// Trace of the icon drags (%TEMP%\rx-drag.log), to find out where one stops working. Cheap: a few lines per drag.
fn drag_log(s: &str) {
    use std::io::Write;
    let p = std::env::temp_dir().join("rx-drag.log");
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) { let _ = writeln!(f, "{s}"); }
}

/// Ctrl+Backspace in an edit box (it arrives as WM_CHAR 0x7F): deletes back to the start of the previous word.
unsafe fn delete_word(h: HWND) {
    unsafe {
        let (mut a, mut b) = (0u32, 0u32);
        SendMessageW(h, EM_GETSEL, &mut a as *mut u32 as usize, &mut b as *mut u32 as isize);
        if a == b {
            let n = GetWindowTextLengthW(h) as usize;
            let mut buf = vec![0u16; n + 1];
            GetWindowTextW(h, buf.as_mut_ptr(), n as i32 + 1);
            let mut i = a as usize;
            while i > 0 && buf[i - 1] == b' ' as u16 { i -= 1; }
            while i > 0 && buf[i - 1] != b' ' as u16 { i -= 1; }
            SendMessageW(h, EM_SETSEL, i, a as isize);
        }
        SendMessageW(h, EM_REPLACESEL, 1, w("").as_ptr() as isize);
    }
}

/// Rename box subclass: only Ctrl+Backspace is ours.
unsafe extern "system" fn label_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if m == WM_CHAR && wp == 0x7F { delete_word(h); return 0; }
        CallWindowProcW(std::mem::transmute::<isize, WNDPROC>(LABEL_OLD.load(std::sync::atomic::Ordering::Relaxed)), h, m, wp, lp)
    }
}

/// Address bar subclass: Enter / Esc go to the main window, everything else is the normal edit control.
unsafe extern "system" fn edit_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if matches!(wp, 13 | 27) && matches!(m, WM_KEYDOWN | WM_CHAR) {
            if m == WM_KEYDOWN { PostMessageW(GetAncestor(h, GA_ROOT), WM_GO, wp, 0); }
            return 0; // WM_CHAR swallowed so there is no beep
        }
        if m == WM_CHAR && wp == 0x7F { delete_word(h); return 0; }
        CallWindowProcW(std::mem::transmute::<isize, WNDPROC>(EDIT_OLD.load(std::sync::atomic::Ordering::Relaxed)), h, m, wp, lp)
    }
}

unsafe extern "system" fn proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if m == winrust_common::style::msg() {
            // the topbar switched the WinRust style: take it now (the desktop's list keeps its see-through colours)
            winrust_common::style::reload();
            if desk() { InvalidateRect(h, ptr::null(), 1); } else { ui::restyle(h); }
            return 0;
        }
        if m == winrust_common::filemenu::act_msg() { // what the bar's menu for files chose
            shellmenu::act(h, wp, lp);
            return 0;
        }
        if desk() && m == deskmenu::cmd_msg() { // what the bar's desktop menu chose
            if let Some(c) = Cmd::decode(wp, lp) { desktop::command(h, c); }
            return 0;
        }
        match m {
            WM_CREATE => {
                let inst = GetModuleHandleW(ptr::null());
                // the bar may run elevated, we do not: what its menu for files chose must not be dropped (UIPI)
                ChangeWindowMessageFilterEx(h, winrust_common::filemenu::act_msg(), MSGFLT_ALLOW, ptr::null_mut());
                // desktop mode has no tab bar / address bar: the null handles are harmless to every SendMessage below
                let (mut tab, mut edit) = (0 as HWND, 0 as HWND);
                if !desk() {
                    ui::dark_title(h);
                    // the chrome (tabs, toolbar, address pill) is one painted window; the address box is a real edit inside its pill
                    tab = CreateWindowExW(0, w("RxChrome").as_ptr(), ptr::null(), WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN, 0, 0, 0, 0, h, 0 as _, inst, ptr::null());
                    ui::nav_init(CreateWindowExW(0, w("RxNav").as_ptr(), ptr::null(), WS_CHILD | WS_VISIBLE, 0, 0, 0, 0, h, 0 as _, inst, ptr::null()));
                    edit = CreateWindowExW(0, w("EDIT").as_ptr(), ptr::null(), WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32, 0, 0, 0, 0, tab, 0 as _, inst, ptr::null());
                    put(edit, WM_SETFONT, ui::font(14, 400, ui::ui_face()) as usize, 1);
                    EDIT_OLD.store(SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as isize), std::sync::atomic::Ordering::Relaxed);
                }
                let view = if desk() { LVS_ICON } else { LVS_REPORT };
                let list = CreateWindowExW(0, w("SysListView32").as_ptr(), ptr::null(), WS_CHILD | WS_VISIBLE | view | if desk() { 0 } else { LVS_OWNERDATA } | LVS_EDITLABELS | LVS_SHOWSELALWAYS | LVS_SHAREIMAGELISTS, 0, 0, 0, 0, h, 0 as _, inst, ptr::null());
                put(list, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize);
                let mut fi: SHFILEINFOW = std::mem::zeroed();
                let il = SHGetFileInfoW(w("C:\\").as_ptr(), 0, &mut fi, size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX | SHGFI_SMALLICON);
                put(list, LVM_SETIMAGELIST, LVSIL_SMALL as usize, il as isize);
                if desk() {
                    desktop::setup_list(h, list, inst, &mut fi);
                } else {
                    let himl = ImageList_Create(THUMB, THUMB, ILC_COLOR32, 64, 64);
                    put(list, LVM_SETIMAGELIST, LVSIL_NORMAL as usize, himl as isize);
                    put(list, LVM_SETICONSPACING, 0, ((THUMB as isize + 34) << 16) | (THUMB as isize + 24));
                    let _ = TX.set(sh::start_thumbs(h as isize));
                    ST.with_borrow_mut(|st| st.himl = himl as isize);
                    // the same columns, in the same order, as Explorer's details view
                    col(list, 0, t!("Имя", "Name", ja: "名前", pt: "Nome", zh: "名称"), ui::sc(360), false);
                    col(list, 1, t!("Дата изменения", "Date modified", ja: "更新日時", pt: "Data de modificação", zh: "修改日期"), ui::sc(160), false);
                    col(list, 2, t!("Тип", "Type", ja: "種類", pt: "Tipo", zh: "类型"), ui::sc(140), false);
                    col(list, 3, t!("Размер", "Size", ja: "サイズ", pt: "Tamanho", zh: "大小"), ui::sc(96), true);
                    ui::style_list(list);
                }
                ST.with_borrow_mut(|st| { st.list = list; st.tab = tab; st.edit = edit; st.tabs = vec![PathBuf::new()]; });
                if !desk() { ui::place_edit(tab); }
                let _ = sh::register_drop(list as _, h as isize);
                if desk() && icons_hidden() { ShowWindow(list, SW_HIDE); }
                // live wallpaper: the list is see-through (colour key), so over empty desktop OLE finds the wallpaper window under it,
                // and walks up from there to the desktop window: it must accept the drop too
                if desk() { let _ = sh::register_drop(h as _, h as isize); }
                let (start, pick) = if desk() { (desktop::desktop_dirs().into_iter().next().unwrap_or_default(), None) } else { start_target() };
                load(h, start);
                if let Some(name) = pick { select_row(&name); }
                if desk() {
                    desktop::register_resident();
                } else {
                    shellwin::navigated(&cur_dir());
                    shellwin::register(h as isize);
                }
                0
            }
            // the bar came, went or changed size (it sets the work area), or the screen changed: the icon list follows
            WM_SETTINGCHANGE | WM_DISPLAYCHANGE if desk() => {
                if m == WM_DISPLAYCHANGE {
                    reload_wall(); // monitors came, went or changed resolution: each one gets its picture again
                    let (vx, vy, vw, vh) = virt();
                    SetWindowPos(h, HWND_BOTTOM, vx, vy, vw, vh, SWP_NOACTIVATE); // WM_SIZE lays out
                    InvalidateRect(h, ptr::null(), 1);
                } else if wp == SPI_SETWORKAREA as usize {
                    place_desk_list(ST.with_borrow(|st| st.list));
                } else if wp == SPI_SETDESKWALLPAPER as usize {
                    wallpaper_changed(h); // Settings or the Photo viewer set a new picture
                }
                0
            }
            WM_WALL if desk() => {
                use windows_sys::Win32::Graphics::Gdi::{RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RedrawWindow};
                reload_wall();
                RedrawWindow(h, ptr::null(), ptr::null_mut(), RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN);
                0
            }
            // mouse buttons 4 / 5 (and keyboard media keys) arrive as APPCOMMAND_BROWSER_BACKWARD / _FORWARD
            0x0319 if !desk() && matches!((lp >> 16) & 0x7FF, 1 | 2) => { nav(h, (lp >> 16) & 0x7FF == 1); 1 }
            // the list reports its button downs here: middle click on a folder row opens it in a new tab
            WM_PARENTNOTIFY if !desk() && (wp & 0xFFFF) as u32 == WM_MBUTTONDOWN => {
                let list = ST.with_borrow(|st| st.list);
                let mut pt: POINT = std::mem::zeroed();
                GetCursorPos(&mut pt);
                if WindowFromPoint(pt) == list {
                    ScreenToClient(list, &mut pt);
                    let mut ht: LVHITTESTINFO = std::mem::zeroed();
                    ht.pt = pt;
                    let i = put(list, LVM_SUBITEMHITTEST, 0, &mut ht as *mut _ as isize);
                    let dir = ST.with_borrow(|st| usize::try_from(i).ok().and_then(|i| st.rows.get(i)).filter(|r| r.dir && !r.hdr).map(|r| st.dir.join(&r.name)));
                    if let Some(d) = dir { tab_open(d); }
                }
                0
            }
            WM_SIZE => {
                let list = ST.with_borrow(|st| st.list);
                let (cw, ch) = ((lp & 0xFFFF) as i32, ((lp >> 16) & 0xFFFF) as i32);
                if desk() {
                    place_desk_list(list);
                    desk::size_workerw(cw, ch);
                } else {
                    ui::layout_children(cw, ch);
                    InvalidateRect(h, ptr::null(), 0);
                }
                0
            }
            WM_PAINT if !desk() => { ui::paint_status(h); 0 }
            ui::WM_UI => {
                match wp {
                    1 => { let n = ST.with_borrow(|st| st.tabs.len()); if (lp as usize) < n { tab_select(h, lp as usize); } }
                    2 => tab_close_at(h, lp as usize),
                    3 => tab_new(),
                    4 => nav(h, true),
                    5 => nav(h, false),
                    6 => { if let Some(p) = cur_dir().parent() { load(h, p.to_path_buf()) } }
                    7 => reload(h),
                    8 => toggle_view(h),
                    9 => search_focus(),
                    10..=14 => {
                        let (list, dir, paths) = selected();
                        match wp {
                            10 | 11 if !paths.is_empty() => match real_paths(&paths) { Ok(p) => { let _ = sh::clip_set(h, &p, wp == 10 && !in_zip()); } Err(e) => warn(h, &e) },
                            12 => { paste(h, &dir); reload(h); }
                            13 if !in_zip() => {
                                let i = put(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
                                if i >= 0 { SetFocus(list); put(list, LVM_EDITLABELW, i as usize, 0); }
                            }
                            14 if !paths.is_empty() && !in_zip() => { let _ = sh::fileop(h, sh::Op::Delete, &paths, &dir); reload(h); }
                            _ => {}
                        }
                    }
                    20 => { if let Some(p) = ui::nav_path(lp as usize) { load(h, p); } } // a folder in the left pane
                    21 => { if let Some(p) = ui::crumb_path(lp as usize) { load(h, p); } } // a part of the path in the address pill
                    _ => {}
                }
                0
            }
            WM_ERASEBKGND if desk() && desk::erase(wp as _, h) => 1,
            WM_WINDOWPOSCHANGING if desk() => { // the desktop never rises above other windows
                let wp = &mut *(lp as *mut WINDOWPOS);
                wp.hwndInsertAfter = HWND_BOTTOM;
                wp.flags |= SWP_NOACTIVATE;
                0
            }
            WM_DRAWITEM | WM_MEASUREITEM if menu::message(h, m, lp) => 1,
            WM_NOTIFY => {
                let nm = &*(lp as *const NMHDR);
                let list = ST.with_borrow(|st| st.list);
                match nm.code {
                    NM_CUSTOMDRAW if nm.hwndFrom == list && !desk() => return ui::list_draw(lp), // the rows are painted by hand
                    LVN_ITEMCHANGED | LVN_ODSTATECHANGED => ui::status_dirty(h), // selection count in the status line
                    // typing a letter jumps to the next row that starts with it (#81): a virtual list asks us to search
                    LVN_ODFINDITEMW if nm.hwndFrom == list => {
                        let f = &*(lp as *const NMLVFINDITEMW);
                        if f.lvfi.flags & (LVFI_STRING | LVFI_PARTIAL) == 0 || f.lvfi.psz.is_null() { return -1; }
                        let mut n = 0;
                        while *f.lvfi.psz.add(n) != 0 { n += 1; }
                        let q = String::from_utf16_lossy(std::slice::from_raw_parts(f.lvfi.psz, n)).to_lowercase();
                        return ST.with_borrow(|st| {
                            let len = st.rows.len();
                            let start = usize::try_from(f.iStart).unwrap_or(0);
                            (0..len).map(|k| (start + k) % len).find(|&i| !st.rows[i].hdr && st.rows[i].name.to_lowercase().starts_with(&q)).map_or(-1, |i| i as isize)
                        });
                    }
                    LVN_GETDISPINFOW => {
                        let it = &mut (*(lp as *mut NMLVDISPINFOW)).item;
                        // a request that arrives while ST is borrowed is skipped; the list repaints later
                        ST.with(|c| {
                            let Ok(mut st) = c.try_borrow_mut() else { return };
                            let State { dir, rows, thumbs, .. } = &mut *st;
                            let Some(i) = usize::try_from(it.iItem).ok() else { return };
                            let Some(r) = rows.get_mut(i) else { return };
                            if it.mask & LVIF_TEXT != 0 && it.cchTextMax > 0 {
                                let s = if r.hdr { if it.iSubItem == 0 { r.name[1..].to_string() } else { String::new() } } else { match it.iSubItem { 0 => shown_name(&r.name, desk()), 1 => date_text(r.time), 2 => type_text(&r.name, r.dir), _ if r.dir && r.size == 0 => String::new(), _ => size_text(r.size) } };
                                let u: Vec<u16> = s.encode_utf16().take(it.cchTextMax as usize - 1).collect();
                                ptr::copy_nonoverlapping(u.as_ptr(), it.pszText, u.len());
                                *it.pszText.add(u.len()) = 0;
                            }
                            if r.hdr { if it.mask & LVIF_IMAGE != 0 { it.iImage = I_IMAGENONE; } return; }
                            if it.mask & LVIF_IMAGE != 0 && *thumbs {
                                if r.thumb == -2 {
                                    r.thumb = -1;
                                    if let Some(tx) = TX.get() { let _ = tx.send((GEN.load(std::sync::atomic::Ordering::Relaxed), i, dir.join(&r.name))); }
                                }
                                it.iImage = if r.thumb >= 0 { r.thumb } else { I_IMAGENONE };
                            } else if it.mask & LVIF_IMAGE != 0 {
                                if r.icon < 0 { r.icon = icon_of(&dir.join(&r.name), r.dir); }
                                it.iImage = r.icon;
                            }
                        });
                    }
                    LVN_ODFINDITEMW if nm.hwndFrom == list => {
                        // typing a name in the list selects the matching row (#130); -1 = no match, the selection stays
                        let f = &*(lp as *const NMLVFINDITEMW);
                        if f.lvfi.flags & LVFI_STRING == 0 || f.lvfi.psz.is_null() { return -1; }
                        let mut n = 0;
                        while *f.lvfi.psz.add(n) != 0 { n += 1; }
                        let typed = String::from_utf16_lossy(std::slice::from_raw_parts(f.lvfi.psz, n));
                        let start = usize::try_from(f.iStart).unwrap_or(0);
                        let hit = ST.with(|c| c.try_borrow().ok().and_then(|st| {
                            find_row(st.rows.len(), start, &typed, |i| st.rows.get(i).filter(|r| !r.hdr).map(|r| shown_name(&r.name, desk())))
                        }));
                        return hit.map_or(-1, |i| i as LRESULT);
                    }
                    LVN_ITEMACTIVATE => {
                        let i = (*(lp as *const NMITEMACTIVATE)).iItem;
                        let hit = ST.with_borrow(|st| usize::try_from(i).ok().and_then(|i| st.rows.get(i)).filter(|r| !r.hdr).map(|r| (st.dir.join(&r.name), r.dir)));
                        if let Some((p, is_dir)) = hit { activate(h, p, is_dir); }
                    }
                    // a double click on empty space goes up one folder, like QTTabBar (#139); not on the desktop
                    NM_DBLCLK if nm.hwndFrom == list && !desk() && (*(lp as *const NMITEMACTIVATE)).iItem < 0 => {
                        PostMessageW(h, WM_NAV, 4, 0);
                    }
                    NM_RCLICK if nm.hwndFrom == list => {
                        let (_, dir, paths) = selected();
                        if !paths.is_empty() { item_menu(h, &dir, &paths, false); } else { bg_menu(h); }
                    }
                    LVN_COLUMNCLICK => {
                        let c = (*(lp as *const NMLISTVIEW)).iSubItem as usize;
                        let asc = if SORT_COL.load(Relaxed) == c { !SORT_ASC.load(Relaxed) } else { true };
                        SORT_COL.store(c, Relaxed);
                        SORT_ASC.store(asc, Relaxed);
                        let n = ST.with_borrow_mut(|st| {
                            st.rows.retain(|r| !r.hdr);
                            sort_rows(&mut st.rows, c, asc);
                            add_headers(&mut st.rows);
                            st.rows.iter_mut().for_each(|r| r.thumb = -2); // rows moved: thumbnails are asked for again
                            st.rows.len()
                        });
                        reset_thumbs();
                        put(list, LVM_REDRAWITEMS, 0, n as isize - 1);
                    }
                    LVN_BEGINDRAG => {
                        let (_, _, paths) = selected();
                        let paths = match real_paths(&paths) { Ok(p) => p, Err(e) => { warn(h, &e); return 0; } }; // out of a zip: real files first
                        drag_log(&format!("BEGINDRAG desk={} selected={}", desk(), paths.len()));
                        if !paths.is_empty() {
                            if desk() { let p = (*(lp as *const NMLISTVIEW)).ptAction; desk::drag_start((p.x, p.y)); }
                            let r = sh::drag(h, &paths);
                            drag_log(&format!("drag finished: {r:?}"));
                            // dropped on the desktop itself: the icons were moved (or went into a folder icon), no reload needed
                            let handled = desk() && desk::drag_end(h, list);
                            drag_log(&format!("moved on the desktop: {handled}"));
                            if !handled { reload(h); }
                        }
                    }
                    LVN_KEYDOWN => {
                        let vk = (*(lp as *const NMLVKEYDOWN)).wVKey as u32;
                        let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
                        if desk() && matches!(vk, 0x08 | 0x46 | 0x4D | 0x54 | 0x57 | 0x09 | 0x4C | 0x73) { return 0; } // browsing keys make no sense on the desktop
                        let (list, dir, paths) = selected();
                        match vk {
                            0x08 => if let Some(p) = dir.parent() { load(h, p.to_path_buf()) },
                            0x1B if jobs::busy() => { jobs::cancel(); return 0; } // Esc stops packing / unpacking
                            0x2E if !paths.is_empty() && !in_zip() => { let _ = sh::fileop(h, sh::Op::Delete, &paths, &dir); }
                            // Space: a quick look. Pictures open in photoview, videos / text / anything else in its default app
                            0x20 if !desk() => if let Some(p) = paths.first().filter(|p| p.is_file()) { activate(h, p.clone(), false); },
                            0x71 if !in_zip() => {
                                let i = put(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
                                if i >= 0 { put(list, LVM_EDITLABELW, i as usize, 0); }
                            }
                            0x46 if ctrl => { search_focus(); return 0; } // Ctrl+F: address bar in search mode
                            0x73 | 0x4C if vk == 0x73 || ctrl => { // F4 / Ctrl+L: address bar
                                let edit = ST.with_borrow(|st| st.edit);
                                SetFocus(edit);
                                put(edit, EM_SETSEL, 0, -1);
                                return 0;
                            }
                            0x5D if !paths.is_empty() => item_menu(h, &dir, &paths, true),
                            0x43 | 0x58 if ctrl && !paths.is_empty() => { // from a zip: real files in the temp folder, and only a copy (nothing can be cut out of an archive)
                                match real_paths(&paths) { Ok(p) => { let _ = sh::clip_set(h, &p, vk == 0x58 && !in_zip()); } Err(e) => warn(h, &e) }
                                return 0;
                            }
                            0x41 if ctrl => { // Ctrl+A: select all (state mask LVIS_SELECTED, item -1 = every row)
                                let mut it: LVITEMW = std::mem::zeroed();
                                it.stateMask = LVIS_SELECTED;
                                it.state = LVIS_SELECTED;
                                put(list, LVM_SETITEMSTATE, usize::MAX, &it as *const _ as isize);
                                return 0;
                            }
                            0x56 if ctrl => paste(h, &dir),
                            0x4E if ctrl => new_entry(&dir, t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹"), true),
                            0x48 if ctrl => { SHOW_HIDDEN.fetch_xor(true, Relaxed); } // Ctrl+H: hidden files
                            0x74 => {} // F5: just the reload below
                            0x4D if ctrl => { toggle_view(h); return 0; } // Ctrl+M: details <-> thumbnails
                            0x54 if ctrl => tab_new(),
                            0x57 if ctrl => { tab_close(h); return 0; }
                            0x09 if ctrl => {
                                let next = ST.with_borrow(|st| (st.active + 1) % st.tabs.len());
                                tab_select(h, next);
                                return 0;
                            }
                            _ => return 0,
                        }
                        reload(h);
                    }
                    LVN_BEGINLABELEDITW => {
                        let hdr = ST.with_borrow(|st| usize::try_from((*(lp as *const NMLVDISPINFOW)).item.iItem).ok().and_then(|i| st.rows.get(i)).is_some_and(|r| r.hdr));
                        if hdr { return 1; } // a heading cannot be renamed
                        let e = SendMessageW(ST.with_borrow(|st| st.list), LVM_GETEDITCONTROL, 0, 0) as HWND;
                        if e != 0 as HWND { LABEL_OLD.store(SetWindowLongPtrW(e, GWLP_WNDPROC, label_proc as *const () as isize), std::sync::atomic::Ordering::Relaxed); }
                    }
                    LVN_ENDLABELEDITW => {
                        let it = &(*(lp as *const NMLVDISPINFOW)).item;
                        if !it.pszText.is_null() {
                            let new = wstr(it.pszText);
                            // reject separators so a rename can never move the item
                            if !new.is_empty() && !new.contains(['\\', '/', ':']) {
                                ST.with_borrow(|st| if let Some(r) = usize::try_from(it.iItem).ok().and_then(|i| st.rows.get(i)) { let _ = fs::rename(st.dir.join(&r.name), st.dir.join(&new)); });
                                PostMessageW(h, WM_APP, 0, 0); // reload after the edit control is gone
                            }
                        }
                    }
                    _ => {}
                }
                0
            }
            WM_SETFOCUS => { SetFocus(ST.with_borrow(|st| st.list)); 0 }
            WM_APP => { reload(h); 0 }
            shellwin::WM_SHELL_SELECT => { shell_select(h as isize, *Box::from_raw(lp as *mut PathBuf)); 0 }
            WM_JOB => {
                let err = *Box::from_raw(wp as *mut Option<String>);
                reload(h);
                ui::status_dirty(h);
                if let Some(e) = err { MessageBoxW(h, w(&e).as_ptr(), w("WinRust Explorer").as_ptr(), MB_ICONWARNING); }
                0
            }
            WM_NAV => {
                match wp {
                    1 => nav(h, true),
                    2 => nav(h, false),
                    3 => { let (_, dir, sel) = selected(); properties(&sel.first().cloned().unwrap_or(dir)); }
                    _ => if let Some(p) = cur_dir().parent() { load(h, p.to_path_buf()) },
                }
                0
            }
            WM_DIRSIZE => {
                let (era, name, n) = *Box::from_raw(wp as *mut (u32, String, u64));
                if era == GEN.load(Relaxed) {
                    let (list, row) = ST.with_borrow_mut(|st| (st.list, st.rows.iter_mut().position(|r| r.dir && r.name == name).inspect(|&i| st.rows[i].size = n)));
                    if let Some(i) = row { put(list, LVM_REDRAWITEMS, i, i as isize); }
                }
                0
            }
            WM_THUMB => {
                let (era, row, bmp) = *Box::from_raw(wp as *mut (u32, usize, isize));
                let (list, himl) = ST.with_borrow(|st| (st.list, st.himl));
                if era == GEN.load(std::sync::atomic::Ordering::Relaxed) {
                    let idx = ImageList_Add(himl as _, bmp as _, ptr::null_mut());
                    let ok = idx >= 0 && ST.with_borrow_mut(|st| st.rows.get_mut(row).map(|r| r.thumb = idx).is_some());
                    if ok { put(list, LVM_REDRAWITEMS, row, row as isize); }
                }
                windows_sys::Win32::Graphics::Gdi::DeleteObject(bmp as _);
                0
            }
            WM_GO => {
                let (edit, list) = ST.with_borrow(|st| (st.edit, st.list));
                if wp == 13 {
                    let mut buf = vec![0u16; GetWindowTextLengthW(edit) as usize + 1];
                    let n = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32) as usize;
                    let p = PathBuf::from(String::from_utf16_lossy(&buf[..n]).trim().trim_matches('"'));
                    let text = String::from_utf16_lossy(&buf[..n]);
                    if let Some(q) = text.trim().strip_prefix('?') {
                        if !q.trim().is_empty() { search(h, q.trim()); SetFocus(list); return 0; } // keep the query in the bar
                    } else if p.is_dir() || zipx::split_zip(&p).is_some() { load(h, p); } // also a path inside a .zip
                }
                SetWindowTextW(edit, w(&cur_dir().to_string_lossy()).as_ptr()); // also undoes a bad path / Esc
                SetFocus(list);
                0
            }
            // a click on the desktop itself, not on the icon list (which has its own NM_RCLICK menu): icons hidden, or
            // a secondary monitor, where the list does not reach (#125)
            WM_CONTEXTMENU if desk() && (icons_hidden() || wp as HWND == h) => { bg_menu(h); 0 }
            desk::SPAWN_WORKERW if desk() => { desk::spawn_workerw(h); 0 }
            WM_TIMER if wp == shellmenu::TIMER => { shellmenu::tick(h); 0 }
            WM_TIMER if desk() && wp == 7 => { desk::check_live(ST.with_borrow(|st| st.list)); 0 }
            WM_DESTROY => { if !desk() { save_place(h); } shellwin::revoke(); PostQuitMessage(0); 0 }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }
}

/// Control Panel, Recycle Bin, "This PC" arrive as shell addresses (`::{GUID}`, `shell:...`), not folders: not ours to show (#85, #80, #70).
fn namespace_target() -> Option<String> {
    env::args().skip(1).find_map(|a| {
        let t = a.trim_matches('"').rsplit(',').next().unwrap_or("").to_string(); // `/e,::{...}`
        let l = t.to_lowercase();
        (l.starts_with("::{") || l.starts_with("shell:")).then_some(t)
    })
}

fn main() {
    if let Some(t) = namespace_target() {
        // control.exe opens its own window; explorer.exe given a namespace address opens it directly (no folder verb involved)
        // Only the "open" verb of folders is ours: control.exe and explorer.exe <address> both come back here through it
        // (an endless control.exe <-> rust-explorer loop, flickering cursor). The "explore" verb still has Explorer's own
        // handler, so it shows the panel / Recycle Bin in a real Explorer window. The hop mark stops any loop that remains.
        if env::var_os("RX_NS_HOP").is_none() {
            let addr = if t.starts_with("::") { format!("shell:{t}") } else { t };
            unsafe { env::set_var("RX_NS_HOP", "1") }; // inherited by whatever the shell starts from here
            let (f, v) = (w(&addr), w("explore"));
            let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
            sei.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
            sei.fMask = 0x0000_0100 | 0x0000_0400; // SEE_MASK_NOASYNC (we exit right after) | SEE_MASK_FLAG_NO_UI
            sei.lpVerb = v.as_ptr();
            sei.lpFile = f.as_ptr();
            sei.nShow = SW_SHOWNORMAL;
            unsafe { ShellExecuteExW(&mut sei) };
        }
        return;
    }
    sh::init();
    unsafe {
        SetProcessDPIAware(); // sharp text at any scaling; every size below goes through ui::sc
        ui::init_scale();
        ui::gdiplus_start();
        ui::dark_mode();
        InitCommonControlsEx(&INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_LISTVIEW_CLASSES });
        let inst = GetModuleHandleW(ptr::null());
        let cls = w("RustExplorer");
        let desktop = env::args().nth(1).as_deref() == Some("--desktop");
        // desktop without a wallpaper file shows black, not window white
        // desktop: COLOR_BACKGROUND, the user's solid colour when there is no picture, never black (#108, #111)
        let bg = if desktop { windows_sys::Win32::Graphics::Gdi::GetSysColorBrush(1) as _ } else { windows_sys::Win32::Graphics::Gdi::CreateSolidBrush(ui::pal().bg) };
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(ui::proc), hInstance: inst, lpszClassName: w("RxChrome").as_ptr(), hCursor: LoadCursorW(0 as _, IDC_ARROW), ..std::mem::zeroed() });
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(ui::nav_proc), hInstance: inst, lpszClassName: w("RxNav").as_ptr(), hCursor: LoadCursorW(0 as _, IDC_ARROW), ..std::mem::zeroed() });
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst, lpszClassName: cls.as_ptr(), hCursor: LoadCursorW(0 as _, IDC_ARROW), hIcon: LoadIconW(inst, 1 as _), hbrBackground: bg, ..std::mem::zeroed() });
        if desktop {
            DESKTOP.store(true, Relaxed);
            desktop::create_window(inst, bg);
        } else {
            // where it was last closed, maximized again if it was (#71); a first start gets the default size
            let win = CreateWindowExW(0, cls.as_ptr(), ptr::null(), WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, CW_USEDEFAULT, CW_USEDEFAULT, ui::sc(1000), ui::sc(660), 0 as _, 0 as _, inst, ptr::null());
            match saved_place() {
                // the same API that saved it: rcNormalPosition is in work-area coordinates, which CreateWindow does not use
                Some((max, r)) => {
                    let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
                    wp.length = size_of::<WINDOWPLACEMENT>() as u32;
                    wp.showCmd = if max { SW_SHOWMAXIMIZED } else { SW_SHOWNORMAL } as u32;
                    wp.rcNormalPosition = r;
                    SetWindowPlacement(win, &wp);
                }
                None => { ShowWindow(win, SW_SHOWNORMAL); }
            }
        }
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
            // Alt+Left / Right / Up / Enter arrive as WM_SYSKEYDOWN at the list, which never reports them: handle them here
            if msg.message == WM_SYSKEYDOWN && !desk() {
                let act = match msg.wParam { 0x25 => 1, 0x27 => 2, 0x0D => 3, 0x26 => 4, _ => 0 };
                if act != 0 { PostMessageW(GetAncestor(msg.hwnd, GA_ROOT), WM_NAV, act, 0); continue; }
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn typing_a_name_finds_the_row_not_the_first_one() {
        let rows = [None, Some("Apps"), Some("Documentos"), Some("docs.txt"), None, Some("Música"), Some("zip")];
        let at = |i: usize| rows[i].map(str::to_string);
        let find = |start, typed| super::find_row(rows.len(), start, typed, at);
        assert_eq!(find(0, "z"), Some(6)); // used to land on row 0
        assert_eq!(find(0, "DOC"), Some(2)); // case is ignored
        assert_eq!(find(3, "doc"), Some(3)); // the search starts at the given row ...
        assert_eq!(find(4, "doc"), Some(2)); // ... and wraps round to the top
        assert_eq!(find(0, "mú"), Some(5)); // accents are plain characters
        assert_eq!(find(0, "x"), None); // no match: the selection stays
        assert_eq!(find(0, ""), None);
        assert_eq!(find(99, "a"), Some(1)); // a stale start row is clamped
        assert_eq!(super::find_row(0, 0, "a", |_| None), None);
    }

    #[test]
    fn window_place_parses_and_rejects_junk() {
        let (max, r) = super::parse_place("max 10 20 1010 680").unwrap();
        assert!(max);
        assert_eq!((r.left, r.top, r.right, r.bottom), (10, 20, 1010, 680));
        assert!(!super::parse_place("normal -1900 5 -900 700").unwrap().0); // a monitor left of the primary
        assert!(super::parse_place("max 0 0 50 50").is_none()); // collapsed
        assert!(super::parse_place("max 0 0 800").is_none());
        assert!(super::parse_place("big 0 0 800 600").is_none());
        assert!(super::parse_place("").is_none());
    }

    #[test]
    fn a_zip_opens_like_a_folder_and_gives_real_files() {
        let d = std::env::temp_dir().join(format!("rx-zipview-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("f.txt"), "hello").unwrap();
        let z = d.join("a.zip");
        let mut w = zipx::Writer::new(fs::File::create(&z).unwrap());
        w.add_prepared(zipx::prepare("inner/f.txt".into(), &d.join("f.txt")).unwrap()).unwrap();
        w.add_prepared(zipx::prepare("top.txt".into(), &d.join("f.txt")).unwrap()).unwrap();
        w.finish().unwrap();
        let names = |dir: &PathBuf| rows_of(dir).unwrap().into_iter().map(|r| format!("{}{}", r.name, if r.dir { "/" } else { "" })).collect::<Vec<_>>();
        assert_eq!(names(&z), ["inner/", "top.txt"]); // folders first
        assert_eq!(names(&z.join("inner")), ["f.txt"]);
        assert!(rows_of(&d.join("nothing.zip")).is_none());
        let paths = [z.join("inner").join("f.txt"), z.join("top.txt")];
        let real = real_paths(&paths).unwrap();
        assert!(real.iter().all(|p| p.is_file() && fs::read_to_string(p).unwrap() == "hello"), "{real:?}");
        assert_eq!(real_paths(&[d.join("f.txt")]).unwrap(), [d.join("f.txt")], "ordinary files stay as they are");
        assert!(zip_names(&[z.join("top.txt"), d.join("f.txt")]).is_none(), "mixed selections are not archive selections");
        let _ = fs::remove_dir_all(&d);
        let _ = fs::remove_dir_all(real[0].parent().unwrap());
    }

    use super::*;

    fn r(name: &str, dir: bool, size: u64, time: u64) -> Row { Row { name: name.into(), dir, size, time, icon: -1, thumb: -2 , hdr: false } }
    fn names(v: &[Row]) -> Vec<&str> { v.iter().map(|r| r.name.as_str()).collect() }

    #[test]
    fn desktop_rows_show_file_names_without_shortcut_extensions() {
        assert_eq!(shown_name(r"C:\Users\Public\Desktop\Steam.lnk", true), "Steam");
        assert_eq!(shown_name("Notes.txt", true), "Notes.txt");
        assert_eq!(shown_name(r"C:\x\Game.URL", true), "Game");
        assert_eq!(shown_name(r"C:\x\Steam.lnk", false), r"C:\x\Steam.lnk");
    }

    #[test]
    fn folders_first_then_by_the_chosen_column() {
        let mut v = vec![r("b.txt", false, 5, 1), r("A", true, 0, 2), r("a.txt", false, 9, 3)];
        sort_rows(&mut v, 0, true);
        assert_eq!(names(&v), ["A", "a.txt", "b.txt"]);
        sort_rows(&mut v, 3, false); // biggest first, the folder still on top
        assert_eq!(names(&v), ["A", "a.txt", "b.txt"]);
        sort_rows(&mut v, 1, true); // oldest first
        assert_eq!(names(&v), ["A", "b.txt", "a.txt"]);
    }

    #[test]
    fn search_hit_paths_sort_by_file_name() {
        let mut v = vec![r("C:\\z\\b.txt", false, 1, 1), r("D:\\a\\a.txt", false, 1, 1)];
        sort_rows(&mut v, 0, true);
        assert_eq!(names(&v), ["D:\\a\\a.txt", "C:\\z\\b.txt"]);
    }

    #[test]
    fn history_goes_back_and_forward_and_a_new_visit_drops_the_forward_trail() {
        let mut h = Hist::default();
        for p in ["a", "b", "c"] { h.visit(p.into()); }
        assert_eq!(h.back(), Some("b".into()));
        assert_eq!(h.back(), Some("a".into()));
        assert_eq!(h.back(), None);
        assert_eq!(h.forward(), Some("b".into()));
        h.visit("x".into()); // from b: c is no longer ahead
        assert_eq!(h.forward(), None);
        assert_eq!(h.back(), Some("b".into()));
        h.visit("x".into()); h.visit("x".into()); // same folder twice is one step
        assert_eq!(h.v.len(), 3);
    }

    #[test]
    fn type_and_size_read_like_explorers() {
        // the tests run in Russian (lang!() in a test build), like Explorer's own Type column
        assert_eq!(type_text("x", true), "Папка с файлами");
        assert_eq!(type_text("a.txt", false), "Файл TXT");
        assert_eq!(type_text("C:\\d\\b.tar.gz", false), "Файл GZ");
        assert_eq!(type_text(".gitignore", false), "Файл"); // a dot file has no extension
        assert_eq!(type_text("Makefile", false), "Файл");
        assert_eq!(size_text(1), "1 KB");
        assert_eq!(size_text(2048), "2 KB");
        assert_eq!(size_text(5 << 20), "5.0 MB");
        assert_eq!(size_text(3 << 30), "3.00 GB");
    }

    #[test]
    fn dates_are_formatted_and_zero_is_blank() {
        assert_eq!(date_text(0), "");
        let t = date_text(132_000_000_000_000_000); // some day in 2019, local time zone
        assert!(t.len() == 16 && t.starts_with("20"), "{t}");
    }
}

#[cfg(test)]
mod search_tests {
    #[test]
    fn local_search_finds_names_below_the_folder() {
        let d = std::env::temp_dir().join(format!("rx-search-test-{}", std::process::id()));
        std::fs::create_dir_all(d.join(r"sub\deep")).unwrap();
        for f in [r"Report.txt", r"sub\notes.md", r"sub\deep\REPORT-2.pdf"] { std::fs::write(d.join(f), "x").unwrap(); }
        let (rows, cut, flipped) = super::local_search(&d, "report");
        let (wrong, _, was_flipped) = super::local_search(&d, "кузщке"); // "report" typed in the Russian layout
        let mut names: Vec<String> = rows.iter().map(|r| std::path::Path::new(&r.name).file_name().unwrap().to_string_lossy().into_owned()).collect();
        names.sort();
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(names, ["REPORT-2.pdf", "Report.txt"]); // any case, any depth, absolute paths
        assert!(!cut && !flipped);
        assert!(was_flipped && wrong.len() == 2);
    }

    #[test]
    fn layout_flip_both_ways() {
        assert_eq!(super::flip_layout("сфкпщ"), "cargo");
        assert_eq!(super::flip_layout("ljrevtyns"), "документы");
        assert_eq!(super::flip_layout("Ыфму"), "Save");
    }
}
