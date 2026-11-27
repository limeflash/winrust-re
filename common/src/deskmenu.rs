//! The desktop's right-click menu, drawn by the bar. rust-explorer's desktop (`--desktop`, GDI, class "Progman", title
//! "rs-desktop") asks the bar to show the design's menu at the pointer ([`ask`]); the bar draws it and posts what was chosen
//! back to the desktop window ([`send`]). Both messages are registered window messages with their data in `wp` and `lp`
//! (no memory crosses the process boundary), so each side's encoding is a pair of plain functions the tests run both ways.
//!
//! Request (desktop -> bar, `SendMessageTimeout`; the bar answers 1 as soon as it takes the request, 0 = cannot show it):
//! `wp` bits 0-1 sort, 2-3 icon size, 4 arrange automatically, 5 icons shown, 6-10 the drive WinRust Disk would scan
//! (0 = WinRust Disk is not installed, 1..26 = A..Z); `lp` is the pointer: x in the low 32 bits, y in the high 32 (screen
//! pixels, either may be negative on a monitor left of or above the primary one).
//!
//! Command (bar -> desktop, `PostMessage`): `wp` is the command, `lp` its argument ([`Cmd::encode`]).
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, PostMessageW, RegisterWindowMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW},
};

/// How the desktop's icons are ordered.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Sort {
    #[default]
    Name,
    Date,
    Size,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Name, Sort::Date, Sort::Size];

    #[must_use]
    pub fn from_u8(n: u8) -> Option<Sort> {
        Self::ALL.get(usize::from(n)).copied()
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        ["name", "date", "size"][self as usize]
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Sort> {
        Self::ALL.into_iter().find(|v| v.name() == s.trim())
    }
}

/// The size of the desktop's icons: the system's 16, 32 (the old "large") and 48 px image lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum IconSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl IconSize {
    pub const ALL: [IconSize; 3] = [IconSize::Small, IconSize::Medium, IconSize::Large];

    #[must_use]
    pub fn from_u8(n: u8) -> Option<IconSize> {
        Self::ALL.get(usize::from(n)).copied()
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        ["small", "medium", "large"][self as usize]
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<IconSize> {
        Self::ALL.into_iter().find(|v| v.name() == s.trim())
    }

    /// The icon's side in pixels.
    #[must_use]
    pub const fn px(self) -> i32 {
        [16, 32, 48][self as usize]
    }
}

/// What the desktop tells the bar when it asks for the menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Request {
    /// The pointer, screen pixels.
    pub at: (i32, i32),
    pub sort: Sort,
    pub size: IconSize,
    /// "Arrange automatically" is on.
    pub auto: bool,
    /// The icons are shown (not hidden).
    pub shown: bool,
    /// The drive WinRust Disk scans when the row is chosen; `None`: it is not installed, the row is left out.
    pub drive: Option<char>,
}

impl Request {
    #[must_use]
    pub fn encode(&self) -> (WPARAM, LPARAM) {
        let drive = self.drive.filter(char::is_ascii_alphabetic).map_or(0, |d| d.to_ascii_uppercase() as usize - 'A' as usize + 1);
        let wp = self.sort as usize | (self.size as usize) << 2 | usize::from(self.auto) << 4 | usize::from(self.shown) << 5 | drive << 6;
        let lp = (u64::from(self.at.0 as u32) | u64::from(self.at.1 as u32) << 32) as i64;
        (wp, lp as LPARAM)
    }

    /// `None` for values no sender makes (a message from anything else).
    #[must_use]
    pub fn decode(wp: WPARAM, lp: LPARAM) -> Option<Request> {
        let drive = (wp >> 6) & 31;
        if wp >> 11 != 0 || drive > 26 {
            return None;
        }
        let lp = lp as u64;
        Some(Request {
            at: (lp as u32 as i32, (lp >> 32) as u32 as i32),
            sort: Sort::from_u8((wp & 3) as u8)?,
            size: IconSize::from_u8(((wp >> 2) & 3) as u8)?,
            auto: wp & 16 != 0,
            shown: wp & 32 != 0,
            drive: (drive != 0).then(|| (b'A' + drive as u8 - 1) as char),
        })
    }
}

/// What the bar asks the desktop to do. The toggles and the segments carry their new value, so a command that arrives twice
/// (or one that finds the state already changed) does the same thing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    NewFolder,
    NewText,
    Paste,
    Sort(Sort),
    Size(IconSize),
    Auto(bool),
    Show(bool),
    Refresh,
    Disk,
    Properties,
}

impl Cmd {
    #[must_use]
    pub fn encode(self) -> (WPARAM, LPARAM) {
        match self {
            Cmd::NewFolder => (1, 0),
            Cmd::NewText => (2, 0),
            Cmd::Paste => (3, 0),
            Cmd::Sort(s) => (4, s as LPARAM),
            Cmd::Size(s) => (5, s as LPARAM),
            Cmd::Auto(on) => (6, LPARAM::from(on)),
            Cmd::Show(on) => (7, LPARAM::from(on)),
            Cmd::Refresh => (8, 0),
            Cmd::Disk => (9, 0),
            Cmd::Properties => (10, 0),
        }
    }

    #[must_use]
    pub fn decode(wp: WPARAM, lp: LPARAM) -> Option<Cmd> {
        let n = u8::try_from(lp).ok();
        let on = match lp {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        };
        Some(match wp {
            1 => Cmd::NewFolder,
            2 => Cmd::NewText,
            3 => Cmd::Paste,
            4 => Cmd::Sort(Sort::from_u8(n?)?),
            5 => Cmd::Size(IconSize::from_u8(n?)?),
            6 => Cmd::Auto(on?),
            7 => Cmd::Show(on?),
            8 => Cmd::Refresh,
            9 => Cmd::Disk,
            10 => Cmd::Properties,
            _ => return None,
        })
    }
}

fn registered(slot: &AtomicU32, name: &str) -> u32 {
    match slot.load(Relaxed) {
        0 => {
            let n: Vec<u16> = name.encode_utf16().chain([0]).collect();
            // SAFETY: a NUL-terminated name.
            let m = unsafe { RegisterWindowMessageW(n.as_ptr()) };
            slot.store(m, Relaxed);
            m
        }
        m => m,
    }
}

/// The message the bar gets when the desktop wants its menu.
pub fn request_msg() -> u32 {
    static M: AtomicU32 = AtomicU32::new(0);
    registered(&M, "WinRustDeskMenu")
}

/// The message the desktop gets with the bar's choice.
pub fn cmd_msg() -> u32 {
    static M: AtomicU32 = AtomicU32::new(0);
    registered(&M, "WinRustDeskCmd")
}

fn find(class: &str, title: Option<&str>) -> HWND {
    let c: Vec<u16> = class.encode_utf16().chain([0]).collect();
    let t: Option<Vec<u16>> = title.map(|t| t.encode_utf16().chain([0]).collect());
    // SAFETY: NUL-terminated names that outlive the call.
    unsafe { FindWindowW(c.as_ptr(), t.as_ref().map_or(std::ptr::null(), |t| t.as_ptr())) }
}

/// The bar's main window (null when the bar is not running).
#[must_use]
pub fn bar() -> HWND {
    find("topbar", None)
}

/// rust-explorer's desktop window (explorer's own is "Progman" too, but titled "Program Manager").
#[must_use]
pub fn desktop() -> HWND {
    find("Progman", Some("rs-desktop"))
}

/// Desktop side: asks the bar to show the menu. `true` = the bar took it (it answers before it opens the panel, so this
/// returns at once); `false` = no bar, a bar that cannot show it, or one that does not answer: the caller shows its own menu.
#[must_use]
pub fn ask(r: &Request) -> bool {
    let bar = bar();
    if bar.is_null() {
        return false;
    }
    // the desktop got the click, so the foreground is ours to give: the bar's panel takes the keyboard (Esc, and closing when the user clicks elsewhere)
    let mut pid = 0u32;
    // SAFETY: reads a window's process id and hands the foreground right to it.
    unsafe {
        GetWindowThreadProcessId(bar, &mut pid);
        AllowSetForegroundWindow(pid);
    }
    let (wp, lp) = r.encode();
    let mut out = 0usize;
    // SAFETY: a message to another process's window with plain integers; a hung bar costs at most the timeout.
    let ok = unsafe { SendMessageTimeoutW(bar, request_msg(), wp, lp, SMTO_ABORTIFHUNG, 600, &mut out) };
    ok != 0 && out == 1
}

/// Bar side: tells the desktop what was chosen (`false`: there is no rust-explorer desktop to tell).
pub fn send(c: Cmd) -> bool {
    let d = desktop();
    if d.is_null() {
        return false;
    }
    let (wp, lp) = c.encode();
    // SAFETY: posts plain integers to a window of another process.
    unsafe { PostMessageW(d, cmd_msg(), wp, lp) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_survives_the_trip_in_every_state_and_for_pointers_on_any_monitor() {
        for sort in Sort::ALL {
            for size in IconSize::ALL {
                for (auto, shown) in [(false, false), (true, false), (false, true), (true, true)] {
                    for drive in [None, Some('C'), Some('Z')] {
                        for at in [(0, 0), (640, 480), (-1920, 17), (30, -1080), (-5, -5), (i32::MAX, i32::MIN)] {
                            let r = Request { at, sort, size, auto, shown, drive };
                            let (wp, lp) = r.encode();
                            assert_eq!(Request::decode(wp, lp), Some(r), "{r:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_request_made_of_nothing_the_desktop_sends_is_refused() {
        let ok = Request { at: (1, 2), sort: Sort::Date, size: IconSize::Large, auto: true, shown: true, drive: Some('D') };
        let (wp, lp) = ok.encode();
        assert_eq!(Request::decode(wp | 3, lp), None, "sort 3");
        assert_eq!(Request::decode((wp & !0b1100) | 0b1100, lp), None, "size 3");
        assert_eq!(Request::decode(wp | 27 << 6, lp), None, "drive 27 and beyond");
        assert_eq!(Request::decode(wp | 1 << 11, lp), None, "a bit nobody sets");
        // a lower-case drive is the same drive
        let lower = Request { drive: Some('d'), ..ok };
        assert_eq!(Request::decode(lower.encode().0, lp).and_then(|r| r.drive), Some('D'));
    }

    #[test]
    fn a_command_survives_the_trip_and_garbage_is_refused() {
        let all = [
            Cmd::NewFolder,
            Cmd::NewText,
            Cmd::Paste,
            Cmd::Sort(Sort::Name),
            Cmd::Sort(Sort::Date),
            Cmd::Sort(Sort::Size),
            Cmd::Size(IconSize::Small),
            Cmd::Size(IconSize::Medium),
            Cmd::Size(IconSize::Large),
            Cmd::Auto(false),
            Cmd::Auto(true),
            Cmd::Show(false),
            Cmd::Show(true),
            Cmd::Refresh,
            Cmd::Disk,
            Cmd::Properties,
        ];
        let mut seen = Vec::new();
        for c in all {
            let (wp, lp) = c.encode();
            assert_eq!(Cmd::decode(wp, lp), Some(c), "{c:?}");
            assert!(!seen.contains(&(wp, lp)), "{c:?} shares its code with another");
            seen.push((wp, lp));
        }
        assert_eq!(Cmd::decode(0, 0), None);
        assert_eq!(Cmd::decode(11, 0), None);
        assert_eq!(Cmd::decode(4, 3), None, "sort 3");
        assert_eq!(Cmd::decode(5, -1), None, "size -1");
        assert_eq!(Cmd::decode(6, 2), None, "a toggle is 0 or 1");
    }

    #[test]
    fn names_and_pixels() {
        assert_eq!(IconSize::ALL.map(IconSize::px), [16, 32, 48]);
        for s in Sort::ALL {
            assert_eq!(Sort::parse(s.name()), Some(s));
        }
        for s in IconSize::ALL {
            assert_eq!(IconSize::parse(&format!(" {}\n", s.name())), Some(s));
        }
        assert_eq!(Sort::parse("type"), None);
    }
}
