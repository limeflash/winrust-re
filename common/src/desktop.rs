//! Win+D without Explorer: minimize every app window, press again to bring the same windows back.
//! Used by reshell (keyboard hook) and rs-shell (when it is the shell).
use std::sync::Mutex;
use windows_sys::{
    core::BOOL,
    Win32::{Foundation::{HWND, LPARAM}, Graphics::Dwm::DwmGetWindowAttribute, UI::WindowsAndMessaging::*},
};

static HIDDEN: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// What one press does, from what is on screen now and what the last press put away (a pure function, so it can be tested).
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Minimize these (the visible app windows) and remember exactly them.
    Away(Vec<isize>),
    /// Bring back these: the remembered ones that are still there and still minimized.
    Back(Vec<isize>),
    Nothing,
}

/// Anything open on screen means "minimize": after a window was brought back by hand the old list must not turn the next
/// press into a silent "restore". With nothing open the remembered windows come back, and only those still minimized (a
/// window that is gone, or that was restored by hand, is left alone: `SW_RESTORE` would un-maximize it).
pub fn plan(visible: &[isize], remembered: &[isize], minimized: impl Fn(isize) -> bool) -> Plan {
    if !visible.is_empty() {
        return Plan::Away(visible.to_vec());
    }
    let back: Vec<isize> = remembered.iter().copied().filter(|&h| minimized(h)).collect();
    if back.is_empty() { Plan::Nothing } else { Plan::Back(back) }
}

fn is_minimized(h: isize) -> bool {
    unsafe { IsWindow(h as HWND) != 0 && IsIconic(h as HWND) != 0 }
}

fn visible_windows() -> Vec<isize> {
    let mut v: Vec<isize> = vec![];
    unsafe { EnumWindows(Some(collect), &mut v as *mut _ as LPARAM) };
    v
}

pub fn toggle() {
    let mut hid = HIDDEN.lock().unwrap();
    match plan(&visible_windows(), &hid, is_minimized) {
        // ShowWindowAsync: ShowWindow on another process's window waits for its message loop, and one hung app would freeze the caller
        Plan::Away(v) => {
            v.iter().for_each(|&h| unsafe { ShowWindowAsync(h as HWND, SW_MINIMIZE); });
            *hid = v;
        }
        Plan::Back(v) => {
            v.iter().for_each(|&h| unsafe { ShowWindowAsync(h as HWND, SW_RESTORE); });
            hid.clear();
        }
        Plan::Nothing => hid.clear(),
    }
}

/// True while the windows the last press put away are still minimized: what the bar's "Show desktop" button shows as pressed.
/// (The caller adds "and nothing else is open": a window opened since means the next press minimizes, not restores.)
pub fn away() -> bool {
    HIDDEN.lock().unwrap().iter().any(|&h| is_minimized(h))
}

unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
    unsafe {
        let mut cloaked = 0u32; // DWMWA_CLOAKED: UWP ghosts that are "visible" but not on screen
        DwmGetWindowAttribute(h, 14, &mut cloaked as *mut _ as _, 4);
        let tool = GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0; // topbar, desktop, tray hosts
        if IsWindowVisible(h) != 0 && IsIconic(h) == 0 && !tool && cloaked == 0 && GetWindow(h, GW_OWNER).is_null() && GetWindowTextLengthW(h) > 0 && !is_desktop(h) {
            (*(lp as *mut Vec<isize>)).push(h as isize);
        }
        1
    }
}

/// The desktop itself (Explorer's, or the rust-explorer one, which also registers as "Progman") must never be minimized.
fn is_desktop(h: HWND) -> bool {
    let mut cls = [0u16; 32];
    let n = unsafe { GetClassNameW(h, cls.as_mut_ptr(), cls.len() as i32) } as usize;
    is_desktop_class(&String::from_utf16_lossy(&cls[..n]))
}

fn is_desktop_class(c: &str) -> bool {
    matches!(c, "Progman" | "WorkerW")
}

#[cfg(test)]
mod tests {
    use super::{Plan, plan};

    #[test]
    fn desktop_classes() {
        assert!(super::is_desktop_class("Progman"));
        assert!(!super::is_desktop_class("Notepad"));
    }

    #[test]
    fn a_press_with_windows_open_minimizes_exactly_those_and_forgets_the_old_list() {
        assert_eq!(plan(&[1, 2], &[7, 8], |_| true), Plan::Away(vec![1, 2]));
    }

    #[test]
    fn the_next_press_restores_only_the_remembered_windows_that_are_still_minimized() {
        let minimized = |h: isize| h != 3; // 3 was brought back by hand (or is gone)
        assert_eq!(plan(&[], &[1, 3, 2], minimized), Plan::Back(vec![1, 2]));
        assert_eq!(plan(&[], &[3], minimized), Plan::Nothing);
        assert_eq!(plan(&[], &[], |_| true), Plan::Nothing);
    }
}
