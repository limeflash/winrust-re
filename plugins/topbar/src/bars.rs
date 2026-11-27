//! topbar: a bar on every monitor. One main bar (the primary monitor: everything, as it always was) and, with "Show the bar on
//! all displays" on, one more per other monitor, each with its own window, renderer, scale, strip of the work area, auto-hide and
//! fullscreen state, hover and pacing. The pure rules (which monitors get a bar, which app buttons each bar shows, the strip
//! and work-area rectangles) are plain functions at the top; the windows and the registry of the bars follow (shares the items
//! of main.rs through `super::*`).
use super::*;
use std::rc::Rc;
use winrust_common::style::{AppsOn, OtherBars, Side};

/// A rectangle as (left, top, right, bottom), device pixels on the virtual screen.
pub(crate) type R4 = (i32, i32, i32, i32);

/// One monitor: its handle (a number that is the same while the monitor is plugged in), where it is, and its scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mon {
    pub id: isize,
    pub rect: R4,
    pub primary: bool,
    pub dpi: u32,
}

impl Mon {
    pub fn w(&self) -> i32 {
        self.rect.2 - self.rect.0
    }
    pub fn h(&self) -> i32 {
        self.rect.3 - self.rect.1
    }
    /// 1.0 at 96 dpi.
    pub fn scale(&self) -> f32 {
        self.dpi.max(1) as f32 / 96.0
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the rules

/// The monitors that get a bar, the primary one first (it always has one): the others only with "all displays" on.
pub(crate) fn bar_monitors(mons: &[Mon], all: bool) -> Vec<Mon> {
    let mut v: Vec<Mon> = mons.iter().copied().filter(|m| m.primary).take(1).collect();
    if v.is_empty() {
        v.extend(mons.first().copied()); // no primary flag (should not happen): the first monitor is the main one
    }
    if all {
        let main = v.first().map(|m| m.id);
        v.extend(mons.iter().copied().filter(|m| Some(m.id) != main));
    }
    v
}

/// Whether a bar holds everything (the gear, the tray, the language, quick settings, the power buttons): the main bar always, the
/// others when the setting says they are "the same".
pub(crate) fn full_content(main: bool, other: OtherBars) -> bool {
    main || other == OtherBars::Same
}

/// The app buttons of a bar on monitor `mon`. `btns` are all the buttons in order: whether the app is pinned, and its windows
/// with the monitor each is on. With `AppsOn::All` every bar shows every button with all its windows; with `AppsOn::Here` a bar
/// keeps of each button only the windows on its monitor, and drops the buttons left with none unless they are pinned (a pin is
/// there on every bar, to launch the app). The answer is the index of each button kept and the windows of it shown.
pub(crate) fn bar_buttons<W: Copy>(btns: &[(bool, Vec<(W, isize)>)], mon: isize, apps: AppsOn) -> Vec<(usize, Vec<W>)> {
    btns.iter()
        .enumerate()
        .filter_map(|(i, (pinned, wins))| {
            let shown: Vec<W> = wins.iter().filter(|(_, m)| apps == AppsOn::All || *m == mon).map(|(w, _)| *w).collect();
            (apps == AppsOn::All || *pinned || !shown.is_empty()).then_some((i, shown))
        })
        .collect()
}

/// The strip a bar asks for on its edge of monitor `mon`, `strip` thick.
pub(crate) fn strip_rect(mon: R4, side: Side, strip: i32) -> R4 {
    match side {
        Side::Left => (mon.0, mon.1, mon.0 + strip, mon.3),
        Side::Right => (mon.2 - strip, mon.1, mon.2, mon.3),
        Side::Bottom => (mon.0, mon.3 - strip, mon.2, mon.3),
    }
}

/// How far from its edge of the monitor the strip may sit after `ABM_QUERYPOS` (room left for other appbars). Explorer sometimes
/// answers with a far-off edge (a stale or huge appbar entry), and the work area then shrank to half the screen (#106): past a
/// quarter of the monitor, or off it, the strip goes back to the edge.
pub(crate) fn sane_edge(offset: i32, size: i32) -> i32 {
    if (0..=size / 4).contains(&offset) { offset } else { 0 }
}

/// The strip `queried` (what `ABM_QUERYPOS` left of `strip_rect`) settled on its edge of monitor `mon`.
pub(crate) fn settle_strip(mon: R4, side: Side, queried: R4, strip: i32) -> R4 {
    let (w, h) = (mon.2 - mon.0, mon.3 - mon.1);
    let mut r = queried;
    match side {
        Side::Left => {
            r.0 = mon.0 + sane_edge(r.0 - mon.0, w);
            r.2 = r.0 + strip;
        }
        Side::Right => {
            r.2 = mon.2 - sane_edge(mon.2 - r.2, w);
            r.0 = r.2 - strip;
        }
        Side::Bottom => {
            r.3 = mon.3 - sane_edge(mon.3 - r.3, h);
            r.1 = r.3 - strip;
        }
    }
    r
}

/// The work area of monitor `mon` once the bar holds `strip` on it: what is left of the monitor.
pub(crate) fn work_rect(mon: R4, side: Side, strip: R4) -> R4 {
    match side {
        Side::Left => (strip.2, mon.1, mon.2, mon.3),
        Side::Right => (mon.0, mon.1, strip.0, mon.3),
        Side::Bottom => (mon.0, mon.1, mon.2, strip.1),
    }
}

/// The pointer is in the hot zone of a bar's edge of monitor `mon`: `reach` deep from the edge (the bar's own depth while it is out,
/// a thin strip while it is away), and on that monitor, not on the next one.
pub(crate) fn pointer_at_edge(mon: R4, side: Side, pt: (i32, i32), reach: i32) -> bool {
    let (x, y) = pt;
    if !(mon.0..mon.2).contains(&x) || !(mon.1..mon.3).contains(&y) {
        return false;
    }
    match side {
        Side::Left => x < mon.0 + reach,
        Side::Right => x >= mon.2 - reach,
        Side::Bottom => y >= mon.3 - reach,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the bars

/// An app button of one bar: its index in `BTNS` and the windows of it this bar shows.
#[derive(Clone, Debug)]
pub(crate) struct BarBtn {
    pub idx: usize,
    pub wins: Vec<HWND>,
}

/// One bar: its windows and everything that is its own (the monitor, the scale, the renderer, the pointer, the hide state).
pub(crate) struct BarCx {
    pub hwnd: HWND,
    pub shadow: HWND,
    pub main: bool,
    pub mon: Cell<Mon>,
    pub scale: Cell<f32>,
    pub ui: RefCell<Option<barui::Bar>>,
    pub hot: Cell<i32>,
    pub press: Cell<(i32, u64)>,
    /// A fullscreen window covers this bar's monitor.
    pub fs: Cell<bool>,
    /// The desktop is in front on this monitor and the pointer away from the bar for 2 s.
    pub desk_hide: Cell<bool>,
    /// The last moment the bar had a reason to stay on the desktop.
    pub desk_t: Cell<u64>,
    /// The app buttons shown here (a bar's key `k >= 1` is `btns[k - 1]`).
    pub btns: RefCell<Vec<BarBtn>>,
    pub appbar: Cell<bool>,
    /// The strip this bar's appbar holds on its monitor now.
    pub strip: Cell<R4>,
}

impl BarCx {
    /// Design pixels to device pixels on this bar's monitor.
    pub fn dp(&self, v: f32) -> i32 {
        (v * self.scale.get()).round() as i32
    }
    /// Where the bar rests, its thickness and the gap to the edge of the monitor (device pixels).
    pub fn outer(&self) -> i32 {
        self.dp(shape().outer())
    }
    pub fn rect(&self) -> R4 {
        self.mon.get().rect
    }
    /// The bar window's rectangle on the screen (left, top, width, height), the strip above a bottom bar included.
    pub fn win(&self) -> (i32, i32, i32, i32) {
        self.ui.borrow().as_ref().map_or((0, 0, 0, 0), |b| b.placement().win)
    }
    /// False once the bar has slid away completely.
    pub fn visible(&self) -> bool {
        self.ui.borrow().as_ref().is_none_or(|b| !b.placement().gone)
    }
    /// The monitor's whole bar, or the lighter one (see [`full_content`]).
    pub fn full(&self) -> bool {
        full_content(self.main, i18n::style::other_bars())
    }
}

thread_local! {
    static BARS: RefCell<Vec<Rc<BarCx>>> = const { RefCell::new(Vec::new()) };
    /// The bar whose message is being handled (null outside the bars' own messages).
    static CUR: Cell<HWND> = const { Cell::new(null_mut()) };
    /// The bar the open panel was opened from: what its follow-ups (a menu, a move) anchor to when no bar message is running.
    static OPENER: Cell<HWND> = const { Cell::new(null_mut()) };
}

/// Every bar, the main one first.
pub(crate) fn all() -> Vec<Rc<BarCx>> {
    BARS.with_borrow(Clone::clone)
}

pub(crate) fn of(h: HWND) -> Option<Rc<BarCx>> {
    BARS.with_borrow(|b| b.iter().find(|b| b.hwnd == h).cloned())
}

pub(crate) fn main_bar() -> Option<Rc<BarCx>> {
    BARS.with_borrow(|b| b.first().cloned())
}

/// The bar a bar-relative question is about: the one whose message is being handled, else the one the open panel came from, else
/// the main bar.
pub(crate) fn cur() -> Option<Rc<BarCx>> {
    of(CUR.get()).or_else(|| of(OPENER.get())).or_else(main_bar)
}

/// Runs `f` with `h` as the bar being served (what `cur` answers), then puts the previous one back.
pub(crate) fn serving<R>(h: HWND, f: impl FnOnce() -> R) -> R {
    let prev = CUR.replace(h);
    let r = f();
    CUR.set(prev);
    r
}

/// The bar a keyboard shortcut means: the one on the monitor the pointer is on (the main bar when there is none).
pub(crate) unsafe fn at_pointer() -> Option<Rc<BarCx>> {
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    let m = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) as isize;
    BARS.with_borrow(|b| b.iter().find(|b| b.mon.get().id == m).or(b.first()).cloned())
}

/// A panel opened or closed: the bar it came from is the one a message is being handled for (the keyboard's are `serving` the
/// bar at the pointer); one opened by nobody's click (a notification's) is the main bar's. (The panel's host is busy while it
/// calls this, so it cannot be asked whether a panel is open.)
pub(crate) fn opened() {
    OPENER.set(CUR.get());
}

/// The bar the open panel (or old flyout) came from; `None` when nothing is open or nobody noted where it came from.
pub(crate) fn opener() -> Option<HWND> {
    (crate::ui::panel::any_open() || fly::any_open()).then(|| OPENER.get()).filter(|h| !h.is_null())
}

/// Whether `b` is the bar the open panel's lit button is on: the one it came from (the main bar when nobody noted).
pub(crate) fn lit(b: &BarCx) -> bool {
    let o = OPENER.get();
    if o.is_null() { b.main } else { o == b.hwnd }
}

/// Asks every bar for a frame.
pub(crate) unsafe fn invalidate_all() {
    for b in all() {
        InvalidateRect(b.hwnd, null(), 0);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the monitors

/// The monitors now, in the order Windows lists them.
pub(crate) unsafe fn monitors() -> Vec<Mon> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> i32 {
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut mi) != 0 {
            let (mut dx, mut dy) = (0u32, 0u32);
            let dpi = if windows_sys::Win32::UI::HiDpi::GetDpiForMonitor(m, 0, &mut dx, &mut dy) == 0 && dx > 0 { dx } else { 96 };
            let r = mi.rcMonitor;
            (*(lp as *mut Vec<Mon>)).push(Mon { id: m as isize, rect: (r.left, r.top, r.right, r.bottom), primary: mi.dwFlags & 1 != 0, dpi });
        }
        1
    }
    let mut v: Vec<Mon> = Vec::new();
    EnumDisplayMonitors(null_mut(), null(), Some(each), &mut v as *mut _ as LPARAM);
    v
}

/// The monitor of a window (the nearest one when it is between two).
pub(crate) unsafe fn monitor_of(w: HWND) -> isize {
    MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST) as isize
}

// ---------------------------------------------------------------------------------------------------------------------
// windows

/// Creates the window of a bar for monitor `mon` and its shadow window under it. `main`: the first one, the one that owns the
/// shell's registrations (its class is the one other programs look for).
unsafe fn create(mon: Mon, main: bool) -> Option<Rc<BarCx>> {
    let hi = GetModuleHandleW(null());
    let class = wide(if main { "topbar" } else { "topbar_extra" });
    let (x, y, w) = (mon.rect.0, mon.rect.1, mon.w());
    if !main {
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hi, hCursor: LoadCursorW(null_mut(), IDC_ARROW), lpszClassName: class.as_ptr(), ..zeroed() });
    }
    let scale = if main { SCALE.get() } else { mon.scale() };
    let thick = (shape().thick * scale).round() as i32;
    let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | ui::gfx::window_ex(), class.as_ptr(), class.as_ptr(), WS_POPUP, x, y, w, thick, null_mut(), null_mut(), hi, null());
    if hwnd.is_null() {
        return None;
    }
    // The floating styles' shadow: a click-through window right under the bar (see redraw())
    let shc = wide("topbar_shadow");
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: hi, lpszClassName: shc.as_ptr(), ..zeroed() });
    let shadow = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | ui::gfx::window_ex_through(), shc.as_ptr(), null(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), hi, null());
    ui::gfx::through(whwnd(shadow));
    // DWM's peek at the desktop (hovering the show-desktop button) keeps the bar on screen, as it does Windows' own taskbar
    for w in [hwnd, shadow] {
        let on = 1u32;
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(w, 12 /* DWMWA_EXCLUDED_FROM_PEEK */, std::ptr::from_ref(&on).cast(), 4);
    }
    let mut renderer = barui::Bar::new(scale).ok();
    if let Some(r) = renderer.as_mut() {
        r.set_origin((mon.rect.0, mon.rect.1));
    }
    // The bar may run elevated while apps and explorer do not; UIPI would drop their messages to us (appbar callbacks, files dropped).
    ChangeWindowMessageFilterEx(hwnd, CALLBACK, MSGFLT_ALLOW, null_mut());
    ChangeWindowMessageFilterEx(hwnd, TB_MSG.get(), MSGFLT_ALLOW, null_mut()); // explorer restarted (it runs unelevated)
    ChangeWindowMessageFilterEx(hwnd, winrust_common::deskmenu::request_msg(), MSGFLT_ALLOW, null_mut()); // rust-explorer's desktop asks for its menu
    // files dragged from a normal-rights app onto the (elevated) bar: Start button = add to Start, elsewhere = pin
    DragAcceptFiles(hwnd, 1);
    for m in [WM_DROPFILES, 0x49 /* WM_COPYGLOBALDATA */, WM_COPYDATA] {
        ChangeWindowMessageFilterEx(hwnd, m, MSGFLT_ALLOW, null_mut());
    }
    let b = Rc::new(BarCx {
        hwnd,
        shadow,
        main,
        mon: Cell::new(mon),
        scale: Cell::new(scale),
        ui: RefCell::new(renderer),
        hot: Cell::new(NONE),
        press: Cell::new((i32::MIN, 0)),
        fs: Cell::new(false),
        desk_hide: Cell::new(false),
        desk_t: Cell::new(0),
        btns: RefCell::new(Vec::new()),
        appbar: Cell::new(false),
        strip: Cell::new((0, 0, 0, 0)),
    });
    BARS.with_borrow_mut(|v| if main { v.insert(0, b.clone()) } else { v.push(b.clone()) });
    Some(b)
}

/// The main bar's window, made at start for the primary monitor.
pub(crate) unsafe fn create_main() -> Option<Rc<BarCx>> {
    let mons = monitors();
    let mon = bar_monitors(&mons, false).first().copied().unwrap_or(Mon { id: 0, rect: (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)), primary: true, dpi: 96 });
    create(mon, true)
}

/// Registers a bar's window as an appbar (again after explorer restarts: its list starts empty).
pub(crate) unsafe fn appbar_register(b: &BarCx) {
    let mut ab: APPBARDATA = zeroed();
    ab.cbSize = size_of::<APPBARDATA>() as u32;
    ab.hWnd = b.hwnd;
    ab.uCallbackMessage = CALLBACK;
    SHAppBarMessage(ABM_NEW, &mut ab);
    b.appbar.set(true);
}

/// Takes down a bar that is not the main one: its appbar, its strip of the work area (given back unless the monitor is gone) and
/// its windows.
unsafe fn destroy(b: &Rc<BarCx>, monitor_alive: bool) {
    pacing::set(b.hwnd as isize, false);
    if b.appbar.replace(false) {
        let mut ab: APPBARDATA = zeroed();
        ab.cbSize = size_of::<APPBARDATA>() as u32;
        ab.hWnd = b.hwnd;
        SHAppBarMessage(ABM_REMOVE, &mut ab);
    }
    if monitor_alive {
        let r = b.rect();
        set_work_area(RECT { left: r.0, top: r.1, right: r.2, bottom: r.3 }, b.mon.get().id);
    }
    BARS.with_borrow_mut(|v| v.retain(|x| !Rc::ptr_eq(x, b)));
    if OPENER.get() == b.hwnd {
        OPENER.set(null_mut());
        ui::panel::close_now();
        fly::close_now();
    }
    DestroyWindow(b.shadow);
    DestroyWindow(b.hwnd);
}

/// The window of a bar that is not the main one was destroyed.
pub(crate) unsafe fn destroyed(h: HWND) {
    BARS.with_borrow_mut(|v| v.retain(|x| x.hwnd != h || x.main));
}

/// Takes down every bar but the main one (the program is closing).
pub(crate) unsafe fn destroy_extra() {
    for b in all().into_iter().filter(|b| !b.main) {
        destroy(&b, true);
    }
}

/// Makes the bars match the monitors and the settings: one for the primary monitor, one for each other monitor when "all
/// displays" is on; a bar whose monitor is gone (or whose setting went off) is taken down and its strip given back, a new one is
/// made, the others follow their monitor (rectangle, scale). Called at start, on `WM_DISPLAYCHANGE` and `WM_DPICHANGED`, and when
/// the settings change; every bar is placed again.
pub(crate) unsafe fn sync() {
    let mons = monitors();
    let want = bar_monitors(&mons, i18n::style::bar_on_all_displays());
    let Some(main) = main_bar() else { return };
    // the main bar follows the primary monitor (the user may have made another one primary)
    if let Some(&m) = want.first() {
        main.mon.set(m);
        if let Some(r) = main.ui.borrow_mut().as_mut() {
            r.set_origin((m.rect.0, m.rect.1));
        }
    }
    for b in all().into_iter().filter(|b| !b.main) {
        match want.iter().skip(1).find(|m| m.id == b.mon.get().id) {
            Some(&m) => {
                let was = b.mon.get();
                b.mon.set(m);
                if was.dpi != m.dpi {
                    // another scale: the renderer is made again (its surfaces and fonts are in device pixels)
                    b.scale.set(m.scale());
                    *b.ui.borrow_mut() = barui::Bar::new(m.scale()).ok();
                }
                if let Some(r) = b.ui.borrow_mut().as_mut() {
                    r.set_origin((m.rect.0, m.rect.1));
                }
            }
            None => destroy(&b, mons.iter().any(|m| m.id == b.mon.get().id)),
        }
    }
    for &m in want.iter().skip(1) {
        if !all().iter().any(|b| b.mon.get().id == m.id) && let Some(b) = create(m, false) {
            appbar_register(&b);
        }
    }
    rebuild_buttons();
    for b in all() {
        place(b.hwnd);
    }
}

/// Re-derives every bar's app buttons from the windows' buttons (`BTNS`), their monitors and the setting.
pub(crate) unsafe fn rebuild_buttons() {
    let apps = i18n::style::apps_on();
    let bars = all();
    let btns: Vec<(bool, Vec<(HWND, isize)>)> = BTNS.with_borrow(|b| {
        b.iter().skip(1).map(|b| (b.pin.is_some(), b.wins.iter().map(|&w| (w, if apps == AppsOn::Here && bars.len() > 1 { monitor_of(w) } else { 0 })).collect())).collect()
    });
    for b in bars {
        let list = bar_buttons(&btns, b.mon.get().id, if bars_count() > 1 { apps } else { AppsOn::All });
        *b.btns.borrow_mut() = list.into_iter().map(|(i, wins)| BarBtn { idx: i + 1, wins }).collect();
    }
}

fn bars_count() -> usize {
    BARS.with_borrow(Vec::len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: isize, rect: R4, primary: bool) -> Mon {
        Mon { id, rect, primary, dpi: 96 }
    }

    /// The user's setup: 2560x1440 primary and 1920x1080 to its left at negative x.
    fn two() -> Vec<Mon> {
        vec![mon(2, (-1920, 0, 0, 1080), false), mon(1, (0, 0, 2560, 1440), true)]
    }

    #[test]
    fn a_bar_for_the_primary_monitor_and_one_for_each_other_when_all_displays_is_on() {
        let ids = |v: Vec<Mon>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids(bar_monitors(&two(), true)), [1, 2], "the main bar first, whatever order Windows lists them in");
        assert_eq!(ids(bar_monitors(&two(), false)), [1], "the setting off: the primary monitor only");
        let one = vec![mon(1, (0, 0, 1920, 1080), true)];
        assert_eq!(ids(bar_monitors(&one, true)), [1]);
        let three = vec![mon(5, (1920, 0, 3840, 1080), false), mon(3, (0, 0, 1920, 1080), true), mon(4, (-1280, 0, 0, 1024), false)];
        assert_eq!(ids(bar_monitors(&three, true)), [3, 5, 4]);
        assert!(bar_monitors(&[], true).is_empty());
        assert_eq!(bar_monitors(&[mon(9, (0, 0, 800, 600), false)], true)[0].id, 9, "no primary flag: the first monitor is the main one");
    }

    #[test]
    fn the_other_bars_are_lighter_unless_they_are_set_to_be_the_same() {
        assert!(full_content(true, OtherBars::Win11) && full_content(true, OtherBars::Same));
        assert!(!full_content(false, OtherBars::Win11));
        assert!(full_content(false, OtherBars::Same));
    }

    #[test]
    fn every_bar_shows_every_window_unless_apps_are_kept_to_their_monitor() {
        // (pinned, windows with their monitors): Edge pinned with a window on each monitor, Notepad unpinned on monitor 2,
        // Files pinned and not running, Calc unpinned on monitor 1
        let btns: Vec<(bool, Vec<(u32, isize)>)> = vec![(true, vec![(10, 1), (11, 2)]), (false, vec![(20, 2)]), (true, vec![]), (false, vec![(30, 1)])];
        let all = |m| bar_buttons(&btns, m, AppsOn::All);
        assert_eq!(all(1), vec![(0, vec![10, 11]), (1, vec![20]), (2, vec![]), (3, vec![30])]);
        assert_eq!(all(2), all(1), "Windows 11's default: the same on every bar");
        let here = |m| bar_buttons(&btns, m, AppsOn::Here);
        assert_eq!(here(1), vec![(0, vec![10]), (2, vec![]), (3, vec![30])], "its own windows; the pin that has none here stays, Notepad is not here");
        assert_eq!(here(2), vec![(0, vec![11]), (1, vec![20]), (2, vec![])], "Calc is not here, the pinned Files is");
    }

    #[test]
    fn a_window_that_moves_to_the_other_monitor_takes_its_button_along() {
        let mut btns: Vec<(bool, Vec<(u32, isize)>)> = vec![(false, vec![(20, 2)])];
        assert_eq!(bar_buttons(&btns, 1, AppsOn::Here), vec![]);
        assert_eq!(bar_buttons(&btns, 2, AppsOn::Here), vec![(0, vec![20])]);
        btns[0].1[0].1 = 1;
        assert_eq!(bar_buttons(&btns, 1, AppsOn::Here), vec![(0, vec![20])]);
        assert_eq!(bar_buttons(&btns, 2, AppsOn::Here), vec![]);
        // two windows of one app, one on each monitor: the button is on both bars with one window each
        btns[0].1.push((21, 2));
        assert_eq!(bar_buttons(&btns, 1, AppsOn::Here), vec![(0, vec![20])]);
        assert_eq!(bar_buttons(&btns, 2, AppsOn::Here), vec![(0, vec![21])]);
    }

    #[test]
    fn the_strip_and_the_work_area_are_per_monitor() {
        let main = (0, 0, 2560, 1440);
        let left = (-1920, 0, 0, 1080);
        // a bottom bar 54 thick: the strip at the foot of its own monitor, the work area is what is above it
        assert_eq!(strip_rect(main, Side::Bottom, 54), (0, 1386, 2560, 1440));
        assert_eq!(work_rect(main, Side::Bottom, (0, 1386, 2560, 1440)), (0, 0, 2560, 1386));
        assert_eq!(strip_rect(left, Side::Bottom, 54), (-1920, 1026, 0, 1080));
        assert_eq!(work_rect(left, Side::Bottom, (-1920, 1026, 0, 1080)), (-1920, 0, 0, 1026));
        // side bars
        assert_eq!(strip_rect(left, Side::Left, 54), (-1920, 0, -1866, 1080));
        assert_eq!(work_rect(left, Side::Left, (-1920, 0, -1866, 1080)), (-1866, 0, 0, 1080));
        assert_eq!(strip_rect(left, Side::Right, 54), (-54, 0, 0, 1080));
        assert_eq!(work_rect(left, Side::Right, (-54, 0, 0, 1080)), (-1920, 0, -54, 1080));
        // auto-hide reserves nothing: the work area is the monitor
        assert_eq!(work_rect(left, Side::Bottom, strip_rect(left, Side::Bottom, 0)), left);
    }

    /// Two bars on "monitors" far off the real ones (nothing is shown on a screen, and no appbar or work area is touched): each
    /// has its own window, renderer, pointer and anchor, and the bar being served is the one `cur` answers.
    #[test]
    fn each_bar_has_its_own_window_renderer_pointer_and_anchor() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread; windows made and destroyed here, far off any monitor.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok();
            let hi = GetModuleHandleW(null());
            let class = wide("topbar");
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hi, lpszClassName: class.as_ptr(), ..zeroed() });
            ui::gfx::compose_windows();
            SCALE.set(1.0);
            let left = Mon { id: 21, rect: (-30_000, 0, -28_080, 1080), primary: false, dpi: 96 };
            let main = Mon { id: 22, rect: (-27_000, 0, -24_440, 1440), primary: true, dpi: 96 };
            let (m, l) = (create(main, true).expect("main bar"), create(left, false).expect("second bar"));
            assert_eq!(all().len(), 2);
            assert!(Rc::ptr_eq(&main_bar().unwrap(), &m), "the main bar comes first");
            assert!(m.full() && (l.full() == (i18n::style::other_bars() == OtherBars::Same)));
            for b in [&m, &l] {
                redraw(b.hwnd);
            }
            // each bar stands on its own monitor, on the virtual screen (whatever side and style the machine has set)
            let inside = |b: &BarCx| {
                let r = b.ui.borrow().as_ref().unwrap().placement().rest;
                let m = b.rect();
                r.0 >= m.0 && r.1 >= m.1 && r.0 + r.2 <= m.2 && r.1 + r.3 <= m.3 && r.2 > 0 && r.3 > 0
            };
            assert!(inside(&m) && inside(&l), "{:?} {:?}", m.ui.borrow().as_ref().unwrap().placement().rest, l.ui.borrow().as_ref().unwrap().placement().rest);
            let (wm, wl) = (m.win(), l.win());
            assert!(wm.0 >= -27_000 - 1 && wl.0 >= -30_000 - 1 && wl.0 < -28_000, "{wm:?} {wl:?}");
            // anchors: the button, the bar and the monitor, all in virtual screen coordinates
            let a = anchor_of(&l, CLOCK).expect("an anchor over the clock");
            assert_eq!((a.origin, a.screen), ((-30_000, 0), (1920, 1080)));
            assert!(a.button.0 >= -30_000 && a.button.2 <= -28_080 && a.button.3 <= 1080 && a.button.2 > a.button.0, "{:?}", a.button);
            assert!(a.bar.0 >= -30_000 && a.bar.2 <= -28_080, "{:?}", a.bar);
            // the pointer: a bar's client point is found in that bar's own layout
            let (cx, cy) = {
                let u = l.ui.borrow();
                let r = u.as_ref().unwrap().layout().rect_of(CLOCK).unwrap();
                (r.cx() as i32, r.cy() as i32)
            };
            assert_eq!(hit(&l, cx, cy), CLOCK);
            assert_eq!(hit(&m, cx, cy), NONE, "the main bar has other things there");
            l.hot.set(CLOCK);
            assert_eq!((l.hot.get(), m.hot.get()), (CLOCK, NONE), "hover is per bar");
            // which bar a question is about
            assert_eq!(cur().unwrap().hwnd, m.hwnd, "nobody served and no panel open: the main bar");
            assert_eq!(serving(l.hwnd, || cur().unwrap().hwnd), l.hwnd);
            assert_eq!(cur().unwrap().hwnd, m.hwnd, "and back");
            serving(l.hwnd, opened);
            assert!(lit(&l) && !lit(&m) && cur().unwrap().hwnd == l.hwnd, "the panel came from the second bar: its button is the lit one");
            serving(null_mut(), opened);
            assert!(lit(&m) && !lit(&l), "a panel nobody's click opened is the main bar's");
            // the lighter bar answers no anchor over buttons it does not have
            if !l.full() {
                assert_eq!(l.ui.borrow().as_ref().unwrap().layout().rect_of(GEAR).map(|r| r.w), None);
            }
            // one bar leaves; the other stays
            destroy(&l, false);
            assert_eq!(all().len(), 1);
            DestroyWindow(m.shadow);
            DestroyWindow(m.hwnd);
            BARS.with_borrow_mut(Vec::clear);
        }
    }

    #[test]
    fn the_hot_zone_of_an_edge_is_on_the_bars_own_monitor() {
        let (main, left) = ((0, 0, 2560, 1440), (-1920, 0, 0, 1080));
        assert!(pointer_at_edge(main, Side::Bottom, (1000, 1439), 6));
        assert!(!pointer_at_edge(main, Side::Bottom, (1000, 1430), 6), "outside the 6 px strip while the bar is away");
        assert!(pointer_at_edge(main, Side::Bottom, (1000, 1400), 54), "over the bar while it is out");
        assert!(!pointer_at_edge(main, Side::Bottom, (-500, 1075), 6), "the other monitor's bottom edge is not this one's");
        assert!(pointer_at_edge(left, Side::Bottom, (-500, 1075), 6));
        assert!(pointer_at_edge(left, Side::Left, (-1920, 500), 6) && !pointer_at_edge(main, Side::Left, (-1, 500), 6), "x < 6 was true all over the monitor to the left");
        assert!(pointer_at_edge(left, Side::Right, (-1, 500), 6) && !pointer_at_edge(left, Side::Right, (1, 500), 6));
    }

    #[test]
    fn a_far_off_answer_of_query_pos_goes_back_to_the_edge_of_its_own_monitor() {
        let left = (-1920, 0, 0, 1080);
        // the system moved the strip up by 20: another appbar is below it; kept
        assert_eq!(settle_strip(left, Side::Bottom, (-1920, 1006, 0, 1060), 54), (-1920, 1006, 0, 1060));
        // a stale entry made it answer half the monitor away: back to the edge
        assert_eq!(settle_strip(left, Side::Bottom, (-1920, 400, 0, 454), 54), (-1920, 1026, 0, 1080));
        assert_eq!(settle_strip(left, Side::Left, (-1000, 0, -946, 1080), 54), (-1920, 0, -1866, 1080));
        assert_eq!(settle_strip(left, Side::Right, (-1000, 0, -946, 1080), 54), (-54, 0, 0, 1080));
        // the main bar's old arithmetic, unchanged: (0, 0) based
        assert_eq!(settle_strip((0, 0, 2560, 1440), Side::Bottom, (0, 1386, 2560, 1440), 54), (0, 1386, 2560, 1440));
        assert_eq!(sane_edge(-3, 1440), 0);
        assert_eq!(sane_edge(360, 1440), 360);
        assert_eq!(sane_edge(361, 1440), 0);
    }
}
