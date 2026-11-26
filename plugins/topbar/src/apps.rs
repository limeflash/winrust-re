//! What the Start menu and the search box show, from the machine: the shortcuts of the Start menu and the shell's Apps
//! folder, what the user pinned, how a typed query ranks them, the recent files, the account, the power actions.
//! (Split out of `fly.rs`, where the old Start panel kept it; the panels themselves are `ui::start_panel` and `ui::search_panel`.)
use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering::Relaxed as Rlx};

/// An app of the Start menu: its name and what to open (a shortcut, an exe, or `shell:AppsFolder\<id>` for a Store app),
/// NUL-terminated.
#[derive(Clone, PartialEq)]
pub struct App {
    pub name: String,
    pub path: Vec<u16>,
}

static APPS: Mutex<Vec<(String, Vec<u16>)>> = Mutex::new(Vec::new());
static PINNED: Mutex<Vec<String>> = Mutex::new(Vec::new()); // paths of .lnk / .exe files, in the order they were added
static JUST: Mutex<(String, u64)> = Mutex::new((String::new(), 0)); // the app pinned last and when: its tile pulses
static NOTE: Mutex<(String, u64)> = Mutex::new((String::new(), 0)); // a line for the Start menu's footer and when it was set
static ICONS: Mutex<Option<HashMap<Vec<u16>, usize>>> = Mutex::new(None); // by path, as HICON values: the list is reloaded and reordered
static EXE_GEN: AtomicU32 = AtomicU32::new(0);
/// Counts changes of the app list from a background thread (a loose exe found): a panel that sees it change repaints.
pub static GEN: AtomicU32 = AtomicU32::new(0);
static ICON_PX: AtomicI32 = AtomicI32::new(64);

/// The size app icons are loaded at, in device pixels: the largest the panels draw (44 dip) at the screen's scale.
pub fn set_icon_px(px: i32) {
    ICON_PX.store(px.max(48), Rlx);
}

fn pin_file() -> PathBuf {
    appdata_file("topbar-start.txt")
}

pub fn pinned_load() {
    *PINNED.lock().unwrap() = std::fs::read_to_string(pin_file()).map(|s| s.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default();
}

// ---------- the app list ----------

/// Background: collects the Start menu shortcuts and the Store apps, then warms the icons of what is pinned.
pub fn load() {
    fn walk(dir: &std::path::Path, depth: u8, out: &mut Vec<(String, Vec<u16>)>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() && depth < 4 {
                walk(&p, depth + 1, out);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if !name.to_lowercase().contains("uninstall") {
                    out.push((name, wide(&p.to_string_lossy())));
                }
            }
        }
    }
    let mut v = Vec::new();
    for (var, sub) in [("ProgramData", r"Microsoft\Windows\Start Menu\Programs"), ("APPDATA", r"Microsoft\Windows\Start Menu\Programs")] {
        if let Some(b) = std::env::var_os(var) {
            walk(&PathBuf::from(b).join(sub), 0, &mut v);
        }
    }
    // Store apps (Settings, Calculator, Terminal, Photos...) have no .lnk: the shell's Apps folder lists them
    let have: std::collections::HashSet<String> = v.iter().map(|a| a.0.to_lowercase()).collect();
    for (name, id) in apps_folder() {
        let ok = id.contains('!') || id.to_lowercase().ends_with(".exe") || id.starts_with("Microsoft.Windows."); // apps, not docs / web links
        if ok && !have.contains(&name.to_lowercase()) && !name.to_lowercase().contains("uninstall") {
            v.push((name, wide(&format!(r"shell:AppsFolder\{id}"))));
        }
    }
    v.sort_by_key(|a| a.0.to_lowercase());
    v.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    *APPS.lock().unwrap() = v;
    GEN.fetch_add(1, Rlx);
    ui::panel::refresh(); // a panel that opened before the list was ready fills in
    // the grid is drawn the moment the menu opens: its first page's icons should be there (all apps' when nothing is pinned)
    let first = pinned();
    let first = if first.is_empty() { all().into_iter().take(12).collect() } else { first };
    for app in first {
        icon(&app.path);
    }
    avatar();
}

/// (display name, app id) of everything in the shell's Apps folder (what the Windows Start menu lists).
pub fn apps_folder() -> Vec<(String, String)> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT, SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING},
    };
    let mut out = vec![];
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); // this runs on its own thread
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else { return out };
        let Ok(en) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut items = [None];
            let mut got = 0u32;
            if en.Next(&mut items, Some(&mut got)).is_err() || got == 0 {
                break;
            }
            let Some(it) = items[0].take() else { break };
            let name = |k: SIGDN| {
                it.GetDisplayName(k).ok().map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                })
            };
            if let (Some(n), Some(id)) = (name(SIGDN_NORMALDISPLAY), name(SIGDN_PARENTRELATIVEPARSING)) {
                out.push((n, id));
            }
        }
    }
    out
}

const EN_KEYS: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`";
const RU_KEYS: &str = "йцукенгшщзхъфывапролджэячсмитьбюё";

/// The same keys typed in the other layout: "сркщьу" -> "chrome", "ntktuhfv" -> "телеграм".
pub fn flip_layout(q: &str) -> String {
    let (en, ru): (Vec<char>, Vec<char>) = (EN_KEYS.chars().collect(), RU_KEYS.chars().collect());
    q.chars().map(|c| en.iter().position(|&x| x == c).map(|i| ru[i]).or_else(|| ru.iter().position(|&x| x == c).map(|i| en[i])).unwrap_or(c)).collect()
}

/// How well an app name matches the query (both lower-case): 0 the name starts with it, 1 a word does, 2 the word
/// initials do ("vsc" = Visual Studio Code), 3 it is somewhere inside; None = no match.
pub fn score(name: &str, q: &str) -> Option<u8> {
    if q.is_empty() {
        return None;
    }
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if name.starts_with(q) {
        Some(0)
    } else if words.iter().any(|w| w.starts_with(q)) {
        Some(1)
    } else if q.chars().count() >= 2 && initials.starts_with(q) {
        Some(2)
    } else if name.contains(q) {
        Some(3)
    } else {
        None
    }
}

/// The apps a query finds, best match first, then the most launched, then A to Z. Typed in the wrong layout, they come after
/// the direct matches.
pub fn search(q: &str) -> Vec<App> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let fq = flip_layout(&q);
    let apps = APPS.lock().unwrap();
    let mut v: Vec<(u8, u32, usize)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let n = a.0.to_lowercase();
            let s = score(&n, &q).or_else(|| score(&n, &fq).map(|s| s + 4))?;
            Some((s, u32::MAX - uses(&n), i))
        })
        .collect();
    v.sort_unstable(); // the list is sorted by name, so the index breaks the last tie
    v.into_iter().map(|x| App { name: apps[x.2].0.clone(), path: apps[x.2].1.clone() }).collect()
}

/// Every app, A to Z.
pub fn all() -> Vec<App> {
    APPS.lock().unwrap().iter().map(|a| App { name: a.0.clone(), path: a.1.clone() }).collect()
}

/// What the user pinned to Start, in the order they pinned it (an app that is not a Start menu shortcut joins the list).
pub fn pinned() -> Vec<App> {
    let pinned = PINNED.lock().unwrap().clone();
    let mut apps = APPS.lock().unwrap();
    pinned
        .iter()
        .map(|p| {
            let w = wide(p);
            let i = apps.iter().position(|a| a.1 == w).unwrap_or_else(|| {
                let stem = std::path::Path::new(p).file_stem().unwrap_or_default().to_string_lossy().to_string();
                let name: String = stem.chars().take(1).flat_map(char::to_uppercase).chain(stem.chars().skip(1)).collect();
                apps.push((name, w));
                apps.len() - 1
            });
            App { name: apps[i].0.clone(), path: apps[i].1.clone() }
        })
        .collect()
}

pub fn start_pinned(path: &str) -> bool {
    PINNED.lock().unwrap().iter().any(|p| p.eq_ignore_ascii_case(path))
}

/// Adds the app to the Start menu, or removes it when it is already there.
pub fn start_toggle(path: &str) {
    let mut p = PINNED.lock().unwrap();
    match p.iter().position(|x| x.eq_ignore_ascii_case(path)) {
        Some(i) => drop(p.remove(i)),
        None => p.push(path.to_string()),
    }
    let _ = std::fs::write(pin_file(), p.join("\n"));
    GEN.fetch_add(1, Rlx); // an open Start menu takes the new list
}

/// Adds the app to the Start menu unless it is already there (a drop on the Start button or the panel). true = it was added.
pub fn start_pin(path: &str) -> bool {
    if start_pinned(path) {
        return false;
    }
    start_toggle(path);
    *JUST.lock().unwrap() = (path.to_string(), unsafe { now_ms() });
    true
}

/// Files dropped on the Start menu or on its button: pins them and says so in the footer.
pub fn pin_dropped(files: &[String]) {
    let added: Vec<&String> = files.iter().filter(|f| start_pin(f)).collect();
    let name = |p: &str| std::path::Path::new(p).file_stem().unwrap_or_default().to_string_lossy().to_string();
    note(&match added.len() {
        0 => t!("Уже в Пуске", "Already in Start", ja: "すでにスタートにあります", pt: "Já está no Iniciar", zh: "已在开始菜单中").to_string(),
        1 => format!("Added to Start: {}", name(added[0])),
        n => format!("Added {n} items to Start"),
    });
}

/// The app pinned last, if it was pinned a moment ago, and how many ms ago.
pub fn just_pinned() -> Option<(Vec<u16>, u64)> {
    let j = JUST.lock().unwrap();
    let age = unsafe { now_ms() }.saturating_sub(j.1);
    (!j.0.is_empty() && age < 1200).then(|| (wide(&j.0), age))
}

/// A short line for the Start menu's footer ("Added to Start: ...").
pub fn note(s: &str) {
    *NOTE.lock().unwrap() = (s.to_string(), unsafe { now_ms() });
}

/// The footer's line while it is fresh.
pub fn current_note() -> Option<String> {
    let n = NOTE.lock().unwrap();
    (!n.0.is_empty() && unsafe { now_ms() }.saturating_sub(n.1) < 3500).then(|| n.0.clone())
}

/// Programs that have no Start menu shortcut (a loose loliland.exe): asks rust-search for .exe files matching what is typed
/// and adds them to the app list, where the normal ranking picks them up. Debounced; panels repaint when they arrive.
pub fn find_loose_exes(q: &str) {
    let q = q.trim().to_string();
    let gen_ = EXE_GEN.fetch_add(1, Rlx) + 1;
    let rs = PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_default()).join(r"rs-search\rs.exe");
    if q.chars().count() < 2 || !rs.is_file() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if EXE_GEN.load(Rlx) != gen_ {
            return; // typed on: a newer search is coming
        }
        // straight to the service's pipe: no rs.exe process per query (and no full disk scan when the service is down)
        let found = winrust_common::search::ask_all(&q);
        if EXE_GEN.load(Rlx) != gen_ {
            return; // the answer took long enough for the query to move on: its exes are not what is typed now
        }
        let mut apps = APPS.lock().unwrap();
        let mut added = false;
        for line in found.iter().filter(|l| l.to_lowercase().ends_with(".exe")).take(12) {
            let w = wide(line);
            if apps.iter().any(|a| a.1 == w) {
                continue;
            }
            let name = std::path::Path::new(line).file_stem().unwrap_or_default().to_string_lossy().to_string();
            apps.push((name, w));
            added = true;
        }
        drop(apps);
        if added {
            GEN.fetch_add(1, Rlx);
            ui::panel::refresh();
        }
    });
}

// ---------- icons ----------

/// The app's icon, loaded once (the shell's, at the size the panels draw it); null if it has none.
pub fn icon(path: &[u16]) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    if let Some(&h) = ICONS.lock().unwrap().as_ref().and_then(|m| m.get(path)) {
        return wicon(h as HICON);
    }
    // SAFETY: shell calls on this thread; the icon is kept for the life of the process.
    let ic = unsafe { lnk_icon(path, &lnk_target(path), ICON_PX.load(Rlx)) };
    wicon(keep_icon(path.to_vec(), ic))
}

/// Puts a loaded icon in the cache, or keeps the one another thread put there meanwhile (and frees this one).
fn keep_icon(path: Vec<u16>, ic: HICON) -> HICON {
    let mut m = ICONS.lock().unwrap();
    let kept = *m.get_or_insert_with(HashMap::new).entry(path).or_insert(ic as usize) as HICON;
    if kept != ic && !ic.is_null() {
        // SAFETY: our own icon, never handed out.
        unsafe { DestroyIcon(ic) };
    }
    kept
}

static ICON_QUEUE: Mutex<Vec<Vec<u16>>> = Mutex::new(Vec::new()); // paths the icon thread loads next, in order
static ICON_THREAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The app's icon if it is loaded already. A panel that pages through many apps draws with this and hands the rest to
/// [`warm_icons`]: one icon through the shell takes milliseconds, and the twelve of a page loaded in the first frame of its
/// slide stalled the slide.
pub fn icon_ready(path: &[u16]) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    ICONS.lock().unwrap().as_ref().and_then(|m| m.get(path)).map(|&h| wicon(h as HICON))
}

/// Loads the icons of `paths` that are not loaded yet, in this order, on a thread of its own; the open panel repaints as they
/// come in.
pub fn warm_icons(paths: impl IntoIterator<Item = Vec<u16>>) {
    let mut q = ICON_QUEUE.lock().unwrap();
    {
        let have = ICONS.lock().unwrap();
        for p in paths {
            if !have.as_ref().is_some_and(|m| m.contains_key(&p)) && !q.contains(&p) {
                q.push(p);
            }
        }
    }
    // decided under the queue's lock: the thread only gives up while holding it, so no path is left behind
    if q.is_empty() || ICON_THREAD.swap(true, Rlx) {
        return;
    }
    drop(q);
    std::thread::spawn(|| {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
        // SAFETY: COM for the shell's icon calls on this thread, ended when it is done.
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let mut n = 0u32;
        loop {
            let next = {
                let mut q = ICON_QUEUE.lock().unwrap();
                if q.is_empty() {
                    ICON_THREAD.store(false, Rlx);
                    None
                } else {
                    Some(q.remove(0))
                }
            };
            let Some(p) = next else { break };
            if icon_ready(&p).is_none() {
                // SAFETY: shell calls on this thread (COM is up); the icon is kept for the life of the process.
                let ic = unsafe { lnk_icon(&p, &lnk_target(&p), ICON_PX.load(Rlx)) };
                keep_icon(p, ic);
                n += 1;
                if n % 6 == 0 {
                    ui::panel::refresh(); // half a page at a time: icons appear while the rest load
                }
            }
        }
        ui::panel::refresh();
        if com {
            // SAFETY: pairs the CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
    });
}

/// A shell image of a file: its thumbnail when available, else its icon. Size in device pixels of what came back.
pub struct Image {
    pub icon: windows::Win32::UI::WindowsAndMessaging::HICON,
    pub w: i32,
    pub h: i32,
    pub thumb: bool,
}

/// What the shell gave for a path: the icon as a handle value (0 = none), its size and whether it is a thumbnail.
type Shot = (usize, i32, i32, bool);
static IMAGES: Mutex<Option<HashMap<String, Shot>>> = Mutex::new(None);

pub fn image(path: &str) -> Option<Image> {
    if let Some(&(h, w, ht, thumb)) = IMAGES.lock().unwrap().as_ref().and_then(|m| m.get(path)) {
        return (h != 0).then(|| Image { icon: wicon(h as HICON), w, h: ht, thumb });
    }
    if on_network(path) {
        return None; // the shell would wait for the share on this thread (once per frame, if it is down)
    }
    let px = ICON_PX.load(Rlx);
    // SAFETY: shell calls on this thread.
    let got = unsafe { shell_image(path, px, true).map(|(i, w, h)| (i, w, h, true)).or_else(|| shell_image(path, px, false).map(|(i, w, h)| (i, w, h, false))) };
    let (h, w, ht, thumb) = got.map_or((0, 0, 0, false), |(i, w, h, t)| (i as usize, w, h, t));
    let mut all = IMAGES.lock().unwrap();
    let all = all.get_or_insert_with(HashMap::new);
    if all.len() >= 256 {
        // the paths of recent files and search results come and go: start over rather than keep every icon for the whole session
        for (_, (old, ..)) in all.drain().filter(|(_, v)| v.0 != 0) {
            // SAFETY: icons made by `shell_image`, owned by this cache; the renderer drops its bitmaps of them first.
            unsafe {
                ui::gfx::retire_icon(wicon(old as HICON));
                DestroyIcon(old as HICON);
            }
        }
    }
    all.insert(path.to_string(), (h, w, ht, thumb));
    (h != 0).then(|| Image { icon: wicon(h as HICON), w, h: ht, thumb })
}

/// The shell's picture of a file: the cached thumbnail (`thumb`) or the icon. The icon and the size the shell made it at.
unsafe fn shell_image(path: &str, px: i32, thumb: bool) -> Option<(HICON, i32, i32)> {
    use windows::{
        Win32::{
            Foundation::SIZE,
            UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY, SIIGBF_INCACHEONLY, SIIGBF_THUMBNAILONLY},
        },
        core::Interface as _,
    };
    let item = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide(path).as_ptr()), None).ok()?;
    let f = item.cast::<IShellItemImageFactory>().ok()?;
    // only a cached thumbnail: making one here would stall the frame that asked (search results, Start); the pictures that must
    // show without a cache are decoded by the panel itself (`Canvas::image`)
    let flags = if thumb { SIIGBF_THUMBNAILONLY | SIIGBF_INCACHEONLY } else { SIIGBF_ICONONLY };
    let bmp = f.GetImage(SIZE { cx: px, cy: px }, flags).ok()?;
    let color = bmp.0 as HBITMAP;
    let mut info: BITMAP = zeroed();
    GetObjectW(color, size_of::<BITMAP>() as i32, &mut info as *mut _ as *mut _);
    let (w, h) = (info.bmWidth.max(1), info.bmHeight.max(1));
    let mask = CreateBitmap(w, h, 1, 1, null()); // ignored: the 32-bit colour bitmap carries the alpha
    let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let ic = CreateIconIndirect(&ii);
    DeleteObject(mask);
    DeleteObject(color);
    (!ic.is_null()).then_some((ic, w, h))
}

// ---------- launching ----------

/// Starts an app and counts the launch (the search ranks by it).
pub fn launch(app: &App) {
    if wstr_of(&app.path).starts_with("shell:") {
        bump_name(&app.name); // open() counts only .lnk files
    }
    // SAFETY: `open` hands the path to the shell.
    unsafe { open(&app.path) };
}

pub fn launch_admin(app: &App) {
    // SAFETY: a shell call with NUL-terminated strings.
    unsafe { ShellExecuteW(null_mut(), wide("runas").as_ptr(), app.path.as_ptr(), null(), null(), SW_SHOWNORMAL) };
}

/// Shows the shortcut's folder (Store apps have no file to show).
pub fn reveal(app: &App) {
    winrust_common::reveal(&wstr_of(&app.path));
}

/// An app that has a file to run elevated or to show in a folder (not a Store app).
pub fn is_file(app: &App) -> bool {
    !wstr_of(&app.path).starts_with("shell:")
}

// ---------- recent files ----------

pub struct Recent {
    pub name: String,
    pub path: String,
    /// "Today, 08:12", "Yesterday, 18:20" or the date.
    pub when: String,
    pub day: usize,
    /// A folder, looked up once here: drawing must not touch the disk (it can be a network share that went away).
    pub dir: bool,
}

/// The files and folders the user opened last, newest first: the shortcuts of the shell's Recent folder, resolved.
pub fn recent(n: usize) -> Vec<Recent> {
    let Some(dir) = recent_dir() else { return Vec::new() };
    let mut lnks: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    lnks.sort_by_key(|l| std::cmp::Reverse(l.0));
    let mut out: Vec<Recent> = Vec::new();
    for (t, lnk) in lnks.into_iter().take(120) {
        let Some((target, was_dir)) = link_info(&lnk) else { continue };
        let tp = std::path::Path::new(&target);
        // a share that is down would hold the bar for the length of the network's timeout, once per file: the shortcut's own
        // record says whether it is a folder, and a file on a share is taken to still be there
        let net = on_network(&target);
        if (!net && !tp.exists()) || out.iter().any(|r| r.path.eq_ignore_ascii_case(&target)) {
            continue;
        }
        let name = tp.file_name().map_or_else(|| target.clone(), |n| n.to_string_lossy().to_string());
        let dir = if net { was_dir } else { tp.is_dir() };
        let (when, day) = when_label(t);
        let when = if dir { format!("{} · {when}", t!("Папка", "Folder", ja: "フォルダー", pt: "Pasta", zh: "文件夹")) } else { when };
        out.push(Recent { name, path: target, when, day, dir });
        if out.len() == n {
            break;
        }
    }
    out
}

fn recent_dir() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Recent, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: the returned shell string is freed once.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Recent, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().unwrap_or_default();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        Some(PathBuf::from(s))
    }
}

/// Remove all shortcuts to this target so a duplicate cannot reappear on the next refresh.
pub fn remove_recent(target: &str) -> bool {
    recent_dir().is_some_and(|dir| remove_recent_in(&dir, target))
}

fn remove_recent_in(dir: &std::path::Path, target: &str) -> bool {
    remove_recent_with(dir, target, link_path)
}

fn remove_recent_with(dir: &std::path::Path, target: &str, resolve: impl Fn(&std::path::Path) -> Option<String>) -> bool {
    let mut removed = false;
    let mut failed = false;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) && resolve(&path).is_some_and(|p| p.eq_ignore_ascii_case(target)) {
            if std::fs::remove_file(path).is_ok() { removed = true; } else { failed = true; }
        }
    }
    removed && !failed
}

/// What a shortcut points at, as written in it (not lower-cased).
fn link_path(lnk: &std::path::Path) -> Option<String> {
    link_info(lnk).map(|i| i.0)
}

/// What a shortcut points at, and whether it pointed at a folder when the shortcut was written (read from the shortcut, the
/// target is not touched).
fn link_info(lnk: &std::path::Path) -> Option<(String, bool)> {
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    let w = wide(&lnk.to_string_lossy());
    // SAFETY: COM on this thread; the buffers are locals.
    unsafe {
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        sl.cast::<IPersistFile>().ok()?.Load(PCWSTR(w.as_ptr()), STGM(0)).ok()?;
        let mut b = [0u16; 520];
        let mut fd: WIN32_FIND_DATAW = zeroed();
        sl.GetPath(&mut b, &mut fd, 0).ok()?;
        let p = String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0)?]);
        (!p.is_empty()).then_some((p, fd.dwFileAttributes & 0x10 != 0))
    }
}

/// A path on a network share (UNC, or a mapped drive): touching it can wait for the network.
fn on_network(p: &str) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    let b = p.as_bytes();
    // SAFETY: a NUL-terminated root string.
    p.starts_with(r"\\") || (b.len() >= 2 && b[1] == b':' && unsafe { GetDriveTypeW(wide(&format!("{}:\\", b[0] as char)).as_ptr()) } == 4) // DRIVE_REMOTE
}

/// "Today, 08:12" / "Yesterday, 18:20" / "11/20/2026, 09:05" in the user's own format.
fn when_label(t: std::time::SystemTime) -> (String, usize) {
    use windows_sys::Win32::{Foundation::FILETIME, System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime}};
    let secs = t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let ticks = (secs + 11_644_473_600) * 10_000_000;
    // SAFETY: plain conversions into locals.
    let (then, now) = unsafe {
        let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
        let (mut utc, mut local): (SYSTEMTIME, SYSTEMTIME) = (zeroed(), zeroed());
        FileTimeToSystemTime(&ft, &mut utc);
        SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local);
        let mut now: SYSTEMTIME = zeroed();
        GetLocalTime(&mut now);
        (local, now)
    };
    let days_ago = day_number(now.wMonth, now.wDay, now.wYear) - day_number(then.wMonth, then.wDay, then.wYear);
    // SAFETY: formatting into local buffers.
    let (date, time) = unsafe {
        let mut d = [0u16; 40];
        let mut tm = [0u16; 20];
        let nd = GetDateFormatW(0x0400, 1, &then, null(), d.as_mut_ptr(), 40).max(1) as usize - 1; // DATE_SHORTDATE
        let nt = GetTimeFormatW(0x0400, 2, &then, null(), tm.as_mut_ptr(), 20).max(1) as usize - 1; // TIME_NOSECONDS
        (String::from_utf16_lossy(&d[..nd]), String::from_utf16_lossy(&tm[..nt]))
    };
    (day_label(days_ago, &date, &time), day_group(days_ago))
}

fn day_group(days_ago: i64) -> usize { if days_ago <= 0 { 0 } else if days_ago == 1 { 1 } else { 2 } }

fn day_label(days_ago: i64, date: &str, time: &str) -> String {
    match days_ago {
        0 => format!("{}, {time}", t!("Сегодня", "Today", ja: "今日", pt: "Hoje", zh: "今天")),
        1 => format!("{}, {time}", t!("Вчера", "Yesterday", ja: "昨日", pt: "Ontem", zh: "昨天")),
        _ => format!("{date}, {time}"),
    }
}

/// Days from a fixed epoch to a calendar day (the usual civil-date arithmetic), to tell today from yesterday.
fn day_number(m: u16, d: u16, y: u16) -> i64 {
    let (y, m, d) = (i64::from(y) - i64::from(m <= 2), i64::from(m), i64::from(d));
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy
}

// ---------- the Run box's history ----------

/// What was typed in the Run box last (Windows keeps it under `RunMRU`, newest first in `MRUList`), without the `\1` it appends.
pub fn run_history(n: usize) -> Vec<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU");
    let read = |name: &str| -> Option<String> {
        let (mut buf, mut len) = ([0u16; 520], 1040u32);
        // SAFETY: a local buffer and its size in bytes.
        let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), wide(name).as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut len) };
        (ok == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
    };
    let order = read("MRUList").unwrap_or_default();
    order.chars().filter_map(|c| read(&c.to_string())).map(|s| s.trim_end_matches(r"\1").to_string()).filter(|s| !s.is_empty()).take(n).collect()
}

// ---------- the account ----------

/// The name of the Windows account (its full name when it has one) and a second line under it.
pub fn user() -> (String, String) {
    use windows_sys::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
    let login = std::env::var("USERNAME").unwrap_or_default();
    // SAFETY: a local buffer and its size.
    let full = unsafe {
        let (mut b, mut n) = ([0u16; 256], 256u32);
        if GetUserNameExW(NameDisplay, b.as_mut_ptr(), &mut n) { String::from_utf16_lossy(&b[..n as usize]) } else { String::new() }
    };
    let name = if full.trim().is_empty() { login.clone() } else { full };
    // the account's name under its full name; without a full name, the computer it lives on
    let sub = if name.eq_ignore_ascii_case(&login) { std::env::var("COMPUTERNAME").unwrap_or_default() } else { login };
    (name, sub)
}

/// The account's picture as a round icon (64 px), or null when the account has none. Windows keeps it in
/// `%APPDATA%\Microsoft\Windows\AccountPictures\*.accountpicture-ms`, a container with the picture in several sizes as JPEG or PNG
/// streams; older and domain setups leave a plain `<user>.png`/`.jpg` under `ProgramData\Microsoft\User Account Pictures`.
pub fn avatar() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    static AVATAR: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let ic = *AVATAR.get_or_init(|| {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::env::var_os("APPDATA")
            .map(|a| PathBuf::from(a).join(r"Microsoft\Windows\AccountPictures"))
            .and_then(|d| std::fs::read_dir(d).ok())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("accountpicture-ms")))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.0));
        let user = std::env::var("USERNAME").unwrap_or_default();
        let plain = ["png", "jpg"].map(|x| PathBuf::from(std::env::var_os("ProgramData").unwrap_or_default()).join(format!(r"Microsoft\User Account Pictures\{user}.{x}")));
        let paths = files.into_iter().map(|f| f.1).chain(plain);
        paths.filter_map(|p| std::fs::read(p).ok()).find_map(|b| round_picture(&b)).map_or(0, |ic| ic as usize)
    });
    wicon(ic as HICON)
}

/// The biggest picture found in `data` (JPEG and PNG streams, wherever they start), cut to a circle of 64 px.
fn round_picture(data: &[u8]) -> Option<HICON> {
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeHighQualityCubic,
        WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnDemand,
    };
    const N: u32 = 64;
    let starts: Vec<usize> = (0..data.len().saturating_sub(4)).filter(|&i| data[i..].starts_with(&[0xFF, 0xD8, 0xFF]) || data[i..].starts_with(&[0x89, 0x50, 0x4E, 0x47])).collect();
    // SAFETY: WIC on this thread; every buffer outlives the calls that read it.
    let best = unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut best: Option<(u32, Vec<u8>)> = None;
        for (n, &s) in starts.iter().enumerate() {
            let part = &data[s..starts.get(n + 1).copied().unwrap_or(data.len())];
            let decoded = (|| {
                let stream = wic.CreateStream().ok()?;
                stream.InitializeFromMemory(part).ok()?;
                let frame = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?.GetFrame(0).ok()?;
                let (mut w, mut h) = (0, 0);
                frame.GetSize(&mut w, &mut h).ok()?;
                let conv = wic.CreateFormatConverter().ok()?;
                conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
                let scaler = wic.CreateBitmapScaler().ok()?;
                scaler.Initialize(&conv, N, N, WICBitmapInterpolationModeHighQualityCubic).ok()?;
                let mut px = vec![0u8; (N * N * 4) as usize];
                scaler.CopyPixels(std::ptr::null(), N * 4, &mut px).ok()?;
                Some((w.min(h), px))
            })();
            if let Some((side, px)) = decoded
                && side >= 32
                && best.as_ref().is_none_or(|b| side > b.0)
            {
                best = Some((side, px));
            }
        }
        best?.1
    };
    // the circle: a pixel's coverage by its distance from the middle, the colour premultiplied by it
    let mut px = best;
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((i as u32 % N) as f32 + 0.5, (i as u32 / N) as f32 + 0.5);
        let d = ((x - N as f32 / 2.0).powi(2) + (y - N as f32 / 2.0).powi(2)).sqrt();
        let cov = (N as f32 / 2.0 - d + 0.5).clamp(0.0, 1.0);
        p.iter_mut().for_each(|c| *c = (f32::from(*c) * cov).round() as u8);
    }
    // SAFETY: GDI bitmaps built from the buffer and handed to an icon, which copies them.
    unsafe {
        let color = CreateBitmap(N as i32, N as i32, 1, 32, px.as_ptr().cast());
        let mask = CreateBitmap(N as i32, N as i32, 1, 1, null());
        let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let ic = CreateIconIndirect(&ii);
        DeleteObject(mask);
        DeleteObject(color);
        (!ic.is_null()).then_some(ic)
    }
}

// ---------- power ----------

/// Lock and sleep (restart and shut down ask first, in `power_click`).
pub fn lock() {
    let _ = Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"]).creation_flags(NO_WINDOW).spawn();
}

/// Sleep (S3 / modern standby), never hibernate. SetSuspendState needs the shutdown privilege switched on in our token; it
/// returns only after wake-up, so it runs on its own thread. (Not `rundll32 powrprof.dll,SetSuspendState`: rundll32 passes garbage
/// arguments and the PC hibernates instead of sleeping whenever hibernation is on.)
pub fn sleep() {
    std::thread::spawn(sleep_now);
}

pub fn sleep_now() {
    use windows_sys::Win32::{
        Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES},
        System::{Power::SetSuspendState, Threading::{GetCurrentProcess, OpenProcessToken}},
    };
    unsafe {
        let mut tok = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut tok) != 0 {
            let mut tp: TOKEN_PRIVILEGES = zeroed();
            tp.PrivilegeCount = 1;
            tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
            if LookupPrivilegeValueW(null(), wide("SeShutdownPrivilege").as_ptr(), &mut tp.Privileges[0].Luid) != 0 {
                AdjustTokenPrivileges(tok, 0, &tp, 0, null_mut(), null_mut());
            }
            CloseHandle(tok);
        }
        SetSuspendState(false, false, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_flip_both_ways() {
        assert_eq!(flip_layout("сркщьу"), "chrome");
        assert_eq!(flip_layout("ntktuhfv"), "телеграм");
        assert_eq!(flip_layout("vs code"), "мы сщву");
    }

    #[test]
    fn ranking() {
        assert_eq!(score("visual studio code", "vis"), Some(0));
        assert_eq!(score("visual studio code", "cod"), Some(1));
        assert_eq!(score("visual studio code", "vsc"), Some(2));
        assert_eq!(score("calculator", "cul"), Some(3));
        assert_eq!(score("calculator", "xyz"), None);
        assert_eq!(score("calculator", ""), None);
        assert_eq!(score("v", "v"), Some(0)); // one-letter initials do not count, the prefix does
    }

    #[test]
    fn a_query_in_the_wrong_layout_ranks_after_the_direct_matches() {
        *APPS.lock().unwrap() = vec![("Chrome".into(), wide("a")), ("Chromium".into(), wide("b")), ("Телеграм".into(), wide("c"))];
        let names = |q: &str| search(q).into_iter().map(|a| a.name).collect::<Vec<_>>();
        assert_eq!(names("chrom"), ["Chrome", "Chromium"]);
        assert_eq!(names("сркщь"), ["Chrome", "Chromium"]); // "chrom" on a Russian layout
        assert_eq!(names("ntktu"), ["Телеграм"]);
        assert!(names("  ").is_empty());
    }

    #[test]
    fn only_a_real_picture_becomes_the_round_avatar() {
        // SAFETY: COM for WIC on this test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        // a 48 px PNG made by our own encoder, wrapped in the junk a container puts around its pictures
        let path = std::env::temp_dir().join("topbar-avatar-test.png");
        let g = ui::gfx::Gfx::new(1.0).unwrap();
        g.save_png(&path.to_string_lossy(), 48, 48, &vec![0xff30_80c0u32; 48 * 48]).unwrap();
        let png = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let mut blob = vec![0u8; 40];
        blob.extend_from_slice(&png);
        blob.extend_from_slice(&[1, 2, 3]);
        assert!(round_picture(&blob).is_some(), "the picture inside the container is found");
        assert!(round_picture(&blob[..60]).is_none(), "a cut-off picture is not");
        assert!(round_picture(b"no pictures here").is_none());
    }

    #[test]
    fn days_and_labels() {
        assert_eq!(day_number(3, 1, 2026) - day_number(2, 28, 2026), 1);
        assert_eq!(day_number(1, 1, 2027) - day_number(12, 31, 2026), 1);
        assert_eq!(day_number(3, 1, 2024) - day_number(2, 28, 2024), 2); // a leap year
        assert!(day_label(0, "d", "08:12").ends_with(", 08:12"));
        assert_eq!(day_label(5, "11/20/2026", "09:05"), "11/20/2026, 09:05");
    }
}
#[cfg(test)]
mod recent_group_tests {
    use super::*;
    #[test]
    fn group_calendar_day_offsets() {
        assert_eq!((day_group(-1), day_group(0), day_group(1), day_group(2), day_group(300)), (0, 0, 1, 2, 2));
    }
    #[test]
    fn removing_duplicates_only_touches_matching_shortcuts() {
        let dir = std::path::PathBuf::from("target").join(format!("topbar-recent-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["one.lnk", "two.lnk", "other.lnk", "one.txt"] { std::fs::write(dir.join(name), b"test").unwrap(); }
        let resolve = |p: &std::path::Path| Some(if p.file_name().unwrap() == "other.lnk" { "other" } else { "target" }.to_string());
        assert!(remove_recent_with(&dir, "TARGET", resolve));
        assert!(!dir.join("one.lnk").exists() && !dir.join("two.lnk").exists());
        assert!(dir.join("other.lnk").exists() && dir.join("one.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
