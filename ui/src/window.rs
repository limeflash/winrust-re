//! The host of an application window: an ordinary top-level window whose caption, frame, corners and shadow are drawn by us.
//!
//! * The window is `WS_OVERLAPPEDWINDOW` with `WS_EX_NOREDIRECTIONBITMAP`: Windows still treats it as a window with a caption
//!   (minimize, maximize and close animations, Alt+Space, Aero Snap, the Snap Layouts flyout over a maximize button), while a
//!   DirectComposition surface is all there is to see. `WM_NCCALCSIZE` gives the whole top to the client and keeps the
//!   system's invisible resize borders on the other three sides, as Windows Terminal does, so a snapped window has no gaps.
//! * The corners are 16 DIPs, which Windows does not offer (it rounds by 8): the content is drawn square, its corners are made
//!   transparent ([`Canvas::round_corners`]), and the window's own rounding is switched off.
//! * The shadow is a second window under the first, click-through, that follows it ([`paint_shadow`]); the system's is not
//!   asked for (no frame is extended into the client area).
//! * Without DirectComposition (`WINRUST_ULW=1`, a machine that cannot) the window is an ordinary redirected one: the frame is
//!   painted into a DIB and copied at once, the corners are a window region, there is no shadow window.
//!
//! The app is a [`App`]; this module is only Win32: messages in, [`Core`] calls out.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{GlobalFree, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, ClientToScreen, EndPaint, GetDC, MONITOR_DEFAULTTONULL, MonitorFromRect, PAINTSTRUCT, ReleaseDC, ScreenToClient};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::Ime::{CANDIDATEFORM, CFS_CANDIDATEPOS, CFS_POINT, COMPOSITIONFORM, GCS_RESULTREADSTR, GCS_RESULTSTR, ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows_sys::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::anim;
use crate::core::{Act, App, Core, HostReq, Nc, RADIUS};
use crate::frame::{Button, Cursor, Mods, WIN_MAX, wm_char};
use crate::gfx::{self, Canvas, Gfx, Rect, Rgba, Rgn, Surface};
use crate::pacing;
use winrust_common::style;

/// How an app's window is made.
pub struct Options {
    /// The window class (one per app).
    pub class: &'static str,
    /// The client area's size, DIPs.
    pub size: (f32, f32),
    /// The icon's resource number in the exe (0: none).
    pub icon: u16,
    /// Where the window remembers its place between runs.
    pub place_file: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self { class: "winrust_app", size: (1160.0, 740.0), icon: 0, place_file: None }
    }
}

/// How far the shadow reaches round the window (the mockup's `0 40px 100px`: three standard deviations of the blur), DIPs, and how
/// far it is moved down.
pub const SHADOW_PAD: f32 = 150.0;
pub const SHADOW_DY: f32 = 40.0;

/// What the shadow window shows for a window of `w` × `h` DIPs, which stands at `(SHADOW_PAD, SHADOW_PAD)`: the soft shadow and
/// the 1-px ring just outside the window (the mockup's `0 0 0 1px rgba(0,0,0,.6)`).
pub fn paint_shadow(c: &mut Canvas<'_>, w: f32, h: f32) {
    let r = Rect::new(SHADOW_PAD, SHADOW_PAD, w, h);
    c.shadow(r, RADIUS, SHADOW_DY, 100.0, Rgba::css(0, 0, 0, 0.55));
    c.ring(r.inset(-1.0), RADIUS + 1.0, 1.0, Rgba::css(0, 0, 0, 0.6));
}

/// The size of the shadow window's picture for a window of `w` × `h` DIPs.
#[must_use]
pub fn shadow_extent(w: f32, h: f32) -> (f32, f32) {
    (w + 2.0 * SHADOW_PAD, h + 2.0 * SHADOW_PAD + SHADOW_DY)
}

// ---------------------------------------------------------------------------------------------------------------------
// the host

const T_WAKE: usize = 1;
const T_DUE: usize = 2;
const WM_MOUSELEAVE: u32 = 0x02A3;
const HT_CLIENT: LRESULT = 1;

pub(crate) struct Host {
    pub(crate) hwnd: HWND,
    shadow: HWND,
    pub(crate) core: Core,
    gfx: Gfx,
    surf: Surface,
    shadow_surf: Surface,
    composed: bool,
    dpi: u32,
    /// What the shadow was last painted for: its picture's size (device pixels), the window's, and the scale.
    shadow_key: (i32, i32, i32, i32, u32),
    shadow_shown: bool,
    pacing_on: bool,
    due: Option<u64>,
    tracking: (bool, bool),
    place: Option<PathBuf>,
    /// Frames drawn (the benchmark's and the self-test's counter).
    pub(crate) frames: u32,
    /// Where each frame's time went (the benchmark reads and clears it).
    pub(crate) spent: Vec<(f64, f64, f64)>,
    pub(crate) timing: bool,
    dark: Option<bool>,
    lost: bool,
}

thread_local! {
    static HOSTS: RefCell<HashMap<isize, Host>> = RefCell::new(HashMap::new());
    /// The mode of the window being made, for the messages it gets before its host is registered.
    static MAKING: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(crate) fn with<R>(hwnd: HWND, f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOSTS.with(|m| {
        let mut m = m.try_borrow_mut().ok()?;
        m.get_mut(&(hwnd as isize)).map(f)
    })
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn whwnd(h: HWND) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(h.cast())
}

/// The invisible resize border Windows keeps on three sides at this DPI, in pixels.
fn frame_px(dpi: u32) -> i32 {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a plain computation on a local rectangle.
    unsafe { AdjustWindowRectExForDpi(&mut r, WS_OVERLAPPEDWINDOW, 0, 0, dpi) };
    -r.left
}

fn client_px(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a query on our own window into a local.
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
}

/// Where the client area's top left corner is on the screen.
pub(crate) fn client_origin(hwnd: HWND) -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: a query on our own window into a local.
    unsafe { ClientToScreen(hwnd, &mut p) };
    (p.x, p.y)
}

fn set_dwm(hwnd: HWND, attr: i32, v: u32) {
    // SAFETY: a 4-byte attribute value on our own window.
    unsafe { DwmSetWindowAttribute(hwnd, attr as u32, (&raw const v).cast(), 4) };
}

fn is_zoomed(hwnd: HWND) -> bool {
    // SAFETY: a plain query.
    unsafe { IsZoomed(hwnd) != 0 }
}

impl Host {
    /// What a frame draws: the window's content, then (composed) the shadow window if its look changed.
    pub(crate) fn frame(&mut self) {
        // SAFETY: plain queries on our own window.
        if unsafe { IsIconic(self.hwnd) } != 0 {
            // minimized: no frames, whatever is still moving; restoring draws one
            if self.pacing_on {
                self.pacing_on = false;
                pacing::set(self.hwnd as isize, false);
            }
            return;
        }
        let (w, h) = client_px(self.hwnd);
        if w <= 0 || h <= 0 {
            return;
        }
        let s = self.core.scale;
        self.core.size = (w as f32 / s, h as f32 / s);
        self.core.maximized = is_zoomed(self.hwnd);
        let now = anim::now_ms();
        let t0 = std::time::Instant::now();
        gfx::bench_start();
        let core = &mut self.core;
        let r = self.gfx.paint(&mut self.surf, w, h, |c| core.draw(c, now));
        let end = gfx::bench_end();
        let drawn = t0.elapsed();
        self.frames += 1;
        if r.is_err() {
            self.lost = true;
        }
        let t1 = std::time::Instant::now();
        if self.composed {
            if !self.surf.show(whwnd(self.hwnd)) {
                self.lost = true;
            }
            self.sync_shadow();
            gfx::commit();
            if !self.surf.is_current() {
                self.lost = true;
            }
        } else {
            let round = !self.core.maximized;
            let rgn = Rgn { x: 0, y: 0, w, h, r: (RADIUS * s).round() as i32 };
            gfx::shape(whwnd(self.hwnd), round.then_some(std::slice::from_ref(&rgn)));
            // SAFETY: the window's own device context, released at once; the DIB holds the frame just painted.
            unsafe {
                let dc = GetDC(self.hwnd);
                self.surf.blit(windows::Win32::Graphics::Gdi::HDC(dc.cast()), 0, 0);
                ReleaseDC(self.hwnd, dc);
            }
        }
        if self.timing {
            self.spent.push((drawn.saturating_sub(end).as_secs_f64() * 1000.0, end.as_secs_f64() * 1000.0, t1.elapsed().as_secs_f64() * 1000.0));
        }
        self.apply_theme();
        self.arm();
    }

    /// The shadow window follows the visible window: its place, its z-order right under it, and its picture when the size changed.
    fn sync_shadow(&mut self) {
        if !self.composed || self.shadow.is_null() {
            return;
        }
        // SAFETY: plain queries on our own windows.
        let show = unsafe { IsWindowVisible(self.hwnd) != 0 && IsIconic(self.hwnd) == 0 && IsZoomed(self.hwnd) == 0 };
        if !show {
            if self.shadow_shown {
                // SAFETY: hiding our own window.
                unsafe { ShowWindow(self.shadow, SW_HIDE) };
                self.shadow_shown = false;
            }
            if self.core.maximized && self.shadow_key.0 != 0 {
                // a maximized window has no shadow: its picture (a few MB of video memory) is not kept
                self.shadow_surf = Surface::composed();
                self.shadow_key = (0, 0, 0, 0, 0);
            }
            return;
        }
        let s = self.core.scale;
        let (w, h) = client_px(self.hwnd);
        let (x, y) = client_origin(self.hwnd);
        let pad = (SHADOW_PAD * s).round() as i32;
        let (nw, nh) = (w + 2 * pad, h + 2 * pad + (SHADOW_DY * s).ceil() as i32);
        // the picture is a little larger than needed, so a window being resized does not make a new surface at every step
        let up = |v: i32| (v + 127) / 128 * 128;
        let key = (up(nw), up(nh), w, h, s.to_bits());
        if key != self.shadow_key || !self.shadow_surf.is_current() {
            let (dw, dh) = (w as f32 / s, h as f32 / s);
            let _ = self.gfx.paint(&mut self.shadow_surf, key.0, key.1, |c| paint_shadow(c, dw, dh));
            self.shadow_key = key;
        }
        let _ = self.shadow_surf.present(whwnd(self.shadow), x - pad, y - pad, 255);
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | if self.shadow_shown { 0 } else { SWP_SHOWWINDOW };
        // SAFETY: z-order of our own windows: right under the window it belongs to.
        unsafe { SetWindowPos(self.shadow, self.hwnd, 0, 0, 0, 0, flags) };
        self.shadow_shown = true;
    }

    /// Windows' dark mode for what it draws (the system menu); follows the mode of the app.
    fn apply_theme(&mut self) {
        let dark = !style::light();
        if self.dark != Some(dark) {
            self.dark = Some(dark);
            set_dwm(self.hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(dark));
        }
    }

    /// Frames while something moves, a timer for what is due while nothing does.
    fn arm(&mut self) {
        let now = anim::now_ms();
        let on = self.core.wants_frames(now);
        if on != self.pacing_on {
            self.pacing_on = on;
            pacing::set(self.hwnd as isize, on);
        }
        let due = self.core.deadline(now);
        if due != self.due {
            self.due = due;
            // SAFETY: a timer of our own window.
            unsafe {
                match due {
                    Some(t) => SetTimer(self.hwnd, T_DUE, (t - now).max(1) as u32, None),
                    None => KillTimer(self.hwnd, T_DUE).try_into().unwrap_or_default(),
                };
            }
        }
    }

    /// What the app asked of the window during a call, then the frame it needs.
    pub(crate) fn settle(&mut self) -> Vec<Deferred> {
        let mut later = Vec::new();
        for r in self.core.take_host() {
            match r {
                HostReq::Title(t) => {
                    // SAFETY: our own window; WM_SETTEXT is answered by DefWindowProc, which needs no host.
                    unsafe { SetWindowTextW(self.hwnd, wide(&t).as_ptr()) };
                }
                HostReq::Clipboard(t) => set_clipboard_text(self.hwnd as isize, &t),
                HostReq::Wake(ms) => {
                    // SAFETY: a timer of our own window.
                    unsafe { SetTimer(self.hwnd, T_WAKE, ms.max(1), None) };
                }
                HostReq::Close => later.push(Deferred::Close),
            }
        }
        if self.core.take_redraw() || self.lost || self.core.wants_frames(anim::now_ms()) {
            self.lost = false;
            self.frame();
        } else {
            self.arm();
        }
        later
    }

    fn set_dpi(&mut self, dpi: u32) {
        self.dpi = dpi;
        let s = dpi as f32 / 96.0;
        self.gfx.set_scale(s);
        self.core.scale = s;
        self.shadow_key = (0, 0, 0, 0, 0);
    }

    fn save_place(&self) {
        let Some(p) = self.place.as_ref() else { return };
        // SAFETY: a query on our own window into a local.
        let wp = unsafe {
            let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
            wp.length = size_of::<WINDOWPLACEMENT>() as u32;
            GetWindowPlacement(self.hwnd, &mut wp);
            wp
        };
        let r = wp.rcNormalPosition;
        let max = i32::from(wp.showCmd == SW_SHOWMAXIMIZED as u32);
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, format!("{} {} {} {} {max}", r.left, r.top, r.right, r.bottom));
    }
}

/// What must be done once the host is not borrowed: calls that send messages to the window itself.
pub(crate) enum Deferred {
    Close,
    Act(Act),
}

fn run_deferred(hwnd: HWND, later: Vec<Deferred>) {
    for d in later {
        // SAFETY: calls on our own window, made with no borrow of the host held (they send it messages).
        unsafe {
            match d {
                Deferred::Close | Deferred::Act(Act::Close) => {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }
                Deferred::Act(Act::Minimize) => {
                    ShowWindow(hwnd, SW_MINIMIZE);
                }
                Deferred::Act(Act::ToggleMaximize) => {
                    ShowWindow(hwnd, if IsZoomed(hwnd) != 0 { SW_RESTORE } else { SW_MAXIMIZE });
                }
            }
        }
    }
}

/// Runs `f` on the host of `hwnd` and does what it asked of the window; `f` says if the frame must be drawn anyway.
fn event(hwnd: HWND, f: impl FnOnce(&mut Host) -> Option<Act>) {
    let later = with(hwnd, |h| {
        let act = f(h);
        let mut later = h.settle();
        later.extend(act.map(Deferred::Act));
        later
    });
    if let Some(later) = later {
        run_deferred(hwnd, later);
    }
}

fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32)
}

fn mods() -> (Mods, bool) {
    // SAFETY: plain queries of the keyboard state.
    unsafe { (Mods { ctrl: GetKeyState(i32::from(VK_CONTROL)) < 0, shift: GetKeyState(i32::from(VK_SHIFT)) < 0 }, GetKeyState(i32::from(VK_MENU)) < 0) }
}

fn cursor_id(c: Cursor) -> *const u16 {
    match c {
        Cursor::Arrow => IDC_ARROW,
        Cursor::Hand => IDC_HAND,
        Cursor::IBeam => IDC_IBEAM,
        Cursor::SizeWE => IDC_SIZEWE,
        Cursor::SizeNS => IDC_SIZENS,
        Cursor::Move => IDC_SIZEALL,
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let now = anim::now_ms();
    if msg == style::msg() {
        style::reload();
        event(hwnd, |h| {
            h.core.ask_redraw();
            None
        });
        return 0;
    }
    match msg {
        WM_NCCALCSIZE => {
            // wParam TRUE: lParam points to the NCCALCSIZE_PARAMS of this call, whose first rectangle is the window going in and
            // the client coming out; FALSE: lParam is that one rectangle
            // SAFETY: as the message says.
            unsafe {
                let rect = if wp != 0 { &raw mut (*(lp as *mut NCCALCSIZE_PARAMS)).rgrc[0] } else { lp as *mut RECT };
                let before = *rect;
                let r = DefWindowProcW(hwnd, msg, wp, lp);
                if r != 0 {
                    return r;
                }
                let dpi = GetDpiForWindow(hwnd);
                let composed = with(hwnd, |h| h.composed).unwrap_or_else(|| MAKING.get());
                let rect = &mut *rect;
                rect.top = before.top;
                if IsZoomed(hwnd) != 0 {
                    // the window overhangs the monitor by its frame: the client starts at the monitor's edge
                    rect.top += GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                } else if !composed {
                    *rect = before; // without the invisible borders the whole window is the client; we draw the resize strips
                }
                return 0;
            }
        }
        WM_NCHITTEST => {
            // SAFETY: the system's own answer for the frame's invisible borders first.
            let def = unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            if def != HT_CLIENT {
                return def;
            }
            let (sx, sy) = xy(lp);
            let mut p = POINT { x: sx, y: sy };
            // SAFETY: a conversion of a local point for our own window.
            unsafe { ScreenToClient(hwnd, &mut p) };
            let nc = with(hwnd, |h| {
                let s = h.core.scale;
                h.core.nc_test(p.x as f32 / s, p.y as f32 / s, !h.composed)
            });
            return match nc {
                Some(n) => ht_of(n),
                None => def,
            };
        }
        // the frame is drawn by us: nothing of the system's, and no repaint when the activation changes
        WM_NCACTIVATE => return unsafe { DefWindowProcW(hwnd, msg, wp, -1) },
        WM_NCPAINT | WM_ERASEBKGND => return 1,
        WM_PAINT => {
            // SAFETY: validates our own window's update region and, without DirectComposition, blits the last frame.
            unsafe {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                with(hwnd, |h| {
                    if !h.composed {
                        h.surf.blit(windows::Win32::Graphics::Gdi::HDC(dc.cast()), 0, 0);
                    }
                });
                EndPaint(hwnd, &ps);
            }
            return 0;
        }
        WM_GETMINMAXINFO => {
            if let Some((mw, mh, fr, composed)) = with(hwnd, |h| {
                let (mw, mh) = h.core.app.min_size();
                (mw * h.core.scale, mh * h.core.scale, frame_px(h.dpi), h.composed)
            }) {
                // SAFETY: lParam points to this message's MINMAXINFO.
                unsafe {
                    let mm = lp as *mut MINMAXINFO;
                    let side = if composed { 2 * fr } else { 0 };
                    (*mm).ptMinTrackSize = POINT { x: mw.round() as i32 + side, y: mh.round() as i32 + if composed { fr } else { 0 } };
                }
                return 0;
            }
        }
        WM_SIZE => {
            let minimized = wp as u32 == SIZE_MINIMIZED;
            event(hwnd, |h| {
                if minimized {
                    if h.shadow_shown {
                        // SAFETY: hiding our own window.
                        unsafe { ShowWindow(h.shadow, SW_HIDE) };
                        h.shadow_shown = false;
                    }
                } else {
                    h.frame();
                }
                None
            });
            return 0;
        }
        WM_WINDOWPOSCHANGED => {
            // the shadow goes where the window went (and over or under whatever the window now stands over or under)
            with(hwnd, |h| {
                if h.core.size.0 > 0.0 {
                    h.sync_shadow();
                    gfx::commit();
                }
            });
        }
        WM_SHOWWINDOW => {
            if wp == 0 {
                with(hwnd, |h| {
                    if h.shadow_shown {
                        // SAFETY: hiding our own window.
                        unsafe { ShowWindow(h.shadow, SW_HIDE) };
                        h.shadow_shown = false;
                    }
                });
            }
        }
        WM_DPICHANGED => {
            // SAFETY: lParam points to the rectangle Windows suggests for the new DPI.
            let r = unsafe { *(lp as *const RECT) };
            let dpi = (wp & 0xffff) as u32;
            with(hwnd, |h| h.set_dpi(dpi));
            // SAFETY: our own window, made with no borrow of the host held (it sends WM_SIZE, which draws the frame).
            unsafe { SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE) };
            return 0;
        }
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            let track = with(hwnd, |h| std::mem::replace(&mut h.tracking.0, true)) == Some(false);
            if track {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                // SAFETY: asks for WM_MOUSELEAVE for our own window.
                unsafe { TrackMouseEvent(&mut t) };
            }
            event(hwnd, |h| {
                let s = h.core.scale;
                if h.core.pointer_move(x as f32 / s, y as f32 / s, now) {
                    h.core.ask_redraw();
                }
                None
            });
            return 0;
        }
        WM_MOUSELEAVE => {
            event(hwnd, |h| {
                h.tracking.0 = false;
                // the maximize button is the system's: the pointer is still "in" it when the client says it left
                let over_max = h.core.hits().get(WIN_MAX).zip(h.core.pointer).is_some_and(|(b, (x, y))| b.r.contains(x, y));
                if !over_max && h.core.pointer_leave(now) {
                    h.core.ask_redraw();
                }
                None
            });
            return 0;
        }
        WM_NCMOUSEMOVE => {
            let on_max = wp as u32 == HTMAXBUTTON;
            let track = on_max && with(hwnd, |h| std::mem::replace(&mut h.tracking.1, true)) == Some(false);
            if track {
                let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE | TME_NONCLIENT, hwndTrack: hwnd, dwHoverTime: 0 };
                // SAFETY: asks for WM_NCMOUSELEAVE for our own window.
                unsafe { TrackMouseEvent(&mut t) };
            }
            event(hwnd, |h| {
                if h.core.set_hover(on_max.then_some(WIN_MAX), now) {
                    h.core.ask_redraw();
                }
                None
            });
        }
        WM_NCMOUSELEAVE => {
            event(hwnd, |h| {
                h.tracking.1 = false;
                h.core.set_press(None);
                if h.core.set_hover(None, now) {
                    h.core.ask_redraw();
                }
                None
            });
            return 0;
        }
        // the maximize button belongs to Windows' hit test, so its press and release arrive as non-client messages; the system's
        // own handling would draw its old button and run a modal loop, so both are ours
        WM_NCLBUTTONDOWN if wp as u32 == HTMAXBUTTON => {
            event(hwnd, |h| {
                h.core.set_press(Some(WIN_MAX));
                h.core.ask_redraw();
                None
            });
            return 0;
        }
        WM_NCLBUTTONUP if wp as u32 == HTMAXBUTTON => {
            let was = with(hwnd, |h| h.core.pressed() == Some(WIN_MAX)).unwrap_or(false);
            event(hwnd, |h| {
                h.core.set_press(None);
                h.core.ask_redraw();
                None
            });
            if was {
                run_deferred(hwnd, vec![Deferred::Act(Act::ToggleMaximize)]);
            }
            return 0;
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (x, y) = xy(lp);
            let b = match msg {
                WM_LBUTTONDOWN => Button::Left,
                WM_RBUTTONDOWN => Button::Right,
                _ => Button::Middle,
            };
            // SAFETY: capturing the mouse for our own window: a held item follows the pointer beyond it.
            unsafe { SetCapture(hwnd) };
            let (m, _) = mods();
            event(hwnd, |h| {
                h.core.held = m;
                let s = h.core.scale;
                h.core.down(b, x as f32 / s, y as f32 / s, now);
                h.core.ask_redraw();
                None
            });
            return 0;
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let (x, y) = xy(lp);
            let b = match msg {
                WM_LBUTTONUP => Button::Left,
                WM_RBUTTONUP => Button::Right,
                _ => Button::Middle,
            };
            // SAFETY: releases the capture taken above (a no-op without one).
            unsafe { ReleaseCapture() };
            let (m, _) = mods();
            event(hwnd, |h| {
                h.core.held = m;
                let s = h.core.scale;
                let act = h.core.up(b, x as f32 / s, y as f32 / s, now);
                h.core.ask_redraw();
                act
            });
            return 0;
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let delta = f32::from((wp >> 16) as i16) / 120.0;
            let (m, _) = mods();
            event(hwnd, |h| {
                if msg == WM_MOUSEWHEEL {
                    h.core.wheel(0.0, delta, m, now);
                } else {
                    h.core.wheel(delta, 0.0, m, now);
                }
                None
            });
            return 0;
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let (m, alt) = mods();
            let used = with(hwnd, |h| h.core.key(wp as u32, m, alt, now)).unwrap_or(false);
            if used {
                event(hwnd, |_| None);
                return 0;
            }
        }
        WM_CHAR => {
            if let Some(ch) = wm_char(wp).filter(|c| !c.is_control()) {
                event(hwnd, |h| {
                    h.core.text(ch.encode_utf8(&mut [0; 4]), now);
                    None
                });
            }
            return 0;
        }
        // the IME composes in its own windows, at the focused text field's caret; what it composed arrives here as one string (the
        // default would send it as WM_CHARs a unit at a time, and a character beyond the BMP as two)
        WM_IME_STARTCOMPOSITION => place_ime(hwnd),
        WM_IME_COMPOSITION => {
            place_ime(hwnd);
            if lp as u32 & GCS_RESULTSTR != 0 {
                if let Some(s) = ime_result(hwnd) {
                    event(hwnd, |h| {
                        h.core.text(&s, now);
                        None
                    });
                }
                // SAFETY: the default procedure of our own window; it draws the composition that is left, and the result is not sent twice.
                return unsafe { DefWindowProcW(hwnd, msg, wp, lp & !((GCS_RESULTSTR | GCS_RESULTREADSTR) as LPARAM)) };
            }
        }
        // Alt and a letter would look for a menu bar's mnemonic and beep: there is none (Alt+Space, the system menu, passes)
        WM_SYSCOMMAND if (wp & 0xfff0) as u32 == SC_KEYMENU && lp != i32::from(b' ') as LPARAM => return 0,
        WM_SETFOCUS | WM_KILLFOCUS => {
            event(hwnd, |h| {
                h.core.active = msg == WM_SETFOCUS;
                h.core.ask_redraw();
                None
            });
        }
        WM_SETCURSOR if (lp & 0xffff) as u32 == HTCLIENT => {
            if let Some(c) = with(hwnd, |h| h.core.cursor()) {
                // SAFETY: a stock cursor.
                unsafe { SetCursor(LoadCursorW(null_mut(), cursor_id(c))) };
                return 1;
            }
        }
        WM_SETTINGCHANGE => {
            // SAFETY: lParam is the message's own: null, or a NUL-terminated string.
            if unsafe { style::is_color_change(lp) } {
                style::set_light(style::follow_system());
                event(hwnd, |h| {
                    h.core.ask_redraw();
                    None
                });
            }
        }
        WM_TIMER => {
            let id = wp;
            // SAFETY: both timers are one-shot.
            if id == T_WAKE {
                unsafe { KillTimer(hwnd, T_WAKE) };
                event(hwnd, |h| {
                    h.core.wake(now);
                    None
                });
            } else if id == T_DUE {
                unsafe { KillTimer(hwnd, T_DUE) };
                event(hwnd, |h| {
                    h.due = None;
                    h.core.ask_redraw();
                    None
                });
            }
            return 0;
        }
        WM_DROPFILES => {
            let hd = wp as HDROP;
            // SAFETY: `hd` is the drop handle this message carries; it is released once.
            let (paths, p) = unsafe {
                let n = DragQueryFileW(hd, u32::MAX, null_mut(), 0);
                let list: Vec<String> = (0..n)
                    .map(|i| {
                        let len = DragQueryFileW(hd, i, null_mut(), 0) as usize;
                        let mut b = vec![0u16; len + 1];
                        DragQueryFileW(hd, i, b.as_mut_ptr(), b.len() as u32);
                        String::from_utf16_lossy(&b[..len])
                    })
                    .collect();
                let mut pt = POINT { x: 0, y: 0 };
                windows_sys::Win32::UI::Shell::DragQueryPoint(hd, &mut pt);
                DragFinish(hd);
                (list, pt)
            };
            event(hwnd, |h| {
                let s = h.core.scale;
                h.core.files(paths, p.x as f32 / s, p.y as f32 / s, now);
                None
            });
            return 0;
        }
        pacing::FRAME => {
            if pacing::received(hwnd as isize) {
                event(hwnd, |h| {
                    h.core.ask_redraw();
                    None
                });
            }
            return 0;
        }
        WM_APP..=0x8063 => {
            event(hwnd, |h| {
                h.core.message(msg, wp, lp, now);
                None
            });
            return 0;
        }
        WM_CLOSE => {
            let ok = with(hwnd, |h| {
                h.save_place();
                h.core.may_close(now)
            });
            let later = with(hwnd, Host::settle).unwrap_or_default();
            run_deferred(hwnd, later);
            if ok != Some(false) {
                // SAFETY: our own window.
                unsafe { DestroyWindow(hwnd) };
            }
            return 0;
        }
        WM_DESTROY => {
            pacing::set(hwnd as isize, false);
            let host = HOSTS.with_borrow_mut(|m| m.remove(&(hwnd as isize)));
            if let Some(h) = host {
                // SAFETY: the shadow window is ours; the host's surfaces go off their visuals with it.
                unsafe { DestroyWindow(h.shadow) };
                drop(h);
            }
            if HOSTS.with_borrow(HashMap::is_empty) {
                // SAFETY: ends the message loop of this thread.
                unsafe { PostQuitMessage(0) };
            }
            return 0;
        }
        _ => {}
    }
    // SAFETY: the default procedure of our own window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Puts the IME's composition and candidate windows at the caret of the focused text field.
fn place_ime(hwnd: HWND) {
    let Some((Some(r), s)) = with(hwnd, |h| (h.core.ime_caret(), h.core.scale)) else { return };
    let (x, y, below) = ((r.x * s).round() as i32, (r.y * s).round() as i32, ((r.y + r.h) * s).round() as i32 + 2);
    // SAFETY: the window's own input context, released at once; the forms are locals.
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.is_null() {
            return;
        }
        let zero = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let comp = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x, y }, rcArea: zero };
        ImmSetCompositionWindow(himc, &comp);
        let cand = CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_CANDIDATEPOS, ptCurrentPos: POINT { x, y: below }, rcArea: zero };
        ImmSetCandidateWindow(himc, &cand);
        ImmReleaseContext(hwnd, himc);
    }
}

/// What the IME has just composed (`GCS_RESULTSTR`).
fn ime_result(hwnd: HWND) -> Option<String> {
    // SAFETY: the window's own input context, released at once; the buffer is `len` bytes as the call asked.
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.is_null() {
            return None;
        }
        let len = ImmGetCompositionStringW(himc, GCS_RESULTSTR, null_mut(), 0);
        let s = (len > 0).then(|| {
            let mut buf = vec![0u16; len as usize / 2];
            ImmGetCompositionStringW(himc, GCS_RESULTSTR, buf.as_mut_ptr().cast(), len as u32);
            String::from_utf16_lossy(&buf)
        });
        ImmReleaseContext(hwnd, himc);
        s
    }
}

fn ht_of(n: Nc) -> LRESULT {
    (match n {
        Nc::Client => HTCLIENT,
        Nc::Caption => HTCAPTION,
        Nc::MaxButton => HTMAXBUTTON,
        Nc::Top => HTTOP,
        Nc::TopLeft => HTTOPLEFT,
        Nc::TopRight => HTTOPRIGHT,
        Nc::Left => HTLEFT,
        Nc::Right => HTRIGHT,
        Nc::Bottom => HTBOTTOM,
        Nc::BottomLeft => HTBOTTOMLEFT,
        Nc::BottomRight => HTBOTTOMRIGHT,
    }) as LRESULT
}

// ---------------------------------------------------------------------------------------------------------------------
// making the window

fn read_place(p: &PathBuf) -> Option<(RECT, bool)> {
    let s = std::fs::read_to_string(p).ok()?;
    let v: Vec<i32> = s.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    let [l, t, r, b, m] = v[..] else { return None };
    let rect = RECT { left: l, top: t, right: r, bottom: b };
    // a monitor that is gone leaves the window nowhere: then the system places it
    // SAFETY: a query with a local rectangle.
    (r - l > 100 && b - t > 100 && unsafe { !MonitorFromRect(&rect, MONITOR_DEFAULTTONULL).is_null() }).then_some((rect, m == 1))
}

/// Makes the window of an app (hidden until its first frame is drawn, so there is no white or black flash). `composed` picks
/// DirectComposition (`gfx::compose_windows()` says whether the machine can); `at` puts it somewhere (the benchmark and the
/// self-tests keep it off the screen).
pub(crate) fn create(app: Box<dyn App>, o: &Options, composed: bool, at: Option<(i32, i32)>) -> Option<HWND> {
    // SAFETY: class and window creation with live, NUL-terminated names; every handle is ours.
    unsafe {
        let hi: HINSTANCE = GetModuleHandleW(null());
        let class = wide(o.class);
        crate::frame::register_class(hi, &class, Some(proc), true);
        let dpi = GetDpiForSystem();
        let scale = dpi as f32 / 96.0;
        let ex = if composed { WS_EX_NOREDIRECTIONBITMAP } else { 0 };
        let fr = if composed { frame_px(dpi) } else { 0 };
        let (cw, ch) = ((o.size.0 * scale).round() as i32, (o.size.1 * scale).round() as i32);
        let (mut x, mut y, mut w, mut h) = (CW_USEDEFAULT, CW_USEDEFAULT, cw + 2 * fr, ch + fr);
        let mut maxed = false;
        if let Some((r, m)) = o.place_file.as_ref().and_then(read_place) {
            (x, y, w, h, maxed) = (r.left, r.top, r.right - r.left, r.bottom - r.top, m);
        }
        if let Some((ax, ay)) = at {
            (x, y) = (ax, ay);
        }
        let title = app.title();
        MAKING.set(composed);
        let hwnd = CreateWindowExW(ex, class.as_ptr(), wide(&title).as_ptr(), WS_OVERLAPPEDWINDOW, x, y, w, h, null_mut(), null_mut(), hi, null());
        if hwnd.is_null() {
            return None;
        }
        let shadow = if composed {
            let name = wide(&format!("{}_shadow", o.class));
            crate::frame::register_class(hi, &name, Some(DefWindowProcW), false);
            let s = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | gfx::window_ex_through(), name.as_ptr(), null(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), hi, null());
            if !s.is_null() {
                gfx::through(whwnd(s));
            }
            s
        } else {
            null_mut()
        };
        if o.icon != 0 {
            let ic = LoadImageW(hi, o.icon as usize as *const u16, IMAGE_ICON, 0, 0, LR_DEFAULTSIZE);
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, ic as isize);
            let small = LoadImageW(hi, o.icon as usize as *const u16, IMAGE_ICON, GetSystemMetricsForDpi(SM_CXSMICON, dpi), GetSystemMetricsForDpi(SM_CYSMICON, dpi), 0);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
        // Windows rounds its own windows by 8: ours are rounded by 16, drawn; and a border of its own would show on the edge
        set_dwm(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, 1); // DWMWCP_DONOTROUND
        set_dwm(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE);
        if !composed {
            let m = MARGINS { cxLeftWidth: 0, cxRightWidth: 0, cyTopHeight: 1, cyBottomHeight: 0 };
            DwmExtendFrameIntoClientArea(hwnd, &m); // the system's shadow, the only one this path has
        }
        DragAcceptFiles(hwnd, 1);
        let real = GetDpiForWindow(hwnd);
        let Ok(gfx) = Gfx::new(real as f32 / 96.0) else {
            DestroyWindow(hwnd);
            return None;
        };
        let mut core = Core::new(app, o.size, real as f32 / 96.0, anim::now_ms());
        core.hwnd = hwnd as isize;
        let host = Host {
            hwnd,
            shadow,
            core,
            gfx,
            surf: if composed { Surface::composed() } else { Surface::new() },
            shadow_surf: Surface::composed(),
            composed,
            dpi: real,
            shadow_key: (0, 0, 0, 0, 0),
            shadow_shown: false,
            pacing_on: false,
            due: None,
            tracking: (false, false),
            place: o.place_file.clone(),
            frames: 0,
            spent: Vec::new(),
            timing: false,
            dark: None,
            lost: false,
        };
        HOSTS.with_borrow_mut(|m| m.insert(hwnd as isize, host));
        if real != dpi {
            // made on a screen of another DPI than the system's: the size was worked out for the wrong one
            let f2 = if composed { frame_px(real) } else { 0 };
            let k = real as f32 / 96.0;
            SetWindowPos(hwnd, null_mut(), 0, 0, (o.size.0 * k).round() as i32 + 2 * f2, (o.size.1 * k).round() as i32 + f2, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        with(hwnd, |h| {
            h.frame();
            gfx::commit();
        });
        if maxed {
            ShowWindow(hwnd, SW_SHOWMAXIMIZED);
        }
        Some(hwnd)
    }
}

/// Opens the window, runs the message loop until it closes, and returns the exit code. COM and the DPI awareness are set up
/// here; call [`quit`] with the code to leave without the loader's teardown.
pub fn run(app: Box<dyn App>, o: Options) -> i32 {
    // SAFETY: process-wide setup before any window exists; failures (a manifest already said it) are harmless.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    style::follow_system();
    let composed = gfx::compose_windows();
    let Some(hwnd) = create(app, &o, composed, None) else { return 1 };
    // SAFETY: the loop of our own thread; the first frame is on the surface, so showing the window cannot flash.
    unsafe {
        if IsZoomed(hwnd) == 0 {
            ShowWindow(hwnd, SW_SHOW);
        }
        let mut m: MSG = std::mem::zeroed();
        while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        m.wParam as i32
    }
}

/// Ends the process without the loader's teardown: the Direct3D / Direct2D objects the main thread's thread-locals hold are
/// released in `DLL_PROCESS_DETACH`, after DXGI has been torn down, and that raised an exception (the bar's 0x87A).
pub fn quit(code: u32) -> ! {
    let _ = std::io::Write::flush(&mut std::io::stdout());
    // SAFETY: ends this process.
    unsafe { windows_sys::Win32::System::Threading::TerminateProcess(windows_sys::Win32::System::Threading::GetCurrentProcess(), code) };
    std::process::exit(code as i32)
}

/// Posts a message to a window from a worker thread (`WM_APP + 1 ..= WM_APP + 99`); `App::message` gets it.
pub fn post(hwnd: isize, msg: u32, wp: usize, lp: isize) {
    // SAFETY: posting to a window; one that is gone just drops the message.
    unsafe { PostMessageW(hwnd as HWND, msg, wp, lp) };
}

// ---------------------------------------------------------------------------------------------------------------------
// the clipboard

/// The clipboard's text, if it holds any.
#[must_use]
pub fn clipboard_text(hwnd: isize) -> Option<String> {
    // SAFETY: the clipboard is opened for the time of the copy; the global memory is read while locked.
    unsafe {
        if OpenClipboard(hwnd as HWND) == 0 {
            return None;
        }
        let h = GetClipboardData(13); // CF_UNICODETEXT
        let text = if h.is_null() {
            None
        } else {
            let p = GlobalLock(h).cast::<u16>();
            let s = (!p.is_null()).then(|| {
                let len = (0..).take_while(|&i| *p.add(i) != 0).count();
                String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
            });
            GlobalUnlock(h);
            s
        };
        CloseClipboard();
        text
    }
}

pub fn set_clipboard_text(hwnd: isize, text: &str) {
    let w = wide(text);
    // SAFETY: the clipboard is opened for the time of the call; the memory is given to it when it takes it, freed otherwise.
    unsafe {
        if OpenClipboard(hwnd as HWND) == 0 {
            return;
        }
        EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        if !mem.is_null() {
            let p = GlobalLock(mem).cast::<u16>();
            if p.is_null() {
                GlobalFree(mem);
            } else {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(mem);
                if SetClipboardData(13, mem).is_null() {
                    GlobalFree(mem);
                }
            }
        }
        CloseClipboard();
    }
}

/// Bench and self-test helper: asks `hwnd` for a repaint of itself.
pub(crate) fn invalidate(hwnd: HWND) {
    event(hwnd, |h| {
        h.core.ask_redraw();
        None
    });
}
