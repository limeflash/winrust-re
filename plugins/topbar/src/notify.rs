//! Notifications: the model behind the notification centre, the pop-ups and the badge on the clock.
//!
//! Where they come from: WinRust's own notices (a saved screenshot, a saved snip) and the balloons of tray icons
//! (`Shell_NotifyIcon` with `NIF_INFO`, which our tray host receives). Other apps' Windows toasts cannot be read by a program
//! without a package identity (`UserNotificationListener` answers "Element not found" to an unpackaged exe), so there is no
//! third source.
//!
//! The model is plain data and has no window in it: [`Store`] holds the notes (newest first, at most [`MAX_NOTES`]), what the
//! centre shows (which stacks are open, the reply being typed, the highlight), the pop-ups on screen and the Do-not-disturb
//! switch. Everything takes the time as an argument (milliseconds of one clock), so it can be tested. The windows read it
//! through [`with`] and change it through the same door.

use std::cell::RefCell;

/// Most notes kept in memory; the oldest go first.
pub const MAX_NOTES: usize = 50;
/// Pop-ups on screen at once (the mockup's `slice(-3)`).
pub const MAX_POPS: usize = 3;
/// How long a dismissed note collapses before it is forgotten (`.nciw` transition .3 s).
pub const GONE_MS: u64 = 300;
/// How long a pop-up that was dismissed stays while it flies away (the mockup removes it after 200 ms).
pub const POP_EXIT_MS: u64 = 200;
/// How long "Clear all" can be undone.
pub const UNDO_MS: u64 = 6000;
/// How long a pop-up stays: 6 s, 8 s with a picture (`dur` of the mockup's card).
pub const POP_LIFE: u64 = 6000;
pub const POP_LIFE_PICTURE: u64 = 8000;
/// How long the highlight ring of a note opened from its pop-up lasts (`nciHl` 1.4 s).
pub const HIGHLIGHT_MS: u64 = 1400;

// ---------------------------------------------------------------------------------------------------------------------
// what a note is

/// The face of an app in a note: the mark of WinRust itself, a coloured plate with a glyph, or the icon of an exe.
#[derive(Clone, Debug, PartialEq)]
pub enum AppIcon {
    Winrust,
    /// A 24x24 stroke path on a plate going from one colour to another (`0xRRGGBB`).
    Plate(&'static str, u32, u32),
    /// The icon of this file, as the shell draws it.
    Exe(String),
}

/// An app that sends notes: notes are grouped by `key`.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub key: String,
    pub name: String,
    pub icon: AppIcon,
}

/// What a button under a note does.
#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    /// Opens a file with its program.
    Open(String),
    /// Shows a file in Explorer.
    Show(String),
    /// The note is read, nothing else happens ("Mark as read").
    Read,
    /// An answer is typed under the note. Only a source that can take it offers this button.
    Reply,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Act {
    pub label: String,
    pub does: Do,
}

/// What a click on the note itself opens.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Nothing,
    File(String),
    /// The icon that showed the balloon: the click is told to its window (`NIN_BALLOONUSERCLICK`).
    Balloon { hwnd: isize, uid: u32 },
}

/// The round letter in front of a chat note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avatar {
    pub letter: char,
    pub from: u32,
    pub to: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: u64,
    pub app: App,
    pub title: String,
    pub body: String,
    pub acts: Vec<Act>,
    /// A picture file shown under the text (a screenshot).
    pub image: Option<String>,
    /// The picture is the cover of a video of this length (`0:24`): a play button over it and the length in a corner.
    pub video: Option<String>,
    pub avatar: Option<Avatar>,
    /// The first button is red (a warning).
    pub danger: bool,
    pub target: Target,
    pub born: u64,
    /// Arrived since the centre was last open.
    pub unread: bool,
    /// Arrived since the centre was last open: it comes in with `nciIn` when the centre shows it.
    pub fresh: bool,
    /// Dismissed at this time (it may be in the future: a group cleared one note after another); it collapses and goes.
    pub gone: Option<u64>,
}

impl Note {
    #[must_use]
    pub fn new(app: App, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            id: 0,
            app,
            title: title.into(),
            body: body.into(),
            acts: Vec::new(),
            image: None,
            video: None,
            avatar: None,
            danger: false,
            target: Target::Nothing,
            born: 0,
            unread: true,
            fresh: false,
            gone: None,
        }
    }

    #[must_use]
    pub fn acts(mut self, acts: Vec<Act>) -> Self {
        self.acts = acts;
        self
    }

    #[must_use]
    pub fn image(mut self, path: impl Into<String>) -> Self {
        self.image = Some(path.into());
        self
    }

    #[must_use]
    pub fn video(mut self, length: impl Into<String>) -> Self {
        self.video = Some(length.into());
        self
    }

    #[must_use]
    pub fn avatar(mut self, letter: char, from: u32, to: u32) -> Self {
        self.avatar = Some(Avatar { letter, from, to });
        self
    }

    #[must_use]
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    #[must_use]
    pub fn target(mut self, t: Target) -> Self {
        self.target = t;
        self
    }

    /// How long its pop-up stays.
    #[must_use]
    pub const fn life(&self) -> u64 {
        if self.image.is_some() { POP_LIFE_PICTURE } else { POP_LIFE }
    }
}

/// How long ago a note arrived, for the label on its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ago {
    Now,
    Min(u64),
    Hour(u64),
    Day(u64),
}

#[must_use]
pub const fn ago(now: u64, born: u64) -> Ago {
    let min = now.saturating_sub(born) / 60_000;
    match min {
        0 => Ago::Now,
        1..=59 => Ago::Min(min),
        60..=1439 => Ago::Hour(min / 60),
        _ => Ago::Day(min / 1440),
    }
}

/// The notes of one app, in the order they stand in the centre.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub app: App,
    /// All its notes, newest first.
    pub ids: Vec<u64>,
    /// Shown one under another; a stack of more than two is closed until it is opened.
    pub open: bool,
}

impl Group {
    /// More than two notes: a "+N" pill opens and closes the stack.
    #[must_use]
    pub const fn can_expand(&self) -> bool {
        self.ids.len() > 2
    }

    /// The notes on show: all, or the newest one with the rest peeking from under it.
    #[must_use]
    pub fn shown(&self) -> &[u64] {
        if self.open { &self.ids } else { &self.ids[..1] }
    }
}

/// A pop-up on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pop {
    pub id: u64,
    pub born: u64,
    /// Dismissed at this time: it flies away.
    pub out: Option<u64>,
    /// Time it has stood still (hovered, being answered).
    held: u64,
    since: Option<u64>,
}

/// The answer being typed under a note.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub id: u64,
    pub text: String,
}

// ---------------------------------------------------------------------------------------------------------------------
// the store

#[derive(Default)]
pub struct Store {
    /// Newest first.
    notes: Vec<Note>,
    next: u64,
    dnd: bool,
    /// The centre is on screen: nothing pops up, what arrives is waiting there.
    centre: bool,
    /// Stacks the user opened (or closed) by hand: app key and whether it is open.
    opened: Vec<(String, bool)>,
    pops: Vec<Pop>,
    reply: Option<Reply>,
    highlight: Option<(u64, u64)>,
    undo: Option<(Vec<Note>, u64)>,
}

impl Store {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    // -- reading

    /// Every note, newest first (those on their way out too).
    #[must_use]
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    #[must_use]
    pub fn note(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Notes that are not dismissed.
    #[must_use]
    pub fn count(&self) -> usize {
        self.notes.iter().filter(|n| n.gone.is_none()).count()
    }

    #[must_use]
    pub fn unread(&self) -> usize {
        self.notes.iter().filter(|n| n.unread && n.gone.is_none()).count()
    }

    #[must_use]
    pub const fn dnd(&self) -> bool {
        self.dnd
    }

    /// The little number on the clock, `None` when there is none to show: nothing unread, Do not disturb is on, or the centre
    /// is open and shows them itself.
    #[must_use]
    pub fn badge(&self) -> Option<usize> {
        let n = self.unread();
        (!self.dnd && n > 0 && !self.centre).then_some(n)
    }

    #[must_use]
    pub const fn centre_open(&self) -> bool {
        self.centre
    }

    #[must_use]
    pub const fn pops(&self) -> &[Pop] {
        // not `&mut`: a pop-up that is leaving is still listed
        self.pops.as_slice()
    }

    #[must_use]
    pub const fn reply(&self) -> Option<&Reply> {
        self.reply.as_ref()
    }

    /// The note whose highlight ring is lit at `now`, and how far along (0..1) the ring's 1.4 s is.
    #[must_use]
    pub fn highlight(&self, now: u64) -> Option<(u64, f32)> {
        let (id, since) = self.highlight?;
        let age = now.saturating_sub(since);
        (age < HIGHLIGHT_MS).then(|| (id, age as f32 / HIGHLIGHT_MS as f32))
    }

    /// What "Clear all" can still take back.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.undo.is_some()
    }

    /// The groups in the order they stand: by the newest note of each app.
    #[must_use]
    pub fn groups(&self) -> Vec<Group> {
        let mut out: Vec<Group> = Vec::new();
        for n in &self.notes {
            match out.iter_mut().find(|g| g.app.key == n.app.key) {
                Some(g) => g.ids.push(n.id),
                None => out.push(Group { app: n.app.clone(), ids: vec![n.id], open: false }),
            }
        }
        for g in &mut out {
            let hand = self.opened.iter().find(|(k, _)| *k == g.app.key).map(|(_, o)| *o);
            g.open = hand.unwrap_or(false) || g.ids.len() <= 2;
        }
        out
    }

    // -- changing

    /// A new note: it stands first, unread; it pops up unless Do not disturb is on or the centre is open. Returns its id.
    pub fn push(&mut self, n: Note, now: u64) -> u64 {
        self.push_quiet(n, now, false)
    }

    /// Like [`Store::push`]; `quiet` keeps it from popping up (something full-screen has the screen).
    pub fn push_quiet(&mut self, mut n: Note, now: u64, quiet: bool) -> u64 {
        self.next += 1;
        n.id = self.next;
        n.born = now;
        n.unread = true;
        n.fresh = true;
        n.gone = None;
        let id = n.id;
        self.notes.insert(0, n);
        if self.notes.len() > MAX_NOTES {
            self.notes.truncate(MAX_NOTES);
            let live: Vec<u64> = self.notes.iter().map(|n| n.id).collect();
            self.pops.retain(|p| live.contains(&p.id));
        }
        if !self.dnd && !self.centre && !quiet {
            self.pops.push(Pop { id, born: now, out: None, held: 0, since: None });
            if self.pops.len() > MAX_POPS {
                self.pops.remove(0);
            }
        }
        id
    }

    /// Moves time on: pop-ups that have had their time fly away, those that have flown and the notes that have collapsed are
    /// forgotten. True if anything changed.
    pub fn tick(&mut self, now: u64) -> bool {
        let before = (self.notes.len(), self.pops.len(), self.undo.is_some());
        let expired: Vec<u64> = self
            .pops
            .iter()
            .filter(|p| p.out.is_none() && self.reply.as_ref().is_none_or(|r| r.id != p.id))
            .filter(|p| self.note(p.id).is_none_or(|n| p.age(now) >= n.life()))
            .map(|p| p.id)
            .collect();
        let mut changed = !expired.is_empty();
        for id in expired {
            self.pop_off(id, now);
        }
        self.pops.retain(|p| p.out.is_none_or(|t| now < t + POP_EXIT_MS));
        self.notes.retain(|n| n.gone.is_none_or(|t| now < t + GONE_MS));
        if self.undo.as_ref().is_some_and(|(_, t)| now >= *t) {
            self.undo = None;
        }
        let live: Vec<u64> = self.notes.iter().map(|n| n.id).collect();
        self.pops.retain(|p| live.contains(&p.id));
        if self.reply.as_ref().is_some_and(|r| !live.contains(&r.id)) {
            self.reply = None;
        }
        changed |= before != (self.notes.len(), self.pops.len(), self.undo.is_some());
        changed
    }

    /// A note is dismissed (`at` is when its collapse starts).
    pub fn dismiss_at(&mut self, id: u64, at: u64) {
        if let Some(n) = self.notes.iter_mut().find(|n| n.id == id && n.gone.is_none()) {
            n.gone = Some(at);
            self.pops.retain(|p| p.id != id);
            if self.reply.as_ref().is_some_and(|r| r.id == id) {
                self.reply = None;
            }
        }
    }

    pub fn dismiss(&mut self, id: u64, now: u64) {
        self.dismiss_at(id, now);
    }

    /// The x of a group: its notes go one after another, 40 ms apart.
    pub fn clear_app(&mut self, key: &str, now: u64) {
        let ids: Vec<u64> = self.notes.iter().filter(|n| n.app.key == key && n.gone.is_none()).map(|n| n.id).collect();
        for (k, id) in ids.into_iter().enumerate() {
            self.dismiss_at(id, now + 40 * k as u64);
        }
    }

    /// The description of an app was read: its notes (key `key`) that still carry the file's name `old` (`stem`) take `name`, in the
    /// app's line and in the title that was filled in from it. `true` if any changed.
    pub fn rename_app(&mut self, key: &str, old: &str, name: &str) -> bool {
        let mut any = false;
        for n in self.notes.iter_mut().filter(|n| n.app.key == key && n.app.name == old && old != name) {
            if n.title == n.app.name {
                n.title = name.to_string();
            }
            n.app.name = name.to_string();
            any = true;
        }
        any
    }

    /// "Clear all": everything collapses at once and can be brought back for [`UNDO_MS`].
    pub fn clear_all(&mut self, now: u64) {
        let kept: Vec<Note> = self.notes.iter().filter(|n| n.gone.is_none()).cloned().collect();
        let ids: Vec<u64> = kept.iter().map(|n| n.id).collect();
        for id in ids {
            self.dismiss_at(id, now);
        }
        if !kept.is_empty() {
            self.undo = Some((kept, now + UNDO_MS));
        }
        self.opened.clear();
    }

    /// "Undo" under the empty centre: the cleared notes are back as they were.
    pub fn undo(&mut self) -> bool {
        let Some((kept, _)) = self.undo.take() else { return false };
        self.notes.retain(|n| n.gone.is_none());
        for n in kept {
            if !self.notes.iter().any(|m| m.id == n.id) {
                self.notes.push(Note { gone: None, fresh: false, ..n });
            }
        }
        self.notes.sort_by_key(|n| std::cmp::Reverse(n.id));
        self.notes.truncate(MAX_NOTES);
        true
    }

    /// The newest note is this very one, a moment ago: an app that sends its balloon again does not add a second card.
    #[must_use]
    pub fn is_repeat(&self, n: &Note, now: u64) -> bool {
        self.notes.first().is_some_and(|m| m.gone.is_none() && m.app.key == n.app.key && m.title == n.title && m.body == n.body && now.saturating_sub(m.born) < 3000)
    }

    pub fn mark_read(&mut self, id: u64) {
        if let Some(n) = self.notes.iter_mut().find(|n| n.id == id && n.unread) {
            n.unread = false;
        }
    }

    /// The centre opens: pop-ups go (what was on screen is in the centre now), and what arrives from now on does not pop up.
    pub fn open_centre(&mut self) {
        self.centre = true;
        self.pops.clear();
    }

    /// The centre closes: everything was seen.
    pub fn close_centre(&mut self) {
        self.centre = false;
        self.reply = None;
        for n in &mut self.notes {
            n.unread = false;
            n.fresh = false;
        }
    }

    /// Do not disturb: turning it on takes the pop-ups away.
    pub fn set_dnd(&mut self, on: bool) {
        if self.dnd != on {
            self.dnd = on;
            if on {
                self.pops.clear();
            }
        }
    }

    /// The "+N" pill or a click on a closed stack.
    pub fn toggle_stack(&mut self, key: &str) {
        let open = self.groups().iter().find(|g| g.app.key == key).is_some_and(|g| g.open);
        match self.opened.iter_mut().find(|(k, _)| k == key) {
            Some((_, o)) => *o = !open,
            None => self.opened.push((key.to_string(), !open)),
        }
    }

    pub fn open_stack(&mut self, key: &str) {
        if !self.groups().iter().find(|g| g.app.key == key).is_some_and(|g| g.open) {
            self.toggle_stack(key);
        }
    }

    // -- pop-ups

    /// A pop-up is dismissed (its x, its time, a click): it flies away.
    pub fn pop_off(&mut self, id: u64, now: u64) {
        if let Some(p) = self.pops.iter_mut().find(|p| p.id == id && p.out.is_none()) {
            p.out = Some(now);
        }
    }

    /// A pop-up leaves at once (swiped away).
    pub fn pop_drop(&mut self, id: u64) {
        self.pops.retain(|p| p.id != id);
    }

    /// The pointer is over a pop-up (or it is being answered): its clock stands still.
    pub fn pop_hold(&mut self, id: u64, hold: bool, now: u64) {
        if let Some(p) = self.pops.iter_mut().find(|p| p.id == id) {
            match (hold, p.since) {
                (true, None) => p.since = Some(now),
                (false, Some(t)) => {
                    p.held += now.saturating_sub(t);
                    p.since = None;
                }
                _ => {}
            }
        }
    }

    /// How much of its time a pop-up has used, 0..1.
    #[must_use]
    pub fn pop_progress(&self, p: &Pop, now: u64) -> f32 {
        let life = self.note(p.id).map_or(POP_LIFE, Note::life);
        (p.age(now) as f32 / life as f32).clamp(0.0, 1.0)
    }

    // -- the answer

    pub fn start_reply(&mut self, id: u64) {
        if self.note(id).is_some_and(|n| n.acts.iter().any(|a| a.does == Do::Reply)) {
            self.reply = Some(Reply { id, text: String::new() });
        }
    }

    pub fn cancel_reply(&mut self) {
        if self.reply.take().is_some() {
        }
    }

    /// A typed character; backspace is `\u{8}`, Ctrl+Backspace `\u{7f}`, Ctrl+A `\u{1}` (clears).
    pub fn reply_char(&mut self, ch: char) {
        let Some(r) = self.reply.as_mut() else { return };
        match ch {
            '\u{8}' => drop(r.text.pop()),
            '\u{7f}' => {
                let kept = r.text.trim_end().rfind(' ').map_or(0, |i| i + 1);
                r.text.truncate(kept);
            }
            '\u{1}' => r.text.clear(),
            c if !c.is_control() => r.text.push(c),
            _ => {}
        }
    }

    /// Enter or the send button: the text goes to its note's source and the note is done. `None` for an empty answer.
    pub fn send_reply(&mut self, now: u64) -> Option<(u64, String)> {
        let r = self.reply.as_ref().filter(|r| !r.text.trim().is_empty())?.clone();
        self.reply = None;
        self.dismiss(r.id, now);
        Some((r.id, r.text.trim().to_string()))
    }

    pub fn highlight_note(&mut self, id: u64, now: u64) {
        self.highlight = Some((id, now));
    }
}

impl Pop {
    fn age(&self, now: u64) -> u64 {
        let standing = self.since.map_or(0, |t| now.saturating_sub(t));
        now.saturating_sub(self.born).saturating_sub(self.held + standing)
    }

    /// The pop-up's clock is stopped.
    #[must_use]
    pub const fn held(&self) -> bool {
        self.since.is_some()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the store of this process

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::new());
}

/// Runs `f` on the store of this process (the bar's thread). Not to be called from inside `f`.
pub fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    STORE.with(|s| f(&mut s.borrow_mut()))
}

// ---------------------------------------------------------------------------------------------------------------------
// Do not disturb is remembered between runs: %APPDATA%\WinRust\dnd.txt holds `1` or `0`. It is the user's own choice, so it lives in the
// user's profile (a standard user cannot write to %ProgramData%\WinRust, and it would be one setting for everyone); the old
// machine-wide file is still read when there is no personal one.

fn dnd_file(var: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(var).map(|p| std::path::PathBuf::from(p).join("WinRust").join("dnd.txt"))
}

/// What the file says: only a `1` turns it on.
#[must_use]
pub fn parse_dnd(text: &str) -> bool {
    text.trim() == "1"
}

#[must_use]
pub fn load_dnd() -> bool {
    ["APPDATA", "ProgramData"].into_iter().find_map(|v| std::fs::read_to_string(dnd_file(v)?).ok()).is_some_and(|s| parse_dnd(&s))
}

pub fn save_dnd(on: bool) {
    if let Some(f) = dnd_file("APPDATA") {
        let _ = std::fs::create_dir_all(f.parent().unwrap_or(&f));
        let _ = std::fs::write(f, if on { "1" } else { "0" });
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// a tray balloon

/// A balloon of `Shell_NotifyIcon` (`NIF_INFO`) as the tray host receives it: the title, the text and the icon kind
/// (`NIIF_*`: 0 none, 1 info, 2 warning, 3 error).
#[derive(Clone, Debug, PartialEq)]
pub struct Balloon {
    pub title: String,
    pub text: String,
    pub kind: u32,
}

/// The balloon in a `WM_COPYDATA` of `Shell_NotifyIcon` (the layout `tray_msg` reads: flags@20, `szInfo`@296, `szInfoTitle`@812,
/// `dwInfoFlags`@940). `None` when the message carries none, or an empty one (the app takes its balloon back).
#[must_use]
pub fn parse_balloon(d: &[u8]) -> Option<Balloon> {
    const NIF_INFO: u32 = 0x10;
    if d.len() < 944 {
        return None;
    }
    let u = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let wide = |from: usize, to: usize| {
        let w: Vec<u16> = d[from..to].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&c| c != 0).collect();
        String::from_utf16_lossy(&w).trim().to_string()
    };
    if u(20) & NIF_INFO == 0 {
        return None;
    }
    let text = wide(296, 808);
    (!text.is_empty()).then(|| Balloon { title: wide(812, 940), text, kind: u(940) & 3 })
}

// ---------------------------------------------------------------------------------------------------------------------
// arrivals (the bar's thread)

fn tick() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

/// The windows that show notes follow the store: the clock's badge, the pop-ups, the centre if it is open.
pub fn changed() {
    // SAFETY: on the bar's thread, which owns the windows and the tray table.
    let anchor = unsafe { crate::anchor(crate::NONE) };
    crate::ui::notice_pop::changed(anchor);
    crate::ui::panel::refresh();
    crate::panel_changed();
}

/// A note arrives.
pub fn post(n: Note) {
    let now = tick();
    if with(|s| s.is_repeat(&n, now)) {
        return;
    }
    // a full-screen app (a game, a film) is not interrupted: the note waits in the centre
    let quiet = crate::FULLSCREEN.load(std::sync::atomic::Ordering::Relaxed);
    with(|s| s.push_quiet(n, now, quiet));
    changed();
}

/// The bar repaints (its badge follows the store). Not before the bar exists.
pub fn bar_changed() {
    if crate::BAR.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        crate::panel_changed();
    }
}

/// The face of WinRust's screenshot tools.
fn shot_app() -> App {
    App {
        key: "winrust-screenshot".into(),
        name: t!("Снимок экрана", "Screenshot", ja: "スクリーンショット", pt: "Captura de tela", zh: "截图").to_string(),
        icon: AppIcon::Plate("M8 5H5v3M16 5h3v3M8 19H5v-3M16 19h3v-3M12 12h.01", 0x7b6cf0, 0x5b4bd8),
    }
}

fn screenshots_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(r"Pictures\Screenshots")
}

/// The picker or PrtSc took a screenshot: the file `name` in Pictures\Screenshots (when it was saved) and/or the clipboard. `size`
/// is the picture's size in pixels; `preview` a picture file to show when nothing was saved.
pub fn screenshot_done(name: Option<&str>, size: Option<(u32, u32)>, copied: bool, preview: Option<String>) {
    let at = size.map(|(w, h)| format!(" \u{b7} {w} \u{d7} {h}")).unwrap_or_default();
    let title = match name {
        Some(_) => t!("Скриншот сохранён", "Screenshot saved", ja: "スクリーンショットを保存しました", pt: "Captura de tela salva", zh: "截图已保存"),
        None => t!("Скриншот скопирован", "Screenshot copied", ja: "スクリーンショットをコピーしました", pt: "Captura de tela copiada", zh: "截图已复制"),
    };
    let body = match name {
        Some(n) => format!("{} \u{203a} {n}{at}", t!("Изображения \u{203a} Снимки экрана", "Pictures \u{203a} Screenshots", ja: "ピクチャ \u{203a} スクリーンショット", pt: "Imagens \u{203a} Capturas de tela", zh: "图片 \u{203a} 屏幕截图")),
        None => format!("{}{at}", t!("Скопирован в буфер обмена", "Copied to clipboard", ja: "クリップボードにコピーしました", pt: "Copiado para a área de transferência", zh: "已复制到剪贴板")),
    };
    let _ = copied;
    let mut n = Note::new(shot_app(), title, body);
    let file = name.map(|n| screenshots_dir().join(n)).filter(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned());
    if let Some(p) = file.clone().or(preview) {
        n = n.image(p);
    }
    if let Some(p) = file {
        n = n
            .acts(vec![
                Act { label: t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开").to_string(), does: Do::Open(p.clone()) },
                Act { label: t!("Показать в папке", "Show in folder", ja: "フォルダーで表示", pt: "Mostrar na pasta", zh: "在文件夹中显示").to_string(), does: Do::Show(p.clone()) },
            ])
            .target(Target::File(p));
    }
    post(n);
}

/// The face of the screen recorder.
fn rec_app() -> App {
    App {
        key: "winrust-recording".into(),
        name: t!("Запись экрана", "Screen recording", ja: "画面録画", pt: "Gravação de tela", zh: "屏幕录制").to_string(),
        icon: AppIcon::Plate("M12 18a6 6 0 1 0 0-12 6 6 0 0 0 0 12zM12 12h.01", 0xe5484d, 0xc13a40),
    }
}

/// A recording was saved: the video at `path` (`secs` long, `bytes` big), with `thumb` as its cover.
pub fn recording_saved(path: &str, secs: u64, bytes: u64, thumb: Option<String>) {
    let file = std::path::Path::new(path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let mb = format!("{:.1} MB", bytes as f64 / 1_048_576.0);
    let body = format!("{} \u{203a} {file} \u{b7} {mb}", t!("Видео \u{203a} Captures", "Videos \u{203a} Captures", ja: "ビデオ \u{203a} Captures", pt: "Vídeos \u{203a} Captures", zh: "视频 \u{203a} Captures"));
    let mut n = Note::new(rec_app(), t!("Запись экрана сохранена", "Screen recording saved", ja: "画面録画を保存しました", pt: "Gravação de tela salva", zh: "屏幕录制已保存"), body)
        .acts(vec![
            Act { label: t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开").to_string(), does: Do::Open(path.to_string()) },
            Act { label: t!("Показать в папке", "Show in folder", ja: "フォルダーで表示", pt: "Mostrar na pasta", zh: "在文件夹中显示").to_string(), does: Do::Show(path.to_string()) },
        ])
        .target(Target::File(path.to_string()))
        .video(crate::ui::snip::clock_text(secs));
    if let Some(t) = thumb {
        n = n.image(t);
    }
    post(n);
}

pub fn recording_failed(why: &str) {
    post(Note::new(rec_app(), t!("Не удалось записать экран", "Could not record the screen", ja: "画面を録画できませんでした", pt: "Não foi possível gravar a tela", zh: "无法录制屏幕"), why));
}

pub fn screenshot_failed() {
    post(Note::new(shot_app(), t!("Не удалось сделать скриншот", "Could not take a screenshot", ja: "スクリーンショットを撮影できませんでした", pt: "Não foi possível capturar a tela", zh: "无法截图"), ""));
}

thread_local! {
    static NAMES: std::cell::RefCell<std::collections::HashMap<String, String>> = std::cell::RefCell::new(std::collections::HashMap::new());
    static ASKED: std::cell::RefCell<std::collections::HashSet<String>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

fn remember_name(exe: &str, name: String) {
    ASKED.with_borrow_mut(|a| a.remove(exe));
    NAMES.with_borrow_mut(|m| {
        if m.len() >= 256 {
            m.clear();
        }
        m.insert(exe.to_string(), name);
    });
}

/// The name an app goes by: its file's description (what Task Manager shows), else the file's name. Reading the description means
/// reading the file, and balloons come in on the bar's thread inside the sender's `WM_COPYDATA` (the sender waits): so the first
/// balloon of a program gets the file's name at once, a thread reads the description, and `names_done` puts it into the notes
/// when it arrives. Cached per program after that.
#[must_use]
pub fn app_name(exe: &str) -> String {
    if let Some(n) = NAMES.with_borrow(|m| m.get(exe).cloned()) {
        return n;
    }
    // reads in flight are bounded (a disk that does not answer): past that, the file's name now and another try with the next balloon
    if ASKED.with_borrow_mut(|a| a.len() < 8 && a.insert(exe.to_string())) {
        let (exe, bar) = (exe.to_string(), crate::BAR.load(std::sync::atomic::Ordering::Relaxed));
        let spawned = std::thread::Builder::new().name("app-name".into()).spawn({
            let exe = exe.clone();
            move || {
                let name = read_app_name(&exe);
                NAMES_DONE.lock().unwrap_or_else(|e| e.into_inner()).push((exe, name));
                // SAFETY: posting to a window that is gone just fails.
                unsafe { windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(bar as _, APP_NAME, 0, 0) };
            }
        });
        if spawned.is_err() {
            ASKED.with_borrow_mut(|a| a.remove(&exe));
        }
    }
    stem(exe)
}

/// Posted to the bar when a thread of `app_name` has read a description: `names_done` on the bar's thread.
pub const APP_NAME: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 21;
static NAMES_DONE: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// The bar's thread, on `APP_NAME`: the descriptions that were read go into the notes already shown under the file's name (and
/// into the cache for the next balloon of the program).
pub fn names_done() {
    let done = std::mem::take(&mut *NAMES_DONE.lock().unwrap_or_else(|e| e.into_inner()));
    let mut any = false;
    for (exe, name) in done {
        any |= with(|s| s.rename_app(&exe.to_lowercase(), &stem(&exe), &name));
        remember_name(&exe, name);
    }
    if any {
        changed();
    }
}

/// The file's name as a title: `C:\Apps\qbittorrent.exe` is `Qbittorrent`.
fn stem(exe: &str) -> String {
    let f = exe.rsplit('\\').next().unwrap_or(exe);
    let f = f.rsplit_once('.').map_or(f, |(a, _)| a);
    let mut c = f.chars();
    c.next().map(|h| h.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn read_app_name(exe: &str) -> String {
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let path = wide(exe);
    // SAFETY: the version block is read through the sizes the API itself gave; the pointers VerQueryValueW returns point into it.
    let from_file = unsafe {
        let mut h = 0u32;
        let size = GetFileVersionInfoSizeW(path.as_ptr(), &mut h);
        if size == 0 {
            return stem(exe);
        }
        let mut block = vec![0u8; size as usize];
        if GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr().cast()) == 0 {
            return stem(exe);
        }
        // `unit`: bytes per length unit (the translation table counts bytes, a string counts characters); a value that does not lie
        // inside the block (a damaged resource) is refused rather than read past its end
        let query = |sub: &str, unit: usize| -> Option<(*const u8, u32)> {
            let (mut p, mut n) = (std::ptr::null_mut(), 0u32);
            let found = VerQueryValueW(block.as_ptr().cast(), wide(sub).as_ptr(), &mut p, &mut n) != 0 && !p.is_null() && n > 0;
            let (at, start, end) = (p as usize, block.as_ptr() as usize, block.as_ptr() as usize + block.len());
            (found && at >= start && at.checked_add(n as usize * unit).is_some_and(|e| e <= end)).then_some((p as *const u8, n))
        };
        // the first language and code page the file lists, `\StringFileInfo\<lang><page>\FileDescription`
        query("\\VarFileInfo\\Translation", 1).and_then(|(p, n)| {
            if n < 4 {
                return None;
            }
            let (lang, page) = (u16::from_le_bytes([*p, *p.add(1)]), u16::from_le_bytes([*p.add(2), *p.add(3)]));
            let (p, n) = query(&format!("\\StringFileInfo\\{lang:04x}{page:04x}\\FileDescription"), 2)?;
            let w = std::slice::from_raw_parts(p.cast::<u16>(), n as usize);
            Some(String::from_utf16_lossy(w).trim_end_matches('\0').trim().to_string())
        })
    };
    from_file.filter(|d| !d.is_empty()).unwrap_or_else(|| stem(exe))
}

// ---------------------------------------------------------------------------------------------------------------------
// what a click does

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn shell(verb: &str, file: &str, args: Option<&str>) {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let (verb, file, args) = (wide(verb), wide(file), args.map(wide));
    // SAFETY: NUL-terminated strings that outlive the call; no window, no directory.
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), args.as_ref().map_or(std::ptr::null(), |a| a.as_ptr()), std::ptr::null(), SW_SHOWNORMAL);
    }
}

/// A button under a note.
pub fn perform(d: &Do) {
    match d {
        Do::Open(path) => shell("open", path, None),
        Do::Show(path) => shell("open", "explorer.exe", Some(&format!("/select,\"{path}\""))),
        Do::Read | Do::Reply => {}
    }
}

/// A click on the note itself: its source comes to the front.
pub fn open_target(t: &Target) {
    match t {
        Target::Nothing => {}
        Target::File(path) => shell("open", path, None),
        // SAFETY: the tray's own table is read and a message posted to the icon's window, as a click on the icon does.
        Target::Balloon { hwnd, uid } => unsafe { crate::balloon_click(*hwnd, *uid) },
    }
}

/// The answer typed under a note goes to its source. No source of ours takes one yet (a tray balloon has no way to), so a note
/// never offers the button and nothing arrives here.
pub fn reply_sent(_id: u64, _text: &str) {}

// ---------------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn app(k: &str) -> App {
        App { key: k.into(), name: k.to_uppercase(), icon: AppIcon::Winrust }
    }

    fn note(k: &str, t: &str) -> Note {
        Note::new(app(k), t, "body")
    }

    #[test]
    fn a_description_that_arrives_late_renames_the_notes_shown_under_the_file_name() {
        let mut s = Store::new();
        let mut n = Note::new(App { key: r"c:\q.exe".into(), name: "Q".into(), icon: AppIcon::Winrust }, "Q", "body");
        n.target = Target::Nothing;
        s.push(n, 0);
        s.push(Note::new(App { key: r"c:\q.exe".into(), name: "Q".into(), icon: AppIcon::Winrust }, "Done", "x"), 10_000);
        assert!(s.rename_app(r"c:\q.exe", "Q", "qBittorrent"));
        assert!(s.notes().iter().all(|n| n.app.name == "qBittorrent"));
        assert_eq!(s.notes().iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Done", "qBittorrent"], "only a title that was the name follows it");
        assert!(!s.rename_app(r"c:\q.exe", "Q", "qBittorrent"), "nothing left under the old name");
        assert_eq!(stem(r"C:\Apps\qbittorrent.exe"), "Qbittorrent");
    }

    #[test]
    fn a_new_note_stands_first_and_is_unread() {
        let mut s = Store::new();
        let a = s.push(note("a", "one"), 0);
        let b = s.push(note("a", "two"), 10);
        assert_eq!(s.notes().iter().map(|n| n.id).collect::<Vec<_>>(), vec![b, a]);
        assert_eq!(s.unread(), 2);
        s.mark_read(a);
        assert_eq!(s.unread(), 1);
    }

    #[test]
    fn groups_follow_the_newest_note_and_a_stack_of_three_is_closed() {
        let mut s = Store::new();
        let t = s.push(note("tg", "1"), 0);
        s.push(note("tm", "x"), 1);
        s.push(note("tg", "2"), 2);
        s.push(note("tg", "3"), 3);
        let g = s.groups();
        assert_eq!(g.iter().map(|g| g.app.key.as_str()).collect::<Vec<_>>(), vec!["tg", "tm"]);
        assert!(g[0].can_expand() && !g[0].open && g[0].shown().len() == 1);
        assert!(g[1].open && !g[1].can_expand());
        s.toggle_stack("tg");
        assert!(s.groups()[0].open && s.groups()[0].shown().len() == 3);
        s.toggle_stack("tg");
        assert!(!s.groups()[0].open);
        // two notes need no stack
        s.dismiss(t, 10);
        s.tick(10 + GONE_MS);
        assert!(s.groups()[0].open && s.groups()[0].ids.len() == 2);
    }

    #[test]
    fn the_list_is_bounded_and_the_oldest_go() {
        let mut s = Store::new();
        let first = s.push(note("a", "0"), 0);
        for i in 1..=MAX_NOTES as u64 + 5 {
            s.push(note("a", "n"), i);
        }
        assert_eq!(s.notes().len(), MAX_NOTES);
        assert!(s.note(first).is_none());
        assert_eq!(s.notes()[0].id, MAX_NOTES as u64 + 6);
        assert!(s.pops().len() <= MAX_POPS);
    }

    #[test]
    fn at_most_three_pop_up_and_the_oldest_makes_room() {
        let mut s = Store::new();
        let ids: Vec<u64> = (0..4).map(|i| s.push(note("a", "n"), i)).collect();
        assert_eq!(s.pops().iter().map(|p| p.id).collect::<Vec<_>>(), ids[1..].to_vec());
    }

    #[test]
    fn do_not_disturb_and_an_open_centre_keep_pop_ups_away_but_not_the_notes() {
        let mut s = Store::new();
        s.set_dnd(true);
        s.push(note("a", "n"), 0);
        assert!(s.pops().is_empty() && s.unread() == 1);
        assert_eq!(s.badge(), None, "the clock shows the moon, not a number");
        s.set_dnd(false);
        assert_eq!(s.badge(), Some(1));
        s.push(note("a", "n"), 1);
        assert_eq!(s.pops().len(), 1);
        s.set_dnd(true);
        assert!(s.pops().is_empty(), "turning it on takes the pop-ups away");
        s.set_dnd(false);
        let quiet = s.push_quiet(note("a", "while a game runs"), 1, true);
        assert!(s.pops().iter().all(|p| p.id != quiet) && s.unread() == 3, "a full-screen app is not interrupted, the note waits");
        s.open_centre();
        assert_eq!(s.badge(), None);
        let id = s.push(note("a", "later"), 2);
        assert!(s.pops().is_empty() && s.note(id).unwrap().fresh && s.unread() == 4);
        s.close_centre();
        assert_eq!(s.unread(), 0, "what was in the centre when it closed was seen");
        assert_eq!(s.badge(), None);
        s.push(note("a", "after"), 3);
        assert_eq!(s.badge(), Some(1));
        // the same balloon sent again at once is one card
        let again = note("a", "after");
        assert!(s.is_repeat(&again, 1000) && !s.is_repeat(&again, 3003) && !s.is_repeat(&note("b", "after"), 1000));
    }

    #[test]
    fn a_dismissed_note_collapses_and_is_forgotten() {
        let mut s = Store::new();
        let id = s.push(note("a", "n"), 0);
        s.dismiss(id, 100);
        assert_eq!((s.count(), s.unread(), s.notes().len()), (0, 0, 1));
        s.tick(100 + GONE_MS - 1);
        assert_eq!(s.notes().len(), 1, "still collapsing");
        assert!(s.tick(100 + GONE_MS));
        assert!(s.notes().is_empty());
    }

    #[test]
    fn clearing_a_group_goes_note_by_note_and_clear_all_can_be_undone() {
        let mut s = Store::new();
        for _ in 0..3 {
            s.push(note("a", "n"), 0);
        }
        let other = s.push(note("b", "x"), 0);
        s.clear_app("a", 1000);
        let times: Vec<_> = s.notes().iter().filter_map(|n| n.gone).collect();
        assert_eq!(times, vec![1000, 1040, 1080]);
        assert!(s.note(other).unwrap().gone.is_none());
        s.clear_all(2000);
        assert_eq!(s.count(), 0);
        assert!(s.can_undo());
        s.tick(2000 + GONE_MS); // all collapsed and gone
        assert!(s.notes().is_empty());
        assert!(s.undo());
        assert_eq!(s.count(), 1, "what \"Clear all\" took comes back; the group cleared before it does not");
        assert_eq!(s.notes().first().map(|n| n.app.key.as_str()), Some("b"));
        assert!(!s.can_undo());
        // the offer lapses
        s.clear_all(5000);
        s.tick(5000 + UNDO_MS);
        assert!(!s.can_undo());
    }

    #[test]
    fn a_pop_up_stands_still_while_held_and_goes_when_its_time_is_up() {
        let mut s = Store::new();
        let id = s.push(note("a", "n"), 0);
        s.tick(POP_LIFE - 1);
        assert_eq!(s.pops().len(), 1);
        s.pop_hold(id, true, 3000); // hovered from 3 s on
        s.tick(20_000);
        assert!(s.pops()[0].out.is_none(), "a held pop-up does not expire");
        s.pop_hold(id, false, 20_000); // let go after 17 s: 3 s used
        assert!((s.pop_progress(&s.pops()[0].clone(), 20_000) - 0.5).abs() < 1e-3);
        s.tick(20_000 + 3000);
        assert!(s.pops()[0].out.is_some(), "the 6 s are up");
        s.tick(20_000 + 3000 + POP_EXIT_MS);
        assert!(s.pops().is_empty());
        assert_eq!(s.count(), 1, "the note stays in the centre");
    }

    #[test]
    fn a_picture_note_stays_longer() {
        let mut s = Store::new();
        let id = s.push(note("a", "n").image("x.png"), 0);
        s.tick(POP_LIFE + 100);
        assert!(s.pops()[0].out.is_none());
        s.tick(POP_LIFE_PICTURE);
        assert!(s.pops().iter().all(|p| p.id != id || p.out.is_some()));
    }

    #[test]
    fn a_reply_holds_the_pop_up_and_sending_it_removes_the_note() {
        let mut s = Store::new();
        let plain = s.push(note("a", "n"), 0);
        s.start_reply(plain);
        assert!(s.reply().is_none(), "a note whose source cannot take an answer has no field");
        let id = s.push(note("a", "n").acts(vec![Act { label: "Reply".into(), does: Do::Reply }]), 0);
        s.start_reply(id);
        assert!(s.send_reply(1).is_none(), "an empty answer is not sent");
        for c in "Hi  there".chars() {
            s.reply_char(c);
        }
        s.reply_char('\u{8}');
        assert_eq!(s.reply().unwrap().text, "Hi  ther");
        s.reply_char('\u{7f}');
        assert_eq!(s.reply().unwrap().text, "Hi  ");
        s.tick(POP_LIFE * 2);
        assert!(s.pops().iter().any(|p| p.id == id && p.out.is_none()), "being answered, it stays");
        s.reply_char('x');
        assert_eq!(s.send_reply(50_000), Some((id, "Hi  x".to_string())));
        assert!(s.reply().is_none() && s.note(id).unwrap().gone.is_some());
    }

    #[test]
    fn ago_reads_like_the_mockups_rows() {
        assert_eq!(ago(30_000, 0), Ago::Now);
        assert_eq!(ago(2 * 60_000, 0), Ago::Min(2));
        assert_eq!(ago(61 * 60_000, 0), Ago::Hour(1));
        assert_eq!(ago(3 * 3_600_000, 0), Ago::Hour(3));
        assert_eq!(ago(2 * 86_400_000, 0), Ago::Day(2));
        assert_eq!(ago(0, 5), Ago::Now, "a clock that went back is not an error");
    }

    #[test]
    fn dnd_file_text() {
        assert!(parse_dnd("1\n") && !parse_dnd("0") && !parse_dnd("") && !parse_dnd("yes"));
    }

    fn balloon_msg(flags: u32, text: &str, title: &str, info: u32) -> Vec<u8> {
        let mut d = vec![0u8; 964];
        d[20..24].copy_from_slice(&flags.to_le_bytes());
        for (i, c) in text.encode_utf16().enumerate() {
            d[296 + 2 * i..298 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        for (i, c) in title.encode_utf16().enumerate() {
            d[812 + 2 * i..814 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        d[940..944].copy_from_slice(&info.to_le_bytes());
        d
    }

    #[test]
    fn a_balloon_is_read_from_the_icon_message() {
        let d = balloon_msg(0x10 | 0x4, "Download finished", "qBittorrent", 0x21);
        assert_eq!(parse_balloon(&d), Some(Balloon { title: "qBittorrent".into(), text: "Download finished".into(), kind: 1 }));
        assert_eq!(parse_balloon(&balloon_msg(0x4, "no info flag", "t", 1)), None);
        assert_eq!(parse_balloon(&balloon_msg(0x10, "", "taken back", 1)), None, "an empty text hides the balloon");
        assert_eq!(parse_balloon(&d[..900]), None, "a short message is not read past its end");
        let ru = balloon_msg(0x10, "Готово ✓", "", 3);
        assert_eq!(parse_balloon(&ru).map(|b| (b.text, b.kind)), Some(("Готово ✓".into(), 3)));
    }
}
