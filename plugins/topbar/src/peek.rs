//! Peek at the desktop: the pointer rests on the show-desktop button for 550 ms and every app window gives way, so that the
//! desktop shows; leaving the button (or a click) ends it. `Peek` is the timer logic (pure, tested); the rest asks DWM.
//!
//! DWM does this for Windows' own taskbar (Aero Peek) and exposes it only through `dwmapi` export #113, the undocumented
//! `DwmpActivateLivePreview(BOOL on, HWND peek, HWND topmost, UINT kind, void* reserved)` (it has no name in the import
//! library). Measured on 10.0.28000: `on = 1, peek = the desktop window, kind = 1` hides every other window (the bar
//! included unless it is excluded from peek (`DWMWA_EXCLUDED_FROM_PEEK`, set on the bar windows in `bars::create`); `topmost` did nothing); `on = 0` brings them back; the peek ends by itself when the process dies.
//! `kind` 2..4 does nothing to app windows. It is looked up at run time: where it is missing, nothing happens.
use std::ffi::c_void;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

/// How long the pointer rests on the button before the peek starts (the mockup's `setTimeout(..., 550)`).
pub const DELAY_MS: u64 = 550;

/// When the peek starts and when it ends.
#[derive(Debug, Default)]
pub struct Peek {
    /// The moment the pending peek is due.
    due: Option<u64>,
    live: bool,
}

impl Peek {
    /// The pointer came onto the button: a peek is due in 550 ms, unless the windows are away already.
    pub fn enter(&mut self, now: u64, on: bool) {
        self.due = (!on).then_some(now + DELAY_MS);
    }

    /// The timer fired: true when the peek is to start now (nothing changed since: still due, and the windows still not away).
    pub fn tick(&mut self, now: u64, on: bool) -> bool {
        if self.due.is_some_and(|d| now >= d) && !on && !self.live {
            self.due = None;
            self.live = true;
            return true;
        }
        false
    }

    /// The pointer left, or the button was clicked: nothing is due any more. True when a peek was live and is to be ended.
    pub fn stop(&mut self) -> bool {
        self.due = None;
        std::mem::take(&mut self.live)
    }

    #[cfg(test)]
    const fn live(&self) -> bool {
        self.live
    }
}

type Activate = unsafe extern "system" fn(i32, HWND, HWND, u32, *mut c_void) -> i32;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// `DwmpActivateLivePreview`, by ordinal; `None` where dwmapi does not have it.
fn activate_fn() -> Option<Activate> {
    // SAFETY: a lookup by ordinal; the signature is the one measured in the module comment.
    unsafe {
        let m = LoadLibraryW(wide("dwmapi.dll").as_ptr());
        if m.is_null() {
            return None;
        }
        GetProcAddress(m, 113 as *const u8).map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, Activate>(f))
    }
}

/// The desktop window: Explorer's `Progman`, or the one of rust-explorer's desktop (also a `Progman`).
fn desktop() -> HWND {
    // SAFETY: a plain lookup.
    unsafe { FindWindowW(wide("Progman").as_ptr(), std::ptr::null()) }
}

/// Starts the peek with the desktop as what stays. False when it cannot be done (no such export, no desktop window, DWM said no).
pub fn begin(bar: HWND) -> bool {
    let (Some(f), d) = (activate_fn(), desktop()) else { return false };
    // SAFETY: the call of the DWM export with null for the reserved pointer.
    !d.is_null() && unsafe { f(1, d, bar, 1, std::ptr::null_mut()) } >= 0
}

/// Ends the peek: the windows are back.
pub fn end() {
    if let Some(f) = activate_fn() {
        // SAFETY: as above; DWM ignores the windows when switching off.
        unsafe { f(0, desktop(), std::ptr::null_mut(), 1, std::ptr::null_mut()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rest_of_550_ms_starts_the_peek_once() {
        let mut p = Peek::default();
        p.enter(1000, false);
        assert!(!p.tick(1549, false), "not yet");
        assert!(p.tick(1550, false));
        assert!(p.live());
        assert!(!p.tick(1600, false), "it started already");
    }

    #[test]
    fn leaving_or_a_click_before_the_time_cancels_it() {
        let mut p = Peek::default();
        p.enter(0, false);
        assert!(!p.stop(), "nothing was live to end");
        assert!(!p.tick(5000, false), "the timer that fires late finds nothing due");
        p.enter(0, false);
        assert!(!p.stop()); // a click
        assert!(!p.tick(600, false));
    }

    #[test]
    fn a_live_peek_ends_on_leave_and_on_click_and_only_once() {
        let mut p = Peek::default();
        p.enter(0, false);
        assert!(p.tick(550, false));
        assert!(p.stop(), "the windows come back");
        assert!(!p.live());
        assert!(!p.stop(), "once");
        assert!(!p.tick(2000, false), "after a click the same visit does not peek again");
    }

    #[test]
    fn no_peek_while_the_windows_are_away() {
        let mut p = Peek::default();
        p.enter(0, true);
        assert!(!p.tick(550, true));
        // they went away (a click, Win+D) while the timer ran
        p.enter(0, false);
        assert!(!p.tick(550, true));
        assert!(!p.live());
    }
}
