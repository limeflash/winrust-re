//! The Identify button of the display arrangement: a number on every display for a moment, as the mockup's `.dlgbg` tiles (120 x 120,
//! radius 20, 64 px white 600 on `rgba(0,0,0,.65)`). One small window per display, composed like the bar's toast (a DirectComposition
//! surface when the machine can, a layered window otherwise), click-through and never active, gone after `SHOW_MS`.
//! Only the real source makes them (`Live::identify`); the demo one writes to its log, and `selftest` makes them off the screen.
use std::cell::RefCell;

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForSystem,
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, KillTimer, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer, SetWindowPos, ShowWindow, WM_TIMER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            WS_POPUP,
        },
    },
};
use winrust_ui::{
    frame::register_class,
    gfx::{self, Align, Font, Gfx, Rect, Rgba, Surface},
};

use crate::display::Mon;

/// How long the numbers stay.
pub const SHOW_MS: u32 = 1600;
/// The tile, DIPs, and how far it stands from the display's corner.
const SIDE: f32 = 120.0;
const INSET: f32 = 60.0;
const CLASS: &str = "WinRustSettingsIdentify\0";

thread_local! {
    static OPEN: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// The surface of each window lives as long as the window does.
    static SURFS: RefCell<Vec<(usize, Surface)>> = const { RefCell::new(Vec::new()) };
}

fn to_windows(h: HWND) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(h)
}

/// Draws the tile of display `n` (1-based) into `surf`, `side` device pixels square at `scale`.
fn paint_tile(g: &mut Gfx, surf: &mut Surface, n: usize, scale: f32) -> bool {
    let side = (SIDE * scale).round() as i32;
    g.paint(surf, side, side, |c| {
        let r = Rect::new(0.0, 0.0, SIDE, SIDE);
        c.fill(r, 20.0, Rgba::css(0, 0, 0, 0.65));
        c.text(&n.to_string(), r, Font::new(64.0, 600), Rgba::pure_white(1.0), Align::Center);
    })
    .is_ok()
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_TIMER && wp == 1 {
        // SAFETY: our own window and its own timer.
        unsafe {
            KillTimer(hwnd, 1);
            ShowWindow(hwnd, SW_HIDE);
            DestroyWindow(hwnd);
        }
        OPEN.with_borrow_mut(|o| o.retain(|&h| h != hwnd as usize));
        SURFS.with_borrow_mut(|v| v.retain(|s| s.0 != hwnd as usize));
        return 0;
    }
    // SAFETY: the default procedure of our own window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// One tile per display, at `(mons[i].x + 60, mons[i].y + 60)` desktop pixels (scaled by the system DPI), shown for `SHOW_MS`;
/// `shift` moves them all (the self-test puts them far off the screen). Returns the windows made.
fn spawn(mons: &[Mon], shift: (i32, i32), ms: u32) -> Vec<HWND> {
    let mut out = Vec::new();
    // SAFETY: a plain query.
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let Ok(mut g) = Gfx::new(scale) else { return out };
    // SAFETY: registers a class of this module and creates windows of it; every handle is checked.
    unsafe {
        let hi = GetModuleHandleW(std::ptr::null());
        let name: Vec<u16> = CLASS.encode_utf16().collect();
        register_class(hi, &name, Some(proc), false);
        for (i, m) in mons.iter().enumerate() {
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | gfx::window_ex_through(), name.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), hi, std::ptr::null());
            if hwnd.is_null() {
                continue;
            }
            gfx::through(to_windows(hwnd));
            let mut surf = Surface::composed();
            let x = m.x + shift.0 + (INSET * scale).round() as i32;
            let y = m.y + shift.1 + (INSET * scale).round() as i32;
            if paint_tile(&mut g, &mut surf, i + 1, scale) && surf.present(to_windows(hwnd), x, y, 255) {
                gfx::commit();
                SURFS.with_borrow_mut(|v| v.push((hwnd as usize, surf)));
                SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
                SetTimer(hwnd, 1, ms, None);
                OPEN.with_borrow_mut(|o| o.push(hwnd as usize));
                out.push(hwnd);
            } else {
                DestroyWindow(hwnd);
            }
        }
    }
    out
}

/// The Identify button: shows the number of every display on it for a moment.
pub fn show(mons: &[Mon]) {
    spawn(mons, (0, 0), SHOW_MS);
}

/// How many tiles are up now.
#[must_use]
pub fn open_count() -> usize {
    OPEN.with_borrow(Vec::len)
}

/// `--selftest identify`: two tiles far off the screen, which exist, and are gone again once their time has run out. 0: all well.
pub fn selftest() -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, IsWindow, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};
    let _ = winrust_ui::gfx::compose_windows();
    let mons = [Mon { x: 0, y: 0, w: 2560, h: 1600, primary: true }, Mon { x: 2560, y: 0, w: 3840, h: 2160, primary: false }];
    let made = spawn(&mons, (-20_000, -20_000), 300);
    let mut failed = 0;
    let mut check = |ok: bool, what: &str| {
        println!("{} {what}", if ok { "ok  " } else { "FAIL" });
        failed += i32::from(!ok);
    };
    check(made.len() == 2, "a window for each of two displays");
    // SAFETY: queries on windows of ours.
    check(made.iter().all(|&h| unsafe { IsWindow(h) } != 0), "they exist");
    let end = std::time::Instant::now() + std::time::Duration::from_millis(1500);
    while open_count() > 0 && std::time::Instant::now() < end {
        // SAFETY: the message loop of this thread.
        unsafe {
            let mut m: MSG = std::mem::zeroed();
            while PeekMessageW(&mut m, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    check(open_count() == 0, "their time ran out and they went");
    // SAFETY: queries on windows of ours.
    check(made.iter().all(|&h| unsafe { IsWindow(h) } == 0), "they are destroyed");
    failed
}
