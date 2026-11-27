use std::borrow::Cow;
use std::collections::HashMap;
use windows_sys::Win32::Foundation::HINSTANCE;
use windows_sys::Win32::UI::WindowsAndMessaging::{IDC_ARROW, LoadCursorW, RegisterClassW, WNDCLASSW};
use winrust_common::style::Anim;
use crate::anim::{self, CubicBezier, EASE, POP, SPRING, Transition};
use crate::gfx::{Canvas, Rect};
use crate::widgets::overlay::{Dialog, MenuItem};

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
    /// What an application window is asked to do after the call (`Cx::menu`, `Cx::toast`..); the panels of the bar leave it empty.
    pub reqs: Vec<Req>,
    /// The application window (0 for a panel), for what needs an owner (a shell context menu, a file operation, UAC).
    pub hwnd: isize,
    /// Alt was held with the key (the panels never asked: `Mods` is theirs and stays as it is).
    pub alt: bool,
}

/// What an application asks of its window through [`Cx`]; the host does it once the call has returned.
pub enum Req {
    Invalidate,
    /// A one-shot timer: `App::wake` runs after this many milliseconds.
    Wake(u32),
    Menu { tag: u32, items: Vec<MenuItem>, at: Rect },
    Toast { text: String, undo: Option<u32>, bottom: f32 },
    Dialog { tag: u32, dialog: Dialog },
    Focus(Id),
    Title(String),
    Cursor(Cursor),
    Clipboard(String),
    Close,
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

    /// Draw one more frame.
    pub fn invalidate(&mut self) {
        self.reqs.push(Req::Invalidate);
    }

    /// Call `App::wake` once after `ms` milliseconds.
    pub fn wake_in(&mut self, ms: u32) {
        self.reqs.push(Req::Wake(ms));
    }

    /// Open a menu next to `at` (an item's rectangle, or a point for a context menu); a choice comes back as
    /// `App::command(tag, choice)`, `choice` being the item's `value` (see [`MenuItem`]).
    pub fn menu(&mut self, tag: u32, items: Vec<MenuItem>, at: Rect) {
        self.reqs.push(Req::Menu { tag, items, at });
    }

    /// A toast at the window's bottom; Undo comes back as `App::command(undo, 0)`.
    pub fn toast(&mut self, text: impl Into<String>, undo: Option<u32>) {
        self.toast_at(text, undo, 18.0);
    }

    /// Like [`Cx::toast`], `bottom` DIPs above the window's lower edge.
    pub fn toast_at(&mut self, text: impl Into<String>, undo: Option<u32>, bottom: f32) {
        self.reqs.push(Req::Toast { text: text.into(), undo, bottom });
    }

    /// A dialog over a scrim; the button pressed comes back as `App::command(tag, button)`.
    pub fn dialog(&mut self, tag: u32, dialog: Dialog) {
        self.reqs.push(Req::Dialog { tag, dialog });
    }

    /// Moves the keyboard focus to item `id` (shown with its ring as after Tab).
    pub fn focus(&mut self, id: Id) {
        self.reqs.push(Req::Focus(id));
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.reqs.push(Req::Title(title.into()));
    }

    /// The cursor until the pointer moves to another item.
    pub fn cursor(&mut self, c: Cursor) {
        self.reqs.push(Req::Cursor(c));
    }

    pub fn set_clipboard_text(&mut self, text: impl Into<String>) {
        self.reqs.push(Req::Clipboard(text.into()));
    }

    /// The text on the clipboard, if there is any.
    #[must_use]
    pub fn clipboard_text(&self) -> Option<String> {
        crate::window::clipboard_text(self.hwnd)
    }

    /// Close the application window (after `App::close` has agreed).
    pub fn close_window(&mut self) {
        self.reqs.push(Req::Close);
    }

    /// The window, for what needs an owner (0 outside a window).
    #[must_use]
    pub const fn hwnd(&self) -> isize {
        self.hwnd
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

/// An item of a window: what the app named with `f.item` while it drew.
pub type Id = i32;

/// The three window buttons; the host handles them, the caption widget draws them.
pub const WIN_MIN: Id = -1;
pub const WIN_MAX: Id = -2;
pub const WIN_CLOSE: Id = -3;
/// Ids from here down belong to the overlays (menus, dialog, toast).
pub const OVERLAY: Id = -1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Arrow,
    Hand,
    IBeam,
    SizeWE,
    SizeNS,
    Move,
}

/// What the window knows of an item from the frame it drew last: where it is, and what the pointer may do over it.
#[derive(Clone, Debug)]
pub struct Hit {
    pub id: Id,
    pub r: Rect,
    pub tip: Option<Cow<'static, str>>,
    pub cursor: Cursor,
    pub focusable: bool,
}

/// The items of the last frame in drawing order, which is also their stacking order: the last one under the pointer wins.
#[derive(Default)]
pub struct Hits {
    list: Vec<Hit>,
    clips: Vec<Rect>,
    pub focus: Option<Id>,
    /// Keyboard navigation shows the focus ring (`:focus-visible`); the mouse hides it again.
    pub focus_visible: bool,
}

impl Hits {
    /// Starts a frame.
    pub fn clear(&mut self) {
        self.list.clear();
        self.clips.clear();
    }

    /// The item under a point (DIPs), the topmost.
    #[must_use]
    pub fn at(&self, x: f32, y: f32) -> Option<&Hit> {
        self.list.iter().rev().find(|h| h.r.contains(x, y))
    }

    #[must_use]
    pub fn get(&self, id: Id) -> Option<&Hit> {
        self.list.iter().rev().find(|h| h.id == id)
    }

    #[must_use]
    pub fn all(&self) -> &[Hit] {
        &self.list
    }

    /// The items Tab walks through, in drawing order.
    pub fn tab_order(&self) -> impl Iterator<Item = Id> + '_ {
        self.list.iter().filter(|h| h.focusable).map(|h| h.id)
    }

    fn clip(&self) -> Option<Rect> {
        self.clips.last().copied()
    }
}

/// How an item looks this frame: the CSS states of the mockup's buttons, eased.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hot {
    /// 0..1, `:hover` over 160 ms.
    pub hover: f32,
    /// 0..1, `:active` over 90 ms.
    pub press: f32,
    /// `:focus-visible`.
    pub focus: bool,
}

fn intersect(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
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

    /// The window is maximized: a caption draws "restore" for "maximize".
    #[must_use]
    pub fn maximized(&self) -> bool {
        self.mo.maximized
    }

    /// The window has the keyboard focus.
    #[must_use]
    pub fn active(&self) -> bool {
        self.mo.active
    }

    /// A menu of the window is open.
    #[must_use]
    pub fn menu_open(&self) -> bool {
        self.mo.menu_open
    }

    /// Names an item of an application window: records where it is (cut to the clip in force) so the host can hit-test it,
    /// and gives its eased hover and press. Items drawn later stand over those drawn earlier.
    pub fn item(&mut self, id: Id, r: Rect) -> Hot {
        let r = self.mo.hits.clip().map_or(r, |c| intersect(r, c));
        if r.w > 0.0 && r.h > 0.0 {
            self.mo.hits.list.push(Hit { id, r, tip: None, cursor: Cursor::Arrow, focusable: false });
        }
        let hover = self.hover(id, 160, EASE);
        let press = self.tween(id, 1, f32::from(self.press == Some(id)), 90, SPRING);
        Hot { hover, press, focus: self.mo.hits.focus == Some(id) && self.mo.hits.focus_visible }
    }

    /// Like [`Frame::item`], and Tab stops here.
    pub fn focusable(&mut self, id: Id, r: Rect) -> Hot {
        let hot = self.item(id, r);
        if let Some(h) = self.mo.hits.list.last_mut().filter(|h| h.id == id) {
            h.focusable = true;
        }
        hot
    }

    /// The hand over the item just named (CSS `cursor: pointer`).
    pub fn hand(&mut self) {
        self.cursor(Cursor::Hand);
    }

    /// The cursor over the item just named.
    pub fn cursor(&mut self, c: Cursor) {
        if let Some(h) = self.mo.hits.list.last_mut() {
            h.cursor = c;
        }
    }

    /// The hint that appears after the pointer rests on the item just named (CSS `title`).
    pub fn tip(&mut self, text: impl Into<Cow<'static, str>>) {
        if let Some(h) = self.mo.hits.list.last_mut() {
            h.tip = Some(text.into());
        }
    }

    /// Runs `body` with everything it draws and every item it names cut to `r` (a scrolled body, a list).
    pub fn clipped(&mut self, c: &mut Canvas<'_>, r: Rect, body: impl FnOnce(&mut Canvas<'_>, &mut Self)) {
        let r = self.mo.hits.clip().map_or(r, |k| intersect(r, k));
        self.mo.hits.clips.push(r);
        c.clipped(r, |c| body(c, self));
        self.mo.hits.clips.pop();
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
    /// The items an application window drew (the panels of the bar do not name any).
    pub hits: Hits,
    /// The application window is maximized (it draws its restore button, its corners are square).
    pub maximized: bool,
    /// The window has the keyboard focus.
    pub active: bool,
    /// A menu of the window is open (its button is drawn pressed in).
    pub menu_open: bool,
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

