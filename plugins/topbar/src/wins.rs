//! topbar: the panels about windows: the previews over an app button, Task view and the Alt+Tab switcher.
//! (split out of main.rs; shares its items through `super::*`)
//!
//! All three show windows live: each window is a hole in the panel and DWM draws its thumbnail through it
//! (`ui::live`); the previews and the switcher do not take the keyboard focus from the app under them.
use super::*;
use std::rc::Rc;
use ui::panel::{self, Id};
use ui::preview::{self, Card, PreviewPanel};
use ui::taskview::{self, Desk, OwnedIcon, Switch, Switcher, TaskView, WinCard};

thread_local! {
    /// The switcher's windows (z-order, the active one first) and the selected one.
    static SW: RefCell<(Vec<HWND>, usize)> = const { RefCell::new((Vec::new(), 0)) };
    // where the pointer was when the switcher opened: a window appearing under a still pointer gets a WM_MOUSEMOVE,
    // which must not pick the card under it (#183, #116); None once the pointer really moved
    static SW_PT: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
    /// The monitor it is shown on (left, top, right, bottom).
    static SW_MON: Cell<(i32, i32, i32, i32)> = const { Cell::new((0, 0, 0, 0)) };
    /// The app button the previews are open for, and when the pointer was last seen away from both.
    static PV_BTN: Cell<i32> = const { Cell::new(NONE) };
    static PV_AWAY: Cell<u64> = const { Cell::new(0) };
    /// A window was closed from a panel: which panel (the app button, or `TASKVIEW`) lays itself out again, without it.
    static REOPEN: Cell<(i32, isize)> = const { Cell::new((NONE, 0)) };
}

pub(crate) unsafe fn cursor() -> (i32, i32) {
    let mut p: POINT = zeroed();
    GetCursorPos(&mut p);
    (p.x, p.y)
}

pub(crate) unsafe fn window_title(w: HWND) -> String {
    let mut b = [0u16; 256];
    let n = GetWindowTextW(w, b.as_mut_ptr(), 256).max(0) as usize;
    String::from_utf16_lossy(&b[..n])
}

/// The icon a window's card shows: its program's own (48 px), or the window's for the hosts that stand for many apps.
/// The caller owns the copy.
unsafe fn card_icon(w: HWND) -> HICON {
    let exe = exe_of(w);
    // generic hosts represent many unrelated applications; preserve the window's icon
    let shell = if exe.is_empty() || is_host(&exe) { null_mut() } else { item_icon(&exe, sc(48)) };
    if !shell.is_null() {
        return shell;
    }
    let own = icon_of(w);
    CopyIcon(if own.is_null() { LoadIconW(null_mut(), IDI_APPLICATION) } else { own })
}

unsafe fn win_cards(wins: &[HWND]) -> Vec<WinCard> {
    wins.iter()
        .map(|&w| WinCard {
            hwnd: w as isize,
            title: window_title(w),
            icon: Rc::new(OwnedIcon(wicon(card_icon(w)))),
            frame: ui::live::frame_of(w as isize),
            minimized: IsIconic(w) != 0 || !ui::live::drawable(BAR.load(Relaxed) as HWND, w),
        })
        .collect()
}

/// A click on a card: the window comes to the front (restored if it was minimized).
unsafe fn show(w: HWND) {
    if IsIconic(w) != 0 {
        ShowWindow(w, SW_RESTORE);
    }
    force_fg(w);
}

/// A window was closed from a panel: after a moment the panel is laid out again without it.
unsafe fn closed(key: i32, w: HWND) {
    close_window(w);
    REOPEN.set((key, w as isize));
    SetTimer(BAR.load(Relaxed) as HWND, 10, 180, None);
}

/// Timer 10: lays out again the panel a window was closed from.
pub(crate) unsafe fn reopen(bar: HWND) {
    KillTimer(bar, 10);
    let (key, skip) = REOPEN.replace((NONE, 0));
    if key == TASKVIEW && panel::is_open(Id::Task) {
        refresh_windows();
        if let Some((c, a)) = taskview_content(true, skip) {
            panel::open(c, a);
        }
    } else if key >= 1 && panel::is_open(Id::Preview) {
        refresh_windows();
        if !preview_open(key, true, skip) {
            panel::close_id(Id::Preview, false);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// previews over an app button

/// Shows the windows of app button `btn` as live cards above it. False when it has none (the caller shows the tip).
pub(crate) unsafe fn preview_open(btn: i32, calm: bool, skip: isize) -> bool {
    let (wins, pin, bicon) = BTNS.with_borrow(|b| b.get(btn as usize).map(|b| (b.wins.clone(), b.pin, b.icon)).unwrap_or((vec![], None, null_mut())));
    let wins: Vec<HWND> = wins.into_iter().filter(|&w| IsWindow(w) != 0 && w as isize != skip).collect();
    if wins.is_empty() || btn <= 0 {
        return false;
    }
    let Some(a) = anchor(btn) else { return false };
    let ic = match pin {
        Some(p) => PINS.with_borrow(|pins| pins.get(p).map_or(bicon, |p| p.icon)),
        None => bicon,
    };
    let ic = if ic.is_null() { wins.first().map_or(ic, |&w| icon_of(w)) } else { ic };
    // as many cards as the screen has room for: the rest are in the jump list
    let fit = (((a.screen.0 as f32 / SCALE.get()) - 24.0 - 16.0 + 8.0) / (preview::CARD_W + 8.0)).max(1.0) as usize;
    let fg = GetForegroundWindow();
    let cards: Vec<Card> = wins
        .iter()
        .take(fit)
        .map(|&w| Card { hwnd: w as isize, title: window_title(w), src: ui::live::source_size(w as isize), minimized: IsIconic(w) != 0, current: w == fg })
        .collect();
    PV_BTN.set(btn);
    PV_AWAY.set(0);
    let panel = PreviewPanel::new(cards, wicon(ic), icon_color(ic), calm, move |p| match p {
        preview::Pick::Window(w) => {
            PV_BTN.set(NONE);
            // SAFETY: handles of other programs' windows, which the system checks.
            unsafe { show(w as HWND) }
        }
        // SAFETY: as above.
        preview::Pick::Close(w) => unsafe { closed(btn, w as HWND) },
    });
    panel::open(Box::new(panel), a);
    SetTimer(BAR.load(Relaxed) as HWND, 9, 80, None);
    true
}

/// Timer 9, while the previews are open: they go once the pointer has been away from the button and from them for 220 ms
/// (the mockup's `hvLeave`), and coming back to either keeps them.
pub(crate) unsafe fn preview_tick(bar: HWND) {
    let Some(r) = panel::rect_of(Id::Preview) else {
        KillTimer(bar, 9);
        PV_BTN.set(NONE);
        return;
    };
    let p = cursor();
    let inside = p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3;
    if inside || HOT.get() == PV_BTN.get() {
        PV_AWAY.set(0);
    } else if PV_AWAY.get() == 0 {
        PV_AWAY.set(now_ms());
    } else if now_ms().saturating_sub(PV_AWAY.get()) >= 220 {
        hide_preview();
    }
}

pub(crate) unsafe fn hide_preview() {
    panel::close_id(Id::Preview, false);
    KillTimer(BAR.load(Relaxed) as HWND, 9);
    PV_BTN.set(NONE);
}

/// The previews are on the screen for this button.
pub(crate) fn preview_for(btn: i32) -> bool {
    panel::is_open(Id::Preview) && PV_BTN.get() == btn
}

// ---------------------------------------------------------------------------------------------------------------------
// Task view

pub(crate) unsafe fn taskview_toggle() {
    panel::toggle(Id::Task, || {
        fly::close_now();
        taskview_content(false, 0)
    });
}

unsafe fn taskview_content(calm: bool, skip: isize) -> Option<(Box<dyn panel::Content>, panel::Anchor)> {
    let a = anchor(TASKVIEW)?;
    let (desks, cur) = vdesk::desktops();
    let mut v: Vec<HWND> = Vec::new();
    EnumWindows(Some(collect), &mut v as *mut _ as LPARAM); // z-order: the active window first
    v.retain(|&w| w as isize != skip); // (`collect` leaves out what DWM cloaks: the windows of the other desktops)
    let mut mini = vdesk::miniatures(&desks, cur);
    // Without Explorer there is only one desktop; use the same windows as the cards even when DWM's cloak state is unavailable.
    if !EXPLORER_UP.load(Relaxed) && let Some(current) = mini.get_mut(cur) {
        current.clear();
        for &w in v.iter().rev().filter(|&&w| IsIconic(w) == 0) {
            let (l, t, r, b) = ui::live::frame_of(w as isize);
            let norm = |v: i32, span: i32| (v as f32 / span.max(1) as f32).clamp(0.0, 1.0);
            let (x0, y0, x1, y1) = (norm(l, a.screen.0), norm(t, a.screen.1), norm(r, a.screen.0), norm(b, a.screen.1));
            if x1 > x0 && y1 > y0 { current.push((x0, y0, x1 - x0, y1 - y0)); }
        }
    }
    let can_add = EXPLORER_UP.load(Relaxed) && desks.len() < 8;
    let list: Vec<Desk> = desks.iter().zip(mini).enumerate().map(|(i, (d, wins))| Desk { name: d.name.clone(), current: i == cur, wallpaper: d.wallpaper.clone(), wins }).collect();
    let tv = TaskView::new(SCALE.get(), a.screen, list, can_add, win_cards(&v), calm, move |p| {
        // SAFETY: handles of other programs' windows, which the system checks; keys go to the system.
        unsafe {
            match p {
                taskview::Pick::Window(w) => show(w as HWND),
                taskview::Pick::Close(w) => closed(TASKVIEW, w as HWND),
                taskview::Pick::Desktop(i) => vdesk::go(cur, i),
                taskview::Pick::NewDesktop => vdesk::add(),
            }
        }
    });
    Some((Box::new(tv), a))
}

// ---------------------------------------------------------------------------------------------------------------------
// Alt+Tab switcher: the windows of the desktop as live cards, most recently used first (#92)

fn sw_selected() -> usize {
    SW.with_borrow(|s| s.1)
}

/// The pointer is on card `i`: it becomes the selection, unless the pointer has not really moved yet.
fn sw_point(i: usize) {
    // SAFETY: a plain query.
    if SW_PT.get().is_some_and(|p| p == unsafe { cursor() }) {
        return;
    }
    SW_PT.set(None);
    SW.with_borrow_mut(|s| {
        if i < s.0.len() {
            s.1 = i;
        }
    });
}

fn sw_pick(p: taskview::Pick) {
    // SAFETY: handles of other programs' windows, which the system checks; keys go to the system.
    unsafe {
        match p {
            taskview::Pick::Window(w) => {
                SW.with_borrow_mut(|s| {
                    if let Some(i) = s.0.iter().position(|&x| x as isize == w) {
                        s.1 = i;
                    }
                });
                SWITCHING.store(false, Relaxed);
                if GetAsyncKeyState(VK_MENU as i32) < 0 {
                    keys(&[0xE8]); // Alt is still down: its release must not open the menu bar of the app we switch to
                }
                alt_done(false);
            }
            taskview::Pick::Close(w) => sw_remove(w as HWND),
            _ => {}
        }
    }
}

unsafe fn open_switcher() {
    let mon = SW_MON.get();
    let wins = SW.with_borrow(|(v, _)| v.clone());
    let Some(mut a) = anchor(NONE) else { return };
    a.screen = (mon.2, mon.3);
    let sw = Switch { selected: sw_selected, point: sw_point, pick: sw_pick };
    let panel = Switcher::new(SCALE.get(), mon, win_cards(&wins), sw);
    panel::open(Box::new(panel), a);
}

pub(crate) unsafe fn alt_tab(back: bool) {
    if !panel::is_open(Id::Switch) {
        let mut v: Vec<HWND> = Vec::new();
        EnumWindows(Some(collect), &mut v as *mut _ as LPARAM); // z-order: the active window first, then the one before it
        // per monitor: only the windows on the active window's monitor (when there is another one to switch to), shown on that monitor
        let mut mon = RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) };
        if i18n::style::alttab_per_monitor() && !v.is_empty() {
            let of = |w: HWND| MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST);
            let m0 = of(v[0]);
            let mut mi: MONITORINFO = zeroed();
            mi.cbSize = size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(m0, &mut mi) != 0 {
                let same: Vec<HWND> = v.iter().copied().filter(|&w| of(w) == m0).collect();
                if same.len() > 1 {
                    v = same;
                    mon = mi.rcMonitor;
                }
            }
        }
        let n = v.len();
        if n == 0 {
            return SWITCHING.store(false, Relaxed);
        }
        SW.set((v, if n == 1 { 0 } else if back { n - 1 } else { 1 }));
        SW_PT.set(Some(cursor()));
        SW_MON.set((mon.left, mon.top, mon.right, mon.bottom));
        open_switcher();
    } else {
        SW.with_borrow_mut(|(v, i)| *i = if back { (*i + v.len() - 1) % v.len() } else { (*i + 1) % v.len() });
        panel::refresh();
    }
}

/// Takes `w` off the switcher and asks it to close.
unsafe fn sw_remove(w: HWND) {
    SW.with_borrow_mut(|(v, i)| {
        if let Some(k) = v.iter().position(|&x| x == w) {
            v.remove(k);
            *i = (*i).min(v.len().saturating_sub(1));
        }
    });
    close_window(w);
    if SW.with_borrow(|(v, _)| v.is_empty()) {
        alt_done(true);
        SWITCHING.store(false, Relaxed);
    } else {
        open_switcher();
    }
}

/// Delete in the switcher: closes the selected window and takes it off the list.
pub(crate) unsafe fn alt_close() {
    if let Some(w) = SW.with_borrow(|(v, i)| v.get(*i).copied()) {
        sw_remove(w);
    }
}

/// Alt released: switch to the selected window (restored if minimized). Esc: just close.
pub(crate) unsafe fn alt_done(cancel: bool) {
    panel::close_id(Id::Switch, true);
    let (v, i) = SW.take();
    if let Some(&w) = v.get(i).filter(|_| !cancel) {
        show(w);
    }
}
