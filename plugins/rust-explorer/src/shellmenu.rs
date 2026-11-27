//! The bar's menu for files, apps and folders, rust-explorer's side (the protocol and its reasons are in
//! `winrust_common::filemenu`). A right click builds the shell's own `IContextMenu` for the selection, reads its items, and offers
//! them to the bar ([`offer`]); the bar draws the menu and sends back what was chosen ([`act`]). The menu stays alive here
//! while the bar shows it: Send to, Open with and the submenus of other programs are filled only when they are opened, and what
//! is chosen runs here, as the desktop user and with a window of ours as its owner.
//!
//! `false` from [`offer`] (no bar, or one that does not answer) leaves the caller to show the old menu.
use std::{cell::RefCell, ffi::c_void, path::{Path, PathBuf}, sync::atomic::{AtomicU32, Ordering::Relaxed}};
use windows::{
    core::*,
    Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};
use winrust_common::filemenu::{self, Act, Request, ShellItem, SubReply, menu, rq};

/// The menu the bar is showing now.
struct Session {
    id: u32,
    cm: IContextMenu,
    cm2: Option<IContextMenu2>,
    menu: HMENU,
    /// The submenus handed out as [`filemenu::Sub::Lazy`], by handle - 1: the menu and its position in its parent.
    subs: Vec<(*mut c_void, u32)>,
    dir: PathBuf,
    paths: Vec<PathBuf>,
    /// Submenus the bar opened that Windows may still be filling (Send to fills from a worker): read again for a while.
    watch: Vec<Watch>,
}

/// A submenu being read again: what the bar was last told about it and how many times it has been read since.
struct Watch {
    handle: u32,
    told: Option<usize>,
    ticks: u8,
}

/// The timer that reads watched submenus again, how often, and for how many reads (about 0.7 s: Send to is whole in a few
/// hundred ms).
pub(crate) const TIMER: usize = 0x5746;
const EVERY: u32 = 120;
const READS: u8 = 6;

/// Whether to tell the bar about a read of `now` items, given the count it was last told (`None`: nothing yet). An empty
/// read is not worth telling while the submenu may still fill; on the last read it ends the bar's waiting.
fn worth_telling(told: Option<usize>, now: usize, last: bool) -> bool {
    (now > 0 && told != Some(now)) || (last && told.is_none())
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the menu is ours; the context menu objects go with the struct.
        unsafe { let _ = DestroyMenu(self.menu); }
    }
}

thread_local! { static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) }; }
static NEXT: AtomicU32 = AtomicU32::new(1);

/// What reads a menu: the verb of a command, and the "about to open" message of a submenu.
struct Src<'a> {
    cm: &'a IContextMenu,
    cm2: Option<&'a IContextMenu2>,
    subs: &'a mut Vec<(*mut c_void, u32)>,
}

impl menu::Source for Src<'_> {
    fn verb(&mut self, id: u32) -> String {
        let at = id.saturating_sub(1) as usize;
        let mut buf = [0u16; 128];
        // SAFETY: the buffer is as long as the call is told; a command without a verb answers an error.
        let ok = unsafe { self.cm.GetCommandString(at, GCS_VERBW, None, PSTR(buf.as_mut_ptr().cast()), buf.len() as u32) };
        if ok.is_ok() {
            return String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
        }
        // some handlers answer only the ANSI question
        let mut a = [0u8; 128];
        // SAFETY: as above.
        let ok = unsafe { self.cm.GetCommandString(at, GCS_VERBA, None, PSTR(a.as_mut_ptr()), a.len() as u32) };
        if ok.is_err() { return String::new(); }
        String::from_utf8_lossy(&a[..a.iter().position(|&c| c == 0).unwrap_or(a.len())]).into_owned()
    }
    fn opening(&mut self, m: *mut c_void, at: u32) {
        // SAFETY: WM_INITMENUPOPUP (the submenu, its position) is how the shell is told a submenu is about to be shown.
        if let Some(c2) = self.cm2 { unsafe { let _ = c2.HandleMenuMsg(WM_INITMENUPOPUP, WPARAM(m as usize), LPARAM(at as isize)); } }
    }
    fn keep(&mut self, m: *mut c_void, at: u32) -> u32 {
        self.subs.push((m, at));
        self.subs.len() as u32
    }
}

impl Session {
    fn src(&mut self) -> (Src<'_>, HMENU) {
        (Src { cm: &self.cm, cm2: self.cm2.as_ref(), subs: &mut self.subs }, self.menu)
    }

    fn tidy(mut items: Vec<ShellItem>) -> Vec<ShellItem> {
        // our own "Compress to ZIP" is the one: Windows' entry in Send to is left out, as in the old menu (other zip tools stay)
        if let Some(zip) = crate::sh::system_zip_name() { items.retain(|i| !i.label.eq_ignore_ascii_case(&zip)); }
        items
    }

    /// The items of the submenu with this handle, now that it is being opened (`None`: not worth telling yet); the submenu is
    /// watched for what Windows adds a moment later.
    fn fill(&mut self, handle: u32) -> Option<Vec<ShellItem>> {
        let &(m, at) = handle.checked_sub(1).and_then(|i| self.subs.get(i as usize))?;
        let (mut src, _) = self.src();
        // SAFETY: `m` is a submenu of the menu this session owns.
        let items = Self::tidy(unsafe { menu::fill(m, at, &mut src) });
        let tell = worth_telling(None, items.len(), false);
        self.watch.retain(|w| w.handle != handle);
        self.watch.push(Watch { handle, told: tell.then_some(items.len()), ticks: 0 });
        tell.then_some(items)
    }

    /// One more read of every watched submenu: what changed since the bar was told, as (handle, items).
    fn reread(&mut self) -> Vec<(u32, Vec<ShellItem>)> {
        let mut out = Vec::new();
        for i in 0..self.watch.len() {
            let (handle, told, ticks) = (self.watch[i].handle, self.watch[i].told, self.watch[i].ticks + 1);
            self.watch[i].ticks = ticks;
            let Some(&(m, _)) = handle.checked_sub(1).and_then(|h| self.subs.get(h as usize)) else { continue };
            let (mut src, _) = self.src();
            // SAFETY: as in `fill`; the shell has been told already, this only reads what it has put there since.
            let items = Self::tidy(unsafe { menu::read(m, &mut src) });
            if worth_telling(told, items.len(), ticks >= READS) {
                self.watch[i].told = Some(items.len());
                out.push((handle, items));
            }
        }
        self.watch.retain(|w| w.ticks < READS);
        out
    }

    /// Runs item `id` of the shell's menu.
    fn run(&self, h: HWND, id: u32) {
        let mut pt = POINT::default();
        // SAFETY: plain calls; the verb is the command's offset, which is how the shell numbers its items.
        unsafe {
            let _ = GetCursorPos(&mut pt);
            // ptInvoke: without it dialogs the shell opens ("Open with" > choose another app) land off screen
            let ci = CMINVOKECOMMANDINFOEX {
                cbSize: size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                fMask: CMIC_MASK_PTINVOKE,
                hwnd: h,
                lpVerb: PCSTR(id.saturating_sub(1) as usize as *const u8),
                nShow: SW_SHOWNORMAL.0,
                ptInvoke: pt,
                ..Default::default()
            };
            let before = crate::sh::top_windows();
            let _ = self.cm.InvokeCommand(&ci as *const _ as *const CMINVOKECOMMANDINFO);
            crate::sh::keep_on_screen(before);
        }
    }
}

/// The shell's menu for `paths` and its top level items, with the session that keeps it alive.
fn build(h: HWND, paths: &[PathBuf], dir: &Path) -> Option<(Session, Vec<ShellItem>)> {
    let cm: IContextMenu = crate::sh::ui(h, paths).ok()?;
    // SAFETY: a fresh menu for the shell to fill, owned by the session.
    let menu_h = unsafe { CreatePopupMenu().ok()? };
    // SAFETY: as above; ids 1..0x7FFF are what the shell numbers its commands in.
    if unsafe { cm.QueryContextMenu(menu_h, 0, 1, 0x7FFF, CMF_NORMAL) }.is_err() {
        // SAFETY: the menu is ours.
        unsafe { let _ = DestroyMenu(menu_h); }
        return None;
    }
    let cm2 = cm.cast::<IContextMenu2>().ok();
    let mut s = Session { id: NEXT.fetch_add(1, Relaxed), cm, cm2, menu: menu_h, subs: Vec::new(), dir: dir.to_path_buf(), paths: paths.to_vec(), watch: Vec::new() };
    let (mut src, m) = s.src();
    // SAFETY: the menu was just filled by the shell and is ours.
    let items = unsafe { menu::read(m.0, &mut src) };
    Some((s, items))
}

/// Asks the bar to show the menu for `paths` (all in `dir`) at the pointer, or at `key_at` when it was the keyboard that opened
/// it. `true` = the bar has it: everything else happens in [`act`].
pub(crate) fn offer(hw: *mut c_void, dir: &Path, paths: &[PathBuf], key_at: Option<(i32, i32)>) -> bool {
    let h = HWND(hw);
    if paths.is_empty() || winrust_common::deskmenu::bar().is_null() { return false; }
    let Some((s, items)) = build(h, paths, dir) else { return false };
    let mut pt = POINT::default();
    // SAFETY: plain call.
    unsafe { let _ = GetCursorPos(&mut pt); }
    let at = key_at.unwrap_or((pt.x, pt.y));
    let one = paths.len() == 1;
    let mut flags = 0;
    if crate::desk() { flags |= rq::DESKTOP; } else { flags |= rq::TAB; }
    if one { flags |= rq::CAN_RENAME; }
    if one && paths[0].is_dir() && crate::ui::pins().contains(&paths[0]) { flags |= rq::PINNED; }
    let req = Request { session: s.id, at, flags, paths: paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(), items };
    let id = s.id;
    // kept before the bar is asked: its first message back can come while it is still answering
    SESSION.set(Some(s));
    if filemenu::ask(hw, &req) { return true; }
    SESSION.with_borrow_mut(|c| if c.as_ref().is_some_and(|s| s.id == id) { *c = None; });
    false
}

/// Where the menu opens when the keyboard asked: the lower middle of the first selected row, in screen pixels (the mockup's
/// `x = middle, y = bottom - 10`).
pub(crate) fn row_at(list: *mut c_void) -> Option<(i32, i32)> {
    use windows_sys::Win32::{Foundation::RECT, Graphics::Gdi::ClientToScreen, UI::Controls::{LVM_GETITEMRECT, LVM_GETNEXTITEM, LVNI_SELECTED}};
    let i = crate::put(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize);
    if i < 0 { return None; }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 }; // LVIR_BOUNDS = 0 is asked for in `left`
    // SAFETY: the list view fills the rectangle; the point is converted with the list's own handle.
    unsafe {
        crate::put(list, LVM_GETITEMRECT, i as usize, &mut r as *mut RECT as isize);
        let mut pt = windows_sys::Win32::Foundation::POINT { x: (r.left + r.right) / 2, y: r.bottom - crate::ui::sc(10) };
        if ClientToScreen(list, &mut pt) == 0 { return None; }
        Some((pt.x, pt.y))
    }
}

fn set_clipboard_text(h: HWND, text: &str) {
    use windows::Win32::{Foundation::HANDLE, System::{DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData}, Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock}}};
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    // SAFETY: the standard way to put text on the clipboard: a moveable block the clipboard owns after SetClipboardData.
    unsafe {
        if OpenClipboard(Some(h)).is_err() { return; }
        let _ = EmptyClipboard();
        if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(mem) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(mem);
                let _ = SetClipboardData(13, Some(HANDLE(mem.0))); // CF_UNICODETEXT
            }
        }
        let _ = CloseClipboard();
    }
}

/// "Copy as path": one quoted path a line, as Windows does.
fn copy_paths(h: HWND, paths: &[PathBuf]) {
    set_clipboard_text(h, &paths.iter().map(|p| format!("\"{}\"", p.display())).collect::<Vec<_>>().join("\r\n"));
}

fn terminal(dir: &Path) {
    use std::{os::windows::process::CommandExt, process::Command};
    // Windows Terminal when it is installed (its alias lives in WindowsApps), else PowerShell in a console of its own
    if Command::new("wt.exe").arg("-d").arg(dir).spawn().is_err() {
        let _ = Command::new("cmd.exe").args(["/c", "start", "powershell.exe", "-NoExit"]).current_dir(dir).creation_flags(0x0800_0000).spawn();
    }
}

/// The timer of the watched submenus ([`TIMER`]): reads them again and tells the bar what Windows has added.
pub(crate) fn tick(hw: *mut c_void) {
    let (session, news, again) = SESSION.with_borrow_mut(|c| match c.as_mut() {
        Some(s) => (s.id, s.reread(), !s.watch.is_empty()),
        None => (0, Vec::new(), false),
    });
    for (handle, items) in news { let _ = filemenu::send_sub(hw, &SubReply { session, handle, items }); }
    if !again {
        // SAFETY: the timer set in `act`.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::KillTimer(hw, TIMER) };
    }
}

/// What the bar chose. `lp` is the menu's session: an answer for a menu that has been replaced is ignored.
pub(crate) fn act(hw: *mut c_void, wp: usize, lp: isize) {
    let h = HWND(hw);
    let Some((a, session)) = Act::decode(wp, lp) else { return };
    if let Act::Fill(n) = a {
        let (known, items) = SESSION.with_borrow_mut(|c| match c.as_mut().filter(|s| s.id == session) {
            Some(s) => (true, s.fill(n)),
            None => (false, None),
        });
        // a menu that is gone: an empty answer ends the bar's spinner
        if !known {
            let _ = filemenu::send_sub(hw, &SubReply { session, handle: n, items: Vec::new() });
            return;
        }
        if let Some(items) = items { let _ = filemenu::send_sub(hw, &SubReply { session, handle: n, items }); }
        // SAFETY: a timer of the window that is handling this message.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetTimer(hw, TIMER, EVERY, None) };
        return;
    }
    // Everything else ends the menu: the session is taken out first, so a modal dialog's message loop (Delete asks) cannot meet it
    let Some(s) = SESSION.with_borrow_mut(|c| if c.as_ref().is_some_and(|s| s.id == session) { c.take() } else { None }) else { return };
    match a {
        Act::Item(id) => {
            s.run(h, id);
            crate::reload(hw);
        }
        Act::Rename => unsafe {
            let (list, ..) = crate::selected();
            let i = crate::put(list, windows_sys::Win32::UI::Controls::LVM_GETNEXTITEM, usize::MAX, windows_sys::Win32::UI::Controls::LVNI_SELECTED as isize);
            if i >= 0 {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(list);
                crate::put(list, windows_sys::Win32::UI::Controls::LVM_EDITLABELW, i as usize, 0);
            }
        },
        Act::Zip => {
            let base = if s.paths.len() == 1 { s.paths[0].file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { s.dir.file_name().map_or("Archive".into(), |n| n.to_string_lossy().into_owned()) };
            let dest = crate::jobs::free_name(&s.dir, &format!("{base}.zip"), false);
            crate::jobs::zip(hw as isize, s.paths.clone(), dest);
        }
        Act::Extract => _ = crate::jobs::extract(hw as isize, s.paths.clone(), s.dir.clone()),
        Act::CopyPath => copy_paths(h, &s.paths),
        Act::NewTab => {
            if let [p] = s.paths.as_slice() {
                if crate::desk() { let _ = std::process::Command::new(std::env::current_exe().unwrap_or_default()).arg(p).spawn(); } else { crate::tab_open(p.clone()); }
            }
        }
        Act::Terminal => {
            if let [p] = s.paths.as_slice() { terminal(p); }
        }
        Act::TogglePin => {
            if let [p] = s.paths.as_slice() { crate::ui::toggle_pin(p); }
        }
        Act::Reveal => {
            if let [p] = s.paths.as_slice() { winrust_common::reveal(&p.to_string_lossy()); }
        }
        Act::Done | Act::Fill(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_submenu_that_is_still_filling_is_told_when_it_has_something_and_when_it_changes() {
        assert!(!worth_telling(None, 0, false), "empty and may still fill: wait");
        assert!(worth_telling(None, 0, true), "empty on the last read: the bar stops waiting");
        assert!(worth_telling(None, 2, false));
        assert!(!worth_telling(Some(2), 2, false), "nothing new");
        assert!(worth_telling(Some(2), 5, false), "Send to grew");
        assert!(!worth_telling(Some(5), 5, true));
        assert!(!worth_telling(Some(5), 0, true), "an empty read after items is not news");
    }

    /// The menu of a real file in the temp folder, only built and read (nothing is invoked): the shell's top level has Open as
    /// the default item and Properties, by their verbs, and every submenu is a handle until it is asked for.
    #[test]
    fn a_real_menu_of_a_temp_file_is_read_and_nothing_is_run() {
        crate::sh::init();
        let d = std::env::temp_dir().join(format!("wr-filemenu-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("note.txt");
        std::fs::write(&f, b"x").unwrap();
        let built = build(HWND::default(), std::slice::from_ref(&f), &d);
        let _ = std::fs::remove_dir_all(&d);
        let (s, items) = built.expect("a menu");
        let verbs: Vec<&str> = items.iter().map(|i| i.verb.as_str()).collect();
        assert!(items.iter().any(|i| i.verb == "open" && i.is_default() && i.id != 0), "{verbs:?}");
        assert!(verbs.contains(&"properties") && verbs.contains(&"copy") && verbs.contains(&"cut") && verbs.contains(&"delete"), "{verbs:?}");
        assert!(items.iter().filter(|i| i.has_sub()).all(|i| matches!(i.sub, filemenu::Sub::Lazy(n) if n as usize <= s.subs.len())), "submenus are handles");
        assert!(s.watch.is_empty());
    }
}
