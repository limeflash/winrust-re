//! Bringing our own popup (Start, the search card) to the front with the keyboard focus. Windows refuses the foreground to a
//! process that did not get the last input: the popup then shows, but typing goes to the old window (#72). Used by topbar
//! and rs-launcher; call it on the thread that owns `h`.
use windows_sys::Win32::{
    Foundation::HWND,
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::{INPUT, INPUT_KEYBOARD, KEYEVENTF_KEYUP, SendInput, SetFocus, VK_MENU},
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow},
    },
};

// HWND is an opaque handle passed to Win32, not dereferenced here.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn force(h: HWND) {
    unsafe {
        // a click on our own window already gives us the right; attaching while its button is down would hand it back
        if SetForegroundWindow(h) != 0 && GetForegroundWindow() == h {
            SetFocus(h);
            return;
        }
        // 1) share the input queue of the foreground thread for a moment
        let me = GetCurrentThreadId();
        let ft = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        let attach = ft != 0 && ft != me && AttachThreadInput(me, ft, 1) != 0;
        SetForegroundWindow(h);
        if attach {
            AttachThreadInput(me, ft, 0);
        }
        // 2) nothing to attach to (looking at the desktop: no foreground window) or an elevated window in front (UIPI refuses
        // the attach): Windows lets the foreground change while Alt is down. 0xE8 (unassigned) in between, so the release is
        // not a lone Alt that opens a menu bar.
        if GetForegroundWindow() != h {
            keys(&[(VK_MENU, false), (0xE8, false), (0xE8, true)]);
            SetForegroundWindow(h);
            keys(&[(VK_MENU, true)]);
        }
        SetFocus(h); // detaching drops this thread's keyboard focus; without it the window gets no typing
    }
}

fn keys(k: &[(u16, bool)]) {
    for &(vk, up) in k {
        let mut i: INPUT = unsafe { std::mem::zeroed() };
        i.r#type = INPUT_KEYBOARD;
        i.Anonymous.ki.wVk = vk;
        i.Anonymous.ki.dwFlags = if up { KEYEVENTF_KEYUP } else { 0 };
        unsafe { SendInput(1, &i, size_of::<INPUT>() as i32) };
    }
}
