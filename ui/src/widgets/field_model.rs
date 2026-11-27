//! The state and the editing of a one-line text field, with no window and no drawing: what a key, a typed string or a click does to
//! the text, the caret, the selection and the undo history. [`super::field`] draws it and feeds it; the tests here feed it by hand.
//!
//! Offsets into `text` are UTF-8 byte boundaries; DirectWrite's are UTF-16 ([`TextField::idx16`], [`TextField::byte_of`]).

use crate::frame::Mods;
use std::ops::Range;

/// Where text goes to and comes from with Ctrl+C / X / V. The window's is [`super::field::CxClip`]; tests and the self-test use
/// [`MemClip`], so they never touch the user's clipboard.
pub trait Clip {
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// A clipboard that is a variable.
#[derive(Default, Debug)]
pub struct MemClip(pub Option<String>);

impl Clip for MemClip {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set(&mut self, text: &str) {
        self.0 = Some(text.to_owned());
    }
}

/// What a key did to a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    /// Not a key of the field's: the app (or Tab) may have it.
    Ignored,
    /// Used, and nothing changed (a key the typed character follows, Ctrl+A on an empty field).
    Used,
    /// The caret or the selection moved.
    Moved,
    /// The text changed.
    Edited,
    /// Enter: the app submits.
    Enter,
    /// Escape: the app clears the field or leaves it.
    Esc,
}

impl Ev {
    /// The key was the field's (the window must not use it for anything else).
    #[must_use]
    pub fn used(self) -> bool {
        self != Ev::Ignored
    }
}

/// How the pointer wants the caret placed once the layout can answer where `x` is (the next frame draws it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Aim {
    Caret { extend: bool },
    Word,
}

/// What the widget keeps between frames; nothing of it is the text's.
#[derive(Clone, Debug, Default)]
pub(crate) struct View {
    /// Where the text begins on the window (DIPs) when it is not scrolled, and how wide the room for it is.
    pub text_x: f32,
    pub inner_w: f32,
    /// A click or a drag waiting for a layout: the x in the text's own space, and what to do with it.
    pub pending: Option<(f32, Aim)>,
    /// The last time the caret moved or the text changed (the blink restarts there), and the last edit (a pause ends a typing group).
    pub touched: u64,
    pub last_edit: u64,
    pub focused: bool,
    /// The last press: when, where, how many in a row.
    pub press: (u64, f32, u8),
    pub dragging: bool,
}

const UNDO_DEPTH: usize = 200;

#[derive(Clone, Debug)]
struct Snapshot {
    text: String,
    caret: usize,
    anchor: usize,
}

/// Single-line editing state. Offsets are UTF-8 byte boundaries.
#[derive(Default, Debug)]
pub struct TextField {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
    pub scroll_x: f32,
    pub password: bool,
    pub(crate) view: View,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    typing: bool,
}
impl TextField {
    /// An empty field showing what is typed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// An empty field that shows a bullet for each character and lets none of it out through the clipboard.
    #[must_use]
    pub fn password() -> Self {
        Self { password: true, ..Self::default() }
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            caret: self.caret,
            anchor: self.anchor,
        }
    }
    fn restore(&mut self, s: Snapshot) {
        self.text = s.text;
        self.caret = s.caret;
        self.anchor = s.anchor;
    }
    fn edit(&mut self, typing: bool) {
        if !typing || !self.typing {
            if self.undo.len() >= UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.undo.push(self.snapshot());
        }
        self.redo.clear();
        self.typing = typing;
    }
    /// Ends a typing group, for example after an input pause.
    pub fn pause(&mut self) {
        self.typing = false;
    }
    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }
    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }
    fn replace(&mut self, value: &str, typing: bool) {
        let r = self.selection();
        if r.is_empty() && value.is_empty() {
            return;
        }
        self.edit(typing && r.is_empty());
        self.text.replace_range(r.clone(), value);
        self.caret = r.start + value.len();
        self.anchor = self.caret;
    }
    /// Typed or composed text: control characters (the Ctrl+key codes of `WM_CHAR`, a Tab) are not text.
    pub fn insert(&mut self, value: &str) {
        let v: String = value.chars().filter(|c| !c.is_control()).collect();
        self.replace(&v, true);
    }
    /// Typed text at time `now`: like [`TextField::insert`], and a pause of a second since the last edit starts a new undo step.
    pub fn typed(&mut self, value: &str, now: u64) {
        if now.saturating_sub(self.view.last_edit) >= 1000 {
            self.pause();
        }
        self.insert(value);
        self.view.last_edit = now;
        self.view.touched = now;
    }
    /// Pasted text on one line: a line break and a tab become a space, other control characters go.
    pub fn paste(&mut self, value: &str) {
        let v: String = value.replace("\r\n", " ").chars().filter_map(|c| if c == '\r' || c == '\n' || c == '\t' { Some(' ') } else { (!c.is_control()).then_some(c) }).collect();
        self.replace(&v, false);
    }
    pub fn copy(&self) -> String {
        self.selected_text().to_owned()
    }
    pub fn cut(&mut self) -> String {
        let s = self.copy();
        self.replace("", false);
        s
    }
    pub fn set_text(&mut self, value: &str) {
        self.text = value.to_owned();
        self.caret = self.text.len();
        self.anchor = self.caret;
        self.undo.clear();
        self.redo.clear();
        self.pause();
    }
    pub fn display(&self) -> String {
        if self.password {
            "•".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        }
    }
    fn prev(&self, p: usize) -> usize {
        self.text[..p]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    fn next(&self, p: usize) -> usize {
        self.text[p..]
            .chars()
            .next()
            .map_or(p, |c| p + c.len_utf8())
    }
    fn move_to(&mut self, p: usize, extend: bool) {
        self.pause();
        self.caret = p;
        if !extend {
            self.anchor = p;
        }
    }
    pub fn left(&mut self, extend: bool) {
        let p = if !extend && !self.selection().is_empty() {
            self.selection().start
        } else {
            self.prev(self.caret)
        };
        self.move_to(p, extend);
    }
    pub fn right(&mut self, extend: bool) {
        let p = if !extend && !self.selection().is_empty() {
            self.selection().end
        } else {
            self.next(self.caret)
        };
        self.move_to(p, extend);
    }
    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }
    pub fn end(&mut self, extend: bool) {
        self.move_to(self.text.len(), extend);
    }
    pub fn select_all(&mut self) {
        self.pause();
        self.anchor = 0;
        self.caret = self.text.len();
    }
    fn word_l(&self, mut p: usize) -> usize {
        while p > 0
            && !self.text[self.prev(p)..p]
                .chars()
                .next()
                .unwrap()
                .is_alphanumeric()
        {
            p = self.prev(p);
        }
        while p > 0
            && self.text[self.prev(p)..p]
                .chars()
                .next()
                .unwrap()
                .is_alphanumeric()
        {
            p = self.prev(p);
        }
        p
    }
    fn word_r(&self, mut p: usize) -> usize {
        while p < self.text.len() && self.text[p..].chars().next().unwrap().is_alphanumeric() {
            p = self.next(p);
        }
        while p < self.text.len() && !self.text[p..].chars().next().unwrap().is_alphanumeric() {
            p = self.next(p);
        }
        p
    }
    pub fn word_left(&mut self, extend: bool) {
        let p = self.word_l(self.caret);
        self.move_to(p, extend);
    }
    pub fn word_right(&mut self, extend: bool) {
        let p = self.word_r(self.caret);
        self.move_to(p, extend);
    }
    pub fn backspace(&mut self) {
        if self.selection().is_empty() {
            self.anchor = self.prev(self.caret);
        }
        self.replace("", false);
    }
    pub fn delete(&mut self) {
        if self.selection().is_empty() {
            self.anchor = self.next(self.caret);
        }
        self.replace("", false);
    }
    pub fn delete_word_back(&mut self) {
        if self.selection().is_empty() {
            self.anchor = self.word_l(self.caret);
        }
        self.replace("", false);
    }
    pub fn delete_word_forward(&mut self) {
        if self.selection().is_empty() {
            self.anchor = self.word_r(self.caret);
        }
        self.replace("", false);
    }
    /// The caret at byte `p` (clamped to a character boundary), the selection extended when `extend`.
    pub fn place(&mut self, p: usize, extend: bool) {
        let mut p = p.min(self.text.len());
        while !self.text.is_char_boundary(p) {
            p -= 1;
        }
        self.move_to(p, extend);
    }
    /// Selects the word at the caret (a run of letters and digits, of blanks, or of one kind of other character): what a double
    /// click does.
    pub fn select_word(&mut self) {
        self.pause();
        let class = |c: char| if c.is_alphanumeric() || c == '_' { 0 } else if c.is_whitespace() { 1 } else { 2 };
        let p = self.caret;
        let after = self.text[p..].chars().next();
        let before = self.text[..p].chars().next_back();
        // the character under the caret: the one after it, unless that one is a blank and a word ends here
        let ch = match (before, after) {
            (Some(b), Some(a)) if class(a) == 1 && class(b) != 1 => b,
            (_, Some(a)) => a,
            (Some(b), None) => b,
            (None, None) => return,
        };
        let k = class(ch);
        let (mut a, mut b) = (p, p);
        while let Some(c) = self.text[..a].chars().next_back().filter(|&c| class(c) == k) {
            a -= c.len_utf8();
        }
        while let Some(c) = self.text[b..].chars().next().filter(|&c| class(c) == k) {
            b += c.len_utf8();
        }
        self.anchor = a;
        self.caret = b;
    }
    /// The UTF-16 index of byte offset `p` in what is drawn ([`TextField::display`]: a password shows one bullet a character).
    #[must_use]
    pub fn idx16(&self, p: usize) -> usize {
        let p = p.min(self.text.len());
        if self.password { self.text[..p].chars().count() } else { self.text[..p].encode_utf16().count() }
    }
    /// The byte offset of a UTF-16 index of what is drawn (inside a surrogate pair: its start).
    #[must_use]
    pub fn byte_of(&self, idx: usize) -> usize {
        let (mut units, mut at) = (0, 0);
        for (i, c) in self.text.char_indices() {
            let w = if self.password { 1 } else { c.len_utf16() };
            if units + w > idx {
                return i;
            }
            units += w;
            at = i + c.len_utf8();
        }
        at
    }
    /// Scrolls so that the caret (at `caret_x` in the text's own space) is inside the room, and no more than the text needs.
    pub fn reveal(&mut self, caret_x: f32, text_w: f32, room: f32) {
        let slack = 2.0;
        if caret_x - self.scroll_x > room - slack {
            self.scroll_x = caret_x - room + slack;
        }
        if caret_x < self.scroll_x {
            self.scroll_x = caret_x;
        }
        self.scroll_x = self.scroll_x.min((text_w - room + slack).max(0.0)).max(0.0);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the pointer: a press only says where, in x; the layout that knows the character there is the next frame's

    /// The button went down at window x `x`. Two presses in a row (within half a second and 4 DIPs) select a word, three the whole text.
    pub fn mouse_down(&mut self, x: f32, extend: bool, now: u64) {
        let (t, px, n) = self.view.press;
        let n = if n > 0 && now.saturating_sub(t) < 500 && (x - px).abs() < 4.0 { n % 3 + 1 } else { 1 };
        self.view.press = (now, x, n);
        self.view.touched = now;
        self.view.dragging = true;
        self.pause();
        let at = x - self.view.text_x + self.scroll_x;
        match n {
            1 => self.view.pending = Some((at, Aim::Caret { extend })),
            2 => self.view.pending = Some((at, Aim::Word)),
            _ => {
                self.view.pending = None;
                self.select_all();
            }
        }
    }
    /// The pointer moved with the button held: the selection follows it (the caret goes to its end).
    pub fn mouse_drag(&mut self, x: f32, now: u64) {
        if self.view.dragging && self.view.press.2 < 3 {
            self.view.pending = Some((x - self.view.text_x + self.scroll_x, Aim::Caret { extend: true }));
            self.view.touched = now;
        }
    }
    pub fn mouse_up(&mut self) {
        self.view.dragging = false;
    }
    /// Does what the pointer asked, now that the layout has said which UTF-16 index of the display string `x` is at.
    pub(crate) fn aim(&mut self, idx: usize) {
        let Some((_, how)) = self.view.pending.take() else { return };
        let p = self.byte_of(idx);
        match how {
            Aim::Caret { extend } => self.place(p, extend),
            Aim::Word => {
                self.place(p, false);
                self.select_word();
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the keyboard

    /// A key went down in the field (`WM_KEYDOWN`: the characters themselves come as text). `alt` is held too: AltGr makes Ctrl+Alt,
    /// and a letter with it is a character, not a shortcut.
    pub fn key(&mut self, vk: u32, m: Mods, alt: bool, now: u64, clip: &mut dyn Clip) -> Ev {
        const BACK: u32 = 0x08;
        const ENTER: u32 = 0x0D;
        const ESC: u32 = 0x1B;
        const SPACE: u32 = 0x20;
        const END: u32 = 0x23;
        const HOME: u32 = 0x24;
        const LEFT: u32 = 0x25;
        const RIGHT: u32 = 0x27;
        const INSERT: u32 = 0x2D;
        const DELETE: u32 = 0x2E;
        let (ctrl, shift) = (m.ctrl && !alt, m.shift);
        let before = (self.text.len(), self.caret, self.anchor);
        let ev = match vk {
            LEFT if ctrl => self.moved(|f| f.word_left(shift)),
            LEFT => self.moved(|f| f.left(shift)),
            RIGHT if ctrl => self.moved(|f| f.word_right(shift)),
            RIGHT => self.moved(|f| f.right(shift)),
            HOME => self.moved(|f| f.home(shift)),
            END => self.moved(|f| f.end(shift)),
            BACK if ctrl => self.edited(|f| f.delete_word_back()),
            BACK => self.edited(|f| f.backspace()),
            DELETE if shift && !ctrl => self.edit_cut(clip),
            DELETE if ctrl => self.edited(|f| f.delete_word_forward()),
            DELETE => self.edited(|f| f.delete()),
            INSERT if ctrl => self.edit_copy(clip),
            INSERT if shift => self.edit_paste(clip),
            0x41 if ctrl => self.moved(|f| f.select_all()), // A
            0x43 if ctrl => self.edit_copy(clip),           // C
            0x58 if ctrl => self.edit_cut(clip),            // X
            0x56 if ctrl => self.edit_paste(clip),          // V
            0x5A if ctrl && shift => self.edited(|f| f.redo()), // Ctrl+Shift+Z
            0x5A if ctrl => self.edited(|f| f.undo()),      // Z
            0x59 if ctrl => self.edited(|f| f.redo()),      // Y
            ENTER => Ev::Enter,
            ESC => Ev::Esc,
            // the character comes with WM_CHAR; the key must not also "click" the field
            SPACE => Ev::Used,
            _ => Ev::Ignored,
        };
        if ev != Ev::Ignored {
            self.view.touched = now;
            if (self.text.len(), self.caret, self.anchor) != before {
                self.view.last_edit = now;
            }
        }
        ev
    }
    fn moved(&mut self, f: impl FnOnce(&mut Self)) -> Ev {
        let before = (self.caret, self.anchor);
        f(self);
        if (self.caret, self.anchor) != before { Ev::Moved } else { Ev::Used }
    }
    fn edited(&mut self, f: impl FnOnce(&mut Self)) -> Ev {
        let before = self.text.clone();
        let sel = (self.caret, self.anchor);
        f(self);
        if self.text != before { Ev::Edited } else if (self.caret, self.anchor) != sel { Ev::Moved } else { Ev::Used }
    }
    fn edit_copy(&mut self, clip: &mut dyn Clip) -> Ev {
        // a password does not leave the field
        if !self.password && !self.selection().is_empty() {
            clip.set(self.selected_text());
        }
        Ev::Used
    }
    fn edit_cut(&mut self, clip: &mut dyn Clip) -> Ev {
        if self.password || self.selection().is_empty() {
            return Ev::Used;
        }
        clip.set(self.selected_text());
        self.edited(|f| {
            f.cut();
        })
    }
    fn edit_paste(&mut self, clip: &mut dyn Clip) -> Ev {
        match clip.get() {
            Some(t) => self.edited(|f| f.paste(&t)),
            None => Ev::Used,
        }
    }

    pub fn undo(&mut self) {
        self.pause();
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(s);
        }
    }
    pub fn redo(&mut self) {
        self.pause();
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(s);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_edit_and_selection() {
        let mut f = TextField::default();
        f.insert("привет мир😀");
        f.left(true);
        assert_eq!(f.selected_text(), "😀");
        assert_eq!(f.copy(), "😀");
        assert_eq!(f.cut(), "😀");
        f.undo();
        f.right(false);
        assert_eq!(f.caret, f.text.len());
        f.backspace();
        f.home(false);
        f.delete();
        assert_eq!(f.text, "ривет мир");
        f.select_all();
        f.paste("a\r\nb\nc");
        assert_eq!(f.text, "a b c");
        f.password = true;
        assert_eq!(f.display(), "•••••");
    }
    #[test]
    fn words_and_history() {
        let mut f = TextField::default();
        f.insert("при");
        f.insert("вет");
        f.undo();
        assert_eq!(f.text, "");
        f.redo();
        f.pause();
        f.insert(" мир!");
        f.word_left(false);
        assert_eq!(&f.text[f.caret..], "мир!");
        f.word_right(true);
        assert_eq!(f.selected_text(), "мир!");
        f.left(false);
        assert_eq!(&f.text[f.caret..], "мир!");
        f.delete_word_forward();
        assert_eq!(f.text, "привет ");
        f.end(false);
        f.delete_word_back();
        assert_eq!(f.text, "");
        f.set_text("x😀");
        f.home(false);
        f.right(true);
        f.right(false);
        assert_eq!(f.caret, 1);
    }

    fn ctrl() -> Mods {
        Mods { ctrl: true, shift: false }
    }
    const NONE: Mods = Mods { ctrl: false, shift: false };

    #[test]
    fn shortcuts_go_through_the_clipboard_they_are_given() {
        let mut f = TextField::default();
        let mut clip = MemClip(Some("pasted\r\nline".into()));
        assert_eq!(f.key(0x56, ctrl(), false, 1, &mut clip), Ev::Edited, "Ctrl+V");
        assert_eq!(f.text, "pasted line");
        assert_eq!(f.key(0x41, ctrl(), false, 2, &mut clip), Ev::Moved, "Ctrl+A");
        assert_eq!(f.key(0x41, ctrl(), false, 2, &mut clip), Ev::Used, "Ctrl+A again changes nothing");
        assert_eq!(f.key(0x43, ctrl(), false, 3, &mut clip), Ev::Used, "Ctrl+C");
        assert_eq!(clip.0.as_deref(), Some("pasted line"));
        assert_eq!(f.key(0x58, ctrl(), false, 4, &mut clip), Ev::Edited, "Ctrl+X");
        assert_eq!(f.text, "");
        assert_eq!(f.key(0x5A, ctrl(), false, 5, &mut clip), Ev::Edited, "Ctrl+Z brings it back");
        assert_eq!(f.text, "pasted line");
        assert_eq!(f.key(0x59, ctrl(), false, 6, &mut clip), Ev::Edited, "Ctrl+Y");
        assert_eq!(f.text, "");
        f.key(0x5A, Mods { ctrl: true, shift: false }, false, 7, &mut clip);
        assert_eq!(f.key(0x5A, Mods { ctrl: true, shift: true }, false, 8, &mut clip), Ev::Edited, "Ctrl+Shift+Z redoes");
        assert_eq!(f.text, "");
        // Ctrl+Alt is AltGr: a letter with it is a character (the text comes by WM_CHAR), not a shortcut
        assert_eq!(f.key(0x56, ctrl(), true, 9, &mut clip), Ev::Ignored);
        // a key of the field's own, a key of the app's
        assert_eq!(f.key(0x0D, NONE, false, 10, &mut clip), Ev::Enter);
        assert_eq!(f.key(0x1B, NONE, false, 10, &mut clip), Ev::Esc);
        assert_eq!(f.key(0x20, NONE, false, 10, &mut clip), Ev::Used, "Space is used so that the window does not click the field with it");
        assert!(!f.key(0x28, NONE, false, 10, &mut clip).used() && !f.key(0x09, NONE, false, 10, &mut clip).used(), "Down and Tab are the app's");
        assert!(f.key(0x1B, NONE, false, 10, &mut clip).used());
    }

    #[test]
    fn a_password_neither_copies_nor_cuts() {
        let mut f = TextField { password: true, ..Default::default() };
        f.insert("s3cret😀");
        let mut clip = MemClip(Some("old".into()));
        f.key(0x41, ctrl(), false, 1, &mut clip);
        f.key(0x43, ctrl(), false, 2, &mut clip);
        assert_eq!(f.key(0x58, ctrl(), false, 3, &mut clip), Ev::Used);
        assert_eq!(clip.0.as_deref(), Some("old"), "the clipboard still holds what it held");
        assert_eq!(f.text, "s3cret😀", "and nothing was cut");
        assert_eq!(f.display(), "•••••••");
        assert_eq!((f.idx16(f.text.len()), f.byte_of(6)), (7, "s3cret".len()), "a bullet a character, an emoji is one");
    }

    #[test]
    fn typed_text_is_text_and_a_pause_makes_an_undo_step() {
        let mut f = TextField::default();
        f.typed("a\u{1}b\u{8}\t", 0);
        assert_eq!(f.text, "ab", "control characters are not text");
        f.typed("c", 100);
        f.typed("d", 200);
        f.typed("e", 2000);
        f.undo();
        assert_eq!(f.text, "abcd", "a pause of a second ended the step");
        f.undo();
        assert_eq!(f.text, "", "the typing before it is one step");
        for i in 0..300 {
            f.paste(&i.to_string());
            f.paste("-");
        }
        let mut n = 0;
        while f.undo.pop().is_some() {
            n += 1;
        }
        assert!(n <= UNDO_DEPTH, "the history is bounded ({n})");
    }

    #[test]
    fn a_double_click_selects_a_word_a_triple_the_line() {
        let mut f = TextField::default();
        f.insert("foo bar.baz  qux");
        for (at, word) in [(5, "bar"), (3, "foo"), (7, "."), (8, "baz"), (12, "  "), (16, "qux"), (0, "foo")] {
            f.place(at, false);
            f.select_word();
            assert_eq!(f.selected_text(), word, "at {at}");
        }
        // presses: the text starts at x = 10; the first puts the caret where the layout says, the second selects a word there
        f.view.text_x = 10.0;
        f.mouse_down(40.0, false, 1000);
        assert_eq!(f.view.pending.map(|p| p.0), Some(30.0), "x in the text's own space");
        f.aim(5);
        assert_eq!((f.caret, f.anchor), (5, 5));
        f.mouse_up();
        f.mouse_down(41.0, false, 1200);
        f.aim(5);
        assert_eq!(f.selected_text(), "bar", "the second press of a double click");
        f.mouse_up();
        f.mouse_down(41.0, false, 1300);
        assert_eq!(f.selected_text(), "foo bar.baz  qux", "the third");
        f.mouse_up();
        f.mouse_down(41.0, false, 1400);
        f.aim(3);
        assert_eq!((f.caret, f.anchor), (3, 3), "the fourth begins again");
        // another place, or a long wait, is a first press
        f.mouse_up();
        f.mouse_down(200.0, false, 1500);
        f.aim(16);
        assert_eq!(f.caret, 16);
        f.mouse_up();
        f.mouse_down(41.0, false, 5000);
        f.aim(5);
        f.mouse_drag(100.0, 5010);
        f.aim(9);
        assert_eq!(f.selection(), 5..9, "a drag from the caret placed by the press extends");
        f.mouse_up();
        f.mouse_drag(150.0, 5020);
        assert!(f.view.pending.is_none(), "a pointer that is not held selects nothing");
        // the caret follows a click in a scrolled field
        f.scroll_x = 20.0;
        f.mouse_down(110.0, false, 9000);
        assert_eq!(f.view.pending.map(|p| p.0), Some(120.0));
    }

    #[test]
    fn an_index_of_the_layout_is_a_boundary_of_the_text() {
        let mut f = TextField::default();
        f.insert("a😀б");
        assert_eq!((f.idx16(0), f.idx16(1), f.idx16(5), f.idx16(7), f.idx16(99)), (0, 1, 3, 4, 4));
        assert_eq!((f.byte_of(0), f.byte_of(1), f.byte_of(3), f.byte_of(4), f.byte_of(99)), (0, 1, 5, 7, 7));
        assert_eq!(f.byte_of(2), 1, "inside the emoji: its start");
        f.place(3, false);
        assert_eq!(f.caret, 1, "a byte offset inside a character is moved back to its start");
    }

    #[test]
    fn the_text_scrolls_to_the_caret_and_not_further() {
        let mut f = TextField::default();
        f.reveal(300.0, 300.0, 100.0);
        assert_eq!(f.scroll_x, 202.0, "the caret at the right edge, a little room");
        f.reveal(250.0, 300.0, 100.0);
        assert_eq!(f.scroll_x, 202.0, "still inside: nothing moves");
        f.reveal(100.0, 300.0, 100.0);
        assert_eq!(f.scroll_x, 100.0, "the caret left of the room");
        f.reveal(10.0, 40.0, 100.0);
        assert_eq!(f.scroll_x, 0.0, "the text fits again");
    }
}
