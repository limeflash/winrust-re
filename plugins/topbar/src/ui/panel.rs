//! The frame every popup panel of the Obsidian bar stands on.
//!
//! A panel is a [`Content`]: it says how big it is, how it looks ([`Look`]), how it comes and goes ([`Entry`], [`Exit`]),
//! draws itself in its own DIP coordinates and answers hit tests and clicks. Everything else is done here:
//!
//! * the window: one layered window with per-pixel alpha for the panel and, under it, a click-through window that holds the
//!   shadow (so a click beside the panel reaches what is under it);
//! * the glass: the screen under the panel is grabbed before the window is shown, blurred and saturated on the GPU and
//!   painted under the tint; it stays where it was grabbed while the panel moves or scales;
//! * the motion: the open and close animations of the mockup (`qsIn`'s translate/scale/opacity, the `clip-path` reveal from the
//!   bar button of `stIn`/`bsIn`/`jlIn`), and per-item helpers ([`Frame::rise`], [`Frame::hover`]) for the staggered items;
//! * the input: hover, clicks (left, right, middle), arrows/Enter/Esc, closing on lost focus, and the guard against a panel
//!   that was just closed by the focus loss being opened again by the click on its own bar button.
//!
//! [`Popup`] is the windowless core (state, geometry, painting into two surfaces); the functions at the bottom ([`init`],
//! [`open`], [`toggle`], [`close`]) bind it to real windows. `topbar --render-panel` drives [`Popup`] directly.
//!
//! A new panel: implement [`Content`], build it where its button is clicked, and call
//! `panel::toggle(Id::X, || Some((Box::new(MyPanel::new()), anchor)))`.

use std::cell::RefCell;
use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, HWND_TOPMOST, IDC_ARROW, KillTimer, LoadCursorW, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer, SetWindowPos, ShowWindow, WM_ACTIVATE, WM_CHAR, WM_ERASEBKGND, WM_MOUSEWHEEL, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use winrust_common::style::{self, Anim, Side};

use super::anim::{self, CubicBezier, POP, Transition};
use super::gfx::{Canvas, Gfx, Picture, Rect, Rgba, Surface, Xf};

const WM_MOUSELEAVE: u32 = 0x02A3; // not exported under the enabled features
const VK_ESCAPE: u32 = 0x1B;
const VK_RETURN: u32 = 0x0D;
const VK_SPACE: u32 = 0x20;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_TAB: u32 = 0x09;

// ---------------------------------------------------------------------------------------------------------------------
// vocabulary

/// Which panel a window belongs to: the bar lights the button of the open one, and a click on that button closes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Id {
    Tray,
    Lang,
    Start,
    Search,
    Qs,
    Cal,
    Bar,
    Jump,
    Preview,
    Power,
    Toast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
    First,
    Last,
}

/// Where a panel opens: the bar, the button it belongs to and the screen, all in device pixels.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub side: Side,
    /// Left, top, right, bottom of the bar button.
    pub button: (i32, i32, i32, i32),
    /// The bar where it rests (not slid away).
    pub bar: (i32, i32, i32, i32),
    pub screen: (i32, i32),
}

/// How the panel's box lines up with its button along the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Along {
    Center,
    /// Right (or bottom) edges together.
    End,
    /// Left (or top) edges together.
    Start,
    /// Against the far edge of the screen, `gap` from it (the mockup's `right: 12px` of quick settings and the calendar).
    ScreenEnd,
}

#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub along: Along,
    /// Gap between the bar and the panel in DIPs: the mockup's 12.
    pub gap: f32,
}

impl Default for Place {
    fn default() -> Self {
        Self { along: Along::Center, gap: 12.0 }
    }
}

/// A CSS box shadow `0 <dy>px <blur>px <color>`.
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    pub dy: f32,
    pub blur: f32,
    pub color: Rgba,
}

/// Paint of a panel: the two glass tokens of the mockup.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub radius: f32,
    /// Colour over the blurred screen.
    pub tint: Rgba,
    /// `backdrop-filter: blur(<blur>px) saturate(<saturate>)`.
    pub blur: f32,
    pub saturate: f32,
    /// `0 0 0 1px` outside the box.
    pub ring_out: Rgba,
    /// `inset 0 0 0 1px` inside it.
    pub ring_in: Rgba,
    pub shadow: Shadow,
}

impl Look {
    /// The glass panel: `rgba(16,17,24,alpha)`, blur 40, saturate 1.6 (Start, bar settings, search, jump list, previews).
    #[must_use]
    pub const fn glass(radius: f32, alpha: f32, shadow: Shadow) -> Self {
        Self {
            radius,
            tint: Rgba::css(16, 17, 24, alpha),
            blur: 40.0,
            saturate: 1.6,
            ring_out: Rgba::css(0, 0, 0, 0.5),
            ring_in: Rgba::white(0.1),
            shadow,
        }
    }

    /// The dense panel: `rgba(27,28,34,.94)`, blur 30 (quick settings, calendar, tray, keyboard layout).
    #[must_use]
    pub const fn dense(radius: f32, shadow: Shadow) -> Self {
        Self {
            radius,
            tint: Rgba::css(27, 28, 34, 0.94),
            blur: 30.0,
            saturate: 1.0,
            ring_out: Rgba::css(0, 0, 0, 0.55),
            ring_in: Rgba::white(0.09),
            shadow,
        }
    }
}

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

    fn lerp(a: Self, b: Self, t: f32, radius: f32) -> Self {
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
            ease: super::anim::CubicBezier::new(0.2, 0.9, 0.1, 1.0),
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
    close: bool,
    then: Option<Box<dyn FnOnce()>>,
}

impl Cx {
    /// Close the panel.
    pub fn close(&mut self) {
        self.close = true;
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
    mo: &'a mut Motion,
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
struct Motion {
    t: HashMap<(i32, u8), Transition>,
    mode: Option<Anim>,
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

    fn running(&self, now: u64) -> bool {
        self.t.values().any(|t| t.running(now))
    }
}

/// A panel. Coordinates in every method are DIPs from the top left corner of the panel's box.
pub trait Content {
    fn id(&self) -> Id;
    fn look(&self) -> Look;
    fn entry(&self) -> Entry {
        Entry::pop()
    }
    fn exit(&self) -> Exit {
        Exit::none()
    }
    fn place(&self) -> Place {
        Place::default()
    }
    /// The box in DIPs; asked once, before the panel is shown.
    fn size(&mut self, g: &mut Gfx) -> (f32, f32);
    /// Draws the inside of the box (the glass, the tint and the rings are already there).
    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>);
    /// The item under a point; its number is whatever the content chooses.
    fn hit(&self, x: f32, y: f32) -> Option<i32>;
    fn click(&mut self, _id: i32, _b: Button, _cx: &mut Cx) {}
    /// The pointer moved to `(x, y)` with the button still down on item `id` (a slider's knob): the pointer is captured, so
    /// the point can be outside the box.
    fn drag(&mut self, _id: i32, _x: f32, _y: f32) {}
    /// The button that went down on item `id` came up.
    fn release(&mut self, _id: i32) {}
    /// A key before the frame handles it (Esc closes, arrows move the hover, Enter and Space click): true = taken. A panel
    /// with a text field takes its editing keys here.
    fn key(&mut self, _vk: u32, _cx: &mut Cx) -> bool {
        false
    }
    /// A typed character (never a control character).
    fn text(&mut self, _c: char, _cx: &mut Cx) {}
    /// The wheel turned over the panel: `up` is away from the user.
    fn wheel(&mut self, _up: bool) {}
    /// The item an arrow key moves the hover to.
    fn nav(&self, _from: Option<i32>, _dir: Dir) -> Option<i32> {
        None
    }
    /// A tooltip for the hovered item and the rectangle it belongs to.
    fn tip(&self, _id: i32) -> Option<(String, Rect)> {
        None
    }
    /// How long after opening the content still moves by itself (its staggered items), in ms.
    fn settle(&self) -> u32 {
        0
    }
    /// The content wants more frames (a spinner, a live value).
    fn animating(&self, _now: u64) -> bool {
        false
    }
    /// A content that shows live values (a clock, the volume) asks for a frame this often, in ms, while it is open (0 = never).
    fn pulse(&self) -> u32 {
        0
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the windowless popup

/// Where the last frame went, in device pixels, for the caller to present.
#[derive(Clone, Copy, Debug, Default)]
pub struct Shown {
    pub win: (i32, i32, i32, i32),
    pub shadow: (i32, i32, i32, i32),
    pub shadow_alpha: u8,
    /// Another frame is needed.
    pub busy: bool,
    /// The exit has finished: the windows can go.
    pub done: bool,
    /// The content's own refresh period in ms (0 = none).
    pub pulse: u32,
}

/// Pixels of a screen rectangle (left, top, width, height).
type Capture = (Vec<u32>, (i32, i32, i32, i32));

pub struct Popup {
    content: Box<dyn Content>,
    id: Id,
    look: Look,
    entry: Entry,
    exit: Exit,
    scale: f32,
    /// The box in DIPs (whole device pixels).
    size: (f32, f32),
    /// The box's top left corner on the screen, device pixels.
    at: (i32, i32),
    /// Room around the box inside the panel window, DIPs.
    m: f32,
    win: (i32, i32, i32, i32),
    sh: (i32, i32, i32, i32),
    button: (i32, i32, i32, i32),
    side: Side,
    t0: u64,
    /// When it started closing and how far in it was.
    closing: Option<(u64, f32)>,
    hover: Option<i32>,
    press: Option<i32>,
    mo: Motion,
    /// What was on the screen under the window, until the first frame bakes it: pixels and their screen rectangle.
    cap: Option<Capture>,
    glass: Option<(Picture, i32, i32)>,
    surf: Surface,
    shade: Surface,
    shade_done: bool,
    last: Shown,
}

/// The pixels of a screen rectangle that may reach beyond the screen: the part on the screen comes from `grab`, the rest
/// repeats the nearest edge pixel (so the blur does not fade out at the screen's edge).
fn backdrop(r: (i32, i32, i32, i32), screen: (i32, i32), grab: &mut dyn FnMut(i32, i32, i32, i32) -> Option<Vec<u32>>) -> Option<Vec<u32>> {
    let (x, y, w, h) = r;
    let (x0, y0, x1, y1) = (x.max(0), y.max(0), (x + w).min(screen.0), (y + h).min(screen.1));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let inner = grab(x0, y0, x1 - x0, y1 - y0)?;
    if (x0, y0, x1, y1) == (x, y, x + w, y + h) {
        return Some(inner);
    }
    let iw = (x1 - x0) as usize;
    let mut out = Vec::with_capacity((w * h) as usize);
    for row in 0..h {
        let sy = ((y + row).clamp(y0, y1 - 1) - y0) as usize;
        out.extend((0..w).map(|col| inner[sy * iw + ((x + col).clamp(x0, x1 - 1) - x0) as usize]));
    }
    Some(out)
}

/// The bounds of a box under `pose` (scaled around `origin`, then moved).
fn pose_bounds(p: &Pose, (w, h): (f32, f32), (ox, oy): (f32, f32)) -> Rect {
    let (x0, y0) = (ox + (0.0 - ox) * p.k + p.dx, oy + (0.0 - oy) * p.k + p.dy);
    Rect::new(x0, y0, w * p.k, h * p.k)
}

impl Popup {
    /// Lays the panel out next to its button and grabs the screen under it. `grab` copies a rectangle of the screen
    /// (entirely on it) as opaque BGRA; it is called now, before the first frame, so the panel's own window is not in it.
    pub fn new(
        g: &mut Gfx,
        mut content: Box<dyn Content>,
        anchor: Anchor,
        now: u64,
        grab: &mut dyn FnMut(i32, i32, i32, i32) -> Option<Vec<u32>>,
    ) -> Self {
        let s = g.scale();
        let (look, entry, exit, place) = (content.look(), content.entry(), content.exit(), content.place());
        let (w, h) = content.size(g);
        let (bw, bh) = ((w * s).round() as i32, (h * s).round() as i32);
        let size = (bw as f32 / s, bh as f32 / s);
        let at = place_box(&anchor, (bw, bh), place, s);
        // the room the motion needs around the box
        let origin = |o: Origin| match o {
            Origin::Frac(fx, fy) => (fx * size.0, fy * size.1),
            Origin::Px(x, y) => (x, y),
            Origin::Button => button_origin(&anchor, at, size, s),
        };
        let (b0, b1) = (pose_bounds(&entry.from, size, origin(entry.origin)), pose_bounds(&exit.to, size, origin(exit.origin)));
        let reach = |r: Rect| (-r.x).max(-r.y).max(r.x + r.w - size.0).max(r.y + r.h - size.1).max(0.0);
        let m_px = ((reach(b0).max(reach(b1)) + 2.0) * s).ceil() as i32;
        let m = m_px as f32 / s;
        let win = (at.0 - m_px, at.1 - m_px, bw + 2 * m_px, bh + 2 * m_px);
        // the shadow window: the box and as far as the shadow reaches
        let sigma = look.shadow.blur / 2.0;
        let pad = ((sigma * 3.0 + look.shadow.dy.abs() + 2.0) * s).ceil() as i32;
        let sh = (at.0 - pad, at.1 - pad, bw + 2 * pad, bh + 2 * pad);
        // what the glass blurs: the panel window and the blur's reach beyond it
        let reach_px = (look.blur * 3.0 * s).ceil() as i32;
        let cr = (win.0 - reach_px, win.1 - reach_px, win.2 + 2 * reach_px, win.3 + 2 * reach_px);
        let cap = backdrop(cr, anchor.screen, grab).map(|px| (px, cr));
        g.forget_icons();
        Self {
            id: content.id(),
            content,
            look,
            entry,
            exit,
            scale: s,
            size,
            at,
            m,
            win,
            sh,
            button: anchor.button,
            side: anchor.side,
            t0: now,
            closing: None,
            hover: None,
            press: None,
            mo: Motion::default(),
            cap,
            glass: None,
            surf: Surface::new(),
            shade: Surface::new(),
            shade_done: false,
            last: Shown::default(),
        }
    }

    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    #[must_use]
    pub const fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    /// The box on the screen (left, top, width, height), device pixels.
    #[must_use]
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (self.at.0, self.at.1, (self.size.0 * self.scale).round() as i32, (self.size.1 * self.scale).round() as i32)
    }

    #[must_use]
    pub const fn side(&self) -> Side {
        self.side
    }

    #[must_use]
    pub const fn surfaces(&self) -> (&Surface, &Surface) {
        (&self.surf, &self.shade)
    }

    #[must_use]
    pub const fn last(&self) -> &Shown {
        &self.last
    }

    fn local(&self, x: i32, y: i32) -> (f32, f32) {
        (x as f32 / self.scale - self.m, y as f32 / self.scale - self.m)
    }

    /// The pointer moved to a client point of the panel window; true if the hovered item changed.
    pub fn pointer(&mut self, x: i32, y: i32) -> bool {
        if self.closing.is_some() {
            return false;
        }
        let (lx, ly) = self.local(x, y);
        if let Some(id) = self.press {
            self.content.drag(id, lx, ly); // the button is still down: the item owns the pointer
            return true;
        }
        let h = self.content.hit(lx, ly);
        std::mem::replace(&mut self.hover, h) != h
    }

    /// The pointer left the window.
    pub fn leave(&mut self) -> bool {
        self.press = None;
        self.hover.take().is_some()
    }

    #[must_use]
    pub const fn hovered(&self) -> Option<i32> {
        self.hover
    }

    /// Puts the hover on an item (the keyboard does it; so does the renderer of previews).
    pub fn set_hover(&mut self, id: Option<i32>) {
        self.hover = id;
    }

    /// A mouse button went down at a client point.
    pub fn button(&mut self, x: i32, y: i32, b: Button) -> Cx {
        let mut cx = Cx::default();
        if self.closing.is_some() {
            return cx;
        }
        let (lx, ly) = self.local(x, y);
        self.hover = self.content.hit(lx, ly);
        if let Some(id) = self.hover {
            self.press = Some(id);
            self.content.click(id, b, &mut cx);
            if b == Button::Left {
                self.content.drag(id, lx, ly); // a slider takes the value under the pointer at once
            }
        }
        cx
    }

    /// The button went up.
    pub fn release(&mut self) {
        if let Some(id) = self.press.take() {
            self.content.release(id);
        }
    }

    /// A button is held on an item (the host keeps the pointer captured).
    #[must_use]
    pub const fn pressed(&self) -> bool {
        self.press.is_some()
    }

    /// The wheel turned.
    pub fn wheel(&mut self, up: bool) {
        if self.closing.is_none() {
            self.content.wheel(up);
        }
    }

    /// A character was typed.
    pub fn text(&mut self, c: char) -> Cx {
        let mut cx = Cx::default();
        if self.closing.is_none() {
            self.content.text(c, &mut cx);
        }
        cx
    }

    /// A key went down; Esc closes, arrows move the hover, Enter and Space click it.
    pub fn key(&mut self, vk: u32) -> Cx {
        let mut cx = Cx::default();
        if self.closing.is_some() || self.content.key(vk, &mut cx) {
            return cx;
        }
        let dir = match vk {
            VK_UP => Some(Dir::Up),
            VK_DOWN | VK_TAB => Some(Dir::Down),
            VK_LEFT => Some(Dir::Left),
            VK_RIGHT => Some(Dir::Right),
            VK_HOME => Some(Dir::First),
            VK_END => Some(Dir::Last),
            _ => None,
        };
        match (vk, dir) {
            (VK_ESCAPE, _) => cx.close(),
            (VK_RETURN | VK_SPACE, _) => {
                if let Some(id) = self.hover {
                    self.content.click(id, Button::Left, &mut cx);
                }
            }
            (_, Some(d)) => {
                if let Some(id) = self.content.nav(self.hover, d) {
                    self.hover = Some(id);
                }
            }
            _ => {}
        }
        cx
    }

    /// Starts the exit (or ends it at once when the mockup has none). Returns false if it was already closing.
    pub fn close(&mut self, now: u64) -> bool {
        if self.closing.is_some() {
            return false;
        }
        let p = self.entry_progress(now);
        self.closing = Some((now, p));
        self.hover = None;
        self.press = None;
        true
    }

    /// The tooltip of the hovered item and the place it wants: above the box (device pixels along the bar).
    #[must_use]
    pub fn tip(&self) -> Option<(String, i32, i32)> {
        let (text, r) = self.content.tip(self.hover?)?;
        let s = self.scale;
        Some((text, self.at.0 + ((r.cx()) * s).round() as i32, self.at.1))
    }

    fn entry_progress(&self, now: u64) -> f32 {
        let (dur, delay) = anim::timing(style::anim(), self.entry.dur, 0);
        if dur == 0 {
            return 1.0;
        }
        let t = now.saturating_sub(self.t0 + u64::from(delay)) as f32 / dur as f32;
        self.entry.ease.ease(t.clamp(0.0, 1.0))
    }

    /// Whether anything is still moving.
    #[must_use]
    pub fn busy(&self, now: u64) -> bool {
        let age = now.saturating_sub(self.t0);
        let (edur, _) = anim::timing(style::anim(), self.entry.dur.max(self.content.settle()), 0);
        self.closing.is_some() || age < u64::from(edur) || self.mo.running(now) || self.content.animating(now) || self.cap.is_some()
    }

    fn origin(&self, o: Origin) -> (f32, f32) {
        match o {
            Origin::Frac(fx, fy) => (fx * self.size.0, fy * self.size.1),
            Origin::Px(x, y) => (x, y),
            Origin::Button => button_origin_of(self.side, self.button, self.at, self.size, self.scale),
        }
    }

    /// The pose now, and how open it is (1 at rest, 0 gone).
    fn pose(&self, now: u64) -> (Pose, f32) {
        let r = self.look.radius;
        let from_entry = |p: f32| Pose::lerp(self.entry.from, Pose::REST, p, r);
        match self.closing {
            None => {
                let p = self.entry_progress(now);
                (from_entry(p), p)
            }
            Some((tc, p0)) => {
                let (dur, delay) = anim::timing(style::anim(), self.exit.dur, 0);
                let t = if dur == 0 { 1.0 } else { (now.saturating_sub(tc + u64::from(delay)) as f32 / dur as f32).clamp(0.0, 1.0) };
                let e = self.exit.ease.ease(t);
                (Pose::lerp(from_entry(p0), self.exit.to, e, r), p0 * (1.0 - e))
            }
        }
    }

    fn finished(&self, now: u64) -> bool {
        let Some((tc, _)) = self.closing else { return false };
        let (dur, delay) = anim::timing(style::anim(), self.exit.dur, 0);
        now >= tc + u64::from(dur) + u64::from(delay)
    }

    /// Moves everything to `now` and paints both surfaces.
    pub fn frame(&mut self, g: &mut Gfx, now: u64) -> Shown {
        self.mo.mode = Some(style::anim());
        let (pose, open) = self.pose(now);
        let (w, h) = self.size;
        let m = self.m;
        let s = self.scale;
        let (o_in, o_out) = (self.origin(self.entry.origin), self.origin(self.exit.origin));
        let (ox, oy) = if self.closing.is_some() { o_out } else { o_in };
        let look = self.look;
        let age = now.saturating_sub(self.t0);
        let leaving = self.closing.map_or(0, |(tc, _)| now.saturating_sub(tc).max(1));
        let (win, hover, press) = (self.win, self.hover, self.press);

        // the shadow is painted once, at rest; its opacity follows the panel's
        if !self.shade_done {
            self.shade_done = true;
            let (sw, sh) = (self.sh.2, self.sh.3);
            let pad = (self.at.0 - self.sh.0) as f32 / s;
            let r = Rect::new(pad, pad, w, h);
            let _ = g.paint(&mut self.shade, sw, sh, |c| {
                c.shadow(r, look.radius, look.shadow.dy, look.shadow.blur, look.shadow.color);
                c.ring(r.inset(-1.0), look.radius + 1.0, 1.0, look.ring_out);
                c.erase(r, look.radius);
            });
        }

        let Self { content, mo, cap, glass, surf, .. } = self;
        let _ = g.paint(surf, win.2, win.3, |c| {
            if let Some((px, (cx, cy, cw, ch))) = cap.take() {
                *glass = c.glass(&px, cw, ch, look.blur, look.saturate).map(|p| (p, cx, cy));
            }
            let mut f = Frame { now, age, leaving, hover, press, size: (w, h), mo };
            let clip = pose.clip.map_or(Rect::new(m, m, w, h), |i| Rect::new(m + i.left, m + i.top, w - i.left - i.right, h - i.top - i.bottom));
            let radius = pose.clip.map_or(look.radius, |i| i.radius);
            let xf = Xf { cx: m + ox, cy: m + oy, k: pose.k, dx: pose.dx, dy: pose.dy };
            c.faded(pose.opacity, |c| {
                c.masked(clip, radius, xf, |c| {
                    if let Some((p, gx, gy)) = glass.as_ref() {
                        c.picture(p, (*gx - win.0) as f32 / s, (*gy - win.1) as f32 / s);
                    }
                    c.transformed(0.0, 0.0, 1.0, m, m, |c| {
                        c.transformed(ox, oy, pose.k, pose.dx, pose.dy, |c| {
                            let tint = if glass.is_some() { look.tint } else { look.tint.alpha(look.tint.a.max(0.97)) };
                            c.fill(Rect::new(0.0, 0.0, w, h), look.radius, tint);
                            c.ring(Rect::new(0.0, 0.0, w, h), look.radius, 1.0, look.ring_in);
                            content.draw(c, &mut f);
                        });
                    });
                });
            });
        });

        let shadow_k = if pose.clip.is_some() { open } else { pose.opacity };
        let shown = Shown {
            win: self.win,
            shadow: self.sh,
            shadow_alpha: (255.0 * shadow_k.clamp(0.0, 1.0)).round() as u8,
            busy: self.busy(now),
            done: self.finished(now),
            pulse: if self.closing.is_some() { 0 } else { self.content.pulse() },
        };
        self.last = shown;
        shown
    }
}

/// The corner of the box nearest to its button, in the box's own coordinates (what `transform-origin: 100% 100%` means for a
/// panel that grows out of the button below it).
fn button_origin_of(side: Side, button: (i32, i32, i32, i32), at: (i32, i32), size: (f32, f32), s: f32) -> (f32, f32) {
    let (bx, by) = (((button.0 + button.2) as f32 / 2.0 - at.0 as f32) / s, ((button.1 + button.3) as f32 / 2.0 - at.1 as f32) / s);
    match side {
        Side::Bottom => (bx.clamp(0.0, size.0), size.1),
        Side::Left => (0.0, by.clamp(0.0, size.1)),
        Side::Right => (size.0, by.clamp(0.0, size.1)),
    }
}

fn button_origin(a: &Anchor, at: (i32, i32), size: (f32, f32), s: f32) -> (f32, f32) {
    button_origin_of(a.side, a.button, at, size, s)
}

/// The top left corner of a `size` px box next to its button.
fn place_box(a: &Anchor, (w, h): (i32, i32), p: Place, s: f32) -> (i32, i32) {
    let gap = (p.gap * s).round() as i32;
    let (l, t, r, b) = a.button;
    let edge = (8.0 * s).round() as i32;
    let along = |lo: i32, hi: i32, len: i32, screen: i32| match p.along {
        Along::Center => (lo + hi) / 2 - len / 2,
        Along::Start => lo,
        Along::End => hi - len,
        Along::ScreenEnd => screen - gap - len,
    };
    match a.side {
        Side::Bottom => ((along(l, r, w, a.screen.0)).clamp(edge, (a.screen.0 - edge - w).max(edge)), a.bar.1 - gap - h),
        Side::Left => (a.bar.2 + gap, along(t, b, h, a.screen.1).clamp(edge, (a.screen.1 - edge - h).max(edge))),
        Side::Right => (a.bar.0 - gap - w, along(t, b, h, a.screen.1).clamp(edge, (a.screen.1 - edge - h).max(edge))),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the windows

struct Host {
    gfx: Gfx,
    popup: Option<Popup>,
    grab: Surface,
    panel: HWND,
    shadow: HWND,
    bar: HWND,
    notify: fn(),
    scale: f32,
    /// A panel closed by losing the focus, and when: its own button must not open it again at once.
    deact: Option<(Id, u64)>,
    timer: bool,
    /// The period of the live-value timer (the content's `pulse`), 0 = none.
    pulsing: u32,
    shown: (i32, i32, i32, i32, u8),
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|h| h.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn now() -> u64 {
    // SAFETY: plain query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

pub(super) fn whwnd(h: HWND) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(h.cast())
}

/// Creates the two windows. `bar` is the bar's window (kept above the panels); `notify` is called when a panel opens or
/// closes, so the bar redraws its lit button.
pub fn init(hi: HINSTANCE, scale: f32, bar: HWND, notify: fn()) -> bool {
    let Ok(gfx) = Gfx::new(scale) else { return false };
    let name: Vec<u16> = "topbar_panel\0".encode_utf16().collect();
    let shade: Vec<u16> = "topbar_panel_shadow\0".encode_utf16().collect();
    // SAFETY: class and window creation with live, NUL-terminated names.
    let (panel, shadow) = unsafe {
        register_class(hi, &name, Some(proc), true);
        register_class(hi, &shade, Some(DefWindowProcW), false);
        let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED;
        let panel = CreateWindowExW(ex, name.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 1, 1, std::ptr::null_mut(), std::ptr::null_mut(), hi, std::ptr::null());
        let shadow = CreateWindowExW(
            ex | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            shade.as_ptr(),
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
        );
        (panel, shadow)
    };
    if panel.is_null() || shadow.is_null() {
        return false;
    }
    HOST.set(Some(Host { gfx, popup: None, grab: Surface::new(), panel, shadow, bar, notify, scale, deact: None, timer: false, pulsing: 0, shown: (0, 0, 0, 0, 0) }));
    true
}

/// Registers a window class of this module (an arrow cursor for the one that takes the mouse).
pub(super) fn register_class(hi: HINSTANCE, name: &[u16], proc: windows_sys::Win32::UI::WindowsAndMessaging::WNDPROC, cursor: bool) {
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

/// Opens `content` next to its button, replacing a panel that is open.
pub fn open(content: Box<dyn Content>, anchor: Anchor) {
    let replaced = with(|h| h.popup.is_some()).unwrap_or(false);
    if replaced {
        close_now();
        // SAFETY: waits for the compositor to take the old panel off the screen, so the grab below does not see it.
        unsafe { DwmFlush() };
    }
    with(|h| {
        let t = now();
        let Host { gfx, grab, .. } = h;
        let mut grabber = |x, y, w, ht| if grab.grab(x, y, w, ht) { Some(grab.pixels().to_vec()) } else { None };
        let p = Popup::new(gfx, content, anchor, t, &mut grabber);
        h.popup = Some(p);
        h.shown = (0, 0, 0, 0, 0);
        h.frame(t);
        // SAFETY: z-order and focus of our own windows: shadow under the panel, the bar above both.
        unsafe {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            SetWindowPos(h.shadow, HWND_TOPMOST, 0, 0, 0, 0, flags | SWP_SHOWWINDOW);
            SetWindowPos(h.panel, HWND_TOPMOST, 0, 0, 0, 0, flags | SWP_SHOWWINDOW);
            SetWindowPos(h.shadow, h.panel, 0, 0, 0, 0, flags);
            SetWindowPos(h.bar, HWND_TOPMOST, 0, 0, 0, 0, flags);
            winrust_common::fg::force(h.panel); // WM_ACTIVATE tells us when the user clicks elsewhere, and Esc needs the focus
        }
        (h.notify)();
    });
}

/// A click on the panel's own bar button: closes the panel if it is open, opens it otherwise. `build` makes the content
/// and says where it goes (`None` = cannot open now).
pub fn toggle(id: Id, build: impl FnOnce() -> Option<(Box<dyn Content>, Anchor)>) {
    let state = with(|h| {
        let guard = h.deact.take().is_some_and(|(d, t)| d == id && now().saturating_sub(t) < 400);
        (h.popup.as_ref().is_some_and(|p| p.id() == id && !p.is_closing()), guard)
    });
    match state {
        Some((true, _)) => close(),
        // the click that closed it by taking the focus is the same click: it must not open it again (#140)
        Some((false, true)) => {}
        _ => {
            if let Some((c, a)) = build() {
                open(c, a);
            }
        }
    }
}

/// Closes the open panel with its exit animation.
pub fn close() {
    with(|h| {
        let t = now();
        if h.popup.as_mut().is_some_and(|p| p.close(t)) {
            h.frame(t);
            (h.notify)();
        }
    });
}

/// Closes the open panel without any animation (another panel replaces it, a full-screen app needs the screen).
pub fn close_now() {
    with(Host::hide);
}

/// The panel is open (and not on its way out).
#[must_use]
pub fn is_open(id: Id) -> bool {
    with(|h| h.popup.as_ref().is_some_and(|p| p.id() == id && !p.is_closing())).unwrap_or(false)
}

#[must_use]
pub fn any_open() -> bool {
    with(|h| h.popup.as_ref().is_some_and(|p| !p.is_closing())).unwrap_or(false)
}

impl Host {
    /// Paints a frame and puts it on screen.
    fn frame(&mut self, t: u64) {
        let Some(p) = self.popup.as_mut() else { return };
        let shown = p.frame(&mut self.gfx, t);
        let (win, sh) = (shown.win, shown.shadow);
        let s = whwnd(self.shadow);
        let key = (sh.0, sh.1, sh.2, sh.3, shown.shadow_alpha);
        if key != self.shown {
            self.shown = key;
            let _ = p.surfaces().1.present(s, sh.0, sh.1, shown.shadow_alpha);
        }
        let _ = p.surfaces().0.present(whwnd(self.panel), win.0, win.1, 255);
        let done = shown.done;
        if done {
            self.hide();
        } else {
            self.timer(shown.busy);
            self.pulse(shown.pulse);
        }
    }

    fn pulse(&mut self, ms: u32) {
        if ms != self.pulsing {
            self.pulsing = ms;
            // SAFETY: a timer on our own window.
            unsafe {
                if ms > 0 {
                    SetTimer(self.panel, 2, ms, None);
                } else {
                    KillTimer(self.panel, 2);
                }
            }
        }
    }

    fn timer(&mut self, on: bool) {
        if on != self.timer {
            self.timer = on;
            // SAFETY: a timer on our own window.
            unsafe {
                if on {
                    SetTimer(self.panel, 1, 16, None);
                } else {
                    KillTimer(self.panel, 1);
                }
            }
        }
    }

    fn hide(&mut self) {
        let had = self.popup.take().is_some();
        self.timer(false);
        self.pulse(0);
        // SAFETY: hiding our own windows.
        unsafe {
            ShowWindow(self.panel, SW_HIDE);
            ShowWindow(self.shadow, SW_HIDE);
        }
        if had {
            (self.notify)();
        }
    }
}

fn apply(cx: Cx) {
    if cx.close {
        close();
    }
    if let Some(f) = cx.then {
        f();
    }
}

/// Mouse position of a message, client device pixels.
fn xy(lp: LPARAM) -> (i32, i32) {
    ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return 1,
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            // SAFETY: asks for WM_MOUSELEAVE.
            unsafe {
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
            }
            hovered(|p| p.pointer(x, y));
        }
        WM_MOUSELEAVE => hovered(Popup::leave),
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (x, y) = xy(lp);
            let b = match msg {
                WM_LBUTTONDOWN => Button::Left,
                WM_RBUTTONDOWN => Button::Right,
                _ => Button::Middle,
            };
            let cx = with(|h| h.popup.as_mut().map(|p| p.button(x, y, b))).flatten();
            if let Some(cx) = cx {
                with(|h| h.frame(now()));
                // a held item (a slider) follows the pointer beyond the window
                if with(|h| h.popup.as_ref().is_some_and(Popup::pressed)).unwrap_or(false) {
                    // SAFETY: capturing the mouse for our own window.
                    unsafe { SetCapture(hwnd) };
                }
                apply(cx);
            }
        }
        0x0202 | 0x0205 | 0x0208 => {
            // WM_xBUTTONUP
            // SAFETY: releases the capture taken above (a no-op without one).
            unsafe { ReleaseCapture() };
            with(|h| {
                h.popup.as_mut().map(Popup::release);
                h.frame(now());
            });
        }
        WM_MOUSEWHEEL => {
            let up = (wp >> 16) as i16 > 0;
            with(|h| {
                if let Some(p) = h.popup.as_mut() {
                    p.wheel(up);
                    h.frame(now());
                }
            });
        }
        WM_CHAR => {
            let cx = char::from_u32(wp as u32).filter(|c| !c.is_control()).and_then(|c| with(|h| h.popup.as_mut().map(|p| p.text(c))).flatten());
            if let Some(cx) = cx {
                with(|h| h.frame(now()));
                apply(cx);
            }
        }
        WM_KEYDOWN => {
            let cx = with(|h| h.popup.as_mut().map(|p| p.key(wp as u32))).flatten();
            if let Some(cx) = cx {
                with(|h| {
                    h.sync_tip();
                    h.frame(now());
                });
                apply(cx);
            }
        }
        // the focus went elsewhere: the panel goes (a click on its own button then must not bring it back)
        WM_ACTIVATE if wp & 0xffff == 0 => {
            with(|h| {
                if let Some(id) = h.popup.as_ref().filter(|p| !p.is_closing()).map(Popup::id) {
                    h.deact = Some((id, now()));
                }
            });
            close();
        }
        WM_TIMER => _ = with(|h| h.frame(now())),
        _ => {
            // SAFETY: the default procedure of our own window.
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
    }
    0
}

/// Runs a pointer change on the open panel and, if it changed what is hovered, repaints and moves the tooltip.
fn hovered(f: impl FnOnce(&mut Popup) -> bool) {
    with(|h| {
        if h.popup.as_mut().is_some_and(f) {
            h.sync_tip();
            h.frame(now());
        }
    });
}

impl Host {
    /// The tooltip of the hovered item (a panel can have one per item), shown above the panel after the usual delay.
    fn sync_tip(&mut self) {
        let Some(p) = self.popup.as_ref() else { return };
        let Some((text, along, top)) = p.tip() else { return super::tip::hide() };
        let place = super::tip::Place { side: Side::Bottom, along, edge: top - (8.0 * self.scale).round() as i32 };
        super::tip::show(&text, place, 500, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tray_popup::{Cell, TrayPanel};
    use windows::Win32::UI::WindowsAndMessaging::HICON;

    fn cells() -> Vec<Cell> {
        (0..6).map(|i| Cell { icon: HICON(std::ptr::null_mut()), tip: format!("icon {i}"), token: i }).collect()
    }

    #[test]
    fn a_popup_stands_above_its_button_and_answers_clicks() {
        // SAFETY: COM for WIC on this test thread.
        unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED).ok().ok() };
        let mut g = Gfx::new(1.0).unwrap();
        let anchor = Anchor { side: Side::Bottom, button: (1000, 846, 1032, 886), bar: (0, 846, 1440, 900), screen: (1440, 900) };
        let mut grab = |_x, _y, w: i32, h: i32| Some(vec![0xff10_1010u32; (w * h) as usize]);
        let mut p = Popup::new(&mut g, Box::new(TrayPanel::new(cells, |_, _| {})), anchor, 0, &mut grab);
        let (x, y, w, h) = p.rect();
        assert_eq!((w, h), (188, 100)); // 4 columns of 40, two rows, padding 8, gap 4
        assert_eq!(y + h, 846 - 12); // 12 above the bar
        assert_eq!(x + w / 2, 1016); // centred on the button
        let shown = p.frame(&mut g, 0);
        assert!(shown.busy && !shown.done);
        assert!(p.pointer(8 + 20 + 4 + 40 + 4 + 10, 8 + 20 + 7 /* in the window: room m around the box */ + 2));
        p.close(500);
        assert!(p.is_closing());
        assert!(p.frame(&mut g, 600).done, "the tray has no exit animation");
    }
}
