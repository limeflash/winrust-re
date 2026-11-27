//! Command blocks derived from OSC 133 marks.
use std::collections::VecDeque;
use std::ops::Range;

use super::term::{Mark, MarkKind, Term};

#[derive(Clone, Debug)]
pub struct Block {
    pub prompt: u64,
    pub input: Option<(u64, usize)>,
    pub output: Option<u64>,
    pub end: Option<u64>,
    pub exit: Option<i32>,
    pub command: String,
    pub cwd: Option<String>,
    pub started_ms: Option<u64>,
    pub ended_ms: Option<u64>,
    screen_end: u64,
}

impl Block {
    pub fn running(&self) -> bool {
        self.output.is_some() && self.end.is_none()
    }

    pub fn duration_ms(&self) -> Option<u64> {
        Some(self.ended_ms?.saturating_sub(self.started_ms?))
    }

    pub fn output_lines(&self) -> Option<Range<u64>> {
        Some(self.output?..self.end.unwrap_or(self.screen_end))
    }
}

#[derive(Default)]
pub struct Blocks {
    list: VecDeque<Block>,
}

impl Blocks {
    pub fn ingest(&mut self, marks: Vec<Mark>, term: &Term, now_ms: u64) {
        for mark in marks {
            match mark.kind {
                MarkKind::Prompt => {
                    if let Some(block) = self.list.back_mut().filter(|block| block.end.is_none()) {
                        block.end = Some(mark.line);
                        block.ended_ms = Some(now_ms);
                    }
                    self.list.push_back(Block {
                        prompt: mark.line,
                        input: None,
                        output: None,
                        end: None,
                        exit: None,
                        command: String::new(),
                        cwd: None,
                        started_ms: None,
                        ended_ms: None,
                        screen_end: term.end_line(),
                    });
                }
                // `clear` / `cls`: every block so far goes, the running `clear` itself included (its D is then ignored)
                MarkKind::Cleared => self.list.clear(),
                kind => {
                    let Some(block) = self.list.back_mut().filter(|block| block.end.is_none())
                    else {
                        continue;
                    };
                    match kind {
                        MarkKind::Input => block.input = Some((mark.line, mark.col)),
                        MarkKind::Output => {
                            block.output = Some(mark.line);
                            block.command = command(term, block.input, mark.line);
                            block.cwd = term.cwd().map(str::to_owned);
                            block.started_ms = Some(now_ms);
                        }
                        MarkKind::Done(exit) => {
                            block.end = Some(mark.line);
                            block.exit = exit;
                            block.ended_ms = Some(now_ms);
                        }
                        MarkKind::Prompt | MarkKind::Cleared => unreachable!(),
                    }
                }
            }
        }
        if let Some(block) = self.list.back_mut() {
            block.screen_end = term.end_line();
        }
        self.prune(term.first_line());
    }

    /// Drops the blocks whose output has scrolled out of the history entirely. A running block stays (a long build keeps
    /// its header and its pill when it ends), and so does one that still has lines kept.
    pub fn prune(&mut self, first_line: u64) {
        self.list.retain(|block| block.prompt >= first_line || block.end.is_none_or(|end| end > first_line));
    }

    pub fn list(&self) -> &VecDeque<Block> {
        &self.list
    }

    pub fn at(&self, line: u64) -> Option<&Block> {
        self.list.iter().enumerate().rev().find_map(|(i, block)| {
            let end = self
                .list
                .get(i + 1)
                .map_or(block.end.unwrap_or(block.screen_end), |next| next.prompt);
            (line >= block.prompt && line < end).then_some(block)
        })
    }

    pub fn last(&self) -> Option<&Block> {
        self.list.back()
    }
}

fn command(term: &Term, input: Option<(u64, usize)>, end: u64) -> String {
    let Some((start, col)) = input else {
        return String::new();
    };
    if start >= end || start < term.first_line() || end > term.end_line() {
        return String::new();
    }
    let mut lines = Vec::new();
    for line in start..end {
        let Some(cells) = term.line(line) else {
            return String::new();
        };
        let from = if line == start {
            col.min(cells.len())
        } else {
            0
        };
        let text: String = cells[from..]
            .iter()
            .filter(|cell| cell.width != 0)
            .map(|cell| cell.ch)
            .collect();
        lines.push(text.trim_end_matches(' ').to_owned());
    }
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn feed(term: &mut Term, blocks: &mut Blocks, bytes: &[u8], now: u64) {
        term.feed(bytes);
        blocks.ingest(term.take_marks(), term, now);
    }
    fn session() -> (Term, Blocks) {
        let mut term = Term::new(40, 10);
        let mut blocks = Blocks::default();
        feed(&mut term, &mut blocks, b"\x1b]7;file://host/C:/work\x07\x1b]133;A\x07PS> \x1b]133;B\x07echo hi\r\n\x1b]133;C\x07hi\r\n", 100);
        feed(
            &mut term,
            &mut blocks,
            b"\x1b]133;D;0\x07\x1b]133;A\x07PS> \x1b]133;B\x07bad\r\n\x1b]133;C\x07error\r\n",
            150,
        );
        feed(
            &mut term,
            &mut blocks,
            b"\x1b]133;D;1\x07\x1b]133;A\x07PS> \x1b]133;B\x07lost\r\n\x1b]133;C\x07output\r\n",
            200,
        );
        feed(
            &mut term,
            &mut blocks,
            b"\x1b]133;A\x07PS> \x1b]133;B\x07run\r\n\x1b]133;C\x07working",
            250,
        );
        (term, blocks)
    }
    #[test]
    fn session_commands() {
        let (_, b) = session();
        assert_eq!(
            b.list()
                .iter()
                .map(|x| x.command.as_str())
                .collect::<Vec<_>>(),
            ["echo hi", "bad", "lost", "run"]
        );
    }
    #[test]
    fn session_exits() {
        let (_, b) = session();
        assert_eq!(
            b.list().iter().map(|x| x.exit).collect::<Vec<_>>(),
            [Some(0), Some(1), None, None]
        );
    }
    #[test]
    fn session_durations() {
        let (_, b) = session();
        assert_eq!(
            b.list().iter().map(Block::duration_ms).collect::<Vec<_>>(),
            [Some(50), Some(50), Some(50), None]
        );
    }
    #[test]
    fn session_cwd() {
        assert_eq!(session().1.last().unwrap().cwd.as_deref(), Some("C:\\work"));
    }
    #[test]
    fn session_running() {
        let (_, b) = session();
        assert!(b.last().unwrap().running());
        assert!(!b.list()[2].running());
    }
    #[test]
    fn session_ranges() {
        let (t, b) = session();
        assert_eq!(b.list()[0].output_lines(), Some(1..2));
        assert_eq!(b.last().unwrap().output_lines(), Some(7..t.end_line()));
    }
    #[test]
    fn lookup() {
        let (_, b) = session();
        for (line, command) in [
            (0, "echo hi"),
            (1, "echo hi"),
            (2, "bad"),
            (4, "lost"),
            (6, "run"),
        ] {
            assert_eq!(b.at(line).unwrap().command, command);
        }
        assert!(b.at(10).is_none());
    }
    #[test]
    fn ignored_without_prompt() {
        let mut t = Term::new(10, 2);
        let mut b = Blocks::default();
        feed(
            &mut t,
            &mut b,
            b"\x1b]133;B\x07\x1b]133;C\x07\x1b]133;D;0\x07",
            0,
        );
        assert!(b.list().is_empty());
    }
    #[test]
    fn done_without_code() {
        let mut t = Term::new(10, 2);
        let mut b = Blocks::default();
        feed(&mut t, &mut b, b"\x1b]133;A\x07\x1b]133;D\x07", 3);
        assert_eq!(b.last().unwrap().exit, None);
        assert_eq!(b.last().unwrap().end, Some(0));
    }
    #[test]
    fn scroll_prunes() {
        let mut t = Term::new(10, 2);
        let mut b = Blocks::default();
        feed(
            &mut t,
            &mut b,
            b"\x1b]133;A\x07\x1b]133;B\x07x\r\n\x1b]133;C\x07",
            0,
        );
        for _ in 0..20_000 {
            t.feed(b"x\r\n");
        }
        b.ingest(t.take_marks(), &t, 1);
        assert!(b.last().is_some_and(Block::running), "a long build keeps its block while its prompt is gone");
        feed(&mut t, &mut b, b"\x1b]133;D;0\x07", 2);
        assert_eq!(b.last().and_then(|b| b.exit), Some(0), "and its pill when it ends");
        for _ in 0..20_000 {
            t.feed(b"x\r\n");
        }
        b.ingest(t.take_marks(), &t, 3);
        assert!(b.list().is_empty(), "gone once its whole output has scrolled out");
    }
    #[test]
    fn clear_prunes() {
        let mut t = Term::new(10, 2);
        let mut b = Blocks::default();
        feed(&mut t, &mut b, b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07a\r\nb\r\n\x1b]133;D;0\x07", 0);
        feed(&mut t, &mut b, b"\x1b]133;A\x07$ \x1b]133;B\x07clear\r\n\x1b]133;C\x07\x1b[H\x1b[2J\x1b[3J", 1);
        feed(&mut t, &mut b, b"\x1b]133;D;0\x07\x1b]133;A\x07$ ", 2);
        assert_eq!(b.list().len(), 1, "only the new prompt is left");
        assert_eq!(b.last().map(|b| b.prompt), Some(t.first_line()));
    }
    #[test]
    fn multiline_command() {
        let mut t = Term::new(12, 5);
        let mut b = Blocks::default();
        feed(
            &mut t,
            &mut b,
            b"\x1b]133;A\x07> \x1b]133;B\x07first\r\nsecond\r\n\x1b]133;C\x07",
            0,
        );
        assert_eq!(b.last().unwrap().command, "first\nsecond");
    }
    #[test]
    fn missing_input() {
        let mut t = Term::new(10, 2);
        let mut b = Blocks::default();
        feed(&mut t, &mut b, b"\x1b]133;A\x07x\r\n\x1b]133;C\x07", 0);
        assert_eq!(b.last().unwrap().command, "");
    }
    #[test]
    fn running_end_updates() {
        let mut t = Term::new(10, 2);
        let mut b = Blocks::default();
        feed(&mut t, &mut b, b"\x1b]133;A\x07\x1b]133;C\x07", 0);
        feed(&mut t, &mut b, b"x\r\ny\r\nz", 1);
        assert_eq!(b.last().unwrap().output_lines(), Some(0..t.end_line()));
    }
}
