//! Xterm keyboard and paste encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

pub fn encode(key: Key, mods: Mods, app_cursor: bool) -> Vec<u8> {
    let m = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let cursor = match key {
        Key::Up => Some('A'),
        Key::Down => Some('B'),
        Key::Right => Some('C'),
        Key::Left => Some('D'),
        Key::Home => Some('H'),
        Key::End => Some('F'),
        _ => None,
    };
    if let Some(c) = cursor {
        return if m != 1 {
            format!("\x1b[1;{m}{c}")
        } else if app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
        .into_bytes();
    }
    let number = match key {
        Key::Insert => Some(2),
        Key::Delete => Some(3),
        Key::PageUp => Some(5),
        Key::PageDown => Some(6),
        Key::F(n @ 5..=12) => Some([15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]),
        _ => None,
    };
    if let Some(n) = number {
        return if m == 1 {
            format!("\x1b[{n}~")
        } else {
            format!("\x1b[{n};{m}~")
        }
        .into_bytes();
    }
    if let Key::F(n @ 1..=4) = key {
        let c = (b'P' + n - 1) as char;
        return if m == 1 {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[1;{m}{c}")
        }
        .into_bytes();
    }
    let mut out = match key {
        Key::Char(c) => {
            let c = if mods.ctrl {
                match c {
                    'a'..='z' => Some(c as u8 - b'a' + 1),
                    'A'..='Z' => Some(c as u8 - b'A' + 1),
                    ' ' | '@' => Some(0),
                    '[' => Some(27),
                    '\\' => Some(28),
                    ']' => Some(29),
                    '^' => Some(30),
                    '_' => Some(31),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(c) = c {
                vec![c]
            } else if let Key::Char(ch) = key {
                ch.to_string().into_bytes()
            } else {
                unreachable!()
            }
        }
        Key::Enter => vec![13],
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![9],
        Key::Backspace => vec![if mods.ctrl { 8 } else { 127 }],
        Key::Escape => vec![27],
        _ => Vec::new(),
    };
    if mods.alt && !out.is_empty() {
        out.insert(0, 27);
    }
    out
}
pub fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let clean = if bracketed {
        text.replace("\x1b[201~", "")
    } else {
        text.to_owned()
    };
    let clean = clean.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        clean.into_bytes()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_and_modifiers() {
        for (key, c) in [
            (Key::Up, 'A'),
            (Key::Down, 'B'),
            (Key::Right, 'C'),
            (Key::Left, 'D'),
            (Key::Home, 'H'),
            (Key::End, 'F'),
        ] {
            assert_eq!(encode(key, Mods::default(), false), format!("\x1b[{c}").as_bytes());
            assert_eq!(encode(key, Mods::default(), true), format!("\x1bO{c}").as_bytes());
        }
        for n in 0..8 {
            let mods = Mods {
                shift: n & 1 != 0,
                alt: n & 2 != 0,
                ctrl: n & 4 != 0,
            };
            let expected = if n == 0 {
                "\x1b[A".to_owned()
            } else {
                format!("\x1b[1;{}A", n + 1)
            };
            assert_eq!(encode(Key::Up, mods, false), expected.as_bytes());
        }
    }
    #[test]
    fn numbered_keys() {
        for (key, n) in [
            (Key::Insert, 2),
            (Key::Delete, 3),
            (Key::PageUp, 5),
            (Key::PageDown, 6),
            (Key::F(5), 15),
            (Key::F(6), 17),
            (Key::F(7), 18),
            (Key::F(8), 19),
            (Key::F(9), 20),
            (Key::F(10), 21),
            (Key::F(11), 23),
            (Key::F(12), 24),
        ] {
            assert_eq!(encode(key, Mods::default(), false), format!("\x1b[{n}~").as_bytes());
            assert_eq!(
                encode(
                    key,
                    Mods {
                        ctrl: true,
                        ..Mods::default()
                    },
                    false
                ),
                format!("\x1b[{n};5~").as_bytes()
            );
        }
        for n in 1..=4 {
            let c = (b'P' + n - 1) as char;
            assert_eq!(encode(Key::F(n), Mods::default(), false), format!("\x1bO{c}").as_bytes());
            assert_eq!(
                encode(
                    Key::F(n),
                    Mods {
                        shift: true,
                        ..Mods::default()
                    },
                    false
                ),
                format!("\x1b[1;2{c}").as_bytes()
            );
        }
    }
    #[test]
    fn characters_and_controls() {
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        for n in 0..26 {
            assert_eq!(encode(Key::Char((b'a' + n) as char), ctrl, false), vec![n + 1]);
        }
        for (c, n) in [(' ', 0), ('[', 27), ('\\', 28), (']', 29), ('^', 30), ('_', 31)] {
            assert_eq!(encode(Key::Char(c), ctrl, false), vec![n]);
        }
        assert_eq!(encode(Key::Char('?'), Mods::default(), false), "?".as_bytes());
        assert_eq!(
            encode(
                Key::Char('x'),
                Mods {
                    alt: true,
                    ..Mods::default()
                },
                false
            ),
            b"\x1bx"
        );
        for (key, bytes) in [(Key::Enter, 13), (Key::Tab, 9), (Key::Backspace, 127), (Key::Escape, 27)] {
            assert_eq!(encode(key, Mods::default(), false), vec![bytes]);
        }
        assert_eq!(encode(Key::Backspace, ctrl, false), vec![8]);
        assert_eq!(
            encode(
                Key::Tab,
                Mods {
                    shift: true,
                    ..Mods::default()
                },
                false
            ),
            b"\x1b[Z"
        );
    }
    #[test]
    fn paste_modes() {
        assert_eq!(paste("a\r\nb\nc", false), b"a\rb\rc");
        assert_eq!(paste("a\x1b[201~b\n", true), b"\x1b[200~ab\r\x1b[201~");
        assert_eq!(paste("\x1b[201~", false), b"\x1b[201~");
    }
}
