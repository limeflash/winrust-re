//! The tray icon and the hotkey: a hidden window of ours that owns them. A click on the icon, a choice in its menu and the hotkey
//! come to the main window as messages (`MSG_TRAY_SHOW`, `MSG_TRAY_CMD`, `MSG_HOTKEY`), which `App::message` answers. The menu is
//! a system one, because the main window may be hidden when it is wanted.
//!
//! A [`Tray`] is made only in a real, normal run: a render, a benchmark or a self-test never calls [`Tray::create`] with the icon
//! (the self-test of the messages makes the window without it, see [`Tray::create`]), so no stray icon ever lands on the user's bar.
use std::{
    ptr::{null, null_mut},
    sync::{Mutex, PoisonError},
};

use winapi::{
    shared::{
        minwindef::{LPARAM, LRESULT, UINT, WPARAM},
        windef::{HICON, HWND, POINT},
    },
    um::{
        libloaderapi::{GetModuleHandleW, GetProcAddress, LoadLibraryW},
        shellapi::{NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW},
        winuser::{
            AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos, GetWindowLongPtrW, IMAGE_ICON, LoadImageW,
            MF_CHECKED, MF_SEPARATOR, MF_STRING, PostMessageW, RegisterClassW, RegisterHotKey, RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, UnregisterHotKey, WM_APP, WM_CONTEXTMENU, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
            GWLP_USERDATA,
        },
    },
};

use crate::backend::{MSG_HOTKEY, MSG_TRAY_CMD, MSG_TRAY_SHOW, TrayItem};

pub const HELPER_CLASS: &str = "WinRustTaskMgrTray";
/// What the shell sends to the helper window about its icon.
pub const CALLBACK: UINT = WM_APP + 60;
const MOD_CONTROL_ALT: UINT = 0x0002 | 0x0001; // MOD_CONTROL | MOD_ALT
const HOTKEY_ID: i32 = 1;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// What the helper window's procedure knows: the window to tell, the menu, the "taskbar was made again" message.
struct Shared {
    main: isize,
    items: Mutex<Vec<TrayItem>>,
    icon: bool,
    created: UINT,
}

pub struct Tray {
    hwnd: HWND,
    shared: *mut Shared,
    icon: bool,
}

// SAFETY: the handle and the pointer are only used on the thread that made the window (the UI thread); `Real` keeps the tray in a
// mutex that is only taken there.
unsafe impl Send for Tray {}

impl Tray {
    /// The helper window of `main`; with `icon` also the icon in the tray and the hotkey Ctrl+Alt+T. Without it nothing of the
    /// user's desktop is touched: the window only answers the messages it is sent (the self-test sends them).
    pub fn create(main: isize, icon: bool) -> Option<Tray> {
        // SAFETY: a class and a hidden window of ours; the pointer is freed in `Drop`.
        unsafe {
            let hi = GetModuleHandleW(null());
            let class = wide(HELPER_CLASS);
            let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: hi, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc); // a second window of the class finds it registered already
            let created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
            let shared = Box::into_raw(Box::new(Shared { main, items: Mutex::new(Vec::new()), icon, created }));
            let hwnd = CreateWindowExW(0, class.as_ptr(), null(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), hi, null_mut());
            if hwnd.is_null() {
                drop(Box::from_raw(shared));
                return None;
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, shared as isize);
            if icon {
                add_icon(hwnd);
                RegisterHotKey(hwnd, HOTKEY_ID, MOD_CONTROL_ALT, u32::from(b'T'));
            }
            Some(Tray { hwnd, shared, icon })
        }
    }

    /// The window the shell and the hotkey talk to (the self-test sends it their messages by hand).
    #[must_use]
    pub fn hwnd(&self) -> isize {
        self.hwnd as isize
    }

    /// What the menu shows the next time it opens.
    pub fn set_menu(&self, items: Vec<TrayItem>) {
        // SAFETY: `shared` lives until `Drop`.
        *unsafe { &*self.shared }.items.lock().unwrap_or_else(PoisonError::into_inner) = items;
    }

    /// A balloon from the icon (what a rule said).
    pub fn balloon(&self, title: &str, msg: &str) {
        if !self.icon {
            return;
        }
        // SAFETY: a call with a structure of ours.
        unsafe {
            let mut nid = base(self.hwnd);
            nid.uFlags = NIF_INFO;
            fill(&mut nid.szInfoTitle, title);
            fill(&mut nid.szInfo, msg);
            Shell_NotifyIconW(NIM_MODIFY, &mut nid);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: our own window, icon and hotkey; the pointer was made in `create`.
        unsafe {
            if self.icon {
                let mut nid = base(self.hwnd);
                Shell_NotifyIconW(NIM_DELETE, &mut nid);
                UnregisterHotKey(self.hwnd, HOTKEY_ID);
            }
            DestroyWindow(self.hwnd);
            drop(Box::from_raw(self.shared));
        }
    }
}

fn fill<const N: usize>(to: &mut [u16; N], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(N - 1).collect();
    to[..w.len()].copy_from_slice(&w);
    to[w.len()] = 0;
}

fn base(hwnd: HWND) -> NOTIFYICONDATAW {
    // SAFETY: a plain structure of integers and arrays.
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

/// Puts the icon in the tray (and again when the taskbar is made anew).
unsafe fn add_icon(hwnd: HWND) {
    // SAFETY: calls with structures and handles of ours.
    unsafe {
        let hi = GetModuleHandleW(null());
        let icon = LoadImageW(hi, std::ptr::without_provenance::<u16>(1), IMAGE_ICON, 16, 16, 0) as HICON;
        let mut nid = base(hwnd);
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = CALLBACK;
        nid.hIcon = icon;
        fill(&mut nid.szTip, t!("Диспетчер задач (Rust)", "Task Manager (Rust)", ja: "タスク マネージャー (Rust)", pt: "Gerenciador de Tarefas (Rust)", zh: "任务管理器（Rust）"));
        Shell_NotifyIconW(NIM_ADD, &mut nid);
    }
}

/// Dark or light system menus (the undocumented functions of uxtheme: 135 SetPreferredAppMode, 136 FlushMenuThemes).
fn set_menu_theme(dark: bool) {
    // SAFETY: the two ordinals are what they are on every Windows 10 1903+ and 11; a missing one is skipped.
    unsafe {
        let lib = LoadLibraryW(wide("uxtheme.dll").as_ptr());
        if lib.is_null() {
            return;
        }
        let set = GetProcAddress(lib, 135 as *const i8);
        let flush = GetProcAddress(lib, 136 as *const i8);
        if !set.is_null() {
            let f: extern "system" fn(i32) -> i32 = std::mem::transmute(set);
            f(if dark { 2 } else { 3 }); // ForceDark / ForceLight
        }
        if !flush.is_null() {
            let f: extern "system" fn() = std::mem::transmute(flush);
            f();
        }
    }
}

/// The menu at the pointer; the code of the row chosen.
unsafe fn popup(hwnd: HWND, items: &[TrayItem]) -> Option<usize> {
    // SAFETY: a menu made, shown and destroyed here; the strings live for the calls.
    unsafe {
        if items.is_empty() {
            return None;
        }
        set_menu_theme(!winrust_common::style::light());
        let m = CreatePopupMenu();
        for (i, it) in items.iter().enumerate() {
            if it.sep {
                AppendMenuW(m, MF_SEPARATOR, 0, null());
            } else {
                AppendMenuW(m, MF_STRING | if it.checked { MF_CHECKED } else { 0 }, i + 1, wide(&it.label).as_ptr());
            }
        }
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        SetForegroundWindow(hwnd);
        let id = TrackPopupMenuEx(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, p.x, p.y, hwnd, null_mut());
        DestroyMenu(m);
        (id > 0).then(|| items[id as usize - 1].code)
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: the pointer was stored by `create` and is freed after the window is destroyed.
    unsafe {
        let shared = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Shared;
        if shared.is_null() {
            return DefWindowProcW(hwnd, msg, wp, lp);
        }
        let s = &*shared;
        let tell = |m: u32, wp: usize| {
            PostMessageW(s.main as HWND, m, wp, 0);
        };
        match msg {
            CALLBACK => match (lp & 0xffff) as UINT {
                WM_LBUTTONUP => tell(MSG_TRAY_SHOW, 0),
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    let items = s.items.lock().unwrap_or_else(PoisonError::into_inner).clone();
                    if let Some(code) = popup(hwnd, &items) {
                        tell(MSG_TRAY_CMD, code);
                    }
                }
                _ => {}
            },
            WM_HOTKEY if wp as i32 == HOTKEY_ID => tell(MSG_HOTKEY, 0),
            WM_DESTROY => {}
            m if m == s.created && s.icon => add_icon(hwnd),
            _ => return DefWindowProcW(hwnd, msg, wp, lp),
        }
        0
    }
}
