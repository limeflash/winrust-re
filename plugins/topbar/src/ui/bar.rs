//! The bar of the Obsidian mockup: one layout table per frame (what is where), the motion state of every element, and the
//! painter. Drawing, hit-testing and the positions of tooltips and popups all read the same `Layout`, and none of it touches
//! the program's globals: `main.rs` hands in a `View` (what the bar shows) and a `Live` (pointer and open panels).
//!
//! Everything is in DIPs, the mockup's CSS pixels; `Bar::frame` turns the result into device pixels for the two windows
//! (the bar, and under it the shadow of the floating styles).

use std::collections::HashMap;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::HICON;
use winrust_common::style::{self, Side, Style};

use super::anim::{self, CubicBezier, EASE, SNAP, SPRING, Transition};
use super::gfx::{self, Align, Canvas, Font, Gfx, Rect, Rgba, Rgn, Surface};
use super::icons;

/// Hit-test results and motion keys. App buttons are `>= 1` (their index in the button list, Start is 0); the tray's chevron
/// and its icons are `TAPP + slot`.
pub mod keys {
    pub const NONE: i32 = -1;
    pub const SEARCH: i32 = -10;
    pub const TASKVIEW: i32 = -14;
    pub const CLOCK: i32 = -4;
    pub const LANG: i32 = -6;
    pub const WIFI_I: i32 = -7;
    pub const VOL_I: i32 = -8;
    pub const BAT_I: i32 = -9;
    pub const TAPP: i32 = -1000;
    pub const SHOWDESK: i32 = -13;
    pub const GEAR: i32 = -12;
    pub const POW_R: i32 = -15;
    pub const POW_S: i32 = -16;
    /// The red button of a recording in progress, before the clock.
    pub const REC_I: i32 = -17;
    /// The one quick-settings button around Wi-Fi, volume and battery.
    pub const SYS_GRP: i32 = -11;
}
use keys::*;

const BAR_KEY: i32 = -5000;
/// Room above a bottom bar for the icons that jump out of it on hover.
const HEAD: f32 = 14.0;

// ---------------------------------------------------------------------------------------------------------------------
// shape of the bar for a style, size and side

/// The numbers a theme and a size decide (spec section 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub thick: f32,
    pub inset: f32,
    pub radius: f32,
    pub dock: bool,
    pub vert: bool,
    pub left: bool,
}

#[must_use]
pub fn shape(style: Style, side: Side, size: u8) -> Shape {
    let s = usize::from(size).min(2);
    let (thick, inset, radius) = match style {
        Style::Classic => ([46.0, 54.0, 62.0][s], 0.0, 0.0),
        Style::Floating => ([46.0, 54.0, 62.0][s], 8.0, 16.0),
        Style::Dock => ([56.0, 64.0, 72.0][s], 10.0, 22.0),
    };
    Shape { thick, inset, radius, dock: style == Style::Dock, vert: side != Side::Bottom, left: side == Side::Left }
}

impl Shape {
    /// Thickness plus the gap to the screen edge: the mockup's `barOuter`, popups stand 12 px beyond it.
    #[must_use]
    pub fn outer(&self) -> f32 {
        self.thick + self.inset
    }

    /// What the bar reserves from the work area: the bar and its gap to the screen edge, so maximized windows end above it
    /// (the dock too, like macOS's: windows must not slide under it). Auto-hide reserves nothing (see `place`).
    #[must_use]
    pub fn reserve(&self) -> f32 {
        self.outer()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// input: what is shown and what the pointer does

/// What the network button draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Net {
    Wifi(i32),
    NoInternet(i32),
    Cable,
    Offline,
}

/// One app button (Start excluded).
#[derive(Clone, Debug)]
pub struct AppView {
    pub icon: HICON,
    pub wins: usize,
    pub active: bool,
    pub flash: bool,
    /// Colour of the window indicators.
    pub color: Rgba,
}

/// Everything the bar shows.
#[derive(Clone, Debug)]
pub struct View {
    pub style: Style,
    pub side: Side,
    pub size: u8,
    pub align_left: bool,
    /// The monitor the bar stands on, in DIPs.
    pub screen: (f32, f32),
    /// The whole bar (the main one, or an other bar set to be "the same"): with the settings gear, the tray, the language, quick
    /// settings, the power buttons and the recording button. Without it the bar holds Start, search, Task view, the app buttons and
    /// the clock, like Windows 11's bars on the other monitors.
    pub full: bool,
    /// Bar opacity, 0..1.
    pub op: f32,
    pub apps: Vec<AppView>,
    /// Tray icons kept on the bar.
    pub tray: Vec<HICON>,
    /// Battery percent and whether it is charging.
    pub battery: Option<(i32, bool)>,
    /// Volume percent and muted.
    pub vol: (i32, bool),
    pub net: Net,
    pub lang: String,
    pub time: String,
    pub date: String,
    /// Unread notifications to count on the clock (0: no badge: none, or Do not disturb, or the centre is open).
    pub unread: usize,
    /// Do not disturb is on: the clock wears a little moon.
    pub dnd: bool,
    /// The time of the recording in progress (`0:24`): a red button with it stands before the clock.
    pub rec: Option<String>,
}

/// The pointer and the panels that are open.
#[derive(Clone, Copy, Debug, Default)]
pub struct Live {
    pub now: u64,
    pub hot: i32,
    pub press: i32,
    pub gear: bool,
    pub start: bool,
    pub search: bool,
    pub qs: bool,
    pub cal: bool,
    pub lang: bool,
    pub tray: bool,
    /// Task view is open.
    pub task: bool,
    /// The app button whose jump list is open (`.app.ctxa`).
    pub jump: Option<i32>,
    /// The bar is slid away (auto-hide, a fullscreen window).
    pub hidden: bool,
}

// ---------------------------------------------------------------------------------------------------------------------
// layout

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gear,
    Search,
    TaskView,
    Start,
    App,
    Chevron,
    Tray,
    Lang,
    Qs,
    Restart,
    Power,
    /// The recording button (`recbar`): a pulsing dot and the time; it stops the recording.
    Rec,
    Clock,
    /// The thin click zone at the far end (show the desktop); drawn as nothing.
    ShowDesk,
}

/// One clickable element, in window-local DIPs.
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub key: i32,
    pub kind: Kind,
    pub rect: Rect,
    pub idx: usize,
}

/// A separator: a 1 px line, solid or fading out at its ends (`edge` is the fade length, 0 = solid).
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub rect: Rect,
    pub vertical: bool,
    pub alpha: f32,
    pub edge: f32,
}

/// The table of one frame.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub items: Vec<Item>,
    pub lines: Vec<Line>,
    /// Window-local rectangle of the bar itself.
    pub bar: Rect,
    /// Centres of the three icons inside the quick settings button (Wi-Fi, volume, battery), with their keys.
    pub qs: Vec<(i32, f32, f32)>,
}

const FONT_LANG: Font = Font::new(12.0, 600).tracking(0.03);
// the clock is `font-variant-numeric: tabular-nums` in the mockup
const FONT_TIME: Font = Font::new(13.0, 600).tabular();
const FONT_TIME_DOCK: Font = Font::new(14.0, 600).tabular();
const FONT_DATE: Font = Font::new(12.0, 400).tabular();
const FONT_REC: Font = Font::new(12.0, 600).tabular();
const FONT_LANG_V: Font = Font::new(11.0, 600).tracking(0.03);
const FONT_STACK: Font = Font::new(15.0, 600).tracking(0.01).tabular();

/// A box of a flex row (or column): its length along the bar, across it, and the margins before and after.
struct Cell {
    kind: Option<Kind>,
    key: i32,
    idx: usize,
    main: f32,
    cross: f32,
    mb: f32,
    ma: f32,
    /// A separator: (alpha, fade edge).
    line: Option<(f32, f32)>,
}

impl Cell {
    fn btn(kind: Kind, key: i32, idx: usize, main: f32, cross: f32) -> Self {
        Self { kind: Some(kind), key, idx, main, cross, mb: 0.0, ma: 0.0, line: None }
    }

    fn sep(cross: f32, mb: f32, ma: f32, alpha: f32, edge: f32) -> Self {
        Self { kind: None, key: NONE, idx: 0, main: 1.0, cross, mb, ma, line: Some((alpha, edge)) }
    }

    fn len(&self) -> f32 {
        self.mb + self.main + self.ma
    }
}

fn group_len(g: &[Cell], gap: f32) -> f32 {
    g.iter().map(Cell::len).sum::<f32>() + gap * g.len().saturating_sub(1) as f32
}

/// The three groups of the bar (settings, centre, right), as flex boxes along its length.
fn groups(g: &mut Gfx, v: &View, sh: &Shape, app_main: Option<f32>, clock_extra: f32) -> [Vec<Cell>; 3] {
    let (vert, dock) = (sh.vert, sh.dock);
    let bw = sh.thick - 8.0; // a vertical bar's buttons are the bar less 4 px each side
    let app_w = if dock { 52.0 } else { 42.0 };
    // (main, cross) of a button in a horizontal bar; vertical bars swap to (cross, main) below
    let b = |kind, key, idx, w: f32, h: f32, vh: f32| {
        if vert { Cell::btn(kind, key, idx, vh, bw) } else { Cell::btn(kind, key, idx, w, h) }
    };
    let mut left = Vec::new();
    if v.full {
        left.push(b(Kind::Gear, GEAR, 0, 40.0, 40.0, 36.0));
        if dock {
            left.push(Cell::sep(28.0, 8.0, 6.0, 0.22, 0.3));
        }
    }

    let mut mid = vec![
        b(Kind::Search, SEARCH, 0, 42.0, 42.0, 36.0),
        b(Kind::TaskView, TASKVIEW, 0, 42.0, 42.0, 36.0),
        b(Kind::Start, 0, 0, 42.0, 42.0, 36.0),
        if vert { Cell::sep(24.0, 4.0, 4.0, 0.22, 0.25) } else { Cell::sep(22.0, 6.0, 6.0, 0.12, 0.0) },
    ];
    for i in 0..v.apps.len() {
        let key = i as i32 + 1;
        mid.push(if vert {
            Cell::btn(Kind::App, key, i, app_main.unwrap_or(if dock { 44.0 } else { 38.0 }), if dock { app_w } else { bw })
        } else {
            Cell::btn(Kind::App, key, i, app_main.unwrap_or(app_w), if dock { 56.0 } else { 42.0 })
        });
    }

    let lang_w = g.measure(&v.lang, if vert { FONT_LANG_V } else { FONT_LANG });
    let n_qs = 2 + usize::from(v.battery.is_some());
    let rsep = || {
        if vert {
            Cell::sep(24.0, 4.0, 4.0, 0.22, 0.25)
        } else if dock {
            Cell::sep(20.0, 3.0, 3.0, 0.24, 0.3)
        } else {
            Cell::sep(24.0, 6.0, 6.0, 0.24, 0.3)
        }
    };
    let mut right = Vec::new();
    if v.full {
        right.push(b(Kind::Chevron, TAPP, 0, 32.0, 40.0, 36.0));
        for i in 0..v.tray.len() {
            right.push(b(Kind::Tray, TAPP + i as i32 + 1, i, 32.0, 40.0, 36.0));
        }
        right.push(b(Kind::Lang, LANG, 0, (lang_w + 20.0).max(46.0), 40.0, 36.0));
        right.push(rsep());
        let qs_len = 24.0 + 18.0 * n_qs as f32 + 12.0 * (n_qs - 1) as f32;
        right.push(b(Kind::Qs, SYS_GRP, 0, qs_len, 40.0, 16.0 + 18.0 * n_qs as f32 + 8.0 * (n_qs - 1) as f32));
        right.push(rsep());
        right.push(b(Kind::Restart, POW_R, 0, 38.0, 40.0, 36.0));
        right.push(b(Kind::Power, POW_S, 0, 38.0, 40.0, 36.0));
        right.push(rsep());
    }
    if let Some(t) = v.rec.as_ref().filter(|_| v.full) {
        // `recbar`: 30 high, `padding: 0 10px 0 9px`, an 8 px dot, 7 px gap, 6 px before the clock
        let w = 9.0 + 8.0 + 7.0 + g.measure(t, FONT_REC) + 10.0;
        let mut cell = if vert { Cell::btn(Kind::Rec, REC_I, 0, 30.0, bw) } else { Cell::btn(Kind::Rec, REC_I, 0, w, 30.0) };
        cell.ma = 6.0;
        right.push(cell);
    }
    let (clock_w, clock_h) = if dock {
        (g.measure(&v.time, FONT_TIME_DOCK) + 20.0 + clock_extra, 44.0)
    } else {
        (g.measure(&v.time, FONT_TIME).max(g.measure(&v.date, FONT_DATE)) + 20.0 + clock_extra, 44.0)
    };
    right.push(b(Kind::Clock, CLOCK, 0, clock_w, clock_h, 16.0 + 2.0 * 16.0));
    [left, mid, right]
}

/// Lays the bar out for `thick` (the animated thickness) on a screen of `v.screen` DIPs; `bar_len` is the length of the bar
/// along its edge (`None`: as long as its content, the dock).
#[must_use]
pub fn layout(g: &mut Gfx, v: &View, sh: &Shape, thick: f32, bar_len: Option<f32>, clock_extra: f32) -> Layout {
    let sh = Shape { thick, ..*sh };
    let (vert, dock) = (sh.vert, sh.dock);
    let (gap_mid, gap_right) = (if vert { 2.0 } else { 4.0 }, if dock { 0.0 } else { 2.0 });
    let gap_left = if vert { 2.0 } else { 0.0 };
    let mut gs = groups(g, v, &sh, None, clock_extra);
    let (mut l1, mut l2, mut l3) = (group_len(&gs[0], gap_left), group_len(&gs[1], gap_mid), group_len(&gs[2], gap_right));
    // too long for the bar: the app buttons give way, first by shrinking, then (nothing to scroll to yet: ponytail, an overflow
    // menu would be the way) the last ones are left off, so that nothing overlaps the right group or leaves the screen. The dock
    // is as long as its content, so its limit is the screen.
    let pad = if dock { 8.0 } else { 10.0 };
    let n = v.apps.len();
    let room = match bar_len {
        Some(len) => len - 2.0 * pad - l1 - l3 - if !vert && v.align_left && l1 > 0.0 { 12.0 } else { 0.0 },
        None if (if vert { v.screen.1 } else { v.screen.0 }) > 0.0 => (if vert { v.screen.1 } else { v.screen.0 }) - 2.0 * pad - l1 - l3 - 40.0, // 40: the dock's own spacers, and some air
        None => f32::INFINITY,
    };
    if n > 0 && l2 > room {
        let cur = gs[1][4].main;
        let shrink = ((l2 - room) / n as f32).ceil();
        gs = groups(g, v, &sh, Some((cur - shrink).max(20.0)), clock_extra);
        (l1, l2, l3) = (group_len(&gs[0], gap_left), group_len(&gs[1], gap_mid), group_len(&gs[2], gap_right));
        while l2 > room && gs[1].len() > 4 {
            gs[1].pop();
            l2 = group_len(&gs[1], gap_mid);
        }
    }

    // starts of the three groups along the bar, and the length of the bar
    let (p1, p2, p3, len) = if dock {
        if vert {
            let (p1, p2) = (pad, pad + l1);
            let p3 = p2 + l2 + 6.0 + 8.0;
            (p1, p2, p3, p3 + l3 + pad)
        } else {
            let p2 = pad + if l1 > 0.0 { l1 + 2.0 } else { 0.0 };
            let p3 = p2 + l2 + 2.0 + 6.0 + 8.0;
            (pad, p2, p3, p3 + l3 + pad)
        }
    } else {
        let total = bar_len.unwrap_or(1000.0);
        if vert {
            let free = (total - 2.0 * pad - l1 - l2 - l3).max(0.0);
            (pad, pad + l1 + free / 2.0, total - pad - l3, total)
        } else {
            let inner = total - 2.0 * pad;
            let c1 = if v.align_left {
                if l1 > 0.0 { l1 + 12.0 } else { 0.0 }
            } else {
                ((inner - l2) / 2.0).min(inner - l2 - l3).max(l1)
            };
            (pad, pad + c1, total - pad - l3, total)
        }
    };

    let mut lay = Layout::default();
    let place = |lay: &mut Layout, cells: &[Cell], start: f32, gap: f32| {
        let mut pos = start;
        for c in cells {
            pos += c.mb;
            let off = (thick - c.cross) / 2.0;
            let r = if vert { Rect::new(off, pos, c.cross, c.main) } else { Rect::new(pos, off, c.main, c.cross) };
            if let Some((alpha, edge)) = c.line {
                lay.lines.push(Line { rect: r, vertical: !vert, alpha, edge });
            } else if let Some(kind) = c.kind {
                lay.items.push(Item { key: c.key, kind, rect: r, idx: c.idx });
            }
            pos += c.main + c.ma + gap;
        }
    };
    place(&mut lay, &gs[0], p1, gap_left);
    place(&mut lay, &gs[1], p2, gap_mid);
    place(&mut lay, &gs[2], p3, gap_right);

    // the QS icons: centres along the button
    if let Some(q) = lay.items.iter().find(|i| i.kind == Kind::Qs).copied() {
        let n = 2 + usize::from(v.battery.is_some());
        for (k, key) in [WIFI_I, VOL_I, BAT_I].into_iter().take(n).enumerate() {
            let step = 18.0 + if vert { 8.0 } else { 12.0 };
            let (cx, cy) = if vert {
                (q.rect.cx(), q.rect.y + 8.0 + 9.0 + step * k as f32)
            } else {
                (q.rect.x + 12.0 + 9.0 + step * k as f32, q.rect.cy())
            };
            lay.qs.push((key, cx, cy));
        }
    }
    // dock: the right group has a line on its near side
    if dock {
        if vert {
            lay.lines.push(Line { rect: Rect::new(4.0, p3 - 8.0, thick - 8.0, 1.0), vertical: false, alpha: 0.12, edge: 0.0 });
        } else {
            lay.lines.push(Line { rect: Rect::new(p3 - 8.0, (thick - 44.0) / 2.0, 1.0, 44.0), vertical: true, alpha: 0.12, edge: 0.0 });
        }
    }
    // the far end: the show-desktop zone
    let zone = if vert { Rect::new(0.0, len - 6.0, thick, 6.0) } else { Rect::new(len - 6.0, 0.0, 6.0, thick) };
    lay.items.push(Item { key: SHOWDESK, kind: Kind::ShowDesk, rect: zone, idx: 0 });
    lay.bar = if vert { Rect::new(0.0, 0.0, thick, len) } else { Rect::new(0.0, 0.0, len, thick) };
    lay
}

impl Layout {
    /// The element under a window-local point; the quick settings button answers with the icon under the point.
    #[must_use]
    pub fn hit(&self, x: f32, y: f32) -> i32 {
        for it in &self.items {
            if !it.rect.contains(x, y) {
                continue;
            }
            if it.kind == Kind::Qs {
                let vert = it.rect.h > it.rect.w;
                let d = |c: &(i32, f32, f32)| if vert { (c.2 - y).abs() } else { (c.1 - x).abs() };
                return self.qs.iter().min_by(|a, b| d(a).total_cmp(&d(b))).map_or(SYS_GRP, |c| c.0);
            }
            return it.key;
        }
        NONE
    }

    /// The rectangle of an element (the quick settings icons answer with their button).
    #[must_use]
    pub fn rect_of(&self, key: i32) -> Option<Rect> {
        let key = if matches!(key, WIFI_I | VOL_I | BAT_I) { SYS_GRP } else { key };
        self.items.iter().find(|i| i.key == key).map(|i| i.rect)
    }

    /// The centre of an element along the bar, for its tooltip and menus; quick settings icons have their own.
    #[must_use]
    pub fn center_of(&self, key: i32) -> Option<(f32, f32)> {
        if let Some(c) = self.qs.iter().find(|c| c.0 == key) {
            return Some((c.1, c.2));
        }
        self.rect_of(key).map(|r| (r.cx(), r.cy()))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// motion

const BG: u8 = 0;
const PLATE: u8 = 1;
const PLATE_K: u8 = 2;
const LIFT: u8 = 3;
const ZOOM: u8 = 4;
const DROP: u8 = 5;
const TURN: u8 = 6;
const SQ: u8 = 7;
const LANG_IN: u8 = 8;
/// The room the clock makes for its badge (`padding .25s`), and how far the badge's `badgeIn` has run.
const EXTRA: u8 = 9;
const BADGE: u8 = 10;
/// What the badge takes: 18 wide and 6 of air (the padding goes from 10 to 34).
const BADGE_ROOM: f32 = 24.0;
// bar-level properties
const APPEAR: u8 = 0;
const HIDE: u8 = 1;
const THICK: u8 = 2;
const INSET: u8 = 3;
const RADIUS: u8 = 4;
const OP: u8 = 5;
const EDGE: u8 = 6;
const RING: u8 = 7;
const SHADOW: u8 = 8;

/// The transitions of every element, by (key, property).
#[derive(Default)]
struct Motion {
    t: HashMap<(i32, u8), Transition>,
    mode: Option<style::Anim>,
}

impl Motion {
    /// Retargets a property like a CSS transition (a first sight settles at once). `dur` is in milliseconds.
    fn go(&mut self, key: i32, prop: u8, to: f32, now: u64, dur: u32, ease: CubicBezier) {
        let (dur, delay) = anim::timing(self.mode.unwrap_or(style::Anim::Full), dur, 0);
        self.t.entry((key, prop)).or_insert_with(|| Transition::new(to)).set(to, now, dur, delay, ease);
    }

    fn val(&self, key: i32, prop: u8, now: u64) -> f32 {
        self.t.get(&(key, prop)).map_or(0.0, |t| t.value(now))
    }

    fn snap(&mut self, key: i32, prop: u8, to: f32) {
        self.t.entry((key, prop)).or_insert_with(|| Transition::new(to)).snap(to);
    }

    fn running(&self, now: u64) -> bool {
        self.t.values().any(|t| t.running(now))
    }

    /// The theme is changing the bar's own shape (thickness, gap, corner, edge, ring): its click region would lag behind it.
    fn morphing(&self, now: u64) -> bool {
        [THICK, INSET, RADIUS, EDGE, RING].into_iter().any(|p| self.t.get(&(BAR_KEY, p)).is_some_and(|t| t.running(now)))
    }
}

/// The x ranges (window-local DIPs) above the app buttons whose icon is out of its rest place (lifted, zoomed, or casting its
/// shadow): they jump up into the strip above a bottom bar, and that strip catches the mouse only over them. A column is the
/// button and 10 DIPs each side (the zoomed icon and its shadow reach that far); touching columns are one.
fn lifted_columns(lay: &Layout, m: &Motion, now: u64) -> Vec<(f32, f32)> {
    let mut cols: Vec<(f32, f32)> = lay
        .items
        .iter()
        .filter(|it| it.kind == Kind::App)
        .filter(|it| {
            let zoom = m.val(it.key, ZOOM, now);
            m.val(it.key, LIFT, now).abs() > 0.01 || (zoom != 0.0 && (zoom - 1.0).abs() > 0.001) || m.val(it.key, DROP, now) > 0.001
        })
        .map(|it| (it.rect.x - 10.0, it.rect.x + it.rect.w + 10.0))
        .collect();
    cols.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for c in cols {
        match merged.last_mut() {
            Some(last) if c.0 <= last.1 => last.1 = last.1.max(c.1),
            _ => merged.push(c),
        }
    }
    merged
}

/// The click region of the bar window, device pixels in window coordinates: the bar (`(left, top, width, height)`, corners of
/// `radius`) and, above it, a column of the strip's height (`head`) for each x range in `lifted`.
fn bar_region(bar: (i32, i32, i32, i32), radius: i32, head: i32, lifted: &[(i32, i32)]) -> Vec<Rgn> {
    let mut parts = vec![Rgn { x: bar.0, y: bar.1, w: bar.2, h: bar.3, r: radius }];
    if head > 0 {
        parts.extend(lifted.iter().filter_map(|&(x0, x1)| {
            let (x0, x1) = (x0.max(bar.0), x1.min(bar.0 + bar.2));
            (x1 > x0).then_some(Rgn { x: x0, y: 0, w: x1 - x0, h: head, r: 0 })
        }));
    }
    parts
}

// ---------------------------------------------------------------------------------------------------------------------
// the bar

/// Where the frame went, in device pixels, for the caller to present.
#[derive(Clone, Copy, Debug, Default)]
pub struct Placement {
    /// The bar window (it includes the strip above a bottom bar).
    pub win: (i32, i32, i32, i32),
    /// The bar itself (left, top, width, height), in screen px.
    pub bar: (i32, i32, i32, i32),
    /// The same where it rests, without the entry and hide slides.
    pub rest: (i32, i32, i32, i32),
    /// How far the hide animation has pushed it from its place, in px.
    pub slide: i32,
    pub alpha: u8,
    pub shadow: Option<(i32, i32, i32, i32)>,
    pub shadow_alpha: u8,
    /// Fully slid away: the windows can be hidden.
    pub gone: bool,
    /// Another frame is needed (something is moving).
    pub busy: bool,
}

/// The renderer and the state of the bar.
pub struct Bar {
    gfx: Gfx,
    surf: Surface,
    shade: Surface,
    /// What the shadow was painted for (width, height, corner radius): it is painted again only when that changes, its
    /// opacity and place follow the bar without a repaint.
    shade_key: Option<(i32, i32, u32)>,
    /// The parts of the bar window that catch the mouse (a composed window has no per-pixel hit test); `None` = all of it.
    shape: Option<Vec<Rgn>>,
    motion: Motion,
    lay: Layout,
    place: Placement,
    scale: f32,
    /// The top left corner of the monitor the bar stands on (device pixels): the placement is in virtual screen coordinates.
    origin: (i32, i32),
    seen: Option<bool>,
    lang_seen: Option<String>,
    dead: Vec<HICON>,
}

impl Bar {
    pub fn new(scale: f32) -> windows::core::Result<Self> {
        Ok(Self {
            gfx: Gfx::new(scale)?,
            surf: Surface::composed(),
            shade: Surface::composed(),
            shade_key: None,
            shape: None,
            motion: Motion::default(),
            lay: Layout::default(),
            place: Placement::default(),
            scale,
            origin: (0, 0),
            seen: None,
            lang_seen: None,
            dead: Vec::new(),
        })
    }

    /// Moves the bar to another monitor (its top left corner, device pixels); the next frame is placed there.
    pub fn set_origin(&mut self, origin: (i32, i32)) {
        self.origin = origin;
    }

    #[must_use]
    pub const fn layout(&self) -> &Layout {
        &self.lay
    }

    #[must_use]
    pub const fn placement(&self) -> &Placement {
        &self.place
    }

    /// The element under a client point of the bar window, in device pixels.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> i32 {
        self.lay.hit(x as f32 / self.scale, y as f32 / self.scale)
    }

    /// Screen position in device pixels of the middle of an element.
    #[must_use]
    pub fn screen_center(&self, key: i32) -> Option<(i32, i32)> {
        let (cx, cy) = self.lay.center_of(key)?;
        Some((self.place.win.0 + (cx * self.scale).round() as i32, self.place.win.1 + (cy * self.scale).round() as i32))
    }

    /// Screen rectangle (left, top, right, bottom) in device pixels of an element.
    #[must_use]
    pub fn screen_rect(&self, key: i32) -> Option<(i32, i32, i32, i32)> {
        let r = self.lay.rect_of(key)?;
        let (x, y) = (self.place.win.0, self.place.win.1);
        let s = self.scale;
        Some((x + (r.x * s).round() as i32, y + (r.y * s).round() as i32, x + ((r.x + r.w) * s).round() as i32, y + ((r.y + r.h) * s).round() as i32))
    }

    /// Lays the bar out, moves everything toward the state in `live` and paints both windows.
    pub fn frame(&mut self, v: &View, live: &Live) -> Placement {
        let now = live.now;
        let s = self.scale;
        self.motion.mode = Some(style::anim());
        let sh = shape(v.style, v.side, v.size);

        // the bar itself: thickness, gap, corner and edge follow the theme; a new look of the dock starts the entry again
        let first = self.seen.is_none();
        let m = &mut self.motion;
        let (edge, ring, shadow) = match v.style {
            Style::Classic => (1.0, 0.0, 0.0),
            _ => (0.0, 1.0, 1.0),
        };
        if first {
            m.t.clear();
            for (p, to) in [(THICK, sh.thick), (INSET, sh.inset), (RADIUS, sh.radius), (OP, v.op), (EDGE, edge), (RING, ring), (SHADOW, shadow), (HIDE, 0.0)] {
                m.snap(BAR_KEY, p, to);
            }
            m.snap(BAR_KEY, APPEAR, 0.0);
        }
        let entry_ms = if sh.dock { 450 } else { 400 };
        if self.seen.is_some_and(|dock| dock != sh.dock) {
            m.snap(BAR_KEY, APPEAR, 0.0); // dock <-> the others starts the entry again
        }
        m.go(BAR_KEY, APPEAR, 1.0, now, entry_ms, SNAP);
        m.go(BAR_KEY, HIDE, f32::from(live.hidden), now, 350, SNAP);
        m.go(BAR_KEY, THICK, sh.thick, now, 400, SNAP);
        m.go(BAR_KEY, INSET, sh.inset, now, 400, SNAP);
        m.go(BAR_KEY, RADIUS, sh.radius, now, 400, EASE);
        m.go(BAR_KEY, OP, v.op, now, 300, EASE);
        m.go(BAR_KEY, EDGE, edge, now, 300, EASE);
        m.go(BAR_KEY, RING, ring, now, 300, EASE);
        m.go(BAR_KEY, SHADOW, shadow, now, 300, EASE);
        // the clock widens for its badge (a vertical bar has no room to widen: its badge sits in the corner)
        let badge = v.unread > 0 && !v.dnd;
        m.go(CLOCK, EXTRA, if (badge || v.dnd) && !sh.vert { BADGE_ROOM } else { 0.0 }, now, 250, SPRING);
        if badge { m.go(CLOCK, BADGE, 1.0, now, 400, anim::LINEAR) } else { m.snap(CLOCK, BADGE, 0.0) }
        let clock_extra = m.val(CLOCK, EXTRA, now);
        let appear = if sh.dock && sh.vert { 1.0 } else { m.val(BAR_KEY, APPEAR, now) }; // a vertical dock has no entry
        let hide = m.val(BAR_KEY, HIDE, now);
        let (thick, inset, radius) = (m.val(BAR_KEY, THICK, now), m.val(BAR_KEY, INSET, now), m.val(BAR_KEY, RADIUS, now));
        let (op, edge, ring, shadow_k) = (m.val(BAR_KEY, OP, now), m.val(BAR_KEY, EDGE, now), m.val(BAR_KEY, RING, now), m.val(BAR_KEY, SHADOW, now));
        self.seen = Some(sh.dock);

        // geometry in device pixels: lengths come from the settled style (left, right and width jump), thickness and gap glide
        let (sw, shh) = ((v.screen.0 * s).round() as i32, (v.screen.1 * s).round() as i32);
        let px = |d: f32| (d * s).round() as i32;
        let thick_px = px(thick);
        let inset_px = px(inset);
        let inset_t = px(sh.inset);
        let along = if sh.vert { shh } else { sw };
        let bar_len = (!sh.dock).then(|| (along - 2 * inset_t) as f32 / s);
        let thick_dip = thick_px as f32 / s;
        let mut lay = layout(&mut self.gfx, v, &sh, thick_dip, bar_len, clock_extra);
        let len_px = px(if sh.dock { lay.bar.w.max(lay.bar.h) } else { (along - 2 * inset_t) as f32 / s });
        // slide: the entry (14 px up from below), the hide (its own length and 14 px more)
        let rise = px((1.0 - appear) * 14.0);
        let slide = (hide * (thick + sh.inset + 14.0) * s).round() as i32;
        let head = if sh.vert { 0 } else { px(HEAD) };
        let (bx, by, bw, bh) = if sh.vert {
            let y = if sh.dock { (shh - len_px) / 2 } else { inset_t };
            let h = if sh.dock { len_px } else { shh - 2 * inset_t };
            let x = if sh.left { inset_px } else { sw - inset_px - thick_px };
            (x, y, thick_px, h)
        } else {
            let x = if sh.dock { (sw - len_px) / 2 } else { inset_t };
            let w = if sh.dock { len_px } else { sw - 2 * inset_t };
            (x, shh - inset_px - thick_px, w, thick_px)
        };
        let (dx, dy) = if sh.vert { (if sh.left { -slide } else { slide }, rise) } else { (0, slide + rise) };
        let (bx, by) = (bx + dx, by + dy);
        // the layout is in bar-local DIPs: move it into the window (the strip above a bottom bar)
        let (ox, oy) = (0.0, head as f32 / s);
        let (bw_d, bh_d) = (bw as f32 / s, bh as f32 / s);
        for it in &mut lay.items {
            it.rect = it.rect.shift(ox, oy);
        }
        for l in &mut lay.lines {
            l.rect = l.rect.shift(ox, oy);
        }
        for q in &mut lay.qs {
            q.1 += ox;
            q.2 += oy;
        }
        lay.bar = Rect::new(ox, oy, bw_d, bh_d);
        let (win_w, win_h) = (bw, bh + head);

        self.drive(v, live, &lay);
        let motion = &self.motion;
        let colors = Palette::now();
        let look = Look { bg: colors.bg.alpha(op), radius: radius.min(bw_d.min(bh_d) / 2.0), edge, ring };
        let (dock, vert) = (sh.dock, sh.vert);
        let lay_ref = &lay;
        let dead = std::mem::take(&mut self.dead);
        let _ = self.gfx.paint(&mut self.surf, win_w, win_h, |c| {
            dead.into_iter().for_each(|i| c.forget_icon(i));
            paint(c, lay_ref, v, live, motion, &look, &colors, dock, vert);
        });
        // what catches the mouse: the bar, and the columns over the icons that jump out of it (a morph of the theme: all)
        self.shape = if self.motion.morphing(now) {
            None
        } else {
            let lifted: Vec<(i32, i32)> = if vert { Vec::new() } else { lifted_columns(&lay, &self.motion, now).into_iter().map(|(x0, x1)| (px(x0), px(x1))).collect() };
            Some(bar_region((0, head, bw, bh), px(look.radius), head, &lifted))
        };
        self.lay = lay;

        // the shadow of the floating styles lives in its own window right under the bar
        let (pad, sdy) = (px(75.0), px(20.0));
        let shadow_alpha = (255.0 * shadow_k * (1.0 - hide) * appear).round() as u8;
        let mut shadow_rect = None;
        if shadow_alpha > 0 {
            let (w, h) = (bw + 2 * pad, bh + 2 * pad + sdy);
            let r = Rect::new(pad as f32 / s, pad as f32 / s, bw_d, bh_d);
            let rad = look.radius;
            let key = (w, h, rad.to_bits());
            if self.shade_key != Some(key) || !self.shade.is_current() {
                let _ = self.gfx.paint(&mut self.shade, w, h, |c| {
                    c.shadow(r, rad, 20.0, 50.0, Rgba::new(0.0, 0.0, 0.0, 0.45));
                    c.erase(r, rad);
                });
                self.shade_key = Some(key);
            }
            shadow_rect = Some((bx - pad, by - pad, w, h));
        }
        let (mx, my) = self.origin;
        let place = Placement {
            win: (mx + bx, my + by - head, win_w, win_h),
            bar: (mx + bx, my + by, bw, bh),
            rest: (mx + bx - dx, my + by - dy, bw, bh),
            slide,
            alpha: (255.0 * appear).round() as u8,
            shadow: shadow_rect.map(|(x, y, w, h)| (mx + x, my + y, w, h)),
            shadow_alpha,
            gone: hide >= 0.999 && live.hidden,
            busy: self.motion.running(now),
        };
        self.place = place;
        place
    }

    /// Presents the last frame: the bar window and, under it, the shadow window. A composed bar is cut to what catches the
    /// mouse first (the cut grows before what it shows does) and reaches the screen with one commit.
    pub fn present(&self, bar: HWND, shadow: HWND) {
        let p = &self.place;
        gfx::shape(bar, self.shape.as_deref());
        let _ = self.surf.present(bar, p.win.0, p.win.1, p.alpha);
        if let Some((x, y, _, _)) = p.shadow {
            let _ = self.shade.present(shadow, x, y, p.shadow_alpha);
        }
        gfx::commit();
    }

    /// Which parts of the bar window catch the mouse (`None` = all of it); what a composed window is cut to.
    #[must_use]
    pub fn shape(&self) -> Option<&[Rgn]> {
        self.shape.as_deref()
    }

    /// The frame's pixels, premultiplied BGRA: (bar window, shadow window).
    #[must_use]
    pub fn surfaces(&self) -> (&Surface, &Surface) {
        (&self.surf, &self.shade)
    }

    #[must_use]
    pub fn gfx(&self) -> &Gfx {
        &self.gfx
    }

    /// An icon is about to be destroyed: its cached bitmap goes with the next frame.
    pub fn forget_icon(&mut self, icon: HICON) {
        self.dead.push(icon);
    }

    /// Moves every element toward the state `live` asks for.
    fn drive(&mut self, v: &View, live: &Live, lay: &Layout) {
        let (now, m) = (live.now, &mut self.motion);
        let hot_qs = matches!(live.hot, WIFI_I | VOL_I | BAT_I);
        let vert = lay.bar.h > lay.bar.w;
        for it in &lay.items {
            let key = it.key;
            let hover = if it.kind == Kind::Qs { hot_qs } else { live.hot == key };
            let (open, open_a) = match it.kind {
                Kind::Gear => (live.gear, 0.11),
                Kind::Start => (live.start, 0.11),
                Kind::Search => (live.search, 0.11),
                Kind::Chevron => (live.tray, 0.13),
                Kind::Lang => (live.lang, 0.13),
                Kind::Qs => (live.qs, 0.13),
                Kind::Clock => (live.cal, 0.13),
                Kind::TaskView => (live.task, 0.11),
                _ => (false, 0.0),
            };
            match it.kind {
                Kind::ShowDesk => {}
                Kind::App => {
                    let a = &v.apps[it.idx];
                    m.go(key, BG, if a.active || live.jump == Some(key) { 0.11 } else { 0.0 }, now, 250, EASE);
                    let pressed = hover && live.press == key;
                    let near = live.hot >= 1 && (live.hot - key).abs() == 1;
                    let (ty, zoom) = match (hover, pressed, near, vert) {
                        (true, true, _, _) => (-2.0, 1.1),
                        (true, _, _, false) => (-6.0, 1.24),
                        (true, _, _, true) => (0.0, 1.12),
                        (_, _, true, false) => (-2.0, 1.09),
                        (_, _, true, true) => (0.0, 1.03),
                        _ => (0.0, 1.0),
                    };
                    let dur = if pressed { 120 } else { 450 };
                    m.go(key, LIFT, ty, now, dur, SPRING);
                    m.go(key, ZOOM, zoom, now, dur, SPRING);
                    m.go(key, DROP, f32::from(hover && !vert), now, 450, EASE);
                    m.go(key, PLATE, f32::from(hover), now, 300, EASE);
                    m.go(key, PLATE_K, if hover { 1.0 } else if vert { 0.85 } else { 0.7 }, now, 450, SPRING);
                }
                _ => {
                    let a = if hover { if it.kind == Kind::Power { 0.16 } else { 0.1 } } else if open { open_a } else { 0.0 };
                    m.go(key, BG, a, now, 250, EASE);
                    match it.kind {
                        Kind::Gear => m.go(key, TURN, if live.gear { 60.0 } else { 0.0 }, now, 500, SNAP),
                        Kind::Chevron => m.go(key, TURN, if live.tray { 180.0 } else { 0.0 }, now, 350, SPRING),
                        Kind::Start => m.go(key, SQ, if live.start { 0.86 } else { 1.0 }, now, 200, SNAP),
                        _ => {}
                    }
                }
            }
        }
        // the language label slides in from below when it changes (the first sight does not animate)
        if self.lang_seen.as_deref() != Some(v.lang.as_str()) {
            let first = self.lang_seen.is_none();
            self.lang_seen = Some(v.lang.clone());
            m.snap(LANG, LANG_IN, if first { 1.0 } else { 0.0 });
        }
        m.go(LANG, LANG_IN, 1.0, now, 450, SPRING);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// painting

/// The theme's colours as the painter wants them.
struct Palette {
    bg: Rgba,
    line: Rgba,
    text: Rgba,
    dim: Rgba,
    accent: Rgba,
}

impl Palette {
    fn now() -> Self {
        let p = style::pal();
        Self { bg: Rgba::colorref(p.bg), line: Rgba::colorref(p.line), text: Rgba::colorref(p.text), dim: Rgba::colorref(p.dim), accent: Rgba::colorref(p.accent) }
    }
}

struct Look {
    bg: Rgba,
    radius: f32,
    edge: f32,
    ring: f32,
}

const RED_HOVER: Rgba = Rgba::css(255, 120, 120, 1.0);
const RED_TEXT: Rgba = Rgba::hex(0xff9a9a);
const BAD: Rgba = Rgba::hex(0xff453a);
const GOOD: Rgba = Rgba::hex(0x30d158);
const WARN: Rgba = Rgba::hex(0xffd60a);

#[allow(clippy::too_many_arguments)]
fn paint(c: &mut Canvas<'_>, lay: &Layout, v: &View, live: &Live, m: &Motion, look: &Look, pal: &Palette, dock: bool, vert: bool) {
    let now = live.now;
    let bar = lay.bar;
    c.fill(bar, look.radius, look.bg);
    // the edge facing the windows: a hairline for the flat bar, a ring for the floating ones
    if look.edge > 0.0 {
        let r = match (vert, v.side == Side::Left) {
            (false, _) => Rect::new(bar.x, bar.y, bar.w, 1.0),
            (true, true) => Rect::new(bar.x + bar.w - 1.0, bar.y, 1.0, bar.h),
            (true, false) => Rect::new(bar.x, bar.y, 1.0, bar.h),
        };
        c.fill(r, 0.0, pal.line.fade(look.edge));
    }
    if look.ring > 0.0 {
        c.ring(bar, look.radius, 1.0, pal.line.fade(look.ring));
    }
    for l in &lay.lines {
        let col = Rgba::white(l.alpha);
        if l.edge > 0.0 {
            c.fade_line(l.rect, l.vertical, l.edge, col);
        } else {
            c.fill(l.rect, 0.0, col);
        }
    }
    for it in &lay.items {
        match it.kind {
            Kind::ShowDesk => {}
            Kind::App => app(c, it, &v.apps[it.idx], m, now, pal, dock, vert, v.side == Side::Left),
            _ => button(c, it, lay, v, live, m, pal, dock, vert),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn button(c: &mut Canvas<'_>, it: &Item, lay: &Layout, v: &View, live: &Live, m: &Motion, pal: &Palette, dock: bool, vert: bool) {
    let (now, r) = (live.now, it.rect);
    let a = m.val(it.key, BG, now);
    let power = it.kind == Kind::Power;
    let hover = if it.kind == Kind::Qs { matches!(live.hot, WIFI_I | VOL_I | BAT_I) } else { live.hot == it.key };
    if a > 0.0 && it.kind != Kind::Rec {
        let base = if power { RED_HOVER } else { Rgba::white(1.0) };
        c.fill(r, 10.0, base.alpha(a));
    }
    let ink = if power && hover { RED_TEXT } else { pal.text };
    let (cx, cy) = (r.cx(), r.cy());
    match it.kind {
        Kind::Gear => c.icon(icons::GEAR, (cx, cy), 19.0, 1.8, ink, m.val(it.key, TURN, now)),
        Kind::Search => c.icon(icons::SEARCH, (cx, cy), 19.0, 1.9, ink, 0.0),
        Kind::TaskView => c.icon(icons::TASK_VIEW, (cx, cy), 19.0, 1.9, ink, 0.0),
        Kind::Start => {
            let k = {
                let t = m.val(it.key, SQ, now);
                if t == 0.0 { 1.0 } else { t }
            };
            c.transformed(cx, cy, k, 0.0, 0.0, |c| {
                let u = 20.0 / 24.0;
                for (x, y) in icons::START_SQUARES {
                    let (sx, sy) = (cx - 10.0 + x * u, cy - 10.0 + y * u);
                    c.fill(Rect::new(sx, sy, icons::START_SQUARE * u, icons::START_SQUARE * u), icons::START_RADIUS * u, pal.accent);
                }
            });
        }
        Kind::Chevron => c.icon(icons::CHEVRON, (cx, cy), 15.0, 2.2, ink, m.val(it.key, TURN, now)),
        Kind::Tray => {
            if let Some(&i) = v.tray.get(it.idx) {
                c.hicon(i, Rect::new(cx - 9.0, cy - 9.0, 18.0, 18.0), 1.0);
            }
        }
        Kind::Lang => {
            let t = m.val(LANG, LANG_IN, now);
            let off = (1.0 - t) * 16.0;
            c.clipped(r, |c| {
                let font = if vert { FONT_LANG_V } else { FONT_LANG };
                c.text(&v.lang, Rect::new(r.x, r.y + off, r.w, r.h), font, ink.fade(t), Align::Center);
            });
        }
        Kind::Qs => qs(c, &lay.qs, v, ink),
        Kind::Restart => c.icon(icons::RESTART, (cx, cy), 18.0, 1.9, ink, 0.0),
        Kind::Power => c.icon(icons::POWER, (cx, cy), 18.0, 1.9, ink, 0.0),
        Kind::Clock => clock(c, r, v, pal, dock, vert, (m, now)),
        Kind::Rec => rec_button(c, r, v, a, vert, now),
        Kind::App | Kind::ShowDesk => {}
    }
}

/// The recording button: `rgba(229,72,77,.16)`, the time in `#ff9a9e`, a dot that pulses (`recPulse`, 1.3 s).
fn rec_button(c: &mut Canvas<'_>, r: Rect, v: &View, hover: f32, vert: bool, now: u64) {
    let Some(t) = &v.rec else { return };
    c.fill(r, 15.0, Rgba::css(229, 72, 77, 0.16));
    if hover > 0.0 {
        c.fill(r, 15.0, Rgba::white(hover));
    }
    let ph = (now % 1300) as f32 / 1300.0;
    let e = CubicBezier::new(0.42, 0.0, 0.58, 1.0).ease(1.0 - (2.0 * ph - 1.0).abs());
    let d = 8.0 * (1.0 - 0.18 * e);
    let red = Rgba::css(229, 72, 77, 1.0).fade(1.0 - 0.55 * e);
    let ink = Rgba::css(255, 154, 158, 1.0);
    if vert {
        c.text(t, r, FONT_REC, ink, Align::Center);
        c.fill(Rect::new(r.cx() - d / 2.0, r.y + 4.0 - d / 2.0, d, d), d / 2.0, red);
    } else {
        c.fill(Rect::new(r.x + 9.0 + 4.0 - d / 2.0, r.cy() - d / 2.0, d, d), d / 2.0, red);
        c.text(t, Rect::new(r.x + 9.0 + 8.0 + 7.0, r.y, r.w, r.h), FONT_REC, ink, Align::Start);
    }
}

fn qs(c: &mut Canvas<'_>, centres: &[(i32, f32, f32)], v: &View, ink: Rgba) {
    let at = |k: usize| centres.get(k).map(|q| (q.1, q.2));
    let (Some(w), Some(vol)) = (at(0), at(1)) else { return };
    let bat = at(2);
    // network
    let dim = ink.alpha(0.35);
    match v.net {
        Net::Wifi(q) | Net::NoInternet(q) => {
            let lit = if q >= 67 { 3 } else if q >= 34 { 2 } else { 1 };
            for (i, d) in [icons::WIFI_ARC_1, icons::WIFI_ARC_2, icons::WIFI_ARC_3].into_iter().enumerate() {
                c.icon(d, w, 18.0, 1.9, if i < lit { ink } else { dim }, 0.0);
            }
            c.icon(icons::WIFI_DOT, w, 18.0, 1.9, ink, 0.0);
            if matches!(v.net, Net::NoInternet(_)) {
                c.icon(icons::WIFI_WARN, w, 18.0, 1.9, WARN, 0.0);
            }
        }
        Net::Cable => c.icon(icons::CABLE, w, 18.0, 1.9, ink, 0.0),
        Net::Offline => {
            for d in [icons::WIFI_ARC_1, icons::WIFI_ARC_2, icons::WIFI_ARC_3] {
                c.icon(d, w, 18.0, 1.9, dim, 0.0);
            }
            c.icon(icons::WIFI_OFF, w, 18.0, 1.9, ink, 0.0);
        }
    }
    // volume
    let (level, muted) = v.vol;
    let tone = if muted { BAD } else { ink };
    c.icon(icons::SPEAKER, vol, 18.0, 1.9, tone, 0.0);
    if muted || level == 0 {
        c.icon(icons::VOLUME_MUTED, vol, 18.0, 1.9, tone, 0.0);
    } else {
        c.icon(icons::VOLUME_1, vol, 18.0, 1.9, ink, 0.0);
        if level >= 34 {
            c.icon(icons::VOLUME_2, vol, 18.0, 1.9, ink, 0.0);
        }
    }
    // battery: the outline, then the charge inside it (24-grid x 4.3..15.7, y 9.75..14.25 at 18/24 scale)
    if let (Some((pct, ac)), Some(b)) = (v.battery, bat) {
        let col = if ac { GOOD } else if pct <= 15 { BAD } else { ink };
        c.icon(icons::BATTERY, b, 18.0, 1.9, col, 0.0);
        let k = 18.0 / 24.0;
        let fw = (11.4 * k * pct.clamp(0, 100) as f32 / 100.0).max(1.0);
        c.fill(Rect::new(b.0 + (4.3 - 12.0) * k, b.1 + (9.75 - 12.0) * k, fw, 4.5 * k), 0.8, col);
    }
}

fn clock(c: &mut Canvas<'_>, r: Rect, v: &View, pal: &Palette, dock: bool, vert: bool, (m, now): (&Motion, u64)) {
    let extra = m.val(CLOCK, EXTRA, now);
    if vert {
        // two stacked numbers, hours over minutes
        let digits: Vec<&str> = v.time.split(|ch: char| !ch.is_ascii_digit()).filter(|s| !s.is_empty()).collect();
        let (hh, mm) = (digits.first().copied().unwrap_or(&v.time), digits.get(1).copied().unwrap_or(""));
        let top = r.cy() - 16.0;
        c.text(hh, Rect::new(r.x, top, r.w, 16.0), FONT_STACK, pal.text, Align::Center);
        c.text(mm, Rect::new(r.x, top + 16.0, r.w, 16.0), FONT_STACK, pal.dim, Align::Center);
        badge(c, Rect::new(r.x + r.w - 3.0 - 16.0, r.y + 3.0, 16.0, 16.0), v, pal, m.val(CLOCK, BADGE, now));
        return;
    }
    let inner = Rect::new(r.x + 10.0, r.y, r.w - 20.0 - extra, r.h);
    if dock {
        c.text(&v.time, inner, FONT_TIME_DOCK, pal.text, Align::End); // the date is hidden in the dock
    } else {
        let top = r.cy() - 15.5;
        c.text(&v.time, Rect::new(inner.x, top, inner.w, 15.6), FONT_TIME, pal.text, Align::End);
        c.text(&v.date, Rect::new(inner.x, top + 16.6, inner.w, 14.4), FONT_DATE, pal.dim, Align::End);
    }
    badge(c, Rect::new(r.x + r.w - 8.0 - 18.0, r.y + 13.0, 18.0, 18.0), v, pal, m.val(CLOCK, BADGE, now));
}

/// The count of unread notifications (`.ncbadge`: the accent, 11 px bold, `badgeIn` pops it in), or the moon of Do not disturb.
/// `at` is the 18 px square at the clock's right edge; `t` how far `badgeIn` has run, 0..1.
fn badge(c: &mut Canvas<'_>, at: Rect, v: &View, pal: &Palette, t: f32) {
    if v.dnd {
        c.fill(at, at.w / 2.0, Rgba::white(0.1));
        c.glyph(icons::MOON, (at.cx(), at.cy()), 9.0 * at.w / 18.0, Rgba::css(201, 205, 216, 1.0));
    } else if v.unread > 0 {
        let label = if v.unread > 9 { "9+".to_string() } else { v.unread.to_string() };
        let font = Font::new(11.0 * at.w / 18.0, 700);
        let w = (c.measure(&label, font).ceil() + 10.0).max(at.w);
        // `badgeIn`: 0 -> 1.25 over the first 60 %, back to 1 over the rest, each leg with the spring curve
        let k = if t < 0.6 { SPRING.ease(t / 0.6) * 1.25 } else { 1.25 - 0.25 * SPRING.ease((t - 0.6) / 0.4) };
        let r = Rect::new(at.x + at.w - w, at.y, w, at.h);
        c.transformed(r.cx(), r.cy(), k, 0.0, 0.0, |c| {
            c.fill(r, r.h / 2.0, pal.accent);
            c.text(&label, r, font, Rgba::css(11, 18, 32, 1.0), Align::Center);
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn app(c: &mut Canvas<'_>, it: &Item, a: &AppView, m: &Motion, now: u64, pal: &Palette, dock: bool, vert: bool, left: bool) {
    let r = it.rect;
    let bg = m.val(it.key, BG, now);
    if bg > 0.0 {
        c.fill(r, 10.0, Rgba::white(bg));
    }
    if a.flash {
        c.fill(r, 10.0, pal.accent.alpha(0.4));
    }
    // the hover plate grows out of the middle
    let (plate, pk) = (m.val(it.key, PLATE, now), m.val(it.key, PLATE_K, now));
    if plate > 0.0 {
        let base = if vert {
            r
        } else if dock {
            Rect::new(r.x + 2.0, r.y + 3.0, r.w - 4.0, r.h - 7.0)
        } else {
            r.inset(1.0)
        };
        let p = base.scale(pk);
        c.fill(p, 12.0 * pk, Rgba::white(if vert { 0.09 } else { 0.1 } * plate));
    }
    // the icon: 30 px (dock 40), centred above the indicator strip
    let size = if dock { 40.0 } else { 30.0 };
    let pad_b = if vert { 0.0 } else if dock { 6.0 } else { 4.0 };
    let (cx, cy) = (r.cx(), r.y + (r.h - pad_b) / 2.0);
    let (ty, zoom, drop) = (m.val(it.key, LIFT, now), m.val(it.key, ZOOM, now), m.val(it.key, DROP, now));
    let zoom = if zoom == 0.0 { 1.0 } else { zoom };
    let box_ = Rect::new(cx - size / 2.0, cy - size / 2.0, size, size);
    c.transformed(cx, cy, zoom, 0.0, ty, |c| {
        if a.icon.is_invalid() {
            c.fill(box_.inset(size * 0.08), size * 0.22, a.color.alpha(0.9));
        } else {
            if drop > 0.0 {
                c.faded(drop, |c| c.icon_shadow(a.icon, box_, 4.0, 6.0, Rgba::new(0.0, 0.0, 0.0, 0.35)));
            }
            c.hicon(a.icon, box_, 1.0);
        }
    });
    indicators(c, r, a, vert, left);
}

/// Window indicators: up to three pills, the first one a dash when the app has the focus.
fn indicators(c: &mut Canvas<'_>, r: Rect, a: &AppView, vert: bool, left: bool) {
    let n = a.wins.min(3);
    if n == 0 {
        return;
    }
    let (rad, gap) = (1.75, 2.5);
    let ws: Vec<f32> = (0..n).map(|i| if i == 0 && a.active { 8.0 } else { rad * 2.0 }).collect();
    let total = ws.iter().sum::<f32>() + gap * (n - 1) as f32;
    let mut pos = -total / 2.0;
    for (i, w) in ws.iter().enumerate() {
        let col = a.color.fade(if i == 0 { 1.0 } else { 0.35 });
        let pill = if vert {
            // on the edge facing the screen border, running along the bar
            let x = if left { r.x + 4.0 } else { r.x + r.w - 4.0 };
            Rect::new(x - rad, r.cy() + pos, rad * 2.0, *w)
        } else {
            Rect::new(r.cx() + pos, r.y + r.h - 5.75, *w, rad * 2.0)
        };
        c.fill(pill, rad, col);
        pos += w + gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(apps: usize) -> View {
        View {
            style: Style::Classic,
            side: Side::Bottom,
            size: 1,
            align_left: false,
            screen: (1440.0, 900.0),
            full: true,
            op: 0.85,
            apps: (0..apps).map(|_| AppView { icon: HICON(std::ptr::null_mut()), wins: 1, active: false, flash: false, color: Rgba::hex(0x62a0ea) }).collect(),
            tray: vec![],
            battery: None,
            vol: (50, false),
            net: Net::Wifi(80),
            lang: "RUS".into(),
            time: "03:57".into(),
            date: "11/26/2026".into(),
            unread: 0,
            dnd: false,
            rec: None,
        }
    }

    #[test]
    fn classic_bar_matches_the_mockup_numbers() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let mut g = Gfx::new(1.0).expect("direct2d");
        let v = view(8);
        let sh = shape(v.style, v.side, v.size);
        let lay = layout(&mut g, &v, &sh, sh.thick, Some(1440.0), 0.0);
        let cx = |k: i32| lay.rect_of(k).map(|r| r.cx()).unwrap();
        assert!((cx(0) - 573.5).abs() < 0.6, "start {}", cx(0));
        for (i, want) in [636.5, 682.5, 728.5, 774.5, 820.5, 866.5, 912.5, 958.5].into_iter().enumerate() {
            assert!((cx(i as i32 + 1) - want).abs() < 0.6, "app {i}: {} vs {want}", cx(i as i32 + 1));
        }
        assert_eq!(lay.rect_of(GEAR).map(|r| (r.x, r.w, r.h)), Some((10.0, 40.0, 40.0)));
        assert_eq!(lay.hit(cx(3), 27.0), 3);
        assert_eq!(lay.hit(cx(3) + 23.0, 27.0), NONE); // the 4 px gap between two buttons
        let clock = lay.rect_of(CLOCK).unwrap();
        assert!((clock.x + clock.w - 1430.0).abs() < 0.5);
    }

    #[test]
    fn the_clock_makes_room_for_its_badge_and_the_rest_of_the_bar_gives_way() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let mut g = Gfx::new(1.0).expect("direct2d");
        let v = view(8);
        let sh = shape(v.style, v.side, v.size);
        let plain = layout(&mut g, &v, &sh, sh.thick, Some(1440.0), 0.0);
        let wide = layout(&mut g, &v, &sh, sh.thick, Some(1440.0), BADGE_ROOM);
        let (a, b) = (plain.rect_of(CLOCK).unwrap(), wide.rect_of(CLOCK).unwrap());
        assert!((b.w - a.w - 24.0).abs() < 1e-3, "padding 10 -> 34 on the right");
        assert!((a.x + a.w - (b.x + b.w)).abs() < 1e-3, "the clock stays at the bar's end");
        let (qs_a, qs_b) = (plain.rect_of(SYS_GRP).unwrap(), wide.rect_of(SYS_GRP).unwrap());
        assert!((qs_a.x - qs_b.x - 24.0).abs() < 1e-3, "the group before it moves left by the same");
    }

    fn live(now: u64) -> Live {
        Live { now, hot: NONE, press: NONE, ..Default::default() }
    }

    fn bar() -> Bar {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        Bar::new(1.0).expect("direct2d")
    }

    #[test]
    fn windows_sit_where_the_mockup_puts_the_bar() {
        let mut b = bar();
        let mut v = view(8);
        b.frame(&v, &live(0));
        let p = b.frame(&v, &live(10_000));
        // classic: the whole width, 54 high at the bottom, the 14 px strip for jumping icons above it; no shadow
        assert_eq!((p.bar, p.win, p.alpha, p.shadow), ((0, 846, 1440, 54), (0, 832, 1440, 68), 255, None));
        v.style = Style::Floating;
        b.frame(&v, &live(10_000));
        let p = b.frame(&v, &live(20_000));
        assert_eq!((p.bar, p.rest), ((8, 838, 1424, 54), (8, 838, 1424, 54)));
        assert_eq!(p.shadow, Some((8 - 75, 838 - 75, 1424 + 150, 54 + 150 + 20)));
        v.style = Style::Dock;
        b.frame(&v, &live(20_000));
        let p = b.frame(&v, &live(30_000));
        let (x, y, w, h) = p.bar;
        assert_eq!((y, h), (900 - 10 - 64, 64));
        assert!((x * 2 + w - 1440).abs() <= 1, "centred");
        assert!(p.shadow.is_some());
        v.side = Side::Left;
        v.style = Style::Classic;
        b.frame(&v, &live(30_000));
        let p = b.frame(&v, &live(40_000));
        assert_eq!(p.bar, (0, 0, 54, 900));
    }

    #[test]
    fn hiding_slides_the_bar_out_and_back() {
        let mut b = bar();
        let v = view(3);
        b.frame(&v, &live(0));
        let shown = b.frame(&v, &live(10_000));
        let hide = Live { hidden: true, ..live(10_000) };
        let first = b.frame(&v, &hide);
        assert_eq!(first.bar, shown.bar, "starts from where it stands");
        let mid = b.frame(&v, &Live { hidden: true, ..live(10_150) });
        assert!(mid.bar.1 > shown.bar.1 && !mid.gone && mid.busy);
        let end = b.frame(&v, &Live { hidden: true, ..live(10_400) });
        assert!(end.gone && end.bar.1 >= 900 - 54 + 54 + 14 - 1, "{end:?}");
        assert_eq!(end.rest, shown.rest);
        let back = b.frame(&v, &live(11_000));
        let back = b.frame(&v, &live(11_400));
        assert_eq!(back.bar, shown.bar);
    }

    #[test]
    fn a_frame_reaches_a_layered_window() {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_POPUP};
        use windows::core::w;
        let mut b = bar();
        b.frame(&view(3), &live(0));
        // SAFETY: a popup that is never shown, destroyed again below.
        unsafe {
            let hw = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW, w!("STATIC"), None, WS_POPUP, 0, 0, 1, 1, None, None, None, None).expect("window");
            assert!(b.surfaces().0.present(hw, 0, 0, 255));
            let _ = DestroyWindow(hw);
        }
    }

    #[test]
    fn a_frame_reaches_a_composed_window() {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
        use windows::core::w;
        // SAFETY: COM for WIC on the test thread.
        unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED).ok().ok() };
        if !gfx::compose_windows() {
            return; // no DirectComposition on this machine: the layered path above is the one that runs
        }
        let mut b = bar();
        b.frame(&view(3), &live(0));
        let p = b.frame(&view(3), &live(10_000));
        // SAFETY: a popup that is never shown, destroyed again below.
        unsafe {
            let hw = CreateWindowExW(WINDOW_EX_STYLE(gfx::window_ex()), w!("STATIC"), None, WS_POPUP, 0, 0, 1, 1, None, None, None, None).expect("window");
            assert!(b.surfaces().0.present(hw, 0, 0, 255), "the frame did not reach the window's visual");
            gfx::shape(hw, b.shape());
            gfx::commit();
            assert_eq!(b.surfaces().0.size(), (p.win.2, p.win.3));
            assert!(b.surfaces().0.pixels().is_empty(), "a composed frame lives on the GPU, not in a DIB");
            // the GPU goes away: the next frame makes a device, surfaces and the window's visual anew, and the frame is shown again
            gfx::lose_device();
            assert!(!b.surfaces().0.is_current(), "the lost device took the pixels");
            b.frame(&view(3), &live(10_016));
            assert!(b.surfaces().0.is_current() && b.surfaces().0.present(hw, 0, 0, 255), "the frame after the loss did not reach the window");
            gfx::commit();
            let _ = DestroyWindow(hw);
        }
        gfx::compose_off();
    }

    #[test]
    fn the_region_is_the_bar_and_a_column_over_each_lifted_icon() {
        // a bottom bar: the 14 px strip above it gets a column per lifted icon, nothing else
        let r = bar_region((0, 14, 1440, 54), 16, 14, &[(100, 160), (-5, 30), (1400, 1500)]);
        assert_eq!(r[0], Rgn { x: 0, y: 14, w: 1440, h: 54, r: 16 });
        assert_eq!(&r[1..], &[Rgn { x: 100, y: 0, w: 60, h: 14, r: 0 }, Rgn { x: 0, y: 0, w: 30, h: 14, r: 0 }, Rgn { x: 1400, y: 0, w: 40, h: 14, r: 0 }]);
        // a side bar's window is the bar: no strip
        assert_eq!(bar_region((0, 0, 54, 900), 16, 0, &[(0, 40)]), vec![Rgn { x: 0, y: 0, w: 54, h: 900, r: 16 }]);
    }

    #[test]
    fn hovering_an_app_lifts_it_out_of_the_region_and_leaving_takes_it_back() {
        let mut b = bar();
        let v = view(8);
        b.frame(&v, &live(0));
        b.frame(&v, &live(10_000));
        let at_rest = b.shape().map(<[Rgn]>::to_vec).expect("a shape at rest");
        assert_eq!(at_rest, vec![Rgn { x: 0, y: 14, w: 1440, h: 54, r: 0 }], "a flat bar at rest: its rectangle, the strip above it clicks through");
        // the pointer on app 3: its icon jumps up, and the column over it (the button and 10 px each side) is part of the region
        let hot = Live { hot: 3, ..live(10_000) };
        b.frame(&v, &hot);
        b.frame(&v, &Live { now: 10_450, ..hot });
        let r = b.layout().rect_of(3).expect("app 3");
        let lifted = b.shape().map(<[Rgn]>::to_vec).expect("a shape");
        assert_eq!(lifted.len(), 2, "the bar and one column over app 3 and its neighbours (they zoom a little too, the columns touch): {lifted:?}");
        assert!(lifted.iter().any(|g| g.y == 0 && (g.x as f32) <= r.x - 10.0 && ((g.x + g.w) as f32) >= r.x + r.w + 10.0), "{lifted:?}");
        // away again: the columns are gone once the icons are home
        b.frame(&v, &Live { now: 10_450, ..live(10_450) });
        b.frame(&v, &live(11_800));
        assert_eq!(b.shape().map(<[Rgn]>::to_vec), Some(at_rest));
        // the dock's and the floating bar's corners are round
        let mut f = view(8);
        f.style = Style::Floating;
        b.frame(&f, &live(12_000));
        assert_eq!(b.shape(), None, "the theme is changing: the whole window until it has settled");
        b.frame(&f, &live(13_000));
        assert_eq!(b.shape().map(|s| s[0].r), Some(16));
    }

    #[test]
    fn shapes() {
        let s = shape(Style::Dock, Side::Bottom, 2);
        assert_eq!((s.thick, s.inset, s.radius, s.outer(), s.reserve()), (72.0, 10.0, 22.0, 82.0, 82.0));
        let s = shape(Style::Floating, Side::Left, 0);
        assert_eq!((s.thick, s.outer(), s.reserve(), s.vert, s.left), (46.0, 54.0, 54.0, true, true));
    }

    #[test]
    fn a_bar_on_another_monitor_holds_start_search_task_view_apps_and_the_clock_only() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let mut g = Gfx::new(1.0).expect("the renderer");
        let sh = shape(Style::Classic, Side::Bottom, 1);
        let mut v = view(8);
        v.battery = Some((80, false));
        v.rec = Some("0:24".into());
        v.tray = vec![HICON(std::ptr::null_mut()); 2];
        let full = layout(&mut g, &v, &sh, 54.0, Some(1920.0), 0.0);
        v.full = false;
        let light = layout(&mut g, &v, &sh, 54.0, Some(1920.0), 0.0);
        let kinds = |l: &Layout| l.items.iter().map(|i| i.kind).collect::<Vec<_>>();
        for gone in [Kind::Gear, Kind::Chevron, Kind::Tray, Kind::Lang, Kind::Qs, Kind::Restart, Kind::Power, Kind::Rec] {
            assert!(kinds(&full).contains(&gone), "the whole bar has {gone:?}");
            assert!(!kinds(&light).contains(&gone), "the lighter bar has no {gone:?}");
        }
        let want = [Kind::Search, Kind::TaskView, Kind::Start, Kind::Clock, Kind::ShowDesk];
        assert!(want.iter().all(|k| kinds(&light).contains(k)));
        assert_eq!(kinds(&light).iter().filter(|k| **k == Kind::App).count(), 8);
        // the clock is at the end of the bar like the whole bar's; the apps stay in the middle of the 1920
        let clock = light.rect_of(CLOCK).expect("the clock");
        assert!((clock.x + clock.w - 1910.0).abs() < 0.5, "{clock:?}");
        let (first, last) = (light.rect_of(SEARCH).expect("search"), light.rect_of(8).expect("app 8"));
        assert!(((first.x + last.x + last.w) / 2.0 - 960.0).abs() < 1.0, "the group is centred: {first:?} {last:?}");
        assert_eq!(light.hit(clock.cx(), clock.cy()), CLOCK);
        assert_eq!(light.hit(10.0, 27.0), NONE, "where the gear was there is nothing");
        // left-aligned like Windows 10: Start comes after a plain margin, there is no gear to make room for
        v.align_left = true;
        let left = layout(&mut g, &v, &sh, 54.0, Some(1920.0), 0.0);
        assert!(left.rect_of(SEARCH).expect("search").x < 30.0, "{:?}", left.rect_of(SEARCH));
        // the dock of a lighter bar is just as short as its content
        let dock = shape(Style::Dock, Side::Bottom, 1);
        v.align_left = false;
        let d = layout(&mut g, &v, &dock, 64.0, None, 0.0);
        v.full = true;
        let d_full = layout(&mut g, &v, &dock, 64.0, None, 0.0);
        assert!(d.bar.w < d_full.bar.w - 200.0, "{} vs {}", d.bar.w, d_full.bar.w);
    }

    #[test]
    fn a_bar_stands_where_its_monitor_is_on_the_virtual_screen() {
        let mut b = bar();
        let v = view(3);
        b.frame(&v, &live(0));
        let at_origin = b.frame(&v, &live(10_000));
        // the monitor to the left of the primary one: its corner is at negative x; windows are placed there, the picture is the same
        b.set_origin((-1440, 0));
        let moved = b.frame(&v, &live(10_000));
        assert_eq!(moved.bar, (-1440 + at_origin.bar.0, at_origin.bar.1, at_origin.bar.2, at_origin.bar.3));
        assert_eq!(moved.win, (-1440, at_origin.win.1, at_origin.win.2, at_origin.win.3));
        assert_eq!(moved.rest.0, -1440 + at_origin.rest.0);
        let r = b.screen_rect(0).expect("start");
        let r0 = (r.0 + 1440, r.1, r.2 + 1440, r.3);
        b.set_origin((0, 0));
        b.frame(&v, &live(10_000));
        assert_eq!(b.screen_rect(0), Some(r0), "buttons are found where the windows are");
        // a monitor below the primary one
        b.set_origin((100, 900));
        let below = b.frame(&v, &live(10_000));
        assert_eq!((below.bar.0, below.bar.1), (100 + at_origin.bar.0, 900 + at_origin.bar.1));
    }

    #[test]
    fn the_recording_button_stands_before_the_clock_and_takes_room_only_while_recording() {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let mut g = Gfx::new(1.0).expect("the renderer");
        let sh = shape(Style::Classic, Side::Bottom, 1);
        let mut v = view(3);
        let plain = layout(&mut g, &v, &sh, 54.0, Some(1440.0), 0.0);
        assert!(plain.rect_of(REC_I).is_none());
        v.rec = Some("0:24".into());
        let on = layout(&mut g, &v, &sh, 54.0, Some(1440.0), 0.0);
        let (rec, clock) = (on.rect_of(REC_I).expect("the button"), on.rect_of(CLOCK).expect("the clock"));
        assert_eq!(rec.h, 30.0, "`recbar` is 30 high");
        assert!((clock.x - (rec.x + rec.w) - 8.0).abs() < 0.01, "6 px of margin and the row's 2 px gap before the clock: {rec:?} {clock:?}");
        assert!(rec.w > 55.0 && rec.w < 70.0, "dot, time and padding: {}", rec.w);
        assert_eq!(on.hit(rec.cx(), rec.cy()), REC_I);
    }
}
