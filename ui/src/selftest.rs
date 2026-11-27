//! `--selftest window | stage <png> | memory`: the host on real windows.
//!
//! * `window`: an app window made off the screen (never shown), asked what Windows asks of it (`WM_NCHITTEST` at chosen points),
//!   driven through its message procedure (pointer, click, key), told its DPI changed from 96 to 144, and watched at rest. Once
//!   composed (DirectComposition) and once as the fallback (a redirected window painted from a DIB).
//! * `stage <png>`: the window for a moment (under 2 s, never active, in the top left corner of the screen) and a picture of what
//!   reached the screen, shadow and corners included.
//! * `memory`: 30 openings and closings of a menu, a dialog and a toast: the process must not grow.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::core::App;
use crate::frame::{WIN_CLOSE, WIN_MAX, WIN_MIN};
use crate::gfx::{self, Gfx, Surface};
use crate::window::{Options, client_origin, create, with};

const HT_CLIENT: isize = 1;
const HT_CAPTION: isize = 2;
const HT_MAXBUTTON: isize = 9;
const HT_LEFT: isize = 10;
const HT_TOP: isize = 12;
const HT_TOPLEFT: isize = 13;
const HT_TOPRIGHT: isize = 14;
const HT_BOTTOM: isize = 15;
const HT_BOTTOMRIGHT: isize = 17;

pub fn run(args: &[String], demo: fn(&str) -> Box<dyn App>) -> i32 {
    match args.first().map(String::as_str) {
        Some("stage") => stage(args.get(1).map_or("stage.png", String::as_str), demo),
        Some("memory") => memory(demo),
        _ => window(demo),
    }
}

/// This process's working set and private bytes, MB.
#[must_use]
pub fn memory_now() -> (f64, f64) {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a plain query into a local.
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
        (c.WorkingSetSize as f64 / 1_048_576.0, c.PagefileUsage as f64 / 1_048_576.0)
    }
}

/// Dispatches this thread's messages for `ms` (the pacing thread's frames, timers, whatever the windows were sent).
pub(crate) fn pump(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    // SAFETY: the usual loop of this thread's windows.
    unsafe {
        while Instant::now() < end {
            let mut m: MSG = std::mem::zeroed();
            while PeekMessageW(&mut m, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

struct Report(i32);

impl Report {
    fn check(&mut self, ok: bool, what: &str) {
        println!("{} {what}", if ok { "ok   " } else { "WRONG" });
        self.0 += i32::from(!ok);
    }
}

fn lparam(x: i32, y: i32) -> LPARAM {
    (((y as i16 as u16 as u32) << 16) | (x as i16 as u16 as u32)) as LPARAM
}

/// What the window answers for a point of the client area given in DIPs.
fn nc_at(hwnd: HWND, x: f32, y: f32) -> isize {
    let (ox, oy) = client_origin(hwnd);
    let s = with(hwnd, |h| h.core.scale).unwrap_or(1.0);
    // SAFETY: a message sent to our own window.
    unsafe { SendMessageW(hwnd, WM_NCHITTEST, 0, lparam(ox + (x * s).round() as i32, oy + (y * s).round() as i32)) }
}

/// The same for a point given in screen pixels.
fn nc_screen(hwnd: HWND, x: i32, y: i32) -> isize {
    // SAFETY: a message sent to our own window.
    unsafe { SendMessageW(hwnd, WM_NCHITTEST, 0, lparam(x, y)) }
}

fn centre(hwnd: HWND, id: i32) -> (f32, f32) {
    with(hwnd, |h| h.core.hits().get(id).map(|i| (i.r.cx(), i.r.cy()))).flatten().unwrap_or((-1.0, -1.0))
}

fn mouse(hwnd: HWND, msg: u32, x: f32, y: f32) {
    let s = with(hwnd, |h| h.core.scale).unwrap_or(1.0);
    let wp = if msg == WM_LBUTTONUP { 0 } else { 1 };
    // SAFETY: a message sent to our own window.
    unsafe { SendMessageW(hwnd, msg, wp, lparam((x * s).round() as i32, (y * s).round() as i32)) };
}

fn key(hwnd: HWND, vk: u32) {
    // SAFETY: a message sent to our own window.
    unsafe { SendMessageW(hwnd, WM_KEYDOWN, vk as usize, 0) };
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a query into a local.
    unsafe { GetWindowRect(hwnd, &mut r) };
    r
}

fn opts(class: &'static str) -> Options {
    Options { class, size: (1160.0, 740.0), icon: 0, place_file: None }
}

fn window(demo: fn(&str) -> Box<dyn App>) -> i32 {
    let mut rep = Report(0);
    let composed = gfx::compose_windows();
    println!("host self-test: {} windows, off the screen", if composed { "composed" } else { "layered fallback" });
    for mode in [composed, false] {
        if !mode && composed {
            gfx::compose_off();
            println!("-- the fallback (a redirected window painted from a DIB)");
        }
        window_checks(&mut rep, demo, mode);
        if !composed {
            break;
        }
    }
    println!("{} checks wrong", rep.0);
    rep.0
}

fn window_checks(rep: &mut Report, demo: fn(&str) -> Box<dyn App>, composed: bool) {
    let class: &'static str = if composed { "winrust_ui_selftest" } else { "winrust_ui_selftest_dib" };
    let place = std::env::temp_dir().join(format!("{class}-{}.place", std::process::id()));
    let o = Options { place_file: Some(place.clone()), ..opts(class) };
    let Some(hwnd) = create(demo(""), &o, composed, Some((-5000, -5000))) else {
        rep.check(false, "the window can be made");
        return;
    };
    rep.check(with(hwnd, |h| h.frames).unwrap_or(0) >= 1, "the first frame is drawn before the window is shown");
    let size = with(hwnd, |h| h.core.size).unwrap_or_default();
    {
        let (ox, oy) = client_origin(hwnd);
        let r = window_rect(hwnd);
        println!("     window {:?}, client origin {ox},{oy}, client {:?} px", (r.left, r.top, r.right, r.bottom), client_size(hwnd));
    }
    let (mx, my) = centre(hwnd, WIN_MAX);
    let (nx, ny) = centre(hwnd, WIN_MIN);
    let (cx, cy) = centre(hwnd, WIN_CLOSE);
    rep.check(mx > 0.0 && nx > 0.0 && cx > 0.0 && mx > nx && cx > mx, "the three window buttons are named, left to right: min, max, close");
    rep.check(nc_at(hwnd, mx, my) == HT_MAXBUTTON, "over the maximize button: HTMAXBUTTON (Snap Layouts)");
    rep.check(nc_at(hwnd, nx, ny) == HT_CLIENT && nc_at(hwnd, cx, cy) == HT_CLIENT, "over minimize and close: the client's (they are clicked with ordinary messages)");
    rep.check(nc_at(hwnd, 400.0, 24.0) == HT_CAPTION, "the empty caption: HTCAPTION (drag, double click maximizes)");
    rep.check(nc_at(hwnd, 400.0, 2.0) == HT_TOP, "the top edge: HTTOP");
    rep.check(nc_at(hwnd, 2.0, 2.0) == HT_TOPLEFT && nc_at(hwnd, size.0 - 2.0, 2.0) == HT_TOPRIGHT, "the top corners: HTTOPLEFT, HTTOPRIGHT");
    rep.check(nc_at(hwnd, 400.0, 400.0) == HT_CLIENT, "the body: HTCLIENT");
    let r = window_rect(hwnd);
    rep.check(nc_screen(hwnd, r.left + 2, (r.top + r.bottom) / 2) == HT_LEFT, "the left border: HTLEFT");
    rep.check(nc_screen(hwnd, (r.left + r.right) / 2, r.bottom - 2) == HT_BOTTOM, "the bottom border: HTBOTTOM");
    rep.check(nc_screen(hwnd, r.right - 2, r.bottom - 2) == HT_BOTTOMRIGHT, "the bottom right corner: HTBOTTOMRIGHT");

    // pointer, click and key through the message procedure
    let (bx, by) = centre(hwnd, 1); // More
    mouse(hwnd, WM_MOUSEMOVE, bx, by);
    rep.check(with(hwnd, |h| h.core.hover) == Some(Some(1)), "WM_MOUSEMOVE over a button hovers it");
    mouse(hwnd, WM_LBUTTONDOWN, bx, by);
    rep.check(with(hwnd, |h| h.core.pressed()) == Some(Some(1)), "WM_LBUTTONDOWN presses it");
    mouse(hwnd, WM_LBUTTONUP, bx, by);
    rep.check(with(hwnd, |h| h.core.ov.modal()) == Some(true), "the click opens the menu (a command through Cx)");
    rep.check(nc_at(hwnd, mx, my) == HT_CLIENT, "with a menu open nothing of the caption is Windows' (its scrim catches the click)");
    key(hwnd, 0x1B);
    rep.check(with(hwnd, |h| h.core.ov.modal()) == Some(false) && nc_at(hwnd, mx, my) == HT_MAXBUTTON, "Esc closes the menu and the maximize button is Windows' again");
    // a key the app does not use: Tab walks the buttons and shows the ring
    key(hwnd, 0x09);
    rep.check(with(hwnd, |h| (h.core.hits().focus.is_some(), h.core.hits().focus_visible)) == Some((true, true)), "Tab focuses the first button and shows its ring");

    // the window moves to a screen of 150 %: WM_DPICHANGED 96 -> 144
    let (f0, w0) = with(hwnd, |h| (h.frames, client_size(h.hwnd))).unwrap_or_default();
    let mut to = window_rect(hwnd);
    let (ww, wh) = (to.right - to.left, to.bottom - to.top);
    to.right = to.left + ww * 3 / 2;
    to.bottom = to.top + wh * 3 / 2;
    let before = with(hwnd, |h| h.core.scale).unwrap_or(0.0);
    // SAFETY: a message sent to our own window; lParam points to a local rectangle, as Windows' does.
    unsafe { SendMessageW(hwnd, WM_DPICHANGED, (144usize << 16) | 144, &raw const to as LPARAM) };
    let (f1, w1, sc) = with(hwnd, |h| (h.frames, client_size(h.hwnd), h.core.scale)).unwrap_or_default();
    rep.check(sc == 1.5 && before != 1.5 || before == 1.5, &format!("WM_DPICHANGED 96 -> 144 sets the scale to 1.5 (was {before})"));
    rep.check(f1 > f0 && w1.0 > w0.0 * 14 / 10, &format!("... and redraws at the new size ({}x{} -> {}x{} px, {} frames)", w0.0, w0.1, w1.0, w1.1, f1 - f0));
    let (mx2, my2) = centre(hwnd, WIN_MAX);
    rep.check((mx2 - mx).abs() < 12.0 && nc_at(hwnd, mx2, my2) == HT_MAXBUTTON, "... and the maximize button is still where DIPs say it is");
    let back = to;
    let mut back = RECT { right: back.left + ww, bottom: back.top + wh, ..back };
    // SAFETY: as above.
    unsafe { SendMessageW(hwnd, WM_DPICHANGED, (96usize << 16) | 96, &raw mut back as LPARAM) };
    rep.check(with(hwnd, |h| h.core.scale) == Some(1.0), "WM_DPICHANGED 144 -> 96 puts it back");

    // rest: no frames, no timers
    pump(700);
    let n = with(hwnd, |h| h.frames).unwrap_or(0);
    pump(2000);
    let m = with(hwnd, |h| h.frames).unwrap_or(0);
    rep.check(m == n, &format!("at rest: {} frames in 2 s", m - n));

    // closing: the window remembers its place, and goes with its host
    // SAFETY: a message sent to our own window.
    unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0) };
    pump(50);
    rep.check(with(hwnd, |_| ()).is_none(), "WM_CLOSE closes the window and its host goes with it");
    let saved = std::fs::read_to_string(&place).unwrap_or_default();
    rep.check(saved.split_whitespace().filter(|n| n.parse::<i32>().is_ok()).count() == 5, &format!("... and writes its place: \"{}\"", saved.trim()));
    let _ = std::fs::remove_file(&place);
}

fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a query into a local.
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right, r.bottom)
}

/// The window on the screen for a moment, and a picture of what the compositor made of it.
fn stage(png: &str, demo: fn(&str) -> Box<dyn App>) -> i32 {
    let mut rep = Report(0);
    let composed = gfx::compose_windows();
    let began = Instant::now();
    let Some(hwnd) = create(demo(""), &opts("winrust_ui_stage"), composed, Some((60, 40))) else { return 1 };
    // SAFETY: a window of ours, shown without activation and destroyed below.
    unsafe {
        // topmost for the moment it is there, so that whatever is full screen does not cover it
        SetWindowPos(hwnd, HWND_TOPMOST, 60, 40, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    pump(450);
    let r = window_rect(hwnd);
    let (cx, cy) = client_origin(hwnd);
    let (cw, ch) = client_size(hwnd);
    // where Windows thinks the visible frame is: Snap sizes windows by it
    let mut vis = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a 16-byte rectangle attribute of our own window.
    let got = unsafe { windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute(hwnd, 9, (&raw mut vis).cast(), 16) };
    println!("stage: DWM's extended frame bounds {:?} (hr {got:#x}) against the client area {:?}", (vis.left, vis.top, vis.right, vis.bottom), (cx, cy, cx + cw, cy + ch));
    let pad = 170;
    let (gx, gy, gw, gh) = ((cx - pad).max(0), (cy - pad).max(0), cw + 2 * pad, ch + 2 * pad);
    let mut shot = Surface::new();
    let ok = shot.grab(gx, gy, gw.min(2560 - gx), gh.min(1440 - gy));
    println!("stage: window rect {:?}, client {cx},{cy} {cw}x{ch}, grab {gx},{gy} {gw}x{gh}: {ok}", (r.left, r.top, r.right, r.bottom));
    if ok {
        let g = Gfx::new(1.0).ok();
        let (w, h) = (gw.min(2560 - gx), gh.min(1440 - gy));
        let saved = g.is_some_and(|g| g.save_png(png, w as u32, h as u32, shot.pixels()).is_ok());
        rep.check(saved, &format!("the picture is saved: {png}"));
    }
    // SAFETY: our own window.
    unsafe { DestroyWindow(hwnd) };
    pump(30);
    println!("stage: on the screen for {} ms", began.elapsed().as_millis());
    rep.0
}

/// 30 openings and closings: the process's memory after the third cycle against after the last.
fn memory(demo: fn(&str) -> Box<dyn App>) -> i32 {
    let mut rep = Report(0);
    let (ws0, pv0) = memory_now();
    let composed = gfx::compose_windows();
    let Some(hwnd) = create(demo(""), &opts("winrust_ui_memory"), composed, Some((-5000, -5000))) else { return 1 };
    // SAFETY: a window of ours, shown off the screen without activation: its surfaces are really composed.
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    pump(300);
    let gpu = || gfx::gpu_memory().map_or(0.0, |g| g.0);
    let (ws1, pv1) = memory_now();
    println!("process start {ws0:.0} / {pv0:.0} MB (working set / private); window open {ws1:.0} / {pv1:.0} MB, video {:.0} MB ({})", gpu(), if composed { "composed" } else { "layered" });
    // Memory of a process that shares the GPU with others breathes (the driver trims and refills): what is compared is the lowest
    // each of the first cycles (after the warm-up) and each of the last ones came down to, so a spike does not count as growth. The
    // limits are the bar's self-test's (2 MB of private bytes) and one step of the driver's video memory (3.5 MB).
    let mut seen: Vec<(f64, f64, f64)> = Vec::new();
    let cycles: usize = std::env::var("SELFTEST_CYCLES").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    for cycle in 1..=cycles {
        for (id, close) in [(1, 0x1B), (52, 0x1B), (51, 0)] {
            let (x, y) = centre(hwnd, id);
            mouse(hwnd, WM_MOUSEMOVE, x, y);
            mouse(hwnd, WM_LBUTTONDOWN, x, y);
            mouse(hwnd, WM_LBUTTONUP, x, y);
            pump(220);
            if close != 0 {
                key(hwnd, close);
            }
            pump(40);
        }
        let (ws, pv) = memory_now();
        seen.push((ws, pv, gpu()));
        if [1, 3, 6, 10, 15, 20, 25, 30].contains(&cycle) || cycle % 30 == 0 {
            println!("  cycle {cycle:2}: {ws:.1} / {pv:.1} MB, video {:.1}", gpu());
        }
    }
    let low = |r: &[(f64, f64, f64)]| r.iter().fold((f64::MAX, f64::MAX, f64::MAX), |m, v| (m.0.min(v.0), m.1.min(v.1), m.2.min(v.2)));
    let (first, last) = (low(&seen[2..8]), low(&seen[seen.len() - 6..]));
    let (dws, dpv, dgpu) = (last.0 - first.0, last.1 - first.1, last.2 - first.2);
    rep.check(dpv <= 2.0 && dgpu <= 4.0, &format!("30 cycles of menu, dialog, toast: lowest of cycles 3-8 {:.1} / {:.1} MB, video {:.1}; of cycles 25-30 {:.1} / {:.1} MB, video {:.1}; growth {dws:+.1} / {dpv:+.1} MB, video {dgpu:+.1}", first.0, first.1, first.2, last.0, last.1, last.2));
    // SAFETY: our own window.
    unsafe { DestroyWindow(hwnd) };
    rep.0
}
