//! What the window knows, with no window: the switches and what they ask for, and the job that carries it out on a thread of its own.
//! The installer's real work (`install_plugin`, `uninstall_plugin`, `rollback`) is reached through [`Backend`], so the window is the same
//! one a render or a test drives with steps that touch nothing.

use std::sync::{Arc, Mutex};

use winrust_common::Lang;

use super::catalog::{ROWS, Source};
use crate::selection::{self, RUST_EXPLORER};

/// One plugin of the installer, as the window needs to see it.
#[derive(Clone, Debug)]
pub struct ModInfo {
    pub name: &'static str,
    pub installed: bool,
    /// Part of "install everything"; the ones that are not (debloat, purge) change the system and only go in on request.
    pub bulk: bool,
    /// It has an install and an uninstall (`chrome-rs` has none yet).
    pub can: bool,
}

/// Everything the window asks of the machine. The real one is in `main.rs`; the one of `--render` and the tests changes nothing.
pub trait Backend: Send + Sync + 'static {
    /// The plugins, in the order they are installed, with what is installed now.
    fn mods(&self) -> Vec<ModInfo>;
    /// Installs (or updates) / removes plugin `m` (an index of [`Backend::mods`]); the text is what the installer reports.
    fn install(&self, m: usize) -> Result<String, String>;
    fn uninstall(&self, m: usize) -> Result<String, String>;
    /// Removes every plugin.
    fn rollback(&self) -> Result<String, String>;
    /// What installing `m` changes beyond its own files, in `lang`: shown before the switch is applied.
    fn warning(&self, m: usize, lang: Lang) -> String;
    fn onedrive_hidden(&self) -> bool;
    fn set_onedrive_hidden(&self, hide: bool) -> Result<(), String>;
    /// Switches the language of the other programs too (`lang.txt`); false when that could not be written.
    fn set_lang(&self, l: Lang) -> bool;
    fn version(&self) -> String;
    /// A newer release than this one, if there is (it asks the network: called on a thread of its own).
    fn newer(&self) -> Option<String>;
    fn open_releases(&self);
    fn sign_out(&self);
    fn elevated(&self) -> bool;
}

/// What one step of a job does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The row of the table.
    pub row: usize,
    /// The plugin ([`ModInfo`] index).
    pub m: usize,
    pub install: bool,
}

/// A running or finished job; the worker writes it, the window reads it when the worker posts a message.
#[derive(Clone, Debug, Default)]
pub struct Job {
    pub steps: Vec<Step>,
    /// "Remove all": one step with no list.
    pub rollback: bool,
    /// The step being done; `steps[..pos]` are finished.
    pub pos: usize,
    /// What each finished step reported.
    pub results: Vec<Result<String, String>>,
    pub finished: bool,
    /// Stop after the step in hand.
    pub cancel: bool,
    /// What a rollback reported.
    pub rolled: Option<Result<String, String>>,
}

pub type Shared = Arc<Mutex<Job>>;

/// Does what the job says, step by step; `notify` is called when the step in hand changes and when it ends.
/// A failed step does not stop the others (they are independent); a cancel stops after the step in hand.
pub fn run_job(be: &dyn Backend, job: &Mutex<Job>, notify: &dyn Fn()) {
    let rollback = job.lock().unwrap().rollback;
    if rollback {
        let r = be.rollback();
        let mut j = job.lock().unwrap();
        j.rolled = Some(r);
        j.finished = true;
        drop(j);
        notify();
        return;
    }
    let n = job.lock().unwrap().steps.len();
    for k in 0..n {
        let step = {
            let mut j = job.lock().unwrap();
            if j.cancel {
                break;
            }
            j.pos = k;
            j.steps[k].clone()
        };
        notify();
        let r = if step.install { be.install(step.m) } else { be.uninstall(step.m) };
        job.lock().unwrap().results.push(r);
        notify();
    }
    let mut j = job.lock().unwrap();
    j.pos = j.results.len();
    j.finished = true;
    drop(j);
    notify();
}

/// The switches.
pub struct Model {
    pub mods: Vec<ModInfo>,
    /// For each row of the table: the plugin it installs, when it is a plugin of its own that can be installed.
    own: Vec<Option<usize>>,
    /// For each row: the row whose plugin carries it (`Source::With`).
    with: Vec<Option<usize>>,
    /// The switch of each row.
    pub sel: Vec<bool>,
}

impl Model {
    #[must_use]
    pub fn new(mods: Vec<ModInfo>) -> Self {
        let own = ROWS.iter().map(|r| if let Source::Plugin(n) = r.source { mods.iter().position(|m| m.name == n && m.can) } else { None }).collect();
        let with = ROWS.iter().map(|r| if let Source::With(n) = r.source { ROWS.iter().position(|o| o.source == Source::Plugin(n)) } else { None }).collect();
        let mut m = Self { mods, own, with, sel: vec![false; ROWS.len()] };
        m.reset_selection();
        m
    }

    /// The switches as the window starts, the installer's "Express" choice: what is installed stays, plus the recommended modules on a
    /// fresh machine (the file manager that replaces Explorer only on request); an applied extra shows as on, a new one is never on.
    pub fn reset_selection(&mut self) {
        for r in 0..ROWS.len() {
            self.sel[r] = match self.own[r] {
                Some(m) => selection::express_pick(self.mods[m].name, self.mods[m].bulk, self.mods[m].installed),
                None => false,
            };
        }
    }

    /// The installed state is read again (after a job); the switches stay where the user put them.
    pub fn refresh(&mut self, mods: Vec<ModInfo>) {
        self.mods = mods;
    }

    /// The plugin a row installs by itself.
    #[must_use]
    pub fn own(&self, r: usize) -> Option<usize> {
        self.own[r]
    }

    /// The row whose switch a row without one follows.
    #[must_use]
    pub fn parent(&self, r: usize) -> Option<usize> {
        self.with[r]
    }

    /// What the row's switch shows: its own, or its carrier's.
    #[must_use]
    pub fn on(&self, r: usize) -> bool {
        match self.with[r] {
            Some(p) => self.sel[p],
            None => self.own[r].is_some() && self.sel[r],
        }
    }

    /// The row's plugin (or its carrier's) is installed now.
    #[must_use]
    pub fn installed(&self, r: usize) -> bool {
        self.own[r].or_else(|| self.with[r].and_then(|p| self.own[p])).is_some_and(|m| self.mods[m].installed)
    }

    /// A purge that has run stays on: unticking it would only delete purge.exe, the apps do not come back.
    #[must_use]
    pub fn locked(&self, r: usize) -> bool {
        ROWS[r].source == Source::Plugin("purge") && self.installed(r)
    }

    /// Whether the row has a switch of its own.
    #[must_use]
    pub fn live(&self, r: usize) -> bool {
        self.own[r].is_some()
    }

    fn row_of(&self, plugin: &str) -> Option<usize> {
        ROWS.iter().position(|r| matches!(r.source, Source::Plugin(n) if n == plugin))
    }

    /// The switch of row `r` is turned; true when something changed. The file manager drags the bar along, and the bar off drags the
    /// file manager off with it (it has no bar to stand on).
    pub fn toggle(&mut self, r: usize) -> bool {
        let Some(m) = self.own[r] else { return false };
        if self.locked(r) {
            return false;
        }
        let on = !self.sel[r];
        self.sel[r] = on;
        if let Some((other, v)) = selection::linked(self.mods[m].name, on)
            && let Some(o) = self.row_of(other)
            && self.own[o].is_some()
        {
            self.sel[o] = v;
        }
        true
    }

    /// The rows of a group that "All" / "None" turns: the ones that are part of "install everything".
    #[must_use]
    pub fn group_rows(&self, g: u8) -> Vec<usize> {
        (0..ROWS.len()).filter(|&r| ROWS[r].group == g && self.own[r].is_some_and(|m| self.mods[m].bulk)).collect()
    }

    /// "All" turns them all on, unless they are, and then "None".
    #[must_use]
    pub fn group_all_on(&self, g: u8) -> bool {
        let rows = self.group_rows(g);
        !rows.is_empty() && rows.iter().all(|&r| self.sel[r])
    }

    pub fn toggle_group(&mut self, g: u8) {
        let on = !self.group_all_on(g);
        for r in self.group_rows(g) {
            self.sel[r] = on;
        }
        // the pairs that go together stay together
        for r in self.group_rows(g) {
            if let Some(m) = self.own[r]
                && let Some((other, v)) = selection::linked(self.mods[m].name, on)
                && let Some(o) = self.row_of(other)
                && self.own[o].is_some()
            {
                self.sel[o] = v;
            }
        }
    }

    /// How many modules are switched on, and how much they weigh.
    #[must_use]
    pub fn selected(&self) -> (usize, f32) {
        let rows: Vec<usize> = (0..ROWS.len()).filter(|&r| self.own[r].is_some() && self.sel[r]).collect();
        (rows.len(), 0.0 + rows.iter().map(|&r| ROWS[r].size_mb).sum::<f32>()) // `0.0 +`: an empty sum is -0.0, which prints as "-0.0"
    }

    /// What the install button would do now: (to remove, to install), in install order. A switch says "I want this installed", so only
    /// the difference is touched; removal runs backwards (the file manager before the bar).
    #[must_use]
    pub fn diff(&self) -> (Vec<Step>, Vec<Step>) {
        let mut order: Vec<usize> = (0..ROWS.len()).filter(|&r| self.own[r].is_some()).collect();
        order.sort_by_key(|&r| self.own(r));
        let (rm, add) = selection::plan(&order, |r| self.installed(r), |r| self.sel[r]);
        let step = |install| move |r: usize| Step { row: r, m: self.own[r].unwrap_or_default(), install };
        (rm.into_iter().map(step(false)).collect(), add.into_iter().map(step(true)).collect())
    }

    /// "Update": every installed module of the list, put in again from this installer (never the extras: a purge would run again).
    #[must_use]
    pub fn update_steps(&self) -> Vec<Step> {
        let mut rows: Vec<usize> = (0..ROWS.len()).filter(|&r| self.own[r].is_some_and(|m| self.mods[m].installed && self.mods[m].bulk)).collect();
        rows.sort_by_key(|&r| self.own[r]);
        rows.into_iter().map(|r| Step { row: r, m: self.own[r].unwrap_or_default(), install: true }).collect()
    }

    /// Something is installed.
    #[must_use]
    pub fn any_installed(&self) -> bool {
        self.mods.iter().any(|m| m.installed && m.can)
    }

    /// The button asks for something that cannot be undone by a click, or that takes a part of Windows away: it is confirmed first.
    #[must_use]
    pub fn needs_confirm(&self, rm: &[Step], add: &[Step]) -> bool {
        !rm.is_empty() || add.iter().any(|s| self.mods[s.m].name == RUST_EXPLORER || !self.mods[s.m].bulk)
    }

    /// The program name of a step.
    #[must_use]
    pub fn name_of(&self, s: &Step) -> &'static str {
        ROWS[s.row].name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(installed: &[&str]) -> Vec<ModInfo> {
        [("rust-search", true), ("rtaskmgr-win32", true), ("topbar", true), ("rust-explorer", true), ("wiztree", true), ("photoview", true), ("rust-settings", true), ("debloat", false), ("purge", false)]
            .iter()
            .map(|&(name, bulk)| ModInfo { name, installed: installed.contains(&name), bulk, can: true })
            .collect()
    }

    fn row(id: &str) -> usize {
        super::super::catalog::index_of(id).unwrap()
    }

    #[test]
    fn a_fresh_machine_gets_the_recommended_set_without_the_file_manager_or_the_extras() {
        let m = Model::new(mods(&[]));
        assert!(m.sel[row("topbar")] && m.sel[row("search")] && m.sel[row("taskmgr")] && m.sel[row("settings")] && m.sel[row("photo")]);
        assert!(!m.sel[row("explorer")], "Explorer is replaced only on request");
        assert!(!m.sel[row("debloat")] && !m.sel[row("purge")], "extras never by default");
        assert!(!m.live(row("term")) && !m.live(row("shell")), "the terminal is not there yet and the shell rides on the file manager");
    }

    #[test]
    fn what_is_installed_stays_on_and_a_change_is_a_difference() {
        let mut m = Model::new(mods(&["topbar", "rust-explorer", "debloat"]));
        assert!(m.sel[row("explorer")] && m.sel[row("debloat")], "installed ones show as on");
        let (rm, add) = m.diff();
        assert!(rm.is_empty());
        let added: Vec<_> = add.iter().map(|s| ROWS[s.row].id).collect();
        assert_eq!(added, ["search", "taskmgr", "wiztree", "photo", "settings"], "the PLUGINS order");
        // the bar off takes the file manager with it, and both are removed, the file manager first
        m.toggle(row("topbar"));
        assert!(!m.sel[row("explorer")]);
        let (rm, _) = m.diff();
        let removed: Vec<_> = rm.iter().map(|s| ROWS[s.row].id).collect();
        assert_eq!(removed, ["explorer", "topbar"]);
    }

    #[test]
    fn the_file_manager_pulls_the_bar_in() {
        let mut m = Model::new(mods(&[]));
        m.sel[row("topbar")] = false;
        m.toggle(row("explorer"));
        assert!(m.sel[row("explorer")] && m.sel[row("topbar")]);
    }

    #[test]
    fn a_row_that_rides_on_another_follows_its_switch_and_has_none() {
        let mut m = Model::new(mods(&[]));
        assert!(m.on(row("reshell")), "the hotkeys come with the bar");
        assert!(!m.toggle(row("reshell")), "there is no switch to turn");
        m.toggle(row("topbar"));
        assert!(!m.on(row("reshell")));
        assert!(!m.on(row("shell")), "the shell comes with the file manager, which is off");
    }

    #[test]
    fn a_purge_that_has_run_cannot_be_switched_off() {
        let mut m = Model::new(mods(&["purge"]));
        assert!(m.sel[row("purge")]);
        assert!(!m.toggle(row("purge")));
        assert!(m.sel[row("purge")]);
    }

    #[test]
    fn all_in_a_group_leaves_the_extras_alone() {
        let mut m = Model::new(mods(&[]));
        assert_eq!(m.group_rows(2), Vec::<usize>::new(), "the system group has no All: nothing in it is part of install everything");
        let apps = m.group_rows(1);
        assert!(apps.iter().all(|&r| m.sel[r]), "they start on");
        m.toggle_group(1);
        assert!(apps.iter().all(|&r| !m.sel[r]), "All, with all of them on, is None");
        m.toggle_group(1);
        assert!(apps.iter().all(|&r| m.sel[r]));
        m.toggle_group(1);
        m.toggle_group(0);
        assert!(m.sel[row("explorer")] && m.sel[row("topbar")]);
        assert!(!m.sel[row("debloat")] && !m.sel[row("purge")]);
    }

    #[test]
    fn the_count_is_of_what_can_be_installed() {
        let m = Model::new(mods(&[]));
        let (n, mb) = m.selected();
        assert_eq!(n, 6, "topbar, search, taskmgr, wiztree, photoview, settings: the shell and the hotkeys ride on others, the terminal is not there");
        assert!((mb - (2.4 + 1.8 + 2.6 + 1.6 + 1.1 + 2.9)).abs() < 0.01, "{mb}");
    }

    #[test]
    fn nothing_selected_is_zero_megabytes_not_minus_zero() {
        let mut m = Model::new(mods(&[]));
        m.sel.iter_mut().for_each(|s| *s = false);
        let (n, mb) = m.selected();
        assert_eq!(n, 0);
        assert_eq!(super::super::text::t(Lang::En).selected(n, mb), "0 selected · 0.0 MB");
    }

    #[test]
    fn what_takes_a_part_of_windows_away_or_cannot_be_undone_is_confirmed() {
        let mut m = Model::new(mods(&[]));
        let (rm, add) = m.diff();
        assert!(!m.needs_confirm(&rm, &add), "the recommended set goes in with no question");
        m.toggle(row("explorer"));
        let (rm, add) = m.diff();
        assert!(m.needs_confirm(&rm, &add), "replacing Explorer is asked about");
        m.toggle(row("explorer"));
        m.toggle(row("purge"));
        let (rm, add) = m.diff();
        assert!(m.needs_confirm(&rm, &add), "purge is asked about");
    }

    #[test]
    fn update_puts_in_what_is_installed_but_never_an_extra() {
        let m = Model::new(mods(&["topbar", "photoview", "purge"]));
        let ids: Vec<_> = m.update_steps().iter().map(|s| ROWS[s.row].id).collect();
        assert_eq!(ids, ["topbar", "photo"]);
    }

    struct Fake {
        log: Mutex<Vec<String>>,
        fail: Option<usize>,
    }

    impl Backend for Fake {
        fn mods(&self) -> Vec<ModInfo> {
            mods(&[])
        }
        fn install(&self, m: usize) -> Result<String, String> {
            self.log.lock().unwrap().push(format!("install {m}"));
            if self.fail == Some(m) { Err("boom".into()) } else { Ok("ok".into()) }
        }
        fn uninstall(&self, m: usize) -> Result<String, String> {
            self.log.lock().unwrap().push(format!("uninstall {m}"));
            Ok("gone".into())
        }
        fn rollback(&self) -> Result<String, String> {
            self.log.lock().unwrap().push("rollback".into());
            Ok("all gone".into())
        }
        fn warning(&self, _: usize, _: Lang) -> String {
            String::new()
        }
        fn onedrive_hidden(&self) -> bool {
            true
        }
        fn set_onedrive_hidden(&self, _: bool) -> Result<(), String> {
            Ok(())
        }
        fn set_lang(&self, _: Lang) -> bool {
            true
        }
        fn version(&self) -> String {
            "v0".into()
        }
        fn newer(&self) -> Option<String> {
            None
        }
        fn open_releases(&self) {}
        fn sign_out(&self) {}
        fn elevated(&self) -> bool {
            true
        }
    }

    fn job(steps: Vec<Step>) -> Shared {
        Arc::new(Mutex::new(Job { steps, ..Job::default() }))
    }

    #[test]
    fn a_failed_step_does_not_stop_the_others_and_each_is_told_of() {
        let be = Fake { log: Mutex::default(), fail: Some(1) };
        let j = job(vec![Step { row: 0, m: 0, install: true }, Step { row: 1, m: 1, install: true }, Step { row: 2, m: 2, install: false }]);
        let told = std::sync::atomic::AtomicUsize::new(0);
        run_job(&be, &j, &|| {
            told.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let j = j.lock().unwrap();
        assert!(j.finished && j.results.len() == 3 && j.results[1].is_err() && j.results[2].is_ok());
        assert_eq!(*be.log.lock().unwrap(), ["install 0", "install 1", "uninstall 2"]);
        assert!(told.load(std::sync::atomic::Ordering::Relaxed) >= 4, "the window is told at every step");
    }

    #[test]
    fn a_cancel_stops_after_the_step_in_hand() {
        let be = Fake { log: Mutex::default(), fail: None };
        let j = job(vec![Step { row: 0, m: 0, install: true }, Step { row: 1, m: 1, install: true }]);
        let jj = j.clone();
        // the cancel comes while the first step is being done
        run_job(&be, &j, &move || {
            let mut g = jj.lock().unwrap();
            if g.results.is_empty() && g.pos == 0 {
                g.cancel = true;
            }
        });
        let j = j.lock().unwrap();
        assert_eq!(j.results.len(), 1, "one step was done and the second was not started");
        assert!(j.finished);
    }

    #[test]
    fn a_rollback_is_one_call() {
        let be = Fake { log: Mutex::default(), fail: None };
        let j: Shared = Arc::new(Mutex::new(Job { rollback: true, ..Job::default() }));
        run_job(&be, &j, &|| {});
        assert_eq!(*be.log.lock().unwrap(), ["rollback"]);
        assert!(j.lock().unwrap().rolled.as_ref().unwrap().is_ok());
    }
}
