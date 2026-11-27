use std::ops::Range;

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
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    typing: bool,
}
impl TextField {
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
    pub fn insert(&mut self, value: &str) {
        self.replace(value, true);
    }
    pub fn paste(&mut self, value: &str) {
        self.replace(
            &value.replace("\r\n", " ").replace(['\r', '\n'], " "),
            false,
        );
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
}
