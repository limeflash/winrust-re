//! How WinRust Disk looks: the mockup's DiskMap.html, element by element (its CSS measures are in the comments). A frame draws the
//! title bar with the drives, the toolbar, the map or the list, the details panel and the status line, then the dialog and the toast.
use winrust_ui::{
    anim::{CubicBezier, EASE, LINEAR, SPRING},
    frame::{Frame, Id, WIN_CLOSE, WIN_MAX, WIN_MIN},
    gfx::{Align, Canvas, Font, Rect, Rgba, Xf},
    widgets::{
        list::{columns, list_with, ListLook},
        look::{self as ui, accent, focus_ring},
        scroll::{scroll, View},
    },
};

use crate::{
    app::*,
    backend::DriveInfo,
    delete::How,
    layout::Tile,
    look::{self, human, pct},
    model::{Category, AGG},
};
use winrust_common::{mft::ROOT, style};

const K_ZOOM: i32 = 9001;
const K_BUILD: i32 = 9002;
const K_TOAST_IN: i32 = 9003;
const K_TOAST_OUT: i32 = 9004;
const K_CARD: i32 = 9005;
const K_BAR: i32 = 9006;
const K_DLG: i32 = 9007;
const S_DIM: u8 = 15;

/// `cubic-bezier(.2,.8,.2,1)`: the zoom between levels and the tiles coming in.
const ZOOM_EASE: CubicBezier = CubicBezier::new(0.2, 0.8, 0.2, 1.0);
const ZOOM_MS: u32 = 420;
const BUILD_MS: f32 = 450.0;

fn font(size: f32, weight: u16) -> Font {
    Font::new(size, weight)
}

/// The title bar's 56 DIPs, the toolbar's 48, the status line's 32.
const BAR_H: f32 = 56.0;
const TOOL_H: f32 = 48.0;
const STATUS_H: f32 = 32.0;
const SIDE_W: f32 = 300.0;

/// A rectangle moved from the map's own coordinates to the window's.
fn at(map: Rect, r: Rect) -> Rect {
    r.shift(map.x, map.y)
}

/// A button's plate and its hover colour, the mockup's `hv`: `rgba(ink, a)` that grows to .08 under the pointer.
fn plate(c: &mut Canvas<'_>, r: Rect, radius: f32, base: f32, hover: f32) {
    c.fill(r, radius, Rgba::white(base + (0.08 - base).max(0.0) * hover));
}

fn ring_focus(c: &mut Canvas<'_>, r: Rect, radius: f32, on: bool) {
    if on {
        c.ring(r.inset(-3.0), radius + 3.0, 2.0, focus_ring().fade(0.9));
    }
}

impl Disk {
    pub fn paint(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (w, h) = f.size;
        if self.window() == 0 {
            self.take_events(); // no window, no message: what a worker has done is taken here
        }
        let map = Rect::new(16.0, BAR_H + TOOL_H, w - SIDE_W - 40.0, h - BAR_H - TOOL_H - STATUS_H - 12.0);
        let side = Rect::new(w - 12.0 - SIDE_W, BAR_H + TOOL_H, SIDE_W, map.h);
        self.ensure_layout((map.w, map.h));
        self.hot = f.hover.and_then(|id| self.record_of(id)).or_else(|| {
            let k = f.hover.filter(|id| (ID_TOP..ID_TOP + 5).contains(id))?;
            self.tops.get((k - ID_TOP) as usize).copied().filter(|r| self.layout.as_ref().is_some_and(|l| l.items.iter().any(|i| i.id == *r)))
        });
        let cats = self.cat_list();
        let hl = match f.hover {
            Some(id) if (ID_LEGEND..ID_LEGEND + cats.len() as Id).contains(&id) => Some(cats[(id - ID_LEGEND) as usize]),
            _ => self.pin_hl,
        };

        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, look::WIN);
        self.draw_map(c, f, map, hl);
        self.draw_side(c, f, side, &cats, hl);
        self.draw_status(c, f, w, h);
        self.draw_toolbar(c, f, w);
        self.draw_title(c, f, w);
        self.draw_dialog(c, f, w, h);
        self.draw_toast(c, f, w, h);
        if self.demo != DemoHook::None {
            f.item(ID_DEMO, Rect::new(0.0, 0.0, 1.0, 1.0));
        }
    }

    /// The categories with something in them, the biggest first: the legend.
    pub fn cat_list(&self) -> Vec<Category> {
        let Some(d) = self.data.as_ref().filter(|_| self.ready()) else { return Vec::new() };
        let mut v: Vec<Category> = Category::ALL.iter().copied().filter(|c| d.totals[*c as usize] > 0).collect();
        v.sort_by_key(|c| std::cmp::Reverse(d.totals[*c as usize]));
        v
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the title bar: the app, the drives, Rescan, the window's buttons

    fn draw_title(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32) {
        c.fill(Rect::new(0.0, 0.0, w, BAR_H), 0.0, look::BAR);
        c.fill(Rect::new(0.0, BAR_H, w, 1.0), 0.0, Rgba::white(0.06));
        // the tile: a 28 px gradient with three blocks in it (a 3fr 2fr grid, 5 px in, 2 px apart)
        let tile = Rect::new(16.0, 14.0, 28.0, 28.0);
        c.gradient_diagonal(tile, 8.0, Rgba::new(0x22 as f32 / 255.0, 0xc5 as f32 / 255.0, 0x5e as f32 / 255.0, 1.0), Rgba::new(0x0e as f32 / 255.0, 0x7a as f32 / 255.0, 0x43 as f32 / 255.0, 1.0));
        let (big, small) = (9.6, 6.4);
        c.fill(Rect::new(tile.x + 5.0, tile.y + 5.0, big, 18.0), 2.0, Rgba::pure_white(0.95));
        c.fill(Rect::new(tile.x + 5.0 + big + 2.0, tile.y + 5.0, small, big), 2.0, Rgba::pure_white(0.7));
        c.fill(Rect::new(tile.x + 5.0 + big + 2.0, tile.y + 5.0 + big + 2.0, small, small), 2.0, Rgba::pure_white(0.45));
        let tf = font(14.0, 600);
        let tw = c.measure("WinRust Disk", tf).ceil();
        c.text("WinRust Disk", Rect::new(56.0, 0.0, tw, BAR_H), tf, ui::T1, Align::Start);
        let sep1 = 56.0 + tw + 12.0 + 4.0;
        c.fill(Rect::new(sep1, 16.0, 1.0, 24.0), 0.0, Rgba::white(0.08));

        // the window's buttons (26, 8 apart, 12 from the edge), a hairline, Rescan
        let btns_x = w - 12.0 - 94.0;
        for (i, (id, d)) in [(WIN_MIN, look::ICON_MIN), (WIN_MAX, if f.maximized() { look::ICON_RESTORE } else { look::ICON_MAX }), (WIN_CLOSE, look::ICON_CLOSE)].into_iter().enumerate() {
            let b = Rect::new(btns_x + i as f32 * 34.0, BAR_H / 2.0 - 13.0, 26.0, 26.0);
            let hot = f.item(id, b);
            f.hand();
            let glide = f.tween(id, 2, f32::from(f.hover == Some(id)), 280, SPRING);
            let (bg, ink) = if id == WIN_CLOSE {
                (Rgba::white(0.07).mix(Rgba::css(229, 72, 77, 0.22), hot.hover), ui::T2.mix(look::RED, hot.hover))
            } else {
                (Rgba::white(0.07 + 0.09 * hot.hover), ui::T2.mix(ui::T1, hot.hover))
            };
            c.transformed(b.cx(), b.cy(), 1.0 - 0.14 * hot.press, 0.0, 0.0, |c| {
                c.fill(b, 13.0, bg);
                let (dy, scale, turn) = match id {
                    WIN_MIN => (2.0 * glide, 1.0, 0.0),
                    WIN_MAX => (0.0, 1.0 + 0.2 * glide, 0.0),
                    _ => (0.0, 1.0, 90.0 * glide),
                };
                c.icon(d, (b.cx(), b.cy() + dy), 12.0 * scale, 2.2, ink, turn);
            });
            ring_focus(c, b, 13.0, hot.focus);
        }
        let sep2 = btns_x - 12.0 - 4.0 - 1.0;
        c.fill(Rect::new(sep2, 18.0, 1.0, 20.0), 0.0, Rgba::white(0.08));
        let rf = font(13.0, 500);
        let rw = 10.0 + 15.0 + 7.0 + c.measure("Rescan", rf).ceil() + 12.0;
        let rr = Rect::new(sep2 - 4.0 - 12.0 - rw, 12.0, rw, 32.0);
        let hot = f.focusable(ID_RESCAN, rr);
        f.hand();
        f.tip("Scan again (F5)");
        plate(c, rr, 9.0, 0.06, hot.hover);
        c.icon(look::ICON_RESCAN, (rr.x + 10.0 + 7.5, rr.cy()), 15.0, 2.0, ui::T1, 0.0);
        c.text("Rescan", Rect::new(rr.x + 10.0 + 15.0 + 7.0, rr.y, rw, rr.h), rf, ui::T1, Align::Start);
        ring_focus(c, rr, 9.0, hot.focus);

        // the drives: 40 high, an icon, the name and what is free, a bar of what is used
        let mut x = sep1 + 1.0 + 4.0 + 12.0;
        let room = rr.x - 12.0 - x;
        let drives = self.drives.clone();
        f.clipped(c, Rect::new(x, 0.0, room.max(0.0), BAR_H), |c, f| {
            for (i, d) in drives.iter().enumerate() {
                x += self.draw_chip(c, f, i, d, x) + 4.0;
            }
        });
    }

    fn draw_chip(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, i: usize, d: &DriveInfo, x: f32) -> f32 {
        let on = i == self.drive;
        let name = format!("{}: {}", d.letter, d.label);
        let free = format!("{} free", human(d.free));
        let (nf, ff) = (font(12.0, 600), font(12.0, 400).tabular());
        let (nw, fw) = (c.measure(&name, nf).ceil(), c.measure(&free, ff).ceil());
        let col = (nw + 10.0 + fw).max(86.0);
        let w = 10.0 + 18.0 + 9.0 + col + 12.0;
        let r = Rect::new(x, 8.0, w, 40.0);
        let hot = f.focusable(ID_DRIVE + i as Id, r);
        f.hand();
        f.tip(format!("{} ({}:) · {} · {}", d.label, d.letter, d.fs, human(d.total)));
        let base = if on { 0.09 } else { 0.0 };
        c.fill(r, 11.0, Rgba::white(base + (0.07 - base) * hot.hover));
        if on {
            c.ring(r, 11.0, 1.0, Rgba::white(0.08));
        }
        let icon = if d.removable { look::ICON_USB } else { look::ICON_DRIVE };
        c.icon(icon, (r.x + 10.0 + 9.0, r.cy()), 18.0, 1.8, if on { ui::T1 } else { ui::T3 }, 0.0);
        let tx = r.x + 10.0 + 18.0 + 9.0;
        // two rows: the text (12 px, a line of 14.5) and the bar (3), 4 apart, in the middle of the 40
        let top = r.y + (40.0 - (14.5 + 4.0 + 3.0)) / 2.0;
        c.text(&name, Rect::new(tx, top, col, 14.5), nf, ui::T1, Align::Start);
        c.text(&free, Rect::new(tx, top, col, 14.5), ff, ui::T3, Align::End);
        let bar = Rect::new(tx, top + 14.5 + 4.0, col, 3.0);
        c.fill(bar, 2.0, Rgba::white(0.1));
        let used = if d.total == 0 { 0.0 } else { (1.0 - d.free as f32 / d.total as f32).clamp(0.0, 1.0) };
        let tone = if used > 0.9 { Rgba::css(229, 72, 77, 1.0) } else if used > 0.75 { Rgba::css(255, 163, 72, 1.0) } else { accent() };
        c.fill(Rect::new(bar.x, bar.y, (bar.w * used).max(2.0), 3.0), 2.0, tone);
        ring_focus(c, r, 11.0, hot.focus);
        w
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the toolbar: up, the path, the size of the folder, Map / List

    fn draw_toolbar(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32) {
        let y = BAR_H;
        // Up one level: 32 square, dim at the root
        let up = Rect::new(10.0, y + 8.0, 32.0, 32.0);
        let at_root = self.nav.at_root();
        let hot = f.focusable(ID_UP, up);
        if !at_root {
            f.hand();
        }
        f.tip("Up (Backspace)");
        let dim = if at_root { 0.35 } else { 1.0 };
        c.faded(dim, |c| {
            c.fill(up, 9.0, Rgba::white(0.08 * hot.hover));
            c.icon(look::ICON_UP, (up.cx(), up.cy()), 16.0, 2.2, ui::T2, 0.0);
        });
        ring_focus(c, up, 9.0, hot.focus);

        // the view switch, flush right: 2 px of well round two buttons
        let vf = font(12.0, 500);
        let labels = [("Map", look::ICON_MAP), ("List", look::ICON_LIST)];
        let widths: Vec<f32> = labels.iter().map(|(l, _)| 10.0 + 14.0 + 6.0 + c.measure(l, vf).ceil() + 10.0).collect();
        let total = 4.0 + widths.iter().sum::<f32>();
        let well = Rect::new(w - 16.0 - total, y + 8.0, total, 32.0);
        c.fill(well, 10.0, look::WELL);
        c.ring(well, 10.0, 1.0, Rgba::white(0.06));
        let mut bx = well.x + 2.0;
        for (k, ((label, icon), bw)) in labels.iter().zip(&widths).enumerate() {
            let b = Rect::new(bx, well.y + 2.0, *bw, 28.0);
            let on = (k == 0) == (self.view == ViewMode::Map);
            let hot = f.focusable(ID_VIEW + k as Id, b);
            f.hand();
            f.tip(*label);
            let t = f.tween(ID_VIEW + k as Id, 12, f32::from(on), 160, EASE);
            c.fill(b, 8.0, Rgba::white(0.12 * t));
            let ink = ui::T2.mix(ui::T1, t);
            c.icon(icon, (b.x + 10.0 + 7.0, b.cy()), 14.0, 2.0, ink, 0.0);
            c.text(label, Rect::new(b.x + 10.0 + 14.0 + 6.0, b.y, *bw, b.h), vf, ink, Align::Start);
            ring_focus(c, b, 8.0, hot.focus);
            bx += bw;
        }

        // the path: crumbs 30 high, a chevron between, and the folder's size in a pill after the last
        let nav = Rect::new(10.0 + 32.0 + 8.0, y, well.x - 8.0 - (10.0 + 32.0 + 8.0), TOOL_H);
        let path: Vec<u32> = self.nav.path().to_vec();
        let labels: Vec<String> = path
            .iter()
            .map(|&id| if id == ROOT { self.cur().map_or_else(String::new, |d| format!("{} ({}:)", d.label, d.letter)) } else { self.data.as_ref().map_or_else(String::new, |d| d.name(id).to_string()) })
            .collect();
        let size_text = if self.ready() { human(self.data.as_ref().map_or(0, |d| d.size(self.nav.current()))) } else if self.scanning() { "scanning…".into() } else { "not scanned".into() };
        f.clipped(c, nav, |c, f| {
            let mut x = nav.x;
            let n = labels.len();
            for (i, label) in labels.iter().enumerate() {
                if i > 0 {
                    x += 2.0;
                    c.icon(look::ICON_CHEVRON, (x + 6.0, y + TOOL_H / 2.0), 12.0, 2.4, ui::T4, 0.0);
                    x += 12.0 + 2.0;
                }
                let last = i + 1 == n;
                let cf = font(13.0, if last { 600 } else { 500 });
                let cw = c.measure(label, cf).ceil() + 16.0;
                let b = Rect::new(x, y + 9.0, cw, 30.0);
                let hot = f.focusable(ID_CRUMB + i as Id, b);
                if !last {
                    f.hand();
                }
                c.fill(b, 8.0, Rgba::white(0.08 * hot.hover));
                c.text(label, b, cf, if last { ui::T1 } else { ui::T2.mix(ui::T1, hot.hover) }, Align::Center);
                ring_focus(c, b, 8.0, hot.focus);
                x += cw;
            }
            let pf = font(12.0, 600).tabular();
            let pw = c.measure(&size_text, pf).ceil() + 16.0;
            let p = Rect::new(x + 10.0, y + (TOOL_H - 22.0) / 2.0, pw, 22.0);
            c.fill(p, 11.0, Rgba::white(0.07));
            c.text(&size_text, p, pf, ui::T2, Align::Center);
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the status line

    fn draw_status(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32, h: f32) {
        let y = h - STATUS_H;
        c.fill(Rect::new(0.0, y - 1.0, w, 1.0), 0.0, Rgba::white(0.05));
        let letter = self.letter();
        let (left, right) = if let Some(j) = self.busy_name() {
            (format!("Deleting {j}…"), String::new())
        } else if self.scanning() {
            (format!("Scanning {letter}:…"), String::new())
        } else if !self.ready() {
            (format!("{letter}: hasn’t been scanned yet"), String::new())
        } else {
            let d = self.data.as_ref();
            let hov = self.hot.and_then(|r| d.map(|d| (d.path(r), d.size(r), d.is_dir(r))));
            let left = match hov {
                Some((p, s, dir)) => format!("{p} — {}{}", human(s), if dir { " · click to open" } else { "" }),
                None => "Click a folder to open it · Backspace goes up · right-click for more · Tab switches drives".into(),
            };
            let files = d.map_or(0, |d| d.counts(ROOT).0);
            let right = match &self.scanned_in {
                Some(t) => format!("{} files · scanned in {t}", look::grouped(u64::from(files))),
                None => format!("{} files", look::grouped(u64::from(files))),
            };
            (left, right)
        };
        let sf = font(12.0, 400);
        let rw = if right.is_empty() { 0.0 } else { c.measure(&right, font(12.0, 400).tabular()).ceil() };
        let _ = f;
        c.text_fit(&left, Rect::new(16.0, y, (w - 32.0 - rw - 12.0).max(0.0), STATUS_H), sf, ui::T3, Align::Start);
        if rw > 0.0 {
            c.text(&right, Rect::new(w - 16.0 - rw, y, rw, STATUS_H), font(12.0, 400).tabular(), ui::T3, Align::End);
        }
    }

    fn busy_name(&self) -> Option<String> {
        self.busy().and_then(|id| self.data.as_ref().map(|d| d.name(id).to_string()))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the map, the list, and the cards that stand in their place

    fn draw_map(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, map: Rect, hl: Option<Category>) {
        c.fill(map, 14.0, look::MAP);
        c.ring(map, 14.0, 1.0, Rgba::white(0.05));
        f.item(ID_MAP, map);
        // a card comes in with `dk_card`: 8 px up, over 400 ms
        let kind: u8 = if let Some(b) = self.block() { 1 + u8::from(b == Block::Fs) } else if self.scanning() { 4 } else if self.empty_dir() { 5 } else { 0 };
        if kind != self.card_seen {
            f.tween(K_CARD, 0, 0.0, 0, LINEAR);
            self.card_seen = kind;
        }
        let card = if kind == 0 { 1.0 } else { f.tween(K_CARD, 0, 1.0, 400, SPRING) };
        f.clipped(c, map, |c, f| {
            if let Some(b) = self.block() {
                self.draw_block(c, f, map, &b, card);
            } else if self.scanning() {
                self.draw_scan(c, f, map, card);
            } else if self.empty_dir() {
                c.faded(card.max(0.004), |c| c.transformed(0.0, 0.0, 1.0, 0.0, 8.0 * (1.0 - card), |c| self.draw_empty(c, f, map)));
            } else if self.ready() {
                match self.view {
                    ViewMode::Map => self.draw_tiles(c, f, map, hl),
                    ViewMode::List => self.draw_list(c, f, map),
                }
            }
        });
    }

    /// The zoom between levels at this frame: progress, where a rectangle of the new level is put, and how opaque the level is.
    fn zoom_now(&mut self, f: &mut Frame<'_>, map: Rect) -> (f32, Aff, f32) {
        if self.zoom_seen != self.zoom_serial {
            f.tween(K_ZOOM, 0, 0.0, 0, LINEAR);
            self.zoom_seen = self.zoom_serial;
        }
        let Some(z) = self.zoom else { return (1.0, Aff::NONE, 1.0) };
        let e = f.tween(K_ZOOM, 0, 1.0, ZOOM_MS, ZOOM_EASE);
        if e >= 1.0 {
            self.zoom = None;
            return (1.0, Aff::NONE, 1.0);
        }
        let z = match z {
            Zoom::Out(id) if self.layout.as_ref().and_then(|l| l.tile(id)).is_none() => Zoom::Fade,
            z => z,
        };
        match z {
            Zoom::In(s) => (e, Aff::new(s.x, s.y, s.w / map.w, s.h / map.h, e), 0.35 + 0.65 * e),
            Zoom::Out(id) => {
                let s = self.layout.as_ref().and_then(|l| l.tile(id)).map(|t| t.slot).unwrap_or_default();
                let (sx, sy) = (map.w / s.w.max(1.0), map.h / s.h.max(1.0));
                (e, Aff::new(-s.x * sx, -s.y * sy, sx, sy, e), 0.35 + 0.65 * e)
            }
            Zoom::Fade => (e, Aff::new(0.0, 0.0, 0.985, 0.985, e), e),
        }
    }

    fn draw_tiles(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, map: Rect, hl: Option<Category>) {
        let (e, xf, group) = self.zoom_now(f, map);
        // the tiles of a fresh scan come in one after the other: `dk_build`, 450 ms each, 14 ms apart
        if self.fresh {
            f.tween(K_BUILD, 0, 0.0, 0, LINEAR);
            self.fresh = false;
            self.building = true;
        }
        let built = if self.building { f.tween(K_BUILD, 0, 1.0, 870, LINEAR) } else { 1.0 };
        if built >= 1.0 {
            self.building = false;
        }
        let Some(layout) = self.layout.take() else { return };
        let (sel, busy) = (self.sel, self.busy());
        let light = style::light();
        for (i, t) in layout.tiles.iter().enumerate() {
            let (id, agg) = (ID_TILE + i as Id, t.id == AGG);
            let r = at(map, xf.map(t.r));
            if r.w <= 0.5 || r.h <= 0.5 {
                continue;
            }
            let hot = f.item(id, r); // (the rectangle is where the tile is now, so a zoom does not stop the pointer)
            if !agg && t.dir {
                f.hand();
            }
            let dim = hl.is_some_and(|h| t.cat != h) || busy == Some(t.id);
            let op = f.tween(id, S_DIM, if busy == Some(t.id) { 0.35 } else if dim { 0.22 } else { 1.0 }, 250, EASE);
            let (mut o, mut k) = (group * op, 1.0);
            if self.building {
                let lt = ((built * 870.0 - (i as f32 * 14.0).min(420.0)) / BUILD_MS).clamp(0.0, 1.0);
                let b = ZOOM_EASE.ease(lt);
                (o, k) = (o * b, 0.9 + 0.1 * b);
            }
            let base = if agg {
                Rgba::TRANSPARENT
            } else {
                match t.bg.get() {
                    Some((l, c)) if l == light => c,
                    _ => {
                        let c = look::tile_bg(t.cat, t.frac);
                        t.bg.set(Some((light, c)));
                        c
                    }
                }
            };
            let bg = look::brighter(base, 1.0 + 0.16 * hot.hover);
            // (the tile's opacity is put into each colour: a layer for each of sixty tiles at every frame of a zoom leaks in Direct2D)
            c.transformed(r.cx(), r.cy(), k, 0.0, 0.0, |c| {
                if agg {
                    draw_hatch(c, r, t.radius, o);
                } else {
                    c.fill(r, t.radius, bg.fade(o));
                }
                let inside = |c: &mut Canvas<'_>| {
                    for s in &t.subs {
                        let sr = at(map, xf.map(Rect::new(t.r.x + s.r.x, t.r.y + s.r.y, s.r.w, s.r.h)));
                        let a = if light { 0.22 + 0.4 * s.k } else { 0.035 + 0.1 * s.k };
                        c.fill(sr, s.radius, Rgba::pure_white(a).fade(o));
                        c.ring(sr, s.radius, 1.0, (if light { Rgba::new(20.0 / 255.0, 22.0 / 255.0, 30.0 / 255.0, 0.06) } else { Rgba::pure_white(0.06) }).fade(o));
                        if s.label && sr.w > 40.0 {
                            // (the width of a text is the tile's own, not the zoom's: a layout for each width of each frame fills the text cache)
                            c.text_fit(&s.name, Rect::new(sr.x + 8.0, sr.y + 6.0, (s.r.w - 14.0).max(0.0), 14.0), font(11.0, 500), look::tile_text2().fade(e * e * o), Align::Start);
                        }
                    }
                    if t.label {
                        draw_tile_label(c, Rect::new(r.x, r.y, t.r.w, t.r.h), t, e * e * o);
                    }
                };
                // (a clip is for what can run over the edge: a tile with no label and no parts has nothing to clip)
                if t.label || !t.subs.is_empty() {
                    c.clipped(r, inside);
                }
                if sel == Some(t.id) {
                    c.ring(r, t.radius, 2.0, accent().fade(o));
                    c.ring(r.inset(-1.0), t.radius + 1.0, 1.0, Rgba::new(0.0, 0.0, 0.0, 0.3).fade(o));
                } else if hot.hover > 0.0 {
                    c.ring(r, t.radius, 1.5, Rgba::pure_white(0.75 * hot.hover).fade(o));
                }
            });
        }
        self.layout = Some(layout);
        // frames go on while a zoom or the build is moving; the tweens above are what asks for them
        let _ = (hl, light);
    }

    fn draw_list(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, map: Rect) {
        let Some(layout) = self.layout.take() else { return };
        let head = Rect::new(map.x + 6.0, map.y + 6.0, map.w - 12.0, 32.0);
        columns(c, f, ID_COLS, head, &self.cols, 4.0);
        let body = Rect::new(map.x, head.y + head.h, map.w, map.h - 6.0 - head.h);
        let look = ListLook { gap: 0.0, radius: 10.0, inset: 6.0, top: 0.0, bottom: 6.0 };
        // the list keeps the selection the window has
        self.list.sel.clear();
        if let Some(i) = self.sel.and_then(|s| self.order.iter().position(|&k| layout.items.get(k).is_some_and(|it| it.id == s))) {
            self.list.sel.insert(i);
        }
        let lay = self.cols.layout(head.w - 8.0);
        let top = layout.items.first().map_or(1, |i| i.bytes).max(1);
        let (order, size) = (&self.order, layout.size);
        let n = order.len();
        list_with(c, f, ID_LIST, body, &mut self.list, n, ROW_H, &look, |c, f, i, row, rs| {
            let it = &layout.items[order[i]];
            let cell = |k: usize| Rect::new(head.x + 4.0 + lay[k].0 + 8.0, row.y, (lay[k].1 - 16.0).max(0.0), row.h);
            let sw = look::rgb(look::cat_color(it.cat));
            // name: the category's swatch, a folder's icon, the name
            let n0 = cell(0);
            c.fill(Rect::new(n0.x, row.cy() - 6.0, 12.0, 12.0), 3.0, sw);
            let mut x = n0.x + 12.0 + 10.0;
            if it.dir {
                c.glyph(look::ICON_FOLDER_FILL, (x + 7.5, row.cy()), 15.0, look::rgb(look::FOLDER_YELLOW));
                x += 15.0 + 10.0;
            }
            c.text_fit(&it.name, Rect::new(x, row.y, (n0.x + n0.w - x).max(0.0), row.h), font(13.0, 500), ui::T1, Align::Start);
            // share: a bar and its percentage
            let s0 = cell(1);
            let bar_w = (s0.w - 8.0 - 40.0).max(0.0);
            let bar = Rect::new(s0.x, row.cy() - 3.0, bar_w, 6.0);
            c.fill(bar, 3.0, Rgba::white(0.08));
            c.fill(Rect::new(bar.x, bar.y, (bar_w * it.bytes as f32 / top as f32).max(0.0), 6.0), 3.0, sw);
            c.text(&pct(it.bytes, size), Rect::new(bar.x + bar_w + 8.0, row.y, 40.0, row.h), font(12.0, 400).tabular(), ui::T3, Align::End);
            // size and items
            let z = cell(2);
            c.text(&human(it.bytes), z, font(13.0, 500).tabular(), ui::T1, Align::End);
            let q = cell(3);
            let items = it.files.map_or_else(|| "—".to_string(), |n| look::grouped(u64::from(n)));
            c.text(&items, q, font(12.0, 400).tabular(), ui::T3, Align::End);
            let _ = (f, rs);
        });
        self.layout = Some(layout);
    }

    fn draw_empty(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, map: Rect) {
        // an icon, a line, a button; 8 apart
        let (bw, bh) = (c.measure("Go up", font(13.0, 500)).ceil() + 24.0, 32.0);
        let total = 30.0 + 8.0 + 16.0 + 8.0 + 6.0 + bh;
        let mut y = map.cy() - total / 2.0;
        c.icon(look::ICON_FOLDER, (map.cx(), y + 15.0), 30.0, 1.6, ui::T3, 0.0);
        y += 30.0 + 8.0;
        c.text("This folder is empty", Rect::new(map.x, y, map.w, 16.0), font(13.0, 500), ui::T1, Align::Center);
        y += 16.0 + 8.0 + 6.0;
        let b = Rect::new(map.cx() - bw / 2.0, y, bw, bh);
        let hot = f.focusable(ID_EMPTY_UP, b);
        f.hand();
        c.fill(b, 9.0, Rgba::white(0.08 + 0.05 * hot.hover));
        c.text("Go up", b, font(13.0, 500), ui::T1, Align::Center);
        ring_focus(c, b, 9.0, hot.focus);
    }

    /// The cards of "Needs administrator" and "Not NTFS": an icon, what is wrong, the way out.
    fn draw_block(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, map: Rect, b: &Block, card: f32) {
        let letter = self.letter();
        let d = self.cur().cloned();
        let (icon, tint, title, body, cta, alt, shield) = match b {
            Block::Admin => (
                look::ICON_SHIELD_BANG,
                Some(look::rgb(look::BLOCK_ORANGE)),
                "Administrator rights needed".to_string(),
                "WinRust Disk reads the drive’s file table directly, which is why a full scan takes seconds. Windows allows that only for administrators.".to_string(),
                "Restart as administrator",
                Some("Scan slowly instead"),
                true,
            ),
            Block::Fs => (
                look::ICON_USB_BANG,
                None,
                format!("{letter}: uses {}", d.as_ref().map_or("this file system", |d| d.fs.as_str())),
                format!("Fast scanning reads the NTFS file table. This drive can still be scanned file by file — it takes about a minute for {}.", human(d.as_ref().map_or(0, |d| d.total))),
                "Scan slowly",
                None,
                false,
            ),
            Block::Failed(e) => (look::ICON_SHIELD_BANG, Some(Rgba::css(229, 72, 77, 1.0)), format!("Couldn’t read {letter}:"), e.clone(), "Try again", None, false),
        };
        let (bf, tf, mf) = (font(13.0, 600), font(17.0, 600), font(13.0, 400));
        let width = 440.0_f32.min(map.w - 120.0).max(200.0);
        let lines = c.lines(&body, mf, width).max(1) as f32;
        let (tl, line) = (20.5, 13.0 * 1.55);
        let total = 64.0 + 14.0 + (tl + 6.0 + lines * line) + 14.0 + 4.0 + 36.0;
        let mut y = map.cy() - total / 2.0;
        c.faded(card.max(0.004), |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, 8.0 * (1.0 - card), |c| {
                let ic = Rect::new(map.cx() - 32.0, y, 64.0, 64.0);
                let tone = tint.unwrap_or(ui::T2);
                c.fill(ic, 18.0, if tint.is_some() { tone.alpha(0.18) } else { Rgba::white(0.07) });
                c.icon(icon, (ic.cx(), ic.cy()), 28.0, 1.8, tone, 0.0);
                y += 64.0 + 14.0;
                c.text(&title, Rect::new(map.x, y, map.w, tl), tf, ui::T1, Align::Center);
                y += tl + 6.0;
                let br = Rect::new(map.cx() - width / 2.0, y, width, lines * line);
                for (n, l) in wrap(c, &body, mf, width).iter().enumerate() {
                    c.text(l, Rect::new(br.x, br.y + n as f32 * line, br.w, line), mf, ui::T2, Align::Center);
                }
                y += lines * line + 14.0 + 4.0;
                let cw = c.measure(cta, bf).ceil() + 32.0 + if shield { 15.0 + 8.0 } else { 0.0 };
                let aw = alt.map_or(0.0, |a| c.measure(a, font(13.0, 500)).ceil() + 28.0);
                let row = cw + if alt.is_some() { aw + 8.0 } else { 0.0 };
                let mut x = map.cx() - row / 2.0;
                if let Some(a) = alt {
                    let r = Rect::new(x, y, aw, 36.0);
                    let hot = f.focusable(ID_BLOCK_ALT, r);
                    f.hand();
                    c.fill(r, 10.0, Rgba::white(0.08 + 0.05 * hot.hover));
                    c.text(a, r, font(13.0, 500), ui::T1, Align::Center);
                    ring_focus(c, r, 10.0, hot.focus);
                    x += aw + 8.0;
                }
                let r = Rect::new(x, y, cw, 36.0);
                let hot = f.focusable(ID_BLOCK_RUN, r);
                f.hand();
                c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                    c.fill(r, 10.0, accent());
                    if hot.hover > 0.0 {
                        c.fill(r, 10.0, Rgba::pure_white(0.12 * hot.hover));
                    }
                    let mut tx = r.x + 16.0;
                    if shield {
                        c.icon(look::ICON_SHIELD, (tx + 7.5, r.cy()), 15.0, 2.0, ui::ON_ACCENT, 0.0);
                        tx += 15.0 + 8.0;
                    }
                    c.text(cta, Rect::new(tx, r.y, cw, r.h), bf, ui::ON_ACCENT, Align::Start);
                });
                ring_focus(c, r, 10.0, hot.focus);
            });
        });
    }

    /// The skeleton of a map and the card over it: "Reading the file table of C:…", a bar, how many records of how many.
    fn draw_scan(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, map: Rect, card: f32) {
        let Phase::Scanning(s) = &self.phase else { return };
        let s = s.clone();
        let letter = self.letter();
        let fs = self.cur().map_or(String::new(), |d| d.fs.clone());
        // the skeleton: ten tiles of made-up sizes that breathe (`dk_skel`, 1.4 s, each 90 ms after the one before)
        let sizes = [30u64, 18, 14, 10, 8, 6, 5, 4, 3, 2];
        let items: Vec<(u32, u64)> = sizes.iter().enumerate().map(|(i, &v)| (i as u32, v)).collect();
        for (i, [x0, y0, x1, y1]) in crate::model::squarify(&items, (8.0, 8.0, f64::from(map.w - 16.0), f64::from(map.h - 16.0))) {
            let r = Rect::new(map.x + x0 as f32 + 1.5, map.y + y0 as f32 + 1.5, (x1 - x0) as f32 - 3.0, (y1 - y0) as f32 - 3.0);
            let phase = ((f.now as f32 / 1000.0 - i as f32 * 0.09) / 1.4).rem_euclid(1.0);
            let k = 0.5 + 0.4 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos());
            c.fill(r, 8.0, Rgba::white(0.06 * k));
        }
        // the pulse needs frames: 30 a second while the scan lasts
        f.due(f.now + 33);

        let p = if s.total == 0 { 0.0 } else { (s.read as f32 / s.total as f32).clamp(0.0, 1.0) };
        let shown = f.tween(K_BAR, 0, p, 200, LINEAR);
        let title = if s.slow { format!("Scanning {letter}: file by file…") } else { format!("Reading the file table of {letter}:…") };
        let sub = if s.slow { format!("{fs} drive · this takes a bit longer") } else { "NTFS · every file is counted, nothing is opened".to_string() };
        let recs = format!("{} of {} {}", look::grouped(s.read), look::grouped(s.total), if s.slow { "files" } else { "records" });
        let eta = match s.eta {
            Some(n) => format!("about {n} s left"),
            None if p >= 0.98 => "finishing".to_string(),
            None if p < 0.03 => "estimating…".to_string(),
            None => format!("about {} s left", ((s.since.elapsed().as_secs_f32() * (1.0 - p) / p).round() as u32).max(1)),
        };
        let r = Rect::new((map.cx() - 190.0).round(), (map.cy() - 62.0).round(), 380.0, 124.0);
        c.faded(card, |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, 8.0 * (1.0 - card), |c| {
                c.outside(r, 18.0, |c| c.shadow(r, 18.0, 24.0, 60.0, Rgba::css(0, 0, 0, 0.4)));
                c.fill(r, 18.0, look::MENU);
                c.ring(r.inset(-1.0), 19.0, 1.0, Rgba::css(0, 0, 0, 0.25));
                c.ring(r, 18.0, 1.0, Rgba::white(0.08));
                let (x, w) = (r.x + 22.0, r.w - 44.0);
                let mut y = r.y + 20.0;
                c.text(&title, Rect::new(x, y, w, 19.0), font(15.0, 600), ui::T1, Align::Start);
                c.text(&format!("{}%", (shown * 100.0).round()), Rect::new(x, y, w, 19.0), font(13.0, 600).tabular(), accent().accent_text(), Align::End);
                y += 19.0 + 6.0;
                c.text(&sub, Rect::new(x, y, w, 15.0), font(12.5, 400), ui::T2, Align::Start);
                y += 15.0 + 14.0;
                let bar = Rect::new(x, y, w, 6.0);
                c.fill(bar, 3.0, Rgba::white(0.08));
                c.fill(Rect::new(bar.x, bar.y, bar.w * shown, 6.0), 3.0, accent());
                y += 6.0 + 10.0;
                c.text(&recs, Rect::new(x, y, w, 15.0), font(12.0, 400).tabular(), ui::T3, Align::Start);
                c.text(&eta, Rect::new(x, y, w, 15.0), font(12.0, 400).tabular(), ui::T3, Align::End);
            });
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the details panel

    fn draw_side(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect, cats: &[Category], hl: Option<Category>) {
        c.fill(r, 14.0, look::SIDE);
        c.ring(r, 14.0, 1.0, Rgba::white(0.05));
        let content_h = self.side_h.max(r.h);
        let mut used_h = 0.0;
        // the scroll state is out of `self` while the content is drawn, which needs the rest of it
        let mut sc = std::mem::take(&mut self.side);
        scroll(c, f, ID_SIDE, r, &mut sc, content_h, |c, f, v| used_h = self.side_content(c, f, v, cats, hl));
        self.side = sc;
        // the height is known only once drawn: a frame more when it changed
        if (used_h - self.side_h).abs() > 0.5 {
            self.side_h = used_h;
            f.due(f.now);
        }
    }

    /// The panel's content from its top (16 in): the drive, the item in focus, the biggest of the level. Returns its height.
    fn side_content(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, v: &View, cats: &[Category], hl: Option<Category>) -> f32 {
        let (x, w) = (v.r.x + 16.0, v.r.w - 32.0);
        let mut y = 16.0;
        let ready = self.ready();
        let d = self.cur().cloned();
        let total = d.as_ref().map_or(0, |d| d.total);
        let used = self.data.as_ref().map_or(0, |d| d.used());
        let label = d.as_ref().map_or(String::new(), |d| format!("{} ({}:)", d.label, d.letter));

        // the drive: its name and how full it is, a bar split by category, the legend
        c.text(&label, Rect::new(x, v.at(y), w, 16.0), font(13.0, 600), ui::T1, Align::Start);
        let ut = if self.data.is_some() { format!("{} of {}", human(used), human(total)) } else { format!("{} of {}", human(total.saturating_sub(d.as_ref().map_or(0, |d| d.free))), human(total)) };
        c.text(&ut, Rect::new(x, v.at(y), w, 16.0), font(12.0, 400).tabular(), ui::T3, Align::End);
        y += 16.0 + 10.0;
        let bar = Rect::new(x, v.at(y), w, 10.0);
        c.fill(bar, 5.0, Rgba::white(0.08));
        let data_totals = self.data.as_ref().map(|d| d.totals);
        c.masked(bar, 5.0, Xf::NONE, |c| {
            let mut sx = bar.x;
            match (ready, data_totals) {
                (true, Some(t)) => {
                    for cat in cats {
                        let sw = t[*cat as usize] as f32 / total.max(1) as f32 * w;
                        let dimmed = hl.is_some_and(|h| h != *cat);
                        let a = f.tween(ID_LEGEND + 50 + *cat as Id, 13, if dimmed { 0.25 } else { 1.0 }, 200, EASE);
                        c.fill(Rect::new(sx, bar.y, sw, 10.0), 0.0, look::rgb(look::cat_color(*cat)).fade(a));
                        sx += sw + 2.0;
                    }
                }
                _ => {
                    let sw = if total == 0 { 0.0 } else { (used.max(d.as_ref().map_or(0, |d| total.saturating_sub(d.free))) as f32 / total as f32 * w).min(w) };
                    c.fill(Rect::new(bar.x, bar.y, sw, 10.0), 0.0, Rgba::white(0.28));
                }
            }
        });
        y += 10.0 + 10.0;
        if ready {
            let free = d.as_ref().map_or(0, |d| d.free);
            let lf = font(12.0, 400);
            let sf = font(12.0, 500).tabular();
            let mut entries: Vec<(String, Rgba, String, bool)> = cats.iter().map(|c0| (c0.label().to_string(), look::rgb(look::cat_color(*c0)), human(self.data.as_ref().map_or(0, |d| d.totals[*c0 as usize])), true)).collect();
            entries.push(("Free".into(), Rgba::white(0.14), human(free), false));
            // wrapped, 24 high, 2 apart, 6 in from the edges of the 280 they stand in
            let (mut lx, mut ly) = (x - 6.0, y);
            for (k, (name, tone, size, is_cat)) in entries.iter().enumerate() {
                let lw = c.measure(name, lf).ceil();
                let iw = 6.0 + 8.0 + 6.0 + lw + 6.0 + c.measure(size, sf).ceil() + 6.0;
                if lx + iw > x - 6.0 + w + 12.0 + 0.5 && lx > x - 6.0 {
                    lx = x - 6.0;
                    ly += 24.0 + 2.0;
                }
                let b = Rect::new(lx, v.at(ly), iw, 24.0);
                let hot = f.item(ID_LEGEND + k as Id, b);
                let dim = if *is_cat { hl.is_some_and(|h| Some(h) != cats.get(k).copied()) } else { hl.is_some() };
                let op = f.tween(ID_LEGEND + k as Id, 13, if dim { 0.5 } else { 1.0 }, 200, EASE);
                c.fill(b, 7.0, Rgba::white(0.07 * hot.hover));
                c.fill(Rect::new(b.x + 6.0, b.cy() - 4.0, 8.0, 8.0), 2.5, tone.fade(op));
                c.text(name, Rect::new(b.x + 6.0 + 8.0 + 6.0, b.y, iw, b.h), lf, ui::T1.fade(op), Align::Start);
                c.text(size, Rect::new(b.x + 6.0 + 8.0 + 6.0 + lw + 6.0, b.y, iw, b.h), sf, ui::T3.fade(op), Align::Start);
                lx += iw + 2.0;
            }
            y = ly + 24.0;
        } else {
            let note = if self.scanning() { "Sorting by type as the scan goes…" } else { "Scan the drive to see what takes the space." };
            let nf = font(12.5, 400);
            let lines = c.lines(note, nf, w).max(1);
            for (i, l) in wrap(c, note, nf, w).iter().enumerate() {
                c.text(l, Rect::new(x, v.at(y + i as f32 * 18.75), w, 18.75), nf, ui::T3, Align::Start);
            }
            y += lines as f32 * 18.75;
        }
        if !ready {
            return y + 16.0;
        }

        // the item in focus (the pointer's, else the selected one, else the folder)
        let focus = self.focus(self.hot);
        y += 18.0;
        c.fill(Rect::new(x, v.at(y), w, 1.0), 0.0, Rgba::white(0.07));
        y += 1.0 + 16.0;
        let Some(data) = self.data.as_ref() else { return y };
        let cur = self.nav.current();
        let id = focus.unwrap_or(cur);
        let (is_file, size, name, path) = {
            let at_root = focus.is_none() && cur == ROOT;
            (
                focus.is_some() && !data.is_dir(id),
                if id == ROOT { data.used() } else { data.size(id) },
                if at_root { label.clone() } else { data.name(id).to_string() },
                if id == ROOT { format!("{}:\\", self.letter()) } else { data.path(id) },
            )
        };
        let cat = data.category(id);
        let at_root = focus.is_none() && cur == ROOT;
        let tile_bg = if focus.is_some() { look::dark_of(look::rgb(look::cat_color(cat)), 0.8) } else { look::dark_of(accent(), 0.7) };
        let nf = font(13.0, 600);
        let name_lines = c.lines(&name, nf, w - 36.0 - 10.0).max(1) as f32;
        let hdr = (name_lines * 16.9 + 2.0 + 14.5).max(36.0);
        let ic = Rect::new(x, v.at(y), 36.0, 36.0);
        c.fill(ic, 10.0, tile_bg);
        let icon = if is_file { look::ICON_FILE } else if at_root { if d.as_ref().is_some_and(|d| d.removable) { look::ICON_USB } else { look::ICON_DRIVE } } else { look::ICON_FOLDER };
        c.icon(icon, (ic.cx(), ic.cy()), 18.0, 1.9, Rgba::pure_white(1.0), 0.0);
        let tx = x + 36.0 + 10.0;
        for (n, l) in wrap(c, &name, nf, w - 46.0).iter().enumerate() {
            c.text(l, Rect::new(tx, v.at(y + n as f32 * 16.9), w - 46.0, 16.9), nf, ui::T1, Align::Start);
        }
        c.text_fit(&path, Rect::new(tx, v.at(y + name_lines * 16.9 + 2.0), w - 46.0, 14.5), font(12.0, 400), ui::T3, Align::Start);
        y += hdr + 12.0;
        // the size, big, and what share it is
        let share = if at_root { format!("{}% full · {}", (used as f64 / total.max(1) as f64 * 100.0).round(), human(total)) } else { format!("{} of {}:", pct(size, used), self.letter()) };
        let bigf = font(26.0, 600).tracking(-0.02).tabular();
        let bw = c.measure(&human(size), bigf).ceil();
        c.text(&human(size), Rect::new(x, v.at(y), bw + 2.0, 31.5), bigf, ui::T1, Align::Start);
        c.text(&share, Rect::new(x + bw + 8.0, v.at(y + 4.0), w - bw - 8.0, 31.5), font(12.5, 400), ui::T3, Align::Start);
        y += 31.5 + 12.0;
        // three facts
        let parent_path = if id == ROOT { String::new() } else { data.path(data.tree.recs.get(&id).map_or(ROOT, |r| r.parent)) };
        let (fc, dc) = data.counts(id);
        let facts: [(&str, String); 3] = if is_file {
            [("Type", format!("{} file", name.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_uppercase()))), ("Category", cat.label().to_string()), ("In", parent_path)]
        } else {
            [("Files", look::grouped(u64::from(fc))), ("Folders", look::grouped(u64::from(dc))), ("Biggest type", cat.label().to_string())]
        };
        for (k, val) in &facts {
            c.text(k, Rect::new(x, v.at(y), w, 15.1), font(12.5, 400), ui::T3, Align::Start);
            let vw = (w - c.measure(k, font(12.5, 400)).ceil() - 10.0).max(0.0);
            c.text_fit(val, Rect::new(x + w - vw, v.at(y), vw, 15.1), font(12.5, 400).tabular(), ui::T1, Align::End);
            y += 15.1 + 7.0;
        }
        y += 12.0 - 7.0;
        // the actions: Open and Show and Delete for an item, Show for the folder
        let acts: Vec<Act> = match focus {
            Some(fid) if is_file => vec![("Show", look::ICON_FOLDER, A_SHOW, Some(fid), false, "Show in the file manager"), ("Delete…", look::ICON_TRASH, A_DELETE, Some(fid), true, "Delete (Del)")],
            Some(fid) => vec![
                ("Open", look::ICON_OPEN, A_OPEN, Some(fid), false, "Open this folder"),
                ("Show", look::ICON_FOLDER, A_SHOW, Some(fid), false, "Show in the file manager"),
                ("Delete…", look::ICON_TRASH, A_DELETE, Some(fid), true, "Delete (Del)"),
            ],
            None => vec![("Show in file manager", look::ICON_FOLDER, A_SHOW, None, false, "Open this folder in the file manager")],
        };
        self.acts = acts.iter().map(|a| (a.2, a.3)).collect();
        let flex: Vec<f32> = acts.iter().map(|a| if acts.len() == 1 { 1.0 } else if a.4 { 1.2 } else { 1.0 }).collect();
        let room = w - 6.0 * (acts.len() as f32 - 1.0);
        let mut ax = x;
        for (k, ((label, icon, _, _, danger, tip), fl)) in acts.iter().zip(&flex).enumerate() {
            let bw = room * fl / flex.iter().sum::<f32>();
            let b = Rect::new(ax, v.at(y), bw, 32.0);
            let hot = f.focusable(ID_ACT + k as Id, b);
            f.hand();
            f.tip(*tip);
            c.transformed(b.cx(), b.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                let (bg, ink) = if *danger {
                    (look::red_bg(), look::RED)
                } else if k == 0 {
                    (accent().alpha(0.22), ui::T1)
                } else {
                    (Rgba::white(0.07), ui::T1)
                };
                c.fill(b, 9.0, bg);
                if hot.hover > 0.0 {
                    c.fill(b, 9.0, Rgba::white(0.06 * hot.hover));
                }
                let lw = c.measure(label, font(12.5, 600)).ceil();
                let gx = b.cx() - (14.0 + 6.0 + lw) / 2.0;
                c.icon(icon, (gx + 7.0, b.cy()), 14.0, 2.0, ink, 0.0);
                c.text(label, Rect::new(gx + 20.0, b.y, lw + 4.0, b.h), font(12.5, 600), ink, Align::Start);
            });
            ring_focus(c, b, 9.0, hot.focus);
            ax += bw + 6.0;
        }
        y += 32.0;

        // the biggest inside: of the folder in focus, else of this one
        let inside = match focus.filter(|&fid| data.is_dir(fid)) {
            Some(fid) => data.largest(fid, 5),
            None => data.largest(cur, 5),
        };
        self.tops = inside.iter().map(|t| t.0).collect();
        if !inside.is_empty() {
            y += 18.0;
            c.fill(Rect::new(x, v.at(y), w, 1.0), 0.0, Rgba::white(0.07));
            y += 1.0 + 16.0;
            let title = match focus.filter(|&fid| data.is_dir(fid)) {
                Some(fid) => format!("Biggest inside {}", data.name(fid)),
                None => "Biggest here".to_string(),
            };
            c.text(&title, Rect::new(x, v.at(y), w, 14.5), font(12.0, 600), ui::T3, Align::Start);
            y += 14.5 + 4.0 + 4.0;
            let max = inside[0].1.max(1);
            for (k, (tid, bytes)) in inside.iter().enumerate() {
                let b = Rect::new(x - 6.0, v.at(y), w + 12.0, 36.0);
                let hot = f.item(ID_TOP + k as Id, b);
                f.hand();
                c.fill(b, 8.0, Rgba::white(0.07 * hot.hover));
                c.text_fit(data.name(*tid), Rect::new(x, b.y + 6.0, w - 70.0, 15.0), font(12.5, 500), ui::T1, Align::Start);
                c.text(&human(*bytes), Rect::new(x, b.y + 6.0, w, 15.0), font(12.5, 400).tabular(), ui::T2, Align::End);
                let bar = Rect::new(x, b.y + 6.0 + 15.0 + 5.0, w, 4.0);
                c.fill(bar, 2.0, Rgba::white(0.07));
                c.fill(Rect::new(bar.x, bar.y, bar.w * *bytes as f32 / max as f32, 4.0), 2.0, look::rgb(look::cat_color(data.category(*tid))));
                y += 36.0 + 4.0;
            }
            y -= 4.0;
        }
        y + 16.0
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the dialog and the toast, over everything

    fn draw_dialog(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32, h: f32) {
        let open = self.dlg.is_some();
        if open != self.dlg_seen {
            f.tween(K_DLG, 0, 0.0, 0, LINEAR);
            self.dlg_seen = open;
        }
        let Some(d) = self.dlg.as_ref() else { return };
        let e = f.tween(K_DLG, 0, 1.0, 260, SPRING);
        let scrim = f.tween(K_DLG, 1, 1.0, 200, EASE);
        // the scrim takes every click behind it, and a click on it is Cancel
        f.item(ID_DLG_SCRIM, Rect::new(0.0, 0.0, w, h));
        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, Rgba::css(5, 7, 12, 0.45 * scrim));

        let (tf, sf, lf, of) = (font(15.0, 600), font(12.5, 400), font(13.0, 600), font(12.0, 400));
        let title = format!("Delete “{}”?", d.name);
        let sub = format!("{}{} · {}", human(d.size), if d.dir { " · folder with everything inside" } else { "" }, d.parent);
        let (inner, head_w) = (376.0, 376.0 - 42.0 - 14.0);
        let tl = c.lines(&title, tf, head_w).max(1) as f32;
        let sl = c.lines(&sub, sf, head_w).max(1) as f32;
        let head = (tl * 20.25 + 4.0 + sl * 15.1).max(42.0);
        let opts = [
            (How::Permanently, "Delete permanently".to_string(), format!("Frees {} right away. This can’t be undone.", human(d.size))),
            (How::RecycleBin, "Move to the Recycle Bin".to_string(), "You can restore it later, but the space stays taken until you empty the bin.".to_string()),
        ];
        let heights: Vec<f32> = opts.iter().map(|o| 22.0 + 15.7 + 2.0 + c.lines(&o.2, of, inner - 24.0 - 26.0) as f32 * 17.4).collect();
        let total = 22.0 + head + 18.0 + heights.iter().sum::<f32>() + 6.0 + 18.0 + 36.0 + 22.0;
        let r = Rect::new(((w - 420.0) / 2.0).round(), ((h - total) / 2.0).round(), 420.0, total);
        // (a layer of no opacity is not drawn at all, and its items would not be named: the first frame must name them)
        c.faded(e.max(0.004), |c| {
            c.transformed(r.cx(), r.cy(), 0.96 + 0.04 * e, 0.0, -0.02 * r.h * (1.0 - e), |c| {
                c.outside(r, 18.0, |c| c.shadow(r, 18.0, 30.0, 80.0, Rgba::css(0, 0, 0, 0.55)));
                c.fill(r, 18.0, look::MENU);
                c.ring(r.inset(-1.0), 19.0, 1.0, Rgba::css(0, 0, 0, 0.4));
                c.ring(r, 18.0, 1.0, Rgba::white(0.09));
                f.item(ID_DLG_SCRIM + 1, r); // a click on the card is not a click on the scrim
                let (x, mut y) = (r.x + 22.0, r.y + 22.0);
                let ic = Rect::new(x, y, 42.0, 42.0);
                c.fill(ic, 12.0, look::red_bg());
                c.icon(look::ICON_TRASH, (ic.cx(), ic.cy()), 20.0, 1.9, look::RED, 0.0);
                for (n, l) in wrap(c, &title, tf, head_w).iter().enumerate() {
                    c.text(l, Rect::new(x + 56.0, y + n as f32 * 20.25, head_w, 20.25), tf, ui::T1, Align::Start);
                }
                for (n, l) in wrap(c, &sub, sf, head_w).iter().enumerate() {
                    c.text_fit(l, Rect::new(x + 56.0, y + tl * 20.25 + 4.0 + n as f32 * 15.1, head_w, 15.1), sf, ui::T3, Align::Start);
                }
                y += head + 18.0;
                for (k, (how, label, text)) in opts.iter().enumerate() {
                    let on = d.how == *how;
                    let b = Rect::new(x, y, inner, heights[k]);
                    let hot = f.focusable(ID_DLG_OPT + k as Id, b);
                    f.hand();
                    let t = f.tween(ID_DLG_OPT + k as Id, 14, f32::from(on), 160, EASE);
                    c.fill(b, 12.0, Rgba::white(0.04).mix(accent().alpha(0.12), t));
                    c.ring(b, 12.0, 1.0, Rgba::white(0.06 * (1.0 - t)));
                    c.ring(b, 12.0, 1.5, accent().fade(t));
                    // the radio: a ring of 1.5 that becomes one of 5
                    let dot = Rect::new(b.x + 12.0, b.y + 11.0 + 1.0, 16.0, 16.0);
                    c.ring(dot, 8.0, 1.5 + 3.5 * t, Rgba::white(0.3).mix(accent(), t));
                    c.text(label, Rect::new(b.x + 12.0 + 16.0 + 10.0, b.y + 11.0, inner - 50.0, 15.7), lf, ui::T1, Align::Start);
                    for (n, l) in wrap(c, text, of, inner - 24.0 - 26.0).iter().enumerate() {
                        c.text(l, Rect::new(b.x + 12.0 + 16.0 + 10.0, b.y + 11.0 + 15.7 + 2.0 + n as f32 * 17.4, inner - 50.0, 17.4), of, ui::T2, Align::Start);
                    }
                    ring_focus(c, b, 12.0, hot.focus);
                    y += heights[k] + 6.0;
                }
                y += 18.0 - 6.0;
                let cta = if d.how == How::Permanently { format!("Delete {}", human(d.size)) } else { "Move to Recycle Bin".to_string() };
                let bf = font(13.0, 500);
                let (cw, okw) = (c.measure("Cancel", bf).ceil() + 32.0, c.measure(&cta, font(13.0, 600)).ceil() + 32.0);
                let ok = Rect::new(x + inner - okw, y, okw, 36.0);
                let cancel = Rect::new(ok.x - 8.0 - cw, y, cw, 36.0);
                let hc = f.focusable(ID_DLG_CANCEL, cancel);
                f.hand();
                c.transformed(cancel.cx(), cancel.cy(), 1.0 - 0.03 * hc.press, 0.0, 0.0, |c| {
                    c.fill(cancel, 10.0, Rgba::white(0.08 + 0.05 * hc.hover));
                    c.text("Cancel", cancel, bf, ui::T1, Align::Center);
                });
                ring_focus(c, cancel, 10.0, hc.focus);
                let ho = f.focusable(ID_DLG_OK, ok);
                f.hand();
                c.transformed(ok.cx(), ok.cy(), 1.0 - 0.03 * ho.press, 0.0, 0.0, |c| {
                    c.fill(ok, 10.0, ui::DANGER);
                    if ho.hover > 0.0 {
                        c.fill(ok, 10.0, Rgba::pure_white(0.12 * ho.hover));
                    }
                    c.text(&cta, ok, font(13.0, 600), Rgba::pure_white(1.0), Align::Center);
                });
                ring_focus(c, ok, 10.0, ho.focus);
            });
        });
    }

    /// The toast: 40 high, a tick, the words, Undo; 46 above the bottom; it comes up over 8 % of 5 s and goes over the last 10 %.
    fn draw_toast(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32, h: f32) {
        let Some(t) = self.toast.as_mut() else { return };
        let born = *t.born.get_or_insert(f.now);
        if t.serial != self.toast_seen {
            f.tween(K_TOAST_IN, 0, 0.0, 0, LINEAR);
            f.tween(K_TOAST_OUT, 0, 0.0, 0, LINEAR);
            self.toast_seen = t.serial;
        }
        let age = f.now.saturating_sub(born);
        if age >= TOAST_MS {
            self.toast = None;
            return;
        }
        let tin = f.tween(K_TOAST_IN, 0, 1.0, (TOAST_MS as f32 * 0.08) as u32, EASE);
        let tout = if age as f32 >= TOAST_MS as f32 * 0.9 {
            f.tween(K_TOAST_OUT, 0, 1.0, (TOAST_MS as f32 * 0.1) as u32, EASE)
        } else {
            f.due(born + (TOAST_MS as f32 * 0.9) as u64);
            0.0
        };
        f.due(born + TOAST_MS);
        let (text, undo, ok) = (t.text.clone(), t.undo, t.ok);
        let (bf, uf) = (font(13.0, 400), font(13.0, 600));
        let tw = c.measure(&text, bf).ceil();
        let uw = if undo { c.measure("Undo", uf).ceil() + 24.0 } else { 10.0 };
        let width = 16.0 + 15.0 + 12.0 + tw + 12.0 + uw + 6.0;
        let r = Rect::new(((w - width) / 2.0).round(), h - 46.0 - 40.0, width, 40.0);
        c.faded((tin * (1.0 - tout)).max(0.004), |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, 10.0 * (1.0 - tin), |c| {
                c.outside(r, 20.0, |c| c.shadow(r, 20.0, 16.0, 40.0, Rgba::css(0, 0, 0, 0.4)));
                c.fill(r, 20.0, look::MENU);
                c.ring(r.inset(-1.0), 21.0, 1.0, Rgba::css(0, 0, 0, 0.3));
                c.ring(r, 20.0, 1.0, Rgba::white(0.1));
                if ok {
                    c.icon(look::ICON_CHECK, (r.x + 16.0 + 7.5, r.cy()), 15.0, 2.4, look::GREEN, 0.0);
                } else {
                    c.icon("M12 8v5M12 16h.01", (r.x + 16.0 + 7.5, r.cy()), 15.0, 2.4, look::RED, 0.0);
                }
                c.text(&text, Rect::new(r.x + 16.0 + 15.0 + 12.0, r.y, tw + 2.0, r.h), bf, ui::T1, Align::Start);
                if undo {
                    let b = Rect::new(r.x + r.w - 6.0 - uw, r.y + 6.0, uw, 28.0);
                    let hot = f.item(ID_TOAST_UNDO, b);
                    f.hand();
                    c.fill(b, 14.0, Rgba::white(0.1 + 0.05 * hot.hover));
                    c.text("Undo", b, uf, accent().accent_text(), Align::Center);
                }
            });
        });
    }
}

/// An action button of the panel: label, icon, what it does, to which record, red or not, its hint.
type Act = (&'static str, &'static str, usize, Option<u32>, bool, &'static str);

/// Word-wrapped lines of `text` in `width`, by measuring (the canvas has no way to hand the lines back).
fn wrap(c: &mut Canvas<'_>, text: &str, f: Font, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split(' ') {
        let try_ = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if cur.is_empty() || c.measure(&try_, f) <= width {
            cur = try_;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// A tile's label: the folder's icon, its name (600), and under it the size, the share, and where most of it is.
fn draw_tile_label(c: &mut Canvas<'_>, r: Rect, t: &Tile, fade: f32) {
    let ink = look::tile_text().fade(fade);
    let line = t.font * 1.21;
    let mut x = r.x + 12.0;
    if t.dir && t.id != AGG {
        c.glyph(look::ICON_FOLDER_FILL, (x + 6.5, r.y + 10.0 + line / 2.0), 13.0, ink.fade(0.8));
        x += 13.0 + 6.0;
    }
    c.text_fit(&t.name, Rect::new(x, r.y + 10.0, (r.x + r.w - 10.0 - x).max(0.0), line), font(t.font, 600), ink, Align::Start);
    if !t.size_line.is_empty() {
        c.text_fit(&t.size_line, Rect::new(r.x + 12.0, r.y + 10.0 + line + 1.0, (r.w - 22.0).max(0.0), 14.0), font(11.5, 500).tabular(), look::tile_text2().fade(fade), Align::Start);
    }
}

/// `repeating-linear-gradient(135deg, ink .06 0 6px, ink .03 6px 12px)`: what the tile of the small items is made of.
fn draw_hatch(c: &mut Canvas<'_>, r: Rect, radius: f32, o: f32) {
    c.fill(r, radius, Rgba::white(0.03).fade(o));
    c.masked(r, radius, Xf::NONE, |c| {
        let period = 12.0 * std::f32::consts::SQRT_2;
        let (s0, s1) = (r.x + r.y, r.x + r.w + r.y + r.h);
        let mut s = s0;
        while s < s1 {
            // a band along the diagonal that runs up to the right, 6 thick
            let sc = s + 3.0 * std::f32::consts::SQRT_2;
            c.line(sc - r.y, r.y, sc - (r.y + r.h), r.y + r.h, 6.0, Rgba::white(0.03).fade(o));
            s += period;
        }
    });
}

/// `translate(zx, zy) scale(sx, sy)` eased to nothing by `e` (the mockup's `dk_zin`), applied to rectangles of the map.
#[derive(Clone, Copy)]
struct Aff {
    tx: f32,
    ty: f32,
    kx: f32,
    ky: f32,
}

impl Aff {
    const NONE: Aff = Aff { tx: 0.0, ty: 0.0, kx: 1.0, ky: 1.0 };

    fn new(zx: f32, zy: f32, sx: f32, sy: f32, e: f32) -> Self {
        Self { tx: zx * (1.0 - e), ty: zy * (1.0 - e), kx: sx + (1.0 - sx) * e, ky: sy + (1.0 - sy) * e }
    }

    fn map(self, r: Rect) -> Rect {
        Rect::new(self.tx + r.x * self.kx, self.ty + r.y * self.ky, r.w * self.kx, r.h * self.ky)
    }
}
