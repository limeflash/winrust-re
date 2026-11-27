//! The Task Manager's window driven with no window: keys and clicks through `App` on the made-up machine (`demo`), and what the fake was
//! asked. Nothing here touches a process, a service or a registry key of this computer, and no tray icon is made.
use std::sync::Arc;

use winrust_ui::{
    core::App,
    frame::{Button, Cx, Mods, Req},
};

use crate::{
    app::*,
    backend::{MSG_HOTKEY, MSG_TRAY_CMD, MSG_TRAY_SHOW},
    demo::{self, Fake},
    model::Page,
};

fn window(state: &str) -> (Tm, Arc<Fake>) {
    demo::build(state)
}

fn key(tm: &mut Tm, vk: u32) -> (bool, Cx) {
    let mut cx = Cx::default();
    let used = tm.key(vk, Mods::default(), &mut cx);
    (used, cx)
}

fn click(tm: &mut Tm, id: i32) -> Cx {
    let mut cx = Cx::default();
    tm.click(id, Button::Left, 0.0, 0.0, &mut cx);
    cx
}

fn dialogs(cx: &Cx) -> Vec<u32> {
    cx.reqs.iter().filter_map(|r| if let Req::Dialog { tag, .. } = r { Some(*tag) } else { None }).collect()
}

fn pick(tm: &mut Tm, key: &str) {
    let i = tm.rows.iter().position(|r| r.key == key).expect("the row");
    tm.select_at(i);
}

#[test]
fn descendants_collects_all_once() {
    let parents = std::collections::HashMap::from([(2, 1), (3, 2), (4, 1), (9, 8)]);
    let mut got = descendants(&parents, &[1]);
    got.sort_unstable();
    assert_eq!(got, vec![1, 2, 3, 4]);
    assert_eq!(descendants(&parents, &[9]), vec![9]);
}

#[test]
fn the_demo_machine_is_made_up_and_the_window_touches_none_of_this_computer() {
    let (tm, fake) = window("processes");
    assert!(tm.is_demo());
    assert!(!tm.d.is_admin && tm.d.etw.is_none());
    assert!(fake.changes().is_empty(), "a window that was only drawn has changed nothing: {:?}", fake.changes());
    assert!(!fake.tray.load(std::sync::atomic::Ordering::Relaxed), "no tray icon is made for a demo");
    assert!(tm.rows.len() > 10 && tm.d.procs.len() > 100);
}

#[test]
fn delete_on_a_group_asks_and_ends_nothing_until_the_red_button() {
    let (mut tm, fake) = window("processes");
    pick(&mut tm, "claude.exe");
    let (used, cx) = key(&mut tm, 0x2E);
    assert!(used);
    assert_eq!(dialogs(&cx), vec![TAG_END], "a group is asked about first");
    assert!(fake.changes().is_empty(), "nothing ended yet");
    // Cancel (button 0) ends nothing
    let mut cx = Cx::default();
    tm.command(TAG_END, 0, &mut cx);
    assert!(fake.changes().is_empty(), "Cancel ends nothing");
    // the red button (1) does, in the fake, for every process of the group
    pick(&mut tm, "claude.exe");
    key(&mut tm, 0x2E);
    let mut cx = Cx::default();
    tm.command(TAG_END, 1, &mut cx);
    let kills = fake.changes().iter().filter(|l| l.starts_with("kill ")).count();
    assert_eq!(kills, 18, "all 18 processes of the group");
}

#[test]
fn a_tree_is_ended_with_its_children_only_after_the_dialog() {
    let (mut tm, fake) = window("processes");
    pick(&mut tm, "msedge.exe");
    let mut cx = Cx::default();
    tm.do_act(Act::EndTree, &mut cx);
    assert_eq!(dialogs(&cx), vec![TAG_END]);
    assert!(fake.changes().is_empty());
    let mut cx = Cx::default();
    tm.command(TAG_END, 1, &mut cx);
    assert_eq!(fake.changes().iter().filter(|l| l.starts_with("kill ")).count(), 11);
}

#[test]
fn a_single_process_is_ended_at_once_and_the_critical_ones_never() {
    let (mut tm, fake) = window("processes");
    pick(&mut tm, "winhelper.exe");
    let (_, cx) = key(&mut tm, 0x2E);
    assert!(dialogs(&cx).is_empty(), "one process needs no question");
    assert_eq!(fake.changes().iter().filter(|l| l.starts_with("kill ")).count(), 1);
    // Registry is critical: protected, skipped, said so
    let before = fake.changes().len();
    pick(&mut tm, "registry");
    key(&mut tm, 0x2E);
    assert_eq!(fake.changes().len(), before, "a critical process is never ended");
    assert!(tm.toast.as_ref().is_some_and(|t| !t.text.is_empty()), "and the user is told");
}

#[test]
fn priority_efficiency_and_suspend_go_to_the_fake_and_nothing_else() {
    let (mut tm, fake) = window("processes");
    pick(&mut tm, "telegram.exe");
    let mut cx = Cx::default();
    tm.do_act(Act::Prio(0x40), &mut cx);
    tm.do_act(Act::Efficiency, &mut cx);
    tm.do_act(Act::Suspend(true), &mut cx);
    tm.do_act(Act::Affinity(true), &mut cx);
    let log = fake.changes();
    assert!(log.iter().any(|l| l.starts_with("priority ") && l.ends_with(" 64")), "{log:?}");
    assert!(log.iter().any(|l| l.starts_with("efficiency ") && l.ends_with(" true")));
    assert!(log.iter().any(|l| l.starts_with("suspend ") && l.ends_with(" true")));
    assert!(log.iter().any(|l| l.starts_with("affinity ") && l.ends_with(" true")));
    assert!(tm.d.suspended.len() > 1, "the window remembers what it suspended");
}

#[test]
fn services_and_startup_entries_change_through_the_fake_and_say_when_it_is_denied() {
    let (mut tm, fake) = window("services");
    let i = tm.rows.iter().position(|r| r.key == "WSearch").expect("a stopped service");
    tm.select_at(i);
    let mut cx = Cx::default();
    tm.do_act(Act::SvcStart, &mut cx);
    assert!(fake.changes().iter().any(|l| l == "service WSearch start"));
    fake.deny.store(true, std::sync::atomic::Ordering::Relaxed);
    tm.do_act(Act::SvcStop, &mut cx);
    assert!(tm.toast.as_ref().is_some_and(|t| t.text.contains("WSearch")), "an access denied is said");
    // startup: the switch of a row
    let (mut tm, fake) = window("startup");
    tm.select_at(0);
    let mut cx = Cx::default();
    tm.click(ID_SWITCH, Button::Left, 0.0, 0.0, &mut cx);
    assert!(fake.changes().iter().any(|l| l.starts_with("startup ")), "{:?}", fake.changes());
}

#[test]
fn the_search_box_filters_and_ctrl_f_focuses_it() {
    let (mut tm, _) = window("processes");
    let n = tm.rows.len();
    let mut cx = Cx::default();
    assert!(tm.key(0x46, Mods { ctrl: true, shift: false }, &mut cx), "Ctrl+F");
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Focus(id) if *id == ID_SEARCH)));
    let mut cx = Cx { focus: Some(ID_SEARCH), ..Cx::default() };
    tm.text("rust", &mut cx);
    assert!(tm.rows.len() < n && !tm.rows.is_empty(), "{} rows for 'rust'", tm.rows.len());
    assert!(tm.rows.iter().any(|r| r.key == "rustc.exe"), "rustc is among them");
    tm.key(0x1B, Mods::default(), &mut cx);
    assert_eq!(tm.rows.len(), n, "Escape clears the search");
}

#[test]
fn the_arrows_walk_the_rows_and_the_chevron_keys_open_a_group() {
    let (mut tm, _) = window("processes");
    tm.sel = None;
    key(&mut tm, 0x28);
    assert_eq!(tm.sel.as_deref(), Some(tm.rows[0].key.as_str()), "Down selects the first");
    key(&mut tm, 0x28);
    assert_eq!(tm.sel.as_deref(), Some(tm.rows[1].key.as_str()));
    key(&mut tm, 0x23);
    assert_eq!(tm.sel.as_deref(), Some(tm.rows.last().expect("rows").key.as_str()), "End");
    pick(&mut tm, "rustc.exe");
    let n = tm.rows.len();
    key(&mut tm, 0x27);
    assert_eq!(tm.rows.len(), n + 3, "Right opens the group of three");
    key(&mut tm, 0x25);
    assert_eq!(tm.rows.len(), n, "Left closes it");
}

#[test]
fn clicking_a_header_sorts_and_clicking_again_turns_the_order() {
    let (mut tm, _) = window("processes");
    assert_eq!(tm.sort_of(Page::Processes), (3, true));
    click(&mut tm, ID_PHEAD + 2); // CPU
    assert_eq!(tm.sort_of(Page::Processes), (2, true));
    assert_eq!(tm.rows[0].key, "rustc.exe", "the busiest first");
    click(&mut tm, ID_PHEAD + 2);
    assert_eq!(tm.sort_of(Page::Processes), (2, false));
    click(&mut tm, ID_PHEAD + 5); // the network column cannot be sorted
    assert_eq!(tm.sort_of(Page::Processes), (2, false));
}

#[test]
fn the_pages_of_the_sidebar_open_in_order_and_remember_the_choice() {
    let (mut tm, fake) = window("processes");
    for (i, p) in NAV_PAGES.iter().enumerate() {
        click(&mut tm, ID_NAV + i as i32);
        assert_eq!(tm.page, *p);
    }
    assert!(fake.log_of().iter().any(|l| l.starts_with("save settings view=")), "the page is saved");
}

#[test]
fn the_alert_names_the_worst_process_and_can_be_dismissed() {
    let (mut tm, fake) = window("processes");
    let pid = tm.alert_key().expect("winhelper is rated High");
    assert_eq!(tm.alert_name(pid), "winhelper.exe");
    click(&mut tm, ID_ALERT_HIDE);
    assert!(tm.alert_key().is_none());
    assert!(fake.changes().is_empty(), "dismissing touches nothing");
    let (mut tm, fake) = window("processes");
    click(&mut tm, ID_ALERT_END);
    assert_eq!(fake.changes().iter().filter(|l| l.starts_with("kill ")).count(), 1, "End task on the alert ends that one process");
}

#[test]
fn the_speed_page_switches_things_through_the_fake_and_asks_before_the_defender() {
    use crate::boost::Id as B;
    let (mut tm, fake) = window("speed");
    assert!(tm.boost_on(B::Shell) && tm.boost_on(B::MemAuto) && !tm.boost_on(B::Game));
    click(&mut tm, ID_PROF + 1); // Game
    assert!(fake.changes().iter().any(|l| l == "boost game true"));
    tm.rebuild();
    assert!(tm.boost_on(B::Game));
    click(&mut tm, ID_PROF); // Normal again
    assert!(fake.changes().iter().any(|l| l == "boost game false"));
    let before = fake.changes().len();
    let cx = click(&mut tm, ID_TWEAK + 2 * 2 + 1); // the Defender switch
    assert_eq!(dialogs(&cx), vec![TAG_BOOST], "exclusions in Defender are asked about");
    assert_eq!(fake.changes().len(), before, "and not made until it is said yes");
    let mut cx = Cx::default();
    tm.command(TAG_BOOST, 1, &mut cx);
    assert!(fake.changes().iter().any(|l| l == "boost defender true"));
}

#[test]
fn rules_come_from_the_menu_and_go_with_delete() {
    let (mut tm, _) = window("processes");
    pick(&mut tm, "winhelper.exe");
    let mut cx = Cx::default();
    tm.do_act(Act::RuleLower, &mut cx);
    assert_eq!(tm.d.rules.len(), 2);
    tm.do_act(Act::RuleLower, &mut cx);
    assert_eq!(tm.d.rules.len(), 2, "the same rule twice is said, not added");
    tm.go_page(Page::Rules);
    tm.select_at(0);
    key(&mut tm, 0x2E);
    assert_eq!(tm.d.rules.len(), 1);
}

#[test]
fn close_asks_once_then_goes_to_the_tray_or_quits_and_the_tray_commands_work() {
    let (mut tm, fake) = window("tray-question");
    let mut cx = Cx::default();
    assert!(!tm.close(&mut cx), "the first close does not close");
    assert_eq!(dialogs(&cx), vec![TAG_TRAY], "it asks: keep it in the tray, or quit?");
    // "Keep in tray" (2): the window is hidden, the program runs on
    let mut cx = Cx::default();
    tm.command(TAG_TRAY, 2, &mut cx);
    assert!(fake.log_of().iter().any(|l| l == "window: hide"));
    assert!(tm.d.cfg.to_tray && tm.d.cfg.tray_asked);
    // from now on a close hides without asking
    let mut cx = Cx::default();
    assert!(!tm.close(&mut cx) && dialogs(&cx).is_empty());
    // the icon, the hotkey: the window comes back
    let mut cx = Cx::default();
    tm.message(MSG_TRAY_SHOW, 0, 0, &mut cx);
    assert!(fake.log_of().iter().any(|l| l == "window: show"));
    tm.m.hide_window(0);
    tm.message(MSG_HOTKEY, 0, 0, &mut cx);
    assert_eq!(fake.shown.load(std::sync::atomic::Ordering::Relaxed), 2);
    // the tray menu's Exit quits for real
    tm.message(MSG_TRAY_CMD, 6, 0, &mut cx);
    assert!(tm.quitting && cx.reqs.iter().any(|r| matches!(r, Req::Close)));
    assert!(tm.close(&mut cx), "and the close that follows is let through");
    assert!(fake.log_of().iter().any(|l| l == "window: tray remove"));
    // "Quit" (1) in the first question quits the same way
    let (mut tm, _) = window("tray-question");
    let mut cx = Cx::default();
    tm.close(&mut cx);
    let mut cx = Cx::default();
    tm.command(TAG_TRAY, 1, &mut cx);
    assert!(tm.quitting && !tm.d.cfg.to_tray);
}

#[test]
fn a_tick_while_the_window_is_away_keeps_the_numbers_and_draws_nothing() {
    let (mut tm, fake) = window("processes");
    tm.m.hide_window(0);
    let mut cx = Cx::default();
    tm.wake(&mut cx);
    assert!(tm.away);
    assert!(!cx.reqs.iter().any(|r| matches!(r, Req::Invalidate)), "nothing to draw for a window nobody sees");
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Wake(ms) if *ms >= 4000)), "the tray looks every 5 s");
    assert!(fake.log_of().iter().any(|l| l == "read: refresh"));
    tm.m.show_window(0);
    let mut cx = Cx::default();
    tm.wake(&mut cx);
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Invalidate)) && cx.reqs.iter().any(|r| matches!(r, Req::Wake(1000))), "shown again: a frame a second");
}

#[test]
fn the_settings_and_the_more_menu_reach_the_fake_not_the_registry() {
    let (mut tm, fake) = window("settings");
    click(&mut tm, ID_SETSW + 2); // start with Windows
    assert!(fake.changes().iter().any(|l| l == "autostart true"));
    click(&mut tm, ID_SETBTN); // restart as administrator
    assert!(fake.changes().iter().any(|l| l == "relaunch as administrator"));
    click(&mut tm, ID_SETSW + 3); // per-process network: needs rights
    assert!(tm.toast.is_some() && fake.changes().iter().all(|l| !l.starts_with("etw")), "without rights ETW is not started");
    let (mut tm, fake) = window("processes");
    let items = tm.more_menu();
    assert!(items.iter().any(|i| i.text.contains("CSV")));
    let mut cx = Cx::default();
    tm.do_act(Act::Export, &mut cx);
    assert!(fake.changes().iter().any(|l| l.starts_with("export rtaskmgr-")), "the CSV goes to the fake's Desktop");
    tm.do_act(Act::RunTask, &mut cx);
    assert!(fake.changes().iter().any(|l| l == "run dialog"));
}

#[test]
fn a_history_deleted_is_the_programs_own_file_through_the_fake() {
    let (mut tm, fake) = window("history");
    assert!(!tm.rows.is_empty());
    let mut cx = Cx::default();
    tm.do_act(Act::ClearHistory, &mut cx);
    assert!(tm.rows.is_empty() && fake.changes().iter().any(|l| l == "clear history"));
}

#[test]
fn the_tips_of_the_flagged_say_why_on_one_line() {
    assert_eq!(crate::draw::tip_line("Why flagged:\nHigh — Unsigned\nHigh — runs from %TEMP%"), "Unsigned · runs from %TEMP%");
    assert_eq!(crate::draw::tip_line(""), "");
}

#[test]
fn every_table_page_has_columns_the_rows_fill() {
    for p in [Page::History, Page::Startup, Page::Details, Page::Services, Page::Rules] {
        let (mut tm, _) = window("processes");
        tm.go_page(p);
        let spec = table_spec(p);
        assert!(spec.iter().filter(|s| s.1 == 0.0).count() == 1, "{p:?}: one column takes what the others leave");
        let cols = tm.table_cols(p).cols.len();
        assert_eq!(cols, spec.len());
        for r in &tm.rows {
            assert!(spec.iter().all(|s| s.0 == SWITCH_COL || s.0 < r.cells.len()), "{p:?}: every column has its cell");
        }
    }
}
