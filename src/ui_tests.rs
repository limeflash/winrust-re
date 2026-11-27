//! The installer window's model and message flow, driven with no window (see `ui/`): the app is fed the calls a pointer and a keyboard
//! would make, over a backend that changes nothing (`ui_fake.rs`); a few tests draw it for real, off the screen.
#![allow(dead_code, unused_imports)]

#[path = "selection.rs"]
mod selection;
#[path = "ui/mod.rs"]
mod ui;
#[path = "ui_fake.rs"]
mod fake;
#[path = "ui_states.rs"]
mod states;

use std::sync::Arc;
use std::time::{Duration, Instant};

use fake::Fake;
use ui::app::{ID_BACK, ID_CANCEL, ID_INSTALL, ID_LANG, ID_NEWER, ID_OD, ID_REMOVE_ALL, ID_ROW, ID_SIGN_OUT, ID_UPDATE, Installer, MSG_JOB, Phase};
use ui::catalog::{ROWS, index_of};
use winrust_common::Lang;
use winrust_ui::core::{App, Core, Nc};
use winrust_ui::frame::{Button, Cx, Req, WIN_CLOSE};
use winrust_ui::gfx::{Gfx, Surface};
use winrust_ui::widgets::overlay::OvKey;

fn com() {
    // SAFETY: COM for WIC (the preview pictures) on this test's thread.
    let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
}

fn app(be: &Arc<Fake>) -> Installer {
    Installer::new(be.clone(), Lang::En)
}

fn row(id: &str) -> i32 {
    ID_ROW + index_of(id).unwrap() as i32
}

fn click(app: &mut Installer, id: i32) -> Cx {
    let mut cx = Cx::default();
    app.click(id, Button::Left, 0.0, 0.0, &mut cx);
    cx
}

fn dialogs(cx: &Cx) -> Vec<(u32, String, String)> {
    cx.reqs.iter().filter_map(|r| if let Req::Dialog { tag, dialog } = r { Some((*tag, dialog.title.clone(), dialog.text.clone())) } else { None }).collect()
}

fn toasts(cx: &Cx) -> Vec<String> {
    cx.reqs.iter().filter_map(|r| if let Req::Toast { text, .. } = r { Some(text.clone()) } else { None }).collect()
}

/// Waits for the worker to finish the job and tells the app, as the window's message would.
fn settle(app: &mut Installer) {
    let t0 = Instant::now();
    while !app.job.lock().unwrap().finished {
        assert!(t0.elapsed() < Duration::from_secs(20), "the job did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
    app.message(MSG_JOB, 0, 0, &mut Cx::default());
}

fn asked(be: &Fake) -> Vec<String> {
    be.asked()
}

#[test]
fn the_safe_set_installs_without_a_question_in_the_order_of_the_table() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    let cx = click(&mut a, ID_INSTALL);
    assert!(dialogs(&cx).is_empty(), "the recommended set is not asked about");
    assert_eq!(a.phase, Phase::Busy);
    settle(&mut a);
    assert_eq!(a.phase, Phase::Done);
    assert_eq!(asked(&be), ["install rust-search", "install rtaskmgr-win32", "install topbar", "install wiztree", "install photoview", "install rust-settings"]);
    let fin = a.fin.as_ref().unwrap();
    assert!(fin.ok() && fin.added == 6 && fin.removed == 0 && fin.chips.len() == 6);
    assert!(a.model.installed(index_of("topbar").unwrap()), "what is installed is read again");
}

#[test]
fn replacing_explorer_is_asked_about_and_a_no_installs_nothing() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    a.click(row("explorer"), Button::Left, 0.0, 0.0, &mut Cx::default());
    let cx = click(&mut a, ID_INSTALL);
    let d = dialogs(&cx);
    assert_eq!(d.len(), 1, "one question");
    assert!(d[0].2.contains("rust-explorer: The REGULAR Explorer will be DISABLED"), "the module's own warning is in it: {}", d[0].2);
    assert!(d[0].2.contains("restore point"), "and the restore point is mentioned");
    assert_eq!(a.phase, Phase::Idle, "nothing runs before the answer");
    // Cancel
    a.command(d[0].0, 0, &mut Cx::default());
    assert!(asked(&be).is_empty() && a.phase == Phase::Idle);
    // again, and agreed: the bar first, then the file manager
    let d = dialogs(&click(&mut a, ID_INSTALL));
    a.command(d[0].0, 1, &mut Cx::default());
    assert_eq!(a.phase, Phase::Busy);
    settle(&mut a);
    let log = asked(&be);
    let (t, e) = (log.iter().position(|l| l == "install topbar"), log.iter().position(|l| l == "install rust-explorer"));
    assert!(t.is_some() && e.is_some() && t < e, "the bar goes in before the file manager: {log:?}");
}

#[test]
fn purge_deletes_apps_for_good_and_nothing_runs_until_it_is_confirmed() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    a.click(row("purge"), Button::Left, 0.0, 0.0, &mut Cx::default());
    let d = dialogs(&click(&mut a, ID_INSTALL));
    assert!(d[0].1.starts_with("Remove apps for good"), "{}", d[0].1);
    assert!(asked(&be).is_empty());
    // a key (Enter) on a dangerous dialog is for the host; here only the answers count
    a.command(d[0].0, 1, &mut Cx::default());
    settle(&mut a);
    assert!(asked(&be).contains(&"install purge".to_string()));
}

#[test]
fn remove_all_is_confirmed_and_is_one_rollback() {
    let be = Arc::new(Fake::new().with(&["topbar", "rust-explorer", "photoview"]));
    let mut a = app(&be);
    assert!(dialogs(&click(&mut a, ID_REMOVE_ALL)).len() == 1);
    assert!(asked(&be).is_empty(), "the question comes first");
    a.command(1, 1, &mut Cx::default());
    settle(&mut a);
    assert_eq!(asked(&be), ["rollback"]);
    assert!(a.fin.as_ref().unwrap().rollback);
    assert!(!a.model.any_installed());
}

#[test]
fn a_failed_step_is_listed_and_the_others_still_go_in() {
    let mut f = Fake::new();
    f.fail = Some("topbar");
    let be = Arc::new(f);
    let mut a = app(&be);
    click(&mut a, ID_INSTALL);
    settle(&mut a);
    let fin = a.fin.as_ref().unwrap();
    assert!(!fin.ok() && fin.errors.len() == 1 && fin.errors[0].starts_with("topbar: "));
    assert_eq!(fin.added, 5, "the other five went in");
    assert_eq!(fin.chips.iter().filter(|c| !c.1).count(), 1);
}

#[test]
fn a_cancel_stops_after_the_step_in_hand() {
    let mut f = Fake::new();
    f.delay = Duration::from_millis(60);
    let be = Arc::new(f);
    let mut a = app(&be);
    click(&mut a, ID_INSTALL);
    // the first step is in hand: it is finished, the second is not begun
    while a.job.lock().unwrap().pos == 0 {
        std::thread::sleep(Duration::from_millis(2));
    }
    click(&mut a, ID_CANCEL);
    settle(&mut a);
    let n = asked(&be).len();
    assert!((1..6).contains(&n), "{n} of 6 steps were done");
    assert_eq!(a.fin.as_ref().unwrap().chips.len(), n);
}

#[test]
fn a_cancel_before_anything_was_done_goes_back_to_the_list() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    // a job that was stopped before its first step: the worker's end, as the window is told of it
    let steps = a.model.diff().1;
    a.phase = Phase::Busy;
    a.job = Arc::new(std::sync::Mutex::new(ui::model::Job { steps, cancel: true, finished: true, ..Default::default() }));
    a.message(MSG_JOB, 0, 0, &mut Cx::default());
    assert_eq!(a.phase, Phase::Idle, "no finished page for a job that did nothing");
    assert!(a.fin.is_none() && asked(&be).is_empty());
}

#[test]
fn the_window_does_not_close_while_it_installs() {
    let mut f = Fake::new();
    f.delay = Duration::from_millis(30);
    let be = Arc::new(f);
    let mut a = app(&be);
    click(&mut a, ID_INSTALL);
    let mut cx = Cx::default();
    assert!(!a.close(&mut cx), "closing mid-install would leave the system half changed");
    assert_eq!(toasts(&cx).len(), 1, "and the user is told why");
    settle(&mut a);
    assert!(a.close(&mut Cx::default()));
}

#[test]
fn the_switch_of_the_language_changes_the_texts_and_the_title_and_says_what_it_did() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    let cx = click(&mut a, ID_LANG + 1);
    assert_eq!(a.lang, Lang::Ru);
    assert_eq!(a.title(), "Установка WinRust");
    assert_eq!(toasts(&cx).len(), 1);
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Title(t) if t == "Установка WinRust")));
    let (label, _) = a.install_button();
    assert_eq!(label, "Установить 6 модулей");
}

#[test]
fn update_puts_in_what_is_there_again_after_one_question() {
    let be = Arc::new(Fake::new().with(&["topbar", "photoview", "purge"]));
    let mut a = app(&be);
    let d = dialogs(&click(&mut a, ID_UPDATE));
    assert_eq!(d.len(), 1);
    a.command(d[0].0, 1, &mut Cx::default());
    settle(&mut a);
    assert_eq!(asked(&be), ["install topbar", "install photoview"], "never the extras: a purge would run again");
}

#[test]
fn nothing_is_installed_by_update_on_a_clean_machine() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    assert!(dialogs(&click(&mut a, ID_UPDATE)).is_empty() && a.phase == Phase::Idle);
}

#[test]
fn signing_out_is_asked_first() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    click(&mut a, ID_INSTALL);
    settle(&mut a);
    let d = dialogs(&click(&mut a, ID_SIGN_OUT));
    assert_eq!(d.len(), 1);
    a.command(d[0].0, 0, &mut Cx::default());
    assert!(!asked(&be).contains(&"sign out".to_string()));
    let d = dialogs(&click(&mut a, ID_SIGN_OUT));
    a.command(d[0].0, 1, &mut Cx::default());
    assert!(asked(&be).contains(&"sign out".to_string()));
    // Back is the first page again, with the switches as the machine is now
    click(&mut a, ID_BACK);
    assert_eq!(a.phase, Phase::Idle);
    assert!(a.model.installed(index_of("topbar").unwrap()));
}

#[test]
fn the_onedrive_switch_writes_the_setting_and_shows_it() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    assert!(a.od);
    click(&mut a, ID_OD);
    assert!(!a.od && !ui::Backend::onedrive_hidden(&*be));
}

#[test]
fn a_row_that_cannot_be_installed_only_shows_its_preview() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    let (term, hot) = (row("term"), row("reshell"));
    click(&mut a, term);
    assert_eq!(a.peek, index_of("term").unwrap());
    let before = a.model.sel.clone();
    click(&mut a, hot);
    assert_eq!(a.model.sel, before, "the hotkeys ride on the bar and have no switch");
}

#[test]
fn the_selected_count_and_size_follow_the_switches() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    assert_eq!(a.model.selected().0, 6);
    click(&mut a, row("search"));
    assert_eq!(a.model.selected().0, 5);
    assert_eq!(a.install_button().0, "Install 5 modules");
}

#[test]
fn the_arrow_keys_walk_down_and_up_the_list_and_do_nothing_while_it_installs() {
    let be = Arc::new(Fake::new());
    let mut a = app(&be);
    a.peek = index_of("topbar").unwrap();
    let mut cx = Cx::default();
    assert!(a.key(0x28, Default::default(), &mut cx), "Down is used");
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Focus(id) if *id == row("shell"))), "the focus goes to the row below");
    let mut cx = Cx::default();
    cx.focus = Some(row("shell"));
    a.key(0x26, Default::default(), &mut cx);
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Focus(id) if *id == row("topbar"))), "and Up goes back");
    a.phase = Phase::Busy;
    assert!(!a.key(0x28, Default::default(), &mut Cx::default()));
}

/// The window drawn for real (off the screen): the items it names, and what the host asks.
struct Rig {
    core: Core,
    g: Gfx,
    surf: Surface,
    t: u64,
}

impl Rig {
    fn new(state: &str) -> Self {
        com();
        let be = Arc::new(Fake::new());
        let mut r = Self { core: Core::new(states::demo(be, state), (1000.0, 640.0), 1.0, 0), g: Gfx::new(1.0).unwrap(), surf: Surface::new(), t: 1_000_000 };
        r.frame();
        r
    }

    fn frame(&mut self) {
        let core = &mut self.core;
        let t = self.t;
        self.g.paint(&mut self.surf, 1000, 640, |c| core.draw(c, t)).unwrap();
    }

    fn pause(&mut self, ms: u64) {
        self.t += ms;
        self.frame();
    }

    fn centre(&self, id: i32) -> (f32, f32) {
        let h = self.core.hits().get(id).unwrap_or_else(|| panic!("item {id} is not on the window"));
        (h.r.cx(), h.r.cy())
    }

    fn click(&mut self, id: i32) {
        let (x, y) = self.centre(id);
        self.core.pointer_move(x, y, self.t);
        self.core.down(Button::Left, x, y, self.t);
        let act = self.core.up(Button::Left, x, y, self.t);
        assert!(act.is_none() || id == WIN_CLOSE);
        self.pause(300);
    }
}

#[test]
fn every_item_the_user_can_reach_is_on_the_window_and_the_caption_drags() {
    let r = Rig::new("");
    for id in [ID_INSTALL, ID_UPDATE, ID_REMOVE_ALL, WIN_CLOSE, ID_LANG, ID_LANG + 4, row("topbar"), row("explorer")] {
        r.centre(id);
    }
    assert!(r.core.hits().get(ID_OD).is_none(), "the OneDrive switch is under the bar's preview only");
    assert_eq!(r.core.nc_test(300.0, 28.0, true), Nc::Caption, "the empty part of the title bar drags the window");
    assert_eq!(r.core.nc_test(500.0, 2.0, true), Nc::Top);
    let (x, y) = r.centre(WIN_CLOSE);
    assert_eq!(r.core.nc_test(x, y, true), Nc::Client, "the close button is clicked with ordinary messages");
}

#[test]
fn the_remove_all_button_opens_a_modal_dialog_that_escape_closes_and_the_window_stays() {
    let mut r = Rig::new("");
    r.click(ID_REMOVE_ALL);
    assert!(r.core.ov.modal());
    assert!(r.core.key(0x1B, Default::default(), false, r.t));
    r.pause(300);
    assert!(!r.core.ov.modal());
}

#[test]
fn the_close_button_asks_the_host_to_close() {
    let mut r = Rig::new("");
    let (x, y) = r.centre(WIN_CLOSE);
    r.core.pointer_move(x, y, r.t);
    r.core.down(Button::Left, x, y, r.t);
    assert_eq!(r.core.up(Button::Left, x, y, r.t), Some(winrust_ui::core::Act::Close));
}

#[test]
fn the_wheel_scrolls_the_list_and_the_rows_move() {
    let mut r = Rig::new("");
    let (x, y) = r.centre(row("search"));
    r.core.pointer_move(x, y, r.t);
    r.core.wheel(0.0, -3.0, Default::default(), r.t);
    r.pause(400);
    let y2 = r.core.hits().get(row("search")).map(|h| h.r.y);
    assert!(y2.is_some_and(|y2| y2 < y), "the list moved up: {y2:?} against {y}");
}

#[test]
fn tab_walks_the_items_and_a_row_that_has_the_focus_shows_its_preview() {
    let mut r = Rig::new("");
    let mut seen = Vec::new();
    for _ in 0..12 {
        r.core.key(0x09, Default::default(), false, r.t);
        r.pause(50);
        seen.push(r.core.hits().focus);
    }
    assert!(seen.iter().all(Option::is_some), "Tab always lands on something");
    assert!(seen.iter().any(|f| f.is_some_and(|i| i >= ID_ROW)), "and gets to the rows");
}

#[test]
fn every_state_of_the_design_draws_in_every_theme_and_language() {
    com();
    let mut sizes = std::collections::HashSet::new();
    for state in states::STATES {
        for light in [false, true] {
            winrust_common::style::set_light(light);
            let be = Arc::new(Fake::new());
            let o = winrust_ui::render::Opts { w: 1080.0, h: 720.0, ..Default::default() };
            let (px, w, h) = winrust_ui::render::render_pixels(states::demo(be, state), &o).unwrap_or_else(|| panic!("{state} did not draw"));
            assert_eq!((w, h), (1080, 720));
            // a picture that is not blank
            assert!(px.iter().collect::<std::collections::HashSet<_>>().len() > 50, "{state} is blank");
            sizes.insert(px.iter().step_by(97).fold(0u64, |a, &p| a.wrapping_mul(31).wrapping_add(u64::from(p))));
        }
    }
    winrust_common::style::set_light(false);
    assert!(sizes.len() > states::STATES.len(), "the states and themes differ from one another");
}

#[test]
fn the_window_at_rest_asks_for_no_frames() {
    let mut r = Rig::new("");
    r.pause(2000);
    assert!(!r.core.wants_frames(r.t) && r.core.deadline(r.t).is_none(), "nothing moves, nothing is due");
}

#[test]
fn the_window_asks_for_frames_while_it_installs_and_stops_when_it_is_done() {
    let r = Rig::new("installing");
    assert!(r.core.wants_frames(r.t) || r.core.deadline(r.t).is_some(), "the spinner turns");
    let mut r = Rig::new("done");
    r.pause(2000);
    assert!(!r.core.wants_frames(r.t) && r.core.deadline(r.t).is_none(), "the finished page is still");
}
