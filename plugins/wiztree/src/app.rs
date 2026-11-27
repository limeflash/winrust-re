//! WinRust Disk's window: what is on the screen and what a click or a key does. It is an [`App`] of `winrust-ui`: it never sees a
//! window, so `--render`, `--bench`, `--selftest` and the tests run it with none, on a [`Backend`] that touches nothing. How it is
//! drawn is `draw.rs`; where the tiles stand, `layout.rs`; what the drive holds, `model.rs`.
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use winrust_ui::{
    core::App,
    frame::{Button, Cx, Frame, Id, Mods},
    gfx::{Canvas, Rect},
    widgets::{
        list::{col_of, list_key, row_of, ColItem},
        list_model::{Col, Columns, ListState},
        overlay::MenuItem,
        scroll,
        scroll_model::Scroll,
    },
};
use winrust_common::mft::ROOT;

use crate::{
    backend::{Backend, DriveInfo},
    delete::How,
    layout::{Layout, Tile, MAX_TILES},
    look::{self, human},
    model::{Category, DiskData, Nav, Removed},
};

// ids of the window's items (0 and up)
pub const ID_DRIVE: Id = 10;
pub const ID_RESCAN: Id = 20;
pub const ID_UP: Id = 21;
pub const ID_CRUMB: Id = 30;
pub const ID_VIEW: Id = 50;
pub const ID_MAP: Id = 60;
pub const ID_SIDE: Id = 70;
pub const ID_LEGEND: Id = 80;
pub const ID_ACT: Id = 100;
pub const ID_TOP: Id = 110;
pub const ID_BLOCK_RUN: Id = 120;
pub const ID_BLOCK_ALT: Id = 121;
pub const ID_EMPTY_UP: Id = 122;
pub const ID_DLG_SCRIM: Id = 129;
pub const ID_DLG_OPT: Id = 130;
pub const ID_DLG_CANCEL: Id = 132;
pub const ID_DLG_OK: Id = 133;
pub const ID_TOAST_UNDO: Id = 140;
pub const ID_COLS: Id = 3000;
pub const ID_TILE: Id = 1000;
pub const ID_LIST: Id = 4000;
/// A 1 × 1 item in the corner that `--render` clicks to open the context menu or the toast of a state of the design (see `demo`).
pub const ID_DEMO: Id = 9999;

pub const TAG_MENU: u32 = 1;
pub const A_OPEN: usize = 1;
pub const A_SHOW: usize = 2;
pub const A_COPY: usize = 3;
pub const A_DELETE: usize = 4;
pub const A_UP: usize = 5;
pub const A_RESCAN: usize = 6;

/// What a worker thread posts to the window: one message, and the news waits in the inbox (`WM_APP + 1`).
pub const MSG_WAKE: u32 = 0x8000 + 1;

/// The toast stays 5 s (`toast` of the mockup), and what is moved to the Recycle Bin goes there when it is over.
pub const TOAST_MS: u64 = 5000;
pub const ROW_H: f32 = 40.0;

/// Where the "How to delete" dialog starts. The mockup has "Delete permanently" chosen; here it is the Recycle Bin, because
/// Enter on a dialog that comes up under the user's hand must not be the one that cannot be undone.
pub const DEFAULT_HOW: How = How::RecycleBin;

pub enum Event {
    Progress { epoch: u64, read: u64, total: u64 },
    Scanned { epoch: u64, result: Result<Box<DiskData>, String>, secs: f32 },
    Deleted { ticket: u64, result: Result<(), String> },
}

pub type Inbox = Arc<Mutex<Vec<Event>>>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    Map,
    List,
}

/// A scan under way.
#[derive(Clone, Debug)]
pub struct Scan {
    pub epoch: u64,
    pub read: u64,
    pub total: u64,
    pub slow: bool,
    pub since: Instant,
    /// What the time left says when it is not the clock's (a state of `--render`).
    pub eta: Option<u32>,
}

pub enum Phase {
    /// Not scanned: the drive waits for a click (or it cannot be read: `error`).
    Idle,
    Scanning(Scan),
    Ready,
}

/// How a zoom starts: into a folder from its tile, out of one to the tile it was, or a plain fade (the folder is the same).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Zoom {
    In(Rect),
    Out(u32),
    Fade,
}

pub struct Dlg {
    pub id: u32,
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// The folder it is in, as `C:\Games`.
    pub parent: String,
    pub how: How,
}

pub struct Toast {
    pub text: String,
    pub undo: bool,
    pub ok: bool,
    /// The frame clock when it was first drawn.
    pub born: Option<u64>,
    pub serial: u32,
}

/// A deletion under way on a worker.
struct Job {
    ticket: u64,
    id: u32,
    name: String,
    size: u64,
}

/// What was moved to the Recycle Bin from the screen but still waits for its toast to end (so that Undo is nothing but a redraw).
struct Pending {
    removed: Removed,
    path: PathBuf,
    name: String,
    due: Instant,
}

/// What the Recycle Bin's worker was given: if the shell refuses, it comes back.
struct Sent {
    ticket: u64,
    removed: Removed,
    name: String,
}

/// What a state of the design needs by hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DemoHook {
    None,
    /// The context menu of the item (a record), at the place the design puts it.
    Menu(u32),
    /// The "Delete" of the dialog that is open.
    Confirm,
}

pub struct Disk {
    pub be: Arc<dyn Backend>,
    hwnd: isize,
    pub inbox: Inbox,
    pub drives: Vec<DriveInfo>,
    pub drive: usize,
    /// The program may read a raw volume (it is elevated).
    pub admin: bool,
    /// The user chose to scan without the rights (the card is gone).
    pub dismissed: bool,
    /// Drives the user said may be scanned file by file.
    pub slow_ok: Vec<char>,
    pub phase: Phase,
    pub error: Option<String>,
    pub data: Option<DiskData>,
    /// What state the data is in (a new scan, a deletion, an Undo): layouts made from older ones are out of date.
    pub data_gen: u64,
    pub scan_gen: u64,
    pub scanned_in: Option<String>,
    pub nav: Nav,
    pub view: ViewMode,
    pub sel: Option<u32>,
    /// The category the legend is pointed at, for a state of the design (the pointer over a legend item does it live).
    pub pin_hl: Option<Category>,
    pub zoom: Option<Zoom>,
    pub zoom_serial: u32,
    pub zoom_seen: u32,
    /// A scan has just ended: the tiles come in one after the other.
    pub fresh: bool,
    pub max_tiles: usize,
    pub layout: Option<Layout>,
    pub list: ListState,
    pub cols: Columns,
    /// The rows of the list, as indexes of `layout.items`, in the order of the header.
    pub order: Vec<usize>,
    pub side: Scroll,
    pub dlg: Option<Dlg>,
    pub toast: Option<Toast>,
    toast_serial: u32,
    job: Option<Job>,
    pending: Option<Pending>,
    sent: Vec<Sent>,
    ticket: u64,
    /// The item of the context menu on the screen.
    menu_on: Option<u32>,
    pub demo: DemoHook,
    /// The record the pointer is over (last frame's items): what the panel shows and the keys act on besides the selection.
    pub hot: Option<u32>,
    /// What the action buttons of the details panel do (action, record), and the records of "Biggest here": from the last frame.
    pub acts: Vec<(usize, Option<u32>)>,
    pub tops: Vec<u32>,
    /// What the frame last knew of the animations it starts (a card, the dialog, a toast, the tiles of a fresh scan).
    pub card_seen: u8,
    pub dlg_seen: bool,
    pub toast_seen: u32,
    pub building: bool,
    /// The height of the details panel's content, from the last frame.
    pub side_h: f32,
    /// How long what was moved to the Recycle Bin waits for its toast (a test makes it short).
    pub bin_delay_ms: u64,
}

fn columns() -> Columns {
    let col = |t: &str, w: f32, min: f32, right: bool| Col { title: t.into(), w, min, align_right: right };
    Columns::new(vec![col("Name", 0.0, 160.0, false), col("Share of this folder", 236.0, 120.0, false), col("Size", 108.0, 60.0, true), col("Items", 80.0, 56.0, true)])
}

impl Disk {
    /// The window for the drives `be` reports, on `letter` (or the first). Nothing is read until [`App::start`] gives it a window to
    /// answer to; it starts a scan then when it may.
    pub fn new(be: Arc<dyn Backend>, letter: Option<char>) -> Self {
        let drives = be.drives();
        let drive = letter.and_then(|c| drives.iter().position(|d| d.letter == c)).unwrap_or(0);
        let admin = be.is_admin();
        Self {
            be,
            hwnd: 0,
            inbox: Arc::new(Mutex::new(Vec::new())),
            drives,
            drive,
            admin,
            dismissed: false,
            slow_ok: Vec::new(),
            phase: Phase::Idle,
            error: None,
            data: None,
            data_gen: 1,
            scan_gen: 0,
            scanned_in: None,
            nav: Nav::new(),
            view: ViewMode::Map,
            sel: None,
            pin_hl: None,
            zoom: None,
            zoom_serial: 0,
            zoom_seen: 0,
            fresh: false,
            max_tiles: MAX_TILES,
            layout: None,
            list: ListState::default(),
            cols: columns(),
            order: Vec::new(),
            side: Scroll::default(),
            dlg: None,
            toast: None,
            toast_serial: 0,
            job: None,
            pending: None,
            sent: Vec::new(),
            ticket: 0,
            menu_on: None,
            demo: DemoHook::None,
            hot: None,
            acts: Vec::new(),
            tops: Vec::new(),
            card_seen: 0,
            dlg_seen: false,
            toast_seen: 0,
            building: false,
            side_h: 0.0,
            bin_delay_ms: TOAST_MS + 200,
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // what is on the screen

    #[must_use]
    pub fn cur(&self) -> Option<&DriveInfo> {
        self.drives.get(self.drive)
    }

    #[must_use]
    pub fn letter(&self) -> char {
        self.cur().map_or('C', |d| d.letter)
    }

    #[must_use]
    pub fn scanning(&self) -> bool {
        matches!(self.phase, Phase::Scanning(_))
    }

    /// The map (or the list) can be shown.
    #[must_use]
    pub fn ready(&self) -> bool {
        matches!(self.phase, Phase::Ready) && self.data.is_some() && self.block().is_none()
    }

    /// Which card stands in the way of the drive, if one does: administrator rights, or a drive that is not NTFS.
    #[must_use]
    pub fn block(&self) -> Option<Block> {
        if !self.admin && !self.dismissed {
            return Some(Block::Admin);
        }
        let d = self.cur()?;
        if !d.ntfs() && !self.slow_ok.contains(&d.letter) && !self.scanning() && self.data.is_none() {
            return Some(Block::Fs);
        }
        self.error.as_ref().map(|e| Block::Failed(e.clone()))
    }

    /// The record being deleted for good, while the shell works.
    #[must_use]
    pub fn busy(&self) -> Option<u32> {
        self.job.as_ref().map(|j| j.id)
    }

    /// The folder on the screen is empty (everything in it was deleted).
    #[must_use]
    pub fn empty_dir(&self) -> bool {
        self.ready() && self.layout.as_ref().is_some_and(|l| l.tiles.is_empty())
    }

    /// Whether the scan of this drive is the slow one.
    fn slow_for(&self, d: &DriveInfo) -> bool {
        !(d.ntfs() && self.admin)
    }

    pub fn ensure_layout(&mut self, map: (f32, f32)) {
        let Some(d) = self.data.as_ref() else {
            self.layout = None;
            return;
        };
        let want = crate::layout::Key { epoch: self.data_gen, folder: self.nav.current(), map: (map.0.round() as i32, map.1.round() as i32), max_tiles: self.max_tiles };
        if self.layout.as_ref().is_none_or(|l| l.key != want) {
            self.layout = Some(Layout::build(d, self.data_gen, self.nav.current(), map, self.max_tiles));
            self.resort();
        }
    }

    /// The list's order: the header's sort over the items of the level.
    pub fn resort(&mut self) {
        let Some(l) = self.layout.as_ref() else {
            self.order.clear();
            return;
        };
        let mut order: Vec<usize> = (0..l.items.len()).collect();
        if let Some((col, asc)) = self.cols.sort {
            let it = &l.items;
            match col {
                0 => order.sort_by_key(|&i| it[i].name.to_lowercase()),
                3 => order.sort_by_key(|&i| it[i].files.map_or(-1, i64::from)),
                _ => order.sort_by_key(|&i| it[i].bytes),
            }
            if !asc {
                order.reverse();
            }
        }
        self.order = order;
    }

    #[must_use]
    pub fn tile_at(&self, id: Id) -> Option<&Tile> {
        let i = usize::try_from(id - ID_TILE).ok()?;
        self.layout.as_ref()?.tiles.get(i)
    }

    /// The record an item of the window stands for (a tile, a row, an item of "Biggest here"): what the pointer is over.
    #[must_use]
    pub fn record_of(&self, id: Id) -> Option<u32> {
        let l = self.layout.as_ref()?;
        if (ID_TILE..ID_TILE + 400).contains(&id) {
            return self.tile_at(id).map(|t| t.id).filter(|&r| r != crate::model::AGG);
        }
        if let Some(i) = row_of(ID_LIST, id).filter(|_| id >= ID_LIST + 2) {
            return self.order.get(i).and_then(|&k| l.items.get(k)).map(|it| it.id);
        }
        None
    }

    /// The record the details panel is about besides the folder: hovered, else selected.
    #[must_use]
    pub fn focus(&self, hovered: Option<u32>) -> Option<u32> {
        hovered.or(self.sel).filter(|&r| self.layout.as_ref().is_some_and(|l| l.items.iter().any(|i| i.id == r)))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // scans

    /// The window's handle (0 where there is none).
    #[must_use]
    pub fn window(&self) -> isize {
        self.hwnd
    }

    /// Reads the drive on screen from the beginning: on a worker, with what it finds coming back through the inbox.
    pub fn start_scan(&mut self) {
        let Some(d) = self.cur().cloned() else { return };
        let slow = self.slow_for(&d);
        self.scan_gen += 1;
        let epoch = self.scan_gen;
        self.phase = Phase::Scanning(Scan { epoch, read: 0, total: 0, slow, since: Instant::now(), eta: None });
        self.error = None;
        self.data = None;
        self.data_gen += 1;
        self.layout = None;
        self.nav = Nav::new();
        self.sel = None;
        self.zoom = None;
        self.scanned_in = None;
        let (be, inbox, hwnd) = (self.be.clone(), self.inbox.clone(), self.hwnd);
        let note = move || {
            if hwnd != 0 {
                winrust_ui::window::post(hwnd, MSG_WAKE, 0, 0);
            }
        };
        self.be.spawn(Box::new(move || {
            let t0 = Instant::now();
            let mut last = Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now);
            let result = be.scan(d.letter, slow, &mut |read, total| {
                // ten a second is plenty for a bar
                if last.elapsed() >= Duration::from_millis(100) || read >= total {
                    last = Instant::now();
                    let mut q = inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    q.retain(|e| !matches!(e, Event::Progress { .. }));
                    q.push(Event::Progress { epoch, read, total });
                    drop(q);
                    note();
                }
            });
            let secs = t0.elapsed().as_secs_f32();
            inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(Event::Scanned { epoch, result: result.map(Box::new), secs });
            note();
        }));
    }

    /// Picks the drive of chip `i`: a scan, or the card that says why there is none.
    fn pick_drive(&mut self, i: usize, cx: &mut Cx) {
        if i == self.drive || i >= self.drives.len() {
            return;
        }
        self.drive = i;
        self.after_drive_change();
        cx.invalidate();
    }

    fn after_drive_change(&mut self) {
        self.sel = None;
        self.dlg = None;
        self.zoom = None;
        self.error = None;
        let d = self.drives[self.drive].clone();
        if self.block().is_some() && !(self.admin || self.dismissed) {
            self.data = None;
            self.layout = None;
            self.phase = Phase::Idle;
            self.nav = Nav::new();
        } else if d.ntfs() || self.slow_ok.contains(&d.letter) {
            self.start_scan();
        } else {
            self.data = None;
            self.layout = None;
            self.data_gen += 1;
            self.phase = Phase::Idle;
            self.nav = Nav::new();
        }
    }

    /// Takes what the workers have put in the inbox and says whether there was any.
    pub fn drain(&mut self, cx: &mut Cx) {
        if self.take_events() {
            cx.invalidate();
        }
    }

    /// The same without a window to ask for a frame (a render and a test have none, and no message to say a worker is done).
    pub fn take_events(&mut self) -> bool {
        let events: Vec<Event> = std::mem::take(&mut *self.inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let any = !events.is_empty();
        for e in events {
            match e {
                Event::Progress { epoch, read, total } => {
                    if let Phase::Scanning(s) = &mut self.phase
                        && s.epoch == epoch
                    {
                        (s.read, s.total) = (read, total);
                    }
                }
                Event::Scanned { epoch, result, secs } => {
                    if epoch != self.scan_gen {
                        continue;
                    }
                    match result {
                        Ok(d) => {
                            self.data = Some(*d);
                            self.data_gen += 1;
                            self.phase = Phase::Ready;
                            self.scanned_in = Some(format!("{secs:.1} s"));
                            self.nav = Nav::new();
                            self.fresh = true;
                        }
                        Err(e) => {
                            self.phase = Phase::Idle;
                            self.error = Some(e);
                        }
                    }
                }
                Event::Deleted { ticket, result } => self.deleted(ticket, result),
            }
        }
        any
    }

    // -----------------------------------------------------------------------------------------------------------------
    // going about

    fn zoom_in(&mut self, id: u32, slot: Option<Rect>) {
        self.nav.enter(id);
        self.sel = None;
        self.zoom = Some(slot.map_or(Zoom::Fade, Zoom::In));
        self.zoom_serial += 1;
        self.list.scroll.scroll_to(0.0, 0);
    }

    fn zoom_to(&mut self, depth: usize) {
        if let Some(left) = self.nav.to(depth) {
            self.sel = Some(left);
            self.zoom = Some(Zoom::Out(left));
            self.zoom_serial += 1;
            self.list.scroll.scroll_to(0.0, 0);
        }
    }

    pub fn up(&mut self) {
        if let Some(left) = self.nav.up() {
            self.sel = Some(left);
            self.zoom = Some(Zoom::Out(left));
            self.zoom_serial += 1;
            self.list.scroll.scroll_to(0.0, 0);
        }
    }

    /// Opens folder `id` of the level on screen (with the zoom from its tile on the map).
    fn open(&mut self, id: u32) {
        let slot = if self.view == ViewMode::Map { self.layout.as_ref().and_then(|l| l.tile(id)).map(|t| t.slot) } else { None };
        self.zoom_in(id, slot);
    }

    fn path_of(&self, id: u32) -> String {
        self.data.as_ref().map_or_else(String::new, |d| d.path(id))
    }

    fn say(&mut self, text: impl Into<String>, undo: bool, ok: bool) {
        self.toast_serial += 1;
        self.toast = Some(Toast { text: text.into(), undo, ok, born: None, serial: self.toast_serial });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // deleting

    /// The dialog for `id` (the "How to delete" of the design). Nothing is touched until it is confirmed.
    pub fn ask_delete(&mut self, id: u32) {
        let Some(d) = self.data.as_ref().filter(|_| self.job.is_none()) else { return };
        if !d.tree.recs.contains_key(&id) || id == ROOT {
            return;
        }
        if !crate::delete::deletable(std::path::Path::new(&d.path(id))) {
            let name = d.name(id).to_string();
            self.say(format!("Windows needs {name}: it isn’t deleted from here"), false, false);
            return;
        }
        let parent = d.tree.recs[&id].parent;
        self.dlg = Some(Dlg { id, name: d.name(id).to_string(), dir: d.is_dir(id), size: d.size(id), parent: d.path(parent), how: DEFAULT_HOW });
    }

    /// The dialog's "Delete": the one place a path goes to the deleter.
    fn confirm_delete(&mut self, cx: &mut Cx) {
        let Some(dlg) = self.dlg.take() else { return };
        let path = PathBuf::from(self.path_of(dlg.id));
        self.flush_pending();
        self.ticket += 1;
        let ticket = self.ticket;
        match dlg.how {
            How::Permanently => {
                self.job = Some(Job { ticket, id: dlg.id, name: dlg.name, size: dlg.size });
                self.run_delete(ticket, path, How::Permanently);
            }
            How::RecycleBin => {
                let Some(removed) = self.data.as_mut().and_then(|d| d.remove(dlg.id)) else { return };
                self.data_gen += 1;
                self.sel = None;
                self.zoom = Some(Zoom::Fade);
                self.zoom_serial += 1;
                self.say(format!("{} moved to the Recycle Bin", dlg.name), true, true);
                let due = Instant::now() + Duration::from_millis(self.bin_delay_ms);
                self.pending = Some(Pending { removed, path, name: dlg.name, due });
                cx.wake_in(self.bin_delay_ms as u32 + 50);
            }
        }
        cx.invalidate();
    }

    /// The deleter on a worker; the answer comes back as an event.
    fn run_delete(&self, ticket: u64, path: PathBuf, how: How) {
        let (be, inbox, hwnd) = (self.be.clone(), self.inbox.clone(), self.hwnd);
        self.be.spawn(Box::new(move || {
            let result = be.delete(&[path], how, true);
            inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(Event::Deleted { ticket, result });
            if hwnd != 0 {
                winrust_ui::window::post(hwnd, MSG_WAKE, 0, 0);
            }
        }));
    }

    /// What was moved to the bin goes there now.
    #[cfg(test)]
    pub fn flush_now(&mut self) {
        self.flush_pending();
    }

    fn flush_pending(&mut self) {
        let Some(p) = self.pending.take() else { return };
        self.ticket += 1;
        self.sent.push(Sent { ticket: self.ticket, removed: p.removed, name: p.name });
        let ticket = self.ticket;
        self.run_delete(ticket, p.path, How::RecycleBin);
    }

    fn deleted(&mut self, ticket: u64, result: Result<(), String>) {
        if let Some(j) = self.job.take().filter(|j| j.ticket == ticket) {
            match result {
                Ok(()) => {
                    if self.data.as_mut().is_some_and(|d| d.remove(j.id).is_some()) {
                        self.data_gen += 1;
                    }
                    // what was deleted for good is free on the drive now
                    if let Some(d) = self.drives.get_mut(self.drive) {
                        d.free = d.free.saturating_add(j.size).min(d.total);
                    }
                    self.sel = None;
                    self.zoom = Some(Zoom::Fade);
                    self.zoom_serial += 1;
                    self.say(format!("{} freed · {} deleted", human(j.size), j.name), false, true);
                }
                Err(e) => self.say(format!("Couldn’t delete {}: {e}", j.name), false, false),
            }
            return;
        }
        if let Some(i) = self.sent.iter().position(|s| s.ticket == ticket) {
            let s = self.sent.remove(i);
            if let Err(e) = result {
                // the shell refused: what was taken off the screen is back
                if let Some(d) = self.data.as_mut() {
                    d.restore(&s.removed);
                    self.data_gen += 1;
                }
                self.say(format!("Couldn’t move {} to the Recycle Bin: {e}", s.name), false, false);
            }
        }
    }

    fn undo(&mut self) {
        if let Some(p) = self.pending.take() {
            if let Some(d) = self.data.as_mut() {
                d.restore(&p.removed);
                self.data_gen += 1;
            }
            self.zoom = Some(Zoom::Fade);
            self.zoom_serial += 1;
        }
        self.toast = None;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // menus

    fn menu_for(&self, id: Option<u32>) -> Vec<MenuItem> {
        let item = |t: &str, icon: &'static str, v: usize| MenuItem::new(t).icon(icon).value(v);
        let Some(d) = self.data.as_ref() else { return Vec::new() };
        match id {
            Some(id) => {
                let dir = d.is_dir(id);
                let mut v = vec![MenuItem::new(d.name(id)).kbd(human(d.size(id))).disabled(true)];
                if dir {
                    v.push(item("Open", look::ICON_OPEN, A_OPEN).kbd("Enter"));
                }
                v.push(item("Show in file manager", look::ICON_FOLDER, A_SHOW));
                v.push(item("Copy path", look::ICON_COPY, A_COPY));
                v.push(MenuItem::sep());
                v.push(item("Delete…", look::ICON_TRASH, A_DELETE).kbd("Del").danger());
                v
            }
            None => {
                let cur = self.nav.current();
                let head = if cur == ROOT { self.cur().map_or_else(String::new, |x| x.label.clone()) } else { d.name(cur).to_string() };
                vec![
                    MenuItem::new(head).kbd(human(d.size(cur))).disabled(true),
                    item("Up one level", look::ICON_UP, A_UP).kbd("Backspace").disabled(self.nav.at_root()),
                    item("Show in file manager", look::ICON_FOLDER, A_SHOW),
                    item("Scan again", look::ICON_RESCAN, A_RESCAN).kbd("F5"),
                ]
            }
        }
    }

    fn open_menu(&mut self, id: Option<u32>, x: f32, y: f32, cx: &mut Cx) {
        let items = self.menu_for(id);
        if items.is_empty() {
            return;
        }
        // a file is selected by the right button; a folder is only pointed at
        if let Some(i) = id.filter(|&i| self.data.as_ref().is_some_and(|d| !d.is_dir(i))) {
            self.sel = Some(i);
        }
        self.menu_on = id;
        cx.menu(TAG_MENU, items, Rect::new(x, y, 0.0, 0.0));
    }

    fn reveal(&self, id: Option<u32>) {
        let id = id.unwrap_or_else(|| self.nav.current());
        let p = self.path_of(id);
        if !p.is_empty() {
            self.be.reveal(&p);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the one place the three ways in (click, key, menu) meet

    fn act(&mut self, a: usize, id: Option<u32>, cx: &mut Cx) {
        match a {
            A_OPEN => {
                if let Some(i) = id.filter(|&i| self.data.as_ref().is_some_and(|d| d.is_dir(i))) {
                    self.open(i);
                }
            }
            A_SHOW => self.reveal(id),
            A_COPY => {
                let p = self.path_of(id.unwrap_or_else(|| self.nav.current()));
                cx.set_clipboard_text(p);
                self.say("Path copied", false, true);
            }
            A_DELETE => {
                if let Some(i) = id {
                    self.ask_delete(i);
                }
            }
            A_UP => self.up(),
            A_RESCAN => self.start_scan(),
            _ => {}
        }
        cx.invalidate();
    }

    fn rescan(&mut self, cx: &mut Cx) {
        if self.block().is_some() {
            return;
        }
        self.start_scan();
        cx.invalidate();
    }

    fn next_drive(&mut self, cx: &mut Cx) {
        let n = self.drives.len();
        if n > 1 {
            self.pick_drive((self.drive + 1) % n, cx);
        }
    }

    /// "Restart as administrator": the UAC prompt; if the user agrees, this window has done its part.
    fn elevate(&mut self, cx: &mut Cx) {
        if self.be.restart_elevated(self.letter()) {
            cx.close_window();
        }
    }

    /// "Scan slowly (instead)".
    fn go_slow(&mut self, cx: &mut Cx) {
        let l = self.letter();
        self.dismissed = true;
        if !self.slow_ok.contains(&l) {
            self.slow_ok.push(l);
        }
        self.start_scan();
        cx.invalidate();
    }

    /// What Enter and Delete are about: the selection, else what the pointer is over.
    fn target(&self) -> Option<u32> {
        let in_level = |r: &u32| self.layout.as_ref().is_some_and(|l| l.items.iter().any(|i| i.id == *r));
        self.sel.filter(in_level).or(self.hot.filter(in_level))
    }

    /// Where "Biggest here" took the user: a folder is gone into, a file of this level is selected, any other is shown in its folder.
    fn top_click(&mut self, k: usize) {
        let Some(&r) = self.tops.get(k) else { return };
        let Some(d) = self.data.as_ref() else { return };
        let trail = d.trail(r);
        if d.is_dir(r) {
            self.nav.set(&trail);
            self.sel = None;
        } else {
            let here = trail.len() == self.nav.path().len() + 1 && trail.starts_with(self.nav.path());
            if !here {
                self.nav.set(&trail[..trail.len() - 1]);
            }
            self.sel = Some(r);
        }
        self.zoom = None;
        self.list.scroll.scroll_to(0.0, 0);
    }

    fn detail_action(&mut self, k: usize, cx: &mut Cx) {
        if let Some(&(a, id)) = self.acts.get(k) {
            self.act(a, id, cx);
        }
    }

    /// The list's keys: the arrows, the pages, Home and End move the selection along the rows.
    fn list_key(&mut self, vk: u32, m: Mods) -> bool {
        let n = self.order.len();
        let rows = (self.list.scroll.view() / ROW_H).max(1.0) as usize;
        if !list_key(&mut self.list, vk, m.shift, false, n, rows) {
            return false;
        }
        if let (Some(c), Some(l)) = (self.list.cursor, self.layout.as_ref()) {
            self.sel = self.order.get(c).and_then(|&k| l.items.get(k)).map(|i| i.id);
        }
        true
    }
}

/// What stands in the way of the drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Admin,
    Fs,
    Failed(String),
}

impl App for Disk {
    fn title(&self) -> String {
        "WinRust Disk".into()
    }

    fn min_size(&self) -> (f32, f32) {
        (760.0, 520.0)
    }

    fn caption_height(&self) -> f32 {
        56.0
    }

    fn start(&mut self, hwnd: isize) {
        self.hwnd = hwnd;
        if self.block().is_none() && matches!(self.phase, Phase::Idle) && self.data.is_none() && self.cur().is_some_and(|d| d.ntfs() || self.slow_ok.contains(&d.letter)) {
            self.start_scan();
        }
    }

    fn demo_clicks(&self) -> Vec<Id> {
        match self.demo {
            DemoHook::None => Vec::new(),
            DemoHook::Menu(_) => vec![ID_DEMO],
            DemoHook::Confirm => vec![ID_DLG_OK],
        }
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        self.paint(c, f);
    }

    fn click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        let hovered = self.record_of(id);
        match id {
            ID_DEMO => {
                // the context menu of the design, at (380, 300) of the window
                if let DemoHook::Menu(item) = self.demo {
                    self.open_menu(Some(item), 380.0, 300.0, cx);
                }
            }
            ID_UP => self.up(),
            ID_RESCAN => self.rescan(cx),
            ID_VIEW => self.view = ViewMode::Map,
            i if i == ID_VIEW + 1 => self.view = ViewMode::List,
            ID_MAP if b == Button::Right => self.open_menu(None, x, y, cx),
            ID_BLOCK_RUN => match self.block() {
                Some(Block::Admin) => self.elevate(cx),
                Some(Block::Fs) => self.go_slow(cx),
                _ => self.rescan(cx),
            },
            ID_BLOCK_ALT => self.go_slow(cx),
            ID_EMPTY_UP => self.up(),
            ID_DLG_CANCEL | ID_DLG_SCRIM => self.dlg = None,
            ID_DLG_OK => self.confirm_delete(cx),
            ID_TOAST_UNDO => self.undo(),
            i if (ID_DLG_OPT..ID_DLG_OPT + 2).contains(&i) => {
                if let Some(d) = self.dlg.as_mut() {
                    d.how = if i == ID_DLG_OPT { How::Permanently } else { How::RecycleBin };
                }
            }
            i if (ID_DRIVE..ID_DRIVE + 10).contains(&i) => self.pick_drive((i - ID_DRIVE) as usize, cx),
            i if (ID_CRUMB..ID_CRUMB + 20).contains(&i) => self.zoom_to((i - ID_CRUMB) as usize),
            i if (ID_ACT..ID_ACT + 4).contains(&i) => self.detail_action((i - ID_ACT) as usize, cx),
            i if (ID_TOP..ID_TOP + 5).contains(&i) => self.top_click((i - ID_TOP) as usize),
            i if (ID_TILE..ID_TILE + 400).contains(&i) => {
                if let Some(t) = hovered {
                    if b == Button::Right {
                        self.open_menu(Some(t), x, y, cx);
                    } else if self.data.as_ref().is_some_and(|d| d.is_dir(t)) {
                        self.open(t);
                    } else {
                        self.sel = if self.sel == Some(t) { None } else { Some(t) };
                    }
                }
            }
            i if i >= ID_LIST + 2 => {
                if let Some(r) = hovered {
                    if b == Button::Right {
                        self.open_menu(Some(r), x, y, cx);
                    } else if self.data.as_ref().is_some_and(|d| d.is_dir(r)) {
                        self.open(r);
                    } else {
                        self.sel = if self.sel == Some(r) { None } else { Some(r) };
                    }
                }
            }
            i => {
                if let (Some(ColItem::Title(k)), Button::Left) = (col_of(ID_COLS, i), b) {
                    self.cols.sort_by(k);
                    self.resort();
                }
            }
        }
        cx.invalidate();
    }

    fn down(&mut self, id: Id, _b: Button, x: f32, y: f32, cx: &mut Cx) {
        let now = winrust_ui::anim::now_ms();
        if id == scroll::bar_id(ID_LIST) {
            scroll::bar_down(&mut self.list.scroll, y, now);
        } else if id == scroll::bar_id(ID_SIDE) {
            scroll::bar_down(&mut self.side, y, now);
        } else if let Some(ColItem::Grip(k)) = col_of(ID_COLS, id) {
            self.cols.grip_down(k, x);
        }
        cx.invalidate();
    }

    fn drag(&mut self, id: Id, x: f32, y: f32, cx: &mut Cx) {
        let now = winrust_ui::anim::now_ms();
        if id == scroll::bar_id(ID_LIST) {
            scroll::bar_drag(&mut self.list.scroll, y, now);
        } else if id == scroll::bar_id(ID_SIDE) {
            scroll::bar_drag(&mut self.side, y, now);
        } else if matches!(col_of(ID_COLS, id), Some(ColItem::Grip(_))) {
            self.cols.grip_drag(x);
        }
        cx.invalidate();
    }

    fn release(&mut self, id: Id, cx: &mut Cx) {
        if id == scroll::bar_id(ID_LIST) {
            scroll::bar_up(&mut self.list.scroll);
        } else if id == scroll::bar_id(ID_SIDE) {
            scroll::bar_up(&mut self.side);
        } else if matches!(col_of(ID_COLS, id), Some(ColItem::Grip(_))) {
            self.cols.grip_up();
        }
        cx.invalidate();
    }

    fn wheel(&mut self, _id: Option<Id>, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        let now = winrust_ui::anim::now_ms();
        if self.view == ViewMode::List {
            scroll::wheel(&mut self.list.scroll, cx.pointer, dy, ROW_H, now);
        }
        scroll::wheel(&mut self.side, cx.pointer, dy, 32.0, now);
        cx.invalidate();
    }

    fn key(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        const ESC: u32 = 0x1B;
        const ENTER: u32 = 0x0D;
        const BACK: u32 = 0x08;
        const TAB: u32 = 0x09;
        const DEL: u32 = 0x2E;
        const F5: u32 = 0x74;
        // the dialog has the keyboard: Enter is its button, Esc its Cancel, the arrows choose
        if self.dlg.is_some() {
            match vk {
                ENTER => self.confirm_delete(cx),
                ESC => self.dlg = None,
                0x26 | 0x28 => {
                    if let Some(d) = self.dlg.as_mut() {
                        d.how = if d.how == How::Permanently { How::RecycleBin } else { How::Permanently };
                    }
                }
                _ => return true,
            }
            cx.invalidate();
            return true;
        }
        let target = self.target();
        let used = match vk {
            ESC => {
                if self.sel.is_some() {
                    self.sel = None;
                } else {
                    cx.close_window();
                }
                true
            }
            BACK => {
                self.up();
                true
            }
            F5 => {
                self.rescan(cx);
                true
            }
            TAB => {
                self.next_drive(cx);
                true
            }
            DEL => match target {
                Some(t) => {
                    self.ask_delete(t);
                    true
                }
                None => false,
            },
            ENTER => match target.filter(|&t| self.data.as_ref().is_some_and(|d| d.is_dir(t))) {
                Some(t) => {
                    self.open(t);
                    true
                }
                None => false,
            },
            _ if self.view == ViewMode::List && self.ready() => self.list_key(vk, m),
            _ => false,
        };
        if used {
            cx.invalidate();
        }
        used
    }

    fn command(&mut self, tag: u32, choice: usize, cx: &mut Cx) {
        if tag == TAG_MENU {
            let id = self.menu_on.take();
            self.act(choice, id, cx);
        }
        cx.invalidate();
    }

    fn message(&mut self, msg: u32, _wp: usize, _lp: isize, cx: &mut Cx) {
        if msg == MSG_WAKE {
            self.drain(cx);
        }
    }

    fn wake(&mut self, cx: &mut Cx) {
        if let Some(p) = &self.pending {
            let left = p.due.saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.flush_pending();
            } else {
                cx.wake_in(left.as_millis() as u32 + 20);
            }
        }
        cx.invalidate();
    }

    fn close(&mut self, _cx: &mut Cx) -> bool {
        // what is waiting for its toast to end goes to the bin now: the process ends with the window, and nothing may be left half done
        if let Some(p) = self.pending.take() {
            let _ = self.be.delete(&[p.path], How::RecycleBin, true);
        }
        true
    }
}
