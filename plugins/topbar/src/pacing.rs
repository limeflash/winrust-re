//! One compositor-paced worker for all animating windows. At most one message is outstanding per window.
use std::{collections::HashMap, sync::{Mutex, OnceLock}, thread, time::{Duration, Instant}};
use windows_sys::Win32::{Graphics::Dwm::DwmFlush, UI::WindowsAndMessaging::{PostMessageW, WM_APP}};

pub const FRAME: u32 = WM_APP + 100;

#[derive(Default)]
struct State { windows: HashMap<isize, bool>, running: bool }
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
fn state() -> &'static Mutex<State> { STATE.get_or_init(|| Mutex::new(State::default())) }

/// Register or re-arm a window. Called on the window's UI thread after its frame.
pub fn set(hwnd: isize, on: bool) {
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    if on {
        s.windows.entry(hwnd).or_insert(false);
        if !s.running {
            s.running = true;
            thread::spawn(run);
        }
    } else {
        s.windows.remove(&hwnd);
    }
}

/// Ignore stale messages after an animation has stopped.
pub fn received(hwnd: isize) -> bool {
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pending) = s.windows.get_mut(&hwnd) { *pending = false; true } else { false }
}

fn run() {
    // A short wait is normal: called late in a frame, DwmFlush returns at the next vblank, which at 360 Hz is under 1 ms
    // away. Only an error, or several returns in a row with no wait at all (composition off: RDP, a broken driver), means
    // DwmFlush is not pacing anything; then fall back to a short sleep so the thread cannot spin.
    let mut instant = 0u32;
    loop {
        let start = Instant::now();
        // SAFETY: DwmFlush takes no arguments; it only blocks until the compositor's next frame.
        let result = unsafe { DwmFlush() };
        instant = if start.elapsed() < Duration::from_micros(100) { instant + 1 } else { 0 };
        if result < 0 || instant >= 3 {
            thread::sleep(Duration::from_millis(4));
        }
        let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
        if s.windows.is_empty() { s.running = false; return; }
        s.windows.retain(|&hwnd, pending| {
            if !*pending {
                *pending = true;
                unsafe { PostMessageW(hwnd as _, FRAME, 0, 0) != 0 }
            } else { true }
        });
    }
}
