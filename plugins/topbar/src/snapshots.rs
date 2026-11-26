use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
use crate::ui::taskview::OwnedIcon;

struct Shot { window: isize, icon: Rc<OwnedIcon>, width: i32, height: i32 }
thread_local! { static SHOTS: RefCell<VecDeque<Shot>> = const { RefCell::new(VecDeque::new()) }; }
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
    SHOTS.with_borrow_mut(|shots| shots.retain(|s| s.window != window));
}

pub unsafe fn capture(window: HWND) {
    // SAFETY: all GDI objects are local, selected objects are restored, and the icon owns a copy of its bitmaps.
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        // the event arrives after the fact: a window already minimized has nothing to show (a 160x28 strip off the screen),
        // and then its card keeps the icon
        if IsIconic(window) != 0 || GetWindowRect(window, &mut rect) == 0 || rect.left <= -32000 { return; }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 || w > 8192 || h > 8192 { return; }
        // PrintWindow waits for the window's own thread, and this runs on the bar's: a program that does not answer (it is the
        // one being minimized because it hangs, often) would freeze the bar with it. A probe that gives up after 100 ms asks first;
        // (ponytail: a program that hangs after the probe still holds the bar; PrintWindow on a thread of its own would not.)
        let mut answer = 0usize;
        if SendMessageTimeoutW(window, WM_NULL, 0, 0, SMTO_ABORTIFHUNG | SMTO_BLOCK, 100, &mut answer) == 0 { return; }
        let dc = GetDC(std::ptr::null_mut());
        if dc.is_null() { return; }
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
                        insert(window as isize, Rc::new(OwnedIcon(crate::wicon(icon))), sw, sh);
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
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
