//! Lists that can be as long as the app likes: only the rows in view are drawn and named, so 10,000 rows cost what the 30 on the
//! screen cost. A header with sort arrows and resizable columns, and a grid of tiles, are the same thing in two other shapes.
//!
//! The app owns a [`ListState`] (and [`Columns`]) and gives the widgets their rows. Item ids, for a list named `base`: `base` is the
//! viewport, `base + 1` its bar, row `i` is `row_id(base, i)`. The app forwards what the window tells it:
//! * `App::down(row)`: [`ListState::press`] with `cx.mods`; `App::click(row)`: [`ListState::lift`] with the same; a double click opens;
//! * `App::wheel`: [`super::scroll::wheel`] on `st.scroll`; the bar: [`super::scroll::bar_down`] and its two sisters;
//! * `App::key` (when the list has the keys): [`ListState::key`], or [`grid_key`] in a grid; `App::text`: [`ListState::typeahead`];
//! * a header: [`col_of`] says which item an id is, [`ListState`]'s sister [`Columns`] does the rest.

use super::list_model::{Columns, Key, ListState};
use super::look;
use super::scroll::scroll_with;
use crate::anim::EASE;
use crate::frame::{Cursor, Frame, Id};
use crate::gfx::{Align, Canvas, Font, Rect, Rgba};

/// The padding and the corners of what a list shows.
#[derive(Clone, Copy, Debug)]
pub struct ListLook {
    /// Between two rows.
    pub gap: f32,
    pub radius: f32,
    /// At the sides, above the first row and below the last.
    pub inset: f32,
    pub top: f32,
    pub bottom: f32,
}

impl ListLook {
    /// The Explorer's rows: `padding: 4px 8px 12px; gap: 1px`, radius 8.
    pub const ROWS: ListLook = ListLook { gap: 1.0, radius: 8.0, inset: 8.0, top: 4.0, bottom: 12.0 };
    /// The Explorer's tiles: `padding: 12px; gap: 6px`, radius 12.
    pub const TILES: ListLook = ListLook { gap: 6.0, radius: 12.0, inset: 12.0, top: 12.0, bottom: 12.0 };
}

/// How a row stands, for what the app draws in it.
#[derive(Clone, Copy, Debug)]
pub struct RowState {
    pub selected: bool,
    /// The row the keys act on.
    pub cursor: bool,
    /// 0..1, eased.
    pub hover: f32,
}

/// The item of row `i` of the list named `base`.
#[must_use]
pub const fn row_id(base: Id, i: usize) -> Id {
    base + 2 + i as Id
}

/// Which row an item id of the list named `base` is (the app checks it is below its `n`).
#[must_use]
pub fn row_of(base: Id, id: Id) -> Option<usize> {
    (id >= base + 2).then(|| (id - base - 2) as usize)
}

/// A list of `n` rows of `row_h` DIPs in `r`; `row` draws row `i` into its rectangle (the plate, hover and selection, is the
/// widget's), and may name items of its own with `f`.
#[allow(clippy::too_many_arguments)] // the widget's own signature: what, where, which rows, how to draw them
pub fn list(c: &mut Canvas<'_>, f: &mut Frame<'_>, base: Id, r: Rect, st: &mut ListState, n: usize, row_h: f32, row: impl FnMut(&mut Canvas<'_>, &mut Frame<'_>, usize, Rect, RowState)) {
    list_with(c, f, base, r, st, n, row_h, &ListLook::ROWS, row);
}

#[allow(clippy::too_many_arguments)] // the widget's own signature: what, where, which rows, how to draw them
pub fn list_with(c: &mut Canvas<'_>, f: &mut Frame<'_>, base: Id, r: Rect, st: &mut ListState, n: usize, row_h: f32, look: &ListLook, mut row: impl FnMut(&mut Canvas<'_>, &mut Frame<'_>, usize, Rect, RowState)) {
    st.set_len(n);
    st.cols = 0;
    let pitch = row_h + look.gap;
    let content = look.top + n as f32 * pitch + look.bottom;
    reveal(st, r.h, content, look.top, look.bottom, pitch, row_h, f.now);
    let ring = f.mo.hits.focus == Some(base) && f.mo.hits.focus_visible;
    let (sel, cursor) = (&st.sel, st.cursor);
    scroll_with(c, f, base, r, &mut st.scroll, content, true, |c, f, v| {
        let first = (((v.y - look.top) / pitch).floor().max(0.0) as usize).min(n);
        let last = (((v.bottom() - look.top) / pitch).ceil().max(0.0) as usize).min(n);
        for i in first..last {
            let rect = Rect::new(r.x + look.inset, v.at(look.top + i as f32 * pitch), r.w - 2.0 * look.inset, row_h);
            draw_cell(c, f, base, i, rect, look.radius, sel.contains(&i), cursor == Some(i), ring, &mut row);
        }
    });
}

/// One row or tile: the item, its plate, then what the app draws.
#[allow(clippy::too_many_arguments)] // the widget's own signature: what, where, which rows, how to draw them
fn draw_cell(c: &mut Canvas<'_>, f: &mut Frame<'_>, base: Id, i: usize, rect: Rect, radius: f32, selected: bool, cursor: bool, ring: bool, row: &mut impl FnMut(&mut Canvas<'_>, &mut Frame<'_>, usize, Rect, RowState)) {
    let id = row_id(base, i);
    let hot = f.item(id, rect);
    let t = f.tween(id, 6, f32::from(selected), 140, EASE);
    c.fill(rect, radius, Rgba::white(0.045 * hot.hover * (1.0 - t)));
    if t > 0.0 {
        c.fill(rect, radius, look::accent().alpha(0.16 * t));
    }
    if cursor && ring && !selected {
        c.ring(rect, radius, 1.5, look::accent().alpha(0.7));
    }
    row(c, f, i, rect, RowState { selected, cursor, hover: hot.hover });
}

/// A key that moves the cursor out of view brings it in, once.
#[allow(clippy::too_many_arguments)] // the widget's own signature: what, where, which rows, how to draw them
fn reveal(st: &mut ListState, view_h: f32, content: f32, top: f32, bottom: f32, pitch: f32, row_h: f32, now: u64) {
    st.scroll.set_extent(content, view_h);
    if st.cursor != st.shown {
        if let Some(i) = st.cursor {
            // the first row brings the padding above it into view, the last the padding below it
            let y = if i == 0 { 0.0 } else { top + i as f32 * pitch };
            let h = row_h + if i == 0 { top } else { 0.0 } + if i + 1 == st.len() { bottom } else { 0.0 };
            st.scroll.ensure_visible(y, h, now);
        }
        st.shown = st.cursor;
    }
}

/// A grid of tiles `cell_h` high and at least `min_w` wide, as many to a line as fit, stretched to fill it (`repeat(auto-fill,
/// minmax(min_w, 1fr))`). `cell` draws tile `i` (the plate is the widget's). Returns the tiles to a line (also in `st.cols`).
#[allow(clippy::too_many_arguments)] // the widget's own signature: what, where, which rows, how to draw them
pub fn grid(c: &mut Canvas<'_>, f: &mut Frame<'_>, base: Id, r: Rect, st: &mut ListState, n: usize, min_w: f32, cell_h: f32, cell: impl FnMut(&mut Canvas<'_>, &mut Frame<'_>, usize, Rect, RowState)) -> usize {
    grid_with(c, f, base, r, st, n, min_w, cell_h, &ListLook::TILES, cell)
}

#[allow(clippy::too_many_arguments)] // the widget's own signature: what, where, which rows, how to draw them
pub fn grid_with(c: &mut Canvas<'_>, f: &mut Frame<'_>, base: Id, r: Rect, st: &mut ListState, n: usize, min_w: f32, cell_h: f32, look: &ListLook, mut cell: impl FnMut(&mut Canvas<'_>, &mut Frame<'_>, usize, Rect, RowState)) -> usize {
    st.set_len(n);
    let inner = r.w - 2.0 * look.inset;
    let cols = (((inner + look.gap) / (min_w + look.gap)).floor() as usize).max(1);
    st.cols = cols;
    let cell_w = (inner - (cols - 1) as f32 * look.gap) / cols as f32;
    let pitch = cell_h + look.gap;
    let lines = n.div_ceil(cols);
    let content = look.top + lines as f32 * pitch + look.bottom;
    // the cursor's tile is brought into view by its line
    st.scroll.set_extent(content, r.h);
    if st.cursor != st.shown {
        if let Some(i) = st.cursor {
            st.scroll.ensure_visible(look.top + (i / cols) as f32 * pitch, cell_h, f.now);
        }
        st.shown = st.cursor;
    }
    let ring = f.mo.hits.focus == Some(base) && f.mo.hits.focus_visible;
    let (sel, cursor) = (&st.sel, st.cursor);
    scroll_with(c, f, base, r, &mut st.scroll, content, true, |c, f, v| {
        let first = (((v.y - look.top) / pitch).floor().max(0.0) as usize).min(lines);
        let last = (((v.bottom() - look.top) / pitch).ceil().max(0.0) as usize).min(lines);
        for line in first..last {
            for k in 0..cols {
                let i = line * cols + k;
                if i >= n {
                    break;
                }
                let rect = Rect::new(r.x + look.inset + k as f32 * (cell_w + look.gap), v.at(look.top + line as f32 * pitch), cell_w, cell_h);
                draw_cell(c, f, base, i, rect, look.radius, sel.contains(&i), cursor == Some(i), ring, &mut cell);
            }
        }
    });
    cols
}

/// The keys of a grid: Left and Right step one tile, Up and Down a line, Home, End and the pages as in a list.
pub fn grid_key(st: &mut ListState, vk: u32, shift: bool, ctrl: bool, n: usize, lines_per_page: usize) -> bool {
    let cols = st.cols.max(1) as isize;
    match vk {
        0x25 => st.step(-1, shift, ctrl, n),
        0x27 => st.step(1, shift, ctrl, n),
        0x26 => st.step(-cols, shift, ctrl, n),
        0x28 => st.step(cols, shift, ctrl, n),
        0x21 => st.step(-cols * lines_per_page.max(1) as isize, shift, ctrl, n),
        0x22 => st.step(cols * lines_per_page.max(1) as isize, shift, ctrl, n),
        0x24 => st.key(Key::Home, shift, ctrl, n, 1),
        0x23 => st.key(Key::End, shift, ctrl, n, 1),
        _ => return false,
    }
    true
}

/// The keys of a list: the arrows, the pages, Home and End (Shift extends, Ctrl only moves), Ctrl+A, Space under Ctrl (toggles the
/// cursor row). True when the key was one of them.
pub fn list_key(st: &mut ListState, vk: u32, shift: bool, ctrl: bool, n: usize, rows_per_page: usize) -> bool {
    let key = match vk {
        0x26 => Key::Up,
        0x28 => Key::Down,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x24 => Key::Home,
        0x23 => Key::End,
        0x41 if ctrl => {
            st.select_all(n);
            return true;
        }
        0x20 if ctrl => {
            if let Some(i) = st.cursor {
                st.click(i, true, false);
            }
            return true;
        }
        _ => return false,
    };
    st.key(key, shift, ctrl, n, rows_per_page);
    true
}

// ---------------------------------------------------------------------------------------------------------------------
// the header

/// What an item of a header is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColItem {
    /// The title of column `i`: a click sorts by it.
    Title(usize),
    /// The grip on the right edge of column `i`: a drag resizes.
    Grip(usize),
}

const GRIPS: Id = 64;

/// Which item of the header named `id` the item `item` is.
#[must_use]
pub fn col_of(id: Id, item: Id) -> Option<ColItem> {
    match item - id {
        i @ 0..GRIPS => Some(ColItem::Title(i as usize)),
        i @ GRIPS..=127 => Some(ColItem::Grip((i - GRIPS) as usize)),
        _ => None,
    }
}

/// The header of a table in `r` (34 DIPs high in the Explorer), the columns laid out in `r.w - 2 * pad`. Titles are `id + i`, grips
/// `id + 64 + i`. A header cell is 26 high, 12 px text at weight 500; the sorted one is brighter and has the arrow.
pub fn columns(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, cols: &Columns, pad: f32) {
    c.fill(Rect::new(r.x, r.y + r.h - 1.0, r.w, 1.0), 0.0, Rgba::white(0.05));
    let font = Font::new(12.0, 500);
    let lay = cols.layout(r.w - 2.0 * pad);
    for (i, ((x, w), col)) in lay.iter().zip(&cols.cols).enumerate() {
        let cell = Rect::new(r.x + pad + x, r.cy() - 13.0, *w, 26.0);
        let hot = f.item(id + i as Id, cell);
        f.hand();
        let sorted = cols.sort.filter(|s| s.0 == i);
        let on = f.tween(id + i as Id, 2, f32::from(sorted.is_some()), 150, EASE);
        c.fill(cell, 6.0, Rgba::white(0.04 * hot.hover));
        let ink = look::T3.mix(look::T1, on.max(hot.hover));
        let label = c.measure(&col.title, font);
        let arrow = 10.0 + 4.0;
        let room = Rect::new(cell.x + 8.0, cell.y, (cell.w - 16.0 - arrow * on).max(0.0), cell.h);
        c.text_fit(&col.title, room, font, ink, if col.align_right { Align::End } else { Align::Start });
        // the arrow turns over when the order does
        let turn = f.tween(id + i as Id, 3, sorted.map_or(0.0, |s| if s.1 { 0.0 } else { 180.0 }), 200, EASE);
        if on > 0.01 {
            let ax = if col.align_right { room.x + room.w + 4.0 + 5.0 } else { room.x + label.min(room.w) + 4.0 + 5.0 };
            c.icon("M12 19V5M6 11l6-6 6 6", (ax, cell.cy()), 10.0, 3.0, ink.fade(on), turn);
        }
    }
    for (i, (x, w)) in lay.iter().enumerate().take(lay.len().saturating_sub(1)) {
        let edge = r.x + pad + x + w;
        let g = Rect::new(edge - 4.0, r.cy() - 10.0, 8.0, 20.0);
        let hot = f.item(id + GRIPS + i as Id, g);
        f.cursor(Cursor::SizeWE);
        let held = f32::from(cols.gripped() == Some(i));
        c.fill(Rect::new(edge - 0.5, r.cy() - 8.0, 1.0, 16.0), 0.0, Rgba::white(0.12 * hot.hover.max(held)));
    }
}
