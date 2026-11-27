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
    pub fn select_all(&mut self, n: usize) {
        self.len = n;
        self.sel = (0..n).collect();
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
            Key::Up => current.saturating_sub(1),
            Key::Down => current.saturating_add(1).min(n - 1),
            Key::PageUp => current.saturating_sub(step),
            Key::PageDown => current.saturating_add(step).min(n - 1),
            Key::Home => 0,
            Key::End => n - 1,
        };
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(current);
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
}
impl Columns {
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
