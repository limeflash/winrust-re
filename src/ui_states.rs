//! The window in the states of the design (`--render state=<name>`, `--render-all`, `--bench`, the tests): a demo backend and the
//! switches and the job put where the state names them. Not part of the installer: only the preview and test targets include it.
use std::sync::{Arc, Mutex};

use winrust_common::Lang;
use winrust_ui::core::App;
use winrust_ui::frame::Id;

use crate::ui::app::{ID_INSTALL, ID_REMOVE_ALL, Installer, Phase};
use crate::ui::catalog::{ROWS, index_of};
use crate::ui::model::{Backend, Job, Step};

/// The mockup's "Recommended" selection (`Component.rec()`), by row id.
pub const RECOMMENDED: [&str; 8] = ["topbar", "shell", "explorer", "search", "taskmgr", "term", "settings", "photo"];

/// What `--render-all` goes through: the three steps and the previews of the design's "Preview states", the three selections, the
/// steps in every language, and a few more (what is installed, the confirmations, a failure, a newer version).
pub const STATES: [&str; 31] = [
    "choose", "installing", "done", "p-explorer", "p-term", "p-keys", "p-tree", "p-purge", "sel-rec", "sel-all", "sel-none", "choose-ru", "installing-ru", "done-ru", "choose-ja",
    "installing-ja", "done-ja", "choose-pt", "installing-pt", "done-pt", "choose-zh", "installing-zh", "done-zh", "installed", "confirm", "remove-all", "failed", "newer", "failed-ru",
    "confirm-zh", "remove-all-ja",
];

/// The state of a demo for the host's `--render` (`state=<name>`).
#[must_use]
pub fn demo(be: Arc<dyn Backend>, state: &str) -> Box<dyn App> {
    let mut s = Installer::new(be, Lang::En);
    apply_state(&mut s, state);
    Box::new(s)
}

/// The app as the design's states have it.
fn apply_state(app: &mut Installer, state: &str) {
    let (base, lang) = match state.rsplit_once('-') {
        Some((b, "en")) => (b, Some(Lang::En)),
        Some((b, "ru")) => (b, Some(Lang::Ru)),
        Some((b, "ja")) => (b, Some(Lang::Ja)),
        Some((b, "pt")) => (b, Some(Lang::Pt)),
        Some((b, "zh")) => (b, Some(Lang::Zh)),
        _ => (state, None),
    };
    if let Some(l) = lang {
        app.lang = l;
    }
    let rec = |me: &mut Installer| {
        for r in 0..ROWS.len() {
            me.model.sel[r] = RECOMMENDED.contains(&ROWS[r].id) && me.model.live(r);
        }
    };
    let queue = |me: &Installer| -> Vec<Step> {
        let mut v: Vec<Step> = (0..ROWS.len()).filter(|&r| me.model.live(r) && me.model.sel[r]).filter_map(|r| me.model.own(r).map(|m| Step { row: r, m, install: true })).collect();
        v.sort_by_key(|s| s.m);
        v
    };
    match base {
        "" => {}
        "choose" | "sel-rec" => rec(app),
        "sel-all" => {
            for r in 0..ROWS.len() {
                app.model.sel[r] = app.model.live(r);
            }
        }
        "sel-none" => app.model.sel.iter_mut().for_each(|s| *s = false),
        "p-explorer" | "p-term" | "p-keys" | "p-tree" | "p-purge" => {
            rec(app);
            app.peek = index_of(match base {
                "p-term" => "term",
                "p-keys" => "reshell",
                "p-tree" => "wiztree",
                "p-purge" => "purge",
                _ => "explorer",
            })
            .unwrap_or(0);
        }
        "installing" | "done" | "failed" => {
            rec(app);
            let steps = queue(app);
            let n = steps.len();
            let at = 3.min(n.saturating_sub(1));
            app.peek = steps.get(at).map_or(app.peek, |s| s.row);
            let mut j = Job { steps: steps.clone(), ..Job::default() };
            if base == "installing" {
                j.pos = at;
                j.results = (0..at).map(|_| Ok("installed".to_string())).collect();
                app.phase = Phase::Busy;
                app.last_pos = at;
                app.job = Arc::new(Mutex::new(j));
            } else {
                j.pos = n;
                j.finished = true;
                j.results = (0..n).map(|k| if base == "failed" && k == 2 { Err("the service could not be stopped (error 1051)".to_string()) } else { Ok("installed".to_string()) }).collect();
                app.job = Arc::new(Mutex::new(j.clone()));
                app.finish(&j);
                app.peek = steps.first().map_or(app.peek, |s| s.row);
            }
        }
        "installed" => {
            // a machine that has the bar, the file manager and the search, with the file manager switched off
            let mut mods = app.model.mods.clone();
            for m in &mut mods {
                m.installed = matches!(m.name, "topbar" | "rust-explorer" | "rust-search" | "rust-settings");
            }
            app.model.refresh(mods);
            app.model.reset_selection();
            if let Some(r) = index_of("explorer") {
                app.model.sel[r] = false;
            }
            app.peek = index_of("search").unwrap_or(0);
        }
        "confirm" => {
            rec(app);
            app.demo_clicks = vec![ID_INSTALL];
        }
        "remove-all" => {
            app.model.reset_selection();
            app.demo_clicks = vec![ID_REMOVE_ALL];
        }
        "newer" => *app.newer.lock().unwrap() = Some("v0.6.0".to_string()),
        _ => {}
    }
    if base == "confirm" {
        // the confirmation is about replacing Explorer
        if let Some(r) = index_of("explorer") {
            app.model.sel[r] = true;
        }
    }
    app.shot_fresh = true;
}
