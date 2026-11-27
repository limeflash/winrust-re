use std::{cell::{Cell, RefCell}, collections::{HashMap, VecDeque}, rc::Rc, sync::{Mutex, atomic::{AtomicUsize, Ordering::Relaxed}}};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
use crate::ui::taskview::OwnedIcon;

struct Shot { window: isize, icon: Rc<OwnedIcon>, width: i32, height: i32 }
/// A picture a worker finished: the window, the ticket it was asked under, the icon handle (an `isize`: a handle is not `Send`).
struct Done { window: isize, ticket: u64, icon: isize, width: i32, height: i32 }
thread_local! {
    static SHOTS: RefCell<VecDeque<Shot>> = const { RefCell::new(VecDeque::new()) };
    // captures in flight (window -> ticket), bar's thread only: a restore or destroy takes the window out, and its picture is then thrown away
    static PENDING: RefCell<HashMap<isize, u64>> = RefCell::new(HashMap::new());
    static TICKET: Cell<u64> = const { Cell::new(0) };
}
static DONE: Mutex<Vec<Done>> = Mutex::new(Vec::new());
static BUSY: AtomicUsize = AtomicUsize::new(0); // capture threads alive (one stuck on a hung program stays until it answers)
const WORKERS: usize = 4;
/// Posted to the bar by a capture thread when a picture is ready: `take_done` on the bar's thread stores it.
pub const SNAPSHOT: u32 = WM_APP + 20;
#[link(name = "user32")]
unsafe extern "system" { fn PrintWindow(window: HWND, dc: HDC, flags: u32) -> i32; }
const LIMIT: usize = 32;

pub fn get(window: isize) -> Option<(Rc<OwnedIcon>, i32, i32)> {
    SHOTS.with_borrow(|shots| shots.iter().find(|s| s.window == window).map(|s| (s.icon.clone(), s.width, s.height)))
}

fn put(shots: &mut VecDeque<Shot>, shot: Shot) {
    shots.retain(|s| s.window != shot.window);
    shots.push_back(shot);
    while shots.len() > LIMIT { shots.pop_front(); }
}

pub fn insert(window: isize, icon: Rc<OwnedIcon>, width: i32, height: i32) {
    SHOTS.with_borrow_mut(|shots| put(shots, Shot { window, icon, width, height }));
}

pub fn remove(window: isize) {
    PENDING.with_borrow_mut(|p| p.remove(&window));
    SHOTS.with_borrow_mut(|shots| shots.retain(|s| s.window != window));
}

/// A capture of `window` is asked for: its ticket.
fn begin(window: isize) -> u64 {
    let ticket = TICKET.get() + 1;
    TICKET.set(ticket);
    PENDING.with_borrow_mut(|p| p.insert(window, ticket));
    ticket
}

/// Is a picture made under `ticket` still wanted (the window was not restored, destroyed or captured again meanwhile)? Takes it off the list.
fn finish(window: isize, ticket: u64) -> bool {
    PENDING.with_borrow_mut(|p| if p.get(&window) == Some(&ticket) { p.remove(&window); true } else { false })
}

/// The bar's thread, on `SNAPSHOT`: stores the pictures the capture threads made, drops those no longer wanted.
pub fn take_done() {
    let done = std::mem::take(&mut *DONE.lock().unwrap_or_else(|e| e.into_inner()));
    for d in done {
        // SAFETY: the icon was made by the capture thread for this message alone; the card owns it from here (OwnedIcon destroys it).
        let icon = Rc::new(OwnedIcon(crate::wicon(d.icon as HICON)));
        if finish(d.window, d.ticket) { insert(d.window, icon, d.width, d.height); }
    }
}

/// The bar's thread, on the minimize event: asks for a picture of `window` from a thread of its own. `PrintWindow` waits for the
/// window's own thread, and a program that does not answer (the one being minimized because it hangs, often) must hold that
/// thread, not the bar's. At most `WORKERS` are alive at once: a program that never answers keeps its thread, the rest is skipped.
pub unsafe fn capture(window: HWND, bar: HWND) {
    // SAFETY: GetWindowRect/IsIconic only read the window's state.
    let (w, h) = unsafe {
        let mut rect: RECT = std::mem::zeroed();
        // the event arrives after the fact: a window already minimized has nothing to show (a 160x28 strip off the screen),
        // and then its card keeps the icon
        if IsIconic(window) != 0 || GetWindowRect(window, &mut rect) == 0 || rect.left <= -32000 { return; }
        (rect.right - rect.left, rect.bottom - rect.top)
    };
    if w <= 0 || h <= 0 || w > 8192 || h > 8192 || BUSY.load(Relaxed) >= WORKERS { return; }
    BUSY.fetch_add(1, Relaxed); // only the bar's thread adds, so the check above cannot be raced past
    let (win, bar) = (window as isize, bar as isize);
    let ticket = begin(win);
    let spawned = std::thread::Builder::new().name("snapshot".into()).spawn(move || {
        // SAFETY: the handles are only asked about; a window that is gone makes the calls fail.
        let shot = unsafe { grab(win as HWND, w, h) };
        BUSY.fetch_sub(1, Relaxed);
        if let Some((icon, width, height)) = shot {
            DONE.lock().unwrap_or_else(|e| e.into_inner()).push(Done { window: win, ticket, icon, width, height });
            // SAFETY: posting to a window that is gone just fails.
            if unsafe { PostMessageW(bar as HWND, SNAPSHOT, 0, 0) } == 0 { /* the bar is closing: the picture goes with the process */ }
        }
    });
    if spawned.is_err() { BUSY.fetch_sub(1, Relaxed); remove(win); }
}

/// On the capture thread: the window's picture (at most 480 wide) as an icon handle, with its size.
unsafe fn grab(window: HWND, w: i32, h: i32) -> Option<(isize, i32, i32)> {
    // SAFETY: all GDI objects are local, selected objects are restored, and the icon owns a copy of its bitmaps.
    unsafe {
        // a probe that gives up after 100 ms asks first: a program that is already hung does not even get a PrintWindow
        let mut answer = 0usize;
        if SendMessageTimeoutW(window, WM_NULL, 0, 0, SMTO_ABORTIFHUNG | SMTO_BLOCK, 100, &mut answer) == 0 { return None; }
        let dc = GetDC(std::ptr::null_mut());
        if dc.is_null() { return None; }
        let mut made = None;
        let src = CreateCompatibleDC(dc);
        let dst = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, w, h);
        let sw = w.min(480);
        let sh = ((h as i64 * sw as i64 / w as i64) as i32).max(1);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = sw;
        info.bmiHeader.biHeight = -sh;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = std::ptr::null_mut();
        let small = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if !src.is_null() && !dst.is_null() && !bitmap.is_null() && !small.is_null() && !bits.is_null() {
            let old_src = SelectObject(src, bitmap);
            let old_dst = SelectObject(dst, small);
            if PrintWindow(window, src, 2) != 0 {
                SetStretchBltMode(dst, HALFTONE);
                StretchBlt(dst, 0, 0, sw, sh, src, 0, 0, w, h, SRCCOPY);
                let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (sw * sh) as usize);
                let nonempty = pixels.iter().any(|p| p & 0x00ff_ffff != 0);
                for pixel in pixels { *pixel |= 0xff00_0000; }
                if nonempty {
                    let mask = CreateBitmap(sw, sh, 1, 1, std::ptr::null());
                    let icon = CreateIconIndirect(&ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: small });
                    if !mask.is_null() { DeleteObject(mask); }
                    if !icon.is_null() {
                        made = Some((icon as isize, sw, sh));
                    }
                }
            }
            SelectObject(src, old_src);
            SelectObject(dst, old_dst);
        }
        if !small.is_null() { DeleteObject(small); }
        if !bitmap.is_null() { DeleteObject(bitmap); }
        if !src.is_null() { DeleteDC(src); }
        if !dst.is_null() { DeleteDC(dst); }
        ReleaseDC(std::ptr::null_mut(), dc);
        made
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_restored_window_throws_its_late_picture_away() {
        let t = begin(7);
        assert!(finish(7, t));
        assert!(!finish(7, t)); // stored once
        let t = begin(7);
        remove(7); // restored or destroyed before the picture arrived
        assert!(!finish(7, t));
        let (old, new) = (begin(7), begin(7)); // minimized again: only the newer picture counts
        assert!(!finish(7, old));
        assert!(finish(7, new));
    }
    #[test]
    fn snapshots_are_bounded_and_replacement_is_recent() {
        let mut shots = VecDeque::new();
        for i in 0..40 {
            let icon = Rc::new(OwnedIcon(windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut())));
            put(&mut shots, Shot { window: i, icon, width: 1, height: 1 });
        }
        assert_eq!(shots.len(), LIMIT);
        assert_eq!(shots.front().unwrap().window, 8);
        let icon = Rc::new(OwnedIcon(windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut())));
        put(&mut shots, Shot { window: 8, icon, width: 1, height: 1 });
        assert_eq!(shots.len(), LIMIT);
        assert_eq!(shots.back().unwrap().window, 8);
    }
}
