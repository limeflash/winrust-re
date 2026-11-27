//! The hint over a bar button: the mockup's `.app::after`. A dark rounded label (`rgba(27,28,34,.96)`, a white ring at .1,
//! `0 8px 24px` shadow, radius 8, padding 5×10, Inter 500 12 px) that fades in over .2 s and rises 6 px into place over .35 s
//! (`SPRING`) once the pointer has rested .25 s, and fades out again when it leaves.
//!
//! [`Tip`] is the windowless core; [`show`] and [`hide`] drive the one real window (click-through, never active; composed, or layered without DirectComposition).

use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_TOPMOST, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    KillTimer, SetTimer, SetWindowPos, ShowWindow, WM_TIMER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use winrust_common::style::{self, Side};

use super::anim::{self, EASE, SPRING, Transition};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba, Surface};

const FONT: Font = Font::new(12.0, 500);
const BG: Rgba = Rgba::css(27, 28, 34, 0.96);
const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
/// Line height of 12 px Inter (`line-height: normal` in the browser).
const LINE: f32 = 15.0;
/// Room around the label for the shadow and the slide, DIPs.
const PAD: f32 = 44.0;
/// How far the label slides in, DIPs.
const SLIDE: f32 = 6.0;

/// Where the label stands: `edge` is the line it keeps to (device pixels) and `along` the point it is centred on along the
/// bar. For a bottom bar the label stands above `edge`; for a left bar it starts right of it; for a right bar it ends left of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub side: Side,
    pub along: i32,
    pub edge: i32,
    /// The monitor it stands on (left, top, width, height, device pixels); zero width: the primary one the tip was made for.
    pub mon: (i32, i32, i32, i32),
}

impl Place {
    /// The label of a bar button: `bottom:58px` above a bottom bar's button, 12 px beside a side bar's.
    #[must_use]
    pub fn button(side: Side, rect: (i32, i32, i32, i32), along: i32, scale: f32) -> Self {
        let dp = |v: f32| (v * scale).round() as i32;
        let edge = match side {
            Side::Bottom => rect.3 - dp(58.0),
            Side::Left => rect.2 + dp(12.0),
            Side::Right => rect.0 - dp(12.0),
        };
        Self { side, along, edge, mon: (0, 0, 0, 0) }
    }

    /// The same label on the monitor `mon` (left, top, width, height).
    #[must_use]
    pub const fn on(mut self, mon: (i32, i32, i32, i32)) -> Self {
        self.mon = mon;
        self
    }
}

/// The label's state and painting, without a window.
pub struct Tip {
    lines: Vec<String>,
    size: (f32, f32),
    place: Place,
    screen: (i32, i32),
    scale: f32,
    /// Opacity and the slide (6 → 0 DIPs).
    op: Transition,
    mv: Transition,
    /// What to switch to once the label that is showing has faded out.
    pending: Option<(String, Place, u64, Option<u32>)>,
    hide_at: Option<u64>,
    surf: Surface,
    painted: Option<(i32, i32, u32)>,
    at: (i32, i32),
}

impl Tip {
    #[must_use]
    pub fn new(scale: f32, screen: (i32, i32)) -> Self {
        Self {
            lines: Vec::new(),
            size: (0.0, 0.0),
            place: Place { side: Side::Bottom, along: 0, edge: 0, mon: (0, 0, 0, 0) },
            screen,
            scale,
            op: Transition::new(0.0),
            mv: Transition::new(SLIDE),
            pending: None,
            hide_at: None,
            surf: Surface::composed(),
            painted: None,
            at: (0, 0),
        }
    }

    /// Shows `text` at `place` after `delay` ms (the mockup's `transition-delay: .25s`); `hold` hides it again after that many
    /// ms (the volume readout). A label that is already up fades out first.
    pub fn show(&mut self, g: &mut Gfx, text: &str, place: Place, now: u64, delay: u32, hold: Option<u32>) {
        if text.is_empty() {
            return self.hide(now);
        }
        if self.op.target() > 0.0 && self.lines.join("\n") == text && self.place == place {
            self.hide_at = hold.map(|h| now + u64::from(h));
            return; // the same label again (the wheel keeps turning)
        }
        if self.op.value(now) > 0.02 && delay > 0 {
            // another label is still on its way out: this one waits for it, counting its delay from now
            self.pending = Some((text.to_string(), place, now + u64::from(delay), hold));
            return self.fade(now);
        }
        self.pending = None;
        self.lines = text.split('\n').map(str::to_string).collect();
        let w = self.lines.iter().map(|l| g.measure(l, FONT)).fold(0.0, f32::max).ceil();
        self.size = (w + 20.0, LINE * self.lines.len() as f32 + 10.0);
        self.place = place;
        let (od, odel) = anim::timing(style::anim(), 200, delay);
        let (md, mdel) = anim::timing(style::anim(), 350, delay);
        self.op.snap(0.0);
        self.mv.snap(SLIDE);
        self.op.set(1.0, now, od, odel, EASE);
        self.mv.set(0.0, now, md, mdel, SPRING);
        self.hide_at = hold.map(|h| now + u64::from(h) + u64::from(delay));
        self.painted = None;
    }

    /// Fades the label out (`opacity .2s ease`; it also slides back down), and drops the one that waited for it: whoever hides
    /// the tip has left the thing the waiting label was for.
    pub fn hide(&mut self, now: u64) {
        self.pending = None;
        self.fade(now);
    }

    fn fade(&mut self, now: u64) {
        self.hide_at = None;
        if self.op.target() <= 0.0 {
            return;
        }
        let (od, _) = anim::timing(style::anim(), 200, 0);
        let (md, _) = anim::timing(style::anim(), 350, 0);
        self.op.set(0.0, now, od, 0, EASE);
        self.mv.set(SLIDE, now, md, 0, SPRING);
    }

    /// Something is moving: frames on every tick of the screen. Waiting (the hold of a readout) is not that: see [`Tip::wake`].
    #[must_use]
    pub fn busy(&self, now: u64) -> bool {
        self.op.running(now) || self.mv.running(now) || self.pending.is_some()
    }

    /// When the label has to be looked at again though nothing moves until then (it hides itself after its hold).
    #[must_use]
    pub const fn wake(&self) -> Option<u64> {
        self.hide_at
    }

    /// Where the label's box rests, in DIPs on the screen.
    fn rest(&self) -> (f32, f32) {
        let s = self.scale;
        let (w, h) = self.size;
        // within the monitor, as if it stood at (0, 0), and moved to where it is
        let (mx, my, mw, mh) = if self.place.mon.2 > 0 { self.place.mon } else { (0, 0, self.screen.0, self.screen.1) };
        let (sw, sh) = (mw as f32 / s, mh as f32 / s);
        let (ox, oy) = (mx as f32 / s, my as f32 / s);
        let (along, edge) = match self.place.side {
            Side::Bottom => ((self.place.along - mx) as f32 / s, (self.place.edge - my) as f32 / s),
            _ => ((self.place.along - my) as f32 / s, (self.place.edge - mx) as f32 / s),
        };
        let (x, y) = match self.place.side {
            Side::Bottom => ((along - w / 2.0).clamp(4.0, (sw - w - 4.0).max(4.0)), edge - h),
            Side::Left => (edge, (along - h / 2.0).clamp(4.0, (sh - h - 4.0).max(4.0))),
            Side::Right => (edge - w, (along - h / 2.0).clamp(4.0, (sh - h - 4.0).max(4.0))),
        };
        (x + ox, y + oy)
    }

    /// Paints the label at `now`. Returns the window rectangle in device pixels, whether the window should be shown, and its
    /// opacity (the fade is the window's, not repainted).
    pub fn frame(&mut self, g: &mut Gfx, now: u64) -> ((i32, i32, i32, i32), bool, u8) {
        if self.hide_at.is_some_and(|t| now >= t) {
            self.hide(now);
        }
        let o = self.op.value(now);
        if o <= 0.001 && self.op.target() <= 0.0 {
            if let Some((t, p, due, h)) = self.pending.take() {
                self.show(g, &t, p, now, due.saturating_sub(now) as u32, h);
            }
            if self.op.target() <= 0.0 && self.pending.is_none() {
                self.lines.clear();
                return ((0, 0, 0, 0), false, 0);
            }
        }
        let s = self.scale;
        let (o, m) = (self.op.value(now), self.mv.value(now));
        let (bx, by) = self.rest();
        let (wx, wy) = (((bx - PAD) * s).round() as i32, ((by - PAD) * s).round() as i32);
        let (ww, wh) = (((self.size.0 + 2.0 * PAD) * s).round() as i32, ((self.size.1 + 2.0 * PAD) * s).round() as i32);
        let (dx, dy) = match self.place.side {
            Side::Bottom => (0.0, m),
            Side::Left => (-m, 0.0),
            Side::Right => (m, 0.0),
        };
        let key = (ww, wh, m.to_bits());
        if self.painted != Some(key) || !self.surf.is_current() {
            self.painted = Some(key);
            let (w, h) = self.size;
            let r = Rect::new(PAD + dx, PAD + dy, w, h);
            let lines = &self.lines;
            let _ = g.paint(&mut self.surf, ww, wh, |c: &mut Canvas<'_>| {
                c.shadow(r, 8.0, 8.0, 24.0, Rgba::css(0, 0, 0, 0.5));
                c.ring(r.inset(-1.0), 9.0, 1.0, Rgba::white(0.1));
                c.erase(r, 8.0);
                c.fill(r, 8.0, BG);
                for (i, l) in lines.iter().enumerate() {
                    c.text(l, Rect::new(r.x + 10.0, r.y + 5.0 + LINE * i as f32, w - 20.0, LINE), FONT, INK, Align::Start);
                }
            });
        }
        self.at = (wx, wy);
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
    tip: Tip,
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

/// Creates the label's window; `screen` is the primary monitor in device pixels.
pub fn init(hi: HINSTANCE, scale: f32, screen: (i32, i32)) -> bool {
    let Ok(gfx) = Gfx::new(scale) else { return false };
    let name: Vec<u16> = "topbar_tip\0".encode_utf16().collect();
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
    HOST.set(Some(Host { tip: Tip::new(scale, screen), gfx, wnd, shown: false, timer: false }));
    true
}

/// Shows `text` over a bar button after `delay` ms; `hold` takes it away again after that many ms.
pub fn show(text: &str, place: Place, delay: u32, hold: Option<u32>) {
    with(|h| {
        let t = now();
        h.tip.show(&mut h.gfx, text, place, t, delay, hold);
        h.frame(t);
    });
}

pub fn hide() {
    with(|h| {
        let t = now();
        h.tip.hide(t);
        h.frame(t);
    });
}

/// A label is on screen or about to be.
#[must_use]
pub fn is_up() -> bool {
    with(|h| h.shown || h.tip.busy(now())).unwrap_or(false)
}

impl Host {
    fn frame(&mut self, t: u64) {
        let (rect, up, alpha) = self.tip.frame(&mut self.gfx, t);
        if up {
            let _ = self.tip.surface().present(super::panel::whwnd(self.wnd), rect.0, rect.1, alpha);
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
        let busy = self.tip.busy(t);
        self.timer = busy;
        crate::pacing::set(self.wnd as isize, busy);
        // SAFETY: a timer on our own window: a one-shot for the end of the hold, which needs no frames until then.
        unsafe {
            match self.tip.wake() {
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
    fn a_label_over_a_bar_on_the_monitor_to_the_left_stays_on_it() {
        let mut t = Tip::new(1.0, (2560, 1440));
        t.size = (100.0, 25.0);
        // the primary monitor: centred on the button, 58 above its foot
        t.place = Place::button(Side::Bottom, (1000, 1386, 1040, 1426), 1020, 1.0);
        assert_eq!(t.rest(), (970.0, 1368.0 - 25.0));
        // the monitor to its left (1920x1080 at negative x): the same arithmetic within that monitor
        let mon = (-1920, 0, 1920, 1080);
        t.place = Place::button(Side::Bottom, (-1000, 1032, -960, 1074), -980, 1.0).on(mon);
        assert_eq!(t.rest(), (-1030.0, 1016.0 - 25.0));
        // at that monitor's left edge it keeps 4 px from it, and does not slide onto the next monitor
        t.place = Place::button(Side::Bottom, (-1920, 1032, -1880, 1074), -1900, 1.0).on(mon);
        assert_eq!(t.rest().0, -1916.0);
        t.place = Place::button(Side::Bottom, (-40, 1032, 0, 1074), -20, 1.0).on(mon);
        assert_eq!(t.rest().0, -104.0, "4 px from the monitor's right edge, which is x = 0");
        // beside a side bar on that monitor
        t.place = Place::button(Side::Left, (-1920, 400, -1866, 440), 420, 1.0).on(mon);
        assert_eq!(t.rest(), (-1866.0 + 12.0, 420.0 - 12.5));
    }

    #[test]
    fn a_label_waiting_out_its_hold_asks_for_a_timer_not_for_frames() {
        let mut t = Tip::new(1.0, (1440, 900));
        t.hide_at = Some(5000);
        assert!(!t.busy(1000), "nothing moves while the readout holds");
        assert_eq!(t.wake(), Some(5000));
        t.op.set(1.0, 1000, 200, 0, EASE);
        assert!(t.busy(1100), "its fade in is motion");
    }
}
