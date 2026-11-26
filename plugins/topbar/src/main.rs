// topbar: replaces the Windows taskbar with a panel at the bottom of the screen (the name stayed from when it sat on top),
// drawn in the Windows 11 taskbar palette: Start, pinned apps and open windows in the center; tray apps, language, wifi,
// volume, battery and date/time on the right. `--restore` brings the normal taskbar back (used by uninstall).
#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]
use std::{
    cell::{Cell, RefCell},
    mem::{size_of, zeroed},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::Command,
    ptr::{null, null_mut},
    thread,
    sync::Mutex,
    collections::HashMap,
    sync::atomic::{AtomicBool, AtomicIsize, Ordering::Relaxed},
};
#[macro_use]
extern crate winrust_common;
use winrust_common as i18n;
use winrust_common::style::{Side, Style, pal, style, ui_face};
mod clip;
mod hist;
mod fly;
mod apps;
mod audio;
mod quick;
mod tray;
mod status;
mod run;
mod shot;
mod snapshots;
mod lang;
mod qs;
mod settings;
mod menus;
mod recent;
mod ui;
mod vdesk;
mod wins;
mod render;
mod pacing;
mod notify;
#[allow(dead_code)]
mod rec;
use ui::bar::{self as barui, keys::*};
use {tray::*, status::*, run::*, shot::*, lang::*, settings::*, qs::*, wins::*};
use windows::{
    Win32::{
        Media::Audio::{Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender},
        System::Com::{CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile, STGM},
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{Interface, PCWSTR},
};
use windows_sys::Win32::{
    Foundation::*,
    Globalization::{GetDateFormatW, GetLocaleInfoW, GetTimeFormatW},
    Graphics::{
        Dwm::DwmGetWindowAttribute,
        Gdi::*,
        GdiPlus::{
            FillModeAlternate, GdipAddPathArc, GdipClosePathFigure, GdipCreateFromHDC, GdipCreatePath, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath, GdipFillPath, GdipSetPixelOffsetMode,
            GdipSetSmoothingMode, GdiplusStartup, GdiplusStartupInput, GpBrush, GpGraphics, GpPath, GpSolidFill, PixelOffsetModeHalf, SmoothingModeAntiAlias,
        },
    },
    NetworkManagement::WiFi::*,
    System::{
        LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW},
        Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
        SystemInformation::{GetLocalTime, GetTickCount64},
        Threading::{OpenProcess, QueryFullProcessImageNameW, TerminateProcess},
    },
    UI::{Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

/// A corner radius in logical px.
fn rr(v: i32) -> i32 {
    sc(v)
}
/// The bar stands on the left or right edge of the screen (its thickness is bar_h(), it spans the screen height).
fn vert() -> bool {
    i18n::style::side() != i18n::style::Side::Bottom
}
/// The dock: a pill in the middle of the edge, as long as its content.
fn dock() -> bool {
    style() == Style::Dock
}
/// The bar's look now: thickness, gap to the screen edge, corner radius (spec section 1).
fn shape() -> barui::Shape {
    barui::shape(style(), i18n::style::side(), i18n::style::bar_size())
}
/// Design pixels to device pixels, rounded like the renderer does.
fn dp(v: f32) -> i32 {
    (v * SCALE.get()).round() as i32
}
/// Thickness of the bar in device pixels.
fn bar_h() -> i32 {
    dp(shape().thick)
}
/// Thickness plus the gap to the screen edge ("barOuter"): popups stand 12 dip beyond it.
fn bar_outer() -> i32 {
    dp(shape().outer())
}
/// Screen y of the top edge of a bottom bar (with its gap) when it is in place.
unsafe fn bar_top() -> i32 {
    GetSystemMetrics(SM_CYSCREEN) - bar_outer()
}
/// Screen x of a popup of width `w` next to a side bar.
unsafe fn side_x(w: i32) -> i32 {
    let o = bar_outer() + dp(12.0);
    if i18n::style::side() == i18n::style::Side::Left { o } else { GetSystemMetrics(SM_CXSCREEN) - o - w }
}
/// Screen y of a popup of height `h` above the bar (beside a side bar: standing 12 dip above the screen's bottom).
unsafe fn pop_y(h: i32) -> i32 {
    if vert() { GetSystemMetrics(SM_CYSCREEN) - dp(12.0) - h } else { bar_top() - dp(12.0) - h }
}
/// False once the bar has slid away completely.
fn bar_visible() -> bool {
    UI.with_borrow(|u| u.as_ref().is_none_or(|b| !b.placement().gone))
}
/// The bar window's rectangle on screen as (left, top, width, height), including the strip above a bottom bar.
fn bar_win() -> (i32, i32, i32, i32) {
    UI.with_borrow(|u| u.as_ref().map_or((0, 0, 0, 0), |b| b.placement().win))
}
const BAD: u32 = winrust_common::style::BAD;
const CALLBACK: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3; // not exported under the enabled features
const NO_WINDOW: u32 = 0x0800_0000;
// hit-test keys (Start 0, app buttons >= 1, tray slots TAPP + n): see ui::bar::keys
const WIN_START: u32 = WM_APP + 10; // posted by the keyboard hook: Win key pressed alone
const ALT_TAB: u32 = WM_APP + 11; // posted by the keyboard hook: Alt+Tab (wp 1 = Shift held: backwards)
const ALT_UP: u32 = WM_APP + 12; // posted by the keyboard hook: Alt released while switching (wp 1 = Esc: cancel)
static SWITCHING: AtomicBool = AtomicBool::new(false); // Alt+Tab switcher open (the hook thread reads it)
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static DESK_HIDE: AtomicBool = AtomicBool::new(false); // the desktop in front and the pointer away from the bar for 2 s
static WIN_ALONE: AtomicBool = AtomicBool::new(false);
static BAR: AtomicIsize = AtomicIsize::new(0); // the bar window, for the hook thread
const WIN_NUM: u32 = WM_APP + 15; // posted by the keyboard hook: Win+1..9 (wp 0..8) = the n-th button of the bar
const WIN_X: u32 = WM_APP + 17; // posted by the keyboard hook: Win+X = our power-user menu (start_menu)
const NEXT_LANG: u32 = WM_APP + 16; // posted by the keyboard hook: Win+Space with no explorer (wp 1 = Shift held: previous layout)
const VOLKEY: u32 = WM_APP + 14; // posted by the keyboard hook: a volume key (wp 0 up, 1 down, 2 mute)
const WIN_V: u32 = WM_APP + 18; // posted by the keyboard hook: Win+V with no explorer (our clipboard history, #171)
const ALT_SPACE: u32 = WM_APP + 19; // posted by the keyboard hook: Alt+Space = our search panel (not rs-launcher's card)
const SNIP: u32 = WM_APP + 13; // posted by the keyboard hook: PrtSc / Win+Shift+S with no explorer to handle them
static EXPLORER_UP: AtomicBool = AtomicBool::new(true); // explorer's taskbar exists (it handles PrtSc itself then); refreshed by the bar

struct Pin {
    lnk: Vec<u16>,
    target: String, // lower-case exe path, "" if unknown
    args: String,   // what the shortcut passes to it (browser profile), "" if none
    aumid: String,  // lower-case AppUserModelID the shortcut carries (a browser profile's own, #132), "" if none
    icon: HICON,
}
struct Btn {
    pin: Option<usize>,
    wins: Vec<HWND>,
    icon: HICON, // the window's icon for unpinned buttons, looked up on refresh, not every animation frame
}

/// One notification-area icon, as an app registered it through Shell_NotifyIcon (we own the "Shell_TrayWnd" they talk to).
struct TIcon {
    hwnd: HWND, // the app's callback window
    uid: u32,
    guid: [u8; 16], // zero unless the app identifies the icon by GUID
    icon: HICON,    // our own copy
    cb: u32,        // uCallbackMessage
    ver: u32,       // NIM_SETVERSION value; >= 4 means the NOTIFYICON_VERSION_4 message format
    hidden: bool,
    tip: String,
    key: String, // owner exe file name, lower-case: what the "keep on the bar" choice is stored under
}
const WM_COPYDATA: u32 = 0x4A;
#[repr(C)]
struct Cds {
    dw: usize,
    cb: u32,
    p: *const u8,
}
static USAGE: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None); // launches per shortcut name, lower-case

fn appdata_file(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join(name)
}
fn uses(name: &str) -> u32 {
    USAGE
        .lock()
        .unwrap()
        .get_or_insert_with(|| std::fs::read_to_string(appdata_file("topbar-usage.txt")).unwrap_or_default().lines().filter_map(|l| l.rsplit_once('\t')).filter_map(|(k, n)| Some((k.to_string(), n.parse().ok()?))).collect())
        .get(name)
        .copied()
        .unwrap_or(0)
}
/// Counts one launch of the shortcut at `path` (by file name, so the same app pinned and in Start counts together).
fn bump(path: &str) {
    bump_name(&std::path::Path::new(path).file_stem().unwrap_or_default().to_string_lossy());
}
/// Counts one launch of the app called `name` (Start search ranks by it).
fn bump_name(name: &str) {
    let name = name.to_lowercase();
    uses(&name); // loads the file first
    let mut g = USAGE.lock().unwrap();
    let m = g.as_mut().unwrap();
    *m.entry(name).or_insert(0) += 1;
    let _ = std::fs::write(appdata_file("topbar-usage.txt"), m.iter().map(|(k, n)| format!("{k}\t{n}\n")).collect::<String>());
}
fn lnk_uses(lnk: &std::path::Path) -> u32 {
    uses(&lnk.file_stem().unwrap_or_default().to_string_lossy().to_lowercase())
}

thread_local! {
    static HOST: Cell<HWND> = const { Cell::new(null_mut()) };
    static SKIP: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) }; // windows their app took off the taskbar (ITaskbarList::DeleteTab)
    static PINS: RefCell<Vec<Pin>> = const { RefCell::new(Vec::new()) };
    static BTNS: RefCell<Vec<Btn>> = const { RefCell::new(Vec::new()) }; // [0] is always Start (pin None, no wins)
    static TAPPS: RefCell<Vec<TIcon>> = const { RefCell::new(Vec::new()) };
    static SHOWN: RefCell<Option<Vec<String>>> = const { RefCell::new(None) }; // tray icons kept on the bar (by exe); None = none: every tray icon lives in the chevron flyout
    static HOT: Cell<i32> = const { Cell::new(NONE) };
    static ORDER: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) }; // unpinned windows in the order their buttons were added
    static HV: RefCell<HashMap<i32, (f32, u64)>> = RefCell::new(HashMap::new()); // hover fade per element: (0..1, last update ms)
    static BUSY: Cell<bool> = const { Cell::new(false) }; // a fade is still running: the painter asks for another frame
    static UI: RefCell<Option<barui::Bar>> = const { RefCell::new(None) }; // the bar's renderer and its animation state
    static SHADOW: Cell<HWND> = const { Cell::new(null_mut()) }; // the window under the floating bars that holds their shadow
    static DESK_T: Cell<u64> = const { Cell::new(0) }; // last moment the bar had a reason to stay on the desktop
    static SCALE: Cell<f32> = const { Cell::new(1.0) };
    static OLD_STATE: Cell<usize> = const { Cell::new(ABS_ALWAYSONTOP as usize) };
    static BAT: Cell<(i32, bool)> = const { Cell::new((-1, false)) }; // (percent or -1 = no battery, on AC)
    static VOL: Cell<(i32, bool)> = const { Cell::new((-1, false)) }; // (percent or -1 = unknown, muted)
    static WIFI: Cell<i32> = const { Cell::new(-1) }; // signal quality 0..100, -1 = not on wifi
    static INET: Cell<bool> = const { Cell::new(true) }; // Windows sees a way to the internet (any adapter)
    static TICK: Cell<u32> = const { Cell::new(0) };
    static EP: RefCell<Option<IAudioEndpointVolume>> = const { RefCell::new(None) };
    static TB_MSG: Cell<u32> = const { Cell::new(0) }; // "TaskbarCreated"
    static SH_MSG: Cell<u32> = const { Cell::new(0) }; // "SHELLHOOK": window events for the shell (we are the shell hook window without explorer)
    static EXP_TRAY: Cell<HWND> = const { Cell::new(null_mut()) }; // explorer's taskbar we registered with
    static FLASH: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) }; // windows asking for attention (FlashWindowEx) until activated (#129)
    static APP_FG: Cell<HWND> = const { Cell::new(null_mut()) }; // last foreground window that is not ours: the language button acts on it
    static JL_KEY: Cell<Option<i32>> = const { Cell::new(None) }; // the app button whose jump list was opened last
    static LANG_TRY: Cell<(u32, usize, usize)> = const { Cell::new((0, 0, 0)) }; // (thread, layout before, layout asked for) of the last switch, checked by timer 7
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
fn sc(v: i32) -> i32 {
    (v as f32 * SCALE.get()) as i32
}
// ---------- taskbar / appbar ----------

/// Tooltip text of a bar element ("" = none).
unsafe fn tip_text(h: i32) -> String {
    match h {
        SEARCH => "Search".into(),
        TASKVIEW => t!("Представление задач", "Task View", ja: "タスク ビュー", pt: "Visão de Tarefas").into(),
        POW_R => t!("Перезагрузка", "Restart", ja: "再起動", pt: "Reiniciar", zh: "重启").into(),
        POW_S => t!("Завершение работы", "Shut down", ja: "シャットダウン", pt: "Desligar", zh: "关机").into(),
        0 => "Start".into(),
        TAPP => "Hidden icons".into(),
        GEAR => t!("Настройки панели", "Bar settings", ja: "バーの設定", pt: "Configurações da barra", zh: "任务栏设置").into(),
        SHOWDESK => t!("Показать рабочий стол", "Show desktop", ja: "デスクトップを表示", pt: "Mostrar a área de trabalho", zh: "显示桌面").into(),
        CLOCK => {
            let mut t: SYSTEMTIME = zeroed();
            GetLocalTime(&mut t);
            let mut b = [0u16; 80];
            let n = GetDateFormatW(0x0400, 2, &t, null(), b.as_mut_ptr(), 80).max(1) as usize - 1; // DATE_LONGDATE
            String::from_utf16_lossy(&b[..n])
        }
        LANG => tf!("Язык ввода: {}", "Input language: {}", ja: "入力言語: {}", pt: "Idioma de entrada: {}", zh: "输入语言：{}", layout_names(layouts().1).0),
        WIFI_I => match net() {
            Link::Wifi(q) => tf!("Wi‑Fi · сигнал {q}%", "Wi‑Fi · signal {q}%", ja: "Wi‑Fi · 信号 {q}%", pt: "Wi‑Fi · sinal {q}%", zh: "Wi‑Fi · 信号 {q}%"),
            Link::NoInternet(_) => t!("Wi‑Fi подключён, но интернета нет", "Wi‑Fi connected, no internet", ja: "Wi‑Fi 接続済み、インターネットなし", pt: "Wi‑Fi conectado, sem internet", zh: "Wi‑Fi 已连接，无 Internet").into(),
            Link::Cable => t!("Интернет по кабелю", "Internet over a cable", ja: "有線のインターネット", pt: "Internet por cabo", zh: "有线网络").into(),
            Link::Offline => t!("Нет подключения к сети", "Not connected", ja: "未接続", pt: "Não conectado", zh: "未连接").into(),
        },
        VOL_I => vol_tip(),
        BAT_I => match BAT.get() {
            (p, ac) if p >= 0 => format!("Battery {p}%{}", if ac { " · plugged in" } else { "" }),
            _ => String::new(),
        },
        t if t > TAPP && t < TAPP + 100 => bar_idx().get((t - TAPP - 1) as usize).and_then(|&i| TAPPS.with_borrow(|t| t.get(i).map(|i| i.tip.clone()))).unwrap_or_default(),
        i if i >= 1 => {
            let (wins, lnk) = BTNS.with_borrow(|b| b.get(i as usize).map(|b| (b.wins.clone(), b.pin.and_then(|p| PINS.with_borrow(|p2| p2.get(p).map(|x| wstr_of(&x.lnk))))))).unwrap_or_default();
            let title = |w: HWND| {
                let mut b = [0u16; 256];
                let n = GetWindowTextW(w, b.as_mut_ptr(), 256).max(0) as usize;
                String::from_utf16_lossy(&b[..n])
            };
            match (wins.first(), lnk) {
                (Some(&w), _) if wins.len() > 1 => format!("{}  (+{})", title(w), wins.len() - 1),
                (Some(&w), _) => title(w),
                (None, Some(l)) => std::path::Path::new(&l).file_stem().unwrap_or_default().to_string_lossy().into(),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

fn vol_tip() -> String {
    match VOL.get() {
        (v, false) if v >= 0 => format!("Volume {v}%"),
        (_, true) => "Muted".into(),
        _ => "Volume".into(),
    }
}

/// The hint of bar element `key` after `delay` ms (the mockup waits .25 s), `hold` ms at most.
unsafe fn key_tip(key: i32, text: String, delay: u32, hold: Option<u32>) {
    let Some(a) = anchor(key) else { return };
    let along = tip_cx(key);
    ui::tip::show(&text, ui::tip::Place::button(a.side, a.button, along, SCALE.get()), delay, hold);
}

/// Where a bar element is, for the panel that belongs to it (`NONE`: the bar as a whole), in device pixels with the bar at rest.
unsafe fn anchor(key: i32) -> Option<ui::panel::Anchor> {
    UI.with_borrow(|u| {
        let b = u.as_ref()?;
        let p = b.placement();
        let (dx, dy) = (p.rest.0 - p.bar.0, p.rest.1 - p.bar.1);
        let button = b.screen_rect(key).map_or((0, 0, 0, 0), |r| (r.0 + dx, r.1 + dy, r.2 + dx, r.3 + dy));
        Some(ui::panel::Anchor {
            side: i18n::style::side(),
            button,
            bar: (p.rest.0, p.rest.1, p.rest.0 + p.rest.2, p.rest.1 + p.rest.3),
            screen: (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)),
        })
    })
}

/// A panel opened or closed: the bar redraws the button that is lit for it.
fn panel_changed() {
    ui::notice_pop::restack(); // the cards stand over a panel that opened in their corner
    // SAFETY: invalidating our own bar window.
    unsafe { InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0) };
}

/// A popup (a panel of the new frame, or an old flyout) is open: hints stay away and the bar stays put.
fn popup_open() -> bool {
    // the previews over an app button are not in the way of the hints and of the next preview
    fly::any_open() || (ui::panel::any_open() && !ui::panel::is_open(ui::panel::Id::Preview))
}

// (the Alt+Tab switcher, the previews over an app button and Task view are in wins.rs)

unsafe fn hide_tip(bar: HWND) {
    KillTimer(bar, 2);
    ui::tip::hide();
}

unsafe fn set_state(hwnd: HWND, state: usize) {
    let mut ab: APPBARDATA = zeroed();
    ab.cbSize = size_of::<APPBARDATA>() as u32;
    ab.hWnd = hwnd;
    ab.lParam = state as isize;
    SHAppBarMessage(ABM_SETSTATE, &mut ab);
}

/// Hides the real taskbar and sets it to auto-hide so Windows gives its work-area strip back.
unsafe fn hide_taskbar() {
    set_state(tray()[0], ABS_AUTOHIDE as usize);
    for t in tray().into_iter().filter(|t| !t.is_null()) {
        ShowWindow(t, SW_HIDE);
    }
}

fn taskbar_state_file() -> PathBuf {
    appdata_file("topbar-taskbar-state.txt")
}

/// The taskbar mode (always on top / auto-hide bits) the user had before the bar hid it. It lives in a file so that
/// `--restore` (reshell's watchdog after a crash) puts back the user's own mode, not the Windows default.
fn saved_taskbar_state() -> Option<usize> {
    std::fs::read_to_string(taskbar_state_file()).ok()?.trim().parse().ok()
}

fn forget_taskbar_state() {
    let _ = std::fs::remove_file(taskbar_state_file());
}

/// At start: the mode to give back later. A file left by a crashed run wins (explorer's current mode is then our own
/// auto-hide); otherwise the current mode is the user's and is written down. Without explorer's taskbar: the default.
unsafe fn remember_taskbar_state() -> usize {
    if let Some(s) = saved_taskbar_state() {
        return s;
    }
    let tb = tray()[0];
    if tb.is_null() {
        return ABS_ALWAYSONTOP as usize;
    }
    let mut ab: APPBARDATA = zeroed();
    ab.cbSize = size_of::<APPBARDATA>() as u32;
    ab.hWnd = tb;
    let s = SHAppBarMessage(ABM_GETSTATE, &mut ab);
    let _ = std::fs::write(taskbar_state_file(), s.to_string());
    s
}

unsafe fn show_taskbar(state: usize) {
    set_state(tray()[0], state);
    for t in tray().into_iter().filter(|t| !t.is_null()) {
        ShowWindow(t, SW_SHOWNA);
    }
}

// explorer re-shows its taskbar on hover / restart; hide it again
unsafe fn hide_taskbar_if_shown() {
    if tray().into_iter().any(|t| !t.is_null() && IsWindowVisible(t) != 0) {
        hide_taskbar();
    }
}

/// Reserves the strip the bar takes (maximized windows stay clear of it) and draws it in its place.
unsafe fn place(hwnd: HWND) {
    let (cx, cy) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
    let mut ab: APPBARDATA = zeroed();
    ab.cbSize = size_of::<APPBARDATA>() as u32;
    ab.hWnd = hwnd;
    // the bar and its gap to the screen edge (classic: no gap): maximized windows end above it, the dock included;
    // auto-hide reserves nothing (the bar slides in over the windows)
    let strip = if i18n::style::autohide() { 0 } else { dp(shape().reserve()) };
    if vert() {
        let left = i18n::style::side() == i18n::style::Side::Left;
        ab.uEdge = if left { ABE_LEFT } else { ABE_RIGHT };
        ab.rc = if left { RECT { left: 0, top: 0, right: strip, bottom: cy } } else { RECT { left: cx - strip, top: 0, right: cx, bottom: cy } };
        SHAppBarMessage(ABM_QUERYPOS, &mut ab);
        if left { ab.rc.left = sane_edge(ab.rc.left, cx) } else { ab.rc.right = cx - sane_edge(cx - ab.rc.right, cx) }
        if left { ab.rc.right = ab.rc.left + strip } else { ab.rc.left = ab.rc.right - strip }
    } else {
        ab.uEdge = ABE_BOTTOM;
        ab.rc = RECT { left: 0, top: cy - strip, right: cx, bottom: cy };
        SHAppBarMessage(ABM_QUERYPOS, &mut ab);
        ab.rc.bottom = cy - sane_edge(cy - ab.rc.bottom, cy);
        ab.rc.top = ab.rc.bottom - strip;
    }
    SHAppBarMessage(ABM_SETPOS, &mut ab);
    // No explorer (rs-shell / rust-explorer): nobody handles appbars, so set the work area ourselves. Without it maximized
    // windows slide under the bar and, covering the whole monitor, count as "fullscreen" and hide the bar.
    // Also with Explorer: it does not always honour the appbar when its own taskbar is hidden (#89).
    {
        set_work_area(if vert() { if ab.uEdge == ABE_LEFT { RECT { left: ab.rc.right, top: 0, right: cx, bottom: cy } } else { RECT { left: 0, top: 0, right: ab.rc.left, bottom: cy } } } else { RECT { left: 0, top: 0, right: cx, bottom: ab.rc.top } });
    }
    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    redraw(hwnd);
}

/// How far from its screen edge the bar may sit after ABM_QUERYPOS (room left for other appbars). Explorer sometimes answers
/// with a far-off edge (a stale or huge appbar entry), and the work area then shrank to half the screen (#106): past a
/// quarter of the screen, or off it, the bar goes back to the edge.
fn sane_edge(offset: i32, size: i32) -> i32 {
    if (0..=size / 4).contains(&offset) { offset } else { 0 }
}

unsafe fn set_work_area(mut want: RECT) {
    let mut cur: RECT = zeroed();
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut cur as *mut _ as *mut _, 0);
    if (cur.left, cur.top, cur.right, cur.bottom) != (want.left, want.top, want.right, want.bottom) {
        // SPIF_SENDCHANGE waits for every top-level window (took ~3 s, #94): set quietly and post the notice instead
        SystemParametersInfoW(SPI_SETWORKAREA, 0, &mut want as *mut _ as *mut _, 0);
        PostMessageW(HWND_BROADCAST, WM_SETTINGCHANGE, SPI_SETWORKAREA as usize, 0);
        EnumWindows(Some(fit_maximized), &want as *const RECT as isize);
    }
}

/// A window that was already maximized keeps its old height when the work area shrinks: trim it (size only, no restore).
unsafe extern "system" fn fit_maximized(h: HWND, lp: isize) -> i32 {
    let want = &*(lp as *const RECT);
    // the work area is the primary monitor's: a window maximized on another monitor would be cut to a sliver
    if IsWindowVisible(h) == 0 || IsZoomed(h) == 0 || MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) != MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) {
        return 1;
    }
    let mut r: RECT = zeroed();
    GetWindowRect(h, &mut r);
    let f =GetSystemMetrics(SM_CXFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER); // a maximized window overhangs by its border
    let n = RECT { left: r.left.max(want.left - f), top: r.top.max(want.top - f), right: r.right.min(want.right + f), bottom: r.bottom.min(want.bottom + f) };
    if (n.left, n.top, n.right, n.bottom) != (r.left, r.top, r.right, r.bottom) && n.right > n.left && n.bottom > n.top {
        SetWindowPos(h, null_mut(), n.left, n.top, n.right - n.left, n.bottom - n.top, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    1
}

// ---------- data: windows, pins, tray apps, volume, wifi, battery ----------

thread_local! {
    static EXE_CACHE: RefCell<HashMap<usize, (u32, String)>> = RefCell::new(HashMap::new()); // hwnd -> (pid, exe): refresh runs twice a second
}

unsafe fn exe_of(h: HWND) -> String {
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, &mut pid);
    if let Some(e) = EXE_CACHE.with_borrow(|c| c.get(&(h as usize)).filter(|(p, e)| *p == pid && !e.is_empty()).map(|(_, e)| e.clone())) {
        return e;
    }
    let p = OpenProcess(0x1000, 0, pid); // PROCESS_QUERY_LIMITED_INFORMATION
    if p.is_null() {
        return String::new();
    }
    let (mut b, mut n) = ([0u16; 520], 520u32);
    let ok = QueryFullProcessImageNameW(p, 0, b.as_mut_ptr(), &mut n);
    CloseHandle(p);
    let exe = if ok == 0 { String::new() } else { String::from_utf16_lossy(&b[..n as usize]).to_lowercase() };
    EXE_CACHE.with_borrow_mut(|c| {
        if c.len() > 512 {
            c.clear(); // closed windows never leave on their own
        }
        c.insert(h as usize, (pid, exe.clone()));
    });
    exe
}

unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> i32 {
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    let mut cloaked = 0u32; // UWP windows that are suspended / on another desktop are visible but cloaked
    DwmGetWindowAttribute(h, 14, &mut cloaked as *mut _ as *mut _, 4);
    let mut cls = [0u16; 32];
    let n = GetClassNameW(h, cls.as_mut_ptr(), 32) as usize;
    let ours = String::from_utf16_lossy(&cls[..n]) == "topbar";
    let skipped = SKIP.with_borrow(|s| s.contains(&h));
    if IsWindowVisible(h) != 0 && GetWindow(h, GW_OWNER).is_null() && cloaked == 0 && !ours && GetWindowTextLengthW(h) > 0 && (ex & WS_EX_TOOLWINDOW == 0 || ex & WS_EX_APPWINDOW != 0) && !skipped {
        (*(lp as *mut Vec<HWND>)).push(h);
    }
    1
}

/// "%windir%\explorer.exe" -> "C:\Windows\explorer.exe" (unknown variables stay as they are).
/// A Run-box command ("cmd", "%appdata%", "control") as a real path the shell can draw an icon for: environment
/// variables expanded, a bare name looked up on PATH like Win+R does. Anything else comes back as it was.
unsafe fn resolve_cmd(s: &str) -> String {
    use windows_sys::Win32::Storage::FileSystem::SearchPathW;
    let e = expand_env(s);
    if e.contains(['\\', '/', ':']) {
        return e;
    }
    let mut b = [0u16; 520];
    let n = SearchPathW(null(), wide(&e).as_ptr(), wide(".exe").as_ptr(), b.len() as u32, b.as_mut_ptr(), null_mut()) as usize;
    if n > 0 && n < b.len() { String::from_utf16_lossy(&b[..n]) } else { e }
}

fn expand_env(s: &str) -> String {
    let parts: Vec<&str> = s.split('%').collect();
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        match std::env::var(p) {
            // odd pieces sit between two '%': a variable name
            Ok(v) if i % 2 == 1 && i + 1 < parts.len() => out.push_str(&v),
            _ if i % 2 == 1 && i + 1 < parts.len() => out.push_str(&format!("%{p}%")),
            _ if i % 2 == 1 => out.push_str(&format!("%{p}")), // a lone '%' with nothing to close it
            _ => out.push_str(p),
        }
    }
    out
}

/// Target exe of a shortcut (lower-case), "" if it has none (e.g. Store apps).
unsafe fn lnk_target(lnk: &[u16]) -> String {
    (|| -> Option<String> {
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        sl.cast::<IPersistFile>().ok()?.Load(PCWSTR(lnk.as_ptr()), STGM(0)).ok()?;
        let mut b = [0u16; 520];
        sl.GetPath(&mut b, null_mut(), 0).ok()?;
        let path = String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0)?]);
        if !path.is_empty() {
            return Some(path.to_lowercase());
        }
        // shell links without a file target (the File Explorer pin) usually take their icon from the app's exe:
        // "%windir%\explorer.exe,0" -> match the app's windows by that exe
        let mut idx = 0;
        sl.GetIconLocation(&mut b, &mut idx).ok()?;
        let icon = String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0)?]);
        let icon = expand_env(&icon).to_lowercase();
        icon.ends_with(".exe").then_some(icon)
    })()
    .unwrap_or_default()
}

/// The target's own icon: SHGetFileInfo on the .lnk adds the shortcut arrow, so that is only the fallback.
unsafe fn lnk_icon(lnk: &[u16], target: &str, px: i32) -> HICON {
    let ic = item_icon(&resolve_cmd(&wstr_of(lnk)), px);
    if !ic.is_null() {
        return ic;
    }
    let mut icon: HICON = null_mut();
    if !target.is_empty() {
        ExtractIconExW(wide(target).as_ptr(), 0, null_mut(), &mut icon, 1);
    }
    if icon.is_null() {
        let mut sfi: SHFILEINFOW = zeroed();
        SHGetFileInfoW(lnk.as_ptr(), 0, &mut sfi, size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_SMALLICON);
        icon = sfi.hIcon;
    }
    icon
}

/// Size the bar's app icons are loaded at: the design draws them up to 40 dip, and 1.24 times that when they lift on hover.
fn bar_icon_px() -> i32 {
    sc(64)
}

/// Icon of a file, shortcut (its target's icon, no arrow) or shell item ("shell:AppsFolder\<app id>") rendered by the
/// shell at exactly `px` pixels, the way Explorer gets its icons. Null if the shell has none.
unsafe fn item_icon(path: &str, px: i32) -> HICON {
    use windows::Win32::{
        Foundation::SIZE,
        UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY},
    };
    let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide(path).as_ptr()), None) else { return null_mut() };
    let Ok(f) = item.cast::<IShellItemImageFactory>() else { return null_mut() };
    let Ok(bmp) = f.GetImage(SIZE { cx: px, cy: px }, SIIGBF_ICONONLY) else { return null_mut() };
    let color = bmp.0 as HBITMAP;
    if !bitmap_has_alpha(color) {
        DeleteObject(color);
        return sysimage_icon(path);
    }
    let mask = CreateBitmap(px, px, 1, 1, null()); // ignored: the 32-bit colour bitmap carries the alpha
    let mut ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let ic = CreateIconIndirect(&mut ii);
    DeleteObject(mask);
    DeleteObject(color);
    ic
}

/// Whether a 32-bit bitmap from `IShellItemImageFactory` carries any alpha. Old icons keep their transparency only in the AND
/// mask, which the factory drops: every alpha byte is 0, and an icon built from such a bitmap would be opaque with a black
/// background. (A bitmap that cannot be read counts as having alpha: nothing better to do with it.)
unsafe fn bitmap_has_alpha(bmp: HBITMAP) -> bool {
    let mut bm: BITMAP = zeroed();
    if GetObjectW(bmp, size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut _) == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        return true;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let mut px = vec![0u32; (w * h) as usize];
    let mut bi: BITMAPINFO = zeroed();
    bi.bmiHeader = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..zeroed() };
    let dc = GetDC(null_mut());
    let rows = GetDIBits(dc, bmp, 0, h as u32, px.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(null_mut(), dc);
    rows == 0 || px.iter().any(|p| p >> 24 != 0)
}

/// The icon of `path` from the system image list (48 px, `SHIL_EXTRALARGE`), where the mask of an old icon is kept and
/// `ILD_TRANSPARENT` turns it into real transparency. Used when the shell item's bitmap has no alpha at all. Null if there is none.
unsafe fn sysimage_icon(path: &str) -> HICON {
    use windows_sys::{Win32::UI::Controls::{HIMAGELIST, ILD_TRANSPARENT, ImageList_GetIcon}, core::GUID};
    let mut sfi: SHFILEINFOW = zeroed();
    if SHGetFileInfoW(wide(path).as_ptr(), 0, &mut sfi, size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX) == 0 {
        return null_mut();
    }
    const IID_IIMAGELIST: GUID = GUID { data1: 0x46EB5926, data2: 0x582E, data3: 0x4017, data4: [0x9F, 0xDF, 0xE8, 0x99, 0x8D, 0xAA, 0x09, 0x50] };
    let mut list: *mut core::ffi::c_void = null_mut();
    if SHGetImageList(SHIL_EXTRALARGE as i32, &IID_IIMAGELIST, &mut list) < 0 || list.is_null() {
        return null_mut();
    }
    let ic = ImageList_GetIcon(list as HIMAGELIST, sfi.iIcon, ILD_TRANSPARENT);
    // SAFETY: `list` is a COM object; slot 2 of its vtable is IUnknown::Release, dropping the reference SHGetImageList added.
    let release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 = std::mem::transmute(*(*list.cast::<*const usize>()).add(2));
    release(list);
    ic
}

thread_local! {
    static EXE_ICONS: RefCell<HashMap<String, HICON>> = RefCell::new(HashMap::new()); // crisp icons of running apps, by exe
}

/// Icon of a running window for its bar button: its exe's icon at the exact size (cached), else what the window reports.
/// Apps run by a generic host (Store apps in ApplicationFrameHost, Java games in javaw, scripts in python...) would all
/// get the host's icon: those keep the window's own icon.
unsafe fn win_icon(w: HWND) -> HICON {
    let exe = exe_of(w);
    let own = |w| {
        let i = icon_of(w);
        if i.is_null() { LoadIconW(null_mut(), IDI_APPLICATION) } else { i } // never a blank button (#91)
    };
    if exe.is_empty() || is_host(&exe) {
        // a Store app lives in ApplicationFrameHost: the shell draws the icon of the app id of the frame's CoreWindow
        if exe.ends_with("applicationframehost.exe") {
            let ic = store_icon(w);
            if !ic.is_null() {
                return ic;
            }
        }
        return own(w);
    }
    let ic = EXE_ICONS.with_borrow_mut(|m| *m.entry(exe.clone()).or_insert_with(|| item_icon(&exe, bar_icon_px())));
    if ic.is_null() { own(w) } else { ic }
}

fn is_host(exe: &str) -> bool {
    const HOSTS: [&str; 8] = ["applicationframehost.exe", "javaw.exe", "java.exe", "pythonw.exe", "python.exe", "mmc.exe", "rundll32.exe", "dllhost.exe"];
    HOSTS.iter().any(|h| exe.rsplit('\\').next() == Some(*h))
}

/// Icon of the Store app behind an ApplicationFrameHost window (cached by app id), null if it cannot be found.
unsafe fn store_icon(frame: HWND) -> HICON {
    use windows_sys::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    let core = FindWindowExW(frame, null_mut(), wide("Windows.UI.Core.CoreWindow").as_ptr(), null());
    if core.is_null() {
        return null_mut();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(core, &mut pid);
    let p = OpenProcess(windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if p.is_null() {
        return null_mut();
    }
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    let ok = GetApplicationUserModelId(p, &mut len, buf.as_mut_ptr()) == 0;
    CloseHandle(p);
    if !ok {
        return null_mut();
    }
    let id = format!(r"shell:AppsFolder\{}", wstr_of(&buf));
    EXE_ICONS.with_borrow_mut(|m| *m.entry(id.clone()).or_insert_with(|| item_icon(&id, bar_icon_px())))
}

/// "c:\program files\windowsapps\claude_5.1.0.0_x64__pzs8\app\claude.exe" -> ("claude_pzs8", "app\claude.exe"):
/// the package family (name + publisher id, the same across versions) and the path inside the package folder.
fn msix_family(target: &str) -> Option<(String, String)> {
    let l = target.to_lowercase();
    let (_, tail) = l.split_once(r"\windowsapps\")?;
    let (folder, rest) = tail.split_once('\\')?;
    let parts: Vec<&str> = folder.split('_').collect(); // name_version_arch_resourceid_publisherid
    (parts.len() == 5).then(|| (format!("{}_{}", parts[0], parts[4]), rest.to_string()))
}

/// A pin of a Store (MSIX) app points into its versioned package folder, which is gone after every app update
/// ("Windows cannot find ...\Claude_5.11492.5.0_x64__...\claude.exe"). Re-points the .lnk to the installed version.
/// Returns the new target (lower-case), None if the target exists or is not in a package.
unsafe fn fix_msix_pin(lnk: &[u16], target: &str) -> Option<String> {
    use windows_sys::Win32::Storage::Packaging::Appx::{GetPackagePathByFullName, GetPackagesByPackageFamily};
    if target.is_empty() || std::path::Path::new(target).exists() {
        return None;
    }
    let (family, rest) = msix_family(target)?;
    let fam = wide(&family);
    let (mut count, mut len) = (0u32, 0u32);
    GetPackagesByPackageFamily(fam.as_ptr(), &mut count, null_mut(), &mut len, null_mut()); // sizes only
    if count == 0 {
        return None; // the app was uninstalled
    }
    let (mut names, mut buf) = (vec![null_mut::<u16>(); count as usize], vec![0u16; len as usize]);
    if GetPackagesByPackageFamily(fam.as_ptr(), &mut count, names.as_mut_ptr(), &mut len, buf.as_mut_ptr()) != 0 {
        return None;
    }
    let mut path = [0u16; 520];
    let mut plen = path.len() as u32;
    if GetPackagePathByFullName(names[0], &mut plen, path.as_mut_ptr()) != 0 {
        return None;
    }
    let new = std::path::Path::new(&wstr_of(&path)).join(&rest);
    if !new.exists() {
        return None;
    }
    let new = new.to_string_lossy().to_lowercase();
    let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
    let pf = sl.cast::<IPersistFile>().ok()?;
    pf.Load(PCWSTR(lnk.as_ptr()), STGM(0)).ok()?;
    sl.SetPath(PCWSTR(wide(&new).as_ptr())).ok()?;
    pf.Save(PCWSTR(lnk.as_ptr()), true).ok()?;
    Some(new)
}

fn pin_order_file() -> PathBuf {
    appdata_file("topbar-pin-order.txt")
}

/// The pins' current order, as the .lnk file names, one per line.
unsafe fn save_pin_order() {
    let text: String = PINS.with_borrow(|p| p.iter().map(|x| format!("{}
", std::path::Path::new(&wstr_of(&x.lnk)).file_name().unwrap_or_default().to_string_lossy())).collect());
    let _ = std::fs::write(pin_order_file(), text);
}

/// What makes two pins the same app: the target exe and its arguments, or the app id of a Store app (no file target).
/// ("", "") when neither is known.
unsafe fn pin_key(lnk: &[u16]) -> (String, String) {
    let target = lnk_target(lnk);
    if !target.is_empty() {
        return (target, lnk_args(lnk).to_lowercase());
    }
    let id = lnk_aumid(lnk).to_lowercase();
    (if id.is_empty() { id } else { format!("aumid:{id}") }, String::new())
}

/// The AppUserModelID a shortcut carries (Store and web app pins), "" if none.
unsafe fn lnk_aumid(lnk: &[u16]) -> String {
    use windows::{core::GUID, Win32::{Foundation::PROPERTYKEY, System::Com::{CoTaskMemFree, StructuredStorage::PropVariantToStringAlloc}, UI::Shell::PropertiesSystem::IPropertyStore}};
    (|| -> Option<String> {
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        sl.cast::<IPersistFile>().ok()?.Load(PCWSTR(lnk.as_ptr()), STGM(0)).ok()?;
        let ps: IPropertyStore = sl.cast().ok()?;
        let key = PROPERTYKEY { fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3), pid: 5 }; // PKEY_AppUserModel_ID
        let v = ps.GetValue(&key).ok()?;
        let p = PropVariantToStringAlloc(&v).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    })()
    .unwrap_or_default()
}

/// The AppUserModelID a window carries (Store apps, installed web apps), "" if none.
unsafe fn win_aumid(h: HWND) -> String {
    use windows::{core::GUID, Win32::{Foundation::{HWND as WHWND, PROPERTYKEY}, System::Com::{CoTaskMemFree, StructuredStorage::PropVariantToStringAlloc}, UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow}}};
    (|| -> Option<String> {
        let ps: IPropertyStore = SHGetPropertyStoreForWindow(WHWND(h)).ok()?;
        let key = PROPERTYKEY { fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3), pid: 5 }; // PKEY_AppUserModel_ID
        let p = PropVariantToStringAlloc(&ps.GetValue(&key).ok()?).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    })()
    .unwrap_or_default()
    .to_lowercase()
}

/// Taskbar pins the user removed from our bar: Windows still lists them in `FavoritesResolve`, they must not come back.
fn unpinned_file() -> PathBuf {
    appdata_file(r"topbar-taskband\unpinned.txt")
}

fn key_line(k: &(String, String)) -> String {
    format!("{}|{}", k.0, k.1)
}

/// Windows keeps every taskbar pin, in order, as serialized shortcuts in `Taskband\FavoritesResolve` (also those with no
/// .lnk in the TaskBar folder: Steam, Chrome app shortcuts, #93). Each one is written out as a .lnk of our own, named by
/// target and arguments so the name stays the same between runs. Returns (key = target + args, file) in the taskbar's order.
unsafe fn taskband_links() -> Vec<((String, String), PathBuf)> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RegGetValueW};
    let mut n = 0u32;
    let (key, val) = (wide(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband"), wide("FavoritesResolve"));
    if RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_BINARY, null_mut(), null_mut(), &mut n) != 0 || n == 0 {
        return vec![];
    }
    let mut blob = vec![0u8; n as usize];
    if RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_BINARY, null_mut(), blob.as_mut_ptr().cast(), &mut n) != 0 {
        return vec![];
    }
    // every entry starts with the ShellLink header (size 0x4C + the shell link CLSID)
    const HEAD: [u8; 20] = [0x4C, 0, 0, 0, 0x01, 0x14, 0x02, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
    let starts: Vec<usize> = (0..blob.len().saturating_sub(HEAD.len())).filter(|&i| blob[i..i + HEAD.len()] == HEAD).collect();
    let dir = appdata_file("topbar-taskband");
    let _ = std::fs::create_dir_all(&dir);
    // copies from an earlier run: a pin removed in Windows since must not linger
    std::fs::read_dir(&dir).into_iter().flatten().flatten().filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk"))).for_each(|e| {
        let _ = std::fs::remove_file(e.path());
    });
    let tmp = dir.join("tmp.lnk");
    let gone: Vec<String> = std::fs::read_to_string(unpinned_file()).unwrap_or_default().lines().map(str::to_string).collect();
    let mut out = vec![];
    for (k, &a) in starts.iter().enumerate() {
        let b = starts.get(k + 1).copied().unwrap_or(blob.len());
        if std::fs::write(&tmp, &blob[a..b]).is_err() {
            continue;
        }
        let key = pin_key(&wide(&tmp.to_string_lossy()));
        if key.0.is_empty() || gone.contains(&key_line(&key)) {
            continue;
        }
        // most entries point at a .lnk (the pin's own in the TaskBar folder, or one in ImplicitAppShortcuts for a browser
        // profile): that file is the pin; one whose file is gone (File Explorer replaced by rust-explorer) is no pin at all.
        // The unpinned list holds the resolved key (unpin() reads it from that same file), so it is checked after resolving.
        if key.0.ends_with(".lnk") {
            let real = PathBuf::from(&key.0);
            let rk = pin_key(&wide(&real.to_string_lossy()));
            if real.is_file() && !rk.0.is_empty() && !gone.contains(&key_line(&rk)) {
                out.push((rk, real));
            }
            continue;
        }
        // a readable, stable file name: the exe's name (or the app id's) plus a hash of the whole key
        let base = key.0.strip_prefix("aumid:").unwrap_or_else(|| std::path::Path::new(&key.0).file_stem().and_then(|s| s.to_str()).unwrap_or(""));
        let stem: String = base.chars().map(|c| if c.is_alphanumeric() || "._-".contains(c) { c } else { '_' }).take(40).collect();
        let h = key_line(&key).bytes().fold(5381u32, |h, c| h.wrapping_mul(33) ^ c as u32);
        let file = dir.join(format!("{stem}-{h:08x}.lnk"));
        if std::fs::copy(&tmp, &file).is_ok() {
            out.push((key, file));
        }
    }
    let _ = std::fs::remove_file(tmp);
    out
}

/// Pinned apps (the .lnk files Windows keeps for the taskbar) with their icons and target exe.
unsafe fn load_pins() {
    let Some(appdata) = std::env::var_os("APPDATA") else { return };
    let dir = PathBuf::from(appdata).join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar");
    let mut files: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"))).collect();
    files.sort();
    files.sort_by_key(|f| std::cmp::Reverse(lnk_uses(f))); // most used first (stable: ties stay alphabetical)
    // the Windows taskbar's own order (#90) and the pins that have no file in the folder (#93)
    let fav = taskband_links();
    let key_of = |f: &PathBuf| pin_key(&wide(&f.to_string_lossy()));
    let mut keys: Vec<_> = files.iter().map(key_of).collect();
    for (k, f) in &fav {
        if !keys.contains(k) {
            keys.push(k.clone());
            files.push(f.clone());
        }
    }
    let at: HashMap<PathBuf, usize> = files.iter().cloned().zip(&keys).map(|(f, k)| (f, fav.iter().position(|(fk, _)| fk == k).unwrap_or(usize::MAX))).collect();
    files.sort_by_key(|f| at[f]); // stable: pins the taskbar does not list keep the most-used order, after the listed ones
    // an order the user set (Move left / right) wins; pins not in it keep the order above, after the listed ones
    let order: Vec<String> = std::fs::read_to_string(pin_order_file()).unwrap_or_default().lines().map(|l| l.to_lowercase()).collect();
    files.sort_by_key(|f| order.iter().position(|n| *n == f.file_name().unwrap_or_default().to_string_lossy().to_lowercase()).unwrap_or(usize::MAX));
    for f in files {
        let lnk = wide(&f.to_string_lossy());
        let target = lnk_target(&lnk);
        let target = fix_msix_pin(&lnk, &target).unwrap_or(target);
        let icon = lnk_icon(&lnk, &target, bar_icon_px());
        let args = lnk_args(&lnk);
        let aumid = lnk_aumid(&lnk).to_lowercase();
        PINS.with_borrow_mut(|p| p.push(Pin { lnk, target, args, aumid, icon }));
    }
    // once: a File Explorer button for the installed file manager (the marker keeps an unpinned one gone)
    let marker = appdata_file("topbar-explorer-pin.txt");
    let rx = std::env::var_os("ProgramFiles").map(|pf| PathBuf::from(pf).join(r"rust-explorer\rust-explorer.exe"));
    if let Some(rx) = rx.filter(|rx| rx.is_file() && !marker.exists()) {
        pin_exe(&rx.to_string_lossy(), "");
        let _ = std::fs::write(marker, "1");
    }
}

/// The exe the File Explorer pin stands for (lower-case): its icon location, "%windir%\explorer.exe".
fn file_explorer_exe() -> String {
    expand_env(r"%windir%\explorer.exe").to_lowercase()
}

/// The windows of WinRust's file manager belong to the File Explorer pin when there is one: that pin is what the user clicks
/// to open "Explorer" here, and only windows of explorer.exe matched it, so its button never showed them (a minimized
/// window looked gone and the click started a second one). Without such a pin they keep their own exe's button.
fn as_pinned_exe(exe: &str, explorer: &str, explorer_pinned: bool) -> String {
    if explorer_pinned && exe.ends_with(r"\rust-explorer\rust-explorer.exe") { explorer.to_string() } else { exe.to_string() }
}

fn refresh_buttons() {
    let mut wins: Vec<HWND> = Vec::new();
    unsafe { EnumWindows(Some(collect), &mut wins as *mut _ as LPARAM) };
    let mut btns = vec![Btn { pin: None, wins: vec![], icon: null_mut() }];
    PINS.with_borrow(|pins| btns.extend((0..pins.len()).map(|i| Btn { pin: Some(i), wins: vec![], icon: null_mut() })));
    let mut loose: Vec<HWND> = Vec::new();
    let explorer = file_explorer_exe();
    let explorer_pinned = PINS.with_borrow(|p| p.iter().any(|p| p.target == explorer));
    for w in wins {
        let exe = unsafe { exe_of(w) };
        let exe = as_pinned_exe(&exe, &explorer, explorer_pinned);
        // pins of one exe (two browser profiles, #132) tell their windows apart by AppUserModelID; a pin or window without one
        // takes any window of its exe
        let pin = PINS.with_borrow(|p| {
            let mine: Vec<usize> = (0..p.len()).filter(|&i| !p[i].target.is_empty() && p[i].target == exe).collect();
            if mine.is_empty() {
                return None;
            }
            let id = unsafe { win_meta(w).0 };
            mine.iter().copied().find(|&i| !id.is_empty() && p[i].aumid == id).or_else(|| mine.iter().copied().find(|&i| id.is_empty() || p[i].aumid.is_empty()))
        });
        match pin {
            Some(i) => btns[1 + i].wins.push(w),
            None => loose.push(w),
        }
    }
    // EnumWindows lists by z-order, so clicking a window would jump its button to the front: keep the order they first appeared in
    let mut keys: Vec<String> = vec![String::new(); btns.len()]; // keys[i] belongs to btns[i]; "" matches nothing
    ORDER.with_borrow_mut(|o| {
        o.retain(|w| loose.contains(w));
        o.extend(loose.iter().filter(|w| !o.contains(w)).copied().collect::<Vec<_>>());
        for &w in o.iter() {
            // windows of one app share a button (a click then offers the list); generic hosts (Store apps, java...) stay apart (#90)
            // the window's own AppUserModelID is part of the key: a web app installed from Brave runs as brave.exe too (#118, #117)
            let exe = unsafe { exe_of(w) };
            let key = format!("{exe}|{}", unsafe { win_meta(w).0 });
            let same = (!exe.is_empty() && !is_host(&exe)).then(|| keys.iter().position(|k| *k == key)).flatten();
            match same {
                Some(i) => btns[i].wins.push(w),
                None => {
                    btns.push(Btn { pin: None, wins: vec![w], icon: unsafe { win_meta(w).1 } });
                    keys.push(key);
                }
            }
        }
    });
    BTNS.set(btns);
}

thread_local! {
    // hwnd -> (pid, when read, AppUserModelID, icon): the AUMID is a COM call and a host app's icon up to two 40 ms
    // WM_GETICON round trips, too much for every refresh; re-read after META_TTL (apps set both shortly after start)
    static META: RefCell<HashMap<usize, (u32, u64, String, HICON)>> = RefCell::new(HashMap::new());
}
const META_TTL: u64 = 4000;

unsafe fn win_meta(w: HWND) -> (String, HICON) {
    let mut pid = 0u32;
    GetWindowThreadProcessId(w, &mut pid);
    let now = now_ms();
    if let Some(v) = META.with_borrow(|m| m.get(&(w as usize)).filter(|e| e.0 == pid && now.saturating_sub(e.1) < META_TTL).map(|e| (e.2.clone(), e.3))) {
        return v;
    }
    let v = (win_aumid(w), win_icon(w));
    META.with_borrow_mut(|m| {
        if m.len() > 512 {
            m.clear(); // closed windows never leave on their own
        }
        m.insert(w as usize, (pid, now, v.0.clone(), v.1));
    });
    v
}

/// A minimized window that Windows left as a caption stub on the desktop (it sits at the bottom left with its restore /
/// maximize / close buttons) goes off screen, as explorer's taskbar would have done. Restoring it is unaffected.
unsafe fn hide_stub(w: HWND) {
    let mut r: RECT = zeroed();
    if IsIconic(w) != 0 && GetWindowRect(w, &mut r) != 0 && r.left > -30000 {
        SetWindowPos(w, null_mut(), -32000, -32000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn refresh() {
    refresh_windows();
    refresh_status();
}

/// The buttons: on window events (win_event) and every few seconds as a safety net.
fn refresh_windows() {
    refresh_buttons();
    BTNS.with_borrow(|b| b.iter().flat_map(|b| &b.wins).for_each(|&w| unsafe { hide_stub(w) }));
}

/// Everything polled once a second: battery, volume, wifi, explorer, dead tray icons.
fn refresh_status() {
    let n = TICK.get();
    TICK.set(n + 1);
    unsafe {
        let mut p: SYSTEM_POWER_STATUS = zeroed();
        let ok = GetSystemPowerStatus(&mut p) != 0 && p.BatteryFlag & 128 == 0 && p.BatteryLifePercent <= 100;
        BAT.set(if ok { (p.BatteryLifePercent as i32, p.ACLineStatus == 1) } else { (-1, false) });
        read_volume();
        let up = !find_other("Shell_TrayWnd").is_null();
        if EXPLORER_UP.swap(up, Relaxed) && !up {
            place(BAR.load(Relaxed) as HWND); // Explorer (started for Settings) is gone and took the reserved strip with it
            // icons that apps put into Explorer's (hidden) tray went with it: ask every app to register again, now with us.
            // The empty-tray check below misses this when some icons already reached the bar.
            announce();
        }
        if TAPPS.with_borrow(|t| t.is_empty()) && n % 4 == 2 {
            announce(); // no icons yet (apps started before us, or explorer restarted): ask them to register again
        }
        if n % 3 == 0 {
            read_wifi();
        }
    }
    unsafe {
        // icons of apps that died without NIM_DELETE
        TAPPS.with_borrow_mut(|t| {
            t.retain(|x| {
                let alive = IsWindow(x.hwnd) != 0;
                if !alive {
                    DestroyIcon(x.icon);
                }
                alive
            })
        });
        // explorer re-raises its own Shell_TrayWnd now and then; ours must stay in front so apps keep finding it first
        SetWindowPos(HOST.get(), HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

// ---------- layout / hit-testing ----------

// ---------- Restart and Shut down on the bar (#173): one pill between the system group and the clock ----------

/// Restart or shut down after the dialog of the mockup (a minute's countdown; Esc or Cancel refuse), so a stray click does not
/// end the session.
unsafe fn power_click(key: i32) {
    let kind = if key == POW_R { ui::power_popup::Kind::Restart } else { ui::power_popup::Kind::Shutdown };
    ui::panel::toggle(ui::panel::Id::Power, || {
        let a = anchor(key)?;
        Some((Box::new(ui::power_popup::PowerPanel::new(kind, power_run)), a))
    });
}

/// What the dialog's action does: the same command whether the user confirmed or the countdown ran out.
fn power_run(kind: ui::power_popup::Kind) {
    let arg = if kind == ui::power_popup::Kind::Restart { "/r" } else { "/s" };
    let _ = Command::new("shutdown.exe").args([arg, "/t", "0"]).creation_flags(NO_WINDOW).spawn();
}

/// The bar element under a client point of the bar window (device pixels), straight from the frame's layout table.
unsafe fn hit(x: i32, y: i32) -> i32 {
    UI.with_borrow(|u| u.as_ref().map_or(NONE, |b| b.hit(x, y)))
}

/// Screen position (x for a bottom bar, y for a side bar) of the middle of a bar element, for its tooltip and menus.
unsafe fn tip_cx(h: i32) -> i32 {
    UI.with_borrow(|u| u.as_ref().and_then(|b| b.screen_center(h))).map_or(0, |(x, y)| if vert() { y } else { x })
}

// ---------- animation ----------

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        (x + (y - x) * t) as u32
    };
    ch(0) | ch(8) << 8 | ch(16) << 16
}

fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

unsafe fn now_ms() -> u64 {
    GetTickCount64()
}

/// Eased 0..1 hover value of one element: glides toward `on` (1) / off (0) instead of jumping. Sets BUSY while it moves,
/// the painter then schedules another frame.
unsafe fn hv(key: i32, on: bool) -> f32 {
    let now = now_ms();
    let target = if on { 1.0 } else { 0.0 };
    let v = HV.with_borrow_mut(|m| {
        let e = m.entry(key).or_insert((0.0, now));
        let dt = now.saturating_sub(e.1).min(50) as f32; // a long pause must not make the first frame jump
        e.1 = now;
        e.0 = if e.0 < target { (e.0 + dt / 130.0).min(target) } else { (e.0 - dt / 180.0).max(target) };
        e.0
    });
    if v != target {
        BUSY.set(true);
    }
    ease(v)
}

/// Rounded highlight that fades in and out (the flyouts' rows); `from` is the colour it sits on.
unsafe fn glow(dc: HDC, key: i32, on: bool, r: RECT, from: u32, to: u32, rad: i32) {
    let t = hv(key, on);
    if t > 0.01 {
        fill(dc, r, mix(from, to, t), rad);
    }
}

thread_local! {
    static PRESS: Cell<(i32, u64)> = const { Cell::new((i32::MIN, 0)) }; // last clicked bar element and when
}

/// Marks `key` as just pressed: its icon squeezes for a moment.
unsafe fn press(key: i32) {
    PRESS.set((key, now_ms()));
}

/// True when the foreground window covers the whole primary monitor (game, F11 video, presentation).
/// A maximized window does not: the bar's reserved strip keeps it below the bar.
unsafe fn fullscreen_now(bar: HWND) -> bool {
    let fg = GetForegroundWindow();
    if fg.is_null() || fg == bar || IsWindowVisible(fg) == 0 || IsIconic(fg) != 0 {
        return false;
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(fg, cls.as_mut_ptr(), 64).max(0) as usize;
    if matches!(String::from_utf16_lossy(&cls[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "RustExplorer" | "topbar_panel") {
        return false; // the desktop itself is full size (rs-shell's desktop is a rust-explorer window; a file manager is never "fullscreen")
    }
    let mut r: RECT = zeroed();
    GetWindowRect(fg, &mut r);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi);
    let m = mi.rcMonitor;
    let caption = GetWindowLongW(fg, GWL_STYLE) as u32 & WS_CAPTION == WS_CAPTION;
    (m.left, m.top) == (0, 0) && covers_monitor(r, m, caption, IsZoomed(fg) != 0)
}

/// A framed maximized window overhangs the monitor by its border and is not fullscreen (#88); but a game in "borderless
/// window" mode that keeps its caption style while sized exactly to the monitor (Dota 2 and others) is.
fn covers_monitor(r: RECT, m: RECT, caption: bool, zoomed: bool) -> bool {
    if caption {
        return !zoomed && (r.left, r.top, r.right, r.bottom) == (m.left, m.top, m.right, m.bottom);
    }
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

/// True when no app window is on screen (none open, or all minimized: Win+D): the user is looking at the desktop.
unsafe fn desktop_now() -> bool {
    BTNS.with_borrow(|b| b.iter().flat_map(|b| &b.wins).all(|&w| IsIconic(w) != 0))
}

unsafe fn fs_update(hwnd: HWND) {
    let fs = fullscreen_now(hwnd);
    // a fullscreen game keeps the screen: the pointer at the bottom edge does not bring the bar (games park it at the
    // edges all the time); Win opens Start, which brings the bar with it
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    // desktop in front: the bar leaves 2 s after the pointer does and comes back at the bottom edge (an open flyout keeps it)
    let now = now_ms();
    let sh = GetSystemMetrics(SM_CYSCREEN);
    let sw = GetSystemMetrics(SM_CXSCREEN);
    // the hot zone is a 6 dip strip at the edge; while the bar is out, the pointer over it keeps it there
    let (edge, outer, shown) = (dp(6.0), bar_outer(), bar_visible());
    let over = if vert() {
        if i18n::style::side() == i18n::style::Side::Left { pt.x < edge || (pt.x < outer && shown) } else { pt.x >= sw - edge || (pt.x >= sw - outer && shown) }
    } else {
        pt.y >= sh - edge || (pt.y >= sh - outer && shown)
    };
    if fs || !(i18n::style::autohide() || desktop_now() || dock_covered()) || popup_open() || over {
        DESK_T.set(now);
    }
    let desk = !fs && now.saturating_sub(DESK_T.get()) >= 2000;
    if DESK_HIDE.swap(desk, Relaxed) != desk {
        redraw(hwnd);
    }
    if FULLSCREEN.swap(fs, Relaxed) != fs {
        if fs {
            fly::close_now();
            ui::panel::close_now();
            hide_tip(hwnd);
            hide_preview();
        }
        redraw(hwnd);
    }
    // the 250 ms watch is only needed while the pointer decides (auto-hide, dock, desktop, bar away); otherwise
    // the foreground event, ABN_FULLSCREENAPP and the 1 s tick call this
    let poll = i18n::style::autohide() || dock() || desktop_now() || DESK_HIDE.load(Relaxed);
    if FS_POLL.replace(poll) != poll {
        if poll {
            SetTimer(hwnd, 3, 250, None);
        } else {
            KillTimer(hwnd, 3);
        }
    }
}

thread_local! {
    static FS_POLL: Cell<bool> = const { Cell::new(true) }; // timer 3 runs (set in main)
}

/// The dock is in the way of the window in front (one dragged down under it): it hides like over the
/// desktop and comes back when the pointer touches the bottom edge.
unsafe fn dock_covered() -> bool {
    if !dock() {
        return false;
    }
    let fg = GetForegroundWindow();
    // a maximized window never hides the dock (it could stay gone until the window was restored)
    if fg.is_null() || IsIconic(fg) != 0 || IsZoomed(fg) != 0 || fg == BAR.load(Relaxed) as HWND || popup_open() {
        return false;
    }
    let mut r: RECT = zeroed();
    GetWindowRect(fg, &mut r);
    let Some((x, y, w, h)) = UI.with_borrow(|u| u.as_ref().map(|b| b.placement().rest)) else { return false };
    r.bottom > y && r.top < y + h && r.right > x && r.left < x + w
}

/// SetForegroundWindow that also works when the request comes from a keyboard hook (we have not "received input" then).
unsafe fn force_fg(hw: HWND) {
    winrust_common::fg::force(hw);
}

/// Fills `r` (right / bottom exclusive) in colour `c`; `rad` > 0 rounds the corners (rad = corner diameter), anti-aliased
/// through GDI+. Plain GDI for square boxes and when GDI+ is not available.
unsafe fn fill(dc: HDC, r: RECT, c: u32, rad: i32) {
    let (w, h) = ((r.right - r.left) as f32, (r.bottom - r.top) as f32);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let mut g: *mut GpGraphics = null_mut();
    if rad <= 0 || GdipCreateFromHDC(dc, &mut g) != 0 || g.is_null() {
        let b = CreateSolidBrush(c);
        if rad <= 0 {
            FillRect(dc, &r, b);
        } else {
            let (ob, op) = (SelectObject(dc, b), SelectObject(dc, GetStockObject(NULL_PEN)));
            RoundRect(dc, r.left, r.top, r.right + 1, r.bottom + 1, rad, rad);
            SelectObject(dc, ob);
            SelectObject(dc, op);
        }
        DeleteObject(b);
        return;
    }
    GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
    GdipSetPixelOffsetMode(g, PixelOffsetModeHalf); // pixel i spans [i, i+1): straight edges stay crisp
    let (x, y, d) = (r.left as f32, r.top as f32, (rad as f32).min(w).min(h));
    let mut path: *mut GpPath = null_mut();
    GdipCreatePath(FillModeAlternate, &mut path);
    GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
    GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
    GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
    GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
    GdipClosePathFigure(path);
    let mut br: *mut GpSolidFill = null_mut();
    GdipCreateSolidFill(0xFF00_0000 | (c & 255) << 16 | (c & 0xFF00) | (c >> 16 & 255), &mut br); // COLORREF -> ARGB
    GdipFillPath(g, br as *mut GpBrush, path);
    GdipDeleteBrush(br as *mut GpBrush);
    GdipDeletePath(path);
    GdipDeleteGraphics(g);
}

unsafe fn text(dc: HDC, s: &str, mut r: RECT, c: u32, flags: u32) {
    SetTextColor(dc, c);
    let w = wide(s);
    DrawTextW(dc, w.as_ptr(), -1, &mut r, flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
}

unsafe fn icon_of(h: HWND) -> HICON {
    for which in [1usize, 2] {
        // ICON_BIG, ICON_SMALL2
        let mut r = 0usize;
        SendMessageTimeoutW(h, WM_GETICON, which, 0, SMTO_ABORTIFHUNG, 40, &mut r);
        if r != 0 {
            return r as HICON;
        }
    }
    let c = GetClassLongPtrW(h, GCLP_HICON);
    (if c != 0 { c } else { GetClassLongPtrW(h, GCLP_HICONSM) }) as HICON
}

/// The app window the user is typing in: the foreground window, or, while our own bar / panel / menu holds the
/// foreground (after a right click, a closed Start...), the last app window that had it.
unsafe fn app_window() -> HWND {
    let fg = GetForegroundWindow();
    let mut pid = 0u32;
    GetWindowThreadProcessId(fg, &mut pid);
    if !fg.is_null() && pid != std::process::id() {
        APP_FG.set(fg);
        return fg;
    }
    let last = APP_FG.get();
    if IsWindow(last) != 0 { last } else { fg }
}

thread_local! {
    static WIN_DIRTY: Cell<bool> = const { Cell::new(false) }; // a window refresh is already scheduled (timer 8)
}

/// Repaints (GDI+, the costly part) only when something shown changed; `tick`: every 10th tick anyway as a safety net.
unsafe fn repaint_if_changed(hwnd: HWND, tick: bool) {
    let (last, quiet) = SIG.get();
    let s = view_sig();
    if s != last || (tick && quiet >= 10) {
        SIG.set((s, 0));
        InvalidateRect(hwnd, null(), 0);
    } else if tick {
        SIG.set((s, quiet + 1));
    }
}

/// Window events (WINEVENT_OUTOFCONTEXT: delivered on the bar's thread inside GetMessageW). Instead of enumerating every
/// window twice a second, the bar looks again ~30 ms after something that can change a button happened.
unsafe extern "system" fn win_event(_: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK, ev: u32, w: HWND, id: i32, child: i32, _: u32, _: u32) {
    if id != 0 || child != 0 || w.is_null() {
        return; // only whole windows (not carets, cursors, parts of controls)
    }
    if ev == 0x0017 || ev == 0x8001 { snapshots::remove(w as isize); }
    let bar = BAR.load(Relaxed) as HWND;
    if bar.is_null() {
        return;
    }
    let known = || BTNS.with_borrow(|b| b.iter().any(|b| b.wins.contains(&w)));
    let top = || GetAncestor(w, GA_PARENT) == GetDesktopWindow();
    let relevant = match ev {
        0x0003 => {
            fs_update(bar); // EVENT_SYSTEM_FOREGROUND: the active button and fullscreen may change
            true
        }
        0x8001 => known(),           // destroyed: it can no longer be asked anything, but it may be one of ours
        0x800C => !known() && top(), // title set: a window with no title had no button yet
        _ => top() || known(),       // created, shown, hidden, cloaked, uncloaked, minimized, restored
    };
    if relevant && ev == 0x0016 { snapshots::capture(w); }
    if relevant && !WIN_DIRTY.replace(true) {
        SetTimer(bar, 8, 30, None);
    }
}

/// Hash of everything the bar paints from polled state.
unsafe fn view_sig() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    BTNS.with_borrow(|b| b.iter().for_each(|b| (b.pin, b.icon as usize, b.wins.iter().map(|&w| (w as usize, IsIconic(w))).collect::<Vec<_>>()).hash(&mut h)));
    (GetForegroundWindow() as usize, BAT.get(), VOL.get(), WIFI.get(), INET.get(), EXPLORER_UP.load(Relaxed), clock_texts(), lang_name()).hash(&mut h);
    notify::with(|s| (s.badge(), s.dnd())).hash(&mut h);
    TAPPS.with_borrow(|t| t.iter().for_each(|x| (x.hwnd as usize, x.uid, x.icon as usize, x.hidden, &x.tip).hash(&mut h)));
    h.finish()
}

unsafe fn clock_texts() -> (String, String) {
    let mut t: SYSTEMTIME = zeroed();
    GetLocalTime(&mut t);
    let (mut a, mut b) = ([0u16; 40], [0u16; 40]);
    let n = GetTimeFormatW(0x0400, 2, &t, null(), a.as_mut_ptr(), 40).max(1) as usize - 1; // LOCALE_USER_DEFAULT, TIME_NOSECONDS
    let m = GetDateFormatW(0x0400, 1, &t, null(), b.as_mut_ptr(), 40).max(1) as usize - 1; // DATE_SHORTDATE
    (String::from_utf16_lossy(&a[..n]), String::from_utf16_lossy(&b[..m]))
}

/// The dominant saturated colour of an icon (what its window indicator is painted in), `None` for grey ones.
unsafe fn dominant(ic: HICON) -> Option<ui::gfx::Rgba> {
    let mut ii: ICONINFO = zeroed();
    if GetIconInfo(ic, &mut ii) == 0 {
        return None;
    }
    if !ii.hbmMask.is_null() {
        DeleteObject(ii.hbmMask);
    }
    let color = ii.hbmColor;
    if color.is_null() {
        return None;
    }
    let mut bm: BITMAP = zeroed();
    GetObjectW(color, size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut _);
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let mut px = vec![0u32; (w.max(0) * h.max(0)) as usize];
    if !px.is_empty() {
        let mut bi: BITMAPINFO = zeroed();
        bi.bmiHeader = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..zeroed() };
        let dc = GetDC(null_mut());
        GetDIBits(dc, color, 0, h as u32, px.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
        ReleaseDC(null_mut(), dc);
    }
    DeleteObject(color);
    let alpha_used = px.iter().any(|p| p >> 24 != 0);
    // 12 hue buckets, each pixel weighing saturation x brightness: the loudest hue wins over big grey areas
    let mut bucket = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); 12];
    for &p in &px {
        if alpha_used && p >> 24 < 128 {
            continue;
        }
        let (r, g, b) = (((p >> 16) & 255) as f32 / 255.0, ((p >> 8) & 255) as f32 / 255.0, (p & 255) as f32 / 255.0);
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        let (sat, d) = (if mx > 0.0 { (mx - mn) / mx } else { 0.0 }, mx - mn);
        if sat < 0.35 || mx < 0.35 {
            continue;
        }
        let hue = if mx == r { ((g - b) / d).rem_euclid(6.0) } else if mx == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
        let k = (hue * 2.0) as usize % 12;
        let wt = sat * mx;
        let e = &mut bucket[k];
        *e = (e.0 + wt, e.1 + r * wt, e.2 + g * wt, e.3 + b * wt);
    }
    let best = bucket.iter().max_by(|a, b| a.0.total_cmp(&b.0))?;
    if best.0 < (w * h) as f32 * 0.02 {
        return None;
    }
    let c = ui::gfx::Rgba::new(best.1 / best.0, best.2 / best.0, best.3 / best.0, 1.0);
    Some(c.mix(ui::gfx::Rgba::white(1.0), 0.12))
}

thread_local! {
    static ICON_COLOR: RefCell<HashMap<usize, Option<ui::gfx::Rgba>>> = RefCell::new(HashMap::new());
}

/// The window indicator colour of an icon: its dominant hue, worked out once per icon; the accent for grey ones.
unsafe fn icon_color(ic: HICON) -> ui::gfx::Rgba {
    let c = ICON_COLOR.with_borrow_mut(|m| {
        if m.len() > 256 {
            m.clear();
        }
        *m.entry(ic as usize).or_insert_with(|| if ic.is_null() { None } else { dominant(ic) })
    });
    c.unwrap_or_else(|| ui::gfx::Rgba::colorref(pal().accent))
}

fn whwnd(h: HWND) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(h.cast())
}

fn wicon(ic: HICON) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    windows::Win32::UI::WindowsAndMessaging::HICON(ic.cast())
}

/// What the bar shows now, gathered from the program's state.
unsafe fn build_view() -> barui::View {
    let s = SCALE.get();
    let fg = GetForegroundWindow();
    let apps = BTNS.with_borrow(|btns| {
        btns.iter()
            .skip(1) // Start is the layout's own button
            .map(|b| {
                let ic = match b.pin {
                    Some(p) => PINS.with_borrow(|pins| pins.get(p).map_or(null_mut(), |p| p.icon)),
                    None => b.icon,
                };
                let ic = if ic.is_null() { b.wins.first().map_or(ic, |&w| icon_of(w)) } else { ic }; // a pin whose icon failed to load (#91)
                let active = b.wins.contains(&fg);
                let flash = !active && FLASH.with_borrow(|f| b.wins.iter().any(|w| f.contains(w))); // lit until it is activated
                barui::AppView { icon: wicon(ic), wins: b.wins.len(), active, flash, color: icon_color(ic) }
            })
            .collect()
    });
    let tray = {
        let bar = bar_idx();
        TAPPS.with_borrow(|t| bar.iter().map(|&i| wicon(t[i].icon)).collect())
    };
    let (time, date) = clock_texts();
    let (bat, ac) = BAT.get();
    let (vol, muted) = VOL.get();
    barui::View {
        style: style(),
        side: i18n::style::side(),
        size: i18n::style::bar_size(),
        align_left: i18n::style::align_left(),
        screen: (GetSystemMetrics(SM_CXSCREEN) as f32 / s, GetSystemMetrics(SM_CYSCREEN) as f32 / s),
        op: i18n::style::bar_opacity() as f32 / 100.0,
        apps,
        tray,
        battery: (bat >= 0).then_some((bat, ac)),
        vol: (if vol < 0 { 50 } else { vol }, muted),
        net: match net() {
            Link::Wifi(q) => barui::Net::Wifi(q),
            Link::NoInternet(q) => barui::Net::NoInternet(q),
            Link::Cable => barui::Net::Cable,
            Link::Offline => barui::Net::Offline,
        },
        lang: lang_name(),
        time,
        date,
        unread: notify::with(|s| s.badge()).unwrap_or(0),
        dnd: notify::with(|s| s.dnd()),
    }
}

/// The pointer and the open panels.
unsafe fn build_live() -> barui::Live {
    // the centre is the calendar's: open, it has seen what arrived (and the pop-ups are gone); closed, it has been seen
    let cal = ui::panel::is_open(ui::panel::Id::Cal);
    if notify::with(|s| s.centre_open()) != cal {
        notify::with(|s| if cal { s.open_centre() } else { s.close_centre() });
        ui::notice_pop::changed(None);
    }
    let now = now_ms();
    let (pk, pt) = PRESS.get();
    barui::Live {
        now,
        hot: HOT.get(),
        press: if now.saturating_sub(pt) < 220 { pk } else { NONE },
        gear: ui::panel::is_open(ui::panel::Id::Bar),
        start: ui::panel::is_open(ui::panel::Id::Start),
        search: ui::panel::is_open(ui::panel::Id::Search),
        qs: ui::panel::is_open(ui::panel::Id::Qs),
        cal,
        lang: ui::panel::is_open(ui::panel::Id::Lang),
        tray: ui::panel::is_open(ui::panel::Id::Tray),
        task: ui::panel::is_open(ui::panel::Id::Task),
        jump: if ui::panel::is_open(ui::panel::Id::Jump) { JL_KEY.get() } else { None },
        hidden: FULLSCREEN.load(Relaxed) || (i18n::style::autohide() && DESK_HIDE.load(Relaxed)),
    }
}

/// Draws one frame of the bar (and of its shadow) and puts it on screen; asks for the next one while anything moves.
/// Everything that wants the bar redrawn invalidates it, WM_PAINT lands here.
unsafe fn redraw(hwnd: HWND) {
    let live = build_live(); // first: it tells the notifications whether their centre is open, and the badge follows
    let view = build_view();
    let Some(p) = UI.with_borrow_mut(|u| u.as_mut().map(|b| b.frame(&view, &live))) else { return };
    let shadow = SHADOW.get();
    if p.gone {
        ShowWindow(hwnd, SW_HIDE); // not just moved away: a hidden bar cannot cover a game
        ShowWindow(shadow, SW_HIDE);
        pacing::set(hwnd as isize, false);
        return;
    }
    UI.with_borrow(|u| {
        if let Some(b) = u {
            b.present(whwnd(hwnd), whwnd(shadow));
        }
    });
    if IsWindowVisible(hwnd) == 0 {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    // the shadow of the floating styles stands right under the bar; the flat style has none
    // (backdrop-filter: blur(30px) saturate(1.4) of the mockup would be a third, "glass" window here, behind the bar)
    if p.shadow.is_some() {
        if IsWindowVisible(shadow) == 0 {
            ShowWindow(shadow, SW_SHOWNOACTIVATE);
        }
        SetWindowPos(shadow, hwnd, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    } else {
        ShowWindow(shadow, SW_HIDE);
    }
    pacing::set(hwnd as isize, p.busy || live.press != NONE);
}

// ---------- input ----------

/// Presses the keys in order, releases them in reverse (e.g. Win+A).
unsafe fn keys(vks: &[u16]) {
    let send = |vk: u16, up: bool| {
        let mut i: INPUT = zeroed();
        i.r#type = INPUT_KEYBOARD;
        i.Anonymous.ki.wVk = vk;
        i.Anonymous.ki.dwFlags = if up { KEYEVENTF_KEYUP } else { 0 };
        SendInput(1, &i, size_of::<INPUT>() as i32);
    };
    vks.iter().for_each(|&v| send(v, false));
    vks.iter().rev().for_each(|&v| send(v, true));
}

/// Releases `vk` alone: a swallowed key-up handed back to the shell as a synthetic one (see `run::kb_hook`).
unsafe fn key_up(vk: u16) {
    let mut i: INPUT = zeroed();
    i.r#type = INPUT_KEYBOARD;
    i.Anonymous.ki.wVk = vk;
    i.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
    SendInput(1, &i, size_of::<INPUT>() as i32);
}

/// WM_CLOSE. A window of an elevated app ignores it from our normal-rights process (UIPI), so then
/// ask for admin once (UAC) and let an elevated taskkill send the close request.
unsafe fn close_window(w: HWND) {
    if PostMessageW(w, WM_CLOSE, 0, 0) == 0 {
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, &mut pid);
        ShellExecuteW(null_mut(), wide("runas").as_ptr(), wide("taskkill.exe").as_ptr(), wide(&format!("/PID {pid}")).as_ptr(), null(), SW_HIDE);
    }
}

/// App id of the Store app a .lnk of a pin points into (its package folder), None for anything else.
unsafe fn msix_app_id(lnk: &str) -> Option<String> {
    let (family, _) = msix_family(&lnk_target(&wide(lnk)))?;
    let prefix = format!("{family}!").to_lowercase();
    apps::apps_folder().into_iter().map(|a| a.1).find(|id| id.to_lowercase().starts_with(&prefix))
}

/// Starts a Store app by its app id. false = it did not start (the caller falls back to explorer).
unsafe fn activate_app(aumid: &str) -> bool {
    use windows::Win32::UI::Shell::{ACTIVATEOPTIONS, ApplicationActivationManager, IApplicationActivationManager};
    CoCreateInstance::<_, IApplicationActivationManager>(&ApplicationActivationManager, None, CLSCTX_ALL)
        .and_then(|m| m.ActivateApplication(PCWSTR(wide(aumid).as_ptr()), PCWSTR::null(), ACTIVATEOPTIONS(0)))
        .is_ok()
}

/// Opens a file, shortcut, URI or program with the desktop user's normal token: the bar itself may run elevated (see the
/// launcher), and everything started from it must not inherit that.
unsafe fn open(path: &[u16]) {
    let n = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    let p = String::from_utf16_lossy(&path[..n]);
    if p.to_lowercase().ends_with(".lnk") {
        bump(&p);
    }
    if p.to_lowercase().starts_with("ms-settings:") {
        return winrust_common::settings::open(&p); // needs Explorer as shell: it is started for the time Settings is open
    }
    // Store (MSIX) apps: by app id through the activation manager. A path into the protected package folder ("no app
    // associated") and shell:AppsFolder via explorer ("class not registered" when Explorer is not the shell) both fail.
    let aumid = if p.to_lowercase().starts_with(r"shell:appsfolder\") { Some(p[r"shell:appsfolder\".len()..].to_string()) } else { msix_app_id(&p) };
    if aumid.is_some_and(|id| activate_app(&id)) {
        return;
    }
    // A program is a path to Explorer, and Explorer is not the shell on a WinRust machine: `explorer.exe wt.exe` only
    // opens a folder window, so the Win+X "Terminal" item showed the file manager and a task manager never started.
    if winrust_common::launch::is_program(&p) {
        winrust_common::launch::program(&p);
        return;
    }
    // Everything else through explorer.exe: it hands paths, shortcuts, shell addresses and URIs to the shell with the
    // desktop user's normal token. ShellExecuteW from here would inherit this bar's admin rights, and an elevated app
    // cannot receive files dragged in from normal windows (UIPI), and PrtSc / Win+Shift+S (ms-screenclip:) do not work
    // when started elevated. With an argument and no explorer shell running, explorer.exe hands the target to its COM
    // host and does not become a shell or raise a taskbar.
    if Command::new("explorer.exe").arg(&p).spawn().is_err() {
        ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(&p).as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

/// Starts the disk cleaner (rs-du, installed next to the task manager) elevated: it reads the raw volume.
/// Right-click menu of an app button, like the Windows taskbar's jump list: pin / unpin, close, end process.
fn wstr_of(w: &[u16]) -> String {
    String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())])
}

/// "1 окно", "3 окна", "5 окон"; "1 window", "2 windows"; "3 个窗口" (no plural in Chinese).
fn windows_ru(n: usize) -> String {
    use i18n::Lang;
    let w = match lang!() {
        Lang::En => if n == 1 { "window" } else { "windows" },
        Lang::Pt => if n == 1 { "janela" } else { "janelas" },
        Lang::Ja => "ウィンドウ",
        Lang::Zh => "个窗口",
        Lang::Ru => match (n % 10, n % 100) {
            (1, x) if x != 11 => "окно",
            (2..=4, x) if !(12..=14).contains(&x) => "окна",
            _ => "окон",
        },
    };
    format!("{n} {w}")
}

/// Right click on an app button: its jump list (`ui::jump`). The app on top, its windows, its recent files when Windows keeps
/// a list for it, then open / pin / move / Start, and below close and end process in red.
unsafe fn jump_list(h: i32) {
    use ui::jump::{Act, Data, JumpPanel, Pick, Recent, Win};
    #[derive(Clone, Copy)]
    enum A {
        Open,
        Pin,
        Left,
        Right,
        Start,
    }
    if ui::panel::is_open(ui::panel::Id::Jump) && JL_KEY.get() == Some(h) {
        return ui::panel::close(); // the same button again
    }
    let Some(anchor) = anchor(h) else { return };
    let (wins, pin, bicon) = BTNS.with_borrow(|b| b.get(h as usize).map(|b| (b.wins.clone(), b.pin, b.icon)).unwrap_or((vec![], None, null_mut())));
    let exe = wins.first().map(|&w| exe_of(w)).unwrap_or_default();
    let (lnk, picon) = pin.and_then(|p| PINS.with_borrow(|p2| p2.get(p).map(|x| (wstr_of(&x.lnk), x.icon)))).unwrap_or_default();
    let launch = if lnk.is_empty() { exe.clone() } else { lnk.clone() };
    let stem = std::path::Path::new(&launch).file_stem().unwrap_or_default().to_string_lossy().to_string();
    let name: String = stem.chars().take(1).flat_map(char::to_uppercase).chain(stem.chars().skip(1)).collect();
    let meta = match (wins.len(), pin.is_some()) {
        (0, _) => t!("Закреплено · не запущено", "Pinned · not running", ja: "固定済み · 未起動", pt: "Fixado · não está em execução", zh: "已固定 · 未运行").to_string(),
        (n, true) => tf!("Закреплено · {}", "Pinned · {}", ja: "固定済み · {}", pt: "Fixado · {}", zh: "已固定 · {}", windows_ru(n)),
        (n, false) => tf!("Запущено · {}", "Running · {}", ja: "実行中 · {}", pt: "Em execução · {}", zh: "运行中 · {}", windows_ru(n)),
    };
    let icon = if picon.is_null() { bicon } else { picon };
    let fg = GetForegroundWindow();
    let rows: Vec<Win> = wins.iter().take(6).map(|&w| Win { hwnd: w as isize, title: window_title(w), active: w == fg }).collect();

    let mut acts: Vec<Act> = Vec::new();
    let mut what: Vec<A> = Vec::new();
    let mut add = |a: A, icon: &'static str, label: &str| {
        acts.push(Act { icon, label: label.to_string() });
        what.push(a);
    };
    if !launch.is_empty() {
        add(A::Open, "M4 5h16v14H4zM12 9v6M9 12h6", if wins.is_empty() { t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开") } else { t!("Новое окно", "New window", ja: "新しいウィンドウ", pt: "Nova janela", zh: "新建窗口") });
    }
    if pin.is_some() {
        add(A::Pin, "M12 17v5M9 4h6l-1 5 3 3v2H7v-2l3-3zM3 3l18 18", t!("Открепить от панели", "Unpin from the bar", ja: "バーから固定を外す", pt: "Desafixar da barra", zh: "从任务栏取消固定"));
    } else if !exe.is_empty() {
        add(A::Pin, "M12 17v5M9 4h6l-1 5 3 3v2H7v-2l3-3z", t!("Закрепить на панели", "Pin to the bar", ja: "バーに固定する", pt: "Fixar na barra", zh: "固定到任务栏"));
    }
    if let Some(p) = pin.filter(|_| !lnk.is_empty()) {
        let n = PINS.with_borrow(|p| p.len());
        if p > 0 {
            add(A::Left, if vert() { "M6 15l6-6 6 6" } else { "M15 6l-6 6 6 6" }, if vert() { t!("Выше", "Move up", ja: "上へ移動", pt: "Mover para cima", zh: "上移") } else { t!("Сдвинуть влево", "Move left", ja: "左へ移動", pt: "Mover para a esquerda", zh: "左移") });
        }
        if p + 1 < n {
            add(A::Right, if vert() { "M6 9l6 6 6-6" } else { "M9 6l6 6-6 6" }, if vert() { t!("Ниже", "Move down", ja: "下へ移動", pt: "Mover para baixo", zh: "下移") } else { t!("Сдвинуть вправо", "Move right", ja: "右へ移動", pt: "Mover para a direita", zh: "右移") });
        }
    }
    if !launch.is_empty() {
        let s = apps::start_pinned(&launch);
        add(
            A::Start,
            if s { "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM14 16.5h6" } else { "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM17 14v6M14 17h6" },
            if s { t!("Убрать из Пуска", "Remove from Start", ja: "スタートから削除", pt: "Remover do Iniciar", zh: "从开始菜单移除") } else { t!("Добавить в Пуск", "Add to Start", ja: "スタートに追加", pt: "Adicionar ao Iniciar", zh: "添加到开始菜单") },
        );
    }

    // the app's own recent files, if Windows keeps a list for it: by its AppUserModelID (the window's, else the shortcut's),
    // else by the exe path. An app without a list shows no block.
    let aumid = wins.first().map(|&w| win_aumid(w)).filter(|a| !a.is_empty()).or_else(|| (!lnk.is_empty()).then(|| lnk_aumid(&wide(&lnk))).filter(|a| !a.is_empty()));
    let docs = aumid.map(|id| recent::recent(&id, 5)).filter(|d| !d.is_empty()).or_else(|| (!exe.is_empty()).then(|| recent::recent(&exe, 5))).unwrap_or_default();
    let recent_rows: Vec<Recent> = docs.iter().map(|d| Recent { name: d.name.clone(), folder: d.folder }).collect();
    let paths: Vec<String> = docs.into_iter().map(|d| d.path).collect();

    let running = !wins.is_empty();
    let data = Data {
        icon: wicon(icon),
        name,
        meta,
        running,
        wins: rows,
        recent: recent_rows,
        acts,
        close: running.then(|| if wins.len() > 1 { t!("Закрыть все окна", "Close all windows", ja: "すべてのウィンドウを閉じる", pt: "Fechar todas as janelas", zh: "关闭所有窗口") } else { t!("Закрыть окно", "Close window", ja: "ウィンドウを閉じる", pt: "Fechar janela", zh: "关闭窗口") }.to_string()),
        kill: running.then(|| t!("Завершить процесс", "End process", ja: "プロセスを終了", pt: "Finalizar processo", zh: "结束进程").to_string()),
    };
    let first_pid = wins.first().map_or(0, |&w| { let mut pid = 0; GetWindowThreadProcessId(w, &mut pid); pid });
    let pick = move |p: Pick| unsafe {
        match p {
            Pick::Window(w) => {
                let w = w as HWND;
                if IsIconic(w) != 0 {
                    ShowWindow(w, SW_RESTORE);
                }
                force_fg(w);
            }
            Pick::CloseWindow(w) => close_window(w as HWND),
            Pick::Recent(i) => paths.get(i).iter().for_each(|p| open(&wide(p))),
            Pick::Act(i) => match what.get(i).copied() {
                Some(A::Open) => open(&wide(&launch)),
                Some(A::Pin) => {
                    // by path, not by the index seen when the menu opened: PINS may have changed meanwhile
                    match PINS.with_borrow(|p| p.iter().position(|x| !lnk.is_empty() && wstr_of(&x.lnk) == lnk)) {
                        Some(p) => unpin(p),
                        None if lnk.is_empty() => pin_exe(&exe, &proc_args(first_pid)),
                        None => {}
                    }
                    refresh_buttons(); // the buttons hold indices into PINS: a stale one would index past the end on the next paint
                }
                Some(a @ (A::Left | A::Right)) => {
                    // by path again; the new order is saved and load_pins takes it
                    if let Some(p) = PINS.with_borrow(|p| p.iter().position(|x| wstr_of(&x.lnk) == lnk)) {
                        let to = if matches!(a, A::Left) { p.wrapping_sub(1) } else { p + 1 };
                        if PINS.with_borrow_mut(|v| (to < v.len()).then(|| v.swap(p, to))).is_some() {
                            save_pin_order();
                            refresh_buttons();
                        }
                    }
                }
                Some(A::Start) => apps::start_toggle(&launch),
                None => {}
            },
            Pick::CloseAll => wins.iter().for_each(|&w| close_window(w)),
            Pick::Kill => {
                let mut pids: Vec<u32> = wins
                    .iter()
                    .map(|&w| {
                        let mut pid = 0u32;
                        GetWindowThreadProcessId(w, &mut pid);
                        pid
                    })
                    .collect();
                pids.sort_unstable();
                pids.dedup(); // several windows of one app: one kill (and at most one UAC prompt), not one per window
                for pid in pids {
                    let p = OpenProcess(1, 0, pid); // PROCESS_TERMINATE; works on elevated apps when the bar itself runs elevated
                    if p.is_null() || TerminateProcess(p, 1) == 0 {
                        ShellExecuteW(null_mut(), wide("runas").as_ptr(), wide("taskkill.exe").as_ptr(), wide(&format!("/F /T /PID {pid}")).as_ptr(), null(), SW_HIDE);
                    }
                    if !p.is_null() {
                        CloseHandle(p);
                    }
                }
            }
        }
        InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0);
    };
    JL_KEY.set(Some(h));
    ui::panel::open(Box::new(JumpPanel::new(data, anchor.side, pick)), anchor);
}

fn pin_dir() -> PathBuf {
    appdata_file(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")
}

unsafe fn unpin(i: usize) {
    let Some(p) = PINS.with_borrow_mut(|p| (i < p.len()).then(|| p.remove(i))) else { return };
    let n = p.lnk.iter().position(|&c| c == 0).unwrap_or(p.lnk.len());
    // remembered, or the next start would bring it back from the Windows taskbar's list
    let key = pin_key(&p.lnk);
    if !key.0.is_empty() {
        let mut t = std::fs::read_to_string(unpinned_file()).unwrap_or_default();
        t.push_str(&key_line(&key));
        t.push('\n');
        let _ = std::fs::create_dir_all(appdata_file("topbar-taskband"));
        let _ = std::fs::write(unpinned_file(), t);
    }
    let _ = std::fs::remove_file(String::from_utf16_lossy(&p.lnk[..n]));
    DestroyIcon(p.icon);
}

/// Arguments of a shortcut ("" if none).
unsafe fn lnk_args(lnk: &[u16]) -> String {
    (|| -> Option<String> {
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        sl.cast::<IPersistFile>().ok()?.Load(PCWSTR(lnk.as_ptr()), STGM(0)).ok()?;
        let mut b = [0u16; 2048];
        sl.GetArguments(&mut b).ok()?;
        Some(wstr_of(&b))
    })()
    .unwrap_or_default()
}

/// What a running process was started with, minus the exe (e.g. `--profile-directory="Profile 2"`).
fn proc_args(pid: u32) -> String {
    use std::os::windows::process::CommandExt;
    if pid == 0 {
        return String::new();
    }
    // the PEB first (microseconds); PowerShell (about a second) only for a process that cannot be opened for reading
    let cl = winrust_common::proc::cmdline(pid).map(|c| c.trim().to_string()).unwrap_or_else(|| {
        let script = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8;(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').CommandLine");
        let out = Command::new("powershell.exe").args(["-NoProfile", "-Command", &script]).creation_flags(NO_WINDOW).output();
        out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    });
    let rest = match cl.strip_prefix('"') {
        Some(r) => r.split_once('"').map_or("", |x| x.1),
        None => cl.split_once(' ').map_or("", |x| x.1),
    };
    rest.trim().to_string()
}

/// Pins an app: a shortcut to its exe (with `args`) in the folder where Windows keeps taskbar pins.
/// A dropped .lnk is copied as it is, so its arguments (browser profile) and icon stay. Already pinned (same target and
/// arguments) = nothing.
unsafe fn pin_exe(path: &str, args: &str) {
    let is_lnk = path.to_lowercase().ends_with(".lnk");
    // targets are compared with exe_of(), which is lower-case
    let exe = if is_lnk { lnk_target(&wide(path)) } else { path.to_lowercase() };
    let args = if is_lnk { lnk_args(&wide(path)) } else { args.to_string() };
    let exe = exe.as_str();
    if exe.is_empty() || PINS.with_borrow(|p| p.iter().any(|p| p.target == exe && p.args == args)) {
        return;
    }
    let stem = std::path::Path::new(if is_lnk { path } else { exe }).file_stem().unwrap_or_default().to_string_lossy().to_string();
    let name: String = stem.chars().enumerate().map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c }).collect();
    let _ = std::fs::create_dir_all(pin_dir());
    // a free name: two pins of one exe with different arguments (two browser profiles) must not overwrite each other
    let Some(file) = (1..).map(|n| pin_dir().join(if n == 1 { format!("{name}.lnk") } else { format!("{name} ({n}).lnk") })).find(|p| !p.exists()) else { return };
    let lnk = wide(&file.to_string_lossy());
    let ok = if is_lnk && std::fs::copy(path, &file).is_ok() {
        Some(())
    } else {
        (|| -> Option<()> {
            let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            sl.SetPath(PCWSTR(wide(exe).as_ptr())).ok()?;
            if !args.is_empty() {
                sl.SetArguments(PCWSTR(wide(&args).as_ptr())).ok()?;
            }
            sl.cast::<IPersistFile>().ok()?.Save(PCWSTR(lnk.as_ptr()), true).ok()
        })()
    };
    if ok.is_some() {
        // pinned again: no longer on the unpinned list
        let line = key_line(&(exe.to_string(), args.to_lowercase()));
        if let Ok(t) = std::fs::read_to_string(unpinned_file()) {
            let _ = std::fs::write(unpinned_file(), t.lines().filter(|l| *l != line).map(|l| format!("{l}\n")).collect::<String>());
        }
        let icon = lnk_icon(&lnk, exe, bar_icon_px());
        PINS.with_borrow_mut(|p| p.push(Pin { lnk, target: exe.to_string(), args, aumid: String::new(), icon }));
    }
}

/// Files dropped on the bar: on the Start button they are added to Start (the panel opens and the new row pulses),
/// anywhere else they are pinned to the bar.
unsafe fn on_drop(hwnd: HWND, hd: HDROP) {
    let mut pt: POINT = zeroed();
    DragQueryPoint(hd, &mut pt);
    let n = DragQueryFileW(hd, u32::MAX, null_mut(), 0);
    let files: Vec<String> = (0..n)
        .map(|i| {
            let len = DragQueryFileW(hd, i, null_mut(), 0) as usize;
            let mut b = vec![0u16; len + 1];
            DragQueryFileW(hd, i, b.as_mut_ptr(), b.len() as u32);
            String::from_utf16_lossy(&b[..len])
        })
        .collect();
    DragFinish(hd);
    let h = hit(pt.x, pt.y);
    press(if h == NONE { 0 } else { h });
    if h == 0 {
        apps::pin_dropped(&files);
        if !ui::panel::is_open(ui::panel::Id::Start) {
            start_toggle();
        }
    } else {
        files.iter().for_each(|f| pin_exe(f, ""));
        refresh_buttons();
    }
    InvalidateRect(hwnd, null(), 0);
}

/// The junk remover (purge, installed on request): lists what it would delete and asks for YES in its own elevated console.
pub(crate) fn purge_installed() -> bool {
    std::env::var_os("ProgramFiles").is_some_and(|pf| PathBuf::from(pf).join(r"purge\purge.exe").is_file())
}

unsafe fn open_purge() {
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        let exe = PathBuf::from(pf).join(r"purge\purge.exe");
        ShellExecuteW(null_mut(), wide("runas").as_ptr(), wide(&exe.to_string_lossy()).as_ptr(), wide("--ask").as_ptr(), null(), SW_SHOWNORMAL);
    }
}

unsafe fn open_cleaner() {
    let Some(pf) = std::env::var_os("ProgramFiles") else { return };
    let exe = PathBuf::from(pf).join(r"rtaskmgr\rs-du.exe");
    if exe.is_file() {
        ShellExecuteW(null_mut(), wide("runas").as_ptr(), wide(&exe.to_string_lossy()).as_ptr(), wide("C: --clean --ask").as_ptr(), null(), SW_SHOWNORMAL);
    }
}

unsafe extern "system" fn find_launcher(h: HWND, lp: LPARAM) -> i32 {
    let mut cls = [0u16; 32];
    let n = GetClassNameW(h, cls.as_mut_ptr(), 32).max(0) as usize;
    if String::from_utf16_lossy(&cls[..n]) == "rs_launcher" {
        *(lp as *mut HWND) = h;
        return 0;
    }
    1
}

/// The Start button and the Win key: opens the Start menu, or closes it when it is open.
pub(crate) unsafe fn start_toggle() {
    ui::panel::toggle(ui::panel::Id::Start, || {
        let a = anchor(0)?;
        apps::pinned_load(); // the search box (rs-launcher) can add to this list from another process
        thread::spawn(apps::load); // apps installed since the last open show up
        Some((Box::new(ui::start_panel::StartPanel::new()), a))
    });
}

/// The search button.
pub(crate) unsafe fn search_toggle() {
    ui::panel::toggle(ui::panel::Id::Search, || {
        let a = anchor(SEARCH)?;
        let screen_h = GetSystemMetrics(SM_CYSCREEN) as f32 / SCALE.get();
        Some((Box::new(ui::search_panel::SearchPanel::new(screen_h)), a))
    });
}

/// Right click on Start, and Win+X: the power-user menu (Explorer's own is gone with the bar).
unsafe fn start_menu() {
    if let Some(a) = anchor(0) {
        menus::winx_menu(a);
    }
}

/// Puts the open, not minimized windows side by side in the work area.
/// A window dropped with the pointer on the screen edge snaps (Windows 11 style): left / right edge = that half of the monitor's
/// work area, top edge = maximized. Only resizable windows; without Explorer nothing else does this.
unsafe extern "system" fn snap_proc(_: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK, ev: u32, w: HWND, id: i32, _: i32, _: u32, _: u32) {
    if id != 0 || w.is_null() {
        return;
    }
    if ev == 0x000A {
        // the drag starts: its size, so that the end can tell a move (magnet) from a resize (left alone)
        let r = frame(w);
        DRAG_SIZE.set((r.right - r.left, r.bottom - r.top));
        return;
    }
    if IsWindowVisible(w) == 0 || IsZoomed(w) != 0 {
        return;
    }
    if stick() && !edge_snap(w) {
        let r = frame(w);
        if DRAG_SIZE.get() == (r.right - r.left, r.bottom - r.top) {
            magnet_window(w, r);
        }
        return;
    }
    if GetWindowLongW(w, GWL_STYLE) as u32 & WS_THICKFRAME == 0 {
        return;
    }
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(mon, &mut mi) == 0 {
        return;
    }
    let (m, a) = (mi.rcMonitor, mi.rcWork);
    let half = (a.right - a.left) / 2;
    if pt.y <= m.top + 1 {
        ShowWindow(w, SW_MAXIMIZE);
    } else if pt.x <= m.left + 1 {
        SetWindowPos(w, null_mut(), a.left, a.top, half, a.bottom - a.top, SWP_NOZORDER | SWP_NOACTIVATE);
    } else if pt.x >= m.right - 2 {
        SetWindowPos(w, null_mut(), a.left + half, a.top, half, a.bottom - a.top, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

// ---------- window magnet (#133): a dragged window that ends near a screen edge or another window's edge sticks to it ----------

thread_local! {
    static DRAG_SIZE: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
}
static STICK: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0xFF); // 0 off, 1 on, 0xFF not read yet

fn stick_file() -> PathBuf {
    appdata_file("topbar-stick.txt")
}

pub(crate) fn stick() -> bool {
    if STICK.load(Relaxed) == 0xFF {
        STICK.store(std::fs::read_to_string(stick_file()).is_ok_and(|s| s.trim() == "1") as u8, Relaxed);
    }
    STICK.load(Relaxed) == 1
}

pub(crate) fn set_stick(on: bool) {
    STICK.store(on as u8, Relaxed);
    let _ = std::fs::write(stick_file(), if on { "1" } else { "0" });
}

/// Would the edge snap above take this drop (the pointer on a screen edge)?
unsafe fn edge_snap(w: HWND) -> bool {
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mi) != 0
        && GetWindowLongW(w, GWL_STYLE) as u32 & WS_THICKFRAME != 0
        && (pt.y <= mi.rcMonitor.top + 1 || pt.x <= mi.rcMonitor.left + 1 || pt.x >= mi.rcMonitor.right - 2)
}

/// The window's visible frame (without the invisible resize borders Windows 10/11 add around it).
unsafe fn frame(w: HWND) -> RECT {
    let mut r: RECT = zeroed();
    if DwmGetWindowAttribute(w, 9, &mut r as *mut _ as *mut _, size_of::<RECT>() as u32) != 0 {
        GetWindowRect(w, &mut r); // DWMWA_EXTENDED_FRAME_BOUNDS failed
    }
    r
}

/// How far to move `r` so that its nearest edge within `d` px lands on one of the edges (0 when none is that close).
/// `xs`/`ys`: the vertical / horizontal lines it may stick to.
fn magnet(r: RECT, xs: &[i32], ys: &[i32], d: i32) -> (i32, i32) {
    let best = |a: i32, b: i32, es: &[i32]| es.iter().flat_map(|&e| [e - a, e - b]).filter(|v| v.abs() <= d).min_by_key(|v| v.abs()).unwrap_or(0);
    (best(r.left, r.right, xs), best(r.top, r.bottom, ys))
}

unsafe fn magnet_window(w: HWND, r: RECT) {
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST), &mut mi) == 0 {
        return;
    }
    let a = mi.rcWork;
    let (mut xs, mut ys) = (vec![a.left, a.right], vec![a.top, a.bottom]);
    // the other windows' edges: only those beside this one (overlapping along the other axis), so a far window does not pull
    let mut v: Vec<HWND> = Vec::new();
    EnumWindows(Some(collect), &mut v as *mut _ as LPARAM);
    for o in v.into_iter().filter(|&o| o != w && IsIconic(o) == 0 && IsZoomed(o) == 0) {
        let f = frame(o);
        if f.top < r.bottom && f.bottom > r.top {
            xs.extend([f.left, f.right]);
        }
        if f.left < r.right && f.right > r.left {
            ys.extend([f.top, f.bottom]);
        }
    }
    let (dx, dy) = magnet(r, &xs, &ys, sc(14));
    if (dx, dy) != (0, 0) {
        let mut wr: RECT = zeroed();
        GetWindowRect(w, &mut wr);
        SetWindowPos(w, null_mut(), wr.left + dx, wr.top + dy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Win+1..9: opens the n-th app button's pin, or brings its window up (a second press on the only window minimizes it,
/// with several windows each press moves on to the next one).
unsafe fn win_num(n: usize) {
    let Some((wins, pin)) = BTNS.with_borrow(|b| b.get(n + 1).map(|b| (b.wins.clone(), b.pin))) else { return };
    if wins.is_empty() {
        if let Some(l) = pin.and_then(|p| PINS.with_borrow(|p2| p2.get(p).map(|x| x.lnk.clone()))) {
            open(&l);
        }
        return;
    }
    let i = wins.iter().position(|&w| w == GetForegroundWindow());
    let w = i.map_or(wins[0], |i| wins[(i + 1) % wins.len()]);
    if wins.len() == 1 && i.is_some() {
        ShowWindow(w, SW_MINIMIZE);
    } else {
        if IsIconic(w) != 0 {
            ShowWindow(w, SW_RESTORE);
        }
        force_fg(w);
    }
}

unsafe fn tile_windows() {
    let wins: Vec<HWND> = BTNS.with_borrow(|b| b.iter().flat_map(|b| b.wins.iter().copied()).filter(|&w| IsIconic(w) == 0).collect());
    if wins.is_empty() {
        return;
    }
    let mut wa: RECT = zeroed();
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut _ as *mut _, 0);
    for &w in &wins {
        ShowWindow(w, SW_RESTORE); // a maximized window ignores tiling
    }
    TileWindows(null_mut(), MDITILE_VERTICAL, &wa, wins.len() as u32, wins.as_ptr());
}

unsafe fn click(h: i32, left: bool, mid: bool) {
    match h {
        SEARCH if left => search_toggle(),
        // Task view: our own (`ui::taskview`): the windows of the desktop live, the virtual desktops in a row (#83, #120)
        TASKVIEW if left => taskview_toggle(),
        // left: the language picker; right: straight to the next one (Win+Space is explorer's, gone under rs-shell)
        LANG if left => lang_toggle(),
        LANG => next_lang(BAR.load(Relaxed) as HWND, false),
        WIFI_I if left => quick_toggle(h),
        WIFI_I => wifi_toggle(), // right click: the network list
        VOL_I | BAT_I => quick_toggle(h),
        CLOCK if left => cal_toggle(),
        CLOCK => {
            // right click: like Windows 11, a menu with the date and time and notification settings (#126)
            if let Some(a) = anchor(CLOCK) {
                menus::bar_menu("clock", a);
            }
        }
        SHOWDESK if left => winrust_common::desktop::toggle(),
        POW_R | POW_S if left => power_click(h),
        GEAR => settings_toggle(),
        TAPP..=-900 => {
            let slot = (h - TAPP) as usize;
            if slot == 0 {
                if left {
                    tray_toggle();
                }
            } else if mid {
                if let Some(k) = bar_idx().get(slot - 1).and_then(|&i| TAPPS.with_borrow(|t| t.get(i).map(|x| x.key.clone()))) {
                    toggle_shown(&k); // back into the flyout
                }
            } else {
                let ti = bar_idx().get(slot - 1).and_then(|&i| TAPPS.with_borrow(|t| t.get(i).map(|x| (x.hwnd, x.uid, x.cb, x.ver))));
                ti.into_iter().for_each(|ti| tray_click(ti, left));
            }
        }
        0 if left => start_toggle(),
        0 => start_menu(),
        1.. => {
            let (wins, lnk) = BTNS.with_borrow(|b| b.get(h as usize).map(|b| (b.wins.clone(), b.pin.and_then(|p| PINS.with_borrow(|p2| p2.get(p).map(|x| x.lnk.clone()))))).unwrap_or_default());
            match (wins.first(), left) {
                // not running: launch the pinned app (a Store app may have updated since the pins were loaded)
                (None, true) => lnk.iter().for_each(|l| {
                    PINS.with_borrow_mut(|p| p.iter_mut().filter(|x| x.lnk == *l).for_each(|x| x.target = fix_msix_pin(l, &x.target).unwrap_or(std::mem::take(&mut x.target))));
                    open(l)
                }),
                (Some(&w), false) => close_window(w), // right / middle click
                // several windows of one app: pick which one (the same list the right-click menu opens with)
                (Some(_), true) if wins.len() > 1 => jump_list(h),
                (Some(&w), true) => {
                    // like the Windows taskbar: the active app's button minimizes it, a minimized one restores, any other raises
                    let fg = GetForegroundWindow();
                    if wins.contains(&fg) && IsIconic(fg) == 0 {
                        ShowWindow(fg, SW_MINIMIZE);
                    } else {
                        if IsIconic(w) != 0 {
                            ShowWindow(w, SW_RESTORE);
                        }
                        SetForegroundWindow(w);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

#[repr(C)]
struct ShellHookInfo {
    hwnd: HWND,
    rc: RECT,
}

/// HSHELL_GETMINRECT: a window is minimizing / restoring and asks where its taskbar button is: the shrink animation
/// flies there instead of to the bottom left corner of the screen.
unsafe fn min_rect(info: *mut ShellHookInfo, _bar: HWND) {
    let w = (*info).hwnd;
    let exe = exe_of(w);
    let i = BTNS.with_borrow(|b| b.iter().position(|b| b.wins.contains(&w)).or_else(|| b.iter().position(|b| !exe.is_empty() && b.wins.first().is_some_and(|&x| exe_of(x) == exe))));
    // no button (yet): fly to Start
    if let Some((left, top, right, bottom)) = UI.with_borrow(|u| u.as_ref().and_then(|b| b.screen_rect(i.map_or(0, |i| i as i32)))) {
        (*info).rc = RECT { left, top, right, bottom };
    }
}

/// Registers the bar as an appbar with explorer (again after explorer restarts: its list starts empty).
unsafe fn appbar_new(hwnd: HWND) {
    let mut ab: APPBARDATA = zeroed();
    ab.cbSize = size_of::<APPBARDATA>() as u32;
    ab.hWnd = hwnd;
    ab.uCallbackMessage = CALLBACK;
    SHAppBarMessage(ABM_NEW, &mut ab);
    EXP_TRAY.set(tray()[0]);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg != 0 && msg == SH_MSG.get() {
        if wp == 5 && lp != 0 {
            min_rect(lp as *mut ShellHookInfo, hwnd); // HSHELL_GETMINRECT
            return 1;
        }
        let w = lp as HWND;
        match wp {
            0x8006 => FLASH.with_borrow_mut(|f| if !f.contains(&w) { f.push(w) }), // HSHELL_FLASH
            4 | 0x8004 | 2 => FLASH.with_borrow_mut(|f| f.retain(|&x| x != w)), // activated (HSHELL_WINDOWACTIVATED / RUDEAPPACTIVATED), destroyed
            _ => return 0,
        }
        InvalidateRect(hwnd, null(), 0);
        return 0;
    }
    if msg != 0 && msg == TB_MSG.get() {
        // also our own announce(): act only when explorer's taskbar is a new window (explorer restarted / came back)
        let t = tray()[0];
        if !t.is_null() && t != EXP_TRAY.get() {
            appbar_new(hwnd);
            hide_taskbar();
            place(hwnd);
        }
        return 0;
    }
    match msg {
        WM_PAINT => {
            ValidateRect(hwnd, null());
            redraw(hwnd);
        }
        WM_ERASEBKGND => return 1,
        WM_TIMER if wp == 2 => {
            KillTimer(hwnd, 2);
            let h = HOT.get();
            if h != NONE && !popup_open() && !preview_for(h) {
                // an app button with windows shows their live previews (#92), anything else its tip
                if !preview_open(h, false, 0) {
                    hide_preview();
                    key_tip(h, tip_text(h), 0, None);
                }
            }
        }
        WM_TIMER if wp == 9 => preview_tick(hwnd),
        WM_TIMER if wp == 10 => reopen(hwnd),
        WM_TIMER if wp == 11 => vdesk::step(hwnd),
        WM_TIMER if wp == 6 => hide_tip(hwnd), // the volume readout after a wheel turn
        WM_TIMER if wp == 3 => fs_update(hwnd),
        WM_TIMER if wp == 7 => {
            KillTimer(hwnd, 7);
            let (tid, before, next) = LANG_TRY.get();
            if GetKeyboardLayout(tid) as usize == before && !EXPLORER_UP.load(Relaxed) {
                // no explorer, so no system switcher to fall back on: ask the app's top-level window as well (some apps
                // only listen there, not on the focused child)
                PostMessageW(app_window(), WM_INPUTLANGCHANGEREQUEST, 0, next as isize);
            } else if GetKeyboardLayout(tid) as usize == before {
                let w = app_window();
                if GetForegroundWindow() != w {
                    force_fg(w); // Win+Space switches the window in front: put the app back there first
                }
                keys(&[VK_LWIN, VK_SPACE]); // the app ignored the request: the system switcher always works (with explorer)
            }
            InvalidateRect(hwnd, null(), 0); // the label shows the new language right away
        }
        pacing::FRAME => { if pacing::received(hwnd as isize) { redraw(hwnd); } }
        WIN_START => {
            ui::panel::forget_deact(); // a Win press right after Start closed (by losing the focus) must open it again
            start_toggle();
        }
        ALT_TAB => alt_tab(wp == 1),
        WIN_NUM => win_num(wp),
        WIN_X => start_menu(),
        WIN_V => {
            let (x, _, w, _) = bar_win();
            hist::show(hwnd, x + w / 2);
        }
        0x031D => hist::on_update(hwnd), // WM_CLIPBOARDUPDATE
        ALT_SPACE => {
            ui::panel::forget_deact(); // pressed right after the search closed by losing the focus: it must open again
            search_toggle();
        }
        NEXT_LANG => next_lang(hwnd, wp == 1),
        ALT_UP if wp == 2 => alt_close(),
        ALT_UP => alt_done(wp == 1),
        SNIP if wp == 1 => screenshot_notice(hwnd),
        SNIP if wp == 2 => clip_notice(hwnd),
        SNIP => clip::start(),
        VOLKEY => {
            if wp == 2 { toggle_mute() } else { add_volume(if wp == 0 { 4 } else { -4 }) }
            KillTimer(hwnd, 2);
            key_tip(VOL_I, vol_tip(), 0, None);
            SetTimer(hwnd, 6, 1200, None);
            InvalidateRect(hwnd, null(), 0);
        }
        WM_DROPFILES => on_drop(hwnd, wp as HDROP),
        WM_TIMER if wp == 8 => {
            // window events since the last look (win_event): one refresh for the whole burst
            KillTimer(hwnd, 8);
            WIN_DIRTY.set(false);
            hide_taskbar_if_shown();
            refresh_windows();
            repaint_if_changed(hwnd, false);
        }
        WM_TIMER => {
            hide_taskbar_if_shown();
            refresh_status();
            if TICK.get() % 5 == 0 {
                refresh_windows(); // safety net for a window event the hook did not report
            }
            fs_update(hwnd); // style switches (auto-hide, dock) take effect without a window event
            repaint_if_changed(hwnd, true);
        }
        WM_MOUSEMOVE => {
            let h = hit((lp & 0xffff) as i16 as i32, (lp >> 16) as i16 as i32);
            let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            TrackMouseEvent(&mut tme);
            if HOT.replace(h) != h {
                hide_tip(hwnd);
                if h != NONE {
                    let has_wins = h >= 1 && BTNS.with_borrow(|b| b.get(h as usize).is_some_and(|b| !b.wins.is_empty()));
                    // the previews come after the mockup's .38 s, at once when others are open (`hvEnter`)
                    SetTimer(hwnd, 2, if has_wins { 380 } else { 450 }, None);
                    if ui::panel::is_open(ui::panel::Id::Preview) {
                        if has_wins {
                            if !preview_for(h) && !popup_open() && !preview_open(h, false, 0) {
                                hide_preview();
                            }
                        } else if h >= 1 {
                            hide_preview();
                        }
                    }
                    // the hint comes by itself after the mockup's .25 s; an app with windows shows previews instead
                    if !popup_open() && !has_wins {
                        key_tip(h, tip_text(h), 250, None);
                    }
                }
                InvalidateRect(hwnd, null(), 0);
            }
        }
        WM_MOUSELEAVE => {
            hide_tip(hwnd);
            HOT.set(-1);
            InvalidateRect(hwnd, null(), 0);
        }
        WM_MOUSEWHEEL => {
            add_volume(if (wp >> 16) as i16 > 0 { 2 } else { -2 }); // wheel anywhere on the bar = volume
            press(VOL_I); // the speaker flashes so it is clear what the wheel changed
            KillTimer(hwnd, 2);
            if !popup_open() {
                key_tip(VOL_I, vol_tip(), 0, None); // and the level shows above it for a moment
                SetTimer(hwnd, 6, 1200, None);
            }
            InvalidateRect(hwnd, null(), 0);
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (px, py) = ((lp & 0xffff) as i16 as i32, (lp >> 16) as i16 as i32);
            let h = hit(px, py);
            let (wx, wy, ..) = bar_win();
            let x = if vert() { py + wy } else { px + wx }; // screen position along the bar
            hide_tip(hwnd);
            hide_preview();
            if h != NONE {
                press(h);
            }
            if h == NONE && msg == WM_RBUTTONDOWN {
                // right click on an empty spot
                if let Some(a) = menus::anchor_at(x) {
                    menus::bar_menu("ctx", a);
                }
            } else if h >= 1 && msg == WM_RBUTTONDOWN {
                jump_list(h);
            } else {
                click(h, msg == WM_LBUTTONDOWN, msg == WM_MBUTTONDOWN);
            }
            InvalidateRect(hwnd, null(), 0);
        }
        CALLBACK => {
            if wp == ABN_FULLSCREENAPP as usize {
                fs_update(hwnd);
            } else if wp == ABN_POSCHANGED as usize {
                place(hwnd);
            }
        }
        WM_DISPLAYCHANGE => place(hwnd),
        WM_DESTROY => {
            pacing::set(hwnd as isize, false);
            let mut ab: APPBARDATA = zeroed();
            ab.cbSize = size_of::<APPBARDATA>() as u32;
            ab.hWnd = hwnd;
            SHAppBarMessage(ABM_REMOVE, &mut ab);
            set_work_area(RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) }); // ours: give the strip back
            show_taskbar(OLD_STATE.get());
            forget_taskbar_state();
            DestroyWindow(HOST.replace(null_mut()));
            DestroyWindow(SHADOW.replace(null_mut()));
            announce(); // icons go back to explorer's tray
            PostQuitMessage(0);
        }
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    0
}

fn main() {
    unsafe {
        if std::env::args().any(|a| a == "--restore") {
            show_taskbar(saved_taskbar_state().unwrap_or(ABS_ALWAYSONTOP as usize));
            forget_taskbar_state();
            set_work_area(RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) }); // a killed bar's reserved strip
            announce(); // a killed bar took its tray icons with it; apps re-register with explorer
            return;
        }
        // `topbar --render <file.png> [classic|floating|dock] [bottom|left|right] [0|1|2] ...`: one frame of the bar into a PNG, no windows
        if let Some(i) = std::env::args().position(|a| a == "--render") {
            render::render(&std::env::args().skip(i + 1).collect::<Vec<_>>());
            return;
        }
        // `topbar --render-panel <file.png> tray|lang|tip ...`: one popup panel (or the hint over a button) next to the bar, into a PNG
        if let Some(i) = std::env::args().position(|a| a == "--render-panel") {
            render::render_panel(&std::env::args().skip(i + 1).collect::<Vec<_>>());
            return;
        }
        // `topbar --rec-selftest <out.mp4> [mic]`: 3 s of the primary monitor's top-left 640x360 through the real screen-recording path
        // (and the microphone with `mic`), prints the frame counts (see rec.rs). Opt-in: it records the screen.
        if let Some(i) = std::env::args().position(|a| a == "--rec-selftest") {
            SetProcessDPIAware();
            std::process::exit(if rec::selftest(&std::env::args().skip(i + 1).collect::<Vec<_>>()) { 0 } else { 1 });
        }
        if std::env::args().any(|a| a == "--bench") {
            render::bench();
            return;
        }
        // `topbar --render-thumbs <file.png>`: whether DWM thumbnails show in the windows the panels are made of (see render.rs)
        if let Some(i) = std::env::args().position(|a| a == "--render-thumbs") {
            render::render_thumbs(&std::env::args().skip(i + 1).collect::<Vec<_>>());
            return;
        }
        if std::env::args().any(|a| a == "--shot") {
            SetProcessDPIAware();
            let (mut tok, mut inp): (usize, GdiplusStartupInput) = (0, zeroed());
            inp.GdiplusVersion = 1;
            GdiplusStartup(&mut tok, &inp, null_mut());
            screenshot();
            return;
        }
        // Atomic single-instance guard (FindWindow alone raced when two launchers started us at once); the handle lives until exit.
        let m = windows_sys::Win32::System::Threading::CreateMutexW(null(), 0, wide("Local\\topbar-single").as_ptr());
        if m.is_null() || GetLastError() == ERROR_ALREADY_EXISTS || !FindWindowW(wide("topbar").as_ptr(), null()).is_null() {
            return; // already running
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // Without explorer nobody hides minimized windows: they sit as small caption stubs at the bottom left of the screen.
        // ARW_HIDE moves them off screen (this session only: fWinIni 0 does not touch the saved setting).
        let mut mm: MINIMIZEDMETRICS = zeroed();
        mm.cbSize = size_of::<MINIMIZEDMETRICS>() as u32;
        if SystemParametersInfoW(SPI_GETMINIMIZEDMETRICS, mm.cbSize, &mut mm as *mut _ as *mut _, 0) != 0 {
            mm.iArrange |= ARW_HIDE as i32;
            SystemParametersInfoW(SPI_SETMINIMIZEDMETRICS, mm.cbSize, &mut mm as *mut _ as *mut _, 0);
        }
        let (mut gtok, mut gin): (usize, GdiplusStartupInput) = (0, zeroed());
        gin.GdiplusVersion = 1;
        GdiplusStartup(&mut gtok, &gin, null_mut()); // anti-aliased shapes; fill() falls back to GDI if this failed
        TB_MSG.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
        SH_MSG.set(RegisterWindowMessageW(wide("SHELLHOOK").as_ptr()));
        SetProcessDPIAware();
        let dc = GetDC(null_mut());
        SCALE.set(GetDeviceCaps(dc, LOGPIXELSX as i32) as f32 / 96.0);
        ReleaseDC(null_mut(), dc);
        SHOWN.set(std::fs::read_to_string(appdata_file("topbar-tray.txt")).ok().map(|s| s.lines().map(str::to_string).collect()));
        load_pins();
        fly::init(GetModuleHandleW(null()));
        apps::pinned_load();
        apps::set_icon_px(bar_icon_px());
        thread::spawn(apps::load);
        thread::spawn(winrust_common::mem::auto_loop); // standby purge when memory runs short, switched on from the task manager's Speed page
        let hi = GetModuleHandleW(null());
        let class = wide("topbar");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hi, hCursor: LoadCursorW(null_mut(), IDC_ARROW), lpszClassName: class.as_ptr(), ..zeroed() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED, class.as_ptr(), class.as_ptr(), WS_POPUP, 0, 0, GetSystemMetrics(SM_CXSCREEN), bar_h(), null_mut(), null_mut(), hi, null());
        // The floating styles' shadow: a click-through layered window right under the bar (see redraw())
        let shc = wide("topbar_shadow");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: hi, lpszClassName: shc.as_ptr(), ..zeroed() });
        SHADOW.set(CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT, shc.as_ptr(), null(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), hi, null()));
        UI.set(barui::Bar::new(SCALE.get()).ok());
        // Our own Shell_TrayWnd (hidden, topmost, so apps find it before explorer's): apps register their tray icons here.
        let tc = wide("Shell_TrayWnd");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(tray_proc), hInstance: hi, lpszClassName: tc.as_ptr(), ..zeroed() });
        HOST.set(CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, tc.as_ptr(), null(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), hi, null()));
        // The bar may run elevated while apps and explorer do not; UIPI would drop their messages to us (icon data, appbar callbacks).
        ChangeWindowMessageFilterEx(HOST.get(), WM_COPYDATA, MSGFLT_ALLOW, null_mut());
        // ITaskbarList::HrInit reads the task band from this property of the first Shell_TrayWnd, which is ours: without it
        // HrInit failed (E_NOTIMPL) for every app while the bar ran. DeleteTab / AddTab then come here (tray_proc).
        SetPropW(HOST.get(), wide("TaskbandHWND").as_ptr(), HOST.get());
        ChangeWindowMessageFilterEx(HOST.get(), SH_MSG.get(), MSGFLT_ALLOW, null_mut());
        ChangeWindowMessageFilterEx(hwnd, CALLBACK, MSGFLT_ALLOW, null_mut());
        ChangeWindowMessageFilterEx(hwnd, TB_MSG.get(), MSGFLT_ALLOW, null_mut()); // explorer restarted (it runs unelevated)
        // files dragged from a normal-rights app onto the (elevated) bar: Start button = add to Start, elsewhere = pin
        DragAcceptFiles(hwnd, 1);
        for m in [WM_DROPFILES, 0x49 /* WM_COPYGLOBALDATA */, WM_COPYDATA] {
            ChangeWindowMessageFilterEx(hwnd, m, MSGFLT_ALLOW, null_mut());
        }
        ui::panel::init(hi, SCALE.get(), hwnd, panel_changed);
        ui::tip::init(hi, SCALE.get(), (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)));
        ui::toast::init(hi, SCALE.get(), (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)));
        {
            // the notification pop-ups, and the icons their cards draw (the shell's, at the size they are drawn)
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            ui::notice_pop::init(hi, SCALE.get(), ui::panel::Anchor { side: i18n::style::side(), button: (0, 0, 0, 0), bar: (0, sh - 54, sw, sh), screen: (sw, sh) });
            ui::notice::set_icon_loader(|p| item_icon(p, sc(32)) as isize);
            notify::with(|s| s.set_dnd(notify::load_dnd()));
        }
        OLD_STATE.set(remember_taskbar_state());
        appbar_new(hwnd);
        // Without explorer nobody is the "taskman" window, so Windows treats minimized windows as loose icons and leaves
        // them as caption stubs on the desktop. Becoming that window (what explorer's taskbar does) makes them hide properly
        // and gets us HSHELL_GETMINRECT. Only when explorer's taskbar is absent: with it running its own registration stays.
        if find_other("Shell_TrayWnd").is_null() {
            if let Some(f) = GetProcAddress(GetModuleHandleW(wide("user32.dll").as_ptr()), b"SetTaskmanWindow\0".as_ptr()) {
                std::mem::transmute::<_, unsafe extern "system" fn(HWND) -> i32>(f)(hwnd);
            }
        }
        // window events for the shell (minimize target, HSHELL_FLASH): also next to explorer, whose taskbar we hide
        RegisterShellHookWindow(hwnd);
        hide_taskbar();
        place(hwnd);
        refresh();
        announce(); // apps already running re-add their icons to us
        SetTimer(hwnd, 1, 1000, None); // status poll; windows come from win_event
        BAR.store(hwnd as isize, Relaxed);
        hist::init(hwnd);
        SetTimer(hwnd, 3, 250, None); // fullscreen watch
        windows_sys::Win32::UI::Accessibility::SetWinEventHook(0x000A, 0x000B, null_mut(), Some(snap_proc), 0, 0, 0); // EVENT_SYSTEM_MOVESIZESTART..END
        // foreground, minimize start/end, create..hide, name, cloaked/uncloaked (2 = WINEVENT_SKIPOWNPROCESS). Not
        // LOCATIONCHANGE: it fires for every caret and cursor move; a fullscreen switch comes as ABN_FULLSCREENAPP and the 1 s tick
        for (a, b) in [(0x0003, 0x0003), (0x0016, 0x0017), (0x8000, 0x8003), (0x800C, 0x800C), (0x8017, 0x8018)] {
            windows_sys::Win32::UI::Accessibility::SetWinEventHook(a, b, null_mut(), Some(win_event), 0, 0, 2);
        }
        thread::spawn(|| {
            let mut hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), GetModuleHandleW(null()), 0);
            // Windows silently drops a low-level hook that once answers too slowly (busy boot), leaving Win+R, Start and
            // Alt+Tab dead with no explorer to fall back on: set it again now and then. Callbacks run only inside
            // GetMessageW on this thread, so swapping here never delivers a key twice.
            SetTimer(null_mut(), 0, 30_000, None);
            let mut m: MSG = zeroed();
            while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
                if m.message == WM_TIMER {
                    let new = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), GetModuleHandleW(null()), 0);
                    if !new.is_null() {
                        if !hook.is_null() {
                            UnhookWindowsHookEx(hook);
                        }
                        hook = new;
                    }
                }
            }
        });
        let mut m: MSG = zeroed();
        while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn magnet_picks_the_nearest_edge_within_reach() {
        let r = windows_sys::Win32::Foundation::RECT { left: 105, top: 50, right: 505, bottom: 350 };
        assert_eq!(super::magnet(r, &[100, 900], &[0, 1000], 14), (-5, 0)); // left edge to 100
        assert_eq!(super::magnet(r, &[512, 100], &[], 14), (-5, 0)); // 105->100 (5) beats 505->512 (7)
        assert_eq!(super::magnet(r, &[515], &[340], 14), (10, -10)); // right edge to 515, bottom to 340
        assert_eq!(super::magnet(r, &[130, 0], &[80], 14), (0, 0)); // nothing close enough
    }

    #[test]
    fn a_far_off_appbar_edge_goes_back_to_the_screen_edge() {
        assert_eq!(super::sane_edge(48, 1080), 48); // another bar below ours: stack above it
        assert_eq!(super::sane_edge(540, 1080), 0); // half the screen (#106)
        assert_eq!(super::sane_edge(-20, 1080), 0);
    }

    #[test]
    fn borderless_games_count_as_fullscreen() {
        use super::{RECT, covers_monitor};
        let m = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert!(covers_monitor(m, m, false, false)); // plain borderless / exclusive fullscreen
        assert!(covers_monitor(m, m, true, false)); // borderless window that kept its caption style
        assert!(!covers_monitor(RECT { left: -8, top: -8, right: 1928, bottom: 1088 }, m, true, true)); // framed maximized window (#88)
        assert!(!covers_monitor(m, m, true, true)); // a maximized window is never a game
        assert!(!covers_monitor(RECT { left: 0, top: 0, right: 1920, bottom: 1032 }, m, false, false)); // leaves the bar's strip
    }

    #[test]
    fn network_icon_state() {
        use super::{INET, Link, WIFI, net};
        let at = |q: i32, inet: bool| {
            WIFI.set(q);
            INET.set(inet);
            net()
        };
        assert!(at(80, true) == Link::Wifi(80));
        assert!(at(80, false) == Link::NoInternet(80)); // wifi without a way out: the warning icon
        assert!(at(-1, true) == Link::Cable);
        assert!(at(-1, false) == Link::Offline); // nothing at all: the crossed-out wifi icon
    }

    #[test]
    fn file_manager_windows_belong_to_the_file_explorer_pin() {
        use super::as_pinned_exe;
        let (rx, ex) = (r"c:\program files\rust-explorer\rust-explorer.exe", r"c:\windows\explorer.exe");
        assert_eq!(as_pinned_exe(rx, ex, true), ex); // the pin the user clicks gets the windows
        assert_eq!(as_pinned_exe(rx, ex, false), rx); // no such pin: the file manager's own pin keeps them
        assert_eq!(as_pinned_exe(r"c:\tools\other.exe", ex, true), r"c:\tools\other.exe");
    }

    #[test]
    fn mix_blends() {
        use winrust_common::style::rgb;
        assert_eq!(super::mix(rgb(0, 0, 0), rgb(200, 100, 50), 0.0), rgb(0, 0, 0));
        assert_eq!(super::mix(rgb(0, 0, 0), rgb(200, 100, 50), 1.0), rgb(200, 100, 50));
        assert_eq!(super::mix(rgb(0, 0, 0), rgb(200, 100, 50), 0.5), rgb(100, 50, 25));
    }

    #[test]
    fn msix_family_from_package_path() {
        let f = super::msix_family(r"C:\Program Files\WindowsApps\Claude_5.11492.5.0_x64__pzs8sxrjxfjjc\app\claude.exe");
        assert_eq!(f, Some(("claude_pzs8sxrjxfjjc".into(), r"app\claude.exe".into())));
        assert_eq!(super::msix_family(r"C:\Program Files\Steam\steam.exe"), None);
    }

    #[test]
    fn a_bitmap_without_alpha_is_told_from_one_with_it() {
        let make = |px: [u32; 4]| unsafe { super::CreateBitmap(2, 2, 1, 32, px.as_ptr().cast()) };
        // SAFETY: bitmaps made and freed here.
        unsafe {
            let (clear, lit) = (make([0x0012_3456; 4]), make([0x0012_3456, 0, 0, 0x8012_3456]));
            assert!(!super::bitmap_has_alpha(clear), "an old icon: all alpha bytes are 0");
            assert!(super::bitmap_has_alpha(lit));
            super::DeleteObject(clear);
            super::DeleteObject(lit);
        }
    }

    #[test]
    fn stale_claude_pin_resolves_to_installed_version() {
        // real system check: skipped where the Claude package is not installed (CI)
        let old = r"c:\program files\windowsapps\claude_5.11492.5.0_x64__pzs8sxrjxfjjc\app\claude.exe";
        let Some((fam, _)) = super::msix_family(old) else { panic!() };
        let mut n = 0u32;
        let mut len = 0u32;
        unsafe { windows_sys::Win32::Storage::Packaging::Appx::GetPackagesByPackageFamily(super::wide(&fam).as_ptr(), &mut n, std::ptr::null_mut(), &mut len, std::ptr::null_mut()) };
        if n == 0 {
            return;
        }
        let tmp = std::env::temp_dir().join("topbar-msix-test.lnk");
        let _ = std::fs::remove_file(&tmp);
        unsafe {
            use windows::Win32::System::Com::*;
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let sl: windows::Win32::UI::Shell::IShellLinkW = CoCreateInstance(&windows::Win32::UI::Shell::ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            sl.SetPath(windows::core::PCWSTR(super::wide(old).as_ptr())).unwrap();
            windows::core::Interface::cast::<IPersistFile>(&sl).unwrap().Save(windows::core::PCWSTR(super::wide(&tmp.to_string_lossy()).as_ptr()), true).unwrap();
            let lnk = super::wide(&tmp.to_string_lossy());
            let new = super::fix_msix_pin(&lnk, old).expect("re-pointed");
            assert!(std::path::Path::new(&new).exists() && new.ends_with(r"\app\claude.exe"), "{new}");
            assert_eq!(super::lnk_target(&lnk), new);
        }
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod env_tests {
    #[test]
    fn expands_known_variables_only() {
        let windir = std::env::var("windir").unwrap();
        assert_eq!(super::expand_env(r"%windir%\explorer.exe"), format!(r"{windir}\explorer.exe"));
        assert_eq!(super::expand_env(r"%no_such_var_x%\a.exe"), r"%no_such_var_x%\a.exe");
        assert_eq!(super::expand_env("100% sure"), "100% sure");
    }
}
