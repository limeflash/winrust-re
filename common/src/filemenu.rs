//! The right-click menu of files, apps, folders and the Recycle Bin, drawn by the bar. rust-explorer (the desktop, `--desktop`, and
//! the file manager window) asks the bar to show it at the pointer, and the bar draws it from Windows' own list of items.
//!
//! Who does what, and why:
//! * rust-explorer builds the shell's `IContextMenu` for the selected paths, reads its items (text, verb, state, 16 px bitmap,
//!   submenus) into [`ShellItem`]s and keeps the menu alive; it also runs whatever is chosen. It runs as the desktop user: the bar
//!   may be elevated, and a document opened by an elevated process would open in an elevated program.
//! * the bar maps the items onto the design's rows, draws them and tells rust-explorer what was chosen ([`Act`]).
//!
//! Three messages cross the boundary, all small or plain:
//! * the request (rust-explorer -> bar): `WM_COPYDATA` with [`TAG_REQUEST`], answered 1 as soon as the bar has taken it (0 = it
//!   cannot show it: the caller shows its own menu). A path list and a tree of items do not fit in two integers; `WM_COPYDATA`
//!   is what Windows gives a synchronous, bounded, copied hand-over (the data is only valid during the call, so the receiver
//!   decodes it and answers), where a shared file would need a place both integrity levels can write, a name, a clean-up and a
//!   way to tell a stale file from a fresh one.
//! * the items of a submenu (rust-explorer -> bar): `WM_COPYDATA` with [`TAG_SUB`]. Windows fills Send to, Open with and the
//!   submenus of other programs only when they are opened, so the bar asks ([`Act::Fill`]) and the answer comes a moment later.
//! * what was chosen (bar -> rust-explorer): the registered message [`act_msg`], plain integers ([`Act::encode`]).
//!
//! Both directions carry the session number of the menu they belong to, so a late answer for a menu that has gone is ignored.
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::DataExchange::COPYDATASTRUCT,
    UI::WindowsAndMessaging::{AllowSetForegroundWindow, GetWindowThreadProcessId, PostMessageW, RegisterWindowMessageW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA},
};

/// `WM_COPYDATA` `dwData` of a request for the menu.
pub const TAG_REQUEST: usize = 0x5746_0001;
/// `WM_COPYDATA` `dwData` of the items of a submenu.
pub const TAG_SUB: usize = 0x5746_0002;

const MAGIC: u32 = 0x4D46_5257; // "WRFM"
const VERSION: u8 = 1;
/// What a decoder accepts: a hostile or broken sender cannot make the bar allocate without bound.
const MAX_BYTES: usize = 4 << 20;
const MAX_ITEMS: usize = 400;
const MAX_PATHS: usize = 5000;
const MAX_TEXT: usize = 2048;
const MAX_ICON: u16 = 48;
/// Lists nest this deep: the menu and its submenus (the design allows two levels).
const MAX_DEPTH: u8 = 2;

/// Flags of a [`Request`].
pub mod rq {
    /// The desktop's icon list asked (the file manager window did not).
    pub const DESKTOP: u32 = 1;
    /// Exactly one item is selected and its name can be edited in place.
    pub const CAN_RENAME: u32 = 2;
    /// The folder is one of the file manager's pinned places (the row offers to unpin it).
    pub const PINNED: u32 = 4;
    /// The window has tabs (the file manager's; the desktop has none).
    pub const TAB: u32 = 8;
    /// The paths stand for the Recycle Bin (`paths` is empty).
    pub const BIN: u32 = 16;
    /// Windows' menu did not come in time: `items` are rust-explorer's own (command numbers from 0x8000, apart from the shell's
    /// 1..0x7FFF), and the whole list of the shell is a submenu ([`super::MORE_SUB`]) that fills when (if) Windows answers.
    pub const SLOW: u32 = 32;
}

/// The handle of the submenu that holds the shell's whole list when it came late (see [`rq::SLOW`]): the bar asks for it with
/// [`Act::Fill`] like for any lazy submenu, and [`SubReply`] brings it, whenever it is ready.
pub const MORE_SUB: u32 = 0x7FFF_FF01;

/// Flags of a [`ShellItem`].
pub mod fl {
    pub const DISABLED: u8 = 1;
    pub const CHECKED: u8 = 2;
    pub const DEFAULT: u8 = 4;
    pub const RADIO: u8 = 8;
    pub const SEP: u8 = 16;
}

/// A small picture a program gave its menu item: premultiplied BGRA, top row first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub w: u16,
    pub h: u16,
    pub px: Vec<u32>,
}

/// What stands behind an item's arrow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sub {
    None,
    /// A submenu not read yet: Windows fills it only when it is opened. The number is the handle the owner asks for it by.
    Lazy(u32),
    Items(Vec<ShellItem>),
}

/// One item of the shell's menu, as the bar gets it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellItem {
    /// The command number to run it by (0 for a separator and for an item that only opens a submenu).
    pub id: u32,
    /// The canonical verb (`open`, `properties`, `Windows.ModernShare`...), empty for most items of other programs.
    pub verb: String,
    /// The text without its `&` and without what follows a tab.
    pub label: String,
    /// What followed the tab: the shortcut the program names for it.
    pub hint: String,
    pub flags: u8,
    pub icon: Option<Icon>,
    pub sub: Sub,
}

impl ShellItem {
    #[must_use]
    pub fn new(id: u32, verb: &str, label: &str) -> Self {
        Self { id, verb: verb.into(), label: label.into(), hint: String::new(), flags: 0, icon: None, sub: Sub::None }
    }

    #[must_use]
    pub fn sep() -> Self {
        Self { id: 0, verb: String::new(), label: String::new(), hint: String::new(), flags: fl::SEP, icon: None, sub: Sub::None }
    }

    #[must_use]
    pub const fn is_sep(&self) -> bool {
        self.flags & fl::SEP != 0
    }

    #[must_use]
    pub const fn disabled(&self) -> bool {
        self.flags & fl::DISABLED != 0
    }

    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.flags & fl::DEFAULT != 0
    }

    #[must_use]
    pub fn has_sub(&self) -> bool {
        !matches!(self.sub, Sub::None)
    }
}

/// What rust-explorer asks the bar for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Which menu this is: every message about it carries the number.
    pub session: u32,
    /// The pointer, screen pixels.
    pub at: (i32, i32),
    pub flags: u32,
    pub paths: Vec<String>,
    pub items: Vec<ShellItem>,
}

/// The items of a submenu the bar asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubReply {
    pub session: u32,
    pub handle: u32,
    pub items: Vec<ShellItem>,
}

// ---------------------------------------------------------------------------------------------------------------------
// the wire format: little-endian integers, strings as a length and UTF-8

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend(v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend(v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        let mut end = s.len().min(MAX_TEXT);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.u32(end as u32);
        self.0.extend(&s.as_bytes()[..end]);
    }
    fn items(&mut self, items: &[ShellItem], depth: u8) {
        let items = &items[..items.len().min(MAX_ITEMS)];
        self.u32(items.len() as u32);
        for it in items {
            self.u32(it.id);
            self.str(&it.verb);
            self.str(&it.label);
            self.str(&it.hint);
            self.u8(it.flags);
            match &it.icon {
                Some(i) if i.w > 0 && i.h > 0 && i.w <= MAX_ICON && i.h <= MAX_ICON && i.px.len() == usize::from(i.w) * usize::from(i.h) => {
                    self.u16(i.w);
                    self.u16(i.h);
                    for p in &i.px {
                        self.u32(*p);
                    }
                }
                _ => {
                    self.u16(0);
                    self.u16(0);
                }
            }
            match &it.sub {
                Sub::None => self.u8(0),
                Sub::Lazy(h) => {
                    self.u8(1);
                    self.u32(*h);
                }
                // too deep: the children are not sent
                Sub::Items(_) if depth + 1 >= MAX_DEPTH => self.u8(0),
                Sub::Items(v) => {
                    self.u8(2);
                    self.items(v, depth + 1);
                }
            }
        }
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        if n > MAX_TEXT {
            return None;
        }
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn items(&mut self, depth: u8) -> Option<Vec<ShellItem>> {
        let n = self.u32()? as usize;
        // every item takes at least 20 bytes: a count the data cannot hold is refused before anything is reserved
        if n > MAX_ITEMS || depth >= MAX_DEPTH || n * 20 > self.0.len() {
            return None;
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            let (id, verb, label, hint, flags) = (self.u32()?, self.str()?, self.str()?, self.str()?, self.u8()?);
            let (w, h) = (self.u16()?, self.u16()?);
            let icon = if w == 0 && h == 0 {
                None
            } else if w == 0 || h == 0 || w > MAX_ICON || h > MAX_ICON {
                return None;
            } else {
                let bytes = self.take(usize::from(w) * usize::from(h) * 4)?;
                Some(Icon { w, h, px: bytes.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect() })
            };
            let sub = match self.u8()? {
                0 => Sub::None,
                1 => Sub::Lazy(self.u32()?),
                2 => Sub::Items(self.items(depth + 1)?),
                _ => return None,
            };
            v.push(ShellItem { id, verb, label, hint, flags, icon, sub });
        }
        Some(v)
    }
}

impl Request {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W(Vec::new());
        w.u32(MAGIC);
        w.u8(VERSION);
        w.u32(self.session);
        w.i32(self.at.0);
        w.i32(self.at.1);
        w.u32(self.flags);
        let paths = &self.paths[..self.paths.len().min(MAX_PATHS)];
        w.u32(paths.len() as u32);
        for p in paths {
            w.str(p);
        }
        w.items(&self.items, 0);
        w.0
    }

    /// `None` for what no sender makes.
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Request> {
        if data.len() > MAX_BYTES {
            return None;
        }
        let mut r = R(data);
        if r.u32()? != MAGIC || r.u8()? != VERSION {
            return None;
        }
        let (session, x, y, flags) = (r.u32()?, r.i32()?, r.i32()?, r.u32()?);
        let n = r.u32()? as usize;
        if n > MAX_PATHS || n * 4 > r.0.len() {
            return None;
        }
        let paths = (0..n).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
        let items = r.items(0)?;
        r.0.is_empty().then_some(Request { session, at: (x, y), flags, paths, items })
    }
}

impl SubReply {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W(Vec::new());
        w.u32(MAGIC);
        w.u8(VERSION);
        w.u32(self.session);
        w.u32(self.handle);
        // the children of a submenu are one level down: they count as such
        w.items(&self.items, 1);
        w.0
    }

    #[must_use]
    pub fn decode(data: &[u8]) -> Option<SubReply> {
        if data.len() > MAX_BYTES {
            return None;
        }
        let mut r = R(data);
        if r.u32()? != MAGIC || r.u8()? != VERSION {
            return None;
        }
        let (session, handle) = (r.u32()?, r.u32()?);
        let items = r.items(1)?;
        r.0.is_empty().then_some(SubReply { session, handle, items })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// what the bar chose

/// What the bar asks rust-explorer to do for menu `session`. The shell's own items go by their command number; what the shell
/// cannot do from here (it has no folder view to rename in, no progress window for our zip) is rust-explorer's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// Run item `n` of the shell's menu.
    Item(u32),
    /// Read submenu `n` (the bar waits for [`TAG_SUB`]).
    Fill(u32),
    Rename,
    Zip,
    /// "Extract here" for archives.
    Extract,
    CopyPath,
    /// A new tab on the folder (the desktop opens a window).
    NewTab,
    /// A terminal in the folder.
    Terminal,
    /// Pin the folder to the file manager's places, or unpin it.
    TogglePin,
    /// The folder of the shortcut's target, with the target selected.
    Reveal,
    /// The menu closed without a choice (or after one): rust-explorer lets go of it.
    Done,
}

impl Act {
    /// `(wp, lp)` of [`act_msg`]: the action and its number in `wp`, the session in `lp`.
    #[must_use]
    pub fn encode(self, session: u32) -> (WPARAM, LPARAM) {
        let (code, n) = match self {
            Act::Item(n) => (1, n),
            Act::Fill(n) => (2, n),
            Act::Rename => (3, 0),
            Act::Zip => (4, 0),
            Act::Extract => (5, 0),
            Act::CopyPath => (6, 0),
            Act::NewTab => (7, 0),
            Act::Terminal => (8, 0),
            Act::TogglePin => (9, 0),
            Act::Reveal => (10, 0),
            Act::Done => (11, 0),
        };
        (code | (n as usize) << 8, session as LPARAM)
    }

    /// The action and the session; `None` for a message nobody of ours sent.
    #[must_use]
    pub fn decode(wp: WPARAM, lp: LPARAM) -> Option<(Act, u32)> {
        let session = u32::try_from(lp).ok()?;
        let n = u32::try_from(wp >> 8).ok()?;
        let plain = |a| (n == 0).then_some(a);
        let a = match wp & 0xff {
            1 => Act::Item(n),
            2 => Act::Fill(n),
            3 => plain(Act::Rename)?,
            4 => plain(Act::Zip)?,
            5 => plain(Act::Extract)?,
            6 => plain(Act::CopyPath)?,
            7 => plain(Act::NewTab)?,
            8 => plain(Act::Terminal)?,
            9 => plain(Act::TogglePin)?,
            10 => plain(Act::Reveal)?,
            11 => plain(Act::Done)?,
            _ => return None,
        };
        Some((a, session))
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

/// The message rust-explorer gets with the bar's choice.
pub fn act_msg() -> u32 {
    static M: AtomicU32 = AtomicU32::new(0);
    registered(&M, "WinRustFileAct")
}

fn copy_to(to: HWND, from: HWND, tag: usize, data: &[u8], timeout: u32) -> bool {
    let cds = COPYDATASTRUCT { dwData: tag, cbData: data.len() as u32, lpData: data.as_ptr() as *mut _ };
    let mut out = 0usize;
    // SAFETY: the data outlives the call, which does not return before the receiver has copied it (or the timeout); a hung
    // receiver costs at most the timeout.
    let ok = unsafe { SendMessageTimeoutW(to, WM_COPYDATA, from as WPARAM, &cds as *const _ as LPARAM, SMTO_ABORTIFHUNG, timeout, &mut out) };
    ok != 0 && out == 1
}

/// rust-explorer's side: asks the bar to show the menu. `from` is the window the choices go back to. `true` = the bar took it;
/// `false` = no bar, or one that cannot show it or does not answer: the caller shows its own menu.
#[must_use]
pub fn ask(from: HWND, r: &Request) -> bool {
    let bar = crate::deskmenu::bar();
    if bar.is_null() {
        return false;
    }
    let mut pid = 0u32;
    // SAFETY: reads a window's process id and hands the foreground right to it (the panel takes the keyboard).
    unsafe {
        GetWindowThreadProcessId(bar, &mut pid);
        AllowSetForegroundWindow(pid);
    }
    copy_to(bar, from, TAG_REQUEST, &r.encode(), 3000)
}

/// rust-explorer's side: the items of a submenu, for a bar that asked.
#[must_use]
pub fn send_sub(from: HWND, r: &SubReply) -> bool {
    let bar = crate::deskmenu::bar();
    !bar.is_null() && copy_to(bar, from, TAG_SUB, &r.encode(), 1500)
}

/// The bar's side: tells window `to` what was chosen (`false`: the window is gone).
pub fn send_act(to: isize, session: u32, a: Act) -> bool {
    let to = to as HWND;
    if to.is_null() {
        return false;
    }
    let (wp, lp) = a.encode(session);
    let mut pid = 0u32;
    // SAFETY: posts plain integers to a window of another process; what the user just clicked is ours to hand the foreground on
    // from, so what rust-explorer opens (an app, a dialog) comes to the front.
    unsafe {
        GetWindowThreadProcessId(to, &mut pid);
        AllowSetForegroundWindow(pid);
        PostMessageW(to, act_msg(), wp, lp) != 0
    }
}

/// The bar's side: the payload of a `WM_COPYDATA` whose `dwData` is one of ours, and the window that sent it.
///
/// # Safety
/// `lp` must be the `LPARAM` of a `WM_COPYDATA` message being handled now (the data is only valid during it).
#[must_use]
pub unsafe fn copydata<'a>(wp: WPARAM, lp: LPARAM) -> Option<(usize, HWND, &'a [u8])> {
    if lp == 0 {
        return None;
    }
    // SAFETY: the caller's promise: the structure and its data belong to the sender until the message returns.
    let c = unsafe { &*(lp as *const COPYDATASTRUCT) };
    if !matches!(c.dwData, TAG_REQUEST | TAG_SUB) || c.lpData.is_null() || c.cbData as usize > MAX_BYTES {
        return None;
    }
    // SAFETY: as above; `cbData` bytes at `lpData`.
    Some((c.dwData, wp as HWND, unsafe { std::slice::from_raw_parts(c.lpData as *const u8, c.cbData as usize) }))
}

// ---------------------------------------------------------------------------------------------------------------------
// reading an HMENU

pub mod menu {
    use super::{Icon, ShellItem, Sub, fl};
    use windows_sys::Win32::{
        Graphics::Gdi::{BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDC, GetDIBits, GetObjectW, HBITMAP, ReleaseDC},
        UI::WindowsAndMessaging::{GetMenuItemCount, GetMenuItemInfoW, HMENU, MENUITEMINFOW, MIIM_BITMAP, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU},
    };

    const MFT_SEPARATOR: u32 = 0x800;
    const MFT_OWNERDRAW: u32 = 0x100;
    const MFT_RADIOCHECK: u32 = 0x200;
    const MFS_DISABLED: u32 = 0x3;
    const MFS_CHECKED: u32 = 0x8;
    const MFS_DEFAULT: u32 = 0x1000;

    /// What an item of a menu is read with: the verb of a command number, and the filling of a submenu that Windows leaves
    /// empty until it is about to open.
    pub trait Source {
        fn verb(&mut self, id: u32) -> String;
        /// The submenu `m` (item `at` of its parent) is about to open: the owner of the menu is told so (`WM_INITMENUPOPUP`).
        fn opening(&mut self, m: HMENU, at: u32);
        /// Remembers a submenu and returns its handle for [`Sub::Lazy`].
        fn keep(&mut self, m: HMENU, at: u32) -> u32;
    }

    /// `&Open with` -> `Open with`, `a && b` -> `a & b`; what follows a tab is the second part.
    #[must_use]
    pub fn clean(raw: &str) -> (String, String) {
        let (text, hint) = raw.split_once('\t').unwrap_or((raw, ""));
        let strip = |s: &str| {
            let mut out = String::with_capacity(s.len());
            let mut it = s.chars().peekable();
            while let Some(c) = it.next() {
                if c == '&' {
                    // a doubled `&` is one; a single one marks the letter after it and is dropped
                    if it.peek() == Some(&'&') {
                        out.push('&');
                        it.next();
                    }
                } else {
                    out.push(c);
                }
            }
            out.trim().to_string()
        };
        (strip(text), strip(hint))
    }

    /// The text of item `pos` as Windows keeps it.
    unsafe fn text(m: HMENU, pos: u32) -> String {
        // SAFETY: plain Win32 calls on a menu the caller owns; the buffer is as long as the call is told.
        unsafe {
            let mut mi: MENUITEMINFOW = std::mem::zeroed();
            mi.cbSize = size_of::<MENUITEMINFOW>() as u32;
            mi.fMask = MIIM_STRING;
            if GetMenuItemInfoW(m, pos, 1, &mut mi) == 0 || mi.cch == 0 {
                return String::new();
            }
            let mut buf = vec![0u16; mi.cch as usize + 1];
            mi.dwTypeData = buf.as_mut_ptr();
            mi.cch += 1;
            if GetMenuItemInfoW(m, pos, 1, &mut mi) == 0 {
                return String::new();
            }
            String::from_utf16_lossy(&buf[..(mi.cch as usize).min(buf.len() - 1)])
        }
    }

    /// A menu item's bitmap as a small picture: its alpha if it has any (a bitmap without alpha is opaque).
    ///
    /// # Safety
    /// `bm` must be a valid bitmap that is not selected into a device context.
    #[must_use]
    pub unsafe fn bitmap(bm: HBITMAP) -> Option<Icon> {
        // special values of `hbmpItem` (HBMMENU_CALLBACK is -1, the system buttons are 1..=11) are not bitmaps
        if (bm as usize) <= 0xFFFF || bm as isize == -1 {
            return None;
        }
        // SAFETY: GDI reads of a bitmap the caller vouches for, into buffers sized from its own header.
        unsafe {
            let mut b: BITMAP = std::mem::zeroed();
            if GetObjectW(bm, size_of::<BITMAP>() as i32, &mut b as *mut _ as *mut _) == 0 || b.bmWidth <= 0 || b.bmHeight <= 0 {
                return None;
            }
            let (w, h) = (b.bmWidth.min(i32::from(super::MAX_ICON)), b.bmHeight.min(i32::from(super::MAX_ICON)));
            if b.bmWidth > i32::from(super::MAX_ICON) || b.bmHeight > i32::from(super::MAX_ICON) {
                return None; // a picture, not an icon
            }
            let mut px = vec![0u32; (w * h) as usize];
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
            let dc = GetDC(std::ptr::null_mut());
            let rows = GetDIBits(dc, bm, 0, h as u32, px.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
            ReleaseDC(std::ptr::null_mut(), dc);
            (rows != 0).then(|| normalized(w as u16, h as u16, px))
        }
    }

    /// Opaque where the bitmap has no alpha at all; premultiplied where it has alpha that is not.
    #[must_use]
    pub fn normalized(w: u16, h: u16, mut px: Vec<u32>) -> Icon {
        if px.iter().all(|p| p >> 24 == 0) {
            px.iter_mut().for_each(|p| *p |= 0xff00_0000);
        } else if px.iter().any(|p| {
            let a = p >> 24;
            (p >> 16 & 255) > a || (p >> 8 & 255) > a || (p & 255) > a
        }) {
            // straight alpha: Direct2D wants it multiplied in
            for p in &mut px {
                let a = *p >> 24;
                let m = |c: u32| (c * a + 127) / 255;
                *p = a << 24 | m(*p >> 16 & 255) << 16 | m(*p >> 8 & 255) << 8 | m(*p & 255);
            }
        }
        Icon { w, h, px }
    }

    /// The items of `m`: everything Windows put in it, separators as items. A submenu is handed to [`Source::keep`] and read
    /// later with [`fill`], when it is about to open.
    ///
    /// # Safety
    /// `m` must be a valid menu.
    pub unsafe fn read(m: HMENU, src: &mut dyn Source) -> Vec<ShellItem> {
        // SAFETY: plain Win32 calls on a menu the caller owns.
        unsafe {
            let n = GetMenuItemCount(m).max(0) as u32;
            let mut out = Vec::new();
            for pos in 0..n {
                let mut mi: MENUITEMINFOW = std::mem::zeroed();
                mi.cbSize = size_of::<MENUITEMINFOW>() as u32;
                mi.fMask = MIIM_FTYPE | MIIM_ID | MIIM_STATE | MIIM_SUBMENU | MIIM_BITMAP;
                if GetMenuItemInfoW(m, pos, 1, &mut mi) == 0 {
                    continue;
                }
                if mi.fType & MFT_SEPARATOR != 0 {
                    // two in a row, and one at the top, say nothing
                    if !out.is_empty() && !out.last().is_some_and(ShellItem::is_sep) {
                        out.push(ShellItem::sep());
                    }
                    continue;
                }
                let (label, hint) = clean(&text(m, pos));
                let mut flags = 0;
                if mi.fState & MFS_DISABLED != 0 {
                    flags |= fl::DISABLED;
                }
                if mi.fState & MFS_CHECKED != 0 {
                    flags |= fl::CHECKED;
                }
                if mi.fState & MFS_DEFAULT != 0 {
                    flags |= fl::DEFAULT;
                }
                if mi.fType & MFT_RADIOCHECK != 0 {
                    flags |= fl::RADIO;
                }
                let icon = if mi.fType & MFT_OWNERDRAW != 0 { None } else { bitmap(mi.hbmpItem) };
                let (id, sub) = if mi.hSubMenu.is_null() { (mi.wID, Sub::None) } else { (0, Sub::Lazy(src.keep(mi.hSubMenu, pos))) };
                if label.is_empty() && icon.is_none() && sub == Sub::None {
                    continue; // an owner-drawn item that draws its own text: nothing to show
                }
                let verb = if id == 0 { String::new() } else { src.verb(id) };
                out.push(ShellItem { id, verb, label, hint, flags, icon, sub });
            }
            while out.last().is_some_and(ShellItem::is_sep) {
                out.pop();
            }
            out
        }
    }

    /// Reads submenu `m` after telling its owner it is about to open.
    ///
    /// # Safety
    /// `m` must be a valid menu.
    pub unsafe fn fill(m: HMENU, at: u32, src: &mut dyn Source) -> Vec<ShellItem> {
        src.opening(m, at);
        // SAFETY: as `read`.
        unsafe { read(m, src) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp(w: u16, h: u16, fill: u32) -> Icon {
        Icon { w, h, px: vec![fill; usize::from(w) * usize::from(h)] }
    }

    fn sample() -> Vec<ShellItem> {
        let mut open = ShellItem::new(1, "open", "Open");
        open.flags = fl::DEFAULT;
        let mut ze = ShellItem::new(9, "", "7-Zip: Extract “New text document.txt\\” somewhere with a very long label that goes on");
        ze.icon = Some(bmp(16, 16, 0xff11_2233));
        let zip = ShellItem { id: 0, verb: String::new(), label: "7-Zip".into(), hint: String::new(), flags: 0, icon: Some(bmp(16, 16, 0x8000_0040)), sub: Sub::Items(vec![ze, ShellItem::sep(), ShellItem::new(10, "", "Test archive")]) };
        let send = ShellItem { id: 0, verb: "sendto".into(), label: "Send to".into(), hint: String::new(), flags: 0, icon: None, sub: Sub::Lazy(7) };
        let mut off = ShellItem::new(3, "", "Disabled");
        off.flags = fl::DISABLED | fl::CHECKED;
        off.hint = "Ctrl+Shift+X".into();
        vec![open, ShellItem::sep(), zip, send, off]
    }

    #[test]
    fn a_request_survives_the_trip_with_its_items_submenus_and_bitmaps() {
        let r = Request { session: 42, at: (-1920, 30), flags: rq::DESKTOP | rq::CAN_RENAME, paths: vec![r"C:\Users\Me\Desktop\a.txt".into(), "D:\\ü ж 日本\\b.txt".into()], items: sample() };
        assert!(Request::decode(&r.encode()).as_ref() == Some(&r), "the whole request, bitmaps and all");
        let empty = Request { session: 0, at: (0, 0), flags: 0, paths: vec![], items: vec![] };
        assert_eq!(Request::decode(&empty.encode()), Some(empty));
    }

    #[test]
    fn a_reply_survives_the_trip() {
        // a submenu's own children are a level down already: its items carry no further list
        let items = sample().into_iter().map(|i| if matches!(i.sub, Sub::Items(_)) { ShellItem { sub: Sub::Lazy(2), ..i } } else { i }).collect();
        let r = SubReply { session: 5, handle: 7, items };
        assert!(SubReply::decode(&r.encode()).as_ref() == Some(&r));
        // and what is deeper than that is not sent
        let deep = SubReply { session: 5, handle: 7, items: sample() };
        let back = SubReply::decode(&deep.encode()).unwrap();
        assert!(back.items.iter().all(|i| !matches!(i.sub, Sub::Items(_))));
    }

    #[test]
    fn garbage_and_cut_messages_are_refused_without_panicking() {
        let good = Request { session: 1, at: (1, 2), flags: 0, paths: vec!["C:\\a".into()], items: sample() }.encode();
        assert!(Request::decode(&good).is_some());
        // every prefix of a good message is a cut one
        for n in 0..good.len() {
            assert_eq!(Request::decode(&good[..n]), None, "cut at {n}");
        }
        let mut extra = good.clone();
        extra.push(0);
        assert_eq!(Request::decode(&extra), None, "trailing bytes");
        let mut bad_magic = good.clone();
        bad_magic[0] ^= 1;
        assert_eq!(Request::decode(&bad_magic), None);
        let mut bad_version = good.clone();
        bad_version[4] = 9;
        assert_eq!(Request::decode(&bad_version), None);
        // a count that the data cannot hold must not be believed
        let mut huge = good.clone();
        huge[9 + 12..9 + 16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Request::decode(&huge), None);
        // a pseudo-random flood of bytes
        let mut x = 0x1234_5678u32;
        for len in [0usize, 1, 7, 64, 500, 4000] {
            let junk: Vec<u8> = (0..len).map(|_| { x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223); (x >> 24) as u8 }).collect();
            assert_eq!(Request::decode(&junk), None);
            assert_eq!(SubReply::decode(&junk), None);
        }
        assert_eq!(Request::decode(&vec![0u8; MAX_BYTES + 1]), None);
    }

    #[test]
    fn long_text_is_cut_on_a_character_and_deep_menus_lose_what_is_too_deep() {
        let long = "ж".repeat(3000);
        let r = Request { session: 1, at: (0, 0), flags: 0, paths: vec![long.clone()], items: vec![ShellItem::new(1, "", &long)] };
        let back = Request::decode(&r.encode()).unwrap();
        assert_eq!(back.paths[0].len(), MAX_TEXT, "cut at a boundary (2 bytes a letter, an even number)");
        assert!(back.items[0].label.chars().all(|c| c == 'ж'));
        let leaf = ShellItem::new(5, "", "leaf");
        let mid = ShellItem { sub: Sub::Items(vec![leaf]), ..ShellItem::new(0, "", "mid") };
        let top = ShellItem { sub: Sub::Items(vec![mid]), ..ShellItem::new(0, "", "top") };
        let back = Request::decode(&Request { session: 1, at: (0, 0), flags: 0, paths: vec![], items: vec![top] }.encode()).unwrap();
        let Sub::Items(l1) = &back.items[0].sub else { panic!("one level down is kept") };
        assert_eq!(l1[0].sub, Sub::None, "two down is dropped");
    }

    #[test]
    fn an_oversized_or_misshapen_bitmap_is_not_sent_and_is_refused_when_it_arrives() {
        let mut it = ShellItem::new(1, "", "x");
        it.icon = Some(Icon { w: 16, h: 16, px: vec![0; 10] }); // fewer pixels than it says
        let back = Request::decode(&Request { session: 1, at: (0, 0), flags: 0, paths: vec![], items: vec![it] }.encode()).unwrap();
        assert_eq!(back.items[0].icon, None);
        let mut it = ShellItem::new(1, "", "x");
        it.icon = Some(bmp(MAX_ICON + 1, 2, 0));
        let back = Request::decode(&Request { session: 1, at: (0, 0), flags: 0, paths: vec![], items: vec![it] }.encode()).unwrap();
        assert_eq!(back.items[0].icon, None);
    }

    #[test]
    fn an_act_survives_the_trip_and_garbage_is_refused() {
        let all = [Act::Item(0x7FFF), Act::Item(1), Act::Fill(3), Act::Rename, Act::Zip, Act::Extract, Act::CopyPath, Act::NewTab, Act::Terminal, Act::TogglePin, Act::Reveal, Act::Done];
        let mut seen = Vec::new();
        for a in all {
            for s in [0u32, 1, u32::MAX] {
                let (wp, lp) = a.encode(s);
                assert_eq!(Act::decode(wp, lp), Some((a, s)), "{a:?}");
            }
            let w = a.encode(1).0;
            assert!(!seen.contains(&w), "{a:?} shares its code");
            seen.push(w);
        }
        assert_eq!(Act::decode(0, 1), None);
        assert_eq!(Act::decode(99, 1), None);
        assert_eq!(Act::decode(3 | 5 << 8, 1), None, "Rename has no number");
        assert_eq!(Act::decode(1, -1), None, "a session is not negative");
        assert_eq!(Act::decode(1 | (1 << 40), 1), None, "a number past u32");
    }

    #[test]
    fn mnemonics_and_accelerators_are_taken_out_of_a_label() {
        assert_eq!(menu::clean("&Open with"), ("Open with".into(), String::new()));
        assert_eq!(menu::clean("Cu&t\tCtrl+X"), ("Cut".into(), "Ctrl+X".into()));
        assert_eq!(menu::clean("Tom && Jerry"), ("Tom & Jerry".into(), String::new()));
        assert_eq!(menu::clean("  Spaced  "), ("Spaced".into(), String::new()));
        assert_eq!(menu::clean("Открыть(&O)"), ("Открыть(O)".into(), String::new()));
    }

    #[test]
    fn bitmaps_without_alpha_are_opaque_and_straight_alpha_is_multiplied() {
        let opaque = menu::normalized(2, 1, vec![0x0011_2233, 0x00ff_00ff]);
        assert_eq!(opaque.px, vec![0xff11_2233, 0xffff_00ff]);
        let kept = menu::normalized(1, 1, vec![0x8040_2010]);
        assert_eq!(kept.px, vec![0x8040_2010], "premultiplied already (no channel above alpha)");
        let straight = menu::normalized(1, 1, vec![0x80ff_ffff]);
        assert_eq!(straight.px[0] >> 24, 0x80);
        assert_eq!(straight.px[0] & 255, 0x80, "255 * 128 / 255");
    }

    /// A real HMENU read the way a shell's menu is: a default item, separators (leading, doubled, trailing), a submenu that is
    /// only handed out as a handle, a disabled and a checked item with an accelerator, a long third-party label with a bitmap.
    #[test]
    fn a_real_menu_is_read_into_items() {
        use windows_sys::Win32::{
            Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject},
            UI::WindowsAndMessaging::{AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, InsertMenuItemW, MENUITEMINFOW, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MIIM_BITMAP, MIIM_FTYPE, MIIM_ID, MIIM_STRING, MFT_STRING, SetMenuDefaultItem},
        };
        struct Fake {
            subs: Vec<HMENU>,
            opened: Vec<HMENU>,
        }
        impl menu::Source for Fake {
            fn verb(&mut self, id: u32) -> String {
                if id == 100 { "open".into() } else { String::new() }
            }
            fn opening(&mut self, m: HMENU, _at: u32) {
                self.opened.push(m);
            }
            fn keep(&mut self, m: HMENU, _at: u32) -> u32 {
                self.subs.push(m);
                self.subs.len() as u32
            }
        }
        let w = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        // SAFETY: Win32 calls on menus and a bitmap this test makes and destroys.
        unsafe {
            let (m, sub) = (CreatePopupMenu(), CreatePopupMenu());
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(m, MF_STRING, 100, w("&Open\tEnter").as_ptr());
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(sub, MF_STRING, 300, w("Notepad").as_ptr());
            AppendMenuW(m, MF_POPUP | MF_STRING, sub as usize, w("Open &with").as_ptr());
            AppendMenuW(m, MF_STRING | MF_DISABLED | MF_GRAYED | MF_CHECKED, 101, w("Locked\tCtrl+L").as_ptr());
            // a 16 x 16 bitmap with alpha, as a third-party extension gives its item
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader = BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: 16, biHeight: -16, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
            let mut bits = std::ptr::null_mut();
            let hb = CreateDIBSection(std::ptr::null_mut(), &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            std::slice::from_raw_parts_mut(bits as *mut u32, 256).fill(0xff20_4060);
            let label = w("Scan with Microsoft Defender… and then some more words to make the label long");
            let mut mi: MENUITEMINFOW = std::mem::zeroed();
            mi.cbSize = size_of::<MENUITEMINFOW>() as u32;
            mi.fMask = MIIM_FTYPE | MIIM_ID | MIIM_STRING | MIIM_BITMAP;
            mi.fType = MFT_STRING;
            mi.wID = 200;
            mi.dwTypeData = label.as_ptr() as *mut u16;
            mi.hbmpItem = hb;
            InsertMenuItemW(m, 99, 1, &mi);
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            SetMenuDefaultItem(m, 100, 0);

            let mut src = Fake { subs: vec![], opened: vec![] };
            let items = menu::read(m, &mut src);
            let names: Vec<(&str, u32, &str)> = items.iter().map(|i| (i.label.as_str(), i.id, i.verb.as_str())).collect();
            assert_eq!(
                names,
                vec![("Open", 100, "open"), ("", 0, ""), ("Open with", 0, ""), ("Locked", 101, ""), ("Scan with Microsoft Defender… and then some more words to make the label long", 200, "")],
                "a leading, a doubled and a trailing separator are gone"
            );
            assert!(items[0].is_default() && items[0].hint == "Enter");
            assert_eq!(items[2].sub, Sub::Lazy(1));
            assert_eq!(src.subs, vec![sub]);
            assert!(src.opened.is_empty(), "nothing is opened until asked");
            assert!(items[3].disabled() && items[3].flags & fl::CHECKED != 0 && items[3].hint == "Ctrl+L");
            let icon = items[4].icon.as_ref().expect("the bitmap");
            assert_eq!((icon.w, icon.h, icon.px[0]), (16, 16, 0xff20_4060));

            let kids = menu::fill(sub, 2, &mut src);
            assert_eq!(src.opened, vec![sub]);
            assert_eq!(kids.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Notepad"]);
            DeleteObject(hb);
            DestroyMenu(m);
        }
    }
}
