//! The window's state, and what a click, a key or a message from the worker does with it. Drawing is in `draw.rs`.
use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};

use winrust_common::Lang;
use winrust_ui::anim;
use winrust_ui::core::App;
use winrust_ui::frame::{Button, Cx, Frame, Id, Mods};
use winrust_ui::gfx::{Canvas, Picture};
use winrust_ui::widgets::overlay::Dialog;
use winrust_ui::widgets::scroll::{self, bar_id};
use winrust_ui::widgets::scroll_model::Scroll;

use super::catalog::{ROWS, Shot, index_of};
use super::model::{Backend, Job, Model, Shared, Step, run_job};
use super::preview;
use super::text::{T, t};
use crate::selection::RUST_EXPLORER;

// ids of the window's items (0 and up)
pub const ID_LIST: Id = 10; // and 11 for its bar
pub const ID_REMOVE_ALL: Id = 52; // what the host's benchmark and self-test open as a dialog
pub const ID_UPDATE: Id = 53;
pub const ID_INSTALL: Id = 54;
pub const ID_CANCEL: Id = 55;
pub const ID_BACK: Id = 56;
pub const ID_SIGN_OUT: Id = 57;
pub const ID_NEWER: Id = 58;
pub const ID_OD: Id = 60;
pub const ID_GROUP: Id = 200; // + group
pub const ID_LANG: Id = 300; // + language
pub const ID_ROW: Id = 400; // + row of the table

/// `Cx::dialog`: what comes back as `App::command`.
const TAG_DIALOG: u32 = 1;

/// What a worker posts (`WM_APP + n`).
pub const MSG_JOB: u32 = 0x8000 + 1;
pub const MSG_NEWER: u32 = 0x8000 + 2;

pub const LANGS: [Lang; 5] = [Lang::En, Lang::Ru, Lang::Ja, Lang::Pt, Lang::Zh];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// "Choose modules".
    Idle,
    /// "Installing".
    Busy,
    /// "Done".
    Done,
}

/// What the open dialog will do when it is agreed to.
enum Pending {
    Apply(Vec<Step>),
    Update(Vec<Step>),
    RemoveAll,
    SignOut,
}

/// What the finished page shows.
#[derive(Clone, Debug, Default)]
pub struct Finished {
    pub added: usize,
    pub removed: usize,
    pub rollback: bool,
    /// The modules that were done, and whether each went through.
    pub chips: Vec<(usize, bool)>,
    /// The errors, then the warnings, one line each.
    pub errors: Vec<String>,
    pub notes: Vec<String>,
}

impl Finished {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub struct Installer {
    pub(super) be: Arc<dyn Backend>,
    pub lang: Lang,
    pub model: Model,
    /// The row whose preview is shown.
    pub peek: usize,
    pub phase: Phase,
    pub job: Shared,
    pub fin: Option<Finished>,
    hwnd: Arc<AtomicIsize>,
    pub newer: Arc<Mutex<Option<String>>>,
    pub(super) scroll: Scroll,
    pub(super) pics: HashMap<(Shot, bool), Option<Picture>>,
    /// The preview (and the text under it) comes in: a new one starts its animation.
    pub shots: u32,
    pub shot_fresh: bool,
    pub(super) ok_fresh: bool,
    pending: Option<Pending>,
    pub od: bool,
    pub(super) elevated: bool,
    pub(super) version: String,
    pub(super) cancelling: bool,
    pub last_pos: usize,
    last_focus: Option<Id>,
    title_sent: String,
    pub(super) win: (f32, f32),
    pub(super) drawn_light: bool,
    pub demo_clicks: Vec<Id>,
}

/// Index of the row of a table the id of an item stands for.
#[must_use]
pub fn row_of_id(id: Id) -> Option<usize> {
    (ID_ROW..ID_ROW + ROWS.len() as Id).contains(&id).then(|| (id - ID_ROW) as usize)
}

impl Installer {
    #[must_use]
    pub fn new(be: Arc<dyn Backend>, lang: Lang) -> Self {
        let model = Model::new(be.mods());
        let peek = index_of("explorer").unwrap_or(0);
        Self {
            od: be.onedrive_hidden(),
            elevated: be.elevated(),
            version: be.version(),
            be,
            lang,
            model,
            peek,
            phase: Phase::Idle,
            job: Arc::new(Mutex::new(Job::default())),
            fin: None,
            hwnd: Arc::new(AtomicIsize::new(0)),
            newer: Arc::new(Mutex::new(None)),
            scroll: Scroll::default(),
            pics: HashMap::new(),
            shots: 0,
            shot_fresh: true,
            ok_fresh: true,
            pending: None,
            cancelling: false,
            last_pos: 0,
            last_focus: None,
            title_sent: String::new(),
            win: (1000.0, 640.0),
            drawn_light: false,
            demo_clicks: Vec::new(),
        }
    }

    pub(super) fn t(&self) -> &'static T {
        t(self.lang)
    }

    /// The version of a newer release that was found.
    pub(super) fn newer_version(&self) -> Option<String> {
        self.newer.lock().unwrap().clone()
    }

    pub(super) fn set_peek(&mut self, r: usize) {
        if self.peek != r {
            self.peek = r;
            self.shots += 1;
            self.shot_fresh = true;
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // what is on the list

    /// Where row `r` is in the content of the list (DIPs from its top), and the content's height.
    #[must_use]
    pub fn layout() -> (Vec<(usize, f32)>, [f32; 3], f32) {
        let mut y = 8.0;
        let mut rows = Vec::new();
        let mut heads = [0.0; 3];
        for g in 0..3u8 {
            heads[g as usize] = y;
            y += 36.0;
            for (r, row) in ROWS.iter().enumerate() {
                if row.group == g {
                    rows.push((r, y));
                    y += 54.0;
                }
            }
        }
        (rows, heads, y + 12.0)
    }

    /// What the install button says and does now: (label, enabled, removes something).
    #[must_use]
    pub fn install_button(&self) -> (String, bool) {
        let t = self.t();
        let (rm, add) = self.model.diff();
        let label = match (rm.len(), add.len()) {
            (0, 0) => t.choose.to_string(),
            (0, a) => t.install_n(a),
            (r, 0) => t.remove_n(r),
            (r, a) => t.apply_ar(a, r),
        };
        (label, self.phase == Phase::Idle && self.elevated && !(rm.is_empty() && add.is_empty()))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // doing things

    fn sync_title(&mut self, cx: &mut Cx) {
        let title = self.t().window.to_string();
        if title != self.title_sent {
            cx.set_title(title.clone());
            self.title_sent = title;
        }
    }

    fn set_language(&mut self, l: Lang, cx: &mut Cx) {
        if l == self.lang {
            return;
        }
        self.lang = l;
        let written = self.be.set_lang(l);
        let t = self.t();
        cx.toast(if written { t.toast_lang_ok } else { t.toast_lang_local }, None);
        self.sync_title(cx);
    }

    /// Starts the job on a thread of its own: the window goes on drawing, and hears of every step as a message.
    fn start_job(&mut self, steps: Vec<Step>, rollback: bool, cx: &mut Cx) {
        if let Some(s) = steps.first() {
            self.set_peek(s.row);
        }
        let job: Shared = Arc::new(Mutex::new(Job { steps, rollback, ..Job::default() }));
        self.job = job.clone();
        self.phase = Phase::Busy;
        self.fin = None;
        self.cancelling = false;
        self.last_pos = 0;
        let (be, hwnd) = (self.be.clone(), self.hwnd.clone());
        std::thread::spawn(move || {
            run_job(&*be, &job, &|| {
                let h = hwnd.load(Ordering::Relaxed);
                // a render and a test have no window to tell: they look at the job themselves
                if h != 0 {
                    winrust_ui::window::post(h, MSG_JOB, 0, 0);
                }
            });
        });
        cx.invalidate();
    }

    /// The worker changed something: the step in hand, a result, the end.
    fn sync_job(&mut self, cx: &mut Cx) {
        let j = self.job.lock().unwrap().clone();
        if self.phase == Phase::Busy {
            // the preview follows the step in hand
            if j.pos != self.last_pos || self.last_pos == 0 {
                self.last_pos = j.pos;
                if let Some(s) = j.steps.get(j.pos).filter(|_| !j.finished) {
                    self.set_peek(s.row);
                }
            }
            if j.finished {
                self.finish(&j);
            }
        }
        cx.invalidate();
    }

    /// The job is over: what is installed is read again and the finished page is made.
    pub fn finish(&mut self, j: &Job) {
        self.model.refresh(self.be.mods());
        // stopped before anything was done: back to the list, as if nothing was asked
        if j.cancel && j.results.is_empty() && !j.rollback {
            self.phase = Phase::Idle;
            self.cancelling = false;
            return;
        }
        let mut f = Finished { rollback: j.rollback, ..Finished::default() };
        if let Some(r) = &j.rolled {
            match r {
                Ok(m) if m.contains("install.log") => f.notes.push(m.clone()),
                Ok(_) => {}
                Err(e) => f.errors.push(e.clone()),
            }
        }
        for (s, r) in j.steps.iter().zip(&j.results) {
            let name = ROWS[s.row].name;
            match r {
                Ok(m) => {
                    if s.install {
                        f.added += 1;
                    } else {
                        f.removed += 1;
                    }
                    f.chips.push((s.row, true));
                    if m.contains("install.log") {
                        f.notes.push(m.clone());
                    }
                }
                Err(e) => {
                    f.chips.push((s.row, false));
                    f.errors.push(format!("{name}: {e}"));
                }
            }
        }
        self.fin = Some(f);
        self.phase = Phase::Done;
        self.ok_fresh = true;
    }

    /// Everything the button asks, with the questions that come first.
    fn apply(&mut self, cx: &mut Cx) {
        if self.phase != Phase::Idle || !self.elevated {
            return;
        }
        let (rm, add) = self.model.diff();
        if rm.is_empty() && add.is_empty() {
            return;
        }
        let steps: Vec<Step> = rm.iter().chain(&add).cloned().collect();
        if !self.model.needs_confirm(&rm, &add) {
            return self.start_job(steps, false, cx);
        }
        let t = self.t();
        let mut text = String::new();
        let names = |v: &[Step]| v.iter().map(|s| self.model.name_of(s)).collect::<Vec<_>>().join(", ");
        if !add.is_empty() {
            text.push_str(&t.will_install(&names(&add)));
        }
        if !rm.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&t.will_remove(&names(&rm)));
        }
        // what the big ones change gets its own warning
        for s in add.iter().filter(|s| self.model.mods[s.m].name == RUST_EXPLORER || !self.model.mods[s.m].bulk) {
            text.push_str(&format!("\n\n{}: {}", self.model.name_of(s), self.be.warning(s.m, self.lang)));
        }
        if !add.is_empty() {
            text.push_str("\n\n");
            text.push_str(t.restore_point);
        }
        let purge = add.iter().any(|s| self.model.mods[s.m].name == "purge");
        let (title, ok, danger) = if purge {
            (t.ap_purge, t.ap_purge_ok, true)
        } else if add.is_empty() {
            (t.ap_remove, t.ap_remove_ok, true)
        } else if rm.is_empty() {
            (t.ap_install, t.ap_install_ok, false)
        } else {
            (t.ap_apply, t.ap_apply_ok, false)
        };
        self.ask(Pending::Apply(steps), dialog(title, &text, ok, danger, t), cx);
    }

    fn ask(&mut self, p: Pending, d: Dialog, cx: &mut Cx) {
        self.pending = Some(p);
        cx.dialog(TAG_DIALOG, d);
    }

    fn reset(&mut self) {
        self.model.reset_selection();
        self.phase = Phase::Idle;
        self.fin = None;
        self.job = Arc::new(Mutex::new(Job::default()));
        self.cancelling = false;
        self.shot_fresh = true;
    }

    /// The row of a key move: the next one that can take the focus.
    fn move_focus(&mut self, from: Option<usize>, d: isize, cx: &mut Cx) {
        let (rows, _, _) = Self::layout();
        let order: Vec<usize> = rows.iter().map(|&(r, _)| r).collect();
        let at = from.and_then(|r| order.iter().position(|&o| o == r));
        let to = match at {
            Some(i) => (i as isize + d).clamp(0, order.len() as isize - 1) as usize,
            None => 0,
        };
        cx.focus(ID_ROW + order[to] as Id);
    }
}

/// A dialog of the window.
fn dialog(title: &str, text: &str, ok: &str, danger: bool, t: &T) -> Dialog {
    let mut d = Dialog::new(title, text, &[t.cancel_btn, ok]);
    d.width = 560.0;
    d.icon(if danger { WARN } else { INFO }, danger)
}

const WARN: &str = "M12 3l9.5 17h-19zM12 10v4.5M12 17.5h.01";
const INFO: &str = "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01";

impl App for Installer {
    fn title(&self) -> String {
        self.t().window.to_string()
    }

    fn min_size(&self) -> (f32, f32) {
        (860.0, 560.0)
    }

    fn start(&mut self, hwnd: isize) {
        self.hwnd.store(hwnd, Ordering::Relaxed);
        // the network is asked once, off the window's thread
        let (be, slot) = (self.be.clone(), self.newer.clone());
        std::thread::spawn(move || {
            if let Some(v) = be.newer() {
                *slot.lock().unwrap() = Some(v);
                winrust_ui::window::post(hwnd, MSG_NEWER, 0, 0);
            }
        });
    }

    fn demo_clicks(&self) -> Vec<Id> {
        self.demo_clicks.clone()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        self.paint(c, f);
    }

    fn click(&mut self, id: Id, _b: Button, _x: f32, _y: f32, cx: &mut Cx) {
        match id {
            i if row_of_id(i).is_some() => {
                let r = row_of_id(i).unwrap_or_default();
                if self.phase == Phase::Idle {
                    self.set_peek(r);
                    self.model.toggle(r);
                }
            }
            i if (ID_GROUP..ID_GROUP + 3).contains(&i) && self.phase == Phase::Idle => self.model.toggle_group((i - ID_GROUP) as u8),
            i if (ID_LANG..ID_LANG + LANGS.len() as Id).contains(&i) => self.set_language(LANGS[(i - ID_LANG) as usize], cx),
            ID_REMOVE_ALL if self.phase == Phase::Idle && self.elevated => {
                let t = self.t();
                self.ask(Pending::RemoveAll, dialog(t.rm_title, t.rm_text, t.rm_ok, true, t), cx);
            }
            ID_UPDATE if self.phase == Phase::Idle && self.elevated => {
                let steps = self.model.update_steps();
                if !steps.is_empty() {
                    let t = self.t();
                    self.ask(Pending::Update(steps), dialog(t.up_title, t.up_text, t.up_ok, false, t), cx);
                }
            }
            ID_INSTALL => self.apply(cx),
            ID_CANCEL if self.phase == Phase::Busy && !self.cancelling => {
                self.cancelling = true;
                self.job.lock().unwrap().cancel = true;
            }
            ID_BACK if self.phase == Phase::Done => self.reset(),
            ID_SIGN_OUT if self.phase == Phase::Done => {
                let t = self.t();
                self.ask(Pending::SignOut, dialog(t.so_title, t.so_text, t.sign_out, false, t), cx);
            }
            ID_NEWER => self.be.open_releases(),
            ID_OD => match self.be.set_onedrive_hidden(!self.od) {
                Ok(()) => self.od = !self.od,
                Err(e) => cx.toast(self.t().error(&e), None),
            },
            _ => {}
        }
        cx.invalidate();
    }

    fn down(&mut self, id: Id, _b: Button, _x: f32, y: f32, _cx: &mut Cx) {
        if id == bar_id(ID_LIST) {
            scroll::bar_down(&mut self.scroll, y, anim::now_ms());
        }
    }

    fn drag(&mut self, id: Id, _x: f32, y: f32, cx: &mut Cx) {
        if id == bar_id(ID_LIST) {
            scroll::bar_drag(&mut self.scroll, y, anim::now_ms());
            cx.invalidate();
        }
    }

    fn release(&mut self, id: Id, _cx: &mut Cx) {
        if id == bar_id(ID_LIST) {
            scroll::bar_up(&mut self.scroll);
        }
    }

    fn wheel_at(&mut self, _id: Option<Id>, x: f32, y: f32, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        if scroll::wheel(&mut self.scroll, Some((x, y)), dy, 54.0, anim::now_ms()) {
            cx.invalidate();
        }
    }

    fn key(&mut self, vk: u32, _m: Mods, cx: &mut Cx) -> bool {
        const UP: u32 = 0x26;
        const DOWN: u32 = 0x28;
        if self.phase != Phase::Idle {
            return false;
        }
        let focus_row = cx.focus.and_then(row_of_id);
        match vk {
            UP => self.move_focus(focus_row.or(Some(self.peek)), -1, cx),
            DOWN => self.move_focus(focus_row.or(Some(self.peek)), 1, cx),
            _ => return false,
        }
        true
    }

    fn command(&mut self, tag: u32, choice: usize, cx: &mut Cx) {
        if tag != TAG_DIALOG {
            return;
        }
        let Some(p) = self.pending.take() else { return };
        if choice != 1 {
            return;
        }
        match p {
            Pending::Apply(steps) | Pending::Update(steps) => self.start_job(steps, false, cx),
            Pending::RemoveAll => self.start_job(Vec::new(), true, cx),
            Pending::SignOut => self.be.sign_out(),
        }
        cx.invalidate();
    }

    fn message(&mut self, msg: u32, _wp: usize, _lp: isize, cx: &mut Cx) {
        match msg {
            MSG_JOB => self.sync_job(cx),
            MSG_NEWER => cx.invalidate(),
            _ => {}
        }
    }

    fn close(&mut self, cx: &mut Cx) -> bool {
        // closing in the middle would leave the system half changed
        if self.phase == Phase::Busy {
            cx.toast(self.t().toast_busy, None);
            return false;
        }
        true
    }
}

impl Installer {
    /// The keyboard focus moved to a row (Tab, the arrows): the list scrolls to it, and its preview comes up.
    pub(super) fn follow_focus(&mut self, focus: Option<Id>, now: u64) {
        if focus == self.last_focus {
            return;
        }
        self.last_focus = focus;
        if let Some(r) = focus.and_then(row_of_id) {
            if self.phase == Phase::Idle {
                self.set_peek(r);
            }
            let (rows, _, _) = Self::layout();
            if let Some(&(_, y)) = rows.iter().find(|&&(o, _)| o == r) {
                self.scroll.ensure_visible(y - 8.0, 54.0 + 16.0, now);
            }
        }
    }

    /// The pointer rests over a row: its preview comes up (`onMouseEnter` of the mockup).
    pub(super) fn hover_peek(&mut self, over: Option<Id>) {
        if self.phase == Phase::Idle
            && let Some(r) = over.and_then(row_of_id)
        {
            self.set_peek(r);
        }
    }

    /// The picture of a preview, on the GPU (made when it is first looked at, and again after a lost device).
    pub(super) fn picture(&mut self, c: &mut Canvas<'_>, shot: Shot, light: bool) -> Option<&Picture> {
        let key = (shot, light && shot == Shot::Keys);
        if self.pics.get(&key).and_then(Option::as_ref).is_some_and(|p| c.picture_stale(p)) {
            self.pics.remove(&key);
        }
        self.pics.entry(key).or_insert_with(|| preview::decode(shot.png(key.1)).and_then(|d| c.pixels(&d.px, d.w as i32, d.h as i32))).as_ref()
    }

    /// Rows in a run, by row: how far each one is.
    pub(super) fn runs(&self) -> HashMap<usize, Run> {
        let j = self.job.lock().unwrap();
        let mut m = HashMap::new();
        for (k, s) in j.steps.iter().enumerate() {
            let run = if let Some(r) = j.results.get(k) {
                if r.is_ok() { Run::Done } else { Run::Failed }
            } else if j.finished {
                continue; // not started: the job was stopped before it
            } else if k == j.pos {
                Run::Now
            } else {
                Run::Queued
            };
            m.insert(s.row, run);
        }
        m
    }

    /// How far the job is: the step in hand (counted from 1), the number of steps, and the share done.
    pub(super) fn progress(&self) -> (usize, usize, f32) {
        let j = self.job.lock().unwrap();
        let n = j.steps.len();
        let at = if j.finished { n } else { j.pos.min(n.saturating_sub(1)) };
        (if n == 0 { 0 } else { (at + 1).min(n) }, n, if n == 0 { 0.0 } else { (at as f32 + if j.finished { 0.0 } else { 0.5 }) / n as f32 })
    }

    /// The step in hand: what it does and to which row.
    pub(super) fn now_step(&self) -> Option<Step> {
        let j = self.job.lock().unwrap();
        j.steps.get(j.pos).filter(|_| !j.finished).cloned()
    }

    pub(super) fn rolling_back(&self) -> bool {
        self.job.lock().unwrap().rollback
    }
}

/// How far a row is in the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    Queued,
    Now,
    Done,
    Failed,
}
