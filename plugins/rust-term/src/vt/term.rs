//! VT parser and terminal state.
use super::grid::{Cell, Color, Flags, Grid, Pen};
use std::collections::VecDeque;
use vte::{Params, Perform};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouse {
    Off,
    Click,
    Drag,
    Motion,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Prompt,
    Input,
    Output,
    Done(Option<i32>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub kind: MarkKind,
    pub line: u64,
    pub col: usize,
}

pub struct Term {
    grid: Grid,
    parser: vte::Parser,
    history: VecDeque<Vec<Cell>>,
    scrolled_total: u64,
    bell: bool,
    replies: Vec<u8>,
    title: String,
    cwd: Option<String>,
    links: Vec<String>,
    marks: Vec<Mark>,
    graphics: bool,
    cursor_visible: bool,
    app_cursor: bool,
    app_keypad: bool,
    bracketed_paste: bool,
    mouse: Mouse,
    sgr_mouse: bool,
    focus_events: bool,
    cursor_shape: u8,
}
impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            parser: vte::Parser::new(),
            history: VecDeque::new(),
            scrolled_total: 0,
            bell: false,
            replies: Vec::new(),
            title: String::new(),
            cwd: None,
            links: Vec::new(),
            marks: Vec::new(),
            graphics: false,
            cursor_visible: true,
            app_cursor: false,
            app_keypad: false,
            bracketed_paste: false,
            mouse: Mouse::Off,
            sgr_mouse: false,
            focus_events: false,
            cursor_shape: 0,
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut parser = std::mem::replace(&mut self.parser, vte::Parser::new());
        parser.advance(self, bytes);
        self.parser = parser;
        self.drain_history();
    }
    fn drain_history(&mut self) {
        for line in self.grid.take_scrolled_off() {
            self.scrolled_total += 1;
            self.history.push_back(line);
            if self.history.len() > 10_000 {
                self.history.pop_front();
            }
        }
    }
    pub fn grid(&self) -> &Grid {
        &self.grid
    }
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.grid.resize(cols, rows);
        self.drain_history();
    }
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }
    pub fn link(&self, id: u32) -> Option<&str> {
        id.checked_sub(1)
            .and_then(|i| self.links.get(i as usize))
            .map(String::as_str)
    }
    pub fn take_marks(&mut self) -> Vec<Mark> {
        std::mem::take(&mut self.marks)
    }
    pub fn history(&self) -> &VecDeque<Vec<Cell>> {
        &self.history
    }
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }
    pub fn app_cursor(&self) -> bool {
        self.app_cursor
    }
    pub fn app_keypad(&self) -> bool {
        self.app_keypad
    }
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }
    pub fn mouse(&self) -> Mouse {
        self.mouse
    }
    pub fn sgr_mouse(&self) -> bool {
        self.sgr_mouse
    }
    pub fn focus_events(&self) -> bool {
        self.focus_events
    }
    pub fn cursor_shape(&self) -> u8 {
        self.cursor_shape
    }
    /// RIS. The history and the line count stay: marks keep pointing at the right lines (blocks are cut by them).
    fn reset(&mut self) {
        self.drain_history();
        let (cols, rows) = (self.grid.cols(), self.grid.rows());
        let parser = std::mem::replace(&mut self.parser, vte::Parser::new());
        let history = std::mem::take(&mut self.history);
        let (scrolled_total, links) = (self.scrolled_total, std::mem::take(&mut self.links));
        *self = Self::new(cols, rows);
        (self.parser, self.history, self.scrolled_total, self.links) = (parser, history, scrolled_total, links);
    }
    fn modes(&mut self, params: &Params, enabled: bool) {
        for p in params {
            match p[0] {
                1 => self.app_cursor = enabled,
                6 => self.grid.set_origin_mode(enabled),
                7 => self.grid.set_autowrap(enabled),
                25 => self.cursor_visible = enabled,
                47 | 1047 | 1049 => {
                    if enabled {
                        self.grid.enter_alt_screen();
                    } else {
                        self.grid.leave_alt_screen();
                    }
                }
                1000 | 1002 | 1003 => {
                    let mode = match p[0] {
                        1000 => Mouse::Click,
                        1002 => Mouse::Drag,
                        _ => Mouse::Motion,
                    };
                    if enabled {
                        self.mouse = mode;
                    } else if self.mouse == mode {
                        self.mouse = Mouse::Off;
                    }
                }
                1004 => self.focus_events = enabled,
                1006 => self.sgr_mouse = enabled,
                2004 => self.bracketed_paste = enabled,
                _ => (),
            }
        }
    }
    fn sgr(&mut self, params: &Params) {
        let mut it = params.iter().peekable();
        if it.peek().is_none() {
            let link = self.grid.pen_mut().link;
            *self.grid.pen_mut() = Pen {
                link,
                ..Pen::default()
            };
            return;
        }
        while let Some(p) = it.next() {
            let code = p[0];
            if matches!(code, 38 | 48 | 58) {
                let mut values = Vec::new();
                if p.len() > 1 {
                    values.extend_from_slice(&p[1..]);
                } else if let Some(mode) = it.next() {
                    values.push(mode[0]);
                    let count = match mode[0] {
                        5 => 1,
                        2 => 3,
                        _ => 0,
                    };
                    for _ in 0..count {
                        if let Some(v) = it.next() {
                            values.push(v[0]);
                        }
                    }
                }
                let color = match values.as_slice() {
                    [5, n, ..] if *n <= 255 => Some(Color::Indexed(*n as u8)),
                    [2, _, r, g, b, ..] if *r <= 255 && *g <= 255 && *b <= 255 => {
                        Some(Color::Rgb(*r as u8, *g as u8, *b as u8))
                    }
                    [2, r, g, b] if *r <= 255 && *g <= 255 && *b <= 255 => {
                        Some(Color::Rgb(*r as u8, *g as u8, *b as u8))
                    }
                    _ => None,
                };
                if let Some(color) = color {
                    match code {
                        38 => self.grid.pen_mut().fg = color,
                        48 => self.grid.pen_mut().bg = color,
                        _ => (),
                    }
                }
                continue;
            }
            let pen = self.grid.pen_mut();
            match code {
                0 => {
                    *pen = Pen {
                        link: pen.link,
                        ..Pen::default()
                    }
                }
                1 => pen.flags.insert(Flags::BOLD),
                2 => pen.flags.insert(Flags::DIM),
                3 => pen.flags.insert(Flags::ITALIC),
                4 if p.get(1) == Some(&0) => pen.flags.remove(Flags::UNDERLINE),
                4 | 21 => pen.flags.insert(Flags::UNDERLINE),
                5 => pen.flags.insert(Flags::BLINK),
                7 => pen.flags.insert(Flags::INVERSE),
                8 => pen.flags.insert(Flags::HIDDEN),
                9 => pen.flags.insert(Flags::STRIKE),
                22 => {
                    pen.flags.remove(Flags::BOLD);
                    pen.flags.remove(Flags::DIM);
                }
                23 => pen.flags.remove(Flags::ITALIC),
                24 => pen.flags.remove(Flags::UNDERLINE),
                25 => pen.flags.remove(Flags::BLINK),
                27 => pen.flags.remove(Flags::INVERSE),
                28 => pen.flags.remove(Flags::HIDDEN),
                29 => pen.flags.remove(Flags::STRIKE),
                30..=37 => pen.fg = Color::Indexed((code - 30) as u8),
                39 => pen.fg = Color::Default,
                40..=47 => pen.bg = Color::Indexed((code - 40) as u8),
                49 => pen.bg = Color::Default,
                90..=97 => pen.fg = Color::Indexed((code - 90 + 8) as u8),
                100..=107 => pen.bg = Color::Indexed((code - 100 + 8) as u8),
                _ => (),
            }
        }
    }
}
fn number(p: Option<&[u16]>, default: usize) -> usize {
    p.and_then(|p| p.first())
        .copied()
        .filter(|n| *n != 0)
        .map(usize::from)
        .unwrap_or(default)
}
fn graphics(c: char) -> char {
    match c {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
    }
}
fn decode_path(bytes: &[u8]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(s) = std::str::from_utf8(&bytes[i + 1..i + 3])
                && let Ok(v) = u8::from_str_radix(s, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
        out.push(bytes[i]);
        i += 1;
    }
    let mut s = String::from_utf8_lossy(&out).into_owned();
    if s.as_bytes().first() == Some(&b'/') && s.as_bytes().get(2) == Some(&b':') {
        s.remove(0);
        s = s.replace('/', "\\");
    }
    s
}
impl Perform for Term {
    fn print(&mut self, c: char) {
        self.grid.print(if self.graphics { graphics(c) } else { c });
    }
    fn execute(&mut self, byte: u8) {
        match byte {
            7 => self.bell = true,
            8 => self.grid.backspace(),
            9 => self.grid.tab(),
            10..=12 => self.grid.linefeed(),
            13 => self.grid.carriage_return(),
            _ => (),
        }
    }
    fn esc_dispatch(&mut self, i: &[u8], ignore: bool, b: u8) {
        if ignore {
            return;
        }
        match (i, b) {
            ([], b'7') => self.grid.save_cursor(),
            ([], b'8') => {
                self.grid.restore_cursor();
            }
            ([], b'D') => self.grid.linefeed(),
            ([], b'E') => self.grid.next_line(),
            ([], b'M') => self.grid.reverse_index(),
            ([], b'H') => self.grid.set_tab_stop(),
            ([], b'c') => self.reset(),
            ([], b'=') => self.app_keypad = true,
            ([], b'>') => self.app_keypad = false,
            ([b'('], b'0') => self.graphics = true,
            ([b'('], b'B') => self.graphics = false,
            _ => (),
        }
    }
    fn csi_dispatch(&mut self, params: &Params, i: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }
        if i == b"?" && matches!(action, 'h' | 'l') {
            self.modes(params, action == 'h');
            return;
        }
        if i == b" " && action == 'q' {
            let n = params.iter().next().map_or(0, |p| p[0]);
            if n <= 6 {
                self.cursor_shape = n as u8;
            }
            return;
        }
        if !i.is_empty() {
            return;
        }
        let mut it = params.iter();
        let first = it.next();
        let n = number(first, 1);
        let second = it.next();
        match action {
            '@' => self.grid.insert_chars(n),
            'A' => self.grid.up(n),
            'B' | 'e' => self.grid.down(n),
            'C' | 'a' => self.grid.forward(n),
            'D' => self.grid.back(n),
            'E' => {
                self.grid.down(n);
                self.grid.carriage_return();
            }
            'F' => {
                self.grid.up(n);
                self.grid.carriage_return();
            }
            'G' | '`' => self.grid.goto_col(n - 1),
            'H' | 'f' => self.grid.goto(n - 1, number(second, 1) - 1),
            'I' => {
                for _ in 0..n.min(self.grid.cols()) {
                    self.grid.tab();
                }
            }
            'J' => {
                let mode = first.map_or(0, |p| p[0]);
                if mode == 3 {
                    self.drain_history();
                    self.history.clear();
                }
                self.grid.erase_display(mode as u8);
            }
            'K' => self.grid.erase_line(first.map_or(0, |p| p[0]) as u8),
            'L' => self.grid.insert_lines(n),
            'M' => self.grid.delete_lines(n),
            'P' => self.grid.delete_chars(n),
            'S' => self.grid.scroll_up(n),
            'T' => self.grid.scroll_down(n),
            'X' => self.grid.erase_chars(n),
            'd' => self.grid.goto_row(n - 1),
            'g' => match first.map_or(0, |p| p[0]) {
                0 => self.grid.clear_tab_stop(false),
                3 => self.grid.clear_tab_stop(true),
                _ => (),
            },
            'r' => {
                let top = n - 1;
                let bottom = number(second, self.grid.rows()) - 1;
                self.grid.set_scroll_region(top, bottom);
            }
            's' if params.is_empty() || (params.len() == 1 && first == Some(&[0][..])) => {
                self.grid.save_cursor()
            }
            'u' if params.is_empty() || (params.len() == 1 && first == Some(&[0][..])) => {
                self.grid.restore_cursor()
            }
            'n' => match first.map_or(0, |p| p[0]) {
                5 => self.replies.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let (row, col) = self.grid.cursor();
                    let row = if self.grid.origin_mode() {
                        row.saturating_sub(self.grid.scroll_top())
                    } else {
                        row
                    };
                    self.replies
                        .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
                }
                _ => (),
            },
            'c' => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            'm' => self.sgr(params),
            _ => (),
        }
    }
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // vte splits an OSC at every ';': a title, a path or a URI that has one comes back in pieces
        let rest = |from: usize| params.get(from..).filter(|p| !p.is_empty()).map(|p| p.join(&b';'));
        match params.first().copied() {
            Some(b"0" | b"2") => {
                if let Some(value) = rest(1) {
                    self.title = String::from_utf8_lossy(&value).into_owned();
                }
            }
            Some(b"7") => {
                if let Some(value) = params.get(1)
                    && let Some(rest) = value.strip_prefix(b"file://")
                        && let Some(pos) = rest.iter().position(|b| *b == b'/') {
                            self.cwd = Some(decode_path(&rest[pos..]));
                        }
            }
            Some(b"9") if params.get(1) == Some(&b"9".as_slice()) => {
                if let Some(value) = rest(2) {
                    let s = String::from_utf8_lossy(&value);
                    self.cwd = Some(s.trim_matches('"').to_owned());
                }
            }
            Some(b"8") => {
                if let Some(uri) = rest(2) {
                    // ponytail: every link keeps its uri for the session's life (cells in the history point at it); recycle
                    // ids no line uses any more if a program floods links
                    let id = if uri.is_empty() {
                        0
                    } else {
                        self.links.push(String::from_utf8_lossy(&uri).into_owned());
                        self.links.len() as u32
                    };
                    self.grid.pen_mut().link = id;
                }
            }
            Some(b"133") => {
                let kind = match params.get(1).copied() {
                    Some(b"A") => Some(MarkKind::Prompt),
                    Some(b"B") => Some(MarkKind::Input),
                    Some(b"C") => Some(MarkKind::Output),
                    Some(b"D") => Some(MarkKind::Done(
                        params
                            .get(2)
                            .and_then(|v| std::str::from_utf8(v).ok())
                            .and_then(|s| s.parse().ok()),
                    )),
                    _ => None,
                };
                if let Some(kind) = kind {
                    self.drain_history();
                    let (row, col) = self.grid.cursor();
                    self.marks.push(Mark {
                        kind,
                        line: self.scrolled_total + row as u64,
                        col,
                    });
                }
            }
            // ponytail: a program may not write the user's clipboard without a setting.
            Some(b"52") => (),
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn term(bytes: &[u8]) -> Term {
        let mut t = Term::new(20, 4);
        t.feed(bytes);
        t
    }
    macro_rules! row_test {
        ($name:ident, $input:expr, $expected:expr) => {
            #[test]
            fn $name() {
                assert_eq!(term($input).grid().row_text(0), $expected);
            }
        };
    }
    row_test!(basic, b"hello", "hello");
    row_test!(carriage, b"abcdef\rxy\x1b[K", "xy");
    row_test!(backspace, b"abc\x08X", "abX");
    row_test!(insert_chars, b"abcd\x1b[2G\x1b[2@X", "aX bcd");
    row_test!(delete_chars, b"abcdef\x1b[2G\x1b[2P", "adef");
    row_test!(erase_chars, b"abcdef\x1b[2G\x1b[2X", "a  def");
    row_test!(erase_line, b"abcdef\x1b[3G\x1b[1K", "   def");
    row_test!(erase_display, b"abcdef\x1b[2J", "");
    row_test!(graphics_set, "\x1b(0lqqk\x1b(B".as_bytes(), "┌──┐");
    row_test!(
        cargo_line,
        b"\x1b[1m\x1b[32m   Compiling \x1b[0mfoo v0.1.0",
        "   Compiling foo v0."
    );
    #[test]
    fn cup() {
        let t = term(b"\x1b[3;4H");
        assert_eq!(t.grid.cursor(), (2, 3));
    }
    #[test]
    fn cha() {
        let t = term(b"\x1b[5G");
        assert_eq!(t.grid.cursor(), (0, 4));
    }
    #[test]
    fn vpa() {
        let t = term(b"\x1b[3d");
        assert_eq!(t.grid.cursor(), (2, 0));
    }
    #[test]
    fn hpr() {
        let t = term(b"\x1b[4a");
        assert_eq!(t.grid.cursor(), (0, 4));
    }
    #[test]
    fn vpr() {
        let t = term(b"\x1b[2e");
        assert_eq!(t.grid.cursor(), (2, 0));
    }
    #[test]
    fn movement() {
        let t = term(b"\x1b[3;5H\x1b[A\x1b[2D\x1b[C\x1b[B");
        assert_eq!(t.grid.cursor(), (2, 3));
    }
    #[test]
    fn insert_lines() {
        let t = term(b"a\r\nb\r\nc\x1b[2;1H\x1b[L");
        assert_eq!(t.grid.row_text(2), "b");
    }
    #[test]
    fn delete_lines() {
        let t = term(b"a\r\nb\r\nc\x1b[2;1H\x1b[M");
        assert_eq!(t.grid.row_text(1), "c");
    }
    #[test]
    fn scrolling_region() {
        let t = term(b"a\r\nb\r\nc\r\nd\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(t.grid.row_text(1), "c");
        assert_eq!(t.grid.row_text(3), "d");
    }
    #[test]
    fn alternate() {
        let t = term(b"main\x1b[?1049halt\x1b[?1049l!");
        assert_eq!(t.grid.row_text(0), "main!");
    }
    #[test]
    fn autowrap_off() {
        let mut t = Term::new(3, 2);
        t.feed(b"\x1b[?7labcd");
        assert_eq!(t.grid.row_text(0), "abd");
    }
    #[test]
    fn origin() {
        let t = term(b"\x1b[2;4r\x1b[?6h\x1b[2;3H\x1b[6n");
        assert_eq!(t.grid.cursor(), (2, 2));
        assert_eq!(t.replies, b"\x1b[2;3R");
    }
    #[test]
    fn replies() {
        let t = term(b"\x1b[5n\x1b[6n\x1b[c");
        assert_eq!(t.replies, b"\x1b[0n\x1b[1;1R\x1b[?62;22c");
    }
    #[test]
    fn title_bel() {
        assert_eq!(term(b"\x1b]0;hello\x07").title(), "hello");
    }
    #[test]
    fn title_st() {
        assert_eq!(term(b"\x1b]2;world\x1b\\").title(), "world");
    }
    #[test]
    fn cwd_url() {
        assert_eq!(
            term(b"\x1b]7;file://host/C:/my%20dir\x07").cwd(),
            Some("C:\\my dir")
        );
    }
    #[test]
    fn cwd_conemu() {
        assert_eq!(term(b"\x1b]9;9;\"C:\\work\"\x07").cwd(), Some("C:\\work"));
    }
    #[test]
    fn hyperlinks() {
        let t = term(b"\x1b]8;;https://example.test\x07a\x1b[0mb\x1b]8;;\x07c");
        let id = t.grid.cell(0, 0).pen.link;
        assert_eq!(t.link(id), Some("https://example.test"));
        assert_eq!(t.grid.cell(0, 1).pen.link, id);
        assert_eq!(t.grid.cell(0, 2).pen.link, 0);
    }
    #[test]
    fn marks() {
        let mut t = Term::new(10, 2);
        t.feed(b"\x1b]133;A\x07\x1b]133;B\x07x\r\ny\r\nz\x1b]133;C\x07\x1b]133;D;3\x07");
        let m = t.take_marks();
        assert_eq!(
            m.iter().map(|m| &m.kind).collect::<Vec<_>>(),
            vec![
                &MarkKind::Prompt,
                &MarkKind::Input,
                &MarkKind::Output,
                &MarkKind::Done(Some(3))
            ]
        );
        assert_eq!(m[2].line, 2);
    }
    #[test]
    fn split_utf8() {
        let mut t = term(&[]);
        t.feed(&[0xe2, 0x94]);
        t.feed(&[0x80]);
        assert_eq!(t.grid.row_text(0), "─");
    }
    #[test]
    fn split_escape() {
        let mut t = term(b"\x1b[3");
        t.feed(b"1mX");
        assert_eq!(t.grid.cell(0, 0).pen.fg, Color::Indexed(1));
    }
    #[test]
    fn indexed_fg() {
        assert_eq!(
            term(b"\x1b[38;5;123mX").grid.cell(0, 0).pen.fg,
            Color::Indexed(123)
        );
    }
    #[test]
    fn rgb_fg() {
        assert_eq!(
            term(b"\x1b[38;2;1;2;3mX").grid.cell(0, 0).pen.fg,
            Color::Rgb(1, 2, 3)
        );
    }
    #[test]
    fn indexed_bg() {
        assert_eq!(
            term(b"\x1b[48;5;123mX").grid.cell(0, 0).pen.bg,
            Color::Indexed(123)
        );
    }
    #[test]
    fn rgb_bg() {
        assert_eq!(
            term(b"\x1b[48;2;1;2;3mX").grid.cell(0, 0).pen.bg,
            Color::Rgb(1, 2, 3)
        );
    }
    #[test]
    fn colon_index() {
        assert_eq!(
            term(b"\x1b[38:5:123mX").grid.cell(0, 0).pen.fg,
            Color::Indexed(123)
        );
    }
    #[test]
    fn colon_rgb_space() {
        assert_eq!(
            term(b"\x1b[38:2::1:2:3mX").grid.cell(0, 0).pen.fg,
            Color::Rgb(1, 2, 3)
        );
    }
    #[test]
    fn colon_rgb() {
        assert_eq!(
            term(b"\x1b[48:2:1:2:3mX").grid.cell(0, 0).pen.bg,
            Color::Rgb(1, 2, 3)
        );
    }
    #[test]
    fn resets() {
        let t = term(b"\x1b[1;2;4;7mA\x1b[22;24;27mB");
        assert_eq!(t.grid.cell(0, 1).pen.flags, Flags::default());
    }
    #[test]
    fn empty_sgr() {
        let t = term(b"\x1b[31mA\x1b[mB");
        assert_eq!(t.grid.cell(0, 1).pen.fg, Color::Default);
    }
    #[test]
    fn underline_colon() {
        let t = term(b"\x1b[4:2mA\x1b[4:0mB");
        assert!(t.grid.cell(0, 0).pen.flags.contains(Flags::UNDERLINE));
        assert!(!t.grid.cell(0, 1).pen.flags.contains(Flags::UNDERLINE));
    }
    #[test]
    fn bright_colors() {
        let t = term(b"\x1b[97;100mX");
        assert_eq!(
            (t.grid.cell(0, 0).pen.fg, t.grid.cell(0, 0).pen.bg),
            (Color::Indexed(15), Color::Indexed(8))
        );
    }
    #[test]
    fn ignored_underline_color() {
        let t = term(b"\x1b[58;2;1;2;3;31mX");
        assert_eq!(t.grid.cell(0, 0).pen.fg, Color::Indexed(1));
    }
    #[test]
    fn git_graph() {
        let t = term(
            "\x1b[33m*\x1b[m first\r\n\x1b[31m|\x1b[m\\ second\r\n\x1b[32m*\x1b[m third".as_bytes(),
        );
        assert_eq!(t.grid.row_text(1), "|\\ second");
        assert_eq!(t.grid.cell(2, 0).pen.fg, Color::Indexed(2));
    }
    #[test]
    fn ris() {
        let t = term(b"abc\x1b[?25l\x1b[31m\x1bcX");
        assert_eq!(t.grid.row_text(0), "X");
        assert!(t.cursor_visible());
        assert_eq!(t.grid.cell(0, 0).pen.fg, Color::Default);
    }
    #[test]
    fn history_cap_and_clear() {
        let mut t = Term::new(5, 2);
        for _ in 0..10005 {
            t.feed(b"x\r\n");
        }
        assert_eq!(t.history.len(), 10000);
        t.feed(b"\x1b[3J");
        assert!(t.history.is_empty());
    }
    #[test]
    fn bell() {
        let mut t = term(b"\x07");
        assert!(t.take_bell());
        assert!(!t.take_bell());
    }
    #[test]
    fn modes() {
        let t = term(b"\x1b[?1;1002;1004;1006;2004h\x1b=\x1b[5 q");
        assert!(
            t.app_cursor()
                && t.app_keypad()
                && t.focus_events()
                && t.sgr_mouse()
                && t.bracketed_paste()
        );
        assert_eq!(t.mouse(), Mouse::Drag);
        assert_eq!(t.cursor_shape(), 5);
    }
    #[test]
    fn tab_stops() {
        let t = term(b"a\tb");
        assert_eq!(t.grid.cell(0, 8).ch, 'b');
    }
    #[test]
    fn semicolons_stay_in_titles_and_links() {
        let t = term(b"\x1b]0;a;b\x07\x1b]8;;https://x.test/?a=1;b=2\x1b\\L");
        assert_eq!(t.title(), "a;b");
        assert_eq!(t.link(t.grid.cell(0, 0).pen.link), Some("https://x.test/?a=1;b=2"));
    }
    #[test]
    fn ris_keeps_the_history_and_line_count() {
        let mut t = Term::new(10, 2);
        t.feed(b"1\r\n2\r\n3\r\n\x1bc\x1b]133;A\x07");
        assert_eq!(t.history().len(), 2);
        assert_eq!(t.take_marks()[0].line, 2, "the line after the two scrolled off");
    }
    #[test]
    #[ignore]
    fn throughput() {
        let mut t = Term::new(100, 30);
        let chunk = b"\x1b[32mCompiling\x1b[0m example package v0.1.0\r\n";
        let iterations = 20_000_000 / chunk.len();
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            t.feed(chunk);
        }
        let mb = iterations as f64 * chunk.len() as f64 / 1_000_000.0;
        println!("throughput: {:.2} MB/s", mb / start.elapsed().as_secs_f64());
    }
}
