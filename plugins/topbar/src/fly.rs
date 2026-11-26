// Own flyouts instead of the stock Windows ones: Start (search + all apps + power) and the bar's menus.
// One popup window; what it shows depends on `Kind`. (Quick settings, Wi-Fi and the calendar are panels of `ui::panel` now.)
use super::*;
use std::sync::Mutex;
use std::{
    collections::HashMap,
    sync::atomic::Ordering::Relaxed,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Start,
    Menu, // right-click menu of the bar
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Menu(u8),
    App(usize), // index into the filtered list
    Power(u8),  // 0 lock, 1 sleep, 2 restart, 3 shut down
    Keyboard,   // the on-screen keyboard button in the Start search field
}

/// The touch keyboard (TabTip), else the classic on-screen keyboard. Windows opens it by itself only for the Explorer shell.
fn touch_keyboard() {
    let tip = std::env::var_os("CommonProgramFiles").map(|p| std::path::PathBuf::from(p).join(r"microsoft shared\ink\TabTip.exe"));
    let exe = tip.filter(|p| p.is_file()).unwrap_or_else(|| "osk.exe".into());
    let _ = std::process::Command::new(exe).spawn();
}

static APPS: Mutex<Vec<(String, Vec<u16>)>> = Mutex::new(Vec::new()); // Start menu shortcuts: (name, .lnk path)
pub static PREVIEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false); // `--preview`: stays open without focus
const ROWS: usize = 11;

thread_local! {
    static FLY: Cell<Option<Kind>> = const { Cell::new(None) };
    static FH: Cell<HWND> = const { Cell::new(null_mut()) };
    static DEACT: Cell<(Option<Kind>, u64)> = const { Cell::new((None, 0)) }; // panel closed by losing focus, and when
    static NOTE: RefCell<String> = const { RefCell::new(String::new()) };
    static QUERY: RefCell<String> = const { RefCell::new(String::new()) };
    static SCROLL: Cell<usize> = const { Cell::new(0) };
    static SELALL: Cell<bool> = const { Cell::new(false) }; // Ctrl+A in the search: the next key replaces the query
    static ICONS: RefCell<HashMap<Vec<u16>, HICON>> = RefCell::new(HashMap::new()); // by path: the app list is reloaded and reordered
    static SEL: Cell<usize> = const { Cell::new(0) }; // Start: the row Enter opens (arrow keys move it)
    static OVER: Cell<Option<Act>> = const { Cell::new(None) };
    static MENU_X: Cell<i32> = const { Cell::new(0) };
    static MENU_CX: Cell<i32> = const { Cell::new(0) }; // where the open menu was asked to stand (screen px along the bar)
    static CONFIRM: Cell<i32> = const { Cell::new(-1) };
    static FIN: Cell<(i32, i32)> = const { Cell::new((0, 0)) }; // where the open flyout rests
    static T0: Cell<u64> = const { Cell::new(0) }; // when it started opening
    static JUST: RefCell<(String, u64)> = const { RefCell::new((String::new(), 0)) }; // the app pinned last and when: its row pulses
    static GHOST: Cell<(HWND, u64, (i32, i32))> = const { Cell::new((null_mut(), 0, (0, 0))) }; // the closing flyout: window, start, position
}

// ---------- data ----------

/// Background: collects the Start menu shortcuts once.
pub fn load_apps() {
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
fn flip_layout(q: &str) -> String {
    let (en, ru): (Vec<char>, Vec<char>) = (EN_KEYS.chars().collect(), RU_KEYS.chars().collect());
    q.chars().map(|c| en.iter().position(|&x| x == c).map(|i| ru[i]).or_else(|| ru.iter().position(|&x| x == c).map(|i| en[i])).unwrap_or(c)).collect()
}

/// How well an app name matches the query (both lower-case): 0 the name starts with it, 1 a word does, 2 the word
/// initials do ("vsc" = Visual Studio Code), 3 it is somewhere inside; None = no match.
fn score(name: &str, q: &str) -> Option<u8> {
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

// ---------- apps pinned to the Start panel (empty at first; added from right-click menus) ----------

static PINNED: Mutex<Vec<String>> = Mutex::new(Vec::new()); // paths of .lnk / .exe files, in the order they were added

fn pin_file() -> PathBuf {
    appdata_file("topbar-start.txt")
}

fn pinned_load() {
    *PINNED.lock().unwrap() = std::fs::read_to_string(pin_file()).map(|s| s.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default();
}

fn wstr(w: &[u16]) -> String {
    String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
}

pub fn start_pinned(path: &str) -> bool {
    PINNED.lock().unwrap().iter().any(|p| p.eq_ignore_ascii_case(path))
}

/// Files dropped on the open Start panel are added to it; the panel stays open and the new row pulses.
unsafe fn on_files(hw: HWND, hd: HDROP) {
    let n = DragQueryFileW(hd, u32::MAX, null_mut(), 0);
    let mut added = vec![];
    for i in 0..n {
        let len = DragQueryFileW(hd, i, null_mut(), 0) as usize;
        let mut b = vec![0u16; len + 1];
        DragQueryFileW(hd, i, b.as_mut_ptr(), b.len() as u32);
        let path = String::from_utf16_lossy(&b[..len]);
        if start_pin(&path) {
            added.push(std::path::Path::new(&path).file_stem().unwrap_or_default().to_string_lossy().to_string());
        }
    }
    DragFinish(hd);
    note(&match added.len() {
        0 => t!("Уже в Пуске", "Already in Start", ja: "すでにスタートにあります", pt: "Já está no Iniciar", zh: "已在开始菜单中").to_string(),
        1 => format!("Added to Start: {}", added[0]),
        n => format!("Added {n} items to Start"),
    });
    KillTimer(hw, 6);
    InvalidateRect(hw, null(), 0);
    force_fg(hw); // active again, so a later click elsewhere closes it as usual
}

/// Adds the app to the Start panel unless it is already there (a drop on the Start button). true = it was added.
pub fn start_pin(path: &str) -> bool {
    if start_pinned(path) {
        return false;
    }
    start_toggle(path);
    JUST.set((path.to_string(), unsafe { now_ms() }));
    true
}

pub fn is_open(k: Kind) -> bool {
    FLY.get() == Some(k)
}

/// Where the open menu stands along the bar (the bar lights the button it belongs to), `None` when no menu is open.
pub fn menu_at() -> Option<i32> {
    is_open(Kind::Menu).then(|| MENU_CX.get())
}

pub fn any_open() -> bool {
    FLY.get().is_some()
}

/// Short status line shown at the bottom of the open panel (Start: "Added to Start: ...").
pub fn note(s: &str) {
    NOTE.set(s.to_string());
}

/// Adds the app to the Start panel, or removes it when it is already there.
pub fn start_toggle(path: &str) {
    let mut p = PINNED.lock().unwrap();
    match p.iter().position(|x| x.eq_ignore_ascii_case(path)) {
        Some(i) => drop(p.remove(i)),
        None => p.push(path.to_string()),
    }
    let _ = std::fs::write(pin_file(), p.join("\n"));
}

static EXE_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Programs that have no Start menu shortcut (a loose loliland.exe): asks rust-search for .exe files matching what is typed
/// and adds them to the app list, where the normal ranking picks them up. Debounced; the panel repaints when they arrive.
fn find_loose_exes(hw: HWND) {
    let q = QUERY.with_borrow(|q| q.trim().to_string());
    let gen_ = EXE_GEN.fetch_add(1, Relaxed) + 1;
    let rs = PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_default()).join(r"rs-search\rs.exe");
    if q.chars().count() < 2 || !rs.is_file() {
        return;
    }
    let hw = hw as isize;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if EXE_GEN.load(Relaxed) != gen_ {
            return; // typed on: a newer search is coming
        }
        // straight to the service's pipe: no rs.exe process per query (and no full disk scan when the service is down)
        let found = winrust_common::search::ask_all(&q);
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
            unsafe { InvalidateRect(hw as HWND, null(), 0) };
        }
    });
}

fn filtered() -> Vec<usize> {
    let q = QUERY.with_borrow(|q| q.to_lowercase());
    let mut apps = APPS.lock().unwrap();
    if q.is_empty() {
        let pinned = PINNED.lock().unwrap();
        // nothing pinned yet: every app, A to Z, like Windows' "All apps" (an empty Start read as "no apps found", #72)
        if pinned.is_empty() {
            return (0..apps.len()).collect();
        }
        // nothing typed: only what the user pinned (an app that is not a Start menu shortcut joins the list on demand)
        return pinned
            .iter()
            .map(|p| {
                let w = wide(p);
                apps.iter().position(|a| a.1 == w).unwrap_or_else(|| {
                    let stem = std::path::Path::new(p).file_stem().unwrap_or_default().to_string_lossy().to_string();
                    let name: String = stem.chars().take(1).flat_map(char::to_uppercase).chain(stem.chars().skip(1)).collect();
                    apps.push((name, w));
                    apps.len() - 1
                })
            })
            .collect();
    }
    // typed in the wrong layout: those matches come after the direct ones
    let fq = flip_layout(&q);
    let mut v: Vec<(u8, u32, usize)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let n = a.0.to_lowercase();
            let s = score(&n, &q).or_else(|| score(&n, &fq).map(|s| s + 4))?;
            Some((s, u32::MAX - uses(&n), i))
        })
        .collect();
    v.sort_unstable(); // best match, then most launched, then alphabetical (the list is sorted by name)
    v.into_iter().map(|x| x.2).collect()
}

// ---------- geometry ----------

fn size(k: Kind) -> (i32, i32) {
    match k {
        Kind::Start => (560, 640),
        Kind::Menu => (MW, menu_rows().1),
    }
}

fn r(l: i32, t: i32, rr: i32, b: i32) -> RECT {
    RECT { left: sc(l), top: sc(t), right: sc(rr), bottom: sc(b) }
}

fn rects(k: Kind) -> Vec<(RECT, Act)> {
    let (w, h) = size(k);
    match k {
        Kind::Menu => {
            let heads: Vec<bool> = MITEMS.with_borrow(|m| m.iter().map(|it| it.head).collect());
            menu_rows().0.into_iter().enumerate().filter(|(i, _)| !heads[*i]).map(|(i, (t, b))| (r(6, t, w - 6, b), Act::Menu(i as u8))).collect()
        }
        Kind::Start => {
            let f = filtered();
            let s = SCROLL.get();
            let mut v: Vec<_> = (0..ROWS).filter(|i| s + i < f.len()).map(|i| (r(12, 76 + 40 * i as i32, w - 12, 76 + 40 * (i as i32 + 1)), Act::App(s + i))).collect();
            v.extend((0..4u8).map(|i| (r(w - 16 - 4 * 44 + 44 * i as i32, h - 52, w - 16 - 4 * 44 + 44 * (i as i32 + 1) - 4, h - 12), Act::Power(i))));
            v.push((r(w - 58, 24, w - 26, 52), Act::Keyboard)); // inside the search field, at its right end
            v
        }
    }
}

fn act_at(k: Kind, x: i32, y: i32) -> Option<(RECT, Act)> {
    rects(k).into_iter().rev().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
}

// ---------- open / close ----------

pub fn init(hi: HINSTANCE) {
    pinned_load();
    unsafe {
        let class = wide("topbar_fly");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: hi, hCursor: LoadCursorW(null_mut(), IDC_ARROW), lpszClassName: class.as_ptr(), ..zeroed() };
        RegisterClassW(&wc);
    }
}

/// One row of a menu flyout: a label (with a window's icon when it stands for a window), a dim keyboard hint on the right,
/// a dot when it is the current choice, red when it destroys something, a separator line above it when `sep`.
pub struct MItem {
    pub icon: HICON,
    pub label: String,
    pub hint: &'static str,
    pub on: bool,
    pub danger: bool,
    pub sep: bool,
    pub head: bool,  // a section caption: small dim text, not clickable
    pub swatch: u32, // a colour dot before the label (COLORREF), u32::MAX = none
}

impl MItem {
    pub fn head(label: impl Into<String>) -> Self {
        MItem { head: true, ..MItem::new(label) }
    }
    pub fn swatch(mut self, c: u32) -> Self {
        self.swatch = c;
        self
    }
    pub fn new(label: impl Into<String>) -> Self {
        MItem { icon: null_mut(), label: label.into(), hint: "", on: false, danger: false, sep: false, head: false, swatch: u32::MAX }
    }
    pub fn hint(mut self, h: &'static str) -> Self {
        self.hint = h;
        self
    }
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn sep(mut self) -> Self {
        self.sep = true;
        self
    }
}

type Pick = Box<dyn FnOnce(usize)>;
thread_local! {
    static MITEMS: RefCell<Vec<MItem>> = const { RefCell::new(Vec::new()) };
    static MHEAD: RefCell<Option<(HICON, &'static str, String, String)>> = const { RefCell::new(None) }; // icon (or a glyph when null), title, subtitle
    static MPICK: RefCell<Option<Pick>> = const { RefCell::new(None) };
}
const MW: i32 = 272; // menu width
const MROW: i32 = 38;
const MHEAD_H: i32 = 58;

/// Menu flyout above the bar, centered on screen x `cx`. `pick` gets the index of the chosen item (not called on cancel).
/// A right click while a menu is open replaces it.
pub fn menu(cx: i32, head: Option<(HICON, &'static str, String, String)>, items: Vec<MItem>, pick: impl FnOnce(usize) + 'static) {
    if is_open(Kind::Menu) {
        close_now();
    }
    MITEMS.set(items);
    MHEAD.set(head);
    MPICK.set(Some(Box::new(pick)));
    MENU_CX.set(cx);
    MENU_X.set(if vert() { cx } else { cx - sc(MW) / 2 }); // along a side bar it holds the anchor y
    toggle(Kind::Menu);
}

/// (top, bottom) of each menu item in logical px, and the panel height.
fn menu_rows() -> (Vec<(i32, i32)>, i32) {
    let mut y = if MHEAD.with_borrow(|h| h.is_some()) { MHEAD_H + 9 } else { 6 };
    let rows = MITEMS.with_borrow(|m| {
        m.iter()
            .map(|it| {
                if it.sep {
                    y += 9;
                }
                let h = if it.head { 28 } else { MROW };
                y += h;
                (y - h, y)
            })
            .collect()
    });
    (rows, y + 6)
}

/// Right-click menu of an empty spot of the bar.
pub fn context_menu(x: i32) {
    let pu = crate::purge_installed();
    let mut items = vec![
        MItem::new(t!("Параметры Windows", "Windows Settings", ja: "Windows の設定", pt: "Configurações do Windows", zh: "Windows 设置")).hint("Win+I"),
        MItem::new(t!("Персонализация", "Personalization", ja: "個人用設定", pt: "Personalização", zh: "个性化")),
        MItem::new(t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器")).hint("Ctrl+Shift+Esc").sep(),
        MItem::new(t!("Чистильщик диска", "Disk Cleaner", ja: "ディスク クリーナー", pt: "Limpador de Disco", zh: "磁盘清理")),
        MItem::new(t!("Закрыть панель", "Close the bar", ja: "バーを閉じる", pt: "Fechar a barra", zh: "关闭任务栏")).danger().sep(),
    ];
    if pu {
        items.insert(4, MItem::new(t!("Удалить мусор Windows (необратимо)", "Remove Windows junk (irreversible)", ja: "Windows の不要なものを削除 (元に戻せません)", pt: "Remover lixo do Windows (irreversível)", zh: "删除 Windows 垃圾文件（不可恢复）")).danger());
    }
    menu(x, None, items, move |i| unsafe {
        let i = if pu && i == 4 { 99 } else { i };
        match i {
            0 => open(&wide("ms-settings:")),
            1 => open(&wide("ms-settings:personalization")),
            // with rtaskmgr-win32 installed this is our task manager; a full path because explorer.exe does not search PATH
            2 => open(&wide(&format!(r"{}\System32\taskmgr.exe", std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())))),
            3 => open_cleaner(),
            99 => crate::open_purge(),
            _ => _ = PostMessageW(FindWindowW(wide("topbar").as_ptr(), null()), WM_CLOSE, 0, 0), // quits the bar, brings the Windows taskbar back
        }
    });
}

/// Starts the fade-out of the open flyout (it stays as a ghost window until the fade ends).
pub fn close() {
    let h = FH.replace(null_mut());
    FLY.set(None);
    unsafe { InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0) }; // the bar's open button goes out
    if h.is_null() {
        return;
    }
    unsafe {
        kill_ghost();
        GHOST.set((h, now_ms(), FIN.get()));
        KillTimer(h, 1);
        KillTimer(h, 2);
        SetTimer(h, 3, 15, None);
    }
}

/// Closes without the fade (another flyout replaces it, or a fullscreen app needs the screen).
pub fn close_now() {
    close();
    unsafe { kill_ghost() };
}

unsafe fn kill_ghost() {
    let (g, _, _) = GHOST.replace((null_mut(), 0, (0, 0)));
    if !g.is_null() {
        DestroyWindow(g);
    }
}

/// One frame of a flyout fade: opening rises up from the bar while fading in, closing fades out while sinking back down.
unsafe fn anim_step(hw: HWND) {
    let now = now_ms();
    let (g, gt0, gpos) = GHOST.get();
    if hw == g {
        let t = (now.saturating_sub(gt0) as f32 / 150.0).min(1.0);
        let q = t * t;
        SetLayeredWindowAttributes(hw, 0, ((1.0 - q) * 255.0) as u8, LWA_ALPHA);
        SetWindowPos(hw, null_mut(), gpos.0, gpos.1 + (q * sc(10) as f32) as i32, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        if t >= 1.0 {
            kill_ghost();
        }
    } else if hw == FH.get() {
        let t = (now.saturating_sub(T0.get()) as f32 / 220.0).min(1.0);
        let p = 1.0 - (1.0 - t).powi(3);
        SetLayeredWindowAttributes(hw, 0, (p * 255.0) as u8, LWA_ALPHA);
        let (x, y) = FIN.get();
        SetWindowPos(hw, null_mut(), x, y + ((1.0 - p) * sc(14) as f32) as i32, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        if t >= 1.0 {
            KillTimer(hw, 3);
        }
    } else {
        KillTimer(hw, 3);
    }
}

/// toggle() for a click on the panel's own bar button. The click can deactivate the panel first (closing it), and a
/// plain toggle then opened it again with its animation (#140): a panel of this kind closed that way just now stays closed.
pub fn toggle_click(k: Kind) {
    let (dk, t) = DEACT.replace((None, 0));
    if dk == Some(k) && unsafe { now_ms() }.saturating_sub(t) < 400 {
        return;
    }
    toggle(k);
}

pub fn toggle(k: Kind) {
    // The panel counts as open only while its window is alive: Alt+F4 (or anything else destroying the window behind our
    // back) leaves the state saying "open", and then every Win press would only "close" a dead window and never open a new
    // one — the button looks dead until the bar restarts.
    if FLY.get() == Some(k) && unsafe { IsWindow(FH.get()) } != 0 {
        return close();
    }
    // No debounce here: a Win press right after the panel closed (or while it is still fading out) must open it. The bar is
    // WS_EX_NOACTIVATE, so the click that closes the panel never comes back as a second toggle of its own.
    close();
    unsafe {
        HV.with_borrow_mut(|m| m.retain(|k, _| *k < 1_000_000)); // flyout keys are >= 1_000_000: no stale glow from the last panel
        QUERY.set(String::new());
        SELALL.set(false);
        SCROLL.set(0);
        SEL.set(0);
        NOTE.set(String::new());
        CONFIRM.set(-1);
        OVER.set(None);
        if k == Kind::Start {
            pinned_load(); // the search card (rs-launcher) can add to this list from another process
            std::thread::spawn(load_apps); // apps installed since the last open show up (the panel repaints every second)
        }
        let (w, h) = (sc(size(k).0), sc(size(k).1));
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let x = if vert() { side_x(w) } else { match k {
            Kind::Start => (sw - w) / 2,
            Kind::Menu => MENU_X.get().min(sw - w - sc(8)).max(sc(8)),
        } };
        let class = wide("topbar_fly");
        let fy = if vert() && k == Kind::Menu { (MENU_X.get() - h / 2).clamp(sc(8), GetSystemMetrics(SM_CYSCREEN) - h - sc(8)) } else { pop_y(h) }; // above the bar (below it at the top of the screen)
        let hw = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED, class.as_ptr(), null(), WS_POPUP, x, fy + sc(14), w, h, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        SetLayeredWindowAttributes(hw, 0, 0, LWA_ALPHA); // starts invisible, anim_step fades it in
        if k == Kind::Start {
            // accept file drops (an app dragged onto the panel is added to Start); the bar may be elevated, so lift UIPI for them
            DragAcceptFiles(hw, 1);
            for m in [WM_DROPFILES, 0x49 /* WM_COPYGLOBALDATA */, WM_COPYDATA] {
                ChangeWindowMessageFilterEx(hw, m, MSGFLT_ALLOW, null_mut());
            }
        }
        FIN.set((x, fy));
        T0.set(now_ms());
        SetWindowRgn(hw, CreateRoundRectRgn(0, 0, w + 1, h + 1, rr(16), rr(16)), 1);
        FH.set(hw);
        FLY.set(Some(k));
        InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0); // the bar's button lights up
        ShowWindow(hw, SW_SHOW);
        force_fg(hw);
        SetTimer(hw, 1, 1000, None);
        SetTimer(hw, 3, 15, None);
    }
}

// ---------- actions ----------

/// Starts the i-th app of the filtered Start list and counts the launch (search ranks by it). false = no such row.
unsafe fn launch(i: usize) -> bool {
    let Some(app) = filtered().get(i).and_then(|&ai| APPS.lock().unwrap().get(ai).cloned()) else { return false };
    if wstr(&app.1).starts_with("shell:") {
        bump_name(&app.0); // open() counts only .lnk files
    }
    open(&app.1);
    true
}

/// Right-click menu of a Start app: open, open as administrator, show the shortcut's folder.
unsafe fn app_menu(hw: HWND, i: usize, x: i32, y: i32) {
    let Some(lnk) = filtered().get(i).and_then(|&ai| APPS.lock().unwrap().get(ai).map(|a| a.1.clone())) else { return };
    let m = CreatePopupMenu();
    let path = wstr(&lnk);
    let file = !path.starts_with("shell:"); // Store apps have no file to run elevated or to show in a folder
    let pin_label = if start_pinned(&path) { t!("Убрать из Пуска", "Remove from Start", ja: "スタートから削除", pt: "Remover do Iniciar", zh: "从开始菜单移除") } else { t!("Добавить в Пуск", "Add to Start", ja: "スタートに追加", pt: "Adicionar ao Iniciar", zh: "添加到开始菜单") };
    let menu = [
        (1, t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开")),
        (2, t!("Запуск от имени администратора", "Run as administrator", ja: "管理者として実行", pt: "Executar como administrador", zh: "以管理员身份运行")),
        (3, t!("Расположение файла", "Open file location", ja: "ファイルの場所を開く", pt: "Abrir local do arquivo", zh: "打开文件所在位置")),
        (4, pin_label),
    ];
    for (id, label) in menu {
        if file || id == 1 || id == 4 {
            AppendMenuW(m, MF_STRING, id, wide(label).as_ptr());
        }
    }
    let mut pt = POINT { x, y };
    ClientToScreen(hw, &mut pt);
    let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hw, null());
    DestroyMenu(m);
    match cmd {
        1 => _ = launch(i),
        2 => {
            ShellExecuteW(null_mut(), wide("runas").as_ptr(), lnk.as_ptr(), null(), null(), SW_SHOWNORMAL);
        }
        3 => {
            let p = String::from_utf16_lossy(&lnk[..lnk.len().saturating_sub(1)]);
            winrust_common::reveal(&p);
        }
        4 => {
            start_toggle(&path);
            return; // stay open, the list just changed
        }
        _ => return,
    }
    close();
}

unsafe fn run_act(a: Act) {
    match a {
        Act::Menu(i) => {
            close();
            if let Some(pick) = MPICK.take() {
                pick(i as usize);
            }
        }
        Act::Keyboard => touch_keyboard(), // the panel stays open: the keys go to its search field
        Act::App(i) => {
            if launch(i) {
                close();
            }
        }
        Act::Power(p) => {
            if p >= 2 && CONFIRM.get() != p as i32 {
                CONFIRM.set(p as i32); // first click only arms it
                return;
            }
            close();
            match p {
                0 => {
                    let _ = Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"]).creation_flags(NO_WINDOW).spawn();
                }
                1 => {
                    // not `rundll32 powrprof.dll,SetSuspendState`: rundll32 passes garbage arguments and the PC hibernates
                    // instead of sleeping whenever hibernation is on. On its own thread: the call returns only after wake-up.
                    std::thread::spawn(|| sleep_now());
                }
                2 => {
                    let _ = Command::new("shutdown").args(["/r", "/t", "0"]).creation_flags(NO_WINDOW).spawn();
                }
                _ => {
                    let _ = Command::new("shutdown").args(["/s", "/t", "0"]).creation_flags(NO_WINDOW).spawn();
                }
            }
        }
    }
}

/// Sleep (S3 / modern standby), never hibernate. SetSuspendState needs the shutdown privilege switched on in our token.
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

// ---------- painting ----------

/// A raised rounded surface: a hairline edge (a soft shadow on the light style) around `base`.
unsafe fn surface(dc: HDC, rc: RECT, base: u32, rad: i32) {
    fill(dc, rc, mix(base, pal().text, 0.07), rad);
    fill(dc, RECT { left: rc.left + 1, top: rc.top + 1, right: rc.right - 1, bottom: rc.bottom - 1 }, base, rad - 2);
}

fn inset(rc: RECT, d: i32) -> RECT {
    RECT { left: rc.left + d, top: rc.top + d, right: rc.right - d, bottom: rc.bottom - d }
}

unsafe fn paint(hw: HWND, k: Kind) {
    let mut ps: PAINTSTRUCT = zeroed();
    let wdc = BeginPaint(hw, &mut ps);
    let mut cr: RECT = zeroed();
    GetClientRect(hw, &mut cr);
    BUSY.set(false);
    let (dc, bmp) = (CreateCompatibleDC(wdc), CreateCompatibleBitmap(wdc, cr.right, cr.bottom));
    let old = SelectObject(dc, bmp);
    fill(dc, cr, mix(pal().bg, pal().text, 0.12), sc(16));
    fill(dc, inset(cr, 1), pal().bg, sc(16));
    SetBkMode(dc, TRANSPARENT as i32);
    let (face, gface) = (wide(ui_face()), wide(winrust_common::style::icon_face()));
    let font = |f: &[u16], px: i32, wt: i32| CreateFontW(-sc(px), 0, 0, 0, wt, 0, 0, 0, 1, 0, 0, 5, 0, f.as_ptr());
    let (f_txt, f_small, f_glyph, f_title, f_big) = (font(&face, 13, 400), font(&face, 12, 400), font(&gface, 16, 400), font(&face, 15, 600), font(&face, 34, 600));
    let (f_semi, f_head, f_gs, f_label) = (font(&face, 13, 600), font(&face, 18, 700), font(&gface, 11, 400), font(&face, 12, 600));
    let card = card();
    let of = SelectObject(dc, f_txt);
    let (w, h) = size(k);
    let over = OVER.get();
    let hot = |a: Act| over == Some(a);
    // hover highlight that fades; the key is the element's position (flyout keys start at 1_000_000)
    let g = |a: Act, rc: RECT, from: u32, to: u32, rad: i32| glow(dc, rc.top * 4096 + rc.left + 1_000_000, hot(a), rc, from, to, rad);

    match k {
        Kind::Menu => {
            // header: the app's icon on an accent-tinted tile, its name, and what is open
            MHEAD.with_borrow(|hd| {
                if let Some((ic, glyph, title, sub)) = hd {
                    let tile = r(14, 12, 48, 46);
                    fill(dc, tile, mix(pal().bg, pal().accent, 0.16), sc(10));
                    if !ic.is_null() {
                        DrawIconEx(dc, tile.left + sc(5), tile.top + sc(5), *ic, sc(24), sc(24), 0, null_mut(), DI_NORMAL);
                    } else {
                        SelectObject(dc, f_glyph); // no app icon (Style, Language): the menu's own symbol
                        text(dc, glyph, tile, pal().accent, DT_CENTER);
                    }
                    SelectObject(dc, f_title);
                    text(dc, title, r(60, 10, w - 14, 32), pal().text, DT_LEFT);
                    SelectObject(dc, f_small);
                    text(dc, sub, r(60, 30, w - 14, 48), pal().dim, DT_LEFT);
                    fill(dc, r(12, MHEAD_H + 4, w - 12, MHEAD_H + 5), pal().line, 0);
                }
            });
            let rs = rects(k);
            MITEMS.with_borrow(|items| {
                // section captions have no rect of their own (not clickable): drawn straight from the row layout
                SelectObject(dc, f_small);
                for (it, (t, b)) in items.iter().zip(menu_rows().0) {
                    if it.head {
                        if it.sep {
                            fill(dc, r(12, t - 5, w - 12, t - 4), pal().line, 0);
                        }
                        text(dc, &it.label, r(20, t + 8, w - 14, b), pal().dim, DT_LEFT);
                    }
                }
                for (rc, a) in rs.iter() {
                    let Act::Menu(i) = *a else { continue };
                    let it = &items[i as usize];
                    if it.sep {
                        fill(dc, RECT { left: rc.left + sc(6), top: rc.top - sc(5), right: rc.right - sc(6), bottom: rc.top - sc(5) + 1 }, pal().line, 0);
                    }
                    let col = if it.danger { BAD } else { pal().text };
                    g(*a, *rc, pal().bg, if it.danger { mix(pal().bg, BAD, 0.22) } else { pal().hover }, sc(8));
                    // plain text rows: a window's own icon is the only picture; the current choice gets an accent dot
                    let mut lx = rc.left + sc(14);
                    if !it.icon.is_null() {
                        DrawIconEx(dc, lx, (rc.top + rc.bottom) / 2 - sc(8), it.icon, sc(16), sc(16), 0, null_mut(), DI_NORMAL);
                        lx += sc(26);
                    }
                    if it.swatch != u32::MAX {
                        let (cy, d) = ((rc.top + rc.bottom) / 2, sc(9));
                        fill(dc, RECT { left: lx, top: cy - d, right: lx + 2 * d, bottom: cy + d }, pal().line, 2 * d); // ring: the dot shows on any background
                        fill(dc, RECT { left: lx + 1, top: cy - d + 1, right: lx + 2 * d - 1, bottom: cy + d - 1 }, it.swatch, 2 * d);
                        lx += sc(28);
                    }
                    let mut rx = rc.right - sc(14);
                    if it.on {
                        let (cy, d) = ((rc.top + rc.bottom) / 2, sc(4));
                        fill(dc, RECT { left: rx - 2 * d, top: cy - d, right: rx, bottom: cy + d }, pal().accent, 2 * d);
                        rx -= sc(18);
                    }
                    SelectObject(dc, f_small);
                    if !it.hint.is_empty() {
                        text(dc, it.hint, RECT { left: lx, right: rx, ..*rc }, mix(pal().dim, pal().bg, 0.25), DT_RIGHT);
                    }
                    SelectObject(dc, f_txt);
                    text(dc, &it.label, RECT { left: lx, right: rx - match it.hint.chars().count() { 0 => 0, 1..=2 => sc(20), _ => sc(96) }, ..*rc }, if it.on { pal().accent } else { col }, DT_LEFT);
                }
            });
        }
        Kind::Start => {
            surface(dc, r(20, 20, w - 20, 56), card, sc(36));
            SelectObject(dc, f_glyph);
            text(dc, "\u{E721}", r(28, 20, 56, 56), pal().dim, DT_CENTER);
            SelectObject(dc, f_txt);
            let q = QUERY.with_borrow(|q| q.clone());
            if q.is_empty() {
                text(dc, t!("Введите для поиска", "Type to search apps", ja: "アプリを検索", pt: "Digite para pesquisar apps", zh: "输入以搜索应用"), r(64, 20, w - 64, 56), pal().dim, DT_LEFT);
            } else {
                text(dc, &q, r(64, 20, w - 64, 56), if SELALL.get() { pal().accent } else { pal().text }, DT_LEFT);
            }
            if let Some((kr, ka)) = rects(k).into_iter().find(|(_, a)| *a == Act::Keyboard) {
                g(ka, kr, card, mix(card, pal().text, 0.1), sc(14));
                SelectObject(dc, f_glyph);
                text(dc, "\u{E765}", kr, if hot(ka) { pal().text } else { pal().dim }, DT_CENTER); // keyboard
                SelectObject(dc, f_txt);
            }
            let f = filtered();
            if f.is_empty() {
                if q.is_empty() {
                    text(dc, t!("Здесь пока пусто", "Nothing here yet", ja: "まだ何もありません", pt: "Nada aqui ainda", zh: "这里还什么都没有"), r(24, 76, w - 24, 106), pal().text, DT_LEFT);
                    text(dc, t!("Перетащите приложение или файл на кнопку «Пуск», чтобы добавить", "Drag an app or file onto the Start button to add it", ja: "スタート ボタンにアプリやファイルをドラッグして追加します", pt: "Arraste um app ou arquivo para o botão Iniciar para adicionar", zh: "把应用或文件拖到“开始”按钮即可添加"), r(24, 106, w - 24, 134), pal().dim, DT_LEFT);
                    text(dc, t!("Или найдите приложение и добавьте его из меню по правой кнопке", "Or search an app and right-click it → Add to Start", ja: "またはアプリを検索し、右クリックして「スタートに追加」を選びます", pt: "Ou pesquise um app e use o menu do botão direito → Adicionar ao Iniciar", zh: "或者搜索应用，右键点击 → 添加到开始菜单"), r(24, 134, w - 24, 162), pal().dim, DT_LEFT);
                } else {
                    text(dc, t!("Приложения не найдены", "No apps found", ja: "アプリが見つかりません", pt: "Nenhum app encontrado", zh: "未找到应用"), r(24, 76, w - 24, 116), pal().dim, DT_LEFT);
                }
            }
            let rs = rects(k);
            let apps = APPS.lock().unwrap();
            for (rc, a) in &rs {
                match *a {
                    Act::App(i) => {
                        // the list is reloaded on a background thread: an index from a moment ago may be gone
                        let Some(app) = f.get(i).and_then(|&ai| apps.get(ai)) else { continue };
                        // the row that was just pinned pulses in the accent colour for a moment
                        let pulse = JUST.with_borrow(|j| {
                            let age = now_ms().saturating_sub(j.1) as f32 / 1200.0;
                            if age < 1.0 && !j.0.is_empty() && app.1 == wide(&j.0) { (1.0 - age) * (1.0 - age) } else { 0.0 }
                        });
                        let sel = i == SEL.get(); // what Enter opens
                        if pulse > 0.0 || sel {
                            BUSY.set(BUSY.get() || pulse > 0.0);
                            fill(dc, *rc, mix(if sel { mix(pal().bg, pal().accent, 0.14) } else { pal().bg }, pal().accent, 0.45 * pulse), sc(8));
                        }
                        g(*a, *rc, if sel { mix(pal().bg, pal().accent, 0.14) } else { pal().bg }, if sel { mix(pal().hover, pal().accent, 0.18) } else { pal().hover }, sc(8));
                        if sel {
                            fill(dc, RECT { left: rc.left, top: rc.top + sc(10), right: rc.left + sc(3), bottom: rc.bottom - sc(10) }, pal().accent, sc(3));
                        }
                        let ic = ICONS.with_borrow_mut(|m| {
                            *m.entry(app.1.clone()).or_insert_with(|| {
                                let t = lnk_target(&app.1);
                                lnk_icon(&app.1, &t, icon_px())
                            })
                        });
                        if !ic.is_null() {
                            DrawIconEx(dc, rc.left + sc(13), (rc.top + rc.bottom - icon_px()) / 2, ic, icon_px(), icon_px(), 0, null_mut(), DI_NORMAL); // at its loaded size: no stretching
                        }
                        let store = wstr(&app.1).starts_with("shell:");
                        text(dc, &app.0, RECT { left: rc.left + sc(52), top: rc.top, right: rc.right - sc(110), bottom: rc.bottom }, pal().text, DT_LEFT);
                        SelectObject(dc, f_small);
                        let kind = if sel && !q.is_empty() { "Enter ↵" } else if store { t!("Приложение", "App", ja: "アプリ", pt: "App", zh: "应用") } else { "" };
                        text(dc, kind, RECT { left: rc.right - sc(110), top: rc.top, right: rc.right - sc(14), bottom: rc.bottom }, pal().dim, DT_RIGHT);
                        SelectObject(dc, f_txt);
                    }
                    Act::Power(p) => {
                        let armed = CONFIRM.get() == p as i32;
                        glow(dc, rc.top * 4096 + rc.left + 1_000_000, armed || hot(*a), *rc, pal().bg, if armed { BAD } else { pal().hover }, sc(8));
                        SelectObject(dc, f_glyph);
                        text(dc, ["\u{E72E}", "\u{E708}", "\u{E777}", "\u{E7E8}"][p as usize], *rc, pal().text, DT_CENTER);
                    }
                    _ => {}
                }
            }
            SelectObject(dc, f_small);
            let note = NOTE.with_borrow(|n| n.clone());
            let hint = match CONFIRM.get() {
                2 => t!("Нажмите ещё раз для перезагрузки", "Click again to restart", ja: "もう一度クリックで再起動", pt: "Clique de novo para reiniciar", zh: "再次点击以重启").to_string(),
                3 => t!("Нажмите ещё раз для выключения", "Click again to shut down", ja: "もう一度クリックでシャットダウン", pt: "Clique de novo para desligar", zh: "再次点击以关机").to_string(),
                _ if !note.is_empty() => note,
                _ => std::env::var("USERNAME").unwrap_or_default(),
            };
            text(dc, &hint, r(24, h - 52, w - 200, h - 12), pal().dim, DT_LEFT);
        }
    }

    BitBlt(wdc, 0, 0, cr.right, cr.bottom, dc, 0, 0, SRCCOPY);
    SelectObject(dc, of);
    for f in [f_txt, f_small, f_glyph, f_title, f_big, f_semi, f_head, f_gs, f_label] {
        DeleteObject(f);
    }
    SelectObject(dc, old);
    DeleteObject(bmp);
    DeleteDC(dc);
    if BUSY.get() {
        SetTimer(hw, 2, 16, None); // next animation frame
    } else {
        KillTimer(hw, 2);
    }
    EndPaint(hw, &ps);
}

// ---------- window proc ----------

unsafe extern "system" fn proc(hw: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_TIMER && wp == 3 {
        anim_step(hw); // also for the fading-out ghost window
        return 0;
    }
    if hw != FH.get() {
        return DefWindowProcW(hw, msg, wp, lp); // ghost or a window still being created
    }
    let Some(k) = FLY.get() else { return DefWindowProcW(hw, msg, wp, lp) };
    let (x, y) = ((lp & 0xffff) as i16 as i32, (lp >> 16 & 0xffff) as i16 as i32);
    match msg {
        WM_PAINT => paint(hw, k),
        WM_ERASEBKGND => return 1,
        WM_ACTIVATE if wp & 0xffff == 0 && !PREVIEW.load(Relaxed) => {
            // A file drag that began elsewhere (desktop icon, folder window) deactivates this panel with the button still
            // down: keep Start open so the drop can land on it, and close once the button is up (timer 6).
            if k == Kind::Start && GetAsyncKeyState(VK_LBUTTON as i32) < 0 {
                SetTimer(hw, 6, 150, None);
            } else {
                DEACT.set((Some(k), now_ms()));
                close();
            }
        }
        WM_TIMER if wp == 6 => {
            if GetAsyncKeyState(VK_LBUTTON as i32) >= 0 {
                KillTimer(hw, 6);
                if GetForegroundWindow() != hw {
                    close();
                }
            }
        }
        WM_DROPFILES => on_files(hw, wp as HDROP),
        WM_TIMER if wp == 2 => {
            InvalidateRect(hw, null(), 0);
        }
        WM_TIMER => {
            InvalidateRect(hw, null(), 0);
        }
        WM_MOUSEMOVE => {
            let o = act_at(k, x, y).map(|a| a.1);
            if OVER.replace(o) != o {
                InvalidateRect(hw, null(), 0);
            }
        }
        WM_LBUTTONDOWN => {
            if let Some((_, a)) = act_at(k, x, y) {
                run_act(a);
            }
            if FLY.get().is_some() {
                InvalidateRect(hw, null(), 0);
            }
        }
        WM_RBUTTONUP => {
            if let (Kind::Start, Some((_, Act::App(i)))) = (k, act_at(k, x, y)) {
                app_menu(hw, i, x, y);
            }
            InvalidateRect(hw, null(), 0);
        }
        WM_MOUSEWHEEL => {
            let up = (wp >> 16) as i16 > 0;
            if k == Kind::Start {
                SCROLL.set(if up { SCROLL.get().saturating_sub(2) } else { (SCROLL.get() + 2).min(filtered().len().saturating_sub(ROWS)) });
            }
            InvalidateRect(hw, null(), 0);
        }
        WM_KEYDOWN if k == Kind::Start && (wp == VK_DOWN as usize || wp == VK_UP as usize) => {
            // arrows move the row Enter opens; the list scrolls to keep it in view
            let n = filtered().len();
            if n > 0 {
                let s = if wp == VK_DOWN as usize { (SEL.get() + 1).min(n - 1) } else { SEL.get().saturating_sub(1) };
                SEL.set(s);
                SCROLL.set(SCROLL.get().min(s).max((s + 1).saturating_sub(ROWS)));
                InvalidateRect(hw, null(), 0);
            }
        }
        WM_CHAR => {
            let c = wp as u32;
            if c == 27 {
                if k == Kind::Start && QUERY.with_borrow(|q| !q.is_empty()) {
                    QUERY.set(String::new()); // Esc clears the search first, the next one closes
                    SCROLL.set(0);
                    SEL.set(0);
                } else {
                    close();
                    return 0;
                }
            } else if k == Kind::Start {
                if c == 13 {
                    if launch(SEL.get()) {
                        close();
                        return 0;
                    }
                } else {
                    QUERY.with_borrow_mut(|q| {
                        let all = SELALL.replace(false);
                        if c == 1 {
                            SELALL.set(!q.is_empty()); // Ctrl+A
                        } else if c == 127 {
                            // Ctrl+Backspace: the last word
                            if all { q.clear() } else { let t = q.trim_end().len(); q.truncate(q[..t].rfind(' ').map_or(0, |i| i + 1)); }
                        } else if c == 8 {
                            if all { q.clear() } else { q.pop(); }
                        } else if let Some(ch) = char::from_u32(c).filter(|_| c >= 32) {
                            if all { q.clear() }
                            q.push(ch);
                        }
                    });
                    SCROLL.set(0);
                    SEL.set(0);
                    find_loose_exes(hw);
                }
            }
            InvalidateRect(hw, null(), 0);
        }
        _ => return DefWindowProcW(hw, msg, wp, lp),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{flip_layout, score, init, is_open, toggle, close_now, Kind, FLY, FH, GetModuleHandleW, SetProcessDPIAware, null, null_mut, DestroyWindow, IsWindow};

    #[test]
    fn layout_flip_both_ways() {
        assert_eq!(flip_layout("сркщьу"), "chrome");
        assert_eq!(flip_layout("ntktuhfv"), "телеграм");
        assert_eq!(flip_layout("vs code"), "мы сщву");
    }

    /// The Win button always opens Start: a press right after a close is not dropped, and a state left saying "open" with no
    /// window behind it (CreateWindowExW failed) does not turn the button into a close-only no-op.
    #[test]
    fn win_button_always_opens_start() {
        unsafe {
            SetProcessDPIAware();
            init(GetModuleHandleW(null()));
            toggle(Kind::Start);
            assert!(is_open(Kind::Start), "the first press opens Start");
            toggle(Kind::Start);
            assert!(!is_open(Kind::Start), "the second press closes it");
            toggle(Kind::Start);
            assert!(is_open(Kind::Start), "a press right after the close opens it again");
            let live = FH.get();
            assert!(!live.is_null() && IsWindow(live) != 0);
            DestroyWindow(live);
            close_now();
            // the window is gone but the state still says "open" (what a failed creation or a kill behind our back leaves)
            FLY.set(Some(Kind::Start));
            FH.set(null_mut());
            toggle(Kind::Start);
            assert!(is_open(Kind::Start) && IsWindow(FH.get()) != 0, "the press opens a real panel");
            close_now();
        }
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
}
