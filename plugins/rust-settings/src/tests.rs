//! Tests of the Settings window without a window: the model, the search, and what a click on each kind of line does, through the same
//! `App` calls the host makes, on the demo source (it records every call that would change the machine, and changes nothing).
//! Nothing here reads or writes the real system: `os()` is the demo source on every thread of `cargo test`.
use super::*;
use crate::{
    more::{Cap, Flag, Gauge, Opt, Who},
    app::{ID_ARR, ID_DLG_KEEP, ID_DLG_REVERT, ID_GACT, ID_HIT, ID_IDENTIFY, ID_NAV, ID_WALL, P_ACT, P_BODY, P_CTL, P_CTL2, P_FIELD, P_MUTE, P_SEG, Settings, TAG_PICK, row_id},
    demo::{self, Demo},
};
use std::rc::Rc;
use winrust_ui::{
    anim,
    core::{App, Core},
    frame::{Button, Cx, Id, Mods, Req},
    gfx::{Gfx, Surface},
    widgets::overlay::MenuItem,
};

// ---------------------------------------------------------------------------------------------------------------------
// helpers

/// The window in a state of the demo, its lines numbered (what a frame does first), and the demo that records what it asks for.
fn win(state: &str) -> (Settings, Rc<Demo>) {
    let (mut s, d) = demo::settings(state);
    s.number_lines();
    (s, d)
}

fn id(s: &Settings, pred: impl Fn(&Line) -> bool, part: Id) -> Id {
    row_id(s.flat_of(pred).expect("the line is on the page"), part)
}

fn click(s: &mut Settings, id: Id) -> Cx {
    let mut cx = Cx::default();
    s.click(id, Button::Left, 0.0, 0.0, &mut cx);
    cx
}

/// A click on a part of the line `pred` picks.
fn press(s: &mut Settings, pred: impl Fn(&Line) -> bool, part: Id) -> Cx {
    let at = id(s, pred, part);
    click(s, at)
}

fn menu(cx: &Cx) -> Option<(u32, Vec<MenuItem>)> {
    cx.reqs.iter().find_map(|r| if let Req::Menu { tag, items, .. } = r { Some((*tag, items.clone())) } else { None })
}

fn toasts(cx: &Cx) -> Vec<String> {
    cx.reqs.iter().filter_map(|r| if let Req::Toast { text, .. } = r { Some(text.clone()) } else { None }).collect()
}

fn invalidated(cx: &Cx) -> bool {
    cx.reqs.iter().any(|r| matches!(r, Req::Invalidate))
}

/// What `choose(op, i)` asks of a fresh demo in `state`: the path every list row went through before the redesign.
fn via_choose(state: &str, op: Op, i: usize) -> Vec<String> {
    let d = Rc::new(Demo::for_state(state));
    os::set_os(d.clone());
    choose(op, i);
    d.log()
}

// ---------------------------------------------------------------------------------------------------------------------
// the model (the old tests that still apply)

#[test]
fn the_default_source_under_test_is_the_demo_one() {
    assert_eq!(os().computer_name(), "DANIIAR-PC");
    assert_eq!(os().display_layout().len(), 2);
}

#[test]
fn toggle_state_follows_value_and_default() {
    assert!(is_on(Some(0), 0, 1)); // dark theme: value 0 is "on"
    assert!(!is_on(Some(1), 0, 1));
    assert!(!is_on(None, 0, 1)); // no value: light theme
    assert!(!is_on(None, 1, 0)); // accent on title bars is off by default
}

#[test]
fn every_page_has_its_keys_and_its_title() {
    assert_eq!(pages().len(), PAGE_KEYS.len());
    assert_eq!(page_for("ms-settings:network-status"), 3);
    assert!(pages().iter().all(|p| !p.title.is_empty()));
}

#[test]
fn the_arrangement_picture_maps_both_ways_and_keeps_displays_inside() {
    let mons = [display::Mon { x: 0, y: 0, w: 1920, h: 1080, primary: true }, display::Mon { x: 1920, y: 0, w: 1280, h: 1024, primary: false }];
    let bx = Box4 { left: 100, top: 20, right: 500, bottom: 180 };
    let m = arr_map(&mons, bx);
    let (x0, y0) = m.dev(0, 0);
    let (x1, y1) = m.dev(3200, 1080);
    assert!(x0 >= bx.left && y0 >= bx.top && x1 <= bx.right && y1 <= bx.bottom);
    let (bx2, by2) = m.dev(1920, 500);
    assert!((m.desk(bx2, by2).0 - 1920).abs() <= 8 && (m.desk(bx2, by2).1 - 500).abs() <= 8); // round trip within a few pixels
}

#[test]
fn slider_maps_position_to_whole_percent() {
    assert_eq!(slider_value(100, 100, 300), 0.0);
    assert_eq!(slider_value(200, 100, 300), 0.5);
    assert_eq!(slider_value(900, 100, 300), 1.0); // dragged past the end
    assert_eq!(slider_value(-5, 100, 300), 0.0);
}

#[test]
fn names_and_pages() {
    assert_eq!(os_name("Windows 10 Pro", 26100), "Windows 11 Pro");
    assert_eq!(os_name("Windows 10 Pro", 19045), "Windows 10 Pro");
    assert_eq!(gb(16 << 30), "16.0 GB");
    assert_eq!((page_for("ms-settings:about"), page_for("more"), page_for("ms-settings:display"), page_for("")), (ABOUT_PAGE, 0, 0, 0));
    assert_eq!((page_for("ms-settings:notifications"), page_for("ms-settings:privacy-microphone"), page_for("ms-settings:keyboard"), page_for("ms-settings:dateandtime")), (NOTIFY_PAGE, PRIVACY_PAGE, KEYBOARD_PAGE, TIME_PAGE));
    assert_eq!((page_for("ms-settings:sound"), page_for("ms-settings:personalization-colors")), (1, 7));
    assert_eq!((page_for("bluetooth"), page_for("apps"), page_for("update"), page_for("network")), (BT_PAGE, APPS_PAGE, WU_PAGE, NET_PAGE));
    assert_eq!((page_for("ms-settings:powersleep"), page_for("ms-settings:storagesense")), (2, 2));
}

#[test]
fn the_network_card_says_how_the_pc_is_connected_or_what_to_do() {
    // the demo's adapters: [0] Wi-Fi, [1] Ethernet
    let set = |wifi_up: bool, cable_up: bool| {
        let mut a = os().net_adapters();
        (a[0].up, a[0].internet) = (wifi_up, wifi_up.then_some(true));
        (a[1].up, a[1].internet) = (cable_up, cable_up.then_some(true));
        a
    };
    let h = network_hero(&set(true, true), Some((true, true)), None, Some(false));
    assert_eq!(h.tone, Tone::Good);
    assert!(h.title.contains("Ethernet") || h.title.contains("Ethernet"), "the cable wins over Wi-Fi");
    assert!(h.act.is_none());
    let h = network_hero(&set(false, false), Some((false, true)), None, Some(false));
    assert_eq!((h.tone, h.act.map(|a| a.1)), (Tone::Bad, Some(Op::Radio)), "Wi-Fi is off: the card offers to turn it on");
    let h = network_hero(&set(false, false), Some((false, true)), None, Some(true));
    assert_eq!(h.act.map(|a| a.1), Some(Op::Air), "airplane mode: the card offers to turn that off");
    let h = network_hero(&set(false, false)[1..], None, None, Some(false));
    assert!(h.act.is_none(), "no Wi-Fi adapter: nothing to turn on");
}

#[test]
fn the_battery_card_words() {
    let on = |pct, ac, charging, left| os::Battery { pct, ac, charging, left };
    let (label, sub, low) = draw::battery_words(&on(14, false, false, Some(38 * 60)));
    assert_eq!(label, "14%");
    assert!(low && sub.contains("38"), "{sub}");
    let (_, sub, low) = draw::battery_words(&on(64, false, false, Some(4 * 3600 + 20 * 60)));
    assert!(!low && sub.contains('4') && sub.contains("20"), "{sub}");
    let (_, _, low) = draw::battery_words(&on(14, true, true, None));
    assert!(!low, "plugged in is never low");
}

// ---------------------------------------------------------------------------------------------------------------------
// the search

#[test]
fn search_finds_lines_by_label_value_and_section_case_insensitively() {
    let pgs = pages();
    // by label
    let f = search::search(&pgs, "BRIGHT"); // only the HDR line says "Brighter highlights"
    assert!(f.is_empty() || f.iter().all(|f| f.page == 0), "{f:?}"); // (the built-in display has no HDR line: nothing, or only Display)
    // by value: the About page holds the computer name as a value
    let f = search::search(&pgs, "daniiar-pc");
    assert!(f.iter().any(|f| f.page == ABOUT_PAGE), "{f:?}");
    // by section title: every line of "Volume per app"
    let title = pgs[1].groups[2].title.to_lowercase();
    let f = search::search(&pgs, &title);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].page, 1);
    assert!(f[0].hits.iter().all(|h| h.sub.ends_with(&pgs[1].groups[2].title)), "{:?}", f[0].hits);
    // case does not matter, and neither does the space round the word
    assert_eq!(search::search(&pgs, "  PAGE "), search::search(&pgs, "page"));
}

#[test]
fn search_groups_by_page_in_the_sidebars_order_and_caps_a_page_at_six() {
    let pgs = pages();
    let f = search::search(&pgs, "e");
    let order: Vec<usize> = f.iter().map(|f| f.page).collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted, "pages come in the sidebar's order");
    assert!(f.iter().all(|f| f.hits.len() <= search::PER_PAGE), "at most six lines for a page");
    // twelve programs, six shown
    let apps = search::search(&pgs, "1");
    let a = apps.iter().find(|f| f.page == APPS_PAGE).expect("the versions match");
    assert_eq!(a.hits.len(), 6);
    // an empty or blank query finds nothing
    assert!(search::search(&pgs, "").is_empty() && search::search(&pgs, "   ").is_empty());
    assert!(search::search(&[], "e").is_empty());
}

#[test]
fn a_page_whose_title_matches_and_has_no_line_hits_shows_itself_as_a_page() {
    let pgs = pages();
    // the Windows Update page's title is "Центр обновления" under test (Russian): search for its own words
    let title = pgs[WU_PAGE].title.to_lowercase();
    let word = title.split_whitespace().next().unwrap().to_string();
    let f = search::search(&pgs, &word);
    let own = f.iter().find(|f| f.page == WU_PAGE);
    assert!(own.is_some(), "{word}: {f:?}");
    // a page that matches by title only: Personalization (no line says it)
    let t = pgs[7].title.to_lowercase();
    let f = search::search(&pgs, &t);
    let hit = f.iter().find(|f| f.page == 7).and_then(|f| f.hits.first()).expect("the page itself");
    assert_eq!((hit.key.as_deref(), hit.sub.as_str()), (None, search::page_word()));
    assert!(search::search(&pgs, "zzzq").is_empty(), "nothing matches: no groups");
}

#[test]
fn a_hit_names_the_line_to_flash_and_where_it_is() {
    let pgs = pages();
    let want = pgs[0].groups.iter().flat_map(|g| &g.rows).find(|l| matches!(&l.row, Row::Cycle(_, _, Op::Hz))).unwrap();
    let f = search::search(&pgs, &want.row.label().to_lowercase());
    let h = &f[0].hits[0];
    assert_eq!((h.page, h.key.as_deref()), (0, Some(want.key.as_str())));
    assert!(h.sub.contains('\u{203a}'), "page \u{203a} section: {}", h.sub);
}

#[test]
fn typing_in_the_box_searches_and_a_click_on_a_hit_opens_the_page_and_flashes_the_line() {
    let (mut s, _d) = win("display");
    let mut cx = Cx { focus: Some(app::ID_SEARCH), ..Cx::default() };
    let word = page(0).groups.iter().flat_map(|g| &g.rows).find(|l| matches!(&l.row, Row::Cycle(_, _, Op::Hz))).unwrap().row.label().to_lowercase();
    s.text(&word, &mut cx);
    assert!(!s.found.is_empty(), "the text of the box was searched");
    click(&mut s, ID_NAV + 3); // another page, to see the hit bring us back
    s.text(&word, &mut cx);
    click(&mut s, ID_HIT);
    assert_eq!(s.page, 0);
    assert!(s.q.text.is_empty(), "the box is cleared");
    assert!(s.flash.is_some() && s.reveal.is_some(), "the line flashes and is scrolled to");
    // Esc in the box clears it
    s.text("zzzq", &mut cx);
    assert!(s.found.is_empty() && !s.q.text.is_empty());
    assert!(s.key(0x1B, Mods::default(), &mut cx), "Esc clears the box");
    assert!(s.q.text.is_empty());
}

#[test]
fn enter_in_the_box_opens_the_first_hit() {
    let (mut s, _d) = win("display");
    let mut cx = Cx { focus: Some(app::ID_SEARCH), ..Cx::default() };
    let word = page(BT_PAGE).groups.iter().flat_map(|g| &g.rows).find(|l| matches!(&l.row, Row::Switch(_, _, Op::Air))).unwrap().row.label().to_lowercase();
    s.text(&word, &mut cx);
    assert!(s.key(0x0D, Mods::default(), &mut cx));
    assert_eq!(s.page, BT_PAGE);
}

// ---------------------------------------------------------------------------------------------------------------------
// a click on each kind of line reaches what it reached before

#[test]
fn every_list_row_opens_the_same_list_and_a_choice_reaches_choose() {
    // (state, the op, the index to choose)
    let cases = [
        ("display", Op::Res, 3),
        ("display", Op::Hz, 1),
        ("display", Op::Scale, 3),
        ("display", Op::Orient, 1),
        ("power", Op::Screen, 4),
        ("power", Op::Sleep, 1),
        ("power-plans-many", Op::Plan, 2),
        ("sound", Op::OutDev, 1),
        ("sound", Op::InDev, 1),
        ("personal", Op::UiLang, 2), // (under test the language is Russian: Japanese is another one)
    ];
    for (state, op, i) in cases {
        let (mut s, d) = win(state);
        let at = id(&s, |l| matches!(&l.row, Row::Cycle(_, _, o) if *o == op), P_CTL);
        let cx = click(&mut s, at);
        let (tag, items) = menu(&cx).unwrap_or_else(|| panic!("{op:?} opens a list"));
        assert_eq!(tag, TAG_PICK);
        let (names, cur) = choices(op).expect("values");
        assert_eq!(items.iter().map(|m| m.text.clone()).collect::<Vec<_>>(), names, "{op:?}: the list is the one of choices()");
        assert_eq!(items.iter().position(|m| m.radio == Some(true)), cur, "{op:?}: the current value is ticked");
        s.command(TAG_PICK, i, &mut Cx::default());
        assert_eq!(d.log(), via_choose(state, op, i), "{op:?}: the same calls as choose()");
        assert!(!d.log().is_empty(), "{op:?} changed something");
    }
}

#[test]
fn the_segmented_rows_choose_the_same_as_before() {
    for (state, op, i) in [("display", Op::Topology, 1), ("power", Op::Plan, 2)] {
        let (mut s, d) = win(state);
        let at = id(&s, |l| matches!(&l.row, Row::Cycle(_, _, o) if *o == op), P_SEG + i as Id);
        click(&mut s, at);
        assert_eq!(d.log(), via_choose(state, op, i), "{op:?}");
        assert!(!d.log().is_empty());
    }
}

#[test]
fn a_changed_display_mode_asks_to_be_kept_and_goes_back_on_its_own_after_fifteen_seconds() {
    let (mut s, d) = win("display");
    let at = id(&s, |l| matches!(&l.row, Row::Cycle(_, _, Op::Res)), P_CTL);
    click(&mut s, at);
    s.command(TAG_PICK, 3, &mut Cx::default()); // 1920 x 1200
    assert_eq!(CONFIRM_SECS, 15);
    assert_eq!(s.confirm.as_ref().map(|c| c.left), Some(CONFIRM_SECS), "the countdown starts at 15");
    let before = d.log();
    assert!(before.iter().any(|l| l.ends_with("save=false")), "the mode is temporary: {before:?}");
    // a tick a second: 14 more seconds count down, nothing is decided yet
    let t0 = anim::now_ms();
    for k in 1..=14u64 {
        s.tick(t0 + 1000 * k + 5, &mut Cx::default());
        assert_eq!(s.confirm.as_ref().map(|c| c.left), Some(CONFIRM_SECS - k as u32));
    }
    assert!(!d.log().iter().any(|l| l == "display_revert"));
    // the fifteenth: it goes back by itself
    s.tick(t0 + 15_000 + 5, &mut Cx::default());
    assert!(s.confirm.is_none());
    assert_eq!(d.log().last().map(String::as_str), Some("display_revert"));
    assert_eq!(os().display_current().map(|m| (m.w, m.h)), Some((2560, 1600)), "the old mode is back");
}

#[test]
fn enter_keeps_the_display_mode_and_esc_goes_back_and_the_dialogs_buttons_do_the_same() {
    for (how, keep) in [("enter", true), ("esc", false), ("button keep", true), ("button revert", false)] {
        let (mut s, d) = win("display");
        let at = id(&s, |l| matches!(&l.row, Row::Cycle(_, _, Op::Hz)), P_CTL);
        click(&mut s, at);
        s.command(TAG_PICK, 1, &mut Cx::default()); // 120 Hz
        assert!(s.confirm.is_some(), "{how}");
        d.clear_log();
        let mut cx = Cx::default();
        match how {
            "enter" => assert!(s.key(0x0D, Mods::default(), &mut cx)),
            "esc" => assert!(s.key(0x1B, Mods::default(), &mut cx)),
            "button keep" => s.click(ID_DLG_KEEP, Button::Left, 0.0, 0.0, &mut cx),
            _ => s.click(ID_DLG_REVERT, Button::Left, 0.0, 0.0, &mut cx),
        }
        assert!(s.confirm.is_none(), "{how}");
        let log = d.log();
        if keep {
            assert_eq!(log, vec!["display_apply 2560x1600@120 save=true"], "{how}: the current mode is saved");
        } else {
            assert_eq!(log, vec!["display_revert"], "{how}");
        }
    }
}

#[test]
fn while_the_dialog_is_up_other_keys_do_nothing() {
    let (mut s, d) = win("dialog-keep-display");
    assert!(s.confirm.is_some());
    assert!(s.key(0x28, Mods::default(), &mut Cx::default()), "Down is taken");
    assert_eq!(s.page, 0);
    assert!(d.log().is_empty());
}

#[test]
fn switches_and_buttons_do_what_display_or_mute_did() {
    type Pred = fn(&Line) -> bool;
    let cases: [(&str, Pred, &str); 7] = [
        ("sound", |l| matches!(&l.row, Row::Slider(_, audio::Src::Mic)), "audio_set_mute mic=true true"),
        ("bluetooth", |l| matches!(&l.row, Row::Switch(_, _, Op::Bt)), "bt_set_bluetooth false"),
        ("bluetooth", |l| matches!(&l.row, Row::Switch(_, _, Op::Air)), "bt_set_airplane true"),
        ("network", |l| matches!(&l.row, Row::Switch(_, _, Op::Radio)), "wifi_set_radio false"),
        ("display", |l| matches!(&l.row, Row::Switch(_, _, Op::Night)), "night_set 5500"),
        ("display", |l| matches!(&l.row, Row::Cycle(_, _, Op::Primary)), "display_make_primary"),
        ("sound", |l| matches!(&l.row, Row::Slider(_, audio::Src::Out)), "audio_set_mute mic=false true"),
    ];
    for (state, pred, expect) in cases {
        let (mut s, d) = win(state);
        let part = if state == "sound" { P_MUTE } else { P_CTL };
        // the Primary line of a display that is the main one has no button: pick the second display first
        if expect == "display_make_primary" {
            os().display_select(1);
            s.reread();
            s.number_lines();
            d.clear_log();
        }
        let at = id(&s, pred, part);
        click(&mut s, at);
        assert_eq!(d.log().last().map(String::as_str), Some(expect), "{state}: {expect}");
    }
}

#[test]
fn hdr_switches_on_a_display_that_can_and_has_no_line_on_one_that_cannot() {
    let (s, _d) = win("display");
    assert!(s.flat_of(|l| matches!(&l.row, Row::Switch(_, _, Op::Hdr))).is_none(), "the built-in display has no HDR");
    let (mut s, d) = win("display");
    os().display_select(1);
    s.reread();
    s.number_lines();
    d.clear_log();
    let at = id(&s, |l| matches!(&l.row, Row::Switch(_, _, Op::Hdr)), P_CTL);
    click(&mut s, at);
    assert_eq!(d.log().last().map(String::as_str), Some("display_set_hdr false"));
}

#[test]
fn registry_toggles_write_the_same_values_and_tell_who_must_know() {
    let (mut s, d) = win("personal");
    let at = id(&s, |l| matches!(&l.row, Row::Toggle { name: "AppsUseLightTheme", .. }), P_CTL);
    click(&mut s, at);
    assert_eq!(d.log(), vec!["reg_set_dword AppsUseLightTheme 1", "announce_theme"], "dark theme off: the value 1 (light), and the system is told");
    // the accent on title bars: off by default, a click turns it on; no broadcast
    d.clear_log();
    let at = id(&s, |l| matches!(&l.row, Row::Toggle { name: "ColorPrevalence", .. }), P_CTL);
    click(&mut s, at);
    assert_eq!(d.log(), vec!["reg_set_dword ColorPrevalence 1"]);
    // Storage Sense (a value of 1 is "on"): a click writes 0
    let (mut s, d) = win("power");
    let at = id(&s, |l| matches!(&l.row, Row::Toggle { name: "01", .. }), P_CTL);
    click(&mut s, at);
    assert_eq!(d.log(), vec!["reg_set_dword 01 0"]);
    // the proxy: written, then the running programs are told
    let (mut s, d) = win("network");
    let at = id(&s, |l| matches!(&l.row, Row::Toggle { name: "ProxyEnable", .. }), P_CTL);
    click(&mut s, at);
    assert_eq!(d.log(), vec!["reg_set_dword ProxyEnable 1", "proxy_changed"]);
}

#[test]
fn a_slider_sets_the_volume_where_it_is_pressed_and_dragged() {
    let (mut s, d) = win("sound");
    let flat = s.flat_of(|l| matches!(&l.row, Row::Slider(_, audio::Src::Out))).unwrap();
    s.tracks.push((flat, 100.0, 400.0)); // what a frame names
    let at = row_id(flat, P_CTL);
    let mut cx = Cx::default();
    s.down(at, Button::Left, 250.0, 0.0, &mut cx);
    assert_eq!(d.log().last().map(String::as_str), Some("audio_set_volume Out 0.50"));
    s.drag(at, 391.0, 0.0, &mut cx);
    assert_eq!(d.log().last().map(String::as_str), Some("audio_set_volume Out 1.00"), "dragged to the end");
    s.drag(at, -50.0, 0.0, &mut cx);
    assert_eq!(d.log().last().map(String::as_str), Some("audio_set_volume Out 0.00"), "past the start");
    // an app's slider is its own source
    let flat = s.flat_of(|l| matches!(&l.row, Row::Slider(_, audio::Src::App(102)))).unwrap();
    s.tracks.push((flat, 100.0, 400.0));
    s.down(row_id(flat, P_CTL), Button::Left, 109.0, 0.0, &mut cx);
    assert_eq!(d.log().last().map(String::as_str), Some("audio_set_volume App(102) 0.00"));
}

#[test]
fn the_night_light_strength_is_four_steps_that_reach_choose() {
    let (mut s, d) = win("display");
    os().night_set(4800);
    d.clear_log();
    s.reread();
    s.number_lines();
    let flat = s.flat_of(|l| matches!(&l.row, Row::Cycle(_, _, Op::Night))).expect("the strength shows while it is on");
    s.tracks.push((flat, 100.0, 400.0));
    s.down(row_id(flat, P_CTL), Button::Left, 391.0, 0.0, &mut Cx::default());
    assert_eq!(d.log(), via_choose("display", Op::Night, 4), "the right end is Maximum, entry 4 of choices()");
}

#[test]
fn the_buttons_of_item_rows_run_the_same_commands() {
    type Pred = fn(&Line) -> bool;
    let cases: [(&str, Pred, &str); 7] = [
        ("network", |l| matches!(&l.row, Row::Item(_, _, Cmd::Vpn(_))), "vpn_dial Work VPN"),
        ("power", |l| matches!(&l.row, Row::Item(_, _, Cmd::CleanTemp)), "temp_clean"),
        ("power", |l| matches!(&l.row, Row::Item(_, _, Cmd::EmptyBin)), "bin_empty"),
        ("bt-searching", |l| matches!(&l.row, Row::Item(_, _, Cmd::BtPair(_))), "bt_pair px"),
        ("wu-up-to-date", |l| matches!(&l.row, Row::Item(_, _, Cmd::Check)), "wu_check"),
        ("wu-available", |l| matches!(&l.row, Row::Item(_, _, Cmd::Install)), "wu_install"),
        ("update", |l| matches!(&l.row, Row::Item(_, _, Cmd::Check)), "wu_check"),
    ];
    for (state, pred, expect) in cases {
        let (mut s, d) = win(state);
        let at = id(&s, pred, P_CTL);
        click(&mut s, at);
        assert_eq!(d.log().last().map(String::as_str), Some(expect), "{state}");
    }
    // a paired device has no button of its own: it unfolds (below)
    let (mut s, d) = win("bt-paired");
    let at = id(&s, |l| matches!(&l.row, Row::Item(_, _, Cmd::BtUnpair(_))), P_CTL);
    click(&mut s, at);
    assert!(d.log().is_empty());
}

#[test]
fn an_adapter_unfolds_its_details_on_a_click() {
    let (mut s, _d) = win("network");
    let eth = |l: &Line| matches!(&l.row, Row::Adapter(a, _) if !a.wifi);
    let l = s.pg.groups.iter().flat_map(|g| &g.rows).find(|l| eth(l)).unwrap();
    assert!(s.expansion(l).is_empty(), "folded at first");
    press(&mut s, eth, P_BODY_FOR_TEST);
    let l = s.pg.groups.iter().flat_map(|g| &g.rows).find(|l| eth(l)).unwrap();
    assert!(matches!(&l.row, Row::Adapter(_, true)));
    assert!(s.expansion(l).iter().any(|e| matches!(e, draw::ExpItem::Fact(..))), "its facts are there");
    press(&mut s, eth, P_BODY_FOR_TEST);
    let l = s.pg.groups.iter().flat_map(|g| &g.rows).find(|l| eth(l)).unwrap();
    assert!(matches!(&l.row, Row::Adapter(_, false)), "a second click folds it");
}

#[test]
fn a_connected_vpn_hangs_up() {
    let (mut s, d) = win("network");
    let at = id(&s, |l| matches!(&l.row, Row::Item(_, _, Cmd::Vpn(_))), P_CTL);
    click(&mut s, at);
    s.number_lines();
    let at = id(&s, |l| matches!(&l.row, Row::Item(_, _, Cmd::Vpn(v)) if v.on), P_CTL);
    click(&mut s, at);
    assert_eq!(d.log().last().map(String::as_str), Some("vpn_hang_up Work VPN"));
}

#[test]
fn the_buttons_of_the_group_titles_and_the_picture_run_their_commands() {
    // "Find devices" is a Cmd::BtScan
    let (mut s, d) = win("bt-paired");
    let gi = s.pg.groups.iter().position(|g| g.act.as_ref().is_some_and(|a| matches!(a.what, GAct::Cmd(Cmd::BtScan)))).expect("the nearby group has the button");
    click(&mut s, ID_GACT + gi as Id);
    assert_eq!(d.log(), vec!["bt_scan"]);
    // "Refresh" looks for Wi-Fi again
    let (mut s, d) = win("network");
    let gi = s.pg.groups.iter().position(|g| g.act.as_ref().is_some_and(|a| matches!(a.what, GAct::WifiRefresh))).unwrap();
    click(&mut s, ID_GACT + gi as Id);
    assert_eq!(d.log(), vec!["wifi_scan"]);
    // "Identify" asks for the numbers of the displays
    let (mut s, d) = win("display");
    click(&mut s, ID_IDENTIFY);
    assert_eq!(d.log(), vec!["identify 2"]);
    // "Choose..." of the backgrounds is the picker, and a picture of the demo is a pick of the demo
    let (mut s, d) = win("personal");
    click(&mut s, ID_WALL + 5);
    click(&mut s, ID_WALL + 2);
    assert_eq!(d.log(), vec!["wallpaper_pick", "wallpaper_select 2"]);
    // "Copy" puts the device details on the clipboard (the window's request; the demo has no clipboard to touch)
    let (mut s, _d) = win("about");
    let gi = s.pg.groups.iter().position(|g| g.act.as_ref().is_some_and(|a| matches!(a.what, GAct::Copy))).unwrap();
    let cx = click(&mut s, ID_GACT + gi as Id);
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Clipboard(t) if t.contains("DANIIAR-PC"))));
}

#[test]
fn a_destructive_row_needs_a_second_click_on_the_same_row() {
    // uninstalling a program
    let (mut s, d) = win("apps");
    let first = id(&s, |l| l.row.label() == "7-Zip", P_CTL);
    let second = id(&s, |l| l.row.label() == "Claude", P_CTL);
    click(&mut s, first);
    assert!(d.log().is_empty(), "the first click only arms it");
    click(&mut s, second);
    assert!(d.log().is_empty(), "another row's click arms that one instead");
    click(&mut s, second);
    assert_eq!(d.log(), vec!["uninstall demo-uninstall Claude"]);
    click(&mut s, first);
    assert_eq!(d.log().len(), 1, "it was disarmed by the other click");
    // removing a paired device
    let (mut s, d) = win("bt-paired");
    let row = id(&s, |l| matches!(&l.row, Row::Item(_, _, Cmd::BtUnpair(_))), P_BODY_FOR_TEST);
    click(&mut s, row); // unfolds it
    s.number_lines();
    let remove = id(&s, |l| matches!(&l.row, Row::Item(_, _, Cmd::BtUnpair(_))), P_ACT);
    click(&mut s, remove);
    assert!(d.log().is_empty(), "first click arms");
    click(&mut s, remove);
    assert_eq!(d.log(), vec!["bt_unpair xm5"]);
}

#[test]
fn the_apps_page_has_a_search_box_of_its_own() {
    let (mut s, d) = win("apps");
    let names = |s: &Settings| s.pg.groups.iter().flat_map(|g| g.rows.iter().map(|l| l.row.label().to_string())).collect::<Vec<_>>();
    assert_eq!(names(&s).len(), 12);
    let count = |s: &Settings| s.pg.groups.iter().find_map(|g| if let Extra::Search(c) = &g.extra { Some(c.clone()) } else { None }).expect("the box is on the page");
    assert_eq!(count(&s), "", "nothing typed: no count");
    // the name or the publisher, any case
    let mut cx = Cx { focus: Some(app::ID_AQ), ..Cx::default() };
    s.text("MICRO", &mut cx);
    assert_eq!(names(&s), ["Microsoft Edge", "Microsoft Outlook", "Visual Studio Code"], "Visual Studio Code is by Microsoft Corporation");
    assert!(count(&s).starts_with('3') && count(&s).ends_with("12"), "3 of 12: {}", count(&s));
    s.number_lines();
    assert!(invalidated(&cx));
    // the title still says how many there are in all, and the sidebar's search does not see the filter
    assert!(s.pg.groups[0].title.contains("12"));
    assert_eq!(pages()[APPS_PAGE].groups.iter().map(|g| g.rows.len()).sum::<usize>(), 12);
    // a click on Uninstall arms the row; typing disarms it, and the second click does not uninstall what was armed before
    let edge = id(&s, |l| l.row.label() == "Microsoft Edge", P_CTL);
    click(&mut s, edge);
    assert!(s.armed.is_some());
    s.text("o", &mut cx);
    assert!(s.armed.is_none() && d.log().is_empty());
    // nothing matches: the line says so
    s.aq.set_text("");
    s.text("photoshop", &mut cx);
    assert!(names(&s).is_empty());
    assert!(s.pg.groups.iter().any(|g| g.empty.contains("photoshop")), "the empty line names the query");
    // Esc empties the box first; with it empty Esc is the window's
    assert!(s.key(0x1B, Mods::default(), &mut cx));
    assert!(s.aq.text.is_empty() && names(&s).len() == 12 && count(&s).is_empty());
    assert!(!s.key(0x1B, Mods::default(), &mut cx), "an empty box lets Esc through");
    // the clear button
    s.text("git", &mut cx);
    assert_eq!(names(&s), ["Git"]);
    click(&mut s, app::ID_AQ + 1);
    assert!(s.aq.text.is_empty() && names(&s).len() == 12);
}

const P_BODY_FOR_TEST: Id = 0;

#[test]
fn wifi_a_saved_network_connects_and_forgets_and_a_connected_one_disconnects() {
    let (mut s, d) = win("network");
    let home24 = |l: &Line| matches!(&l.row, Row::Wifi(n, _) if n.ssid == "Home-2.4G");
    press(&mut s, home24, P_BODY_FOR_TEST); // unfold
    s.number_lines();
    press(&mut s, home24, P_ACT); // Connect
    assert_eq!(d.log(), vec!["wifi_connect Home-2.4G"]);
    press(&mut s, home24, P_ACT + 1); // Forget
    assert_eq!(d.log().last().map(String::as_str), Some("wifi_forget Home-2.4G"));
    let home5 = |l: &Line| matches!(&l.row, Row::Wifi(n, _) if n.ssid == "Home-5G");
    press(&mut s, home5, P_BODY_FOR_TEST);
    s.number_lines();
    press(&mut s, home5, P_ACT);
    assert_eq!(d.log().last().map(String::as_str), Some("wifi_disconnect"));
}

#[test]
fn a_right_click_on_a_saved_network_unfolds_it_and_on_an_unsaved_one_does_not() {
    let (mut s, _d) = win("network");
    let right = |s: &mut Settings, ssid: &str| {
        let ssid = ssid.to_string();
        let at = id(s, |l| matches!(&l.row, Row::Wifi(n, _) if n.ssid == ssid), P_BODY_FOR_TEST);
        s.click(at, Button::Right, 0.0, 0.0, &mut Cx::default());
    };
    right(&mut s, "Home-2.4G");
    assert_eq!(s.open.as_deref(), Some("wifi:Home-2.4G"));
    right(&mut s, "Home-2.4G");
    assert_eq!(s.open.as_deref(), Some("wifi:Home-2.4G"), "a second right click keeps it open");
    s.open = None;
    right(&mut s, "Kepez_Free");
    assert!(s.open.is_none(), "nothing to forget or disconnect: a right click does nothing");
}

#[test]
fn wifi_a_password_goes_into_the_profile_and_nowhere_else() {
    let (mut s, d) = win("network");
    let net = |l: &Line| matches!(&l.row, Row::Wifi(n, _) if n.ssid == "TurkTelekom_ZT4F2");
    press(&mut s, net, P_BODY_FOR_TEST);
    s.number_lines();
    let field = id(&s, net, P_FIELD);
    // a short password is refused by the profile code (8 to 63), and the field keeps it for another try
    let mut cx = Cx { focus: Some(field), ..Cx::default() };
    s.text("short", &mut cx);
    press(&mut s, net, P_ACT);
    assert!(!s.field_err.is_empty(), "the reason is shown under the field");
    assert_eq!(s.fields.pass.text, "short");
    s.text("-and-long-enough", &mut cx);
    let secret = s.fields.pass.text.clone();
    press(&mut s, net, P_ACT);
    let log = d.log();
    assert!(log.last().is_some_and(|l| l.starts_with("wifi_add_and_connect TurkTelekom_ZT4F2") && l.contains("key_len=21")), "{log:?}");
    assert!(log.iter().all(|l| !l.contains(&secret)), "the password is not in what was asked of the system, only its length");
    assert!(s.fields.pass.text.is_empty(), "it is wiped from the field once it has gone into the profile");
    assert!(s.field_err.is_empty());
    assert!(s.connecting.is_some(), "the connection is waited for");
}

#[test]
fn wifi_an_open_network_connects_with_no_password_and_an_enterprise_one_asks_for_a_name() {
    let (mut s, d) = win("network");
    let open = |l: &Line| matches!(&l.row, Row::Wifi(n, _) if n.ssid == "Kepez_Free");
    press(&mut s, open, P_BODY_FOR_TEST);
    s.number_lines();
    assert!(s.fields_of(s.flat_of(open).unwrap()).is_empty(), "no field for an open network");
    press(&mut s, open, P_ACT);
    assert!(d.log().last().unwrap().starts_with("wifi_add_and_connect Kepez_Free Open key_len=0"));
    // 802.1X: a user name first
    let (mut s, d) = win("network");
    let corp = |l: &Line| matches!(&l.row, Row::Wifi(n, _) if n.ssid == "Office-Corp");
    press(&mut s, corp, P_BODY_FOR_TEST);
    s.number_lines();
    press(&mut s, corp, P_ACT);
    assert!(d.log().is_empty() && !s.field_err.is_empty(), "no name: nothing is asked of the system");
    let flat = s.flat_of(corp).unwrap();
    let mut cx = Cx { focus: Some(row_id(flat, P_FIELD)), ..Cx::default() };
    s.text("corp\\anna", &mut cx);
    cx.focus = Some(row_id(flat, P_FIELD + 1));
    s.text("p4ss", &mut cx);
    press(&mut s, corp, P_ACT);
    assert_eq!(d.log(), vec!["wifi_add_eap_and_connect Office-Corp WPA2 user=corp\\anna pass_len=4 hidden=false"]);
    assert!(s.fields.pass.text.is_empty());
}

#[test]
fn wifi_a_hidden_network_takes_its_name_and_an_empty_password_means_open() {
    let (mut s, d) = win("network");
    let other = |l: &Line| l.key == "wifi:other";
    press(&mut s, other, P_BODY_FOR_TEST);
    s.number_lines();
    let flat = s.flat_of(other).unwrap();
    press(&mut s, other, P_ACT);
    assert!(d.log().is_empty() && !s.field_err.is_empty(), "a name is needed");
    let mut cx = Cx { focus: Some(row_id(flat, P_FIELD)), ..Cx::default() };
    s.text("Secret SSID", &mut cx);
    press(&mut s, other, P_ACT);
    assert_eq!(d.log(), vec!["wifi_add_and_connect Secret SSID Open key_len=0 hidden=true"]);
}

#[test]
fn the_proxy_fields_save_a_valid_address_on_enter_and_refuse_a_bad_one() {
    let (mut s, d) = win("proxy-on");
    let proxy = |l: &Line| matches!(&l.row, Row::Cycle(_, _, Op::Proxy));
    let flat = s.flat_of(proxy).expect("the address line shows while the proxy is on");
    let mut cx = Cx { focus: Some(row_id(flat, P_FIELD)), ..Cx::default() };
    s.fields.proxy.set_text("");
    s.text("bad address", &mut cx);
    assert!(s.key(0x0D, Mods::default(), &mut cx));
    assert!(d.log().is_empty() && !s.field_err.is_empty(), "a space is not a proxy address");
    s.fields.proxy.set_text("proxy.example.com:3128");
    s.key(0x0D, Mods::default(), &mut cx);
    assert_eq!(d.log(), vec!["proxy_set pac=false proxy.example.com:3128"]);
    // the script address is the line below, with a field of its own
    let pac = s.flat_of(|l| matches!(&l.row, Row::Cycle(_, _, Op::Pac))).expect("the script line");
    cx.focus = Some(row_id(pac, P_FIELD));
    s.fields.pac.set_text("ftp://nope");
    s.key(0x0D, Mods::default(), &mut cx);
    assert_eq!(d.log().len(), 1, "a script address must be http, https or file");
    s.fields.pac.set_text("https://example.com/proxy.pac");
    s.key(0x0D, Mods::default(), &mut cx);
    assert_eq!(d.log().last().map(String::as_str), Some("proxy_set pac=true https://example.com/proxy.pac"));
}

#[test]
fn dragging_a_display_moves_it_and_a_click_picks_it() {
    let (mut s, d) = win("display");
    let b4 = Box4 { left: 100, top: 100, right: 600, bottom: 260 };
    s.arr_box = Some(b4);
    let map = arr_map(&s.mons, b4);
    let m2 = s.mons[1];
    let (x, y) = map.dev(m2.x + m2.w / 2, m2.y + m2.h / 2);
    let mut cx = Cx::default();
    // a click: down and up without moving
    s.down(ID_ARR + 1, Button::Left, x as f32, y as f32, &mut cx);
    s.release(ID_ARR + 1, &mut cx);
    assert_eq!(d.log(), vec!["display_select 1"]);
    d.clear_log();
    // a drag below the first display
    s.down(ID_ARR + 1, Button::Left, x as f32, y as f32, &mut cx);
    let (tx, ty) = map.dev(m2.x + m2.w / 2 - 3000, m2.y + m2.h / 2 + 1800);
    s.drag(ID_ARR + 1, tx as f32, ty as f32, &mut cx);
    assert!(s.arr.as_ref().is_some_and(|a| a.moved));
    s.release(ID_ARR + 1, &mut cx);
    let log = d.log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].starts_with("display_move_monitor 1 "), "{log:?}");
}

#[test]
fn a_click_on_every_control_of_every_page_asks_only_the_demo() {
    for p in 0..PAGES {
        let (mut s, _d) = win("display");
        click(&mut s, ID_NAV + p as Id);
        s.number_lines();
        for f in 0..s.flat.len() {
            for part in [0, P_CTL, P_CTL + 1, P_MUTE, P_SEG, P_ACT] {
                s.click(row_id(f, part), Button::Left, 0.0, 0.0, &mut Cx::default());
                s.number_lines();
            }
        }
    }
}

#[test]
fn the_language_row_says_whether_the_other_programs_will_see_it() {
    let (mut s, d) = win("personal");
    let at = id(&s, |l| matches!(&l.row, Row::Cycle(_, _, Op::UiLang)), P_CTL);
    click(&mut s, at);
    let mut cx = Cx::default();
    s.command(TAG_PICK, 2, &mut cx);
    assert_eq!(d.log(), vec!["set_lang Ja"]);
    assert_eq!(toasts(&cx), vec![lang_note(true)]);
}

// ---------------------------------------------------------------------------------------------------------------------
// pages, timers and the rest

#[test]
fn the_sidebar_pages_history_and_back() {
    let (mut s, _d) = win("display");
    click(&mut s, ID_NAV + 3);
    click(&mut s, ID_NAV + 5);
    assert_eq!((s.page, s.hist.clone()), (5, vec![0, 3]));
    click(&mut s, app::ID_BACK);
    assert_eq!(s.page, 3);
    click(&mut s, app::ID_BACK);
    assert_eq!(s.page, 0);
    click(&mut s, app::ID_BACK);
    assert_eq!(s.page, 0, "nothing to go back to");
    // Ctrl+Down walks the pages, the profile opens About
    assert!(s.key(0x28, Mods { ctrl: true, shift: false }, &mut Cx::default()));
    assert_eq!(s.page, 1);
    click(&mut s, app::ID_PROFILE);
    assert_eq!(s.page, ABOUT_PAGE);
}

#[test]
fn the_pages_that_change_by_themselves_are_read_again_and_only_a_change_asks_for_a_frame() {
    let (mut s, _d) = win("sound");
    let t0 = anim::now_ms();
    let mut cx = Cx::default();
    s.tick(t0 + 10_000, &mut cx);
    assert!(!invalidated(&cx), "nothing changed: no frame");
    // the volume moved by itself (another program): the next reading shows it
    os().audio_set_volume(audio::Src::Out, 0.10);
    let mut cx = Cx::default();
    s.tick(t0 + 20_000, &mut cx);
    assert!(invalidated(&cx), "something changed: one frame");
    // and a reading is not made while a list is open or something is held
    os().audio_set_volume(audio::Src::Out, 0.90);
    s.pick = Some(Op::Res);
    let mut cx = Cx::default();
    s.tick(t0 + 30_000, &mut cx);
    assert!(!invalidated(&cx));
}

#[test]
fn the_page_signature_notices_a_change() {
    let (mut s, _d) = win("display");
    let a = app::page_sig(&s.pg);
    assert_eq!(a, app::page_sig(&page(0)));
    os().display_select(1);
    assert_ne!(a, app::page_sig(&page(0)), "another display: other values");
    s.reread();
    assert_ne!(a, s.pg.groups.len().to_string());
}

#[test]
fn the_source_files_of_the_window_never_call_the_system_but_through_os() {
    // the window's own files may name the types of the logic modules and use their pure helpers; every other call goes through `os()`
    const PURE: [&str; 10] = ["display::resolutions", "display::with_res", "display::rates_at", "display::snap", "wifi::reason_text", "ext::valid_proxy", "ext::valid_pac", "ext::night_helper", "power::size", "net::speed_text"];
    let modules = ["audio", "bt", "display", "ext", "net", "power", "sys", "sysx", "wifi", "wu"];
    for (file, text) in [("main.rs", include_str!("main.rs")), ("app.rs", include_str!("app.rs")), ("draw.rs", include_str!("draw.rs")), ("search.rs", include_str!("search.rs")), ("more.rs", include_str!("more.rs"))] {
        for m in modules {
            let pat = format!("{m}::");
            let mut rest = text;
            while let Some(i) = rest.find(&pat) {
                let before = rest[..i].chars().next_back();
                let tail = &rest[i + pat.len()..];
                let name: String = tail.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let after = tail[name.len()..].chars().next();
                let part_of_a_word = before.is_some_and(|c| c.is_alphanumeric() || c == '_');
                let call = after == Some('(') && name.chars().next().is_some_and(char::is_lowercase);
                let full = format!("{m}::{name}");
                assert!(part_of_a_word || !call || PURE.contains(&full.as_str()), "{file} calls {full}() itself: it must go through os()");
                rest = &rest[i + pat.len()..];
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// through the host's core, drawn for real

struct Rig {
    core: Core,
    gfx: Gfx,
    surf: Surface,
    demo: Rc<Demo>,
    t: u64,
}

impl Rig {
    fn new(state: &str) -> Self {
        use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
        // SAFETY: COM for WIC on the test thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok() };
        let (s, demo) = demo::settings(state);
        let mut r = Self { core: Core::new(Box::new(s), (1000.0, 720.0), 1.0, 0), gfx: Gfx::new(1.0).expect("direct2d"), surf: Surface::new(), demo, t: 1_000_000 };
        r.frame();
        r
    }

    fn frame(&mut self) {
        self.t += 400;
        let (core, t) = (&mut self.core, self.t);
        self.gfx.paint(&mut self.surf, 1000, 720, |c| core.draw(c, t)).expect("a frame");
    }

    fn centre(&self, id: Id) -> (f32, f32) {
        let r = self.core.hits().get(id).unwrap_or_else(|| panic!("item {id} was named")).r;
        (r.cx(), r.cy())
    }

    fn click(&mut self, id: Id) {
        let (x, y) = self.centre(id);
        self.core.pointer_move(x, y, self.t);
        self.core.down(Button::Left, x, y, self.t);
        self.core.up(Button::Left, x, y, self.t);
        self.core.take_host();
        self.frame();
        self.frame();
    }
}

#[test]
fn drawn_for_real_the_window_names_its_items_and_a_click_on_a_list_reaches_choose() {
    use winrust_ui::frame::{WIN_CLOSE, WIN_MAX, WIN_MIN};
    let mut r = Rig::new("display");
    for id in [WIN_MIN, WIN_MAX, WIN_CLOSE, app::ID_BACK, app::ID_SEARCH, app::ID_PROFILE, ID_NAV, ID_NAV + 8, ID_IDENTIFY, ID_ARR, ID_ARR + 1] {
        r.centre(id);
    }
    // over the maximize button Windows shows its Snap Layouts; the empty header drags
    let (mx, my) = r.centre(WIN_MAX);
    assert_eq!(r.core.nc_test(mx, my, true), winrust_ui::core::Nc::MaxButton);
    assert_eq!(r.core.nc_test(600.0, 30.0, true), winrust_ui::core::Nc::Caption);
    // the first drop-down (the one the host's tests click) opens a menu; a row of it is chosen
    r.click(app::ID_FIRST_SELECT);
    assert!(r.core.ov.modal(), "the drop-down is open");
    r.click(winrust_ui::widgets::overlay::menu_row_id(0, 3));
    assert!(!r.core.ov.modal());
    assert_eq!(r.demo.log(), via_choose("display", Op::Res, 3), "the same calls as choose(Res, 3)");
    r.frame();
    r.centre(ID_DLG_KEEP);
    r.centre(ID_DLG_REVERT);
    r.click(ID_DLG_KEEP);
    assert!(r.core.hits().get(ID_DLG_KEEP).is_none(), "the dialog is gone");
}

#[test]
fn the_wheel_scrolls_the_page_and_only_the_lines_in_view_are_named() {
    let mut r = Rig::new("apps-many");
    let rows = |r: &Rig| r.core.hits().all().iter().filter(|h| h.id >= app::ID_ROW && h.id < ID_HIT).map(|h| h.id).collect::<Vec<_>>();
    let before = rows(&r);
    assert!(!before.is_empty() && r.core.hits().all().len() < 60, "400 programs, only the ones in view are named ({} items)", r.core.hits().all().len());
    r.t = anim::now_ms(); // the window reads the wheel's time from the real clock, so the frames after it must be on that clock
    r.core.pointer_move(600.0, 400.0, r.t);
    r.core.wheel(0.0, -6.0, Mods::default(), r.t);
    for _ in 0..4 {
        r.frame();
    }
    let after = rows(&r);
    assert!(!after.is_empty() && after.iter().min() > before.iter().min(), "scrolled down: later lines are in view ({before:?} -> {after:?})");
    assert!(r.core.hits().all().len() < 60);
}

#[test]
fn esc_folds_an_open_line_first_and_then_closes_the_window() {
    let (mut s, _d) = win("network");
    press(&mut s, |l| matches!(&l.row, Row::Wifi(n, _) if n.ssid == "Home-2.4G"), P_BODY_FOR_TEST);
    assert!(s.open.is_some());
    assert!(s.key(0x1B, Mods::default(), &mut Cx::default()));
    assert!(s.open.is_none());
    let mut cx = Cx::default();
    assert!(s.key(0x1B, Mods::default(), &mut cx));
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Close)), "Esc with nothing open closes the window");
}

#[test]
fn the_page_the_window_opens_on_starts_what_it_needs() {
    for (state, call) in [("network", "wifi_scan"), ("power", "")] {
        let (mut s, d) = win(state);
        s.start(0);
        if !call.is_empty() {
            assert_eq!(d.log(), vec![call], "{state}");
        }
    }
}

#[test]
fn at_rest_nothing_asks_for_a_frame_and_a_search_or_a_spinner_is_what_does() {
    for st in ["display", "bt-paired", "network", "wu-up-to-date", "dialog-none"] {
        let r = Rig::new(st);
        assert!(!r.core.wants_frames(r.t + 5000) && r.core.deadline(r.t + 5000).is_none(), "{st}: at rest no frames and no timer of the frame loop");
    }
    let r = Rig::new("display");
    assert!(!r.core.wants_frames(r.t + 5000), "a page at rest: no frames");
    assert_eq!(r.core.deadline(r.t + 5000), None, "and no timer of the frame loop");
    // the Bluetooth radar and the update spinner move: frames are due while they do
    let r = Rig::new("bt-searching");
    assert!(r.core.deadline(r.t).is_some() || r.core.wants_frames(r.t), "the radar draws while a search runs");
    let r = Rig::new("update-checking");
    assert!(r.core.deadline(r.t).is_some() || r.core.wants_frames(r.t), "the spinner draws while something runs");
}

#[test]
fn light_and_dark_draw_every_state_of_the_design() {
    for light in [false, true] {
        winrust_common::style::set_light(light);
        for st in demo::STATES {
            let mut r = Rig::new(st);
            r.frame();
        }
    }
    winrust_common::style::set_light(false);
}

// ---------------------------------------------------------------------------------------------------------------------
// the pages of the second design round: Notifications, Privacy & security, Keyboard & mouse, Date & time

#[test]
fn the_new_pages_are_in_the_sidebars_order_and_the_search_finds_their_lines() {
    assert_eq!(PAGES, 13);
    assert_eq!((NOTIFY_PAGE, PERSONAL_PAGE, PRIVACY_PAGE, KEYBOARD_PAGE, TIME_PAGE, WU_PAGE, ABOUT_PAGE), (6, 7, 8, 9, 10, 11, 12));
    assert_eq!(look::NAV.len(), PAGES);
    let pgs = pages();
    assert!(pgs.iter().all(|p| !p.groups.is_empty()));
    // (under test the language is Russian: ask for a line by the label the page itself has)
    let label = |p: usize, pred: fn(&Row) -> bool| pgs[p].groups.iter().flat_map(|g| &g.rows).find(|l| pred(&l.row)).unwrap().row.label().to_lowercase();
    let f = search::search(&pgs, &label(PRIVACY_PAGE, |r| matches!(r, Row::Info(_, v) if v == "On" || v == "Включён")));
    assert!(f.iter().any(|f| f.page == PRIVACY_PAGE), "{f:?}");
    let f = search::search(&pgs, &label(KEYBOARD_PAGE, |r| matches!(r, Row::Gauge(_, Gauge::Pointer))));
    assert!(f.iter().any(|f| f.page == KEYBOARD_PAGE), "{f:?}");
}

#[test]
fn a_notifying_app_unfolds_and_its_way_reaches_the_system() {
    let (mut s, d) = win("notify");
    let tg = |l: &Line| l.key == "na:telegram";
    press(&mut s, tg, P_BODY);
    assert_eq!(s.open.as_deref(), Some("na:telegram"), "a click unfolds it");
    let flat = s.flat_of(tg).unwrap();
    let acts = s.expansion(s.line(flat).unwrap());
    assert!(matches!(acts.as_slice(), [draw::ExpItem::Acts(a)] if a.len() == 3 && a[0].kind == draw::BKind::Chosen), "{acts:?}");
    click(&mut s, row_id(flat, P_ACT + 1)); // only in the centre
    assert_eq!(d.log(), vec!["notify_set telegram 1"]);
    assert_eq!(s.line(flat).unwrap().cur, Some(1), "the page was read again");
    click(&mut s, row_id(flat, P_ACT + 2)); // off
    assert_eq!(d.log().last().map(String::as_str), Some("notify_set telegram 2"));
}

#[test]
fn the_master_switch_dims_what_depends_on_it_and_do_not_disturb_is_the_bars_file() {
    let (mut s, d) = win("notify");
    press(&mut s, |l| matches!(&l.row, Row::Flag(_, Flag::Dnd)), P_CTL);
    assert_eq!(d.log(), vec!["flag_set Dnd true"]);
    d.clear_log();
    press(&mut s, |l| matches!(&l.row, Row::Toggle { name: "ToastEnabled", .. }), P_CTL);
    assert_eq!(d.log(), vec!["reg_set_dword ToastEnabled 0"]);
    d.clear_log();
    // everything that depends on it is dimmed and does nothing
    assert!(s.line(s.flat_of(|l| matches!(&l.row, Row::Flag(_, Flag::Dnd))).unwrap()).unwrap().dis);
    press(&mut s, |l| matches!(&l.row, Row::Flag(_, Flag::Dnd)), P_CTL);
    press(&mut s, |l| matches!(&l.row, Row::Toggle { name: "NOC_GLOBAL_SETTING_ALLOW_NOTIFICATION_SOUND", .. }), P_CTL);
    assert!(d.log().is_empty(), "{:?}", d.log());
    // a drawn frame of the dimmed page
    Rig::new("notify-off").frame();
}

#[test]
fn the_lists_of_the_new_pages_reach_opt_set_and_a_refusal_says_so() {
    // a segmented row: a segment picks its entry
    let (mut s, d) = win("notify");
    press(&mut s, |l| matches!(&l.row, Row::Cycle(_, _, Op::Opt(Opt::Toast))), P_SEG + 3);
    assert_eq!(d.log(), vec!["opt_set Toast 3"]);
    // a drop-down: the same list the model has, a pick reaches it
    let (mut s, d) = win("time");
    let at = id(&s, |l| matches!(&l.row, Row::Cycle(_, _, Op::Opt(Opt::Zone))), P_CTL);
    let cx = click(&mut s, at);
    let (tag, items) = menu(&cx).expect("a list");
    assert_eq!(tag, TAG_PICK);
    assert_eq!(items.iter().map(|m| m.text.clone()).collect::<Vec<_>>(), choices(Op::Opt(Opt::Zone)).unwrap().0);
    let mut cx = Cx::default();
    s.command(TAG_PICK, 2, &mut cx);
    assert_eq!(d.log(), vec!["opt_set Zone 2"]);
    assert!(toasts(&cx).is_empty());
    // Windows refuses (no administrator rights): a toast says so, and the list is read again
    d.deny_admin();
    s.click(at, Button::Left, 0.0, 0.0, &mut Cx::default());
    let mut cx = Cx::default();
    s.command(TAG_PICK, 3, &mut cx);
    assert_eq!(toasts(&cx).len(), 1, "{:?}", toasts(&cx));
    assert_eq!(choices(Op::Opt(Opt::Zone)).unwrap().1, Some(2), "the zone did not move");
    // the clock preview follows the 12-hour choice
    press(&mut s, |l| matches!(&l.row, Row::Cycle(_, _, Op::Opt(Opt::Clock))), P_SEG + 1);
    assert!(s.pg.groups[0].rows[0].row.label().contains("AM"), "{}", s.pg.groups[0].rows[0].row.label());
}

#[test]
fn the_gauges_snap_to_their_steps_and_say_it_in_words() {
    let (mut s, d) = win("keyboard");
    let flat = s.flat_of(|l| matches!(&l.row, Row::Gauge(_, Gauge::KeyDelay))).unwrap();
    s.tracks.push((flat, 100.0, 400.0));
    let at = row_id(flat, P_CTL);
    let mut cx = Cx::default();
    s.down(at, Button::Left, 391.0, 0.0, &mut cx);
    assert_eq!(d.log().last().map(String::as_str), Some("gauge_set KeyDelay 1.00"));
    assert_eq!(s.line(flat).unwrap().sub, more::gauge_text(Gauge::KeyDelay, Some(1.0)));
    s.drag(at, 109.0, 0.0, &mut cx);
    assert_eq!(s.line(flat).unwrap().sub, more::gauge_text(Gauge::KeyDelay, Some(0.0)));
    s.release(at, &mut cx);
    assert!(s.hold.is_none());
    let flat = s.flat_of(|l| matches!(&l.row, Row::Gauge(_, Gauge::Pointer))).unwrap();
    s.tracks.push((flat, 100.0, 400.0));
    s.down(row_id(flat, P_CTL), Button::Left, 250.0, 0.0, &mut cx);
    assert_eq!(d.log().last().map(String::as_str), Some("gauge_set Pointer 0.50"));
}

#[test]
fn pointer_precision_and_the_clock_service_are_flags_and_a_refusal_is_told() {
    let (mut s, d) = win("keyboard");
    press(&mut s, |l| matches!(&l.row, Row::Flag(_, Flag::MouseAccel)), P_CTL);
    assert_eq!(d.log(), vec!["flag_set MouseAccel false"]);
    assert_eq!(s.line(s.flat_of(|l| matches!(&l.row, Row::Flag(_, Flag::MouseAccel))).unwrap()).unwrap().on, Some(false));
    let (mut s, d) = win("time");
    d.deny_admin();
    let cx = press(&mut s, |l| matches!(&l.row, Row::Flag(_, Flag::AutoTime)), P_CTL);
    assert_eq!(toasts(&cx).len(), 1, "the time service needs administrator rights");
    assert_eq!(os().flag(&Flag::AutoTime), Some(true), "nothing changed");
}

#[test]
fn the_try_it_field_takes_keys_and_does_nothing_with_them() {
    let (mut s, d) = win("keyboard");
    let flat = s.flat_of(|l| matches!(&l.row, Row::Cycle(_, _, Op::Try))).unwrap();
    assert_eq!(s.fields_of(flat), vec![app::Fid::Try]);
    let mut cx = Cx { focus: Some(row_id(flat, P_FIELD)), ..Cx::default() };
    s.text("abc", &mut cx);
    assert_eq!(s.fields.tryit.text, "abc");
    s.key(0x0D, Mods::default(), &mut cx);
    assert!(d.log().is_empty(), "{:?}", d.log());
}

#[test]
fn privacy_tabs_follow_the_pick_and_the_permissions_reach_the_consent_store() {
    let (mut s, d) = win("privacy");
    assert_eq!(more::pv_tab(), Cap::Mic);
    // "Right now": a click on the camera line opens its tab and scrolls to the list
    press(&mut s, |l| l.key == "now:webcam", P_BODY);
    assert_eq!(more::pv_tab(), Cap::Cam);
    assert_eq!(s.reveal.as_deref(), Some("pvtab"));
    assert!(s.pg.groups[2].rows.iter().any(|l| l.key == "webcam:wcam"), "the camera's apps are listed");
    // the tab control picks another
    press(&mut s, |l| l.key == "pvtab", P_SEG + 2);
    assert_eq!(more::pv_tab(), Cap::Loc);
    assert!(s.pg.groups[2].rows.iter().any(|l| l.key == "location:weather"));
    // one Store app's switch, the user's switch
    press(&mut s, |l| l.key == "location:weather", P_CTL);
    press(&mut s, |l| l.key == "location:user", P_CTL);
    assert_eq!(d.log(), vec![format!("flag_set {:?} false", Flag::Access(Cap::Loc, Who::App("weather".into()))), format!("flag_set {:?} false", Flag::Access(Cap::Loc, Who::User))]);
    d.clear_log();
    // with the user's switch off the apps below are dimmed and do nothing
    press(&mut s, |l| l.key == "location:maps", P_CTL);
    assert!(d.log().is_empty(), "{:?}", d.log());
    // the PC's switch is the administrator's
    d.deny_admin();
    let cx = press(&mut s, |l| l.key == "location:dev", P_CTL);
    assert_eq!(toasts(&cx).len(), 1);
}

#[test]
fn a_camera_off_for_the_pc_says_blocked_for_every_app_in_right_now() {
    let (s, _d) = win("privacy-camera-off");
    let cam = s.pg.groups[0].rows.iter().find(|l| l.key == "now:webcam").unwrap();
    assert!(cam.sub.contains("Blocked") || cam.sub.contains("Закрыт"), "{}", cam.sub);
    assert!(cam.badge.is_some());
    // and in use: the microphone says who has it
    let (s, _d) = win("privacy");
    let mic = s.pg.groups[0].rows.iter().find(|l| l.key == "now:microphone").unwrap();
    assert!(mic.sub.contains("Discord") && mic.badge.is_some(), "{}", mic.sub);
}

#[test]
fn windows_security_scans_and_opens() {
    let (mut s, d) = win("privacy");
    press(&mut s, |l| l.key == "sec:av", P_CTL);
    assert_eq!(d.log(), vec!["quick_scan"]);
    d.clear_log();
    press(&mut s, |l| l.key == "sec:open", P_BODY);
    assert_eq!(d.log(), vec!["open_security"]);
    // a scan under way: the button is busy and does nothing
    let (mut s, d) = win("privacy-scanning");
    assert!(s.pg.groups[1].rows[0].busy);
    press(&mut s, |l| l.key == "sec:av", P_CTL);
    assert!(d.log().is_empty());
}

#[test]
fn sync_now_runs_once_until_the_time_service_answers() {
    let (mut s, d) = win("time");
    press(&mut s, |l| l.key == "sync", P_CTL);
    assert_eq!(d.log(), vec!["time_sync"]);
    assert!(s.syncing);
    press(&mut s, |l| l.key == "sync", P_CTL);
    assert_eq!(d.log().len(), 1, "a second click waits");
    // the answer (the worker's note) frees the button
    s.message(app::MSG_NOTE, 0, Box::into_raw(Box::new("Clock synced".to_string())) as isize, &mut Cx::default());
    assert!(!s.syncing);
}

#[test]
fn names_dates_and_times_of_the_new_pages_read_well() {
    assert_eq!(more::app_name("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "Windows Camera");
    assert_eq!(more::app_name("C:#Program Files#Discord#discord.exe"), "Discord");
    assert_eq!(more::app_name("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"), "Windows Terminal");
    assert_eq!(more::app_name("Chrome"), "Chrome");
    assert_eq!(more::fmt_date("dd.MM.yyyy", 2026, 11, 7), "07.11.2026");
    assert_eq!(more::fmt_date("M/d/yy", 2026, 11, 7), "11/7/26");
    assert_eq!(more::fmt_date("yyyy-MM-dd", 2026, 1, 27), "2026-01-27");
    let w = |ago| more::When { days_ago: ago, hour: 8, min: 5, day: 20, month: 11 };
    assert!(more::when_text(w(0)).ends_with("08:05") && more::when_text(w(1)).ends_with("08:05"));
    assert!(more::when_text(w(5)).starts_with("20 "));
}

#[test]
fn an_app_that_installs_into_a_new_folder_each_update_is_one_row_and_a_crash_is_not_in_use() {
    use crate::more::{CApp, RawDesk, When, group_desktop};
    let w = |ago: u32| When { days_ago: ago, hour: 9, min: 0, day: 1, month: 1 };
    let k = |key: &str, start: u64, stop: u64, ago: u32| RawDesk { key: key.into(), start, stop, start_when: Some(w(ago)), stop_when: (stop != 0).then(|| w(ago)) };
    let raw = || vec![k("C:#x#Discord#app-1.0.9248#DISCORD.EXE", 100, 0, 130), k("C:#x#Discord#app-1.0.9251#Discord.exe", 200, 0, 100), k("C:#x#Edge#1#msedgewebview2.exe", 10, 20, 9), k("C:#x#Edge#2#msedgewebview2.exe", 30, 40, 7)];
    let named = |_: &str| None;
    // not running: two crashed keys, none of them "in use"; one row per exe, the latest start speaks for it
    let apps: Vec<CApp> = group_desktop(raw(), &|_| false, &named);
    assert_eq!(apps.len(), 2, "{apps:?}");
    let d = apps.iter().find(|a| a.id.contains("9251")).expect("the latest Discord key");
    assert!(!d.live && d.last == Some(w(100)), "a crash left it open: it was last used at its start: {d:?}");
    let e = apps.iter().find(|a| a.id.contains("#2#")).unwrap();
    assert_eq!(e.last, Some(w(7)), "the closed one: its stop time");
    // running: the newest key was never closed and Discord runs: in use now
    assert!(group_desktop(raw(), &|exe| exe == "discord.exe", &named).iter().any(|a| a.id.contains("9251") && a.live));
    // the name comes from the exe's own description when it is there, else the prettified file name
    let apps = group_desktop(raw(), &|_| false, &|p| p.ends_with("msedgewebview2.exe").then(|| "Microsoft Edge WebView2".to_string()));
    assert!(apps.iter().any(|a| a.name == "Microsoft Edge WebView2") && apps.iter().any(|a| a.name == "Discord"), "{apps:?}");
}

#[test]
fn the_microphone_list_shows_discord_once_and_names_the_webview_properly() {
    let (s, _d) = win("privacy");
    let rows = &s.pg.groups[2].rows;
    assert_eq!(rows.iter().filter(|l| l.row.label() == "Discord").count(), 1, "one Discord row");
    assert!(rows.iter().any(|l| l.row.label() == "Microsoft Edge WebView2"));
    assert!(rows.iter().filter(|l| l.badge.is_some()).count() >= 2, "the PC's Admin badge and Discord's In use now");
    let mic = s.pg.groups[0].rows.iter().find(|l| l.key == "now:microphone").unwrap();
    assert!(mic.sub.contains("Discord"), "{}", mic.sub);
}

#[test]
fn the_accent_swatches_reach_the_accent_and_show_the_current_one() {
    let (mut s, d) = win("personal");
    let line = |s: &Settings| s.pg.groups[1].rows.iter().find(|l| l.key == "accent").map(|l| (l.opts.len(), l.cur)).unwrap();
    assert_eq!(line(&s), (winrust_common::style::Accent::ALL.len(), Some(0)), "the demo's accent is the first");
    press(&mut s, |l| matches!(&l.row, Row::Cycle(_, _, Op::Accent)), P_SEG + 3);
    assert_eq!(d.log(), vec!["accent_set 3"]);
    assert_eq!(line(&s), (5, Some(3)), "the page was read again");
    Rig::new("personal").frame();
}

#[test]
fn a_wallpaper_tile_reaches_the_system_and_the_current_one_is_marked() {
    let (mut s, d) = win("personal");
    let on = |s: &Settings| match &s.pg.groups[0].extra {
        Extra::Walls(w) => w.iter().position(|w| w.on),
        _ => None,
    };
    assert_eq!(on(&s), Some(0));
    click(&mut s, ID_WALL + 2);
    assert_eq!(d.log(), vec!["wallpaper_select 2"]);
    assert_eq!(on(&s), Some(2), "the picked preset is the selected one");
    // the "Choose..." tile is the last one
    d.clear_log();
    click(&mut s, ID_WALL + 5);
    assert_eq!(d.log(), vec!["wallpaper_pick"]);
}

#[test]
fn the_update_status_says_when_windows_last_looked_and_not_the_verdict_twice() {
    let (s, _d) = win("wu-up-to-date");
    let st = &s.pg.groups[0].rows[0];
    assert!(st.sub.contains("17:02"), "{}", st.sub);
    assert_ne!(st.sub, os().wu_state().note);
    assert_ne!(st.sub, st.row.label(), "the title is the verdict, the line under it is not");
    assert!(st.sub.contains('\u{b7}'), "and the last installation: {}", st.sub);
    // not looked in this window yet: Windows' own date instead of a bare "Not checked yet"
    let d = Rc::new(Demo::for_state("wu-up-to-date"));
    os::set_os(d.clone());
    let mut w = Settings::new(WU_PAGE);
    w.reread();
    assert!(!w.pg.groups[0].rows[0].sub.is_empty());
}

#[test]
fn restart_now_needs_a_second_click_and_only_then_restarts() {
    let (mut s, d) = win("wu-restart");
    let st = |l: &Line| l.key == "wu-status";
    assert!(s.pg.groups[0].rows[0].restart);
    press(&mut s, st, P_CTL2);
    assert!(d.log().is_empty(), "the first click only arms the button: {:?}", d.log());
    assert_eq!(s.armed.as_deref(), Some("wu-restart"));
    let cx = press(&mut s, st, P_CTL2);
    assert_eq!(d.log(), vec!["restart_now"]);
    assert_eq!(toasts(&cx).len(), 1);
    // "Later" does nothing to the machine
    d.clear_log();
    press(&mut s, st, P_CTL);
    assert!(d.log().is_empty());
    // leaving the page disarms it
    press(&mut s, st, P_CTL2);
    click(&mut s, ID_NAV);
    assert!(s.armed.is_none());
}

#[test]
fn the_about_page_has_every_fact_of_the_design_and_copy_takes_them_all() {
    let (mut s, _d) = win("about");
    let labels: Vec<String> = s.pg.groups.iter().flat_map(|g| g.rows.iter().map(|l| l.row.label().to_string())).collect();
    assert_eq!(labels.len(), 9, "name, processor, memory, graphics, system type, edition, version, build, WinRust: {labels:?}");
    let cx = {
        let mut cx = Cx::default();
        s.click(ID_GACT, Button::Left, 0.0, 0.0, &mut cx);
        cx
    };
    let copied = cx.reqs.iter().find_map(|r| if let Req::Clipboard(t) = r { Some(t.clone()) } else { None });
    let copied = copied.expect("the details are on the clipboard");
    for want in ["DANIIAR-PC", "Ryzen", "32 GB", "Radeon 780M", "x64", "26100.2314", "0.5.9"] {
        assert!(copied.contains(want), "{want} in {copied}");
    }
}

#[test]
fn a_computer_name_is_checked_before_windows_sees_it_and_a_refusal_is_told() {
    assert!(more::valid_pc_name("DANIIAR-PC") && more::valid_pc_name("A"));
    for bad in ["", "-A", "A-", "MY PC", "ABCDEFGHIJKLMNOP", "123", "A_B", "\u{43f}\u{43a}"] {
        assert!(!more::valid_pc_name(bad), "{bad:?}");
    }
    let (mut s, d) = win("about");
    let at = |l: &Line| l.key == "pcname";
    press(&mut s, at, P_CTL);
    assert_eq!(s.open.as_deref(), Some("pcname"));
    assert_eq!(s.fields.pc.text, "DANIIAR-PC", "the field starts with the current name");
    let flat = s.flat_of(at).unwrap();
    // a bad name: an error under the field, nothing reaches Windows
    s.fields.pc.set_text("my pc");
    press(&mut s, at, P_ACT);
    assert!(!s.field_err.is_empty() && d.log().is_empty(), "{:?}", d.log());
    // an administrator is needed: the error is said and nothing changes
    d.deny_admin();
    s.fields.pc.set_text("new-pc");
    press(&mut s, at, P_ACT);
    assert_eq!(d.log(), vec!["rename_pc NEW-PC"]);
    assert!(s.field_err.contains("dmin") || s.field_err.contains("дминистр"), "{}", s.field_err);
    assert_eq!(s.open.as_deref(), Some("pcname"), "it stays open to try again");
    // Enter in the field saves too; the name waits for a restart
    let (mut s, d) = win("about");
    press(&mut s, at, P_CTL);
    s.fields.pc.set_text("new-pc");
    let mut cx = Cx { focus: Some(row_id(flat, P_FIELD)), ..Cx::default() };
    s.key(0x0D, Mods::default(), &mut cx);
    assert_eq!(d.log(), vec!["rename_pc NEW-PC"]);
    assert_eq!(toasts(&cx).len(), 1);
    assert!(s.open.is_none());
    assert!(s.line(flat).unwrap().sub.contains("NEW-PC"), "{}", s.line(flat).unwrap().sub);
    // Cancel folds it without a call
    let (mut s, d) = win("about");
    press(&mut s, at, P_CTL);
    press(&mut s, at, P_ACT + 1);
    assert!(s.open.is_none() && d.log().is_empty());
}
