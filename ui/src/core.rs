//! An application window without the window: what the app draws, what the pointer is over, which item is pressed or focused, the
//! overlays in front, and what a click or a key does. [`crate::window`] feeds it Win32 messages; `--render` and the tests feed it
//! the same calls by hand, which is why an [`App`] never sees an `HWND`.

use std::path::PathBuf;

use crate::frame::{Button, Cursor, Cx, Frame, Hits, Id, Mods, Motion, Req, WIN_CLOSE, WIN_MAX, WIN_MIN};
use crate::gfx::{Canvas, Rect, Rgba};
use crate::widgets::overlay::{OvKey, Overlays};
use winrust_common::style;

/// The corner radius of an application window (the mockups' `border-radius: 16px`); a maximized window has none.
pub const RADIUS: f32 = 16.0;
/// The strip along the edge of the window where the pointer resizes it, DIPs.
pub const EDGE: f32 = 6.0;

/// Ids of an app's own items are 0 and up; the window's buttons and the overlays use the negative ones.
/// An application: it draws itself and answers what happens to it. It is told nothing of windows, so it can be driven by tests.
pub trait App {
    fn title(&self) -> String;
    fn min_size(&self) -> (f32, f32) {
        (560.0, 380.0)
    }
    /// The height of the caption strip (the mockups' 48): what the empty part of it, without an item, does is drag the window.
    fn caption_height(&self) -> f32 {
        48.0
    }
    /// Draws the window's content into `c` (DIPs, `f.size` big) and names every item it has with `f.item`.
    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>);
    /// A click: the button went down and up on item `id`; `(x, y)` is where it went up.
    fn click(&mut self, _id: Id, _b: Button, _x: f32, _y: f32, _cx: &mut Cx) {}
    /// The button went down on `id` (a slider takes hold, a field puts its caret).
    fn down(&mut self, _id: Id, _b: Button, _x: f32, _y: f32, _cx: &mut Cx) {}
    fn double(&mut self, _id: Id, _cx: &mut Cx) {}
    /// The pointer moved with the button held that went down on `id`.
    fn drag(&mut self, _id: Id, _x: f32, _y: f32, _cx: &mut Cx) {}
    fn release(&mut self, _id: Id, _cx: &mut Cx) {}
    /// The wheel, in notches (a notch is 1.0; a touchpad gives fractions); `id` is the item under the pointer.
    fn wheel(&mut self, _id: Option<Id>, _dx: f32, _dy: f32, _mods: Mods, _cx: &mut Cx) {}
    /// A key went down; true when the app used it (Tab, Enter and Space on a focused item are done by the window otherwise).
    fn key(&mut self, _vk: u32, _mods: Mods, _cx: &mut Cx) -> bool {
        false
    }
    /// Typed text: `WM_CHAR` (a surrogate pair joined) or what an IME composed.
    fn text(&mut self, _s: &str, _cx: &mut Cx) {}
    /// A menu row, a dialog button or Undo was chosen: `tag` is what the app named it with in `Cx::menu`/`dialog`.
    fn command(&mut self, _tag: u32, _choice: usize, _cx: &mut Cx) {}
    /// A message a worker thread posted with [`crate::window::post`] (`WM_APP + 1 ..= WM_APP + 99`).
    fn message(&mut self, _msg: u32, _wp: usize, _lp: isize, _cx: &mut Cx) {}
    /// The timer of `Cx::wake_in` fired.
    fn wake(&mut self, _cx: &mut Cx) {}
    fn drag_over(&mut self, _paths: &[String], _x: f32, _y: f32) -> bool {
        false
    }
    /// Files were dropped on the window.
    fn drop(&mut self, _paths: Vec<String>, _x: f32, _y: f32, _cx: &mut Cx) {}
    /// The window is about to close; false refuses (unsaved work, a copy in progress).
    fn close(&mut self, _cx: &mut Cx) -> bool {
        true
    }
    /// The window exists (its first frame is not drawn yet): `hwnd` is what a worker thread posts to ([`crate::window::post`]).
    fn start(&mut self, _hwnd: isize) {}
    /// The wheel with the pointer's place in the window (DIPs): zooming about the pointer needs it. By default it is `wheel`.
    #[allow(clippy::too_many_arguments)]
    fn wheel_at(&mut self, id: Option<Id>, _x: f32, _y: f32, dx: f32, dy: f32, mods: Mods, cx: &mut Cx) {
        self.wheel(id, dx, dy, mods, cx);
    }
    /// What is dragged over the window left it without dropping.
    fn drag_leave(&mut self, _cx: &mut Cx) {}
    /// `--render`: the items the renderer clicks, one after the other, before it takes the picture (what a state of the design needs
    /// open: a menu, a toast).
    fn demo_clicks(&self) -> Vec<Id> {
        Vec::new()
    }
}

/// What Windows is told about a point of the window (`WM_NCHITTEST`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nc {
    Client,
    /// The empty part of the caption: it drags the window, a double click maximizes it.
    Caption,
    /// Over the maximize button: Windows 11 shows its Snap Layouts for it.
    MaxButton,
    Top,
    TopLeft,
    TopRight,
    Left,
    Right,
    Bottom,
    BottomLeft,
    BottomRight,
}

/// What the host must do that the core cannot.
pub enum HostReq {
    Title(String),
    Clipboard(String),
    Close,
    Wake(u32),
}

/// What a click on a window button asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Minimize,
    ToggleMaximize,
    Close,
}

pub struct Core {
    pub app: Box<dyn App>,
    pub mo: Motion,
    pub ov: Overlays,
    /// The client area, DIPs, and its scale.
    pub size: (f32, f32),
    pub scale: f32,
    pub maximized: bool,
    pub active: bool,
    pub hover: Option<Id>,
    press: Option<(Id, Button)>,
    pub pointer: Option<(f32, f32)>,
    last_click: Option<(Id, u64, f32, f32)>,
    cursor: Option<Cursor>,
    pub hwnd: isize,
    /// Shift and Ctrl at the last click or key: what the host tells it (`Cx::mods`).
    pub held: Mods,
    born: u64,
    redraw: bool,
    host: Vec<HostReq>,
}

impl Core {
    #[must_use]
    pub fn new(app: Box<dyn App>, size: (f32, f32), scale: f32, now: u64) -> Self {
        Self {
            app,
            mo: Motion::default(),
            ov: Overlays::default(),
            size,
            scale,
            maximized: false,
            active: true,
            hover: None,
            press: None,
            pointer: None,
            last_click: None,
            cursor: None,
            hwnd: 0,
            held: Mods::default(),
            born: now,
            redraw: false,
            host: Vec::new(),
        }
    }

    fn cx(&self) -> Cx {
        Cx { scale: self.scale, hwnd: self.hwnd, focus: self.mo.hits.focus, mods: self.held, pointer: self.pointer, ..Cx::default() }
    }

    pub fn hits(&self) -> &Hits {
        &self.mo.hits
    }

    /// Draws one frame: the app, the overlays, then the window's own shape (round corners, the hairline ring).
    pub fn draw(&mut self, c: &mut Canvas<'_>, now: u64) {
        self.mo.mode = Some(style::anim());
        self.mo.hits.clear();
        self.mo.due = None;
        self.mo.maximized = self.maximized;
        self.mo.active = self.active;
        self.mo.menu_open = self.ov.menu_open();
        let (w, h) = self.size;
        let mut f = Frame { now, age: now.saturating_sub(self.born), leaving: 0, hover: self.hover, press: self.press.map(|p| p.0), size: self.size, opacity: 1.0, mo: &mut self.mo };
        self.app.draw(c, &mut f);
        self.ov.draw(c, &mut f, Rect::new(0.0, 0.0, w, h), now);
        let win = Rect::new(0.0, 0.0, w, h);
        if !self.maximized {
            c.round_corners(win, RADIUS);
            c.ring(win, RADIUS, 1.0, Rgba::white(0.08));
        }
        self.mo.sweep(now);
        self.redraw = false;
    }

    /// Frames must go on: something moves.
    #[must_use]
    pub fn wants_frames(&self, now: u64) -> bool {
        self.redraw || self.mo.running(now) || self.ov.animating(now, self.mo.mode.unwrap_or(style::Anim::Full))
    }

    /// The time a frame is due though nothing moves (a hint's delay, a toast's end).
    #[must_use]
    pub fn deadline(&self, now: u64) -> Option<u64> {
        match (self.ov.deadline(now), self.mo.due.filter(|&d| d > now)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Where the focused text field's caret is, in DIPs of the client (the IME puts its windows there).
    #[must_use]
    pub fn ime_caret(&self) -> Option<Rect> {
        self.mo.hits.caret
    }

    /// An `invalidate` has not been answered with a frame yet.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    pub fn take_host(&mut self) -> Vec<HostReq> {
        std::mem::take(&mut self.host)
    }

    pub fn ask_redraw(&mut self) {
        self.redraw = true;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the pointer

    fn sync_tip(&mut self, now: u64) {
        let tip = self.hover.and_then(|id| self.mo.hits.get(id)).and_then(|h| h.tip.clone().map(|t| (h.id, t, h.r)));
        self.ov.set_tip(tip, now);
    }

    /// The pointer is at `(x, y)` DIPs of the client; true when something changed that needs a frame.
    pub fn pointer_move(&mut self, x: f32, y: f32, now: u64) -> bool {
        self.pointer = Some((x, y));
        self.cursor = None;
        let over = self.mo.hits.at(x, y).map(|h| h.id);
        let mut changed = over != self.hover;
        if changed {
            self.hover = over;
            self.ov.hover(over, now);
            self.sync_tip(now);
        }
        if let Some((id, _)) = self.press.filter(|p| p.0 >= 0) {
            let mut cx = self.cx();
            self.app.drag(id, x, y, &mut cx);
            self.finish(cx, now);
            changed = true;
        }
        changed
    }

    /// The pointer left the window.
    pub fn pointer_leave(&mut self, now: u64) -> bool {
        self.pointer = None;
        let changed = self.hover.take().is_some();
        if changed {
            self.ov.hover(None, now);
            self.sync_tip(now);
        }
        changed
    }

    /// The pointer is over a window button that Windows owns (the non-client maximize button): the same hover, from another message.
    pub fn set_hover(&mut self, id: Option<Id>, now: u64) -> bool {
        if self.hover == id {
            return false;
        }
        self.hover = id;
        self.sync_tip(now);
        true
    }

    pub fn set_press(&mut self, id: Option<Id>) {
        self.press = id.map(|i| (i, Button::Left));
    }

    #[must_use]
    pub fn pressed(&self) -> Option<Id> {
        self.press.map(|p| p.0)
    }

    pub fn down(&mut self, b: Button, x: f32, y: f32, now: u64) {
        self.pointer_move(x, y, now);
        let id = self.hover;
        self.press = id.map(|i| (i, b));
        self.mo.hits.focus_visible = false;
        if b == Button::Left {
            self.mo.hits.focus = id.filter(|&i| self.mo.hits.get(i).is_some_and(|h| h.focusable));
        }
        if let Some(id) = id.filter(|&i| i >= 0) {
            let mut cx = self.cx();
            self.app.down(id, b, x, y, &mut cx);
            self.finish(cx, now);
        }
        self.ov.set_tip(None, now);
    }

    /// The button went up; the window action a click on one of the window's own buttons asks for.
    pub fn up(&mut self, _b: Button, x: f32, y: f32, now: u64) -> Option<Act> {
        let (pid, pb) = self.press.take()?;
        let over = self.mo.hits.at(x, y).map(|h| h.id);
        if pid >= 0 {
            let mut cx = self.cx();
            self.app.release(pid, &mut cx);
            self.finish(cx, now);
        }
        if over != Some(pid) {
            return None;
        }
        match pid {
            WIN_MIN => return Some(Act::Minimize),
            WIN_MAX => return Some(Act::ToggleMaximize),
            WIN_CLOSE => return Some(Act::Close),
            _ => {}
        }
        if Overlays::owns(pid) {
            if let Some((tag, choice)) = self.ov.click(pid, now) {
                self.command(tag, choice, now);
            }
            self.redraw = true;
            return None;
        }
        let mut cx = self.cx();
        self.app.click(pid, pb, x, y, &mut cx);
        let twice = self.last_click.is_some_and(|(id, t, lx, ly)| id == pid && now.saturating_sub(t) < 500 && (lx - x).abs() < 4.0 && (ly - y).abs() < 4.0);
        if twice && pb == Button::Left {
            self.app.double(pid, &mut cx);
            self.last_click = None;
        } else {
            self.last_click = Some((pid, now, x, y));
        }
        self.finish(cx, now);
        self.redraw = true;
        None
    }

    /// Wheel notches over the window (positive `dy` scrolls up, as the wheel reports it).
    pub fn wheel(&mut self, dx: f32, dy: f32, mods: Mods, now: u64) {
        self.held = mods;
        if self.ov.modal() {
            return;
        }
        let id = self.hover.filter(|&i| !Overlays::owns(i));
        let (x, y) = self.pointer.unwrap_or_default();
        let mut cx = self.cx();
        self.app.wheel_at(id, x, y, dx, dy, mods, &mut cx);
        self.finish(cx, now);
        self.redraw = true;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the keyboard

    fn command(&mut self, tag: u32, choice: usize, now: u64) {
        let mut cx = self.cx();
        self.app.command(tag, choice, &mut cx);
        self.finish(cx, now);
    }

    /// A key went down; true when it was used.
    pub fn key(&mut self, vk: u32, mods: Mods, alt: bool, now: u64) -> bool {
        self.held = mods;
        const TAB: u32 = 0x09;
        const ENTER: u32 = 0x0D;
        const ESC: u32 = 0x1B;
        const SPACE: u32 = 0x20;
        match self.ov.key(vk, now) {
            OvKey::Command(tag, choice) => {
                self.command(tag, choice, now);
                self.redraw = true;
                return true;
            }
            OvKey::Taken => {
                self.redraw = true;
                return true;
            }
            OvKey::Pass => {}
        }
        let mut cx = self.cx();
        cx.alt = alt;
        let used = self.app.key(vk, mods, &mut cx);
        self.finish(cx, now);
        if used {
            self.redraw = true;
            return true;
        }
        let order: Vec<Id> = self.mo.hits.tab_order().collect();
        match vk {
            TAB if !order.is_empty() => {
                let at = self.mo.hits.focus.and_then(|f| order.iter().position(|&i| i == f));
                let n = order.len();
                let to = match (at, mods.shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                };
                self.mo.hits.focus = Some(order[to]);
                self.mo.hits.focus_visible = true;
            }
            ENTER | SPACE if self.mo.hits.focus.is_some() => {
                let id = self.mo.hits.focus.unwrap_or_default();
                self.mo.hits.focus_visible = true;
                if let Some(r) = self.mo.hits.get(id).map(|h| h.r) {
                    let mut cx = self.cx();
                    self.app.click(id, Button::Left, r.cx(), r.cy(), &mut cx);
                    self.finish(cx, now);
                }
            }
            ESC if self.mo.hits.focus.is_some() => self.mo.hits.focus = None,
            _ => return false,
        }
        self.redraw = true;
        true
    }

    pub fn text(&mut self, s: &str, now: u64) {
        if self.ov.modal() {
            return;
        }
        let mut cx = self.cx();
        self.app.text(s, &mut cx);
        self.finish(cx, now);
        self.redraw = true;
    }

    pub fn message(&mut self, msg: u32, wp: usize, lp: isize, now: u64) {
        let mut cx = self.cx();
        self.app.message(msg, wp, lp, &mut cx);
        self.finish(cx, now);
    }

    pub fn wake(&mut self, now: u64) {
        let mut cx = self.cx();
        self.app.wake(&mut cx);
        self.finish(cx, now);
    }

    pub fn files(&mut self, paths: Vec<String>, x: f32, y: f32, now: u64) {
        let mut cx = self.cx();
        App::drop(self.app.as_mut(), paths, x, y, &mut cx);
        self.finish(cx, now);
        self.redraw = true;
    }

    /// Files are dragged over the window at `(x, y)` DIPs (the first time or again): whether the app takes them.
    pub fn drag_over(&mut self, paths: &[String], x: f32, y: f32) -> bool {
        let ok = self.app.drag_over(paths, x, y);
        self.redraw = true;
        ok
    }

    /// What was dragged over the window left it.
    pub fn drag_leave(&mut self, now: u64) {
        let mut cx = self.cx();
        self.app.drag_leave(&mut cx);
        self.finish(cx, now);
        self.redraw = true;
    }

    /// The window was asked to close: whether the app agrees.
    pub fn may_close(&mut self, now: u64) -> bool {
        let mut cx = self.cx();
        let ok = self.app.close(&mut cx);
        self.finish(cx, now);
        ok
    }

    /// Does what the app asked for in `cx`.
    pub fn finish(&mut self, cx: Cx, now: u64) {
        for r in cx.reqs {
            match r {
                Req::Invalidate => self.redraw = true,
                Req::Menu { tag, items, at } => {
                    self.ov.open_menu(tag, items, at, now);
                    self.redraw = true;
                }
                Req::Toast { text, undo, bottom } => {
                    self.ov.show_toast(text, undo, bottom, now);
                    self.redraw = true;
                }
                Req::Dialog { tag, dialog } => {
                    self.ov.open_dialog(tag, dialog, now);
                    self.redraw = true;
                }
                Req::Focus(id) => {
                    self.mo.hits.focus = Some(id);
                    self.mo.hits.focus_visible = true;
                    self.redraw = true;
                }
                Req::Cursor(c) => self.cursor = Some(c),
                Req::Title(t) => self.host.push(HostReq::Title(t)),
                Req::Clipboard(t) => self.host.push(HostReq::Clipboard(t)),
                Req::Wake(ms) => self.host.push(HostReq::Wake(ms)),
                Req::Close => self.host.push(HostReq::Close),
            }
        }
        if cx.close {
            self.host.push(HostReq::Close);
        }
        if let Some(f) = cx.then {
            f();
        }
    }

    /// The cursor over the pointer now.
    #[must_use]
    pub fn cursor(&self) -> Cursor {
        self.cursor.or_else(|| self.pointer.and_then(|(x, y)| self.mo.hits.at(x, y)).map(|h| h.cursor)).unwrap_or_default()
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Windows' questions

    /// What `WM_NCHITTEST` answers for a point of the client area (DIPs). `edges` makes the resize strip run along all four
    /// sides; the window keeps Windows' own invisible borders on three of them, so only the top needs one of ours.
    #[must_use]
    pub fn nc_test(&self, x: f32, y: f32, edges: bool) -> Nc {
        let (w, h) = self.size;
        if !self.maximized {
            let (l, r, t, b) = (x < EDGE && edges, x >= w - EDGE && edges, y < EDGE, y >= h - EDGE && edges);
            match (l, r, t, b) {
                (true, _, true, _) => return Nc::TopLeft,
                (_, true, true, _) => return Nc::TopRight,
                (true, _, _, true) => return Nc::BottomLeft,
                (_, true, _, true) => return Nc::BottomRight,
                (true, ..) => return Nc::Left,
                (_, true, ..) => return Nc::Right,
                (_, _, true, _) => {
                    return if x < EDGE {
                        Nc::TopLeft
                    } else if x >= w - EDGE {
                        Nc::TopRight
                    } else {
                        Nc::Top
                    };
                }
                (.., true) => return Nc::Bottom,
                _ => {}
            }
        }
        match self.mo.hits.at(x, y) {
            Some(h) if h.id == WIN_MAX => Nc::MaxButton,
            Some(_) => Nc::Client,
            None if y < self.app.caption_height() => Nc::Caption,
            None => Nc::Client,
        }
    }
}

/// Where a window of this app remembers its place: `%LOCALAPPDATA%\WinRust\<name>.place` (the app picks the name).
#[must_use]
pub fn place_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("WinRust").join(format!("{name}.place")))
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
