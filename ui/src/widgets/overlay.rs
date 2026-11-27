//! What opens over the content of an application window, inside it: a menu (with sub-menus), the hint over an item, a toast and a
//! dialog. They are drawn after the app's own frame and named in the same registry, so the window hit-tests them like anything
//! else; a menu that would stand outside the window is moved in, as the mockup's `overflow: hidden` does. The app asks for them
//! through `Cx::menu`, `Cx::toast` and `Cx::dialog`, and hears of a choice as `App::command(tag, choice)`.

use std::borrow::Cow;

use super::look::{self, BODY};
use crate::anim::{self, EASE, SPRING};
use crate::frame::{Frame, Hot, Id, OVERLAY};
use crate::gfx::{Align, Canvas, Font, Rect, Rgba};
use winrust_common::style;

/// One row of a menu. `value` is what `App::command` gets as the choice; a row without one is numbered in drawing order, leaves
/// only (separators and rows that open a sub-menu do not count).
#[derive(Clone, Debug, Default)]
pub struct MenuItem {
    pub text: String,
    pub icon: Option<&'static str>,
    pub kbd: String,
    pub disabled: bool,
    /// A radio row: `Some(true)` is the selected one, drawn with a tick in the icon's place.
    pub radio: Option<bool>,
    pub danger: bool,
    pub sep: bool,
    pub sub: Vec<MenuItem>,
    pub value: Option<usize>,
}

impl MenuItem {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Self::default() }
    }

    #[must_use]
    pub fn sep() -> Self {
        Self { sep: true, ..Self::default() }
    }

    #[must_use]
    pub fn icon(mut self, d: &'static str) -> Self {
        self.icon = Some(d);
        self
    }

    #[must_use]
    pub fn kbd(mut self, k: impl Into<String>) -> Self {
        self.kbd = k.into();
        self
    }

    #[must_use]
    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }

    #[must_use]
    pub fn radio(mut self, on: bool) -> Self {
        self.radio = Some(on);
        self
    }

    #[must_use]
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    #[must_use]
    pub fn submenu(mut self, items: Vec<MenuItem>) -> Self {
        self.sub = items;
        self
    }

    #[must_use]
    pub fn value(mut self, v: usize) -> Self {
        self.value = Some(v);
        self
    }

    fn live(&self) -> bool {
        !self.sep && !self.disabled
    }
}

/// A dialog: an icon, a title, a text and buttons, over a scrim. The last button is the primary one (Enter), `cancel` the one Esc
/// presses.
#[derive(Clone, Debug)]
pub struct Dialog {
    pub title: String,
    pub text: String,
    pub icon: Option<&'static str>,
    /// The primary button is red (an action that cannot be undone) instead of the accent.
    pub danger: bool,
    pub buttons: Vec<String>,
    pub cancel: usize,
    pub width: f32,
}

impl Dialog {
    #[must_use]
    pub fn new(title: impl Into<String>, text: impl Into<String>, buttons: &[&str]) -> Self {
        Self { title: title.into(), text: text.into(), icon: None, danger: false, buttons: buttons.iter().map(|b| (*b).to_string()).collect(), cancel: 0, width: 400.0 }
    }

    #[must_use]
    pub fn icon(mut self, d: &'static str, danger: bool) -> Self {
        self.icon = Some(d);
        self.danger = danger;
        self
    }
}

/// What a key meant to the overlays.
#[derive(Debug, PartialEq, Eq)]
pub enum OvKey {
    /// No overlay wants it.
    Pass,
    /// Taken (moved a highlight, closed a menu).
    Taken,
    /// A choice: `App::command(tag, choice)`.
    Command(u32, usize),
}

// ids of the overlays' items; all below `OVERLAY`
const MENU_BACK: Id = OVERLAY - 1;
const DLG_BACK: Id = OVERLAY - 2;
/// The toast's Undo button.
pub const TOAST_UNDO: Id = OVERLAY - 3;
const DLG_BTN: Id = OVERLAY - 10;
const MENU_ITEM: Id = OVERLAY - 100;

pub fn menu_row_id(level: usize, i: usize) -> Id {
    MENU_ITEM - (level * 256 + i) as Id
}

fn menu_slot(id: Id) -> Option<(usize, usize)> {
    let n = MENU_ITEM - id;
    (n >= 0).then_some((n as usize / 256, n as usize % 256))
}

const MENU_MS: u32 = 160;
const DIALOG_MS: u32 = 220;
const TOAST_MS: u64 = 4200;
const TIP_DELAY: u64 = 500;
const MARGIN: f32 = 8.0;
const ROW: f32 = 32.0;
const SEP: f32 = 11.0;
const PAD: f32 = 6.0;

struct Level {
    items: Vec<MenuItem>,
    /// What the menu hangs from; the rows of the parent level for a sub-menu.
    anchor: Rect,
    side: bool,
    /// The row of the level below that opened it.
    from: Option<usize>,
    /// Where it stands, known after its first frame.
    r: Rect,
    rows: Vec<Rect>,
    hover: Option<usize>,
    origin: (f32, f32),
    born: u64,
}

struct MenuState {
    tag: u32,
    levels: Vec<Level>,
}

struct DialogState {
    tag: u32,
    d: Dialog,
    born: u64,
    focus: usize,
}

struct ToastState {
    text: String,
    undo: Option<u32>,
    bottom: f32,
    born: u64,
}

struct TipState {
    id: Id,
    text: Cow<'static, str>,
    anchor: Rect,
    since: u64,
}

#[derive(Default)]
pub struct Overlays {
    menu: Option<MenuState>,
    dialog: Option<DialogState>,
    toast: Option<ToastState>,
    tip: Option<TipState>,
}

/// Numbers the rows that have no value yet (leaves, depth first).
fn number(items: &mut [MenuItem], n: &mut usize) {
    for it in items {
        if it.sep {
            continue;
        }
        if it.sub.is_empty() {
            if it.value.is_none() {
                it.value = Some(*n);
            }
            *n += 1;
        } else {
            number(&mut it.sub, n);
        }
    }
}

/// Where a box of `size` stands for an `anchor`, inside `win`: under the anchor and flush with its left edge; flush with its
/// right edge when that would run out; above it when there is no room below. Also the point it grows from.
#[must_use]
pub fn place(anchor: Rect, size: (f32, f32), win: Rect) -> (Rect, (f32, f32)) {
    let (w, h) = size;
    let mut x = anchor.x;
    let mut right = false;
    if x + w > win.x + win.w - MARGIN {
        x = anchor.x + anchor.w - w;
        right = true;
    }
    x = x.clamp(win.x + MARGIN, (win.x + win.w - w - MARGIN).max(win.x + MARGIN));
    let gap = if anchor.h > 0.0 { 4.0 } else { 0.0 };
    let mut y = anchor.y + anchor.h + gap;
    let mut up = false;
    if y + h > win.y + win.h - MARGIN {
        y = anchor.y - h - gap;
        up = true;
    }
    y = y.clamp(win.y + MARGIN, (win.y + win.h - h - MARGIN).max(win.y + MARGIN));
    let r = Rect::new(x, y, w, h);
    (r, (x + if right { w } else { 0.0 }, y + if up { h } else { 0.0 }))
}

/// A sub-menu stands beside its parent, its first row level with the row that opened it.
fn place_side(parent: Rect, row: Rect, size: (f32, f32), win: Rect) -> (Rect, (f32, f32)) {
    let (w, h) = size;
    let mut x = parent.x + parent.w - 4.0;
    let mut left = false;
    if x + w > win.x + win.w - MARGIN {
        x = parent.x - w + 4.0;
        left = true;
    }
    x = x.max(win.x + MARGIN);
    let y = (row.y - PAD).clamp(win.y + MARGIN, (win.y + win.h - h - MARGIN).max(win.y + MARGIN));
    (Rect::new(x, y, w, h), (x + if left { w } else { 0.0 }, row.y.max(y)))
}

fn level_height(items: &[MenuItem]) -> f32 {
    2.0 * PAD + items.iter().map(|i| if i.sep { SEP } else { ROW }).sum::<f32>()
}

impl Overlays {
    /// Something is open that takes the whole window's pointer and keys (a menu, a dialog).
    #[must_use]
    pub fn modal(&self) -> bool {
        self.menu.is_some() || self.dialog.is_some()
    }

    #[must_use]
    pub fn owns(id: Id) -> bool {
        id <= OVERLAY
    }

    pub fn open_menu(&mut self, tag: u32, mut items: Vec<MenuItem>, at: Rect, now: u64) {
        number(&mut items, &mut 0);
        self.tip = None;
        let level = Level { items, anchor: at, side: false, from: None, r: Rect::default(), rows: Vec::new(), hover: None, origin: (at.x, at.y), born: now };
        self.menu = Some(MenuState { tag, levels: vec![level] });
    }

    pub fn open_dialog(&mut self, tag: u32, d: Dialog, now: u64) {
        self.menu = None;
        self.tip = None;
        let focus = d.buttons.len().saturating_sub(1);
        self.dialog = Some(DialogState { tag, d, born: now, focus });
    }

    pub fn show_toast(&mut self, text: String, undo: Option<u32>, bottom: f32, now: u64) {
        self.toast = Some(ToastState { text, undo, bottom, born: now });
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    /// The hint for the item under the pointer (or none): it shows once the pointer has rested on it for half a second.
    pub fn set_tip(&mut self, tip: Option<(Id, Cow<'static, str>, Rect)>, now: u64) {
        match tip {
            Some((id, text, anchor)) if !self.modal() => {
                if self.tip.as_ref().is_none_or(|t| t.id != id) {
                    self.tip = Some(TipState { id, text, anchor, since: now });
                }
            }
            _ => self.tip = None,
        }
    }

    /// The pointer is over `id` (an item of the window or of an overlay).
    pub fn hover(&mut self, id: Option<Id>, now: u64) {
        let Some(m) = self.menu.as_mut() else { return };
        let Some((lv, i)) = id.and_then(menu_slot) else { return };
        if lv >= m.levels.len() || i >= m.levels[lv].items.len() {
            return;
        }
        m.levels[lv].hover = Some(i);
        Self::open_sub(m, lv, i, now);
    }

    /// Opens the sub-menu of row `i` of level `lv` (and closes what stood deeper) if the row has one.
    fn open_sub(m: &mut MenuState, lv: usize, i: usize, now: u64) {
        let it = m.levels[lv].items[i].clone();
        let want = (it.live() && !it.sub.is_empty()).then_some(i);
        if m.levels.len() > lv + 1 && m.levels[lv + 1].from == want {
            return; // already open
        }
        m.levels.truncate(lv + 1);
        if want.is_some() {
            let row = m.levels[lv].rows.get(i).copied().unwrap_or(m.levels[lv].anchor);
            let items = it.sub.clone();
            m.levels.push(Level { items, anchor: row, side: true, from: want, r: Rect::default(), rows: Vec::new(), hover: None, origin: (row.x, row.y), born: now });
        }
    }

    /// A click (the button went up over `id`): the command it makes, if any.
    pub fn click(&mut self, id: Id, now: u64) -> Option<(u32, usize)> {
        if id == MENU_BACK {
            self.menu = None;
            return None;
        }
        if let Some((lv, i)) = menu_slot(id).filter(|_| self.menu.is_some()) {
            let m = self.menu.as_mut()?;
            let it = m.levels.get(lv)?.items.get(i)?.clone();
            if !it.live() {
                return None;
            }
            if !it.sub.is_empty() {
                m.levels[lv].hover = Some(i);
                Self::open_sub(m, lv, i, now);
                return None;
            }
            let tag = m.tag;
            self.menu = None;
            return it.value.map(|v| (tag, v));
        }
        if id <= DLG_BTN && id > DLG_BTN - 16 {
            let d = self.dialog.take()?;
            return Some((d.tag, (DLG_BTN - id) as usize));
        }
        if id == TOAST_UNDO {
            let t = self.toast.take()?;
            return t.undo.map(|u| (u, 0));
        }
        None
    }

    pub fn key(&mut self, vk: u32, now: u64) -> OvKey {
        const ESC: u32 = 0x1B;
        const ENTER: u32 = 0x0D;
        const SPACE: u32 = 0x20;
        const LEFT: u32 = 0x25;
        const UP: u32 = 0x26;
        const RIGHT: u32 = 0x27;
        const DOWN: u32 = 0x28;
        const TAB: u32 = 0x09;
        if let Some(d) = self.dialog.as_mut() {
            let n = d.d.buttons.len();
            match vk {
                ESC => {
                    let (tag, c) = (d.tag, d.d.cancel);
                    self.dialog = None;
                    return OvKey::Command(tag, c);
                }
                ENTER | SPACE => {
                    let (tag, c) = (d.tag, d.focus);
                    self.dialog = None;
                    return OvKey::Command(tag, c);
                }
                LEFT | UP => d.focus = (d.focus + n - 1) % n.max(1),
                RIGHT | DOWN | TAB => d.focus = (d.focus + 1) % n.max(1),
                _ => {}
            }
            return OvKey::Taken;
        }
        if self.menu.is_some() && vk == ESC {
            self.menu = None;
            return OvKey::Taken;
        }
        let Some(m) = self.menu.as_mut() else { return OvKey::Pass };
        let lv = m.levels.len() - 1;
        let n = m.levels[lv].items.len();
        let step = |from: Option<usize>, dir: isize, items: &[MenuItem]| -> Option<usize> {
            let mut i = from.map_or(if dir > 0 { -1 } else { n as isize }, |i| i as isize);
            for _ in 0..n {
                i = (i + dir).rem_euclid(n as isize);
                if items[i as usize].live() {
                    return Some(i as usize);
                }
            }
            None
        };
        match vk {
            LEFT if m.levels.len() > 1 => {
                m.levels.pop();
            }
            DOWN | UP => {
                let items = &m.levels[lv].items;
                let to = step(m.levels[lv].hover, if vk == DOWN { 1 } else { -1 }, items);
                m.levels[lv].hover = to;
                if let Some(i) = to {
                    Self::open_sub(m, lv, i, now);
                }
            }
            RIGHT | ENTER | SPACE => {
                let Some(i) = m.levels[lv].hover else { return OvKey::Taken };
                let it = m.levels[lv].items[i].clone();
                if !it.live() {
                    return OvKey::Taken;
                }
                if !it.sub.is_empty() {
                    Self::open_sub(m, lv, i, now);
                    return OvKey::Taken;
                }
                if vk == RIGHT {
                    return OvKey::Taken;
                }
                let tag = m.tag;
                self.menu = None;
                return it.value.map_or(OvKey::Taken, |v| OvKey::Command(tag, v));
            }
            _ => {}
        }
        OvKey::Taken
    }

    /// Frames are needed while something moves.
    #[must_use]
    pub fn animating(&self, now: u64, mode: style::Anim) -> bool {
        let run = |born: u64, ms: u32| now < born + u64::from(anim::timing(mode, ms, 0).0);
        self.menu.as_ref().is_some_and(|m| m.levels.iter().any(|l| run(l.born, MENU_MS) || l.r.w == 0.0))
            || self.dialog.as_ref().is_some_and(|d| run(d.born, DIALOG_MS))
            || self.toast.as_ref().is_some_and(|t| {
                let p = (now - t.born.min(now)) as f32 / TOAST_MS as f32;
                p < 0.08 || (0.9..1.0).contains(&p)
            })
            || self.tip.as_ref().is_some_and(|t| now >= t.since + TIP_DELAY && now < t.since + TIP_DELAY + 350)
    }

    /// When a frame is due though nothing moves: the hint's delay, the toast's leave and its end.
    #[must_use]
    pub fn deadline(&self, now: u64) -> Option<u64> {
        let mut d: Option<u64> = None;
        let mut at = |t: u64| {
            if t > now {
                d = Some(d.map_or(t, |x| x.min(t)));
            }
        };
        if let Some(t) = &self.tip {
            at(t.since + TIP_DELAY);
        }
        if let Some(t) = &self.toast {
            at(t.born + (TOAST_MS as f32 * 0.9) as u64);
            at(t.born + TOAST_MS);
        }
        d
    }

    /// Drops what has run its time (a toast after its 4.2 s).
    pub fn expire(&mut self, now: u64) {
        if self.toast.as_ref().is_some_and(|t| now >= t.born + TOAST_MS) {
            self.toast = None;
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // drawing

    pub fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, win: Rect, now: u64) {
        let mode = f.mo.mode.unwrap_or(style::Anim::Full);
        self.expire(now);
        self.draw_tip(c, win, now, mode);
        self.draw_toast(c, f, win, now);
        if self.menu.is_some() {
            self.draw_menu(c, f, win, now, mode);
        }
        if self.dialog.is_some() {
            self.draw_dialog(c, f, win, now, mode);
        }
    }

    fn draw_tip(&mut self, c: &mut Canvas<'_>, win: Rect, now: u64, mode: style::Anim) {
        let Some(t) = self.tip.as_ref().filter(|t| now >= t.since + TIP_DELAY) else { return };
        let font = Font::new(12.0, 500);
        let w = (c.measure(&t.text, font) + 20.0).ceil();
        let h = 25.0;
        let (r, _) = place(Rect::new(t.anchor.x + (t.anchor.w - w) / 2.0, t.anchor.y, w, t.anchor.h), (w, h), win);
        let (fade, rise) = (anim::timing(mode, 200, 0).0, anim::timing(mode, 350, 0).0);
        let age = (now - t.since - TIP_DELAY) as f32;
        let op = if fade == 0 { 1.0 } else { (age / fade as f32).clamp(0.0, 1.0) };
        let k = if rise == 0 { 1.0 } else { SPRING.ease((age / rise as f32).clamp(0.0, 1.0)) };
        let text = t.text.clone();
        c.faded(op, |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, 6.0 * (1.0 - k), |c| {
                c.shadow(r, 8.0, 8.0, 24.0, Rgba::css(0, 0, 0, 0.5));
                c.ring(r.inset(-1.0), 9.0, 1.0, Rgba::white(0.1));
                c.fill(r, 8.0, Rgba::css(27, 28, 34, 0.96));
                c.text(&text, r, font, Rgba::css(236, 238, 244, 1.0), Align::Center);
            });
        });
    }

    fn draw_toast(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, win: Rect, now: u64) {
        let Some(t) = self.toast.as_ref() else { return };
        // `pv_toast`: in over the first 8 %, out over the last 10 % of 4.2 s, each leg with `ease`
        let p = (now.saturating_sub(t.born)) as f32 / TOAST_MS as f32;
        let (op, dy) = if p < 0.08 {
            let k = EASE.ease(p / 0.08);
            (k, 10.0 * (1.0 - k))
        } else if p < 0.9 {
            (1.0, 0.0)
        } else {
            (1.0 - EASE.ease(((p - 0.9) / 0.1).min(1.0)), 0.0)
        };
        let label = Font::new(13.0, 600);
        let text_w = c.measure(&t.text, BODY).ceil();
        let undo_w = if t.undo.is_some() { 12.0 + c.measure("Undo", label).ceil() + 12.0 } else { 10.0 };
        let w = 16.0 + text_w + 12.0 + undo_w + 6.0;
        let r = Rect::new(((win.w - w) / 2.0).round(), win.h - t.bottom - 40.0, w, 40.0);
        let undo = t.undo.is_some();
        let text = t.text.clone();
        c.faded(op, |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, dy, |c| {
                c.outside(r, 20.0, |c| c.shadow(r, 20.0, 16.0, 40.0, Rgba::css(0, 0, 0, 0.35)));
                c.fill(r, 20.0, look::TOAST);
                c.ring(r.inset(-1.0), 21.0, 1.0, Rgba::css(0, 0, 0, 0.2));
                c.ring(r, 20.0, 1.0, Rgba::white(0.1));
                c.text(&text, Rect::new(r.x + 16.0, r.y, text_w + 2.0, r.h), BODY, look::T1, Align::Start);
                if undo {
                    let b = Rect::new(r.x + r.w - 6.0 - undo_w, r.y + 6.0, undo_w, 28.0);
                    let hot = f.item(TOAST_UNDO, b);
                    f.hand();
                    c.fill(b, 14.0, Rgba::white(0.1 + 0.06 * hot.hover));
                    c.text("Undo", b, label, look::accent(), Align::Center);
                }
            });
        });
    }

    fn draw_menu(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, win: Rect, now: u64, mode: style::Anim) {
        f.item(MENU_BACK, win); // a click outside closes it
        let Some(m) = self.menu.as_mut() else { return };
        let icons = m.levels.iter().any(|l| l.items.iter().any(|i| i.icon.is_some() || i.radio.is_some()));
        for lv in 0..m.levels.len() {
            let (done, rest) = m.levels.split_at_mut(lv);
            let l = &mut rest[0];
            let font = BODY;
            let kfont = Font::new(12.0, 400);
            let fit = l
                .items
                .iter()
                .filter(|i| !i.sep)
                .map(|i| {
                    let (t, k) = (c.measure(&i.text, font), if i.kbd.is_empty() { 0.0 } else { 24.0 + c.measure(&i.kbd, kfont) });
                    PAD + 10.0 + if icons { 25.0 } else { 0.0 } + t + k + if i.sub.is_empty() { 0.0 } else { 24.0 } + 10.0 + PAD
                })
                .fold(0.0, f32::max)
                .ceil();
            let size = (fit.max(232.0), level_height(&l.items));
            let (r, origin) = match done.last() {
                Some(p) if l.side => {
                    let row = l.from.and_then(|i| p.rows.get(i)).copied().unwrap_or(l.anchor);
                    place_side(p.r, row, size, win)
                }
                _ => place(l.anchor, size, win),
            };
            l.r = r;
            l.origin = origin;
            let dur = anim::timing(mode, MENU_MS, 0).0;
            let t = if dur == 0 { 1.0 } else { SPRING.ease(((now - l.born.min(now)) as f32 / dur as f32).clamp(0.0, 1.0)) };
            l.rows.clear();
            let mut y = r.y + PAD;
            for it in &l.items {
                let h = if it.sep { SEP } else { ROW };
                l.rows.push(Rect::new(r.x + PAD, y, r.w - 2.0 * PAD, h));
                y += h;
            }
            let (rows, items, hover) = (l.rows.clone(), l.items.clone(), l.hover);
            c.faded(t, |c| {
                c.transformed(origin.0, origin.1, 0.97 + 0.03 * t, 0.0, -4.0 * (1.0 - t), |c| {
                    c.outside(r, 12.0, |c| c.shadow(r, 12.0, 18.0, 48.0, Rgba::css(0, 0, 0, 0.55)));
                    c.fill(r, 12.0, look::MENU);
                    c.ring(r.inset(-1.0), 13.0, 1.0, Rgba::css(0, 0, 0, 0.55));
                    c.ring(r, 12.0, 1.0, Rgba::white(0.09));
                    for (i, it) in items.iter().enumerate() {
                        let row = rows[i];
                        if it.sep {
                            c.fill(Rect::new(row.x + 2.0, row.y + 5.0, row.w - 4.0, 1.0), 0.0, Rgba::white(0.07));
                            continue;
                        }
                        let hot: Hot = f.item(menu_row_id(lv, i), row);
                        if it.live() {
                            f.hand();
                        }
                        let lit = hot.hover.max(f32::from(hover == Some(i) && it.live()));
                        c.fill(row, 8.0, Rgba::white(0.08 * lit));
                        let ink = if it.danger { look::RED } else { look::T1 };
                        let dim = if it.disabled { 0.4 } else { 1.0 };
                        let mut x = row.x + 10.0;
                        if icons {
                            let mid = (x + 7.5, row.cy());
                            if it.radio == Some(true) {
                                c.icon("M5 12.5l4.5 4.5L19 7", mid, 15.0, 2.2, look::T1.fade(dim), 0.0);
                            } else if let Some(d) = it.icon {
                                c.icon(d, mid, 15.0, 1.9, (if it.danger { look::RED } else { look::T2 }).fade(dim), 0.0);
                            }
                            x += 25.0;
                        }
                        let end = row.x + row.w - 10.0 - if it.sub.is_empty() { 0.0 } else { 24.0 };
                        c.text(&it.text, Rect::new(x, row.y, end - x, row.h), font, ink.fade(dim), Align::Start);
                        if !it.kbd.is_empty() {
                            c.text(&it.kbd, Rect::new(row.x, row.y, row.w - 10.0, row.h), kfont, look::T3.fade(dim), Align::End);
                        }
                        if !it.sub.is_empty() {
                            c.icon("M9 6l6 6-6 6", (row.x + row.w - 17.0, row.cy()), 14.0, 2.0, look::T3.fade(dim), 0.0);
                        }
                    }
                });
            });
        }
    }

    fn draw_dialog(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, win: Rect, now: u64, mode: style::Anim) {
        let Some(s) = self.dialog.as_ref() else { return };
        let d = &s.d;
        let dur = anim::timing(mode, DIALOG_MS, 0).0;
        let t = if dur == 0 { 1.0 } else { ((now - s.born.min(now)) as f32 / dur as f32).clamp(0.0, 1.0) };
        let k = SPRING.ease(t);
        f.item(DLG_BACK, win); // takes every click behind the dialog
        c.fill(win, 0.0, Rgba::css(0, 0, 0, 0.45 * EASE.ease(t)));
        let (title_f, label_f) = (Font::new(16.0, 600).tracking(-0.01), Font::new(13.0, 500));
        let inner = d.width - 44.0;
        let lines = c.lines(&d.text, BODY, inner) as f32;
        let mut h = 22.0 + 20.0 + 10.0 + lines * 19.5 + 10.0 + 10.0 + 34.0 + 22.0;
        if d.icon.is_some() {
            h += 40.0 + 4.0 + 10.0;
        }
        let r = Rect::new(((win.w - d.width) / 2.0).round(), ((win.h - h) / 2.0).round(), d.width, h);
        let widths: Vec<f32> = d.buttons.iter().map(|b| 28.0 + c.measure(b, label_f).ceil()).collect();
        let (title, text, icon, danger, buttons, focus) = (d.title.clone(), d.text.clone(), d.icon, d.danger, d.buttons.clone(), s.focus);
        c.faded(k, |c| {
            c.transformed(r.cx(), r.cy(), 0.95 + 0.05 * k, 0.0, 0.0, |c| {
                c.outside(r, 16.0, |c| c.shadow(r, 16.0, 30.0, 80.0, Rgba::css(0, 0, 0, 0.5)));
                c.fill(r, 16.0, look::MENU);
                c.ring(r.inset(-1.0), 17.0, 1.0, Rgba::css(0, 0, 0, 0.6));
                c.ring(r, 16.0, 1.0, Rgba::white(0.09));
                let mut y = r.y + 22.0;
                if let Some(ic) = icon {
                    let tint = if danger { look::RED } else { look::accent() };
                    let disc = Rect::new(r.x + 22.0, y, 40.0, 40.0);
                    c.fill(disc, 20.0, (if danger { Rgba::new(229.0 / 255.0, 72.0 / 255.0, 77.0 / 255.0, 0.14) } else { look::accent().alpha(0.16) }).fade(1.0));
                    c.icon(ic, (disc.cx(), disc.cy()), 20.0, 2.0, tint, 0.0);
                    y += 40.0 + 4.0 + 10.0;
                }
                c.text(&title, Rect::new(r.x + 22.0, y, inner, 20.0), title_f, look::T1, Align::Start);
                y += 20.0 + 10.0;
                c.text_wrapped(&text, Rect::new(r.x + 22.0, y, inner, lines * 19.5), BODY, look::T2, 19.5, 0);
                y += lines * 19.5 + 10.0 + 10.0;
                let mut x = r.x + r.w - 22.0;
                for i in (0..buttons.len()).rev() {
                    let b = Rect::new(x - widths[i], y, widths[i], 34.0);
                    x -= widths[i] + 8.0;
                    let hot = f.item(DLG_BTN - i as Id, b);
                    f.hand();
                    let primary = i + 1 == buttons.len();
                    let bg = if primary { if danger { look::DANGER } else { look::accent() } } else { Rgba::white(0.08) };
                    let sc = 1.0 - 0.03 * hot.press;
                    c.transformed(b.cx(), b.cy(), sc, 0.0, 0.0, |c| {
                        c.fill(b, 10.0, bg);
                        if hot.hover > 0.0 {
                            c.fill(b, 10.0, Rgba::pure_white(0.12 * hot.hover));
                        }
                        let ink = if primary { if danger { Rgba::pure_white(1.0) } else { look::ON_ACCENT } } else { look::T1 };
                        c.text(&buttons[i], b, label_f, ink, Align::Center);
                    });
                    if i == focus {
                        c.ring(b.inset(-3.0), 13.0, 2.0, look::focus_ring().fade(0.9));
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_stands_under_its_anchor_and_is_moved_inside_the_window() {
        let win = Rect::new(0.0, 0.0, 1000.0, 700.0);
        let (r, o) = place(Rect::new(100.0, 40.0, 32.0, 32.0), (232.0, 200.0), win);
        assert_eq!((r.x, r.y), (100.0, 76.0));
        assert_eq!(o, (100.0, 76.0), "it grows from its top left corner");
        // no room on the right: flush with the anchor's right edge, growing from the top right
        let (r, o) = place(Rect::new(900.0, 40.0, 32.0, 32.0), (232.0, 200.0), win);
        assert_eq!(r.x + r.w, 932.0);
        assert_eq!(o.0, 932.0);
        // no room below: above the anchor
        let (r, _) = place(Rect::new(100.0, 600.0, 32.0, 32.0), (232.0, 200.0), win);
        assert_eq!(r.y + r.h, 596.0);
        // a point (context menu) at the edge: pulled in by the margin
        let (r, _) = place(Rect::new(995.0, 695.0, 0.0, 0.0), (232.0, 200.0), win);
        assert!(r.x + r.w <= 1000.0 - MARGIN && r.y + r.h <= 700.0 - MARGIN, "{r:?}");
    }

    #[test]
    fn hovering_a_row_with_a_submenu_opens_it_beside_and_another_row_closes_it_again() {
        let mut o = Overlays::default();
        let items = vec![MenuItem::new("a").submenu(vec![MenuItem::new("a1"), MenuItem::new("a2")]), MenuItem::new("b")];
        o.open_menu(1, items, Rect::new(10.0, 10.0, 0.0, 0.0), 0);
        o.menu.as_mut().unwrap().levels[0].rows = vec![Rect::new(16.0, 16.0, 220.0, 32.0), Rect::new(16.0, 48.0, 220.0, 32.0)];
        o.hover(Some(menu_row_id(0, 0)), 5);
        assert_eq!(o.menu.as_ref().unwrap().levels.len(), 2, "the sub-menu opened");
        o.hover(Some(menu_row_id(1, 1)), 6);
        assert_eq!(o.menu.as_ref().unwrap().levels.len(), 2, "moving inside it keeps it");
        o.hover(Some(menu_row_id(0, 1)), 7);
        assert_eq!(o.menu.as_ref().unwrap().levels.len(), 1, "a row without one closes it");
        assert_eq!(o.click(menu_row_id(0, 0), 8), None, "a click on the parent only opens it");
        assert_eq!(o.menu.as_ref().unwrap().levels.len(), 2);
        assert_eq!(o.click(menu_row_id(1, 1), 9), Some((1, 1)), "a leaf of the sub-menu: the second leaf");
    }

    #[test]
    fn rows_are_numbered_in_drawing_order_leaves_only() {
        let mut items = vec![
            MenuItem::new("a"),
            MenuItem::sep(),
            MenuItem::new("b").submenu(vec![MenuItem::new("b1"), MenuItem::new("b2").value(9)]),
            MenuItem::new("c"),
        ];
        number(&mut items, &mut 0);
        assert_eq!(items[0].value, Some(0));
        assert_eq!(items[2].sub[0].value, Some(1));
        assert_eq!(items[2].sub[1].value, Some(9), "an explicit value stays");
        assert_eq!(items[3].value, Some(3));
    }

    #[test]
    fn keys_walk_a_menu_skipping_separators_and_disabled_rows_and_choose() {
        let mut o = Overlays::default();
        let items = vec![MenuItem::new("a"), MenuItem::sep(), MenuItem::new("b").disabled(true), MenuItem::new("c")];
        o.open_menu(7, items, Rect::new(10.0, 10.0, 0.0, 0.0), 0);
        assert_eq!(o.key(0x28, 0), OvKey::Taken); // down: a
        assert_eq!(o.key(0x28, 0), OvKey::Taken); // down: c (b and the separator are skipped)
        assert_eq!(o.menu.as_ref().unwrap().levels[0].hover, Some(3));
        assert_eq!(o.key(0x0D, 0), OvKey::Command(7, 2), "c is the third leaf");
        assert!(!o.modal());
        o.open_menu(7, vec![MenuItem::new("a")], Rect::default(), 0);
        assert_eq!(o.key(0x1B, 0), OvKey::Taken);
        assert!(!o.modal(), "Esc closes it");
        assert_eq!(o.key(0x1B, 0), OvKey::Pass);
    }

    #[test]
    fn a_dialog_answers_enter_with_its_primary_button_and_esc_with_cancel() {
        let mut o = Overlays::default();
        o.open_dialog(3, Dialog::new("Delete?", "Gone for good.", &["Cancel", "Delete"]), 0);
        assert!(o.modal());
        assert_eq!(o.key(0x1B, 0), OvKey::Command(3, 0));
        o.open_dialog(3, Dialog::new("Delete?", "Gone for good.", &["Cancel", "Delete"]), 0);
        assert_eq!(o.key(0x0D, 0), OvKey::Command(3, 1));
        o.open_dialog(3, Dialog::new("Delete?", "x", &["Cancel", "Delete"]), 0);
        assert_eq!(o.click(DLG_BTN, 0), Some((3, 0)), "a click on the first button");
    }

    #[test]
    fn a_toast_lives_4_2_seconds_and_wakes_the_window_only_at_the_changes() {
        let mut o = Overlays::default();
        o.show_toast("Moved".into(), Some(5), 18.0, 1000);
        assert!(o.animating(1100, style::Anim::Full), "coming in");
        assert!(!o.animating(3000, style::Anim::Full), "standing still: no frames");
        assert_eq!(o.deadline(3000), Some(1000 + 3780), "the next frame is due when it starts to leave");
        assert!(o.animating(1000 + 4000, style::Anim::Full), "leaving");
        o.expire(1000 + 4200);
        assert!(o.toast.is_none());
        o.show_toast("x".into(), Some(5), 18.0, 0);
        assert_eq!(o.click(TOAST_UNDO, 10), Some((5, 0)), "Undo comes back as a command");
    }
}
