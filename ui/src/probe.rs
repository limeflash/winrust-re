//! What an app's own benchmark and self-test need of the host, which `bench` and `selftest` keep to themselves: a window made off the
//! screen, the app's [`Core`] to drive by hand, a frame on demand, the timings of the frames, and plain Win32 messages to the window.
//! `--bench` and `--selftest` of an app that has screens the generic ones know nothing of (ids, a map, a list in another place) are
//! written on this.

use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, GetClientRect, GetWindowRect, SW_SHOWNOACTIVATE, SendMessageW, ShowWindow};

use crate::core::{App, Core};
pub use crate::window::Options;
use crate::window::{client_origin, create, with};

/// A window of `app` at `at` (a place off the screen, as the benchmarks put them), its first frame drawn, not shown.
#[must_use]
pub fn make(app: Box<dyn App>, o: &Options, composed: bool, at: (i32, i32)) -> Option<isize> {
    create(app, o, composed, Some(at)).map(|h| h as isize)
}

/// Shows it without taking the focus (its surfaces are then really composed; it stays where `make` put it).
pub fn show_off_screen(hwnd: isize) {
    // SAFETY: a window of ours.
    unsafe { ShowWindow(hwnd as HWND, SW_SHOWNOACTIVATE) };
}

pub fn destroy(hwnd: isize) {
    // SAFETY: a window of ours.
    unsafe { DestroyWindow(hwnd as HWND) };
}

/// The window's core: what the app draws and what the pointer is over.
pub fn with_core<R>(hwnd: isize, f: impl FnOnce(&mut Core) -> R) -> Option<R> {
    with(hwnd as HWND, |h| f(&mut h.core))
}

/// One frame now (what a pending redraw asks for).
pub fn frame(hwnd: isize) {
    with(hwnd as HWND, |h| {
        h.core.take_redraw();
        h.frame();
    });
}

/// How many frames the window has drawn.
#[must_use]
pub fn frames(hwnd: isize) -> u32 {
    with(hwnd as HWND, |h| h.frames).unwrap_or(0)
}

/// Starts or stops timing the frames; stopping hands back what each took (milliseconds: encode, end, present).
pub fn timing(hwnd: isize, on: bool) -> Vec<(f64, f64, f64)> {
    with(hwnd as HWND, |h| {
        if on {
            h.spent.clear();
        }
        h.timing = on;
        if on { Vec::new() } else { std::mem::take(&mut h.spent) }
    })
    .unwrap_or_default()
}

/// Dispatches this thread's messages for `ms`.
pub fn pump(ms: u64) {
    crate::selftest::pump(ms);
}

/// The GPU's wait after a frame (what is left to it once the CPU is done).
#[must_use]
pub fn gpu_wait() -> Duration {
    crate::gfx::gpu_wait()
}

/// A message to the window's procedure, as Windows would send it.
pub fn send(hwnd: isize, msg: u32, wp: usize, lp: isize) -> isize {
    // SAFETY: a message to our own window.
    unsafe { SendMessageW(hwnd as HWND, msg, wp, lp as LPARAM) }
}

/// `lparam` of a mouse message at `(x, y)` DIPs.
#[must_use]
pub fn mouse_lparam(hwnd: isize, x: f32, y: f32) -> isize {
    let s = with_core(hwnd, |c| c.scale).unwrap_or(1.0);
    let (px, py) = ((x * s).round() as i32, (y * s).round() as i32);
    (((py as i16 as u16 as u32) << 16) | (px as i16 as u16 as u32)) as isize
}

/// What `WM_NCHITTEST` answers for a point of the client area (DIPs).
#[must_use]
pub fn nc_hit(hwnd: isize, x: f32, y: f32) -> isize {
    let (ox, oy) = client_origin(hwnd as HWND);
    let s = with_core(hwnd, |c| c.scale).unwrap_or(1.0);
    let (px, py) = (ox + (x * s).round() as i32, oy + (y * s).round() as i32);
    send(hwnd, windows_sys::Win32::UI::WindowsAndMessaging::WM_NCHITTEST, 0, (((py as i16 as u16 as u32) << 16) | (px as i16 as u16 as u32)) as isize)
}

#[must_use]
pub fn client_size(hwnd: isize) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a query into a local.
    unsafe { GetClientRect(hwnd as HWND, &mut r) };
    (r.right, r.bottom)
}

#[must_use]
pub fn window_rect(hwnd: isize) -> (i32, i32, i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a query into a local.
    unsafe { GetWindowRect(hwnd as HWND, &mut r) };
    (r.left, r.top, r.right, r.bottom)
}
