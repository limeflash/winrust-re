//! topbar: tray (split out of main.rs; shares its items through `super::*`).
use super::*;

/// First top-level window of the class that is not our own tray host (explorer's real taskbar).
pub(crate) unsafe fn find_other(class: &str) -> HWND {
    let (c, mut h) = (wide(class), null_mut());
    loop {
        h = FindWindowExW(null_mut(), h, c.as_ptr(), null());
        if h.is_null() || h != HOST.get() {
            return h;
        }
    }
}

pub(crate) unsafe fn tray() -> [HWND; 2] {
    [find_other("Shell_TrayWnd"), find_other("Shell_SecondaryTrayWnd")]
}

/// Tells every app to re-register its tray icon with whichever Shell_TrayWnd is found first.
pub(crate) unsafe fn announce() {
    PostMessageW(HWND_BROADCAST, RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()), 0, 0);
}

pub(crate) fn hide_onedrive() -> bool {
    // topbar.cfg sits next to the exe (the launcher writes it); no file or no line = OneDrive hidden
    std::env::current_exe().ok().and_then(|e| std::fs::read_to_string(e.with_file_name("topbar.cfg")).ok()).is_none_or(|c| !c.contains("hide_onedrive=0"))
}

/// Explorer's volume, network and power icons ({7820AE73/74/75-23E3-4229-82C1-E41CB67D5B9C}): the bar draws those itself.
/// Its other icons (printer queue, Safely Remove Hardware, microphone in use) stay (#127).
fn own_icon(guid: &[u8; 16]) -> bool {
    matches!(guid[0], 0x73..=0x75) && guid[1..] == [0xAE, 0x20, 0x78, 0xE3, 0x23, 0x29, 0x42, 0x82, 0xC1, 0xE4, 0x1C, 0xB6, 0x7D, 0x5B, 0x9C]
}

/// WM_COPYDATA (dwData 1) from Shell_NotifyIcon: u32 magic, u32 NIM_*, then the platform-neutral NOTIFYICONDATA
/// (32-bit handles): hWnd@12 uID@16 uFlags@20 callback@24 hIcon@28 dwState@288 mask@292 version@808 guid@944.
pub(crate) unsafe fn tray_msg(d: &[u8]) {
    let u = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let (op, flags, uid) = (u(4), u(20), u(16));
    let h = u(12) as i32 as isize as HWND;
    let guid: [u8; 16] = if flags & 0x20 != 0 { d[944..960].try_into().unwrap() } else { [0; 16] };
    let mut balloon = None;
    TAPPS.with_borrow_mut(|t| {
        let pos = t.iter().position(|x| x.hwnd == h && x.uid == uid && x.guid == guid);
        match (op, pos) {
            (2, Some(i)) => {
                DestroyIcon(t.remove(i).icon);
            }
            (4, Some(i)) => t[i].ver = u(808),
            (0 | 1, _) => {
                let i = match pos {
                    Some(i) => i,
                    None if op == 1 => return, // modify of an icon we never saw
                    None => {
                        let exe = exe_of(h);
                        let n = exe.rsplit('\\').next().unwrap_or("");
                        if (n == "explorer.exe" && own_icon(&guid)) || n == "topbar.exe" || (n == "onedrive.exe" && hide_onedrive()) {
                            return; // the system's own icons are drawn by the bar itself
                        }
                        t.push(TIcon { hwnd: h, uid, guid, icon: null_mut(), cb: 0, ver: 0, hidden: false, tip: String::new(), key: n.to_string() });
                        t.len() - 1
                    }
                };
                let x = &mut t[i];
                if flags & 1 != 0 {
                    x.cb = u(24);
                }
                if flags & 2 != 0 {
                    let ic = CopyIcon(u(28) as i32 as isize as HICON);
                    if !ic.is_null() {
                        if !x.icon.is_null() {
                            DestroyIcon(x.icon);
                        }
                        x.icon = ic;
                    }
                }
                if flags & 4 != 0 {
                    let w: Vec<u16> = d[32..288].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
                    x.tip = String::from_utf16_lossy(&w);
                }
                if flags & 8 != 0 && u(292) & 1 != 0 {
                    x.hidden = u(288) & 1 != 0; // NIS_HIDDEN
                }
                // a balloon: it becomes a notification of the app that owns the icon
                balloon = notify::parse_balloon(d).map(|b| (b, x.hwnd, x.uid));
            }
            _ => {}
        }
    });
    if let Some((b, hwnd, uid)) = balloon {
        balloon_note(&b, hwnd, uid);
    }
}

/// A tray balloon as a note: the app's own face and name; a click on it is a click on the balloon.
unsafe fn balloon_note(b: &notify::Balloon, hwnd: HWND, uid: u32) {
    use notify::{App, AppIcon, Note, Target};
    let exe = exe_of(hwnd);
    let name = if exe.is_empty() { b.title.clone() } else { notify::app_name(&exe) };
    let key = if exe.is_empty() { format!("window:{:x}", hwnd as usize) } else { exe.to_lowercase() };
    let icon = if exe.is_empty() { AppIcon::Winrust } else { AppIcon::Exe(exe) };
    let (title, body) = if b.title.is_empty() { (name.clone(), b.text.clone()) } else { (b.title.clone(), b.text.clone()) };
    notify::post(Note::new(App { key, name, icon }, title, body).target(Target::Balloon { hwnd: hwnd as isize, uid }));
}

/// A click on a balloon's note: the app is told what Explorer tells it, `NIN_BALLOONUSERCLICK`, the way a click on its icon is.
pub(crate) unsafe fn balloon_click(hwnd: isize, uid: u32) {
    let Some((cb, ver)) = TAPPS.with_borrow(|t| t.iter().find(|x| x.hwnd as isize == hwnd && x.uid == uid).map(|x| (x.cb, x.ver))) else { return };
    if cb == 0 {
        return;
    }
    let hwnd = hwnd as HWND;
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    AllowSetForegroundWindow(pid); // so the app can bring its window to the front
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    const NIN_BALLOONUSERCLICK: u32 = 0x405;
    if ver >= 4 {
        PostMessageW(hwnd, cb, ((pt.x as u32 & 0xffff) | (pt.y as u32) << 16) as usize, (NIN_BALLOONUSERCLICK | (uid & 0xffff) << 16) as isize);
    } else {
        PostMessageW(hwnd, cb, uid as usize, NIN_BALLOONUSERCLICK as isize);
    }
}

/// A Shell_NotifyIconGetRect request (WM_COPYDATA, dwData 3): the icon asked about and which corner of its rectangle.
#[derive(Debug, PartialEq)]
pub(crate) struct IconQuery {
    /// 1 = top left corner, 2 = bottom right corner.
    corner: u32,
    hwnd: HWND,
    uid: u32,
    guid: [u8; 16],
}

/// The request Windows sends (32-bit handles for every app): u32 signature 0x34753423, u32 corner, u32 cbSize, u32 padding,
/// u32 hWnd@16, u32 uID@20, GUID@24. None for anything else.
pub(crate) fn parse_icon_query(d: &[u8]) -> Option<IconQuery> {
    let u = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    if d.len() < 40 || u(0) != 0x3475_3423 || !matches!(u(4), 1 | 2) {
        return None;
    }
    Some(IconQuery { corner: u(4), hwnd: u(16) as i32 as isize as HWND, uid: u(20), guid: d[24..40].try_into().unwrap() })
}

/// left, top, right, bottom in screen pixels.
type ScreenRect = (i32, i32, i32, i32);

thread_local! {
    /// The icon of the hidden icons panel clicked last, and its cell on the screen: where `Shell_NotifyIconGetRect` puts it.
    static PANEL_CLICK: Cell<(usize, u32, ScreenRect)> = const { Cell::new((0, 0, (0, 0, 0, 0))) };
}

/// Where the icon of the query is on the screen (left, top, right, bottom): its button on the bar, its cell in the panel that
/// was clicked last, else the arrow that opens the panel. None if no such icon is known.
unsafe fn icon_rect(q: &IconQuery) -> Option<(i32, i32, i32, i32)> {
    let (i, hwnd, uid) = TAPPS.with_borrow(|t| {
        let by_guid = q.guid != [0; 16];
        let i = t.iter().position(|x| if by_guid { x.guid == q.guid } else { x.hwnd == q.hwnd && x.uid == q.uid })?;
        Some((i, t[i].hwnd, t[i].uid))
    })?;
    let key = match bar_idx().iter().position(|&b| b == i) {
        Some(slot) => TAPP + slot as i32 + 1, // the same key click() works out for the button
        None => match PANEL_CLICK.get() {
            (h, u, r) if h == hwnd as usize && u == uid => return Some(r),
            _ => TAPP,
        },
    };
    anchor(key).map(|a| a.button)
}

/// The answer to a request for one corner: MAKELONG(x, y) in screen pixels.
fn rect_reply(corner: u32, r: (i32, i32, i32, i32)) -> LRESULT {
    let (x, y) = if corner == 1 { (r.0, r.1) } else { (r.2, r.3) };
    ((x as i16 as u16 as u32) | (y as i16 as u16 as u32) << 16) as LRESULT
}

/// What ITaskbarList asks of the task band, applied to `skip`: DeleteTab (HSHELL_WINDOWDESTROYED for a window that lives
/// on) takes the window's button away, AddTab (HSHELL_WINDOWCREATED) gives it back. Closed windows are forgotten.
/// true = the window's button comes or goes.
fn taskband(skip: &mut Vec<HWND>, code: WPARAM, w: HWND, alive: impl Fn(HWND) -> bool) -> bool {
    skip.retain(|&x| alive(x));
    let had = skip.contains(&w);
    match code {
        2 if !had && alive(w) => skip.push(w),
        1 if had => skip.retain(|&x| x != w),
        _ => return false,
    }
    true
}

pub(crate) unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg != 0 && msg == SH_MSG.get() {
        // ITaskbarList on the task band we named in TaskbandHWND. Tauri's skipTaskbar works this way, and Tauri sends it again
        // when the bar announces itself (TaskbarCreated), so a restarted bar learns it too.
        if SKIP.with_borrow_mut(|s| taskband(s, wp, lp as HWND, |x| IsWindow(x) != 0)) {
            let bar = BAR.load(Relaxed) as HWND;
            if !bar.is_null() && !WIN_DIRTY.replace(true) {
                SetTimer(bar, 8, 30, None);
            }
        }
        return 0;
    }
    if msg == WM_COPYDATA {
        let c = &*(lp as *const Cds);
        if c.dw == 1 && c.cb >= 964 {
            tray_msg(std::slice::from_raw_parts(c.p, c.cb as usize));
            return 1;
        }
        if c.dw == 3 && c.cb >= 40 {
            // Shell_NotifyIconGetRect: apps (Tauri tray-icon, Epic Games Launcher) ask where an icon is before they open a menu
            if let Some(q) = parse_icon_query(std::slice::from_raw_parts(c.p, c.cb as usize))
                && let Some(r) = icon_rect(&q)
            {
                return rect_reply(q.corner, r);
            }
        }
        // SHAppBarMessage (our own bar's work-area reservation, ABM_SETSTATE...) also goes to "Shell_TrayWnd": hand it to explorer's
        // with a timeout: a hung explorer must not freeze the bar
        let mut r = 0usize;
        SendMessageTimeoutW(find_other("Shell_TrayWnd"), msg, wp, lp, SMTO_ABORTIFHUNG, 2000, &mut r);
        return r as LRESULT;
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

thread_local! {
    static LAST_CLICK: Cell<(usize, u32, u32)> = const { Cell::new((0, 0, 0)) }; // (icon window, icon id, tick) of the last left click
}

/// Posts the same messages explorer would for a click on the icon; the app shows its own window or menu.
pub(crate) unsafe fn tray_click(t: (HWND, u32, u32, u32), left: bool) {
    let (hwnd, uid, cb, ver) = t;
    if cb == 0 {
        return;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    AllowSetForegroundWindow(pid); // so the app can bring its popup menu to the front
    let mut pt: POINT = zeroed();
    GetCursorPos(&mut pt);
    let post = |ev: u32| {
        if ver >= 4 {
            PostMessageW(hwnd, cb, ((pt.x as u32 & 0xffff) | (pt.y as u32) << 16) as usize, (ev | (uid & 0xffff) << 16) as isize);
        } else {
            PostMessageW(hwnd, cb, uid as usize, ev as isize);
        }
    };
    // what Explorer sends: the raw button messages always, plus NIN_SELECT / WM_CONTEXTMENU for version 4. Many apps open
    // only on the button-up or the double click (Steam), so NIN_SELECT alone left them dead (#115).
    let now = windows_sys::Win32::System::SystemInformation::GetTickCount();
    let prev = if left { LAST_CLICK.replace((hwnd as usize, uid, now)) } else { (0, 0, 0) };
    let double = left && (prev.0, prev.1) == (hwnd as usize, uid) && now.wrapping_sub(prev.2) <= GetDoubleClickTime();
    if double {
        LAST_CLICK.set((0, 0, 0)); // a third click starts a new pair
    }
    match (left, ver >= 4) {
        (true, v4) => {
            [WM_LBUTTONDOWN, WM_LBUTTONUP].into_iter().for_each(post);
            if v4 { post(0x400); } // NIN_SELECT
            if double { post(WM_LBUTTONDBLCLK); }
        }
        (false, v4) => {
            [WM_RBUTTONDOWN, WM_RBUTTONUP].into_iter().for_each(post);
            if v4 { post(WM_CONTEXTMENU); }
        }
    }
}

/// Indices into TAPPS of the icons kept on the bar; the rest live in the chevron flyout.
pub(crate) fn bar_idx() -> Vec<usize> {
    let shown = SHOWN.with_borrow(|s| s.clone());
    TAPPS.with_borrow(|t| t.iter().enumerate().filter(|(_, x)| !x.hidden && shown.as_ref().is_some_and(|s| s.contains(&x.key))).map(|(i, _)| i).collect())
}
/// Indices into TAPPS of the icons only the flyout shows (everything not kept on the bar).
pub(crate) fn over_idx() -> Vec<usize> {
    let bar = bar_idx();
    TAPPS.with_borrow(|t| (0..t.len()).filter(|i| !t[*i].hidden && !bar.contains(i)).collect())
}

pub(crate) fn toggle_shown(key: &str) {
    let mut cur = SHOWN.with_borrow(|s| s.clone()).unwrap_or_else(|| TAPPS.with_borrow(|t| bar_idx().into_iter().map(|i| t[i].key.clone()).collect()));
    match cur.iter().position(|k| k == key) {
        Some(p) => drop(cur.remove(p)),
        None => cur.push(key.to_string()),
    }
    let _ = std::fs::write(appdata_file("topbar-tray.txt"), cur.join("\n"));
    SHOWN.set(Some(cur));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(sig: u32, corner: u32) -> Vec<u8> {
        let mut d = [sig, corner, 40, 0, 0x1234, 7].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        d.extend(0..16u8); // the GUID
        d
    }

    #[test]
    fn get_rect_request_is_parsed_and_answered() {
        for corner in [1, 2] {
            let q = parse_icon_query(&query(0x3475_3423, corner)).unwrap();
            assert_eq!((q.corner, q.hwnd as usize, q.uid, q.guid[15]), (corner, 0x1234, 7, 15));
        }
        assert_eq!(parse_icon_query(&query(0x1111_1111, 1)), None, "somebody else's signature");
        assert_eq!(parse_icon_query(&query(0x3475_3423, 3)), None, "an unknown corner");
        assert_eq!(parse_icon_query(&query(0x3475_3423, 1)[..39]), None, "too short");
        // MAKELONG(x, y), negative coordinates (a monitor on the left) keep their 16 bits
        assert_eq!(rect_reply(1, (100, 200, 300, 400)), 100 | 200 << 16);
        assert_eq!(rect_reply(2, (100, 200, 300, 400)), 300 | 400 << 16);
        assert_eq!(rect_reply(1, (-5, 2, 0, 0)), 0xfffb | 2 << 16);
    }

    #[test]
    fn delete_tab_hides_and_add_tab_brings_back() {
        let (a, b, gone) = (1 as HWND, 2 as HWND, 3 as HWND);
        let alive = |x: HWND| x != gone;
        let mut skip = vec![];
        assert!(taskband(&mut skip, 2, a, alive)); // DeleteTab
        assert!(!taskband(&mut skip, 2, a, alive)); // again (Tauri repeats it on TaskbarCreated): nothing new
        assert!(!taskband(&mut skip, 1, b, alive)); // AddTab of a window that kept its button
        assert!(!taskband(&mut skip, 2, gone, alive)); // a closed window
        assert!(!taskband(&mut skip, 4, a, alive)); // ActivateTab and the rest change nothing
        assert_eq!(skip, [a]);
        assert!(taskband(&mut skip, 1, a, alive)); // AddTab
        assert!(skip.is_empty());
        skip.push(gone);
        assert!(!taskband(&mut skip, 1, b, alive));
        assert!(skip.is_empty(), "closed windows are forgotten");
    }
}


// ---------- the hidden icons panel (the chevron): every icon, clicks work as in the real tray ----------

/// The icons the panel shows, read afresh while it is open (programs add and drop their icons any time).
fn panel_cells() -> Vec<ui::tray_popup::Cell> {
    let over = over_idx();
    TAPPS.with_borrow(|t| {
        over.iter()
            .map(|&i| ui::tray_popup::Cell { icon: wicon(t[i].icon), tip: t[i].tip.clone(), token: i })
            .collect()
    })
}

/// A click on an icon of the panel (`cell`: its rectangle on the screen): left and right go to its program, middle moves it onto the bar.
fn panel_click(token: usize, b: ui::panel::Button, cell: (i32, i32, i32, i32)) {
    use ui::panel::Button;
    let icon = TAPPS.with_borrow(|t| t.get(token).map(|x| ((x.hwnd, x.uid, x.cb, x.ver), x.key.clone())));
    let Some((t, key)) = icon else { return };
    PANEL_CLICK.set((t.0 as usize, t.1, cell)); // before the click: the program asks where the icon is as it opens its menu
    // SAFETY: posts the click to the program's own window.
    unsafe {
        match b {
            Button::Middle => toggle_shown(&key),
            Button::Left => tray_click(t, true),
            Button::Right => tray_click(t, false),
        }
    }
}

/// The arrow on the bar was clicked: open the panel of hidden icons, or close it.
pub(crate) unsafe fn tray_toggle() {
    ui::panel::toggle(ui::panel::Id::Tray, || {
        let a = anchor(TAPP)?;
        Some((Box::new(ui::tray_popup::TrayPanel::new(panel_cells, panel_click)), a))
    });
}
