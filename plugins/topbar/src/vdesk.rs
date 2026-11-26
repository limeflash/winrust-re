//! topbar: virtual desktops, as far as Windows lets a program see them without its private interfaces.
//! (split out of main.rs; shares its items through `super::*`)
//!
//! `IVirtualDesktopManager` (the documented one) says which desktop a window is on; it cannot list, create or switch desktops,
//! and while our bar is the shell it answers "class not registered" (it looks for the task band of the first `Shell_TrayWnd`,
//! which is ours). The list and the current desktop are in the registry where Explorer keeps them (`...\Explorer\VirtualDesktops`:
//! `VirtualDesktopIDs`, `CurrentVirtualDesktop`, and per desktop a `Name` and a `Wallpaper`); the windows of the other desktops
//! are the ones DWM cloaks for the shell; switching is what Explorer's own shortcuts do (Win+Ctrl+Left/Right, Win+Ctrl+D), which
//! we press. Without Explorer (rs-shell) there are no virtual desktops at all: the list is one desktop, nothing can be switched.
use super::*;
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
use windows_sys::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW};

const VD_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops";

/// A GUID as the registry and `GetWindowDesktopId` hold it: Data1..3 little endian, then 8 bytes.
pub(crate) type Id = [u8; 16];

pub(crate) struct Desktop {
    pub id: Id,
    pub name: String,
    /// The wallpaper file of this desktop, "" if it has none of its own.
    pub wallpaper: String,
}

thread_local! {
    static MGR: RefCell<Option<IVirtualDesktopManager>> = const { RefCell::new(None) };
}

fn manager() -> Option<IVirtualDesktopManager> {
    MGR.with_borrow_mut(|m| {
        if m.is_none() {
            // SAFETY: COM on the thread that initialised it (the bar's); the manager is served by Explorer.
            *m = unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL).ok() };
        }
        m.clone()
    })
}

unsafe fn reg_value(path: &str, name: &str) -> Option<Vec<u8>> {
    let mut key: HKEY = null_mut();
    if RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, KEY_READ, &mut key) != 0 {
        return None;
    }
    let mut len = 0u32;
    let n = wide(name);
    let mut out = None;
    if RegQueryValueExW(key, n.as_ptr(), null(), null_mut(), null_mut(), &mut len) == 0 && len > 0 {
        let mut buf = vec![0u8; len as usize];
        if RegQueryValueExW(key, n.as_ptr(), null(), null_mut(), buf.as_mut_ptr(), &mut len) == 0 {
            buf.truncate(len as usize);
            out = Some(buf);
        }
    }
    RegCloseKey(key);
    out
}

unsafe fn reg_string(path: &str, name: &str) -> String {
    let Some(b) = reg_value(path, name) else { return String::new() };
    let w: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    wstr_of(&w)
}

fn guid_text(id: &Id) -> String {
    let (d1, d2, d3) = (u32::from_le_bytes([id[0], id[1], id[2], id[3]]), u16::from_le_bytes([id[4], id[5]]), u16::from_le_bytes([id[6], id[7]]));
    format!("{{{d1:08X}-{d2:04X}-{d3:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}", id[8], id[9], id[10], id[11], id[12], id[13], id[14], id[15])
}

/// "Desktop 2" in the user's language.
fn default_name(n: usize) -> String {
    tf!("Рабочий стол {}", "Desktop {}", ja: "デスクトップ {}", pt: "Área de Trabalho {}", zh: "桌面 {}", n)
}

/// The wallpaper file the system shows (the one a desktop without its own uses).
pub(crate) unsafe fn system_wallpaper() -> String {
    let mut b = [0u16; 520];
    SystemParametersInfoW(SPI_GETDESKWALLPAPER, 520, b.as_mut_ptr().cast(), 0);
    wstr_of(&b)
}

/// The desktops in order and the index of the current one. One unnamed desktop when Windows keeps no list.
pub(crate) unsafe fn desktops() -> (Vec<Desktop>, usize) {
    let ids: Vec<Id> = reg_value(VD_KEY, "VirtualDesktopIDs").map(|b| b.as_chunks::<16>().0.to_vec()).unwrap_or_default();
    if ids.is_empty() || !EXPLORER_UP.load(Relaxed) {
        return (vec![Desktop { id: [0; 16], name: default_name(1), wallpaper: system_wallpaper() }], 0);
    }
    let cur = reg_value(VD_KEY, "CurrentVirtualDesktop").and_then(|b| <Id>::try_from(b.as_slice()).ok());
    let system = system_wallpaper();
    let list = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let key = format!(r"{VD_KEY}\Desktops\{}", guid_text(id));
            let name = reg_string(&key, "Name");
            let wallpaper = Some(reg_string(&key, "Wallpaper")).filter(|w| !w.is_empty()).unwrap_or_else(|| system.clone());
            Desktop { id: *id, name: if name.is_empty() { default_name(i + 1) } else { name }, wallpaper }
        })
        .collect();
    // the desktop the window in front is on is the current one (the registry value follows Explorer's own bookkeeping)
    let by_window = desktop_of(GetForegroundWindow()).and_then(|d| ids.iter().position(|i| *i == d));
    (list, by_window.or_else(|| cur.and_then(|c| ids.iter().position(|i| *i == c))).unwrap_or(0))
}

/// The desktop a window is on (None where Windows cannot say).
pub(crate) unsafe fn desktop_of(w: HWND) -> Option<Id> {
    let g = manager()?.GetWindowDesktopId(whwnd(w)).ok()?;
    let mut id = [0u8; 16];
    id[..4].copy_from_slice(&g.data1.to_le_bytes());
    id[4..6].copy_from_slice(&g.data2.to_le_bytes());
    id[6..8].copy_from_slice(&g.data3.to_le_bytes());
    id[8..].copy_from_slice(&g.data4);
    Some(id)
}

/// Every window that sits on a desktop's surface, as rectangles in fractions of the screen, back to front, one list per desktop
/// (`cur` is the current one): for the miniatures of Task view. The windows DWM cloaks for the shell are on another desktop:
/// Windows names which one when it answers, and with two desktops it can only be the other one.
pub(crate) unsafe fn miniatures(desks: &[Desktop], cur: usize) -> Vec<Vec<(f32, f32, f32, f32)>> {
    struct Acc(Vec<HWND>);
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> i32 {
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if IsWindowVisible(h) != 0 && IsIconic(h) == 0 && GetWindow(h, GW_OWNER).is_null() && GetWindowTextLengthW(h) > 0 && (ex & WS_EX_TOOLWINDOW == 0 || ex & WS_EX_APPWINDOW != 0) {
            (*(lp as *mut Acc)).0.push(h);
        }
        1
    }
    let mut acc = Acc(Vec::new());
    EnumWindows(Some(each), &mut acc as *mut _ as LPARAM);
    let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN) as f32, GetSystemMetrics(SM_CYSCREEN) as f32);
    let mut out = vec![Vec::new(); desks.len()];
    for &h in acc.0.iter().rev() {
        let (l, t, r, b) = ui::live::frame_of(h as isize);
        if r - l < 40 || b - t < 40 {
            continue;
        }
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(h, 14, &mut cloaked as *mut _ as *mut _, 4); // DWMWA_CLOAKED
        let at = if cloaked & 2 == 0 {
            (cloaked == 0).then_some(cur)
        } else {
            desktop_of(h).and_then(|d| desks.iter().position(|x| x.id == d)).or_else(|| (desks.len() == 2).then_some(1 - cur.min(1)))
        };
        if let Some(i) = at.filter(|&i| i < out.len()) {
            let f = |v: i32, m: f32| (v as f32 / m).clamp(0.0, 1.0);
            let (x0, y0, x1, y1) = (f(l, sw), f(t, sh), f(r, sw), f(b, sh));
            out[i].push((x0, y0, x1 - x0, y1 - y0));
        }
    }
    out
}

thread_local! {
    /// Presses of Win+Ctrl+Arrow still to send (+ right, - left); timer 11 sends one per tick.
    static STEPS: Cell<i32> = const { Cell::new(0) };
}

/// Switches to desktop `to` from `from` the way the shortcut does, one press per tick of timer 11.
pub(crate) unsafe fn go(from: usize, to: usize) {
    STEPS.set(to as i32 - from as i32);
    SetTimer(BAR.load(Relaxed) as HWND, 11, 90, None);
}

/// Timer 11: the next press of the switch.
pub(crate) unsafe fn step(bar: HWND) {
    let n = STEPS.get();
    if n == 0 {
        KillTimer(bar, 11);
        return;
    }
    STEPS.set(n - n.signum());
    keys(&[VK_LWIN, 0x11, if n > 0 { 0x27 } else { 0x25 }]); // Win+Ctrl+Right / Left
}

/// A new desktop, and Windows switches to it (Win+Ctrl+D).
pub(crate) unsafe fn add() {
    keys(&[VK_LWIN, 0x11, 0x44]);
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_list_has_a_current_desktop_and_windows_have_one() {
        unsafe {
            let _ = super::CoInitializeEx(None, super::COINIT_APARTMENTTHREADED);
            let (list, cur) = super::desktops();
            assert!(!list.is_empty() && cur < list.len());
            println!("{} desktops, current {}: {:?}", list.len(), cur, list.iter().map(|d| (&d.name, &d.wallpaper)).collect::<Vec<_>>());
            let mini = super::miniatures(&list, cur);
            println!("miniatures per desktop: {:?}", mini.iter().map(Vec::len).collect::<Vec<_>>());
            assert_eq!(mini.len(), list.len());
        }
    }

    #[test]
    fn guids_are_written_the_way_the_registry_names_them() {
        let id = [0x33, 0xD8, 0x47, 0x6D, 0xB8, 0xAD, 0x72, 0x4B, 0xBD, 0x6D, 0xDD, 0x27, 0xCC, 0xE9, 0x81, 0x14];
        assert_eq!(super::guid_text(&id), "{6D47D833-ADB8-4B72-BD6D-DD27CCE98114}");
    }
}
