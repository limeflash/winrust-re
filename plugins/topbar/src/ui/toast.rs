//! The toast: a short notice that stands over the bar for a moment (the mockup's `.toast2`). A pill 40 px high with a check
//! mark in the accent colour and one line of text, on the dark glass tint (`rgba(16,17,24,.9)`), radius 20, a white ring at
//! .1, `0 16px 40px` shadow. It plays `t2` for 2.4 s: it fades in and settles over the first 12 %, rests until 85 %, then
//! fades out while it drifts 6 px away; each leg eases with `cubic-bezier(.2,.9,.1,1)`.
//!
//! A shared component: any panel or bar action can call [`show`]. [`Toast`] is the windowless core; [`show`] and [`hide`]
//! drive the one real window (click-through, never active; composed, or layered without DirectComposition), so a panel that is open keeps the focus.

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_TOPMOST, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    KillTimer, SetTimer, SetWindowPos, ShowWindow, WM_TIMER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use winrust_common::style::{self, Anim, Side, pal};

use super::anim::{self, SNAP};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba, Surface};

const FONT: Font = Font::new(13.0, 500);
const TINT: Rgba = Rgba::css(16, 17, 24, 0.9);
const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const CHECK: &str = "M5 12.5l4.5 4.5L19 7.5";
const HEIGHT: f32 = 40.0;
/// Room around the pill for its shadow (`0 16px 40px`) and the drift, DIPs.
const ROOM: f32 = 84.0;
/// The gap between the bar and the pill.
const GAP: f32 = 12.0;
/// `t2` lasts this long, ms.
pub const LIFE: u64 = 2400;

/// `t2`: opacity and the drift (DIPs down) `age` ms in.
fn keyframes(age: u64) -> (f32, f32) {
    let p = age as f32 / LIFE as f32;
    if p < 0.12 {
        let e = SNAP.ease(p / 0.12);
        (e, anim::lerp(-10.0, 0.0, e))
    } else if p < 0.85 {
        (1.0, 0.0)
    } else {
        let e = SNAP.ease((p - 0.85) / 0.15);
        (1.0 - e, anim::lerp(0.0, -6.0, e))
    }
}

/// The pill's state and painting, without a window.
pub struct Toast {
    text: String,
    width: f32,
    side: Side,
    /// The bar where it rests, device pixels.
    bar: (i32, i32, i32, i32),
    screen: (i32, i32),
    /// The top left corner of the monitor it is shown on (the screen above is that monitor's size).
    origin: (i32, i32),
    scale: f32,
    since: Option<u64>,
    surf: Surface,
    painted: Option<(i32, i32, u32)>,
}

impl Toast {
    #[must_use]
    pub fn new(scale: f32, screen: (i32, i32)) -> Self {
        Self {
            text: String::new(),
            width: 0.0,
            side: Side::Bottom,
            bar: (0, 0, 0, 0),
            screen,
            origin: (0, 0),
            scale,
            since: None,
            surf: Surface::composed(),
            painted: None,
        }
    }

    /// Shows `text` over the bar (the one on screen is replaced and the animation starts again).
    pub fn show(&mut self, g: &mut Gfx, text: &str, side: Side, bar: (i32, i32, i32, i32), now: u64) {
        self.text = text.to_string();
        // padding 16 + icon 16 + gap 10 + text + padding 16
        self.width = (g.measure(text, FONT) + 58.0).ceil();
        self.side = side;
        self.bar = bar;
        self.since = Some(now);
        self.painted = None;
    }

    pub fn hide(&mut self) {
        self.since = None;
    }

    /// The pill is shown on the monitor `mon` (left, top, width, height, device pixels) from now on.
    pub fn on(&mut self, mon: (i32, i32, i32, i32)) {
        self.origin = (mon.0, mon.1);
        self.screen = (mon.2, mon.3);
    }

    /// The pill is moving (it comes in over the first 12 % of its life and goes out over the last 15 %): frames on every tick of
    /// the screen. While it rests nothing changes: [`Toast::wake`] says when to look again.
    #[must_use]
    pub fn busy(&self, now: u64) -> bool {
        self.since.is_some_and(|t| {
            let age = now.saturating_sub(t);
            age < LIFE && style::anim() == Anim::Full && (age * 100 < LIFE * 12 || age * 100 >= LIFE * 85)
        })
    }

    /// When the pill has to be looked at again though it stands still until then: when it starts to leave (or, with the motion off,
    /// when it is over).
    #[must_use]
    pub fn wake(&self, now: u64) -> Option<u64> {
        let t = self.since?;
        if now.saturating_sub(t) >= LIFE || self.busy(now) {
            return None;
        }
        Some(t + if style::anim() == Anim::Full { LIFE * 85 / 100 } else { LIFE })
    }

    /// Opacity and the drift (DIPs down) at `now`: the keyframes of `t2`. A reduced or off motion setting keeps the pill still.
    fn pose(&self, now: u64) -> Option<(f32, f32)> {
        let age = now.saturating_sub(self.since?);
        if age >= LIFE {
            return None;
        }
        if style::anim() != Anim::Full {
            return Some((1.0, 0.0));
        }
        Some(keyframes(age))
    }

    /// Where the pill rests: centred on the room the bar leaves along the screen's bottom, `GAP` above a bottom bar.
    fn rest(&self) -> (f32, f32) {
        let s = self.scale;
        let (sw, sh) = (self.screen.0 as f32 / s, self.screen.1 as f32 / s);
        let (ox, oy) = (self.origin.0, self.origin.1);
        let (l, t, r, _) = ((self.bar.0 - ox) as f32 / s, (self.bar.1 - oy) as f32 / s, (self.bar.2 - ox) as f32 / s, self.bar.3);
        let (cx, bottom) = match self.side {
            Side::Bottom => ((l + r) / 2.0, t - GAP),
            Side::Left => ((r + sw) / 2.0, sh - GAP),
            Side::Right => (l / 2.0, sh - GAP),
        };
        (cx - self.width / 2.0 + ox as f32 / s, bottom - HEIGHT + oy as f32 / s)
    }

    /// Paints the pill at `now`. Returns the window rectangle in device pixels, whether the window should be shown, and its
    /// opacity (the fade is the window's, not repainted).
    pub fn frame(&mut self, g: &mut Gfx, now: u64) -> ((i32, i32, i32, i32), bool, u8) {
        let Some((o, dy)) = self.pose(now) else {
            self.since = None;
            return ((0, 0, 0, 0), false, 0);
        };
        let s = self.scale;
        let (bx, by) = self.rest();
        let (wx, wy) = (((bx - ROOM) * s).round() as i32, ((by - ROOM) * s).round() as i32);
        let (ww, wh) = (((self.width + 2.0 * ROOM) * s).round() as i32, ((HEIGHT + 2.0 * ROOM) * s).round() as i32);
        let key = (ww, wh, dy.to_bits());
        if self.painted != Some(key) || !self.surf.is_current() {
            self.painted = Some(key);
            let r = Rect::new(ROOM, ROOM + dy, self.width, HEIGHT);
            let text = &self.text;
            let accent = Rgba::colorref(pal().accent);
            let _ = g.paint(&mut self.surf, ww, wh, |c: &mut Canvas<'_>| {
                c.shadow(r, 20.0, 16.0, 40.0, Rgba::css(0, 0, 0, 0.5));
                c.ring(r.inset(-1.0), 21.0, 1.0, Rgba::css(0, 0, 0, 0.5));
                c.erase(r, 20.0);
                c.fill(r, 20.0, TINT);
                c.ring(r, 20.0, 1.0, Rgba::white(0.1));
                c.icon(CHECK, (r.x + 16.0 + 8.0, r.cy()), 16.0, 2.4, accent, 0.0);
                c.text(text, Rect::new(r.x + 42.0, r.y, self.width - 42.0 - 16.0 + 8.0, HEIGHT), FONT, INK, Align::Start);
            });
        }
        ((wx, wy, ww, wh), true, (255.0 * o.clamp(0.0, 1.0)).round() as u8)
    }

    #[must_use]
    pub const fn surface(&self) -> &Surface {
        &self.surf
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the window

struct Host {
    toast: Toast,
    gfx: Gfx,
    wnd: HWND,
    shown: bool,
    timer: bool,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|h| h.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn now() -> u64 {
    anim::now_ms()
}

/// Creates the toast's window; `screen` is the primary monitor in device pixels.
pub fn init(hi: HINSTANCE, scale: f32, screen: (i32, i32)) -> bool {
    let Ok(gfx) = Gfx::new(scale) else { return false };
    let name: Vec<u16> = "topbar_toast\0".encode_utf16().collect();
    super::panel::register_class(hi, &name, Some(proc), false);
    // SAFETY: window creation with a live, NUL-terminated class name.
    let wnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | super::gfx::window_ex_through(),
            name.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hi,
            std::ptr::null(),
        )
    };
    if wnd.is_null() {
        return false;
    }
    super::gfx::through(super::panel::whwnd(wnd));
    HOST.set(Some(Host { toast: Toast::new(scale, screen), gfx, wnd, shown: false, timer: false }));
    true
}

/// Shows `text` over the bar (`bar`: where it rests, device pixels) on the monitor `mon` (left, top, width, height).
pub fn show(text: &str, side: Side, bar: (i32, i32, i32, i32), mon: (i32, i32, i32, i32)) {
    with(|h| {
        let t = now();
        h.toast.on(mon);
        h.toast.show(&mut h.gfx, text, side, bar, t);
        h.frame(t);
    });
}

pub fn hide() {
    with(|h| {
        h.toast.hide();
        h.frame(now());
    });
}

impl Host {
    fn frame(&mut self, t: u64) {
        let (rect, up, alpha) = self.toast.frame(&mut self.gfx, t);
        if up {
            let _ = self.toast.surface().present(super::panel::whwnd(self.wnd), rect.0, rect.1, alpha);
            super::gfx::commit();
            if !self.shown {
                self.shown = true;
                // SAFETY: showing our own window without activating it.
                unsafe { SetWindowPos(self.wnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW) };
            }
        } else if self.shown {
            self.shown = false;
            // SAFETY: hiding our own window.
            unsafe { ShowWindow(self.wnd, SW_HIDE) };
        }
        let busy = self.toast.busy(t);
        self.timer = busy;
        crate::pacing::set(self.wnd as isize, busy);
        // SAFETY: a timer on our own window: a one-shot for the end of the pill's rest, which needs no frames until then.
        unsafe {
            match self.toast.wake(t) {
                Some(at) => _ = SetTimer(self.wnd, 1, at.saturating_sub(t).max(1) as u32, None),
                None => _ = KillTimer(self.wnd, 1),
            }
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == crate::pacing::FRAME {
        if crate::pacing::received(hwnd as isize) { with(|h| h.frame(now())); }
        return 0;
    }
    if msg == WM_TIMER && wp == 1 {
        // SAFETY: our own window's timer.
        unsafe { KillTimer(hwnd, 1) };
        with(|h| h.frame(now()));
        return 0;
    }
    // SAFETY: the default procedure of our own window.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_stands_over_the_bar_of_the_monitor_it_is_shown_on() {
        let mut t = Toast::new(1.0, (2560, 1440));
        t.width = 200.0;
        t.side = Side::Bottom;
        t.bar = (0, 1386, 2560, 1440);
        assert_eq!(t.rest(), (1280.0 - 100.0, 1386.0 - GAP - HEIGHT));
        // the monitor to the left (1920x1080 at negative x)
        t.on((-1920, 0, 1920, 1080));
        t.bar = (-1920, 1026, 0, 1080);
        assert_eq!(t.rest(), (-960.0 - 100.0, 1026.0 - GAP - HEIGHT));
        // beside a left bar: centred on what the bar leaves along the monitor's foot
        t.side = Side::Left;
        t.bar = (-1920, 0, -1866, 1080);
        assert_eq!(t.rest(), (-1920.0 + (54.0 + 1920.0) / 2.0 - 100.0, 1080.0 - GAP - HEIGHT));
    }

    #[test]
    fn a_resting_toast_asks_for_no_frames_only_for_a_timer() {
        let mut t = Toast::new(1.0, (1440, 900));
        t.since = Some(1000);
        let full = style::anim() == Anim::Full;
        assert_eq!(t.busy(1100), full, "coming in");
        if full {
            assert!(!t.busy(1000 + 1200) && t.wake(1000 + 1200) == Some(1000 + 2040), "resting until 85 %: a timer, no frames");
            assert!(t.busy(1000 + 2100) && t.wake(1000 + 2100).is_none(), "leaving");
        }
        assert!(!t.busy(1000 + LIFE) && t.wake(1000 + LIFE).is_none(), "over");
    }

    #[test]
    fn t2_keyframes() {
        assert_eq!(keyframes(0), (0.0, -10.0)); // starts clear, 10 px up
        let (o, dy) = keyframes(288); // 12 % of 2.4 s: arrived
        assert!((o - 1.0).abs() < 1e-4 && dy.abs() < 1e-3);
        assert_eq!(keyframes(1200), (1.0, 0.0)); // resting
        let (o, dy) = keyframes(2300); // on its way out
        assert!(o < 1.0 && dy < 0.0 && dy > -6.0);
    }
}
