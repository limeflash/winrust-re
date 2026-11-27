use std::collections::HashMap;
use windows_sys::Win32::Foundation::HINSTANCE;
use windows_sys::Win32::UI::WindowsAndMessaging::{IDC_ARROW, LoadCursorW, RegisterClassW, WNDCLASSW};
use winrust_common::style::Anim;
use crate::anim::{self, CubicBezier, POP, Transition};
use crate::gfx::{Canvas, Rect};

/// CSS `clip-path: inset(top right bottom left round radius)`, in DIPs from the edges of the box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inset {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
    pub radius: f32,
}

impl Inset {
    #[must_use]
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32, radius: f32) -> Self {
        Self { top, right, bottom, left, radius }
    }

    fn lerp(a: Self, b: Self, t: f32) -> Self {
        let l = anim::lerp;
        Self::new(l(a.top, b.top, t), l(a.right, b.right, t), l(a.bottom, b.bottom, t), l(a.left, b.left, t), l(a.radius, b.radius, t))
    }
}

/// `transform-origin`.
#[derive(Clone, Copy, Debug)]
pub enum Origin {
    /// Fractions of the box: `Frac(1.0, 1.0)` is `100% 100%`.
    Frac(f32, f32),
    /// The corner of the box nearest to the bar button, on the button's own line: what `100% 100%` means for a panel
    /// that stands right above its button.
    Button,
    /// DIPs from the box's top left corner.
    Px(f32, f32),
}

/// What an element looks like at a moment of its entry or exit: the properties the mockup's keyframes move.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub opacity: f32,
    pub dx: f32,
    pub dy: f32,
    pub k: f32,
    pub clip: Option<Inset>,
}

impl Pose {
    pub const REST: Pose = Pose { opacity: 1.0, dx: 0.0, dy: 0.0, k: 1.0, clip: None };

    pub fn lerp(a: Self, b: Self, t: f32, radius: f32) -> Self {
        let l = anim::lerp;
        let full = Inset::new(0.0, 0.0, 0.0, 0.0, radius);
        let clip = match (a.clip, b.clip) {
            (None, None) => None,
            (x, y) => Some(Inset::lerp(x.unwrap_or(full), y.unwrap_or(full), t)),
        };
        Self { opacity: l(a.opacity, b.opacity, t), dx: l(a.dx, b.dx, t), dy: l(a.dy, b.dy, t), k: l(a.k, b.k, t), clip }
    }
}

/// How a panel arrives: from `from` to rest.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub from: Pose,
    pub dur: u32,
    pub ease: CubicBezier,
    pub origin: Origin,
}

impl Entry {
    /// `qsIn`: `opacity 0, translateY(18px) scale(.96)` to rest in 280 ms, `cubic-bezier(.16,1,.3,1)`; quick settings, calendar,
    /// tray and keyboard layout.
    #[must_use]
    pub const fn pop() -> Self {
        Self { from: Pose { opacity: 0.0, dx: 0.0, dy: 18.0, k: 0.96, clip: None }, dur: 280, ease: POP, origin: Origin::Button }
    }

    /// `stIn`/`bsIn`/`jlIn`: the panel rises `dy` and unrolls from `clip` (a button-sized rounded rectangle at the bar).
    #[must_use]
    pub const fn reveal(dy: f32, clip: Inset, opacity: f32, dur: u32) -> Self {
        Self {
            from: Pose { opacity, dx: 0.0, dy, k: 1.0, clip: Some(clip) },
            dur,
            ease: anim::CubicBezier::new(0.2, 0.9, 0.1, 1.0),
            origin: Origin::Frac(0.5, 1.0),
        }
    }
}

/// How a panel leaves: from rest to `to`; a zero duration is the mockup's instant removal.
#[derive(Clone, Copy, Debug)]
pub struct Exit {
    pub to: Pose,
    pub dur: u32,
    pub ease: CubicBezier,
    pub origin: Origin,
}

impl Exit {
    #[must_use]
    pub const fn none() -> Self {
        Self { to: Pose::REST, dur: 0, ease: anim::LINEAR, origin: Origin::Frac(0.5, 0.5) }
    }

    /// `stOut`/`bsOut`/`jlOut`: sinks `dy` into the button while the clip closes.
    #[must_use]
    pub const fn reveal(dy: f32, clip: Inset, opacity: f32, dur: u32) -> Self {
        Self {
            to: Pose { opacity, dx: 0.0, dy, k: 1.0, clip: Some(clip) },
            dur,
            ease: anim::EXIT,
            origin: Origin::Frac(0.5, 1.0),
        }
    }
}

/// An item that rises in (the mockup's `stItem`, `jliIn`, `bsiIn`): from `opacity 0, translateY(dy) scale(k)`.
#[derive(Clone, Copy, Debug)]
pub struct Rise {
    pub opacity: f32,
    pub dy: f32,
    pub k: f32,
}

impl Rise {
    /// Draws `f` the way it is now; `origin` is the point the scale goes around.
    pub fn draw(self, c: &mut Canvas<'_>, origin: (f32, f32), f: impl FnOnce(&mut Canvas<'_>)) {
        if self.opacity >= 0.999 && self.dy.abs() < 0.01 && (self.k - 1.0).abs() < 1e-3 {
            return f(c);
        }
        c.faded(self.opacity, |c| c.transformed(origin.0, origin.1, self.k, 0.0, self.dy, f));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// what a panel implements

/// What a content asks the frame to do after a click or a key.
#[derive(Default)]
pub struct Cx {
    pub close: bool,
    pub then: Option<Box<dyn FnOnce()>>,
    /// Where the box is on the screen (device pixels) and its scale: set by the frame for clicks and keys.
    pub at: (i32, i32),
    pub scale: f32,
}

impl Cx {
    /// A rectangle of the box (DIPs) on the screen: left, top, right, bottom in device pixels.
    #[must_use]
    pub fn screen_rect(&self, r: Rect) -> (i32, i32, i32, i32) {
        let s = self.scale;
        (self.at.0 + (r.x * s).round() as i32, self.at.1 + (r.y * s).round() as i32, self.at.0 + ((r.x + r.w) * s).round() as i32, self.at.1 + ((r.y + r.h) * s).round() as i32)
    }

    /// Close the panel.
    pub fn close(&mut self) {
        self.close = true;
    }

    /// The panel has been asked to close.
    #[must_use]
    pub const fn close_asked(&self) -> bool {
        self.close
    }

    /// Close the panel, then run `f` (an app's own menu must not open under a panel that still holds the focus).
    pub fn close_then(&mut self, f: impl FnOnce() + 'static) {
        self.close = true;
        self.then = Some(Box::new(f));
    }

    /// Run `f` with the panel still open.
    pub fn run(&mut self, f: impl FnOnce() + 'static) {
        self.then = Some(Box::new(f));
    }

    /// Runs what the click asked for, for a window that is not the panel host (the notification pop-ups).
    pub fn finish(self) {
        if let Some(f) = self.then {
            f();
        }
    }

    /// What a test does in place of the frame: runs the callback the click asked for.
    pub fn run_for_test(self) {
        if let Some(f) = self.then {
            f();
        }
    }
}

/// Modifier keys held with a key.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
}

/// What a frame hands to a content while it draws.
pub struct Frame<'a> {
    pub now: u64,
    /// Milliseconds since the panel opened.
    pub age: u64,
    /// Milliseconds since it started closing; 0 while it is open.
    pub leaving: u64,
    pub hover: Option<i32>,
    /// The item the pointer went down on and still holds.
    pub press: Option<i32>,
    /// The box in DIPs.
    pub size: (f32, f32),
    /// The panel's own opacity (its entry and exit fade), for a content that must follow it.
    pub opacity: f32,
    pub mo: &'a mut Motion,
}

/// Hover and the like live in slots; items' own tweens use slots below this.
const HOVER: u8 = 255;

impl Frame<'_> {
    /// A value that glides to `to` like a CSS transition and starts at `to` on first sight. `key` names the item, `slot`
    /// the property.
    pub fn tween(&mut self, key: i32, slot: u8, to: f32, dur: u32, ease: CubicBezier) -> f32 {
        self.mo.go(key, slot, to, self.now, dur, ease)
    }

    /// 0..1 hover of item `id` (`transition: background <dur>ms`).
    pub fn hover(&mut self, id: i32, dur: u32, ease: CubicBezier) -> f32 {
        let on = f32::from(self.hover == Some(id));
        self.mo.go(id, HOVER, on, self.now, dur, ease)
    }

    /// `animation: <keyframes> <dur>ms <ease> <delay>ms backwards` of an item that rises in at the opening: before its delay it
    /// is invisible and displaced by `dy` and scaled by `k`.
    #[must_use]
    pub fn rise(&self, delay: u32, dur: u32, ease: CubicBezier, dy: f32, k: f32) -> Rise {
        let (dur, delay) = anim::timing(self.mo.mode(), dur, delay);
        if dur == 0 {
            return Rise { opacity: 1.0, dy: 0.0, k: 1.0 };
        }
        let t = (self.age as f32 - delay as f32) / dur as f32;
        let e = ease.ease(t.clamp(0.0, 1.0));
        Rise { opacity: e, dy: anim::lerp(dy, 0.0, e), k: anim::lerp(k, 1.0, e) }
    }

    /// 1 while the panel is open, falling to 0 over `ms` once it starts closing (`.stOut .sti{transition:opacity .12s}`).
    #[must_use]
    pub fn fade_out(&self, ms: u32) -> f32 {
        if self.leaving == 0 || ms == 0 { 1.0 } else { 1.0 - (self.leaving as f32 / ms as f32).min(1.0) }
    }
}

/// The transitions of a panel's items.
#[derive(Default)]
pub struct Motion {
    t: HashMap<(i32, u8), Transition>,
    pub mode: Option<Anim>,
}

impl Motion {
    fn mode(&self) -> Anim {
        self.mode.unwrap_or(Anim::Full)
    }

    /// Retargets like a CSS transition (a first sight settles at once) and returns the value now.
    fn go(&mut self, key: i32, slot: u8, to: f32, now: u64, dur: u32, ease: CubicBezier) -> f32 {
        let (dur, delay) = anim::timing(self.mode(), dur, 0);
        let t = self.t.entry((key, slot)).or_insert_with(|| Transition::new(to));
        t.set(to, now, dur, delay, ease);
        t.value(now)
    }

    pub fn running(&self, now: u64) -> bool {
        self.t.values().any(|t| t.running(now))
    }
}

/// Registers a window class of this module (an arrow cursor for the one that takes the mouse).
pub fn register_class(hi: HINSTANCE, name: &[u16], proc: windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC, cursor: bool) {
    // SAFETY: `name` is NUL-terminated and outlives the call; the class stores what it needs.
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: hi,
            hCursor: if cursor { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) } else { std::ptr::null_mut() },
            lpszClassName: name.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
    }
}

/// The character of a WM_CHAR message: the system sends a character beyond the BMP (an emoji) as two messages, the high
/// surrogate first; it is held until the low one comes. None for the first half and for a stray half.
pub fn wm_char(unit: usize) -> Option<char> {
    thread_local! { static HIGH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
    let u = unit as u32 & 0xFFFF;
    match u {
        0xD800..=0xDBFF => {
            HIGH.set(u);
            None
        }
        0xDC00..=0xDFFF => match HIGH.replace(0) {
            0 => None,
            hi => char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (u - 0xDC00)),
        },
        _ => {
            HIGH.set(0);
            char::from_u32(u)
        }
    }
}

