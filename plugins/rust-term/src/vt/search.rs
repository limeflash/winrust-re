//! Search kept terminal lines using cell columns.
use super::term::Term;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: u64,
    pub start_col: usize,
    pub end_col: usize,
}

pub fn find(term: &Term, needle: &str, case_sensitive: bool) -> Vec<Match> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    // ponytail: matches never span two lines, including wrapped lines.
    for line in term.first_line()..term.end_line() {
        let Some(cells) = term.line(line) else {
            continue;
        };
        let chars: Vec<(usize, char, usize)> = cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.width != 0)
            .map(|(col, cell)| (col, cell.ch, col + usize::from(cell.width)))
            .collect();
        let mut i = 0;
        while i + needle.len() <= chars.len() {
            let equal = needle
                .iter()
                .zip(&chars[i..])
                .all(|(&want, &(_, found, _))| {
                    if case_sensitive {
                        want == found
                    } else {
                        want.to_lowercase().eq(found.to_lowercase())
                    }
                });
            if equal {
                matches.push(Match {
                    line,
                    start_col: chars[i].0,
                    end_col: chars[i + needle.len() - 1].2,
                });
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    fn search(input: &str, needle: &str, sensitive: bool) -> Vec<Match> {
        let mut term = Term::new(40, 2);
        term.feed(input.as_bytes());
        find(&term, needle, sensitive)
    }
    fn m(line: u64, start_col: usize, end_col: usize) -> Match {
        Match {
            line,
            start_col,
            end_col,
        }
    }
    #[test]
    fn plain() {
        assert_eq!(search("hello world", "world", true), vec![m(0, 6, 11)]);
    }
    #[test]
    fn cyrillic() {
        assert_eq!(search("Привет", "привет", false), vec![m(0, 0, 6)]);
    }
    #[test]
    fn case_sensitive() {
        assert!(search("Hello", "hello", true).is_empty());
    }
    #[test]
    fn wide() {
        assert_eq!(search("a界b", "界b", true), vec![m(0, 1, 4)]);
    }
    #[test]
    fn multiple() {
        assert_eq!(search("aba aba", "aba", true), vec![m(0, 0, 3), m(0, 4, 7)]);
    }
    #[test]
    fn non_overlapping() {
        assert_eq!(search("aaaa", "aa", true), vec![m(0, 0, 2), m(0, 2, 4)]);
    }
    #[test]
    fn history_order() {
        assert_eq!(
            search("x\r\nx\r\nx", "x", true),
            vec![m(0, 0, 1), m(1, 0, 1), m(2, 0, 1)]
        );
    }
    #[test]
    fn empty() {
        assert!(search("abc", "", true).is_empty());
    }
    #[test]
    fn no_cross_line() {
        assert!(search("ab\r\ncd", "bc", true).is_empty());
    }
    #[test]
    fn unicode_lowercase_expansion() {
        assert_eq!(search("İ", "i\u{307}", false), Vec::<Match>::new());
    }
    #[test]
    fn absent() {
        assert!(search("abc", "z", false).is_empty());
    }
}
