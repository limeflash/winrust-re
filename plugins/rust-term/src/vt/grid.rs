//! Terminal screen grid.

use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags(u16);
impl Flags {
    pub const BOLD: Self = Self(1);
    pub const DIM: Self = Self(2);
    pub const ITALIC: Self = Self(4);
    pub const UNDERLINE: Self = Self(8);
    pub const BLINK: Self = Self(16);
    pub const INVERSE: Self = Self(32);
    pub const HIDDEN: Self = Self(64);
    pub const STRIKE: Self = Self(128);
    pub fn contains(self, flags: Self) -> bool {
        self.0 & flags.0 == flags.0
    }
    pub fn insert(&mut self, flags: Self) {
        self.0 |= flags.0;
    }
    pub fn remove(&mut self, flags: Self) {
        self.0 &= !flags.0;
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pen {
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub width: u8,
    pub pen: Pen,
}
impl Cell {
    fn blank(pen: Pen) -> Self {
        Self {
            ch: ' ',
            width: 1,
            pen: Pen {
                bg: pen.bg,
                ..Pen::default()
            },
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Saved {
    row: usize,
    col: usize,
    pen: Pen,
    origin: bool,
    wrap: bool,
}
#[derive(Clone, Debug)]
struct Buffer {
    cells: Vec<Vec<Cell>>,
    row: usize,
    col: usize,
    pen: Pen,
    origin: bool,
    wrap: bool,
    saved: Option<Saved>,
}
impl Buffer {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cells: vec![vec![Cell::blank(Pen::default()); cols]; rows],
            row: 0,
            col: 0,
            pen: Pen::default(),
            origin: false,
            wrap: false,
            saved: None,
        }
    }
}
/// A screen has at least one row and column; zero dimensions are clamped to one.
pub struct Grid {
    cols: usize,
    rows: usize,
    active: Buffer,
    main: Option<Buffer>,
    top: usize,
    bottom: usize,
    main_region: Option<(usize, usize)>,
    autowrap: bool,
    tabs: Vec<bool>,
    scrolled: Vec<Vec<Cell>>,
}
impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            cols,
            rows,
            active: Buffer::new(cols, rows),
            main: None,
            top: 0,
            bottom: rows - 1,
            main_region: None,
            autowrap: true,
            tabs: (0..cols).map(|i| i != 0 && i % 8 == 0).collect(),
            scrolled: Vec::new(),
        }
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.active.cells[row][col]
    }
    pub fn row_text(&self, row: usize) -> String {
        let mut s = String::new();
        for cell in &self.active.cells[row] {
            if cell.width != 0 {
                s.push(cell.ch);
            }
        }
        s.trim_end_matches(' ').to_owned()
    }
    pub fn cursor(&self) -> (usize, usize) {
        (self.active.row, self.active.col)
    }
    pub fn pen_mut(&mut self) -> &mut Pen {
        &mut self.active.pen
    }
    pub fn pending_wrap(&self) -> bool {
        self.active.wrap
    }
    pub fn set_autowrap(&mut self, enabled: bool) {
        self.autowrap = enabled;
        if !enabled {
            self.active.wrap = false;
        }
    }
    pub fn set_origin_mode(&mut self, enabled: bool) {
        self.active.origin = enabled;
        self.goto(0, 0);
    }
    fn blank(&self) -> Cell {
        Cell::blank(self.active.pen)
    }
    fn clear_wide(&mut self, row: usize, col: usize) {
        let blank = self.blank();
        let width = self.active.cells[row][col].width;
        if width == 0 && col > 0 {
            self.active.cells[row][col - 1] = blank;
        }
        if width == 2 && col + 1 < self.cols {
            self.active.cells[row][col + 1] = blank;
        }
        self.active.cells[row][col] = blank;
    }
    fn repair(&mut self, row: usize) {
        let blank = self.blank();
        for col in 0..self.cols {
            let w = self.active.cells[row][col].width;
            if (w == 0 && (col == 0 || self.active.cells[row][col - 1].width != 2))
                || (w == 2 && (col + 1 == self.cols || self.active.cells[row][col + 1].width != 0))
            {
                self.active.cells[row][col] = blank;
            }
        }
    }
    pub fn print(&mut self, ch: char) {
        // ponytail: zero-width characters are dropped until combining sequences are supported.
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width == 0 || width > 2 || width > self.cols {
            return;
        }
        if self.active.wrap || (width == 2 && self.active.col + 1 >= self.cols && self.autowrap) {
            self.active.wrap = false;
            self.active.col = 0;
            self.linefeed();
        }
        if width == 2 && self.active.col + 1 >= self.cols {
            return;
        }
        let row = self.active.row;
        let col = self.active.col;
        self.clear_wide(row, col);
        if width == 2 {
            self.clear_wide(row, col + 1);
        }
        self.active.cells[row][col] = Cell {
            ch,
            width: width as u8,
            pen: self.active.pen,
        };
        if width == 2 {
            self.active.cells[row][col + 1] = Cell {
                ch: ' ',
                width: 0,
                pen: self.active.pen,
            };
        }
        if col + width >= self.cols {
            self.active.col = self.cols - 1;
            self.active.wrap = self.autowrap;
        } else {
            self.active.col += width;
        }
    }
    pub fn carriage_return(&mut self) {
        self.active.col = 0;
        self.active.wrap = false;
    }
    pub fn linefeed(&mut self) {
        self.active.wrap = false;
        if self.active.row == self.bottom {
            self.scroll_up(1);
        } else {
            self.active.row = (self.active.row + 1).min(self.rows - 1);
        }
    }
    pub fn reverse_index(&mut self) {
        self.active.wrap = false;
        if self.active.row == self.top {
            self.scroll_down(1);
        } else {
            self.active.row = self.active.row.saturating_sub(1);
        }
    }
    pub fn next_line(&mut self) {
        self.carriage_return();
        self.linefeed();
    }
    pub fn backspace(&mut self) {
        self.active.col = self.active.col.saturating_sub(1);
        self.active.wrap = false;
    }
    pub fn tab(&mut self) {
        self.active.col = ((self.active.col + 1)..self.cols)
            .find(|&i| self.tabs[i])
            .unwrap_or(self.cols - 1);
        self.active.wrap = false;
    }
    pub fn set_tab_stop(&mut self) {
        self.tabs[self.active.col] = true;
    }
    pub fn clear_tab_stop(&mut self, all: bool) {
        if all {
            self.tabs.fill(false);
        } else {
            self.tabs[self.active.col] = false;
        }
    }
    pub fn up(&mut self, n: usize) {
        let min = if (self.top..=self.bottom).contains(&self.active.row) {
            self.top
        } else {
            0
        };
        self.active.row = self.active.row.saturating_sub(n).max(min);
        self.active.wrap = false;
    }
    pub fn down(&mut self, n: usize) {
        let max = if (self.top..=self.bottom).contains(&self.active.row) {
            self.bottom
        } else {
            self.rows - 1
        };
        self.active.row = self.active.row.saturating_add(n).min(max);
        self.active.wrap = false;
    }
    pub fn forward(&mut self, n: usize) {
        self.active.col = self.active.col.saturating_add(n).min(self.cols - 1);
        self.active.wrap = false;
    }
    pub fn back(&mut self, n: usize) {
        self.active.col = self.active.col.saturating_sub(n);
        self.active.wrap = false;
    }
    pub fn goto(&mut self, row: usize, col: usize) {
        self.active.row = if self.active.origin {
            self.top.saturating_add(row).min(self.bottom)
        } else {
            row.min(self.rows - 1)
        };
        self.active.col = col.min(self.cols - 1);
        self.active.wrap = false;
    }
    pub fn goto_col(&mut self, col: usize) {
        self.active.col = col.min(self.cols - 1);
        self.active.wrap = false;
    }
    pub fn goto_row(&mut self, row: usize) {
        self.active.row = if self.active.origin {
            self.top.saturating_add(row).min(self.bottom)
        } else {
            row.min(self.rows - 1)
        };
        self.active.wrap = false;
    }
    fn erase_range(&mut self, row: usize, start: usize, end: usize) {
        for col in start..end {
            self.clear_wide(row, col);
        }
    }
    pub fn erase_line(&mut self, mode: u8) {
        let row = self.active.row;
        let col = self.active.col;
        match mode {
            0 => self.erase_range(row, col, self.cols),
            1 => self.erase_range(row, 0, col + 1),
            2 => self.erase_range(row, 0, self.cols),
            _ => {}
        }
    }
    pub fn erase_display(&mut self, mode: u8) {
        let row = self.active.row;
        match mode {
            0 => {
                self.erase_line(0);
                for r in row + 1..self.rows {
                    self.erase_range(r, 0, self.cols);
                }
            }
            1 => {
                for r in 0..row {
                    self.erase_range(r, 0, self.cols);
                }
                self.erase_line(1);
            }
            2 => {
                for r in 0..self.rows {
                    self.erase_range(r, 0, self.cols);
                }
            }
            3 => self.scrolled.clear(),
            _ => {}
        }
    }
    pub fn insert_chars(&mut self, n: usize) {
        let r = self.active.row;
        let c = self.active.col;
        let n = n.min(self.cols - c);
        let blank = self.blank();
        self.active.cells[r].copy_within(c..self.cols - n, c + n);
        self.active.cells[r][c..c + n].fill(blank);
        self.repair(r);
        self.active.wrap = false;
    }
    pub fn delete_chars(&mut self, n: usize) {
        let r = self.active.row;
        let c = self.active.col;
        let n = n.min(self.cols - c);
        let blank = self.blank();
        self.active.cells[r].copy_within(c + n..self.cols, c);
        self.active.cells[r][self.cols - n..].fill(blank);
        self.repair(r);
        self.active.wrap = false;
    }
    pub fn erase_chars(&mut self, n: usize) {
        let r = self.active.row;
        let c = self.active.col;
        self.erase_range(r, c, c.saturating_add(n).min(self.cols));
        self.active.wrap = false;
    }
    pub fn insert_lines(&mut self, n: usize) {
        let r = self.active.row;
        if (self.top..=self.bottom).contains(&r) {
            let n = n.min(self.bottom - r + 1);
            let blank = vec![self.blank(); self.cols];
            for _ in 0..n {
                self.active.cells.remove(self.bottom);
                self.active.cells.insert(r, blank.clone());
            }
            self.active.col = 0;
            self.active.wrap = false;
        }
    }
    pub fn delete_lines(&mut self, n: usize) {
        let r = self.active.row;
        if (self.top..=self.bottom).contains(&r) {
            let n = n.min(self.bottom - r + 1);
            let blank = vec![self.blank(); self.cols];
            for _ in 0..n {
                self.active.cells.remove(r);
                self.active.cells.insert(self.bottom, blank.clone());
            }
            self.active.col = 0;
            self.active.wrap = false;
        }
    }
    pub fn scroll_up(&mut self, n: usize) {
        let n = n.min(self.bottom - self.top + 1);
        let blank = vec![self.blank(); self.cols];
        for _ in 0..n {
            let line = self.active.cells.remove(self.top);
            if self.main.is_none() && self.top == 0 && self.bottom == self.rows - 1 {
                self.scrolled.push(line);
            }
            self.active.cells.insert(self.bottom, blank.clone());
        }
    }
    pub fn scroll_down(&mut self, n: usize) {
        let n = n.min(self.bottom - self.top + 1);
        let blank = vec![self.blank(); self.cols];
        for _ in 0..n {
            self.active.cells.remove(self.bottom);
            self.active.cells.insert(self.top, blank.clone());
        }
    }
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        if top < bottom && bottom < self.rows {
            self.top = top;
            self.bottom = bottom;
        } else {
            self.top = 0;
            self.bottom = self.rows - 1;
        }
        self.goto(0, 0);
    }
    pub fn save_cursor(&mut self) {
        self.active.saved = Some(Saved {
            row: self.active.row,
            col: self.active.col,
            pen: self.active.pen,
            origin: self.active.origin,
            wrap: self.active.wrap,
        });
    }
    /// DECRC; with nothing saved the cursor goes home with the default pen, as in xterm.
    pub fn restore_cursor(&mut self) {
        let s = self.active.saved.unwrap_or(Saved {
            row: 0,
            col: 0,
            pen: Pen::default(),
            origin: false,
            wrap: false,
        });
        self.active.row = s.row.min(self.rows - 1);
        self.active.col = s.col.min(self.cols - 1);
        self.active.pen = s.pen;
        self.active.origin = s.origin;
        self.active.wrap = s.wrap;
    }
    pub fn enter_alt_screen(&mut self) {
        if self.main.is_none() {
            self.save_cursor();
            self.main_region = Some((self.top, self.bottom));
            let alt = Buffer::new(self.cols, self.rows);
            self.main = Some(std::mem::replace(&mut self.active, alt));
            self.top = 0;
            self.bottom = self.rows - 1;
        }
    }
    pub fn leave_alt_screen(&mut self) {
        if let Some(main) = self.main.take() {
            self.active = main;
            (self.top, self.bottom) = self.main_region.take().unwrap_or((0, self.rows - 1));
            self.restore_cursor();
        }
    }
    pub fn is_alt(&self) -> bool {
        self.main.is_some()
    }
    pub fn take_scrolled_off(&mut self) -> Vec<Vec<Cell>> {
        std::mem::take(&mut self.scrolled)
    }
    // ponytail: no reflow on resize, Windows Terminal reflows — add it if cutting lines on narrowing gets in the way
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        fn resize_buffer(buf: &mut Buffer, cols: usize, rows: usize) -> Vec<Vec<Cell>> {
            let remove = buf.row.saturating_add(1).saturating_sub(rows);
            let mut lost = Vec::new();
            for _ in 0..remove {
                lost.push(buf.cells.remove(0));
            }
            buf.row = buf.row.saturating_sub(remove);
            buf.cells.truncate(rows);
            buf.cells.resize_with(rows, || vec![Cell::blank(Pen::default()); cols]);
            for line in &mut buf.cells {
                line.resize(cols, Cell::blank(Pen::default()));
                if line[cols - 1].width == 2 {
                    line[cols - 1] = Cell::blank(Pen::default());
                }
            }
            buf.col = buf.col.min(cols - 1);
            buf.wrap = false;
            lost
        }
        let lost = resize_buffer(&mut self.active, cols, rows);
        if self.main.is_none() {
            self.scrolled.extend(lost);
        }
        if let Some(main) = &mut self.main {
            self.scrolled.extend(resize_buffer(main, cols, rows));
        }
        let old = self.cols;
        self.tabs.resize(cols, false);
        for (i, tab) in self.tabs.iter_mut().enumerate().skip(old) {
            *tab = i != 0 && i % 8 == 0;
        }
        self.cols = cols;
        self.rows = rows;
        self.top = 0;
        self.bottom = rows - 1;
        if self.main.is_some() {
            self.main_region = Some((0, rows - 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn filled() -> Grid {
        let mut g = Grid::new(4, 3);
        for ch in "abcdefghijkl".chars() {
            g.print(ch);
        }
        g
    }
    fn text(g: &Grid) -> Vec<String> {
        (0..g.rows()).map(|r| g.row_text(r)).collect()
    }
    #[test]
    fn dimensions() {
        let g = Grid::new(0, 0);
        assert_eq!((g.cols(), g.rows()), (1, 1));
    }
    #[test]
    fn flags() {
        let mut f = Flags::default();
        f.insert(Flags::BOLD);
        assert!(f.contains(Flags::BOLD));
        f.remove(Flags::BOLD);
        assert!(!f.contains(Flags::BOLD));
    }
    #[test]
    fn pending_wrap() {
        let mut g = Grid::new(2, 2);
        g.print('a');
        g.print('b');
        assert!(g.pending_wrap());
        assert_eq!(g.cursor(), (0, 1));
        g.print('c');
        assert_eq!(g.cursor(), (1, 1));
        assert_eq!(text(&g), ["ab", "c"]);
    }
    #[test]
    fn carriage_clears_wrap() {
        let mut g = Grid::new(1, 2);
        g.print('a');
        g.carriage_return();
        g.print('b');
        assert_eq!(text(&g), ["b", ""]);
    }
    #[test]
    fn no_autowrap() {
        let mut g = Grid::new(2, 2);
        g.set_autowrap(false);
        for c in "abc".chars() {
            g.print(c);
        }
        assert_eq!(text(&g), ["ac", ""]);
    }
    #[test]
    fn cjk_wraps_whole() {
        let mut g = Grid::new(3, 2);
        for c in "ab界".chars() {
            g.print(c);
        }
        assert_eq!(text(&g), ["ab", "界"]);
        assert_eq!(g.cell(1, 1).width, 0);
    }
    #[test]
    fn emoji_width() {
        let mut g = Grid::new(3, 1);
        g.print('😀');
        assert_eq!((g.cell(0, 0).width, g.cell(0, 1).width), (2, 0));
    }
    #[test]
    fn combining_dropped() {
        let mut g = Grid::new(3, 1);
        g.print('a');
        g.print('\u{301}');
        assert_eq!(g.row_text(0), "a");
    }
    #[test]
    fn overwrite_wide_half() {
        let mut g = Grid::new(4, 1);
        g.print('界');
        g.goto_col(1);
        g.print('x');
        assert_eq!(g.row_text(0), " x");
    }
    #[test]
    fn bce() {
        let mut g = filled();
        g.pen_mut().bg = Color::Indexed(2);
        g.pen_mut().fg = Color::Indexed(3);
        g.erase_line(2);
        assert_eq!(
            g.cell(2, 0).pen,
            Pen {
                bg: Color::Indexed(2),
                ..Pen::default()
            }
        );
    }
    #[test]
    fn ed_zero() {
        let mut g = filled();
        g.goto(1, 1);
        g.erase_display(0);
        assert_eq!(text(&g), ["abcd", "e", ""]);
    }
    #[test]
    fn ed_one() {
        let mut g = filled();
        g.goto(1, 1);
        g.erase_display(1);
        assert_eq!(text(&g), ["", "  gh", "ijkl"]);
    }
    #[test]
    fn ed_two() {
        let mut g = filled();
        g.erase_display(2);
        assert_eq!(text(&g), ["", "", ""]);
    }
    #[test]
    fn ed_three() {
        let mut g = filled();
        g.scroll_up(1);
        g.erase_display(3);
        assert!(g.take_scrolled_off().is_empty());
        assert_eq!(g.row_text(0), "efgh");
    }
    #[test]
    fn el_zero() {
        let mut g = filled();
        g.goto(0, 1);
        g.erase_line(0);
        assert_eq!(g.row_text(0), "a");
    }
    #[test]
    fn el_one() {
        let mut g = filled();
        g.goto(0, 1);
        g.erase_line(1);
        assert_eq!(g.row_text(0), "  cd");
    }
    #[test]
    fn el_two() {
        let mut g = filled();
        g.erase_line(2);
        assert_eq!(g.row_text(2), "");
    }
    #[test]
    fn ich() {
        let mut g = filled();
        g.goto(0, 1);
        g.insert_chars(2);
        assert_eq!(g.row_text(0), "a  b");
    }
    #[test]
    fn dch() {
        let mut g = filled();
        g.goto(0, 1);
        g.delete_chars(2);
        assert_eq!(g.row_text(0), "ad");
    }
    #[test]
    fn ech() {
        let mut g = filled();
        g.goto(0, 1);
        g.erase_chars(2);
        assert_eq!(g.row_text(0), "a  d");
    }
    #[test]
    fn edits_repair_wide() {
        let mut g = Grid::new(4, 1);
        g.print('界');
        g.goto_col(1);
        g.insert_chars(1);
        assert_eq!(g.row_text(0), "");
    }
    #[test]
    fn il_inside() {
        let mut g = filled();
        g.set_scroll_region(1, 2);
        g.goto(1, 2);
        g.insert_lines(1);
        assert_eq!(text(&g), ["abcd", "", "efgh"]);
        assert_eq!(g.cursor(), (1, 0));
    }
    #[test]
    fn dl_inside() {
        let mut g = filled();
        g.set_scroll_region(1, 2);
        g.goto(1, 2);
        g.delete_lines(1);
        assert_eq!(text(&g), ["abcd", "ijkl", ""]);
        assert_eq!(g.cursor(), (1, 0));
    }
    #[test]
    fn lines_outside() {
        let mut g = filled();
        g.set_scroll_region(1, 2);
        g.goto(0, 2);
        g.insert_lines(1);
        g.delete_lines(1);
        assert_eq!(text(&g), ["abcd", "efgh", "ijkl"]);
    }
    #[test]
    fn su() {
        let mut g = filled();
        g.scroll_up(2);
        assert_eq!(text(&g), ["ijkl", "", ""]);
    }
    #[test]
    fn sd() {
        let mut g = filled();
        g.scroll_down(1);
        assert_eq!(text(&g), ["", "abcd", "efgh"]);
    }
    #[test]
    fn region_linefeed() {
        let mut g = filled();
        g.set_scroll_region(1, 2);
        g.goto(2, 1);
        g.linefeed();
        assert_eq!(text(&g), ["abcd", "ijkl", ""]);
        assert_eq!(g.cursor(), (2, 1));
    }
    #[test]
    fn reverse_at_top() {
        let mut g = filled();
        g.set_scroll_region(1, 2);
        g.goto(1, 0);
        g.reverse_index();
        assert_eq!(text(&g), ["abcd", "", "efgh"]);
    }
    #[test]
    fn invalid_region_full() {
        let mut g = filled();
        g.set_scroll_region(2, 1);
        g.goto(2, 0);
        g.linefeed();
        assert_eq!(g.row_text(0), "efgh");
    }
    #[test]
    fn origin_cup() {
        let mut g = Grid::new(4, 5);
        g.set_scroll_region(1, 3);
        g.set_origin_mode(true);
        g.goto(20, 2);
        assert_eq!(g.cursor(), (3, 2));
        g.goto_row(0);
        assert_eq!(g.cursor(), (1, 2));
    }
    #[test]
    fn moves_clamp_region() {
        let mut g = Grid::new(4, 5);
        g.set_scroll_region(1, 3);
        g.goto(2, 2);
        g.up(99);
        assert_eq!(g.cursor(), (1, 2));
        g.down(99);
        g.forward(99);
        assert_eq!(g.cursor(), (3, 3));
        g.back(99);
        assert_eq!(g.cursor(), (3, 0));
    }
    #[test]
    fn cursor_save_restore() {
        let mut g = Grid::new(2, 3);
        g.set_scroll_region(1, 2);
        g.set_origin_mode(true);
        g.pen_mut().fg = Color::Indexed(1);
        g.print('a');
        g.print('b');
        g.save_cursor();
        g.set_origin_mode(false);
        g.pen_mut().fg = Color::Default;
        g.restore_cursor();
        assert_eq!(g.cursor(), (1, 1));
        assert!(g.pending_wrap());
        assert_eq!(g.pen_mut().fg, Color::Indexed(1));
        g.goto(0, 0);
        assert_eq!(g.cursor(), (1, 0));
    }
    #[test]
    fn alternate_preserves_main() {
        let mut g = filled();
        g.enter_alt_screen();
        assert!(g.is_alt());
        assert_eq!(text(&g), ["", "", ""]);
        g.print('z');
        g.leave_alt_screen();
        assert_eq!(text(&g), ["abcd", "efgh", "ijkl"]);
        assert!(g.pending_wrap());
    }
    #[test]
    fn resize_smaller_keeps_cursor() {
        let mut g = filled();
        g.resize(2, 2);
        assert_eq!(text(&g), ["ef", "ij"]);
        assert_eq!(g.cursor(), (1, 1));
        assert_eq!(g.take_scrolled_off().len(), 1);
    }
    #[test]
    fn resize_larger() {
        let mut g = filled();
        g.resize(6, 4);
        assert_eq!(text(&g), ["abcd", "efgh", "ijkl", ""]);
        assert_eq!((g.cols(), g.rows()), (6, 4));
    }
    #[test]
    fn scrollback_order() {
        let mut g = filled();
        g.scroll_up(2);
        let lines = g.take_scrolled_off();
        assert_eq!(lines.iter().map(|l| l[0].ch).collect::<String>(), "ae");
        assert!(g.take_scrolled_off().is_empty());
    }
    #[test]
    fn no_partial_or_alt_scrollback() {
        let mut g = filled();
        g.set_scroll_region(1, 2);
        g.scroll_up(1);
        g.enter_alt_screen();
        g.scroll_up(1);
        assert!(g.take_scrolled_off().is_empty());
    }
    #[test]
    fn tabs() {
        let mut g = Grid::new(20, 1);
        g.tab();
        assert_eq!(g.cursor().1, 8);
        g.clear_tab_stop(false);
        g.goto_col(0);
        g.tab();
        assert_eq!(g.cursor().1, 16);
        g.goto_col(3);
        g.set_tab_stop();
        g.clear_tab_stop(true);
        g.set_tab_stop();
        g.goto_col(0);
        g.tab();
        assert_eq!(g.cursor().1, 3);
    }
    #[test]
    fn tab_extension() {
        let mut g = Grid::new(5, 1);
        g.resize(18, 1);
        g.tab();
        assert_eq!(g.cursor().1, 8);
    }
    #[test]
    fn restore_without_save_goes_home() {
        let mut g = Grid::new(4, 3);
        g.goto(2, 2);
        g.pen_mut().fg = Color::Indexed(1);
        g.restore_cursor();
        assert_eq!(g.cursor(), (0, 0));
        assert_eq!(g.pen_mut().fg, Color::Default);
    }
    #[test]
    fn next_line_and_backspace() {
        let mut g = Grid::new(5, 3);
        g.goto(0, 3);
        g.backspace();
        assert_eq!(g.cursor(), (0, 2));
        g.next_line();
        assert_eq!(g.cursor(), (1, 0));
    }
}
