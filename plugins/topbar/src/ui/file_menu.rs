//! The menu for files, apps, folders and the Recycle Bin (the mockup's `FileMenu` board, `mn` and `sm`): a header with the target's
//! icon, name and a second line; a row of tiles (Cut, Copy, Rename, Share, Delete) whose hint shows in the header; the design's
//! rows for the kind of target; and "Show more options", which swaps the rows for the whole list Windows gave, in the same style.
//! A row with an arrow opens a submenu that stands beside the menu on a glass of its own (Open with, Send to, the submenus of
//! other programs); Windows fills those when they are opened, so the row shows a spinner until the items come.
//!
//! The box is the bounding box of the menu as it is and as it is with the whole list (the first is where the pointer put it, the
//! second flips and grows by the same rules), and the glass, the rings and the shadow follow the card of the one that is shown
//! ([`Content::cards`]): "Show more options" is a change of card, not of window. A click beside the cards but inside the box
//! closes the menu, as a click beside a menu does.
//!
//! The geometry is the mockup's, row by row (measured in its DOM): padding 6, header 48, tiles 40 high, rows 36 a pixel apart, a
//! rule 9, a caption 24; the width grows with the longest label from 248 to 320 (submenus 220 to 300); the list scrolls above 80%
//! of the monitor's height. The keys are the mockup's: arrows, Enter, Esc, a letter jumps to the next label that starts with it,
//! Right opens a submenu and Left closes it, F2 renames and Delete deletes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use winrust_common::filemenu::Icon;
use winrust_common::style;

use super::anim::{EASE, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Picture, Rect, Rgba, Sample};
use super::menu::{DANGER, DIM, INK, MUTED};
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Look, Mods, Origin, Place, Pose, Shadow};
use crate::filemenu::{CHECK, Do, Head, HeadIcon, Kind, Menu, Pic, Quick, Row, Tint, Under};

// ---------------------------------------------------------------------------------------------------------------------
// the mockup's numbers

const PAD: f32 = 6.0;
const HEAD_H: f32 = 48.0;
/// The tiles: 40 high, 2 below the header and 6 above the list.
const QUICK_H: f32 = 48.0;
const ROW_H: f32 = 36.0;
const GAP: f32 = 1.0;
const SEP_H: f32 = 9.0;
const CAP_H: f32 = 24.0;
const MENU_W: (f32, f32) = (248.0, 320.0);
const SUB_W: (f32, f32) = (220.0, 300.0);
/// Where a menu keeps off the edge of the room it has (`EDGE`).
const EDGE: f32 = 8.0;
/// How far a submenu overlaps its menu (`sxp = x + W - 4`), and how high it stands over its row (`- 6`).
const SUB_LAP: f32 = 4.0;
const SUB_UP: f32 = 6.0;
const SUB_PAD: f32 = 6.0;
/// `maxList = 900 * .8`: a list scrolls above this share of the monitor.
const MAX_SHARE: f32 = 0.8;
/// What the window keeps around the menu for a submenu and its shadow, DIPs.
const REACH: f32 = SUB_W.1 + 6.0 + 40.0;
/// Hover before a row opens its submenu, and before another row closes it, ms (`_ht`).
const OPEN_AFTER: u64 = 170;
const CLOSE_AFTER: u64 = 220;
const CLOSE_TILE_AFTER: u64 = 200;
/// A long label is tipped after the pointer rests on it, ms.
const TIP_AFTER: u64 = 550;
/// A label longer than this is tipped (`r.label.length > 34`).
const TIP_LEN: usize = 34;

// the ids of what can be hit
const QUICK: i32 = 0;
const ROW: i32 = 100;
const SUB: i32 = 600;
const BLANK: i32 = 1000;
/// The submenu's own tween.
const T_SUB: i32 = 2001;

// the colour tokens of the mockup (dark / light)
fn light() -> bool {
    style::light()
}
fn pick(dark: f32, light_v: f32) -> f32 {
    if light() { light_v } else { dark }
}
/// `--menu`.
fn glass() -> Rgba {
    if light() { Rgba::new(250.0 / 255.0, 250.0 / 255.0, 252.0 / 255.0, 0.92) } else { Rgba::new(26.0 / 255.0, 27.0 / 255.0, 33.0 / 255.0, 0.9) }
}
/// `--tile`, `--hov`, `--foc`, `--redBg`.
fn tile_bg() -> Rgba {
    Rgba::white(pick(0.055, 0.045))
}
fn hov() -> Rgba {
    Rgba::white(pick(0.075, 0.055))
}
fn foc() -> Rgba {
    Rgba::white(pick(0.1, 0.08))
}
fn red_bg() -> Rgba {
    Rgba::css(229, 72, 77, pick(0.16, 0.12))
}
const BLUE: Rgba = Rgba::pair(0x4f8ff0, 0x2f6fd0);

// ---------------------------------------------------------------------------------------------------------------------
// layout

/// Where each row of a list stands (y from the list's top, height) and how tall the list is: rows 36, a rule 9, a caption 24,
/// each a pixel below the one before.
#[must_use]
pub fn stack(rows: &[Row]) -> (Vec<(f32, f32)>, f32) {
    let mut y = 0.0;
    let mut out = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            y += GAP;
        }
        let h = match r.kind {
            Kind::Item => ROW_H,
            Kind::Sep => SEP_H,
            Kind::Caption => CAP_H,
        };
        out.push((y, h));
        y += h;
    }
    (out, y)
}

/// The width the mockup gives a menu for its longest row: `round(longest * 6.9 + 82)` within 248..320, where a row counts its
/// characters, its keys (9/10 of a character each, and 2) and its badge (and 1).
#[must_use]
pub fn menu_width(rows: &[Row]) -> f32 {
    let longest = rows
        .iter()
        .filter(|r| r.is_item())
        .map(|r| {
            let keys = if r.keys.is_empty() { 0.0 } else { r.keys.iter().map(|k| k.chars().count()).sum::<usize>() as f32 * 0.9 + 2.0 };
            let badge = if r.badge.is_empty() { 0.0 } else { r.badge.chars().count() as f32 + 1.0 };
            r.label.chars().count() as f32 + keys + badge
        })
        .fold(0.0, f32::max);
    (longest * 6.9 + 82.0).round().clamp(MENU_W.0, MENU_W.1)
}

/// A submenu's width: `round(longest * 6.9 + 70)` within 220..300 (a badge counts 8).
#[must_use]
pub fn sub_width(rows: &[Row]) -> f32 {
    let longest = rows.iter().filter(|r| r.is_item()).map(|r| r.label.chars().count() as f32 + if r.badge.is_empty() { 0.0 } else { 8.0 }).fold(0.0, f32::max);
    (longest * 6.9 + 70.0).round().clamp(SUB_W.0, SUB_W.1)
}

/// What one card of the menu is: its size, where its rows stand, and how much of the list shows.
#[derive(Clone, Debug)]
pub struct Geo {
    pub w: f32,
    pub h: f32,
    /// The list's top in the card, and how tall it shows (`maxList` at most).
    pub list_y: f32,
    pub view_h: f32,
    pub content_h: f32,
    pub rows: Vec<(f32, f32)>,
}

impl Geo {
    #[must_use]
    pub fn of(m: &Menu, rows: &[Row], monitor_h: f32) -> Geo {
        let quick = if m.quick.is_empty() { 0.0 } else { QUICK_H };
        let (placed, content_h) = stack(rows);
        let max_list = (monitor_h * MAX_SHARE).round() - HEAD_H - quick - 2.0 * PAD;
        let view_h = content_h.min(max_list.max(ROW_H));
        let list_y = PAD + HEAD_H + quick;
        Geo { w: menu_width(rows), h: list_y + view_h + PAD, list_y, view_h, content_h, rows: placed }
    }

    #[must_use]
    pub fn scrolls(&self) -> bool {
        self.content_h > self.view_h + 0.5
    }

    #[must_use]
    pub fn max_scroll(&self) -> f32 {
        (self.content_h - self.view_h).max(0.0)
    }
}

/// Where a box of `size` goes for a pointer at `at` in a `room` (left, top, right, bottom), all device pixels, by the mockup's
/// rules: to the left of the pointer when it would pass the right edge less `edge`; above it when it would pass the bottom less
/// `edge` (and then no higher than `top + edge`, and not past the bottom either). Returns the top left corner and the corner at
/// the pointer.
#[must_use]
pub fn place(at: (i32, i32), size: (i32, i32), room: (i32, i32, i32, i32), edge: i32) -> ((i32, i32), (bool, bool)) {
    let (mut x, mut y) = at;
    let left = x + size.0 > room.2 - edge;
    if left {
        x = (x - size.0).max(room.0 + edge);
    }
    let up = y + size.1 > room.3 - edge;
    if up {
        y = (y - size.1).max(room.1 + edge);
        if y + size.1 > room.3 - edge {
            y = room.3 - edge - size.1;
        }
    }
    ((x, y), (left, up))
}

/// Where a submenu of `size` stands for a menu card at `card` with the row at `row_y` (screen DIPs): to the right with its
/// edge 4 under the menu's, to the left when it would pass the room's right edge, 6 above the row, kept inside the room.
#[must_use]
pub fn sub_place(card: Rect, row_y: f32, size: (f32, f32), room: (f32, f32, f32, f32)) -> (Rect, bool) {
    let mut x = card.x + card.w - SUB_LAP;
    let flip = x + size.0 > room.2 - EDGE;
    if flip {
        x = card.x - size.0 + SUB_LAP;
    }
    let mut y = row_y - SUB_UP;
    if y + size.1 > room.3 - EDGE {
        y = room.3 - EDGE - size.1;
    }
    y = y.max(room.1 + EDGE);
    (Rect::new(x, y, size.0, size.1), flip)
}

/// The next item of `rows` after `from` going `down` or up, round the ends; from nothing: the first going down, the last going up.
#[must_use]
pub fn step(rows: &[Row], from: Option<usize>, down: bool) -> Option<usize> {
    let items: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.is_item()).map(|(i, _)| i).collect();
    if items.is_empty() {
        return None;
    }
    let at = from.and_then(|f| items.iter().position(|&i| i == f));
    let to = match (at, down) {
        (None, true) => 0,
        (None, false) => items.len() - 1,
        (Some(p), true) => (p + 1) % items.len(),
        (Some(p), false) => (p + items.len() - 1) % items.len(),
    };
    Some(items[to])
}

/// What a row's keys press: `["Ctrl", "Shift", "C"]` is Ctrl+Shift+C. Only a chord (a key with a modifier) counts: a plain Enter
/// belongs to the lit row.
#[must_use]
pub fn chord(keys: &[String]) -> Option<((bool, bool, bool), u32)> {
    let (last, mods) = keys.split_last()?;
    let has = |m: &str| mods.iter().any(|k| k.eq_ignore_ascii_case(m));
    let held = (has("Ctrl"), has("Shift"), has("Alt"));
    if mods.len() != usize::from(held.0) + usize::from(held.1) + usize::from(held.2) || !(held.0 || held.1 || held.2) {
        return None;
    }
    let vk = match last.to_ascii_uppercase().as_str() {
        "ENTER" => 0x0D,
        "DEL" | "DELETE" => 0x2E,
        k if k.len() >= 2 && k.starts_with('F') && k[1..].parse::<u32>().is_ok_and(|n| (1..=12).contains(&n)) => 0x6F + k[1..].parse::<u32>().ok()?,
        k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => u32::from(k.as_bytes()[0]),
        _ => return None,
    };
    Some((held, vk))
}

/// Ctrl, Shift and Alt as they are held now.
fn held() -> (bool, bool, bool) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    // SAFETY: a plain query of the keyboard state.
    let down = |k| unsafe { GetKeyState(i32::from(k)) } < 0;
    (down(VK_CONTROL), down(VK_SHIFT), down(VK_MENU))
}

/// The first item after `from` (round) whose label starts with `ch`, ignoring case.
#[must_use]
pub fn jump(rows: &[Row], from: Option<usize>, ch: char) -> Option<usize> {
    let items: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| r.is_item()).map(|(i, _)| i).collect();
    let at = from.and_then(|f| items.iter().position(|&i| i == f));
    let n = items.len();
    (1..=n).map(|k| items[(at.unwrap_or(n - 1) + k) % n.max(1)]).find(|&i| rows[i].label.to_lowercase().starts_with(ch.to_ascii_lowercase()))
}

// ---------------------------------------------------------------------------------------------------------------------
// the menu

/// What the host does for the panel: what was chosen, and which submenu to ask Windows for.
pub struct Hooks {
    pub pick: Box<dyn Fn(&Do)>,
    pub fill: Box<dyn Fn(u32)>,
}

/// The submenus that came after the panel opened, by handle: the host puts them in, the panel takes them out.
pub type Inbox = Rc<RefCell<Vec<(u32, Vec<Row>)>>>;

/// Where the menu stands: the pointer, the room it has and the monitor.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    pub scale: f32,
    /// The pointer, device pixels.
    pub at: (i32, i32),
    /// Left, top, right, bottom of the room (the monitor's work area less the bar), device pixels.
    pub room: (i32, i32, i32, i32),
    /// The monitor's height in DIPs (the list scrolls above 80% of it).
    pub monitor_h: f32,
}

/// The two cards (the menu, and the menu with the whole list) in the box's own DIPs, the box, and where it stands.
#[derive(Clone, Debug)]
struct Stage {
    /// The box's top left corner on the screen, device pixels, and its size in DIPs.
    at: (i32, i32),
    size: (f32, f32),
    cards: [Rect; 2],
    geo: [Geo; 2],
    /// The corner of each card the pointer is at (left, up).
    corner: [(bool, bool); 2],
    /// The room in the box's own DIPs.
    room: (f32, f32, f32, f32),
}

/// Where a card goes: its top left corner, the corner the pointer is at, and its size, device pixels.
type Placed = ((i32, i32), (bool, bool), (i32, i32));

fn stage(m: &Menu, env: Env) -> Stage {
    let s = env.scale;
    let geo = [Geo::of(m, &m.rows, env.monitor_h), Geo::of(m, &m.full, env.monitor_h)];
    let px = |v: f32| (v * s).round() as i32;
    let edge = px(EDGE);
    let placed: Vec<Placed> = geo
        .iter()
        .map(|g| {
            let size = (px(g.w), px(g.h));
            let (at, corner) = place(env.at, size, env.room, edge);
            (at, corner, size)
        })
        .collect();
    let (x0, y0) = (placed[0].0.0.min(placed[1].0.0), placed[0].0.1.min(placed[1].0.1));
    let (x1, y1) = ((placed[0].0.0 + placed[0].2.0).max(placed[1].0.0 + placed[1].2.0), (placed[0].0.1 + placed[0].2.1).max(placed[1].0.1 + placed[1].2.1));
    let card = |i: usize| Rect::new((placed[i].0.0 - x0) as f32 / s, (placed[i].0.1 - y0) as f32 / s, placed[i].2.0 as f32 / s, placed[i].2.1 as f32 / s);
    Stage {
        at: (x0, y0),
        size: ((x1 - x0) as f32 / s, (y1 - y0) as f32 / s),
        cards: [card(0), card(1)],
        geo: [geo[0].clone(), geo[1].clone()],
        corner: [placed[0].1, placed[1].1],
        room: ((env.room.0 - x0) as f32 / s, (env.room.1 - y0) as f32 / s, (env.room.2 - x0) as f32 / s, (env.room.3 - y0) as f32 / s),
    }
}

/// An open submenu: the row of the menu it belongs to, and the item the keyboard is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Open {
    parent: usize,
    f: Option<usize>,
}

/// What a hover is about to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Later {
    Open(usize),
    Close,
}

#[derive(Default)]
struct St {
    more: bool,
    scroll: f32,
    /// The row that is lit: the pointer's or the keyboard's.
    foc: Option<usize>,
    sub: Option<Open>,
    sub_scroll: f32,
    /// The handle being waited for, the row it opens from, and whether the keyboard asked.
    loading: Option<(u32, usize, bool)>,
    asked: HashSet<u32>,
    later: Option<(Later, u64)>,
    /// The host's hover as the last frame saw it, and the one the keyboard put there itself.
    seen: Option<i32>,
    /// When the pointer came to rest on the row it is on (for the tip).
    rest: Option<(i32, u64)>,
    /// A tile is hovered: its hint is in the header.
    tile: Option<usize>,
    /// When the card last changed (the list's swap animation).
    swap: Option<u64>,
    last_now: u64,
}

pub struct FileMenu {
    menu: Menu,
    stage: Stage,
    hooks: Rc<Hooks>,
    inbox: Inbox,
    /// For each row of the menu and of the whole list: the submenu it opens (a key of `loaded`).
    subkey: [Vec<Option<u32>>; 2],
    loaded: RefCell<HashMap<u32, Rc<Vec<Row>>>>,
    st: RefCell<St>,
    pics: RefCell<HashMap<usize, Picture>>,
}

impl FileMenu {
    #[must_use]
    pub fn new(menu: Menu, env: Env, hooks: Hooks, inbox: Inbox) -> Self {
        let stage = stage(&menu, env);
        let mut loaded = HashMap::new();
        let mut next = 0x8000_0000u32;
        let mut keys = |rows: &[Row]| -> Vec<Option<u32>> {
            rows.iter()
                .map(|r| match &r.sub {
                    Some(Under::Lazy(h)) => Some(*h),
                    Some(Under::Items(v)) => {
                        next += 1;
                        loaded.insert(next, Rc::new(v.clone()));
                        Some(next)
                    }
                    None => None,
                })
                .collect()
        };
        let subkey = [keys(&menu.rows), keys(&menu.full)];
        Self { menu, stage, hooks: Rc::new(hooks), inbox, subkey, loaded: RefCell::new(loaded), st: RefCell::new(St::default()), pics: RefCell::new(HashMap::new()) }
    }

    /// For a render of a state of the board: the whole list or not, the lit row and the row whose submenu is open.
    pub fn show(&self, more: bool, foc: Option<usize>, sub: Option<usize>) {
        {
            let mut st = self.st.borrow_mut();
            st.more = more;
            st.foc = foc;
        }
        if let Some(r) = sub {
            self.open_sub(r, false);
        }
    }

    /// Where the box stands (device pixels) and its size in DIPs.
    #[must_use]
    pub fn stage_at(&self) -> ((i32, i32), (f32, f32)) {
        (self.stage.at, self.stage.size)
    }

    fn rows(&self, more: bool) -> &[Row] {
        if more { &self.menu.full } else { &self.menu.rows }
    }

    fn idx(more: bool) -> usize {
        usize::from(more)
    }

    fn card(&self, more: bool) -> Rect {
        self.stage.cards[Self::idx(more)]
    }

    fn geo(&self, more: bool) -> &Geo {
        &self.stage.geo[Self::idx(more)]
    }

    /// The items of the submenu of row `r` once it is there.
    fn list(&self, more: bool, r: usize) -> Option<Rc<Vec<Row>>> {
        let key = (*self.subkey[Self::idx(more)].get(r)?)?;
        self.loaded.borrow().get(&key).cloned()
    }

    fn handle(&self, more: bool, r: usize) -> Option<u32> {
        self.subkey[Self::idx(more)].get(r).copied().flatten()
    }

    /// What the inbox brought: the rows are kept, and a submenu that was waited for opens.
    fn take_inbox(&self, now: u64) {
        let news: Vec<(u32, Vec<Row>)> = std::mem::take(&mut *self.inbox.borrow_mut());
        for (h, rows) in news {
            // the keys of `subkey` are the handles of the shell's submenus
            self.loaded.borrow_mut().insert(h, Rc::new(rows));
            let mut st = self.st.borrow_mut();
            if let Some((lh, row, kb)) = st.loading
                && lh == h
            {
                st.loading = None;
                let n = self.loaded.borrow().get(&h).map_or(0, |v| v.len());
                if n > 0 {
                    let first = if kb { step(&self.loaded.borrow()[&h], None, true) } else { None };
                    st.sub = Some(Open { parent: row, f: first });
                    st.sub_scroll = 0.0;
                    st.foc = Some(row);
                }
            } else if let more = st.more
                && let Some(o) = st.sub.as_mut()
                && self.handle(more, o.parent) == Some(h)
                && let Some(len) = self.loaded.borrow().get(&h).map(|v| v.len())
                && o.f.is_some_and(|f| f >= len)
            {
                o.f = None; // the list got shorter under the keyboard
            }
        }
        self.st.borrow_mut().last_now = now;
    }

    /// Opens the submenu of row `row` (`kb`: the keyboard did, so its first item is lit). A submenu Windows has not filled yet is
    /// asked for once, and shows a spinner until it comes.
    fn open_sub(&self, row: usize, kb: bool) {
        let more = self.st.borrow().more;
        if self.rows(more).get(row).is_none_or(|r| r.sub.is_none()) {
            self.st.borrow_mut().sub = None;
            return;
        }
        if let Some(list) = self.list(more, row) {
            let mut st = self.st.borrow_mut();
            st.loading = None;
            if list.is_empty() {
                return;
            }
            st.sub = Some(Open { parent: row, f: if kb { step(&list, None, true) } else { None } });
            st.sub_scroll = 0.0;
            st.foc = Some(row);
            return;
        }
        let Some(h) = self.handle(more, row) else { return };
        let ask = {
            let mut st = self.st.borrow_mut();
            st.loading = Some((h, row, kb));
            st.foc = Some(row);
            st.asked.insert(h)
        };
        if ask {
            (self.hooks.fill)(h);
        }
    }

    fn close_sub(&self) {
        let mut st = self.st.borrow_mut();
        st.sub = None;
        st.loading = None;
    }

    fn set_more(&self, more: bool, now: u64) {
        let mut st = self.st.borrow_mut();
        st.more = more;
        st.scroll = 0.0;
        st.foc = None;
        st.sub = None;
        st.loading = None;
        st.later = None;
        st.swap = Some(now);
    }

    /// The row's top in the box now (DIPs), for the submenu to stand beside.
    fn row_top(&self, more: bool, r: usize, scroll: f32) -> f32 {
        let (g, c) = (self.geo(more), self.card(more));
        c.y + g.list_y + g.rows[r].0 - scroll
    }

    /// The submenu's card now, if one is open: its rect, which side of the menu it stands on, its rows and its parent row.
    fn sub_card(&self) -> Option<(Rect, bool, Rc<Vec<Row>>, usize)> {
        self.sub_card_with(&self.st.borrow())
    }

    /// What a row does when it is chosen (a click or Enter).
    fn run(&self, act: &Do, off: bool, cx: &mut Cx, now: u64) {
        if off {
            return;
        }
        match act {
            Do::Nothing => {}
            Do::More => self.set_more(true, now),
            Do::Less => self.set_more(false, now),
            a => {
                // handed to the host once the panel has closed
                let (a, hooks) = (a.clone(), Rc::clone(&self.hooks));
                cx.close_then(move || (hooks.pick)(&a));
            }
        }
    }

    fn pick_row(&self, id: i32, cx: &mut Cx, now: u64) {
        let more = self.st.borrow().more;
        if let Some(r) = usize::try_from(id - ROW).ok().filter(|_| (ROW..SUB).contains(&id)) {
            let Some(row) = self.rows(more).get(r) else { return };
            if row.sub.is_some() {
                if !row.off {
                    self.open_sub(r, false);
                }
                return;
            }
            self.run(&row.act.clone(), row.off, cx, now);
        } else if (SUB..BLANK).contains(&id) {
            let j = (id - SUB) as usize;
            let Some((_, _, list, _)) = self.sub_card() else { return };
            if let Some(row) = list.get(j) {
                self.run(&row.act.clone(), row.off, cx, now);
            }
        }
    }
}

impl Drop for FileMenu {
    fn drop(&mut self) {
        // the menu is gone whether or not something was chosen: the host tells rust-explorer to let go of it
        (self.hooks.pick)(&Do::Nothing);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// drawing

fn tint(t: Tint) -> Rgba {
    match t {
        Tint::Dim => DIM,
        Tint::Blue => BLUE,
        Tint::Red => DANGER,
    }
}

/// A key cap of a row (`kbd`: 10.5 px, weight 500, 1 x 5 padding, radius 4, `--t3` on `rgba(ink,.07)`).
fn kbd_width(c: &mut Canvas<'_>, k: &str) -> f32 {
    c.measure(k, Font::new(10.5, 500)).ceil() + 10.0
}

fn kbd(c: &mut Canvas<'_>, x: f32, cy: f32, k: &str) -> f32 {
    let w = kbd_width(c, k);
    let r = Rect::new(x, cy - 7.5, w, 15.0);
    c.fill(r, 4.0, Rgba::white(0.07));
    c.fill(Rect::new(r.x + 1.5, r.y + r.h - 1.0, r.w - 3.0, 1.0), 0.0, Rgba::white(0.08));
    c.text(k, r, Font::new(10.5, 500), MUTED, Align::Center);
    w
}

fn spinner(c: &mut Canvas<'_>, cx: f32, cy: f32, now: u64) {
    c.ring(Rect::new(cx - 6.0, cy - 6.0, 12.0, 12.0), 6.0, 2.0, Rgba::white(0.15));
    let turn = (now % 800) as f32 / 800.0 * 360.0; // `fc_spin .8s linear infinite`
    c.arc((cx, cy), 6.0, -90.0 + turn, 90.0, 2.0, DIM);
}

impl FileMenu {
    fn bitmap(&self, c: &mut Canvas<'_>, icon: &Rc<Icon>, r: Rect) {
        let key = Rc::as_ptr(icon) as usize;
        let mut pics = self.pics.borrow_mut();
        if pics.get(&key).is_some_and(|p| c.picture_stale(p)) {
            pics.remove(&key);
        }
        if !pics.contains_key(&key) && let Some(p) = c.pixels(&icon.px, i32::from(icon.w), i32::from(icon.h)) {
            pics.insert(key, p);
        }
        if let Some(p) = pics.get(&key) {
            c.picture_in(p, r, 0.0, 1.0, Sample::Linear);
        }
    }

    /// One row at `r` (a menu's or a submenu's): the plate, the icon, the label, the keys, the badge, the arrow or the spinner.
    #[allow(clippy::too_many_arguments)]
    fn draw_row(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>, id: i32, row: &Row, r: Rect, lit: bool, open: bool, loading: bool) {
        match row.kind {
            Kind::Sep => {
                c.fill(Rect::new(r.x + 8.0, r.y + 4.0, r.w - 16.0, 1.0), 0.0, Rgba::white(0.08));
                return;
            }
            Kind::Caption => {
                c.text(&row.label, Rect::new(r.x + 10.0, r.y + 6.0, r.w - 20.0, r.h - 6.0), Font::new(11.5, 500), MUTED, Align::Start);
                return;
            }
            Kind::Item => {}
        }
        let on = f.tween(id, 3, f32::from(u8::from(lit || open)), 100, EASE);
        let press = f.tween(id, 4, f32::from(f.press == Some(id) && !row.off), 90, SPRING);
        c.faded(if row.off { 0.4 } else { 1.0 }, |c| {
            c.transformed(r.cx(), r.cy(), 1.0 - 0.01 * press, 0.0, 0.0, |c| {
                if on > 0.001 {
                    c.fill(r, 9.0, foc().fade(on));
                }
                let (ink, icon_ink) = if row.danger { (DANGER, DANGER) } else { (INK, DIM) };
                // the icon: 17 px in an 18 px slot
                let ic = (r.x + 10.0 + 9.0, r.cy());
                match &row.pic {
                    Pic::None => {}
                    Pic::Line(d, t) => c.icon(d, ic, 17.0, 1.8, if row.danger { DANGER } else if *t == Tint::Dim { icon_ink } else { tint(*t) }, 0.0),
                    Pic::Bitmap(b) => self.bitmap(c, b, Rect::new(ic.0 - 8.0, ic.1 - 8.0, 16.0, 16.0)),
                }
                // what stands on the right, from the right edge in
                let mut right = r.x + r.w - 10.0;
                if loading {
                    spinner(c, right - 6.5, r.cy(), f.now);
                    right -= 13.0 + 11.0;
                } else if row.sub.is_some() {
                    c.icon("M9 6l6 6-6 6", (right - 6.5, r.cy()), 13.0, 2.2, MUTED, 0.0);
                    right -= 13.0 + 11.0;
                } else if row.checked {
                    c.icon(CHECK, (right - 6.5, r.cy()), 13.0, 2.2, DIM, 0.0);
                    right -= 13.0 + 11.0;
                } else if !row.keys.is_empty() {
                    let total: f32 = row.keys.iter().map(|k| kbd_width(c, k)).sum::<f32>() + 3.0 * (row.keys.len() as f32 - 1.0);
                    let mut x = right - total;
                    for k in &row.keys {
                        x += kbd(c, x, r.cy(), k) + 3.0;
                    }
                    right -= total + 11.0;
                }
                if !row.badge.is_empty() {
                    let w = c.measure(&row.badge, Font::new(11.5, 400)).ceil() + 2.0;
                    c.text(&row.badge, Rect::new(right - w, r.y, w, r.h), Font::new(11.5, 400), MUTED, Align::End);
                    right -= w + 11.0;
                }
                let x = r.x + 10.0 + 18.0 + 11.0;
                c.text_fit(&row.label, Rect::new(x, r.y, (right + 11.0 - x).max(10.0), r.h), Font::new(13.0, if row.bold { 600 } else { 400 }), ink, Align::Start);
            });
        });
    }

    fn draw_head(&self, c: &mut Canvas<'_>, head: &Head, w: f32, meta: &str, meta_lit: bool) {
        let icon = Rect::new(PAD + 6.0, PAD + 8.0, 32.0, 32.0);
        match &head.icon {
            HeadIcon::Shell(i) => c.hicon(i.0, icon, 1.0),
            HeadIcon::Thumb(i) => c.hicon_cover(i.0, icon, 8.0),
            HeadIcon::Plate(d, from, to, ink) => {
                let col = |v: u32| Rgba::new(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, 1.0);
                c.gradient_diagonal(icon, 8.0, col(*from), col(*to));
                c.icon(d, (icon.cx(), icon.cy()), 17.0, 1.9, col(*ink), 0.0);
            }
        }
        let x = PAD + 6.0 + 32.0 + 10.0;
        let tw = (w - x - PAD - 8.0).max(10.0);
        c.text_fit(&head.name, Rect::new(x, PAD + 8.0, tw, 16.0), Font::new(13.0, 600), INK, Align::Start);
        c.text_fit(meta, Rect::new(x, PAD + 8.0 + 17.0, tw, 15.0), Font::new(12.0, 400), if meta_lit { INK } else { MUTED }, Align::Start);
    }

    fn quick_rects(&self, g: &Geo, n: usize) -> Vec<Rect> {
        if n == 0 {
            return Vec::new();
        }
        let tw = (g.w - 2.0 * PAD - 4.0 - 4.0 * (n as f32 - 1.0)) / n as f32;
        (0..n).map(|i| Rect::new(PAD + 2.0 + i as f32 * (tw + 4.0), PAD + HEAD_H + 2.0, tw, 40.0)).collect()
    }

    /// The viewport of the list, in the card's own DIPs.
    fn view(&self, g: &Geo) -> Rect {
        Rect::new(PAD, g.list_y, g.w - 2.0 * PAD, g.view_h)
    }

    fn fade_below(&self, c: &mut Canvas<'_>, r: Rect) {
        let a = glass().alpha(0.92);
        c.gradient(Rect::new(r.x - 2.0, r.y + r.h - 28.0, r.w + 4.0, 28.0), 0.0, a.alpha(0.0), a);
    }

    fn scrollbar(&self, c: &mut Canvas<'_>, view: Rect, scroll: f32, content_h: f32) {
        if content_h <= view.h + 0.5 {
            return;
        }
        let th = (view.h * view.h / content_h).max(24.0);
        let y = view.y + (view.h - th) * (scroll / (content_h - view.h)).clamp(0.0, 1.0);
        c.fill(Rect::new(view.x + view.w - 4.0, y, 3.0, th), 1.5, Rgba::white(0.18));
    }
}

impl Content for FileMenu {
    fn id(&self) -> Id {
        Id::Menu
    }

    /// `--menu`, blur 40, saturate 1.6, radius 16, `0 0 0 1px rgba(0,0,0,.45)`, `inset 0 0 0 1px rgba(ink,.1)`, `0 24px 64px rgba(0,0,0,.45)`.
    fn look(&self) -> Look {
        Look {
            radius: 16.0,
            tint: glass(),
            blur: 40.0,
            saturate: 1.6,
            ring_out: Rgba::new(0.0, 0.0, 0.0, 0.45),
            ring_in: Rgba::white(0.1),
            top_light: Rgba::TRANSPARENT,
            shadow: Shadow { dy: 24.0, blur: 64.0, color: Rgba::new(0.0, 0.0, 0.0, 0.45) },
            scrim: None,
        }
    }

    /// `fc_in .17s`: from `opacity 0, scale(.96)` around the corner at the pointer.
    fn entry(&self) -> Entry {
        let (c, (left, up)) = (self.card(false), self.stage.corner[0]);
        let origin = Origin::Px(c.x + if left { c.w } else { 0.0 }, c.y + if up { c.h } else { 0.0 });
        Entry { from: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 0.96, clip: None }, dur: 170, ease: SPRING, origin }
    }

    fn exit(&self) -> Exit {
        let st = self.st.borrow();
        let (c, (left, up)) = (self.card(st.more), self.stage.corner[Self::idx(st.more)]);
        Exit { to: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 0.98, clip: None }, dur: 110, ease: EASE, origin: Origin::Px(c.x + if left { c.w } else { 0.0 }, c.y + if up { c.h } else { 0.0 }) }
    }

    fn place(&self) -> Place {
        Place { along: Along::Abs(self.stage.at.0, self.stage.at.1), gap: 0.0 }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.stage.size
    }

    fn reach(&self) -> f32 {
        REACH
    }

    fn settle(&self) -> u32 {
        400
    }

    fn cards(&self) -> Vec<Rect> {
        vec![self.card(self.st.borrow().more)]
    }

    fn animating(&self, _now: u64) -> bool {
        let st = self.st.borrow();
        st.loading.is_some() || st.later.is_some()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        self.take_inbox(f.now);
        self.sync(f);
        let (more, scroll, foc_row, sub, loading, tile, swap) = {
            let st = self.st.borrow();
            (st.more, st.scroll, st.foc, st.sub, st.loading, st.tile, st.swap)
        };
        let (g, card) = (self.geo(more), self.card(more));
        let rows = self.rows(more);
        let sw = swap.map_or(1.0, |t| EASE.ease((f.now.saturating_sub(t)) as f32 / 200.0));
        if sw < 1.0 {
            f.due(f.now + 16);
        }
        c.transformed(0.0, 0.0, 1.0, card.x, card.y, |c| {
            // the header; while a tile is hovered its hint is in the second line
            let (meta, lit) = match tile.and_then(|t| self.menu.quick.get(t)) {
                Some(q) => (q.tip.as_str(), true),
                None => (self.menu.head.meta.as_str(), false),
            };
            self.draw_head(c, &self.menu.head, g.w, meta, lit);
            // the tiles
            for (i, (q, r)) in self.menu.quick.iter().zip(self.quick_rects(g, self.menu.quick.len())).enumerate() {
                self.draw_tile(c, f, i, q, r);
            }
            // the list, scrolled and cut to its viewport
            let view = self.view(g);
            c.clipped(view, |c| {
                c.faded(sw, |c| {
                    c.transformed(0.0, 0.0, 1.0, 0.0, -4.0 * (1.0 - sw) - scroll, |c| {
                        for (i, row) in rows.iter().enumerate() {
                            let (y, h) = g.rows[i];
                            if g.list_y + y + h - scroll < g.list_y - 2.0 || g.list_y + y - scroll > g.list_y + g.view_h + 2.0 {
                                continue;
                            }
                            let r = Rect::new(PAD, g.list_y + y, g.w - 2.0 * PAD, h);
                            let open = sub.is_some_and(|o| o.parent == i);
                            let busy = loading.is_some_and(|(_, row, _)| row == i);
                            self.draw_row(c, f, ROW + i as i32, row, r, foc_row == Some(i), open, busy);
                        }
                    });
                });
            });
            if g.scrolls() {
                if scroll < g.max_scroll() - 0.5 {
                    self.fade_below(c, view);
                }
                self.scrollbar(c, view, scroll, g.content_h);
            }
        });
    }

    /// The submenu, beside the menu on a glass of its own (`.fsm`: opacity and a 4 px slide in .14 s).
    fn overlay(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (scroll_sub, sub_f) = {
            let st = self.st.borrow();
            (st.sub_scroll, st.sub.and_then(|o| o.f))
        };
        let open = self.sub_card();
        let shown = f.tween(T_SUB, 1, f32::from(u8::from(open.is_some())), 140, SPRING);
        if let Some((card, flip, list, _)) = open {
            let slide = 4.0 * (1.0 - shown) * if flip { 1.0 } else { -1.0 };
            let (_, content_h) = stack(&list);
            let view = Rect::new(card.x + SUB_PAD, card.y + SUB_PAD, card.w - 2.0 * SUB_PAD, card.h - 2.0 * SUB_PAD);
            c.faded(shown, |c| {
                c.transformed(0.0, 0.0, 1.0, slide, 0.0, |c| {
                    c.shadow(card, 14.0, 20.0, 50.0, Rgba::new(0.0, 0.0, 0.0, 0.45));
                    c.fill(card, 14.0, glass().alpha(0.97));
                    c.ring(card, 14.0, 1.0, Rgba::white(0.1));
                    c.ring(card.inset(-1.0), 15.0, 1.0, Rgba::new(0.0, 0.0, 0.0, 0.45));
                    let (placed, _) = stack(&list);
                    c.clipped(view, |c| {
                        for (j, row) in list.iter().enumerate() {
                            let (y, h) = placed[j];
                            let r = Rect::new(view.x, view.y + y - scroll_sub, view.w, h);
                            self.draw_row(c, f, SUB + j as i32, row, r, sub_f == Some(j), false, false);
                        }
                    });
                    self.scrollbar(c, view, scroll_sub, content_h);
                });
            });
        }
        self.draw_tip(c, f);
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        let st = self.st.borrow();
        // the submenu stands over everything
        if let Some((card, _, list, _)) = self.sub_card_with(&st)
            && card.contains(x, y)
        {
            let (placed, _) = stack(&list);
            for (j, row) in list.iter().enumerate() {
                let r = Rect::new(card.x + SUB_PAD, card.y + SUB_PAD + placed[j].0 - st.sub_scroll, card.w - 2.0 * SUB_PAD, placed[j].1);
                if row.is_item() && r.contains(x, y) && y >= card.y + SUB_PAD && y <= card.y + card.h - SUB_PAD {
                    return Some(SUB + j as i32);
                }
            }
            return None;
        }
        let (g, card) = (self.geo(st.more), self.card(st.more));
        if !card.contains(x, y) {
            return Some(BLANK);
        }
        let (lx, ly) = (x - card.x, y - card.y);
        for (i, r) in self.quick_rects(g, self.menu.quick.len()).iter().enumerate() {
            if r.contains(lx, ly) {
                return Some(QUICK + i as i32);
            }
        }
        let view = self.view(g);
        if view.contains(lx, ly) {
            for (i, row) in self.rows(st.more).iter().enumerate() {
                let (ry, rh) = g.rows[i];
                if row.is_item() && ly >= g.list_y + ry - st.scroll && ly <= g.list_y + ry + rh - st.scroll {
                    return Some(ROW + i as i32);
                }
            }
        }
        None
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        if id == BLANK {
            cx.close(); // a click beside the menu closes it, and goes no further
            return;
        }
        if b != Button::Left {
            return;
        }
        let now = self.st.borrow().last_now;
        if (QUICK..ROW).contains(&id) {
            if let Some(q) = self.menu.quick.get((id - QUICK) as usize) {
                let a = q.act.clone();
                self.run(&a, false, cx, now);
            }
            return;
        }
        self.pick_row(id, cx, now);
    }

    fn enter(&mut self, _hover: Option<i32>, _mods: Mods, _cx: &mut Cx) {}

    /// Esc closes the submenu first.
    fn escape(&mut self) -> bool {
        let mut st = self.st.borrow_mut();
        let had = st.sub.is_some() || st.loading.is_some();
        st.sub = None;
        st.loading = None;
        had
    }

    fn wheel(&mut self, x: f32, y: f32, up: bool) {
        let step = if up { -48.0 } else { 48.0 };
        let mut st = self.st.borrow_mut();
        if let Some((card, _, list, _)) = self.sub_card_with(&st) && card.contains(x, y) {
            let max = (stack(&list).1 + 2.0 * SUB_PAD - card.h).max(0.0);
            st.sub_scroll = (st.sub_scroll + step).clamp(0.0, max);
            return;
        }
        let (g, card) = (self.geo(st.more), self.card(st.more));
        if card.contains(x, y) && g.scrolls() {
            st.scroll = (st.scroll + step).clamp(0.0, g.max_scroll());
            st.sub = None; // `onScroll`: the submenu goes
            st.loading = None;
        }
    }

    /// The mockup's keys: arrows, Enter, Esc (`escape`), F2, Delete, and a letter that jumps to the next label that starts with it.
    fn key(&mut self, vk: u32, cx: &mut Cx) -> bool {
        self.key_with(vk, held(), cx)
    }

    fn nav(&self, _from: Option<i32>, _dir: Dir) -> Option<i32> {
        None
    }
}

impl FileMenu {
    /// [`Content::key`] with the modifiers given. A row's keys do what they say while the menu is open (Alt+Enter is Properties,
    /// Ctrl+Shift+C Copy as path, Shift+F10 Show more options); the rest are the mockup's.
    fn key_with(&mut self, vk: u32, mods: (bool, bool, bool), cx: &mut Cx) -> bool {
        const UP: u32 = 0x26;
        const DOWN: u32 = 0x28;
        const LEFT: u32 = 0x25;
        const RIGHT: u32 = 0x27;
        const HOME: u32 = 0x24;
        const END: u32 = 0x23;
        const TAB: u32 = 0x09;
        const ENTER: u32 = 0x0D;
        const F2: u32 = 0x71;
        const DELETE: u32 = 0x2E;
        let now = self.st.borrow().last_now;
        let (more, sub, foc_row) = {
            let st = self.st.borrow();
            (st.more, st.sub, st.foc)
        };
        if mods != (false, false, false) {
            let hit = self.rows(more).iter().find(|r| r.is_item() && !r.off && chord(&r.keys) == Some((mods, vk))).map(|r| (r.act.clone(), r.off));
            if let Some((act, off)) = hit {
                self.run(&act, off, cx, now);
                return true;
            }
            return false;
        }
        // in an open submenu the keys are its own
        if let Some(o) = sub && let Some(list) = self.list(more, o.parent) {
            let mut st = self.st.borrow_mut();
            let set = |st: &mut St, f: Option<usize>| {
                if let Some(s) = st.sub.as_mut() {
                    s.f = f;
                }
            };
            match vk {
                DOWN | TAB => set(&mut st, step(&list, o.f, true)),
                UP => set(&mut st, step(&list, o.f, false)),
                HOME => set(&mut st, step(&list, None, true)),
                END => set(&mut st, step(&list, None, false)),
                LEFT => {
                    st.sub = None;
                    st.foc = Some(o.parent);
                }
                ENTER => {
                    drop(st);
                    if let Some(row) = o.f.and_then(|f| list.get(f)) {
                        self.run(&row.act.clone(), row.off, cx, now);
                    }
                }
                0x41..=0x5A => {
                    if let Some(j) = jump(&list, o.f, vk as u8 as char) {
                        set(&mut st, Some(j));
                    }
                }
                _ => return false,
            }
            return true;
        }
        let rows = self.rows(more);
        match vk {
            DOWN | TAB => {
                let to = step(rows, foc_row, true);
                self.focus(to);
            }
            UP => {
                let to = step(rows, foc_row, false);
                self.focus(to);
            }
            HOME => self.focus(step(rows, None, true)),
            END => self.focus(step(rows, None, false)),
            RIGHT => {
                if let Some(r) = foc_row.filter(|&r| rows.get(r).is_some_and(|x| x.sub.is_some() && !x.off)) {
                    self.open_sub(r, true);
                }
            }
            LEFT => {}
            ENTER => {
                if let Some(row) = foc_row.and_then(|r| rows.get(r)) {
                    if row.sub.is_some() {
                        if !row.off {
                            self.open_sub(foc_row.unwrap_or(0), true);
                        }
                    } else {
                        self.run(&row.act.clone(), row.off, cx, now);
                    }
                }
            }
            F2 => {
                if let Some(q) = self.menu.quick.iter().find(|q| q.act == Do::Ours(winrust_common::filemenu::Act::Rename)) {
                    self.run(&q.act.clone(), false, cx, now);
                }
            }
            DELETE => {
                if let Some(q) = self.menu.quick.iter().find(|q| q.danger) {
                    self.run(&q.act.clone(), false, cx, now);
                }
            }
            0x41..=0x5A => self.focus(jump(rows, foc_row, vk as u8 as char)),
            _ => return false,
        }
        true
    }

    fn sub_card_with(&self, st: &St) -> Option<(Rect, bool, Rc<Vec<Row>>, usize)> {
        let o = st.sub?;
        let list = self.list(st.more, o.parent)?;
        let (w, content_h) = (sub_width(&list), stack(&list).1);
        let room = self.stage.room;
        let h = (content_h + 2.0 * SUB_PAD).min(room.3 - room.1 - 2.0 * EDGE);
        let (r, flip) = sub_place(self.card(st.more), self.row_top(st.more, o.parent, st.scroll), (w, h), room);
        Some((r, flip, list, o.parent))
    }

    /// Lights row `to` (the keyboard) and brings it into view.
    fn focus(&self, to: Option<usize>) {
        let mut st = self.st.borrow_mut();
        let g = self.geo(st.more);
        st.foc = to;
        st.sub = None;
        st.loading = None;
        st.later = None;
        st.tile = None;
        if let Some(r) = to {
            let (y, h) = g.rows[r];
            if y < st.scroll {
                st.scroll = y;
            } else if y + h > st.scroll + g.view_h {
                st.scroll = (y + h - g.view_h).min(g.max_scroll());
            }
        }
    }

    /// What the pointer's hover does to the menu, as the mockup's `onMouseEnter` does it: the lit row follows the pointer, a row
    /// with a submenu opens it after 170 ms, any other row closes an open one after 220 ms, and a tile closes it after 200 ms.
    fn sync(&self, f: &mut Frame<'_>) {
        let mut st = self.st.borrow_mut();
        let now = f.now;
        st.last_now = now;
        if f.hover != st.seen {
            st.seen = f.hover;
            st.rest = f.hover.map(|h| (h, now));
            match f.hover {
                Some(h) if (ROW..SUB).contains(&h) => {
                    let r = (h - ROW) as usize;
                    st.foc = Some(r);
                    st.tile = None;
                    let has = self.rows(st.more).get(r).is_some_and(|x| x.sub.is_some());
                    let open_here = st.sub.is_some_and(|o| o.parent == r) || st.loading.is_some_and(|l| l.1 == r);
                    st.later = if has && !open_here { Some((Later::Open(r), now + OPEN_AFTER)) } else if !has && (st.sub.is_some() || st.loading.is_some()) { Some((Later::Close, now + CLOSE_AFTER)) } else { None };
                }
                Some(h) if (QUICK..ROW).contains(&h) => {
                    st.tile = Some((h - QUICK) as usize);
                    st.foc = None;
                    st.later = if st.sub.is_some() || st.loading.is_some() { Some((Later::Close, now + CLOSE_TILE_AFTER)) } else { None };
                }
                Some(h) if (SUB..BLANK).contains(&h) => {
                    // in the submenu: it stays, its lit item follows the pointer
                    st.later = None;
                    let j = (h - SUB) as usize;
                    if let Some(o) = st.sub.as_mut() {
                        o.f = Some(j);
                    }
                }
                _ => {
                    st.tile = None;
                }
            }
        }
        if let Some((what, due)) = st.later {
            if now >= due {
                st.later = None;
                drop(st);
                match what {
                    Later::Open(r) => self.open_sub(r, false),
                    Later::Close => self.close_sub(),
                }
            } else {
                f.due(due);
            }
        }
    }

    /// A long label, tipped once the pointer has rested on it.
    fn draw_tip(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (rest, more) = {
            let st = self.st.borrow();
            (st.rest, st.more)
        };
        let Some((id, since)) = rest.filter(|(id, _)| *id == f.hover.unwrap_or(-1)) else { return };
        let (label, r) = if (SUB..BLANK).contains(&id) {
            let Some((card, _, list, _)) = self.sub_card() else { return };
            let j = (id - SUB) as usize;
            let (placed, _) = stack(&list);
            (list[j].label.clone(), Rect::new(card.x + SUB_PAD, card.y + SUB_PAD + placed[j].0 - self.st.borrow().sub_scroll, card.w - 2.0 * SUB_PAD, ROW_H))
        } else if (ROW..SUB).contains(&id) {
            let i = (id - ROW) as usize;
            let (g, card) = (self.geo(more), self.card(more));
            let Some(row) = self.rows(more).get(i) else { return };
            (row.label.clone(), Rect::new(card.x + PAD, card.y + g.list_y + g.rows[i].0 - self.st.borrow().scroll, g.w - 2.0 * PAD, ROW_H))
        } else {
            return;
        };
        if label.chars().count() <= TIP_LEN {
            return;
        }
        if f.now < since + TIP_AFTER {
            f.due(since + TIP_AFTER);
            return;
        }
        let w = (c.measure(&label, Font::new(12.0, 400)).ceil() + 20.0).min(420.0);
        let tip = Rect::new(r.x + 12.0, r.y + r.h + 2.0, w, 26.0);
        c.shadow(tip, 8.0, 8.0, 20.0, Rgba::new(0.0, 0.0, 0.0, 0.4));
        c.fill(tip, 8.0, Rgba::css(12, 13, 17, 0.96));
        c.ring(tip, 8.0, 1.0, Rgba::white(0.12));
        c.text_fit(&label, Rect::new(tip.x + 10.0, tip.y, tip.w - 20.0, tip.h), Font::new(12.0, 400), Rgba::css(236, 238, 244, 1.0), Align::Start);
    }

    fn draw_tile(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>, i: usize, q: &Quick, r: Rect) {
        let id = QUICK + i as i32;
        let hot = f.hover(id, 120, EASE);
        let press = f.tween(id, 2, f32::from(f.press == Some(id)), 90, SPRING);
        c.transformed(r.cx(), r.cy(), 1.0 - 0.05 * press, 0.0, 0.0, |c| {
            c.fill(r, 10.0, tile_bg());
            if hot > 0.001 {
                c.fill(r, 10.0, if q.danger { red_bg() } else { hov() }.fade(hot));
            }
            let ink = if q.danger { DIM.mix(DANGER, hot) } else { DIM };
            c.icon(q.d, (r.cx(), r.cy()), 18.0, 1.8, ink, 0.0);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winrust_common::filemenu::Act;

    fn item(l: &str, act: Do) -> Row {
        Row::line(l, "M0 0", act)
    }

    fn menu_with(rows: Vec<Row>, full: Vec<Row>, quick: usize) -> Menu {
        Menu {
            head: Head { name: "New text document.txt".into(), meta: "Text document · 2 KB".into(), icon: HeadIcon::Plate("M0 0", 0, 0, 0) },
            quick: (0..quick).map(|i| Quick { d: "M0 0", tip: format!("tip {i}"), danger: i == quick - 1, act: Do::Shell(i as u32 + 1) }).collect(),
            rows,
            full,
        }
    }

    fn env() -> Env {
        Env { scale: 1.0, at: (96, 600), room: (0, 0, 1440, 852), monitor_h: 900.0 }
    }

    /// The text file of the mockup's state 1: Open, Edit, Open with ›, rule, Copy as path, Compress to ZIP, Send to ›, rule, Show
    /// more options, Properties.
    fn text_rows() -> Vec<Row> {
        vec![
            item("Open", Do::Shell(1)).bold().keys(&["Enter"]),
            item("Edit", Do::Shell(2)),
            item("Open with", Do::Nothing).under(Under::Items(vec![item("Notepad", Do::Shell(10)).badge("Default"), item("Visual Studio Code", Do::Shell(11)), Row::sep(), item("Choose another app…", Do::Shell(12))])),
            Row::sep(),
            item("Copy as path", Do::Ours(Act::CopyPath)).keys(&["Ctrl", "Shift", "C"]),
            item("Compress to ZIP", Do::Ours(Act::Zip)),
            item("Send to", Do::Nothing).under(Under::Lazy(7)),
            Row::sep(),
            item("Show more options", Do::More).keys(&["Shift", "F10"]),
            item("Properties", Do::Shell(3)).keys(&["Alt", "Enter"]),
        ]
    }

    fn full_rows() -> Vec<Row> {
        let mut v = vec![item("Fewer options", Do::Less), Row::caption("All options from Windows and other apps")];
        v.extend((0..30).map(|i| item(&format!("Item {i}"), Do::Shell(100 + i))));
        v
    }

    type Rig = (FileMenu, Rc<RefCell<Vec<Do>>>, Rc<RefCell<Vec<u32>>>, Inbox);

    fn panel() -> Rig {
        let (picked, asked, inbox) = (Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(Vec::new())));
        let (p, a) = (Rc::clone(&picked), Rc::clone(&asked));
        let hooks = Hooks { pick: Box::new(move |d| p.borrow_mut().push(d.clone())), fill: Box::new(move |h| a.borrow_mut().push(h)) };
        (FileMenu::new(menu_with(text_rows(), full_rows(), 5), env(), hooks, Rc::clone(&inbox)), picked, asked, inbox)
    }

    #[test]
    fn the_list_is_stacked_as_the_mockups_dom_is() {
        // measured in the mockup (state 1): rows at 102 139 176 | 223 260 297 | 344 381 from the top of the card, 423 tall, 263 wide
        let rows = text_rows();
        let (placed, h) = stack(&rows);
        let tops: Vec<f32> = rows.iter().zip(&placed).filter(|(r, _)| r.is_item()).map(|(_, p)| 102.0 + p.0).collect();
        assert_eq!(tops, vec![102.0, 139.0, 176.0, 223.0, 260.0, 297.0, 344.0, 381.0]);
        assert_eq!(h, 8.0 * 36.0 + 2.0 * 9.0 + 9.0, "eight rows, two rules and nine pixels between ten children");
        let m = menu_with(rows.clone(), vec![], 5);
        let g = Geo::of(&m, &rows, 900.0);
        assert_eq!((g.w, g.h), (263.0, 423.0));
        assert!(!g.scrolls());
        assert_eq!(g.list_y, 102.0);
    }

    #[test]
    fn the_width_follows_the_longest_label_keys_and_badge_within_248_and_320() {
        let one = |l: &str| vec![item(l, Do::Nothing)];
        assert_eq!(menu_width(&one("Open")), 248.0);
        assert_eq!(menu_width(&one(&"w".repeat(100))), 320.0);
        // 17 characters + 8 key letters * .9 + 2 = 26.2 -> 262.8 -> 263
        assert_eq!(menu_width(&text_rows()), 263.0);
        let long = vec![item("Scan with Microsoft Defender…", Do::Nothing)]; // 29 chars -> 282.1
        assert_eq!(menu_width(&long), 282.0);
        // a badge counts its text and one
        assert_eq!(menu_width(&[item("Empty Recycle Bin", Do::Nothing).badge("14 items")]), (26.0f32 * 6.9 + 82.0).round());
        assert_eq!(sub_width(&[item("Notepad", Do::Nothing)]), 220.0);
        assert_eq!(sub_width(&[item(&"x".repeat(60), Do::Nothing)]), 300.0);
        assert_eq!(sub_width(&[item("Visual Studio Code", Do::Nothing).badge("Default")]), ((18.0f32 + 8.0) * 6.9 + 70.0).round());
    }

    #[test]
    fn a_long_list_scrolls_above_80_percent_of_the_monitor() {
        let m = menu_with(full_rows(), full_rows(), 5);
        let g = Geo::of(&m, &m.full, 900.0);
        // 720 less the header, the tiles and the padding
        assert_eq!(g.view_h, 720.0 - 48.0 - 48.0 - 12.0);
        assert!(g.scrolls() && g.max_scroll() > 0.0);
        assert_eq!(g.h, 6.0 + 48.0 + 48.0 + g.view_h + 6.0);
        let no_tiles = menu_with(full_rows(), full_rows(), 0);
        assert_eq!(Geo::of(&no_tiles, &no_tiles.full, 900.0).list_y, 54.0, "no tiles: no 48 under the header");
    }

    #[test]
    fn the_card_flips_left_and_up_by_the_mockups_rules() {
        let room = (0, 0, 1440, 852);
        let size = (263, 423);
        assert_eq!(place((400, 300), size, room, 8), ((400, 300), (false, false)));
        // the right edge: 8 of margin; the bottom: 8 above the bar
        assert!(!place((1169, 300), size, room, 8).1.0);
        assert_eq!(place((1170, 300), size, room, 8), ((907, 300), (true, false)));
        assert_eq!(place((96, 600), size, room, 8), ((96, 177), (false, true)), "state 1: the card ends at the pointer");
        assert_eq!(place((1392, 812), size, room, 8), ((1129, 389), (true, true)), "state 8: the corner");
        // taller than the room above the pointer: kept 8 from the top and then from the bottom
        let ((_, y), (_, up)) = place((50, 100), (200, 900), room, 8);
        assert!(up && y == 852 - 8 - 900, "{y}");
        // a monitor that is not at (0, 0)
        assert_eq!(place((-1000, 100), size, (-1920, 0, 0, 1080), 8).0, (-1000, 100));
        assert_eq!(place((-200, 100), size, (-1920, 0, 0, 1080), 8).0, (-463, 100));
    }

    #[test]
    fn a_submenu_stands_beside_the_menu_flips_and_stays_in_the_room() {
        let card = Rect::new(100.0, 100.0, 263.0, 423.0);
        let room = (0.0, 0.0, 1440.0, 852.0);
        let (r, flip) = sub_place(card, 200.0, (220.0, 200.0), room);
        assert_eq!((r.x, r.y, flip), (359.0, 194.0, false), "4 under the menu's edge, 6 over its row");
        let (r, flip) = sub_place(Rect::new(1100.0, 100.0, 263.0, 423.0), 200.0, (220.0, 200.0), room);
        assert!(flip && r.x == 1100.0 - 220.0 + 4.0);
        let (r, _) = sub_place(card, 700.0, (220.0, 300.0), room);
        assert_eq!(r.y + r.h, 852.0 - 8.0, "kept 8 above the bottom");
        let (r, _) = sub_place(card, 10.0, (220.0, 300.0), room);
        assert_eq!(r.y, 8.0);
    }

    #[test]
    fn the_box_covers_both_cards_and_each_card_stands_where_its_own_rules_put_it() {
        let (p, ..) = panel();
        let s = &p.stage;
        // the compact card ends at the pointer (up); the full one is taller than the room above it, so it stands 8 from the top
        let (a, b) = (s.cards[0], s.cards[1]);
        assert_eq!((s.at.0 as f32 + a.x, s.at.1 as f32 + a.y + a.h), (96.0, 600.0));
        assert_eq!(s.at.1 as f32 + b.y, 8.0);
        assert!(b.h > a.h && s.size.1 >= b.h && s.size.0 >= b.w);
        assert!(s.corner[0] == (false, true));
        // every card is inside the box
        for c in [a, b] {
            assert!(c.x >= 0.0 && c.y >= 0.0 && c.x + c.w <= s.size.0 + 0.01 && c.y + c.h <= s.size.1 + 0.01);
        }
    }

    #[test]
    fn a_pointer_finds_tiles_rows_the_submenu_and_a_click_beside_the_menu_is_a_close() {
        let (p, ..) = panel();
        let (c, g) = (p.card(false), p.geo(false));
        let q = p.quick_rects(g, 5);
        assert_eq!(p.hit(c.x + q[2].cx(), c.y + q[2].cy()), Some(QUICK + 2));
        let (y, h) = g.rows[2];
        assert_eq!(p.hit(c.x + 50.0, c.y + g.list_y + y + h / 2.0), Some(ROW + 2));
        assert_eq!(p.hit(c.x + 50.0, c.y + g.list_y + g.rows[3].0 + 4.0), None, "a rule is nothing");
        assert_eq!(p.hit(c.x + 50.0, c.y + 20.0), None, "the header is nothing");
        assert_eq!(p.hit(c.x - 3.0, c.y + 100.0), Some(BLANK), "beside the card, inside the box");
        // open Open with: its items are found and win over what is under them
        p.open_sub(2, false);
        let (sc, _, list, _) = p.sub_card().expect("the submenu");
        let (placed, _) = stack(&list);
        assert_eq!(p.hit(sc.x + 30.0, sc.y + SUB_PAD + placed[1].0 + 10.0), Some(SUB + 1));
        assert_eq!(p.hit(sc.x + 30.0, sc.y + SUB_PAD + placed[2].0 + 4.0), None, "a rule in the submenu");
    }

    #[test]
    fn a_click_on_a_tile_or_a_row_closes_the_menu_and_then_hands_the_choice_over() {
        let (mut p, picked, ..) = panel();
        let mut cx = Cx::default();
        p.click(QUICK + 4, Button::Left, &mut cx);
        assert!(cx.close && picked.borrow().is_empty(), "closes first");
        cx.run_for_test();
        assert_eq!(*picked.borrow(), vec![Do::Shell(5)]);
        for (id, want) in [(ROW, Do::Shell(1)), (ROW + 4, Do::Ours(Act::CopyPath)), (ROW + 9, Do::Shell(3))] {
            let (mut p, picked, ..) = panel();
            let mut cx = Cx::default();
            p.click(id, Button::Left, &mut cx);
            cx.run_for_test();
            assert_eq!(*picked.borrow(), vec![want], "{id}");
        }
        // the right button chooses nothing; a click beside the menu closes without a choice
        let (mut p, picked, ..) = panel();
        let mut cx = Cx::default();
        p.click(ROW, Button::Right, &mut cx);
        assert!(!cx.close && picked.borrow().is_empty());
        p.click(BLANK, Button::Right, &mut cx);
        assert!(cx.close);
        cx.run_for_test();
        assert!(picked.borrow().is_empty());
    }

    #[test]
    fn dropping_the_menu_tells_the_host_it_is_gone() {
        let (p, picked, ..) = panel();
        drop(p);
        assert_eq!(*picked.borrow(), vec![Do::Nothing]);
    }

    #[test]
    fn show_more_options_swaps_the_list_and_fewer_options_comes_back() {
        let (mut p, picked, ..) = panel();
        let mut cx = Cx::default();
        p.click(ROW + 8, Button::Left, &mut cx);
        assert!(!cx.close, "the menu stays open");
        assert!(p.st.borrow().more && p.st.borrow().foc.is_none());
        assert_eq!(p.cards(), vec![p.card(true)]);
        p.click(ROW, Button::Left, &mut cx); // "Fewer options"
        assert!(!p.st.borrow().more);
        assert!(picked.borrow().is_empty());
        // the whole list scrolls with the wheel and closes a submenu
        p.click(ROW + 8, Button::Left, &mut cx);
        let c = p.card(true);
        p.wheel(c.x + 40.0, c.y + 200.0, false);
        assert_eq!(p.st.borrow().scroll, 48.0);
        p.wheel(c.x + 40.0, c.y + 200.0, true);
        p.wheel(c.x + 40.0, c.y + 200.0, true);
        assert_eq!(p.st.borrow().scroll, 0.0, "not above the top");
        for _ in 0..40 {
            p.wheel(c.x + 40.0, c.y + 200.0, false);
        }
        assert_eq!(p.st.borrow().scroll, p.geo(true).max_scroll());
    }

    #[test]
    fn a_submenu_windows_fills_later_shows_a_spinner_asks_once_and_opens_when_it_comes() {
        let (p, _, asked, inbox) = panel();
        p.open_sub(6, false); // Send to: a handle, nothing yet
        assert_eq!(*asked.borrow(), vec![7]);
        assert_eq!(p.st.borrow().loading, Some((7, 6, false)));
        assert!(p.st.borrow().sub.is_none() && p.animating(0), "the spinner needs frames");
        p.open_sub(6, false);
        assert_eq!(*asked.borrow(), vec![7], "asked once");
        inbox.borrow_mut().push((7, vec![item("Bluetooth device", Do::Shell(30)), item("Documents", Do::Shell(32))]));
        p.take_inbox(100);
        assert!(p.st.borrow().loading.is_none());
        assert_eq!(p.st.borrow().sub, Some(Open { parent: 6, f: None }));
        assert!(!p.animating(0));
        // a later, longer answer replaces the list under an open submenu
        inbox.borrow_mut().push((7, vec![item("A", Do::Shell(1)), item("B", Do::Shell(2)), item("C", Do::Shell(3))]));
        p.take_inbox(200);
        assert_eq!(p.list(false, 6).unwrap().len(), 3);
        // an answer with nothing in it ends the wait and opens nothing
        let (p, _, _, inbox) = panel();
        p.open_sub(6, true);
        inbox.borrow_mut().push((7, vec![]));
        p.take_inbox(10);
        assert!(p.st.borrow().sub.is_none() && p.st.borrow().loading.is_none());
    }

    #[test]
    fn the_keyboard_walks_the_rows_opens_and_closes_submenus_and_runs_with_enter() {
        let (mut p, picked, ..) = panel();
        let mut cx = Cx::default();
        assert!(p.key(0x28, &mut cx)); // Down from nothing: the first
        assert_eq!(p.st.borrow().foc, Some(0));
        p.key(0x28, &mut cx);
        p.key(0x28, &mut cx);
        assert_eq!(p.st.borrow().foc, Some(2));
        p.key(0x27, &mut cx); // Right: Open with, its first item lit
        assert_eq!(p.st.borrow().sub, Some(Open { parent: 2, f: Some(0) }));
        p.key(0x28, &mut cx);
        p.key(0x28, &mut cx); // the rule is skipped: Choose another app
        assert_eq!(p.st.borrow().sub.unwrap().f, Some(3));
        p.key(0x28, &mut cx);
        assert_eq!(p.st.borrow().sub.unwrap().f, Some(0), "round");
        p.key(0x26, &mut cx);
        p.key(0x0D, &mut cx); // Enter on the last item
        assert!(cx.close);
        cx.run_for_test();
        assert_eq!(*picked.borrow(), vec![Do::Shell(12)]);
        // Left comes back to the row; Esc closes the submenu first and the menu second
        let (mut p, ..) = panel();
        let mut cx = Cx::default();
        p.focus(Some(2));
        p.key(0x27, &mut cx);
        p.key(0x25, &mut cx);
        assert!(p.st.borrow().sub.is_none() && p.st.borrow().foc == Some(2));
        p.key(0x27, &mut cx);
        assert!(p.escape(), "the submenu used Esc");
        assert!(!p.escape(), "the menu closes");
        // Up from nothing is the last item; Home and End
        let (mut p, ..) = panel();
        p.key(0x26, &mut cx);
        assert_eq!(p.st.borrow().foc, Some(9));
        p.key(0x24, &mut cx);
        assert_eq!(p.st.borrow().foc, Some(0));
        p.key(0x23, &mut cx);
        assert_eq!(p.st.borrow().foc, Some(9));
        // Enter on a row with a submenu opens it; Enter on a disabled one does nothing
        let (mut p, picked, ..) = panel();
        let mut cx = Cx::default();
        p.focus(Some(2));
        p.key(0x0D, &mut cx);
        assert!(p.st.borrow().sub.is_some() && !cx.close);
        let mut rows = text_rows();
        rows[1] = item("Edit", Do::Shell(2)).off();
        let hooks = Hooks { pick: Box::new(|_| {}), fill: Box::new(|_| {}) };
        let mut q = FileMenu::new(menu_with(rows, full_rows(), 5), env(), hooks, Rc::new(RefCell::new(Vec::new())));
        q.focus(Some(1));
        q.key(0x0D, &mut cx);
        assert!(!cx.close && picked.borrow().is_empty());
    }

    #[test]
    fn a_letter_jumps_to_the_next_label_that_starts_with_it_round_the_list() {
        let rows = text_rows();
        assert_eq!(jump(&rows, None, 'c'), Some(4), "Copy as path is the first with C");
        assert_eq!(jump(&rows, Some(4), 'c'), Some(5), "then Compress to ZIP");
        assert_eq!(jump(&rows, Some(5), 'c'), Some(4), "and round");
        assert_eq!(jump(&rows, Some(9), 'o'), Some(0));
        assert_eq!(jump(&rows, None, 'z'), None);
        assert_eq!(jump(&rows, Some(0), 'P'), Some(9), "either case");
        assert_eq!(step(&rows, Some(2), true), Some(4), "the rule is skipped");
        assert_eq!(step(&rows, Some(4), false), Some(2));
        assert_eq!(step(&[], None, true), None);
    }

    #[test]
    fn the_keys_a_row_names_are_chords_and_do_what_they_say_while_the_menu_is_open() {
        let k = |s: &[&str]| s.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        assert_eq!(chord(&k(&["Ctrl", "Shift", "C"])), Some(((true, true, false), 0x43)));
        assert_eq!(chord(&k(&["Alt", "Enter"])), Some(((false, false, true), 0x0D)));
        assert_eq!(chord(&k(&["Shift", "F10"])), Some(((false, true, false), 0x79)));
        assert_eq!(chord(&k(&["Ctrl", "Enter"])), Some(((true, false, false), 0x0D)));
        assert_eq!(chord(&k(&["Enter"])), None, "a plain Enter is the lit row's");
        assert_eq!(chord(&k(&["Ctrl", "Ctrl", "X"])), None);
        assert_eq!(chord(&k(&["Ctrl", "PgUp"])), None);
        assert_eq!(chord(&[]), None);
        let (mut p, picked, ..) = panel();
        let mut cx = Cx::default();
        assert!(p.key_with(0x43, (true, true, false), &mut cx), "Ctrl+Shift+C");
        cx.run_for_test();
        assert_eq!(*picked.borrow(), vec![Do::Ours(Act::CopyPath)]);
        let mut cx = Cx::default();
        assert!(p.key_with(0x0D, (false, false, true), &mut cx), "Alt+Enter");
        cx.run_for_test();
        assert_eq!(picked.borrow().last(), Some(&Do::Shell(3)), "Properties");
        // Shift+F10 opens the whole list; a chord nobody has is not taken
        let (mut p, ..) = panel();
        let mut cx = Cx::default();
        assert!(p.key_with(0x79, (false, true, false), &mut cx) && p.st.borrow().more && !cx.close);
        assert!(!p.key_with(0x5A, (true, false, false), &mut cx));
    }

    #[test]
    fn f2_renames_and_delete_deletes_from_the_tiles_when_the_menu_has_them() {
        let mut m = menu_with(text_rows(), full_rows(), 5);
        m.quick[2].act = Do::Ours(Act::Rename);
        let got = Rc::new(RefCell::new(Vec::new()));
        let g = Rc::clone(&got);
        let hooks = Hooks { pick: Box::new(move |d| g.borrow_mut().push(d.clone())), fill: Box::new(|_| {}) };
        let mut p = FileMenu::new(m, env(), hooks, Rc::new(RefCell::new(Vec::new())));
        let mut cx = Cx::default();
        assert!(p.key(0x71, &mut cx));
        cx.run_for_test();
        let mut cx = Cx::default();
        p.key(0x2E, &mut cx); // Delete: the danger tile
        cx.run_for_test();
        assert_eq!(*got.borrow(), vec![Do::Ours(Act::Rename), Do::Shell(5)]);
    }

    /// A frame at `now` with the pointer on `hover`, as the host makes one.
    fn frame<'a>(mo: &'a mut super::super::panel::Motion, now: u64, hover: Option<i32>) -> Frame<'a> {
        Frame { now, age: now, leaving: 0, hover, press: None, size: (300.0, 400.0), opacity: 1.0, mo }
    }

    #[test]
    fn hover_opens_a_submenu_after_170_ms_and_another_row_closes_it_after_220() {
        let (p, ..) = panel();
        let mut mo = super::super::panel::Motion::default();
        // the pointer comes to Open with at t = 1000: lit at once, the submenu not yet
        p.sync(&mut frame(&mut mo, 1000, Some(ROW + 2)));
        assert_eq!(p.st.borrow().foc, Some(2));
        assert!(p.st.borrow().sub.is_none());
        assert_eq!(mo.due, Some(1170), "a frame is asked for when it is due");
        p.sync(&mut frame(&mut mo, 1169, Some(ROW + 2)));
        assert!(p.st.borrow().sub.is_none());
        p.sync(&mut frame(&mut mo, 1170, Some(ROW + 2)));
        assert_eq!(p.st.borrow().sub, Some(Open { parent: 2, f: None }));
        // the pointer goes into the submenu: it stays, its item is lit
        p.sync(&mut frame(&mut mo, 1300, Some(SUB + 1)));
        assert_eq!(p.st.borrow().sub, Some(Open { parent: 2, f: Some(1) }));
        // and on to Copy as path: the submenu goes 220 ms later
        p.sync(&mut frame(&mut mo, 2000, Some(ROW + 4)));
        assert!(p.st.borrow().sub.is_some() && p.st.borrow().foc == Some(4));
        p.sync(&mut frame(&mut mo, 2219, Some(ROW + 4)));
        assert!(p.st.borrow().sub.is_some());
        p.sync(&mut frame(&mut mo, 2220, Some(ROW + 4)));
        assert!(p.st.borrow().sub.is_none());
        // a tile shows its hint in the header and lights no row
        p.sync(&mut frame(&mut mo, 3000, Some(QUICK + 1)));
        assert_eq!((p.st.borrow().tile, p.st.borrow().foc), (Some(1), None));
        // the pointer passing over a row and away again before 170 ms opens nothing
        p.sync(&mut frame(&mut mo, 4000, Some(ROW + 2)));
        p.sync(&mut frame(&mut mo, 4100, Some(ROW + 4)));
        p.sync(&mut frame(&mut mo, 4300, Some(ROW + 4)));
        assert!(p.st.borrow().sub.is_none());
    }

    #[test]
    fn the_whole_lists_rows_and_a_third_level_never_reach_the_panel_as_a_submenu_of_a_submenu() {
        // a submenu's own rows are drawn without an arrow of their own: the panel only ever opens one level
        let (p, ..) = panel();
        p.open_sub(2, false);
        let list = p.list(false, 2).unwrap();
        assert!(list.iter().all(|r| r.sub.is_none()));
        let (c, g) = (p.card(false), p.geo(false));
        // a row of the menu answers while the submenu is open
        assert_eq!(p.hit(c.x + 40.0, c.y + g.list_y + g.rows[4].0 + 10.0), Some(ROW + 4));
    }
}
