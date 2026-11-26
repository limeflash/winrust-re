//! Task view and the Alt+Tab switcher: the windows of the desktop as live pictures with their names under them.
//!
//! Task view is the whole screen: a scrim (`rgba(5,7,12,.5)`, the screen under it blurred 18 and saturated 1.2) with the
//! desktops in a row at the top (192 × 108 tiles, radius 12, 16 apart, the current one in an accent ring) and the windows
//! below: each flies from where it is to its place in the grid (`transform .5s`), a name and a close cross under and over it.
//! The windows are holes in the scrim: DWM shows each one live through its hole (see [`super::live`]). The switcher puts
//! the same cards in a glass box in the middle of the screen. Neither is shown by this file alone: the owner opens them.

use std::rc::Rc;

use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
use winrust_common::style::pal;

use super::anim::{EASE, SNAP, SPRING};
use super::gfx::{Align, Canvas, Font, Gfx, Picture, Rect, Rgba};
use super::menu::{DIM, INK, MUTED, cap, cap_width};
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Exit, Frame, Hole, Id, Look, Origin, Place, Pose, Scrim, Shadow};

/// A Windows icon the panel owns for as long as it lives.
pub struct OwnedIcon(pub HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: the icon was created for this panel alone (CopyIcon / the shell's own copy).
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}

/// One window of the desktop.
pub struct WinCard {
    pub hwnd: isize,
    pub title: String,
    pub icon: Rc<OwnedIcon>,
    /// Its visible frame on the screen, device pixels.
    pub frame: (i32, i32, i32, i32),
    /// A minimized window has nothing to show live: its card has the icon.
    pub minimized: bool,
}

impl WinCard {
    fn size(&self) -> (f32, f32) {
        (((self.frame.2 - self.frame.0).max(1)) as f32, ((self.frame.3 - self.frame.1).max(1)) as f32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Window(isize),
    Close(isize),
    Desktop(usize),
    NewDesktop,
    /// The switcher's scope button: this monitor's windows or all of them.
    Scope,
}

/// One virtual desktop.
pub struct Desk {
    pub name: String,
    pub current: bool,
    /// The wallpaper file behind its tile.
    pub wallpaper: String,
    /// The windows on it, back to front: the window (0 = none to show live) and its rectangle in fractions of the screen (left,
    /// top, width, height).
    pub wins: Vec<(isize, (f32, f32, f32, f32))>,
}

/// A window's miniature in a desktop's tile.
fn mini(tile: Rect, (x, y, w, h): (f32, f32, f32, f32)) -> Rect {
    Rect::new((tile.x + x * tile.w).round(), (tile.y + y * tile.h).round(), (w * tile.w).max(6.0).round(), (h * tile.h).max(4.0).round())
}

const CLOSE_BASE: i32 = 100;
const DESK_BASE: i32 = 200;
const NEW_DESK: i32 = 300;
const SCRIM: i32 = 400;

const CROSS: &str = "M7 7l10 10M17 7L7 17";
const PLUS: &str = "M12 5v14M5 12h14";

/// `n` cells of `cw` × `ch` in rows that fit `aw` × `ah`: the rows are balanced, a short last row is centred, and the cells
/// shrink (down to a third) when there are too many. The cells from (0, 0), and the total width and height.
#[must_use]
pub fn grid(n: usize, aw: f32, ah: f32, cw: f32, ch: f32, gap: f32) -> (Vec<Rect>, f32, f32) {
    if n == 0 {
        return (Vec::new(), 0.0, 0.0);
    }
    let mut s = 1.0f32;
    let (w, h, rows, cols) = loop {
        let (w, h) = ((cw * s).floor(), (ch * s).floor());
        let fit = (((aw + gap) / (w + gap)).floor() as usize).max(1);
        let rows = n.div_ceil(fit);
        if rows as f32 * (h + gap) - gap <= ah || s < 0.35 {
            break (w, h, rows, n.div_ceil(rows));
        }
        s *= 0.9;
    };
    let (tw, th) = (cols as f32 * (w + gap) - gap, rows as f32 * (h + gap) - gap);
    let mut out = Vec::with_capacity(n);
    for r in 0..rows {
        let count = cols.min(n - r * cols);
        let x0 = ((tw - (count as f32 * (w + gap) - gap)) / 2.0).floor();
        for c in 0..count {
            out.push(Rect::new(x0 + c as f32 * (w + gap), r as f32 * (h + gap), w, h));
        }
    }
    (out, tw, th)
}

/// The largest rectangle of `src`'s shape that fits in `cell` (no more than `cap` times the size of `src`), in its middle.
fn fit(src: (f32, f32), cell: Rect, cap: f32) -> Rect {
    let k = (cell.w / src.0).min(cell.h / src.1).min(cap);
    let (w, h) = ((src.0 * k).floor().max(1.0), (src.1 * k).floor().max(1.0));
    Rect::new((cell.cx() - w / 2.0).round(), (cell.cy() - h / 2.0).round(), w, h)
}

/// Pack windows at one scale into centred rows; bottom-align pictures so each row's labels share a baseline.
fn task_grid(sizes: &[(f32, f32)], area: Rect) -> Vec<Rect> {
    if sizes.is_empty() { return Vec::new(); }
    let gap = 40.0;
    let label = 34.0;
    let rows_at = |scale: f32| {
        let mut rows: Vec<(usize, usize, f32, f32)> = Vec::new();
        let (mut start, mut width, mut height) = (0, 0.0f32, 0.0f32);
        for (i, &(w, h)) in sizes.iter().enumerate() {
            let (w, h) = (w.max(1.0) * scale, h.max(1.0) * scale);
            if i > start && width + gap + w > area.w {
                rows.push((start, i, width, height));
                (start, width, height) = (i, 0.0, 0.0);
            }
            width += if i > start { gap + w } else { w };
            height = height.max(h);
        }
        rows.push((start, sizes.len(), width, height));
        rows
    };
    // Greedy wrapping changes at the width thresholds of contiguous runs. Examine each threshold so that a change in
    // row composition cannot fool a binary search over the total block height.
    let mut candidates = vec![0.5f32];
    for start in 0..sizes.len() {
        let mut width = 0.0;
        for (count, &(w, _)) in sizes[start..].iter().enumerate() {
            width += w.max(1.0);
            let s = (area.w - gap * count as f32) / width;
            if s > 0.0 && s < 0.5 {
                candidates.push(s);
                // Allow for rounding at a wrap boundary: the row just below it can be the largest feasible layout.
                candidates.push(s * (1.0 - 1e-6));
            }
        }
    }
    let mut best = 0.0f32;
    for upper in candidates {
        let rows = rows_at(upper);
        let overhead = rows.len() as f32 * label + (rows.len() - 1) as f32 * gap;
        let heights: f32 = rows.iter().map(|r| r.3 / upper).sum();
        let s = upper.min(((area.h - overhead) / heights).max(0.0));
        if s <= best { continue; }
        let actual = rows_at(s);
        let total = actual.iter().map(|r| r.3 + label).sum::<f32>() + (actual.len() - 1) as f32 * gap;
        if total <= area.h + 0.01 && actual.iter().all(|r| r.2 <= area.w + 0.01) { best = s; }
    }
    let rows = rows_at(best);
    let total_h = rows.iter().map(|r| r.3 + label).sum::<f32>() + (rows.len() - 1) as f32 * gap;
    let mut y = area.y + (area.h - total_h) / 2.0;
    let mut out = Vec::with_capacity(sizes.len());
    for (start, end, width, height) in rows {
        let mut x = area.x + (area.w - width) / 2.0;
        for &(w, h) in &sizes[start..end] {
            let (w, h) = (w.max(1.0) * best, h.max(1.0) * best);
            out.push(Rect::new(x, y + height - h, w, h));
            x += w + gap;
        }
        y += height + label + gap;
    }
    out
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Rect::new(l(a.x, b.x), l(a.y, b.y), l(a.w, b.w), l(a.h, b.h))
}

/// The cross a card has: `28 px` circle, `rgba(30,31,38,.95)`, a white ring at .12 and a shadow; red when pointed at.
fn draw_cross(c: &mut Canvas<'_>, at: Rect, hot: f32, opacity: f32) {
    c.faded(opacity, |c| {
        c.shadow(at, 14.0, 4.0, 12.0, Rgba::css(0, 0, 0, 0.5));
        c.fill(at, 14.0, Rgba::css(30, 31, 38, 0.95).mix(Rgba::css(229, 72, 77, 1.0), hot));
        c.ring(at, 14.0, 1.0, Rgba::white(0.12 * (1.0 - hot)));
        c.icon(CROSS, (at.cx(), at.cy()), 12.0, 2.6, INK, 0.0);
    });
}

/// The name of a window under its picture: its icon (20 px) and the title, centred on `cx`.
fn draw_label(c: &mut Canvas<'_>, card: &WinCard, cx: f32, y: f32, max_w: f32, font: Font) {
    let tw = c.measure(&card.title, font).ceil().min((max_w - 28.0).max(20.0));
    let w = 20.0 + 8.0 + tw;
    let x = cx - w / 2.0;
    c.hicon(card.icon.0, Rect::new(x, y, 20.0, 20.0), 1.0);
    c.text_fit(&card.title, Rect::new(x + 28.0, y, tw + 1.0, 20.0), font, INK, Align::Start);
}

/// A card that has no live picture: a plate with the icon.
fn draw_plate(c: &mut Canvas<'_>, card: &WinCard, r: Rect) {
    c.fill(r, 14.0, Rgba::css(29, 30, 36, 1.0));
    c.ring(r, 14.0, 1.0, Rgba::white(0.08));
    if let Some((picture, _, _)) = crate::snapshots::get(card.hwnd) {
        c.hicon_cover(picture.0, r, 14.0);
        c.forget_icon(picture.0);
    } else {
        let k = 48.0f32.min(r.w - 8.0).min(r.h - 8.0).max(16.0);
        c.hicon(card.icon.0, Rect::new(r.cx() - k / 2.0, r.cy() - k / 2.0, k, k), 1.0);
    }
}

/// Cuts the hole a live picture shows through; a hair of alpha keeps the pointer on the panel.
fn cut(c: &mut Canvas<'_>, r: Rect, radius: f32) {
    c.erase(r, radius);
    c.fill(r, radius, Rgba::new(0.0, 0.0, 0.0, 1.0 / 255.0));
}

// ---------------------------------------------------------------------------------------------------------------------
// Task view

pub struct TaskView {
    scale: f32,
    screen: (f32, f32),
    desks: Vec<Desk>,
    can_add: bool,
    cards: Vec<WinCard>,
    /// Where each window rests, DIPs.
    target: Vec<Rect>,
    from: Vec<Rect>,
    pick: Rc<dyn Fn(Pick)>,
    holes: Vec<Hole>,
    calm: bool,
    empty: String,
    /// The wallpapers of the tiles, loaded when the first frame is drawn.
    walls: Option<Vec<Option<Picture>>>,
}

impl TaskView {
    /// `screen` in device pixels. `calm`: it appears at once (laid out again after a window was closed).
    #[must_use]
    pub fn new(scale: f32, screen: (i32, i32), desks: Vec<Desk>, can_add: bool, cards: Vec<WinCard>, calm: bool, pick: impl Fn(Pick) + 'static) -> Self {
        let screen = (screen.0 as f32 / scale, screen.1 as f32 / scale);
        let (aw, top, bottom) = (screen.0 - 160.0, 190.0, screen.1 - 150.0);
        let sizes: Vec<(f32, f32)> = cards.iter().map(|c| { let s = c.size(); (s.0 / scale, s.1 / scale) }).collect();
        let target = task_grid(&sizes, Rect::new(80.0, top, aw.max(1.0), (bottom - top).max(1.0)));
        let from = cards
            .iter()
            .map(|c| Rect::new(c.frame.0 as f32 / scale, c.frame.1 as f32 / scale, (c.frame.2 - c.frame.0) as f32 / scale, (c.frame.3 - c.frame.1) as f32 / scale))
            .collect();
        let name = desks.iter().find(|d| d.current).map_or("", |d| d.name.as_str());
        let empty = winrust_common::tf!("Нет окон на {}", "No windows on {}", ja: "{} にウィンドウはありません", pt: "Nenhuma janela em {}", zh: "{} 上没有窗口", name);
        Self { scale, screen, desks, can_add, cards, target, from, pick: Rc::new(pick), holes: Vec::new(), calm, empty, walls: None }
    }

    /// Left edge of the row of tiles, the tiles and the "new desktop" one.
    fn strip(&self) -> (f32, Vec<Rect>, Option<Rect>) {
        let n = self.desks.len() as f32;
        let add = if self.can_add { 16.0 + 108.0 } else { 0.0 };
        let total = n * 192.0 + (n - 1.0).max(0.0) * 16.0 + add;
        let x0 = ((self.screen.0 - total) / 2.0).floor();
        let tiles = (0..self.desks.len()).map(|i| Rect::new(x0 + i as f32 * 208.0, 40.0, 192.0, 108.0)).collect();
        (x0, tiles, self.can_add.then(|| Rect::new(x0 + n * 208.0, 40.0, 108.0, 108.0)))
    }

    fn close_at(&self, i: usize) -> Rect {
        let t = self.target[i];
        Rect::new(t.x + t.w - 20.0, t.y - 8.0, 28.0, 28.0)
    }

    fn draw_tile(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>, i: usize, r: Rect, accent: Rgba) {
        let d = &self.desks[i];
        let hot = f.hover(DESK_BASE + i as i32, 200, SPRING);
        let r = r.shift(0.0, -2.0 * hot);
        if d.current {
            c.shadow(r, 12.0, 8.0, 24.0, Rgba::css(0, 0, 0, 0.45));
            c.ring(r.inset(-2.0), 14.0, 2.0, accent);
        } else {
            c.shadow(r, 12.0, 8.0, 24.0, Rgba::css(0, 0, 0, 0.35));
            c.ring(r.inset(-1.0), 13.0, 1.0, Rgba::white(0.12));
        }
        c.fill(r, 12.0, Rgba::css(11, 13, 20, 1.0));
        if let Some(Some(p)) = self.walls.as_ref().and_then(|w| w.get(i)) {
            c.picture_cover(p, r, 12.0, 1.0);
        }
        c.clipped(r, |c| {
            // the current desktop's windows are live pictures (cut in `draw`); the others' are drawn like the mockup's
            for &(_, m) in d.wins.iter().filter(|w| !(d.current && w.0 != 0)) {
                let m = mini(r, m);
                c.fill(m.inset(-0.5), 3.0, Rgba::css(0, 0, 0, 0.5));
                c.fill(m, 3.0, Rgba::css(29, 30, 36, 1.0));
                c.fill(Rect::new(m.x, m.y, m.w, (m.h * 0.12).clamp(2.0, 8.0)), 0.0, Rgba::css(38, 39, 47, 1.0));
            }
        });
        c.clipped(Rect::new(r.x, r.y + r.h - 7.0, r.w, 7.0), |c| c.fill(r, 12.0, Rgba::css(20, 21, 27, 0.9)));
        let ink = if d.current { INK } else { DIM };
        c.text_fit(&d.name, Rect::new(r.x, 40.0 + 108.0 + 8.0, r.w, 15.0), Font::new(12.0, 500), ink, Align::Center);
    }
}

impl Content for TaskView {
    fn id(&self) -> Id {
        Id::Task
    }

    fn look(&self) -> Look {
        Look {
            radius: 0.0,
            tint: Rgba::css(5, 7, 12, 0.5),
            blur: 18.0,
            saturate: 1.2,
            ring_out: Rgba::TRANSPARENT,
            ring_in: Rgba::TRANSPARENT,
            top_light: Rgba::TRANSPARENT,
            shadow: Shadow { dy: 0.0, blur: 0.0, color: Rgba::TRANSPARENT },
            scrim: None,
        }
    }

    fn entry(&self) -> Entry {
        // `tvIn`: opacity .35 s, `cubic-bezier(.22,1,.36,1)`
        let dur = if self.calm { 0 } else { 350 };
        Entry { from: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 1.0, clip: None }, dur, ease: SPRING, origin: Origin::Frac(0.5, 0.5) }
    }

    fn exit(&self) -> Exit {
        // `tvOut`: opacity .3 s ease
        Exit { to: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 1.0, clip: None }, dur: 300, ease: EASE, origin: Origin::Frac(0.5, 0.5) }
    }

    fn place(&self) -> Place {
        Place { along: Along::Abs(0, 0), gap: 0.0 }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        self.screen
    }

    fn settle(&self) -> u32 {
        900
    }

    fn holes(&self) -> Vec<Hole> {
        self.holes.clone()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let _ = self.scale;
        // how far the windows have flown from where they were to their places (`.5s` spring); back again on the way out
        let arrive = if self.calm { 1.0 } else { f.rise(0, 500, SPRING, 0.0, 1.0).opacity };
        let back = if f.leaving > 0 { 1.0 - SPRING.ease((f.leaving as f32 / 300.0).min(1.0)) } else { 1.0 };
        let p = arrive * back;
        // `.tvIn .tvs`: .45 s spring after .08 s, from 12 px above
        let strip = if self.calm { f.rise(0, 0, SPRING, 0.0, 1.0) } else { f.rise(80, 450, SPRING, -12.0, 1.0) };
        let cross = if self.calm { 1.0 } else { f.rise(300, 300, EASE, 0.0, 1.0).opacity };
        self.holes.clear();

        if self.walls.is_none() {
            let walls = self.desks.iter().map(|d| c.image(&d.wallpaper, 192.0, 108.0)).collect();
            self.walls = Some(walls);
        }
        let (_, tiles, add) = self.strip();
        c.faded(strip.opacity, |c| {
            c.transformed(self.screen.0 / 2.0, 40.0, 1.0, 0.0, strip.dy, |c| {
                for (i, r) in tiles.iter().enumerate() {
                    self.draw_tile(c, f, i, *r, accent);
                }
                if let Some(r) = add {
                    let hot = f.hover(NEW_DESK, 200, SPRING);
                    let r = r.shift(0.0, -2.0 * hot);
                    c.fill(r, 12.0, Rgba::white(0.06));
                    c.ring(r, 12.0, 1.0, Rgba::white(0.1));
                    c.icon(PLUS, (r.cx(), r.cy()), 22.0, 2.0, DIM, 0.0);
                    let label = winrust_common::t!("Новый рабочий стол", "New desktop", ja: "新しいデスクトップ", pt: "Nova área de trabalho", zh: "新建桌面");
                    c.text_fit(label, Rect::new(r.x - 20.0, 40.0 + 108.0 + 8.0, r.w + 40.0, 15.0), Font::new(12.0, 500), DIM, Align::Center);
                }
            });
        });
        // the current desktop's windows show live in its tile, back to front (the later thumbnail is drawn on top), cut out of
        // the tile's round corners and kept under its bottom edge; outside the strip's fade, which a cut could not go through
        for (i, t) in tiles.iter().enumerate().filter(|(i, _)| self.desks[*i].current) {
            let t = t.shift(0.0, strip.dy - 2.0 * f.hover(DESK_BASE + i as i32, 200, SPRING));
            for &(win, m) in self.desks[i].wins.iter().filter(|w| w.0 != 0) {
                let m = mini(t, m);
                c.clipped(Rect::new(t.x, t.y, t.w, t.h - 7.0), |c| c.erase_within(m, 3.0, t, 12.0, Rgba::new(0.0, 0.0, 0.0, 1.0 / 255.0)));
                self.holes.push(Hole { win, rect: m, opacity: strip.opacity });
            }
        }

        if self.cards.is_empty() {
            c.faded(strip.opacity, |c| c.text_fit(&self.empty, Rect::new(0.0, 430.0 - 10.0, self.screen.0, 20.0), Font::new(15.0, 400), DIM, Align::Center));
        }

        for (i, (card, &to)) in self.cards.iter().zip(&self.target).enumerate() {
            let r = if card.minimized { to } else { lerp_rect(self.from[i], to, p) };
            let hot = f.hover(i as i32, 200, EASE);
            if card.minimized {
                c.faded(strip.opacity, |c| draw_plate(c, card, to));
            } else {
                cut(c, r, 14.0);
                self.holes.push(Hole { win: card.hwnd, rect: r, opacity: 1.0 });
            }
            // `.tvhit:hover { box-shadow: inset 0 0 0 6px accent }`
            c.ring(r, 14.0, 6.0, accent.alpha(hot));
            let label = strip.opacity;
            c.faded(label, |c| {
                c.transformed(to.cx(), to.y + to.h, 1.0, 0.0, strip.dy, |c| draw_label(c, card, to.cx(), to.y + to.h + 14.0, to.w.max(160.0), Font::new(13.0, 500)));
            });
            draw_cross(c, Rect::new(r.x + r.w - 20.0, r.y - 8.0, 28.0, 28.0), f.hover(CLOSE_BASE + i as i32, 160, EASE), cross * (0.9 + 0.1 * f.hover(CLOSE_BASE + i as i32, 160, EASE)));
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        for i in 0..self.cards.len() {
            if self.close_at(i).contains(x, y) {
                return Some(CLOSE_BASE + i as i32);
            }
        }
        if let Some(i) = self.target.iter().position(|r| r.contains(x, y)) {
            return Some(i as i32);
        }
        let (_, tiles, add) = self.strip();
        if let Some(i) = tiles.iter().position(|r| r.contains(x, y)) {
            return Some(DESK_BASE + i as i32);
        }
        if add.is_some_and(|r| r.contains(x, y)) {
            return Some(NEW_DESK);
        }
        Some(SCRIM)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        let pick = Rc::clone(&self.pick);
        match id {
            SCRIM => cx.close(),
            NEW_DESK => cx.close_then(move || pick(Pick::NewDesktop)),
            DESK_BASE..NEW_DESK => {
                let i = (id - DESK_BASE) as usize;
                if self.desks.get(i).is_some_and(|d| d.current) {
                    cx.close();
                } else {
                    cx.close_then(move || pick(Pick::Desktop(i)));
                }
            }
            CLOSE_BASE..DESK_BASE => {
                let Some(w) = self.cards.get((id - CLOSE_BASE) as usize).map(|c| c.hwnd) else { return };
                cx.run(move || pick(Pick::Close(w)));
            }
            _ => {
                let Some(w) = self.cards.get(id as usize).map(|c| c.hwnd) else { return };
                if b == Button::Middle {
                    cx.run(move || pick(Pick::Close(w)));
                } else {
                    cx.close_then(move || pick(Pick::Window(w)));
                }
            }
        }
    }

    fn nav(&self, from: Option<i32>, dir: Dir) -> Option<i32> {
        let n = self.cards.len() as i32;
        let cur = from.filter(|f| *f < CLOSE_BASE);
        let i = match (cur, dir) {
            (_, Dir::First) | (None, Dir::Down | Dir::Right) => 0,
            (_, Dir::Last) | (None, Dir::Up | Dir::Left) => n - 1,
            (Some(i), Dir::Up | Dir::Left) => i - 1,
            (Some(i), Dir::Down | Dir::Right) => i + 1,
        };
        (n > 0).then(|| i.clamp(0, n - 1))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the switcher

/// What the switcher asks its owner: which card is selected now (the keyboard moves it), and "the pointer is on card `i`".
pub struct Switch {
    pub selected: fn() -> usize,
    pub point: fn(usize),
    pub pick: fn(Pick),
}

const SCOPE: i32 = 500;
/// `.atp`: the padding (14 14 10), the gap of the cards, `max-width: 1380px`, and the widest card.
const SW_PAD: f32 = 14.0;
const SW_PAD_BOTTOM: f32 = 10.0;
const SW_GAP: f32 = 6.0;
const SW_MAX: f32 = 1380.0;
const SW_CARD: f32 = 228.0;
/// The mockup keeps every card in one row; below this width the cards wrap into more rows instead of shrinking away.
const SW_CARD_MIN: f32 = 140.0;
const SW_BAR: f32 = 26.0;
const MONITOR: &str = "M3 5h18v11H3zM8 20h8M12 16v4";
const MONITORS: &str = "M2 6h12v9H2zM5 19h6M8 15v4M16 9h6v8h-6";
/// `#15161b`, what stands behind a picture.
const THUMB: Rgba = Rgba::css(21, 22, 27, 1.0);
const SUB: Rgba = Rgba::css(201, 205, 216, 1.0);

pub struct Switcher {
    scale: f32,
    /// The monitor it opens on (left, top, right, bottom), device pixels.
    mon: (i32, i32, i32, i32),
    /// The list holds this monitor's windows only (the label says so, and the button flips it).
    per_monitor: bool,
    cards: Vec<WinCard>,
    lay: SwLayout,
    sw: Switch,
    holes: Vec<Hole>,
}

#[derive(Default)]
struct SwLayout {
    size: (f32, f32),
    at: (i32, i32),
    cells: Vec<Rect>,
    scope: Rect,
    /// Where the hints end.
    hints_right: f32,
    bar_y: f32,
}

/// The keys the bar under the cards names, with what each does.
fn hints() -> [(&'static str, &'static str); 4] {
    [
        ("Tab", t!("вперёд", "next", ja: "次へ", pt: "seguinte", zh: "下一个")),
        ("Shift", t!("назад", "back", ja: "戻る", pt: "voltar", zh: "上一个")),
        ("Q", t!("закрыть", "close", ja: "閉じる", pt: "fechar", zh: "关闭")),
        ("Esc", t!("отмена", "cancel", ja: "キャンセル", pt: "cancelar", zh: "取消")),
    ]
}

/// "This monitor · 3 windows" (the scope of the list and how many windows are in it).
fn scope_label(per_monitor: bool, n: usize) -> String {
    let what = if per_monitor {
        t!("Этот монитор", "This monitor", ja: "このモニター", pt: "Este monitor", zh: "此显示器")
    } else {
        t!("Все мониторы", "All monitors", ja: "すべてのモニター", pt: "Todos os monitores", zh: "所有显示器")
    };
    let windows = match winrust_common::lang!() {
        winrust_common::Lang::Ru => format!("{n} {}", if n % 10 == 1 && n % 100 != 11 { "окно" } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) { "окна" } else { "окон" }),
        winrust_common::Lang::Ja => format!("{n} ウィンドウ"),
        winrust_common::Lang::Pt => format!("{n} {}", if n == 1 { "janela" } else { "janelas" }),
        winrust_common::Lang::Zh => format!("{n} 个窗口"),
        winrust_common::Lang::En => format!("{n} {}", if n == 1 { "window" } else { "windows" }),
    };
    format!("{what} \u{b7} {windows}")
}

impl Switcher {
    /// `screen` (left, top, right, bottom) is the monitor it opens on, in device pixels. `per_monitor`: the cards are that
    /// monitor's windows only.
    #[must_use]
    pub fn new(scale: f32, screen: (i32, i32, i32, i32), cards: Vec<WinCard>, per_monitor: bool, sw: Switch) -> Self {
        Self { scale, mon: screen, per_monitor, cards, lay: SwLayout::default(), sw, holes: Vec::new() }
    }

    /// The box and where the cards stand in it: one row of up to 228 px cards, `.62` as high as wide, shrinking to fit
    /// `max-width: 1380px`; a row that would make them narrower than [`SW_CARD_MIN`] wraps instead.
    fn layout(&mut self, g: &mut Gfx) {
        let s = self.scale;
        let (mw, mh) = ((self.mon.2 - self.mon.0) as f32 / s, (self.mon.3 - self.mon.1) as f32 / s);
        let n = self.cards.len().max(1);
        let inner_max = (SW_MAX.min(mw - 40.0) - 2.0 * SW_PAD).max(SW_CARD_MIN);
        let fit = (((inner_max + SW_GAP) / (SW_CARD_MIN + SW_GAP)).floor() as usize).max(1);
        let rows = n.div_ceil(fit);
        let cols = n.div_ceil(rows);
        let cw = SW_CARD.min(((inner_max - (cols - 1) as f32 * SW_GAP) / cols as f32).floor());
        let th = (cw * 0.62).round();
        let ch = th + 50.0;
        let cards_w = cols as f32 * cw + (cols - 1) as f32 * SW_GAP;
        // the bar under them: the scope button and the hints, 16 apart, with `padding: 0 6px`
        let label = scope_label(self.per_monitor, self.cards.len());
        let scope_w = 9.0 + 14.0 + 7.0 + g.measure(&label, Font::new(12.0, 400)).ceil() + 9.0;
        let hints_w: f32 = hints()
            .iter()
            .map(|(k, v)| g.measure(k, Font::new(11.0, 500)).ceil() + 12.0 + 3.0 + 3.4 + g.measure(v, Font::new(12.0, 400)).ceil())
            .sum::<f32>()
            + 12.0 * 3.0;
        let inner = cards_w.max(scope_w + 22.0 + hints_w);
        let size = ((inner + 2.0 * SW_PAD).ceil(), SW_PAD + rows as f32 * ch + (rows - 1) as f32 * SW_GAP + 10.0 + SW_BAR + SW_PAD_BOTTOM);
        let bar_y = size.1 - SW_PAD_BOTTOM - SW_BAR;
        self.lay = SwLayout {
            size,
            at: (self.mon.0 + ((mw - size.0) / 2.0 * s).round() as i32, self.mon.1 + (((mh * 0.46 - size.1 / 2.0).max(8.0)) * s).round() as i32),
            cells: (0..self.cards.len()).map(|i| Rect::new(SW_PAD + (i % cols) as f32 * (cw + SW_GAP), SW_PAD + (i / cols) as f32 * (ch + SW_GAP), cw, ch)).collect(),
            scope: Rect::new(SW_PAD, bar_y, scope_w, SW_BAR),
            hints_right: SW_PAD + inner - 6.0,
            bar_y,
        };
    }

    /// The frame of a card's picture: `padding 8`, a 24 px header and 8 below it.
    fn thumb_of(cell: Rect) -> Rect {
        Rect::new(cell.x + 8.0, cell.y + 40.0, cell.w - 16.0, cell.h - 50.0)
    }

    fn close_at(&self, i: usize) -> Rect {
        let c = self.lay.cells[i];
        Rect::new(c.x + c.w - 10.0 - 22.0, c.y + 9.0, 22.0, 22.0)
    }
}

impl Content for Switcher {
    fn id(&self) -> Id {
        Id::Switch
    }

    /// `.atp`: `rgba(18,19,26,.74)`, blur 50, radius 26, `0 40px 120px rgba(0,0,0,.6)`, over a scrim of `rgba(5,7,12,.32)`.
    fn look(&self) -> Look {
        let mut look = Look::glass(26.0, 0.74, Shadow { dy: 40.0, blur: 120.0, color: Rgba::css(0, 0, 0, 0.6) });
        look.tint = Rgba::css(18, 19, 26, 0.74);
        look.blur = 50.0;
        look.dimming(Scrim { color: Rgba::css(5, 7, 12, 0.32), fade_in: 160, fade_out: 140 })
    }

    /// `atpIn`: from `opacity 0, scale(.96)` in 220 ms.
    fn entry(&self) -> Entry {
        Entry { from: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 0.96, clip: None }, dur: 220, ease: SNAP, origin: Origin::Frac(0.5, 0.5) }
    }

    /// `atOut`: 140 ms of fading.
    fn exit(&self) -> Exit {
        Exit { to: Pose { opacity: 0.0, dx: 0.0, dy: 0.0, k: 1.0, clip: None }, dur: 140, ease: EASE, origin: Origin::Frac(0.5, 0.5) }
    }

    fn place(&self) -> Place {
        Place { along: Along::Abs(self.lay.at.0, self.lay.at.1), gap: 0.0 }
    }

    fn size(&mut self, g: &mut Gfx) -> (f32, f32) {
        self.layout(g);
        self.lay.size
    }

    fn activates(&self) -> bool {
        false
    }

    fn holes(&self) -> Vec<Hole> {
        self.holes.clone()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let accent = Rgba::colorref(pal().accent);
        let sel = (self.sw.selected)();
        self.holes.clear();
        if let Some(h) = f.hover.filter(|h| *h < CLOSE_BASE || (CLOSE_BASE..DESK_BASE).contains(h)) {
            (self.sw.point)((h % CLOSE_BASE) as usize);
        }
        for i in 0..self.cards.len() {
            let (card, cell) = (&self.cards[i], self.lay.cells[i]);
            let on = f.tween(i as i32, 1, f32::from(i == sel), 160, EASE);
            // the card: a plate and a ring of the accent when it is the chosen one (`.atc`, radius 18)
            c.fill(cell, 18.0, Rgba::white(0.1 * on));
            c.ring(cell, 18.0, 2.0, accent.alpha(on));
            // the header: the icon, the name, the cross
            c.hicon(card.icon.0, Rect::new(cell.x + 10.0, cell.y + 11.0, 18.0, 18.0), 1.0);
            let cross = self.close_at(i);
            let name = Rect::new(cell.x + 36.0, cell.y + 8.0, cross.x - 8.0 - (cell.x + 36.0), 24.0);
            c.text_fit(&card.title, name, Font::new(12.5, 500), SUB.mix(Rgba::white(1.0), on), Align::Start);
            let shown = on.max(f.hover(i as i32, 140, EASE));
            if shown > 0.01 {
                let hot = f.hover(CLOSE_BASE + i as i32, 140, EASE);
                c.faded(shown, |c| {
                    c.transformed(cross.cx(), cross.cy(), 0.8 + 0.2 * shown, 0.0, 0.0, |c| {
                        c.fill(cross, 11.0, Rgba::white(0.1).mix(Rgba::css(229, 72, 77, 0.85), hot));
                        c.icon(CROSS, (cross.cx(), cross.cy()), 10.0, 3.0, INK, 0.0);
                    });
                });
            }
            // the picture: a frame with a hairline, the live window through a hole in it, or the icon of a minimized one
            let frame = Self::thumb_of(cell);
            c.fill(frame, 11.0, THUMB);
            c.ring(frame, 11.0, 1.0, Rgba::white(0.07));
            if card.minimized {
                if let Some((picture, _, _)) = crate::snapshots::get(card.hwnd) {
                    c.hicon_cover(picture.0, frame, 11.0);
                    c.forget_icon(picture.0);
                } else {
                    let k = 46.0;
                    let at = Rect::new(frame.cx() - k / 2.0, frame.cy() - k / 2.0, k, k);
                    c.icon_shadow(card.icon.0, at, 8.0, 18.0, Rgba::css(0, 0, 0, 0.45));
                    c.hicon(card.icon.0, at, 1.0);
                }
                let word = t!("Свёрнуто", "Minimized", ja: "最小化", pt: "Minimizada", zh: "已最小化");
                let font = Font::new(11.0, 500);
                let w = c.measure(word, font).ceil() + 16.0;
                let pill = Rect::new(frame.x + 8.0, frame.y + frame.h - 8.0 - 20.0, w, 20.0);
                c.fill(pill, 10.0, Rgba::css(10, 11, 16, 0.72));
                c.text(word, pill, font, SUB, Align::Center);
            } else {
                let size = card.size();
                let r = fit((size.0 / self.scale, size.1 / self.scale), frame, 1.0);
                cut(c, r, if r.w >= frame.w - 1.0 && r.h >= frame.h - 1.0 { 11.0 } else { 4.0 });
                self.holes.push(Hole { win: card.hwnd, rect: r, opacity: f.opacity });
            }
        }

        // the bar under the cards: the scope of the list, and the keys
        let lay = &self.lay;
        let hot = f.hover(SCOPE, 140, SPRING);
        c.fill(lay.scope, 8.0, Rgba::white(0.09 * hot));
        c.icon(if self.per_monitor { MONITOR } else { MONITORS }, (lay.scope.x + 9.0 + 7.0, lay.scope.cy()), 14.0, 1.9, DIM, 0.0);
        c.text(&scope_label(self.per_monitor, self.cards.len()), Rect::new(lay.scope.x + 9.0 + 14.0 + 7.0, lay.scope.y, lay.scope.w, lay.scope.h), Font::new(12.0, 400), DIM, Align::Start);
        let widths: Vec<(f32, f32)> = hints().iter().map(|(k, v)| (cap_width(c, k), c.measure(v, Font::new(12.0, 400)).ceil())).collect();
        let total: f32 = widths.iter().map(|(k, v)| k + 3.0 + 3.4 + v).sum::<f32>() + 12.0 * 3.0;
        let (mut x, cy) = (lay.hints_right - total, lay.bar_y + SW_BAR / 2.0);
        for ((key, label), (kw, lw)) in hints().into_iter().zip(widths) {
            cap(c, x, cy, key, SUB, 0.08);
            x += kw + 3.0 + 3.4;
            c.text(label, Rect::new(x, cy - 10.0, lw + 2.0, 20.0), Font::new(12.0, 400), MUTED, Align::Start);
            x += lw + 12.0;
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        let sel = (self.sw.selected)();
        if self.cards.get(sel).is_some() && self.close_at(sel).contains(x, y) {
            return Some(CLOSE_BASE + sel as i32);
        }
        if self.lay.scope.contains(x, y) {
            return Some(SCOPE);
        }
        self.lay.cells.iter().position(|r| r.contains(x, y)).map(|i| i as i32)
    }

    fn click(&mut self, id: i32, b: Button, cx: &mut Cx) {
        let pick = self.sw.pick;
        if id == SCOPE {
            return cx.run(move || pick(Pick::Scope));
        }
        let i = (id % CLOSE_BASE) as usize;
        let Some(w) = self.cards.get(i).map(|c| c.hwnd) else { return };
        if id >= CLOSE_BASE || b == Button::Middle {
            cx.run(move || pick(Pick::Close(w)));
        } else {
            cx.run(move || pick(Pick::Window(w)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::grid;

    #[test]
    fn grid_balances_rows_and_centers_the_last_one() {
        // 5 cards of 100 in 350: at most 3 fit per row -> 2 rows of 3 + 2, the 2 centred under the 3
        let (r, tw, th) = grid(5, 350.0, 1000.0, 100.0, 50.0, 10.0);
        assert_eq!((tw, th), (320.0, 110.0));
        assert_eq!(r[0].x, 0.0);
        assert_eq!(r[3].x, 55.0);
        assert_eq!(r[4].y, 60.0);
    }

    #[test]
    fn grid_shrinks_cards_when_they_do_not_fit() {
        let (r, _, th) = grid(40, 500.0, 200.0, 100.0, 50.0, 10.0);
        assert_eq!(r.len(), 40);
        assert!(th <= 200.0, "{th}");
        assert!(r[0].w < 100.0);
    }

    #[test]
    fn grid_of_nothing_is_empty() {
        assert!(grid(0, 100.0, 100.0, 10.0, 10.0, 1.0).0.is_empty());
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    #[test]
    fn task_rows_share_a_scale_gaps_and_a_label_baseline() {
        let area = Rect::new(80.0, 190.0, 2400.0, 1100.0);
        let cards = task_grid(&[(850.0, 560.0), (760.0, 300.0), (640.0, 420.0)], area);
        assert_eq!(cards.len(), 3);
        assert!((cards[0].w / 850.0 - 0.5).abs() < 0.001);
        assert!((cards[1].w / 760.0 - cards[0].w / 850.0).abs() < 0.001);
        assert!((cards[2].h / 420.0 - cards[0].h / 560.0).abs() < 0.001);
        assert!((cards[0].y + cards[0].h - cards[2].y - cards[2].h).abs() < 0.01);
        assert!((cards[1].x - cards[0].x - cards[0].w - 40.0).abs() < 0.01);
        assert!((cards[2].x - cards[1].x - cards[1].w - 40.0).abs() < 0.01);
        assert!(((cards[0].x + cards[2].x + cards[2].w) / 2.0 - area.cx()).abs() < 0.01);
    }

    #[test]
    fn task_rows_wrap_and_shrink_to_fit() {
        let area = Rect::new(0.0, 0.0, 400.0, 260.0);
        let sizes = [(600.0, 300.0), (800.0, 500.0), (700.0, 400.0)];
        let cards = task_grid(&sizes, area);
        let scale = cards[0].w / sizes[0].0;
        assert!(scale > 0.0 && scale < 0.5);
        for (card, size) in cards.iter().zip(sizes) {
            assert!((card.w / size.0 - scale).abs() < 0.001);
            assert!(card.x >= area.x - 0.01 && card.x + card.w <= area.x + area.w + 0.01);
            assert!(card.y >= area.y - 0.01 && card.y + card.h + 34.0 <= area.y + area.h + 0.01);
        }
    }

    #[test]
    fn task_grid_considers_the_layout_just_below_a_wrap() {
        let sizes = [(480.0, 772.0), (211.0, 1036.0), (1716.0, 328.0), (925.0, 1130.0), (1574.0, 914.0), (218.0, 632.0), (1250.0, 906.0), (1573.0, 1239.0)];
        let cards = task_grid(&sizes, Rect::new(0.0, 0.0, 1603.0, 463.0));
        assert!(cards[0].w / sizes[0].0 > 0.16);
        assert!((cards[0].y + cards[0].h - cards[7].y - cards[7].h).abs() < 0.01);
    }

    #[test]
    fn a_picture_keeps_its_shape_in_the_middle_of_its_cell() {
        let r = fit((1600.0, 900.0), Rect::new(100.0, 50.0, 400.0, 400.0), 1.0);
        assert_eq!((r.w, r.h), (400.0, 225.0));
        assert_eq!((r.x, r.y), (100.0, 138.0));
        // never more than the cap times the window
        let small = fit((200.0, 100.0), Rect::new(0.0, 0.0, 400.0, 400.0), 0.5);
        assert_eq!((small.w, small.h), (100.0, 50.0));
    }
}
