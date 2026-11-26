//! Live window thumbnails under a panel of per-pixel alpha.
//!
//! DWM draws a thumbnail (`DwmRegisterThumbnail`) as part of its destination window, and a layered window that is painted
//! with `UpdateLayeredWindow` is a bitmap that DWM composites as it is: a thumbnail registered on it is not drawn. So the
//! thumbnails live in a plain window that stands right under the panel's own: the panel cuts holes in itself where a
//! thumbnail belongs ([`super::panel::Hole`]), and DWM shows the thumbnails through them. The window's region is the union of
//! the thumbnails' rectangles, so nothing of it is seen but the pictures (a panel that fades does not show a black box).

use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HWND, RECT, SIZE};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
    DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
};
use windows_sys::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, RGN_OR, SetWindowRgn};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, SW_HIDE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow};

use super::panel::ScreenHole;

const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;

/// The size in device pixels of what a window's thumbnail shows (its visible frame: no invisible resize borders).
#[must_use]
pub fn source_size(w: isize) -> (i32, i32) {
    let r = frame_of(w);
    ((r.2 - r.0).max(1), (r.3 - r.1).max(1))
}

/// Check that DWM can supply a non-empty thumbnail before exposing a hole.
pub fn drawable(host: HWND, source: HWND) -> bool {
    // SAFETY: both handles are windows; the temporary registration is always released.
    unsafe {
        let mut id = 0isize;
        if DwmRegisterThumbnail(host, source, &mut id) != 0 { return false; }
        let mut size = SIZE { cx: 0, cy: 0 };
        let ok = DwmQueryThumbnailSourceSize(id, &mut size) == 0 && size.cx > 0 && size.cy > 0;
        DwmUnregisterThumbnail(id);
        ok
    }
}

/// The visible frame of a window on the screen (left, top, right, bottom).
#[must_use]
pub fn frame_of(w: isize) -> (i32, i32, i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: plain queries into a local.
    unsafe {
        let ok = DwmGetWindowAttribute(w as HWND, DWMWA_EXTENDED_FRAME_BOUNDS, (&raw mut r).cast(), size_of::<RECT>() as u32) == 0;
        if !ok || r.right <= r.left {
            GetWindowRect(w as HWND, &mut r);
        }
    }
    (r.left, r.top, r.right, r.bottom)
}

/// The part of window `w`'s thumbnail (`id`) that is its visible frame, and the flag that asks for it; nothing (all of it) when
/// the thumbnail is not the size of the window's rectangle, where the offsets would mean nothing.
unsafe fn visible_part(id: isize, w: isize) -> (RECT, u32) {
    let all = (RECT { left: 0, top: 0, right: 0, bottom: 0 }, 0);
    let mut sz = SIZE { cx: 0, cy: 0 };
    let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(w as HWND, &mut wr);
    if DwmQueryThumbnailSourceSize(id, &mut sz) != 0 || sz.cx != wr.right - wr.left || sz.cy != wr.bottom - wr.top {
        return all;
    }
    let f = frame_of(w);
    let r = RECT { left: f.0 - wr.left, top: f.1 - wr.top, right: f.2 - wr.left, bottom: f.3 - wr.top };
    if (r.left, r.top, r.right, r.bottom) == (0, 0, sz.cx, sz.cy) || r.left < 0 || r.top < 0 || r.right > sz.cx || r.bottom > sz.cy || r.right <= r.left || r.bottom <= r.top {
        return all;
    }
    (r, DWM_TNP_RECTSOURCE)
}

/// The thumbnails of the panel's holes and the window that shows them.
pub struct Thumbs {
    host: HWND,
    /// Source window → thumbnail id.
    reg: HashMap<isize, isize>,
    rects: Vec<(i32, i32, i32, i32)>,
    shown: bool,
}

impl Thumbs {
    #[must_use]
    pub fn new(host: HWND) -> Self {
        Self { host, reg: HashMap::new(), rects: Vec::new(), shown: false }
    }

    #[must_use]
    pub const fn host(&self) -> HWND {
        self.host
    }

    /// The thumbnails' window is on the screen.
    #[must_use]
    pub const fn visible(&self) -> bool {
        self.shown
    }

    /// Puts the thumbnails where `holes` are. True when the window has just appeared (its place in the z-order must be set).
    pub fn update(&mut self, holes: &[ScreenHole]) -> bool {
        let live: Vec<&ScreenHole> = holes.iter().filter(|h| h.rect.2 > h.rect.0 && h.rect.3 > h.rect.1 && h.opacity > 0).collect();
        let gone: Vec<isize> = self.reg.keys().copied().filter(|k| !live.iter().any(|h| h.win == *k)).collect();
        for k in gone {
            if let Some(id) = self.reg.remove(&k) {
                // SAFETY: the id came from DwmRegisterThumbnail.
                unsafe { DwmUnregisterThumbnail(id) };
            }
        }
        if live.is_empty() {
            self.hide();
            return false;
        }
        let (l, t) = live.iter().fold((i32::MAX, i32::MAX), |a, h| (a.0.min(h.rect.0), a.1.min(h.rect.1)));
        let (r, b) = live.iter().fold((i32::MIN, i32::MIN), |a, h| (a.0.max(h.rect.2), a.1.max(h.rect.3)));
        let rects: Vec<_> = live.iter().map(|h| h.rect).collect();
        let appeared = !self.shown;
        // SAFETY: our own window, regions and thumbnails; the region's ownership passes to the window.
        unsafe {
            if appeared || rects != self.rects {
                let rgn = CreateRectRgn(0, 0, 0, 0);
                for h in &live {
                    let part = CreateRectRgn(h.rect.0 - l, h.rect.1 - t, h.rect.2 - l, h.rect.3 - t);
                    CombineRgn(rgn, rgn, part, RGN_OR);
                    DeleteObject(part);
                }
                SetWindowRgn(self.host, rgn, 0);
                SetWindowPos(self.host, std::ptr::null_mut(), l, t, r - l, b - t, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
                self.rects = rects;
                self.shown = true;
            }
            for h in live {
                let id = match self.reg.get(&h.win) {
                    Some(&id) => id,
                    None => {
                        let mut id = 0isize;
                        if DwmRegisterThumbnail(self.host, h.win as HWND, &mut id) != 0 {
                            continue;
                        }
                        self.reg.insert(h.win, id);
                        id
                    }
                };
                // what the thumbnail shows is the window with its invisible resize borders; the picture is the visible frame
                let (src, flags) = visible_part(id, h.win);
                let p = DWM_THUMBNAIL_PROPERTIES {
                    dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY | flags,
                    rcDestination: RECT { left: h.rect.0 - l, top: h.rect.1 - t, right: h.rect.2 - l, bottom: h.rect.3 - t },
                    rcSource: src,
                    opacity: h.opacity,
                    fVisible: 1,
                    fSourceClientAreaOnly: 0,
                };
                DwmUpdateThumbnailProperties(id, &p);
            }
        }
        appeared
    }

    fn hide(&mut self) {
        if self.shown {
            self.shown = false;
            self.rects.clear();
            // SAFETY: hiding our own window.
            unsafe { ShowWindow(self.host, SW_HIDE) };
        }
    }

    /// Takes every thumbnail down and hides the window.
    pub fn clear(&mut self) {
        for (_, id) in self.reg.drain() {
            // SAFETY: the id came from DwmRegisterThumbnail.
            unsafe { DwmUnregisterThumbnail(id) };
        }
        self.hide();
    }
}
