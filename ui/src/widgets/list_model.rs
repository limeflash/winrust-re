//! What a list is, with no window and no drawing: the selection (click, Ctrl, Shift, keys, typing a name), the rows that are in view,
//! and the columns of a header (their widths, the sort, a grip being dragged). [`super::list`] draws it.
use super::scroll_model::Scroll;
use std::{collections::BTreeSet, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}
#[derive(Default, Debug)]
pub struct ListState {
    pub sel: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub cursor: Option<usize>,
    pub scroll: Scroll,
    typed: (String, u64),
    len: usize,
    /// The row a press on a selected row of a multi-selection waits on: it may become a drag of them all, or a click.
    held: Option<usize>,
    /// The cursor row the view was last scrolled to (so that a key that moves it brings it into view, once).
    pub shown: Option<usize>,
    /// How many cells a line of a grid has (0 in a list): [`super::list::grid`] writes it so that Up and Down know their step.
    pub cols: usize,
}
impl ListState {
    pub fn click(&mut self, i: usize, ctrl: bool, shift: bool) {
        if shift {
            let a = self.anchor.unwrap_or(i);
            if !ctrl {
                self.sel.clear();
            }
            self.sel.extend(a.min(i)..=a.max(i));
            self.anchor = Some(a);
        } else if ctrl {
            if !self.sel.insert(i) {
                self.sel.remove(&i);
            }
            self.anchor = Some(i);
        } else {
            self.sel.clear();
            self.sel.insert(i);
            self.anchor = Some(i);
        }
        self.cursor = Some(i);
    }
    /// The pointer went down on row `i`. A plain press on a row of a selection of several waits for the button to go up (it may be
    /// the start of dragging them all); any other press selects at once.
    pub fn press(&mut self, i: usize, ctrl: bool, shift: bool) {
        if !ctrl && !shift && self.sel.len() > 1 && self.sel.contains(&i) {
            self.held = Some(i);
            self.cursor = Some(i);
        } else {
            self.held = None;
            self.click(i, ctrl, shift);
        }
    }
    /// The button came up on row `i` without a drag: what a held press was waiting for.
    pub fn lift(&mut self, i: usize) {
        if self.held.take() == Some(i) {
            self.click(i, false, false);
        }
    }
    /// A drag began: the held press was not a click.
    pub fn drop_held(&mut self) {
        self.held = None;
    }
    pub fn select_all(&mut self, n: usize) {
        self.len = n;
        self.sel = (0..n).collect();
    }
    /// How many rows there are (as of the last `set_len`).
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn set_len(&mut self, n: usize) {
        self.len = n;
        self.sel.retain(|&i| i < n);
        if self.anchor.is_some_and(|i| i >= n) {
            self.anchor = None;
        }
        if self.cursor.is_some_and(|i| i >= n) {
            self.cursor = if n == 0 { None } else { Some(n - 1) };
        }
    }
    pub fn key(&mut self, key: Key, shift: bool, ctrl: bool, n: usize, rows_per_page: usize) {
        self.set_len(n);
        if n == 0 {
            return;
        }
        let current = self.cursor.unwrap_or(0);
        let step = rows_per_page.max(1);
        let i = match key {
            // with no cursor yet, the first move lands on the first row
            Key::Up | Key::Down | Key::PageUp | Key::PageDown if self.cursor.is_none() => 0,
            Key::Up => current.saturating_sub(1),
            Key::Down => current.saturating_add(1).min(n - 1),
            Key::PageUp => current.saturating_sub(step),
            Key::PageDown => current.saturating_add(step).min(n - 1),
            Key::Home => 0,
            Key::End => n - 1,
        };
        self.go(i, shift, ctrl);
    }
    /// Moves the cursor `by` rows (a grid's Up and Down are a whole line) as a key does: Shift extends, Ctrl only moves.
    pub fn step(&mut self, by: isize, shift: bool, ctrl: bool, n: usize) {
        self.set_len(n);
        if n == 0 {
            return;
        }
        let i = if self.cursor.is_none() { 0 } else { (self.cursor.unwrap_or(0) as isize + by).clamp(0, n as isize - 1) as usize };
        self.go(i, shift, ctrl);
    }
    /// The cursor to row `i`, selecting or extending it the way the modifiers say.
    pub fn go(&mut self, i: usize, shift: bool, ctrl: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor.unwrap_or(0));
            }
            self.click(i, ctrl, true);
        } else if ctrl {
            self.cursor = Some(i);
        } else {
            self.click(i, false, false);
        }
    }
    pub fn typeahead(
        &mut self,
        ch: char,
        now: u64,
        n: usize,
        name_at: impl Fn(usize) -> Option<String>,
    ) -> Option<usize> {
        self.set_len(n);
        let fresh = self.typed.0.is_empty() || now.saturating_sub(self.typed.1) >= 1000;
        if fresh {
            self.typed.0.clear();
        }
        self.typed.0.push(ch);
        self.typed.1 = now;
        let start = if fresh {
            self.cursor.map_or(0, |i| i.saturating_add(1))
        } else {
            self.cursor.unwrap_or(0)
        };
        let found = find_row(n, start, &self.typed.0, name_at);
        if let Some(i) = found {
            self.click(i, false, false);
        }
        found
    }
    pub fn visible_range(&self, row_h: f32, now: u64) -> Range<usize> {
        if row_h <= 0.0 || self.scroll.view() <= 0.0 {
            return 0..0;
        }
        let y = self.scroll.offset(now);
        let start = ((y / row_h).floor() as usize).min(self.len);
        let end = (((y + self.scroll.view()) / row_h).ceil() as usize).min(self.len);
        start..end.max(start)
    }
}
/// Find a case-insensitive prefix, wrapping around and skipping unnamed rows.
pub fn find_row(
    len: usize,
    start: usize,
    typed: &str,
    name_at: impl Fn(usize) -> Option<String>,
) -> Option<usize> {
    let typed = typed.to_lowercase();
    if typed.is_empty() || len == 0 {
        return None;
    }
    (0..len)
        .map(|k| (start.min(len) + k) % len)
        .find(|&i| name_at(i).is_some_and(|n| n.to_lowercase().starts_with(&typed)))
}
/// True once movement reaches four pixels in either direction.
pub fn drag_threshold(dx: f32, dy: f32) -> bool {
    dx.abs() >= 4.0 || dy.abs() >= 4.0
}
#[derive(Clone, Debug, PartialEq)]
pub struct Col {
    pub title: String,
    pub w: f32,
    pub min: f32,
    pub align_right: bool,
}
#[derive(Default, Debug)]
pub struct Columns {
    pub cols: Vec<Col>,
    pub sort: Option<(usize, bool)>,
    /// The column that takes what the others leave (the mockups' `minmax(0, 1fr)`): the first unless set; `usize::MAX` for none.
    pub flex: usize,
    /// The grip being dragged: its column and the x it was last at.
    grab: Option<(usize, f32)>,
}
impl Columns {
    /// Columns in the order given; the first takes what the others leave, nothing is sorted yet.
    #[must_use]
    pub fn new(cols: Vec<Col>) -> Self {
        Self { cols, ..Self::default() }
    }
    /// `(x, width)` of every column in a header `avail` wide: the flex column gets what is left, never under its minimum.
    #[must_use]
    pub fn layout(&self, avail: f32) -> Vec<(f32, f32)> {
        let fixed: f32 = self.cols.iter().enumerate().filter(|(i, _)| *i != self.flex).map(|(_, c)| c.w).sum();
        let mut x = 0.0;
        self.cols
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let w = if i == self.flex { (avail - fixed).max(c.min) } else { c.w };
                x += w;
                (x - w, w)
            })
            .collect()
    }
    /// The edge between column `i` and the next moved by `dx`: left of the flex column it is the column's own width; right of it
    /// (or at it) the flex column gives and takes, so the edge follows the pointer by the next column giving way.
    pub fn resize_edge(&mut self, i: usize, dx: f32) {
        if i < self.flex {
            self.resize(i, dx);
        } else {
            self.resize(i + 1, -dx);
        }
    }
    /// The pointer took hold of the grip on the right edge of column `i` at window x `x`.
    pub fn grip_down(&mut self, i: usize, x: f32) {
        self.grab = Some((i, x));
    }
    /// The grip follows the pointer; true when a width changed.
    pub fn grip_drag(&mut self, x: f32) -> bool {
        let Some((i, last)) = self.grab else { return false };
        let before: Vec<f32> = self.cols.iter().map(|c| c.w).collect();
        self.resize_edge(i, x - last);
        // a column held at its minimum does not let the grip run away from the pointer
        let moved = before.iter().zip(&self.cols).any(|(b, c)| *b != c.w);
        if moved {
            self.grab = Some((i, x));
        }
        moved
    }
    pub fn grip_up(&mut self) {
        self.grab = None;
    }
    pub fn gripped(&self) -> Option<usize> {
        self.grab.map(|g| g.0)
    }
    pub fn resize(&mut self, col: usize, dx: f32) {
        if let Some(c) = self.cols.get_mut(col) {
            c.w = (c.w + dx).max(c.min);
        }
    }
    pub fn sort_by(&mut self, col: usize) {
        if col >= self.cols.len() {
            return;
        }
        self.sort = Some(match self.sort {
            Some((i, asc)) if i == col => (col, !asc),
            _ => (col, true),
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clicks_and_pruning() {
        let mut s = ListState::default();
        s.set_len(8);
        s.click(2, false, false);
        s.click(4, true, false);
        assert_eq!(s.sel, BTreeSet::from([2, 4]));
        s.click(6, false, true);
        assert_eq!(s.sel, BTreeSet::from([4, 5, 6]));
        s.click(1, true, true);
        assert_eq!(s.sel, BTreeSet::from([1, 2, 3, 4, 5, 6]));
        s.click(4, true, false);
        assert!(!s.sel.contains(&4));
        s.select_all(8);
        s.set_len(3);
        assert_eq!(s.sel, BTreeSet::from([0, 1, 2]));
        assert_eq!(s.anchor, None);
        assert_eq!(s.cursor, Some(2));
        s.set_len(0);
        assert_eq!(s.cursor, None);
    }
    #[test]
    fn keys_and_visible() {
        let mut s = ListState::default();
        s.click(2, false, false);
        s.key(Key::Down, true, false, 10, 3);
        assert_eq!(s.sel, BTreeSet::from([2, 3]));
        s.key(Key::End, false, true, 10, 3);
        assert_eq!(s.cursor, Some(9));
        assert_eq!(s.sel, BTreeSet::from([2, 3]));
        s.key(Key::PageUp, false, false, 10, 3);
        assert_eq!(s.cursor, Some(6));
        s.key(Key::Home, false, false, 10, 3);
        s.key(Key::PageDown, false, false, 10, 3);
        s.key(Key::Up, false, false, 10, 3);
        assert_eq!(s.cursor, Some(2));
        s.scroll.set_extent(100.0, 25.0);
        s.scroll.scroll_to(15.0, 0);
        assert_eq!(s.visible_range(10.0, 150), 1..4);
    }
    #[test]
    fn a_press_on_a_selected_row_waits_for_the_button() {
        let mut s = ListState::default();
        s.set_len(10);
        s.click(2, false, false);
        s.click(4, true, false);
        s.press(2, false, false);
        assert_eq!(s.sel, BTreeSet::from([2, 4]), "the selection stays: it may be dragged");
        s.lift(2);
        assert_eq!(s.sel, BTreeSet::from([2]), "a click without a drag selects the one row");
        s.click(4, true, false);
        s.press(4, false, false);
        s.drop_held();
        s.lift(4);
        assert_eq!(s.sel, BTreeSet::from([2, 4]), "after a drag began, letting go is not a click");
        s.press(7, false, false);
        assert_eq!(s.sel, BTreeSet::from([7]), "a press elsewhere selects at once");
        s.press(8, true, false);
        assert_eq!(s.sel, BTreeSet::from([7, 8]));
    }

    #[test]
    fn the_first_key_lands_on_the_first_row_and_a_grid_steps_by_lines() {
        let mut s = ListState::default();
        s.key(Key::Down, false, false, 10, 3);
        assert_eq!(s.cursor, Some(0), "no cursor: Down selects the first row, not the second");
        let mut g = ListState::default();
        g.step(1, false, false, 20);
        assert_eq!(g.cursor, Some(0));
        g.step(5, false, false, 20);
        g.step(5, true, false, 20);
        assert_eq!(g.cursor, Some(10));
        assert_eq!(g.sel, BTreeSet::from([5, 6, 7, 8, 9, 10]), "Shift+Down in a grid selects the line between");
        g.step(50, false, false, 20);
        assert_eq!(g.cursor, Some(19), "the end clamps");
        g.step(-100, false, true, 20);
        assert_eq!((g.cursor, g.sel.len()), (Some(0), 1), "Ctrl moves the cursor and keeps the selection");
    }

    #[test]
    fn columns_share_the_width_and_the_grips_move_the_edges() {
        let col = |t: &str, w: f32, min: f32| Col { title: t.into(), w, min, align_right: false };
        let mut c = Columns { cols: vec![col("Name", 0.0, 120.0), col("Date", 152.0, 80.0), col("Type", 128.0, 60.0), col("Size", 84.0, 60.0)], ..Default::default() };
        let l = c.layout(800.0);
        assert_eq!(l, vec![(0.0, 436.0), (436.0, 152.0), (588.0, 128.0), (716.0, 84.0)], "Name takes what the rest leave");
        assert_eq!(c.layout(300.0)[0].1, 120.0, "never under its minimum");
        // the edge between Name and Date moved 30 right: Date gives way
        c.resize_edge(0, 30.0);
        assert_eq!(c.cols[1].w, 122.0);
        // the edge between Date and Type moved 20 left: Type grows
        c.resize_edge(1, -20.0);
        assert_eq!(c.cols[2].w, 148.0);
        let r = c.layout(800.0);
        assert_eq!((r[1].0 + r[1].1, r[2].0), (r[2].0, r[1].0 + r[1].1));
        // a grip dragged past a column's minimum stops there and the pointer's x is not forgotten
        c.grip_down(1, 500.0);
        assert!(c.grip_drag(480.0));
        assert_eq!(c.cols[2].w, 168.0);
        assert!(c.grip_drag(600.0), "right");
        assert_eq!(c.cols[2].w, 60.0, "Type held at its minimum");
        assert!(!c.grip_drag(900.0), "no more to give: nothing moves");
        assert!(!c.grip_drag(901.0));
        c.grip_up();
        assert!(!c.grip_drag(10.0) && c.gripped().is_none());
        // a column left of the flex one takes the width itself
        c.flex = 3;
        c.resize_edge(0, 200.0);
        assert_eq!(c.cols[0].w, 200.0);
    }

    #[test]
    fn search_drag_columns() {
        let names = ["Alpha", "Alpine", "Beta"];
        let mut s = ListState::default();
        assert_eq!(s.typeahead('a', 0, 3, |i| Some(names[i].into())), Some(0));
        assert_eq!(s.typeahead('l', 10, 3, |i| Some(names[i].into())), Some(0));
        assert_eq!(
            s.typeahead('b', 1010, 3, |i| Some(names[i].into())),
            Some(2)
        );
        assert_eq!(find_row(3, 3, "AL", |i| Some(names[i].into())), Some(0));
        assert!(!drag_threshold(3.0, 3.0));
        assert!(drag_threshold(-4.0, 0.0));
        let mut c = Columns {
            cols: vec![Col {
                title: "Name".into(),
                w: 100.0,
                min: 40.0,
                align_right: false,
            }],
            ..Default::default()
        };
        c.resize(0, -100.0);
        assert_eq!(c.cols[0].w, 40.0);
        c.sort_by(0);
        c.sort_by(0);
        assert_eq!(c.sort, Some((0, false)));
    }
}
