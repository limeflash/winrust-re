//! The file manager without a window: every key of the old window's list (`main.rs` header), the tabs, the address and search boxes, the
//! menus, drops and drags, driven through `App` on the made-up machine of the design, which records what it was asked to do. The tests
//! run in Russian (`lang!()`), so what they look at is structure and calls, not words.
use std::{path::PathBuf, sync::Arc};

use winrust_ui::{
    core::{App, DropFx},
    frame::{Button, Cx, Id, Mods, Req},
    gfx::Rect,
    widgets::{list::row_id, overlay::MenuItem},
};

use super::{
    backend::{Backend, Clock, Do, Facts, Flag, Found, JobView, Opts, Places},
    demo::{Fake, build},
    live::{MSG_DRAG_DONE, MSG_MENU_OWN, MSG_RELOAD, MSG_TICK},
    model::*,
    ops::*,
    text,
};
use crate::Row;

/// A window and its fake machine, with the keyboard focus the way the host keeps it between calls.
struct T {
    fm: Fm,
    fake: Arc<Fake>,
    focus: Option<Id>,
    menu: Option<(u32, Vec<MenuItem>)>,
    toasts: Vec<String>,
}

impl T {
    fn new(state: &str) -> Self {
        let (fm, fake) = build(state);
        Self { fm, fake, focus: None, menu: None, toasts: Vec::new() }
    }

    fn cx(&self, alt: bool) -> Cx {
        Cx { alt, focus: self.focus, scale: 1.0, ..Cx::default() }
    }

    /// What the host does with what the app asked for.
    fn settle(&mut self, cx: Cx) {
        for r in cx.reqs {
            match r {
                Req::Focus(id) => self.focus = Some(id),
                Req::Menu { tag, items, .. } => self.menu = Some((tag, items)),
                Req::Toast { text, .. } => self.toasts.push(text),
                _ => {}
            }
        }
    }

    fn key(&mut self, vk: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
        let mut cx = self.cx(alt);
        let used = self.fm.key(vk, Mods { ctrl, shift }, &mut cx);
        self.settle(cx);
        used
    }

    fn press(&mut self, vk: u32) -> bool {
        self.key(vk, false, false, false)
    }

    fn ctrl(&mut self, vk: u32) -> bool {
        self.key(vk, true, false, false)
    }

    fn alt(&mut self, vk: u32) -> bool {
        self.key(vk, false, false, true)
    }

    fn text(&mut self, s: &str) {
        let mut cx = self.cx(false);
        self.fm.text(s, &mut cx);
        self.settle(cx);
    }

    fn click(&mut self, id: Id, b: Button) {
        let mut cx = self.cx(false);
        self.fm.down(id, b, 10.0, 10.0, &mut cx);
        self.fm.click(id, b, 10.0, 10.0, &mut cx);
        self.fm.release(id, &mut cx);
        self.settle(cx);
    }

    fn message(&mut self, msg: u32) {
        let mut cx = self.cx(false);
        self.fm.message(msg, 0, 0, &mut cx);
        self.settle(cx);
    }

    /// What the machine was asked since the last time (a listing's own housekeeping, `Navigated` and `Sizes`, is not looked at).
    fn calls(&self) -> Vec<String> {
        quiet(self.fake.take_calls())
    }

    fn names(&self) -> Vec<String> {
        self.fm.rows.iter().map(|r| r.name.clone()).collect()
    }

    fn row(&self, name: &str) -> usize {
        self.fm.rows.iter().position(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}"))
    }

    fn select(&mut self, name: &str) {
        self.fm.select_names(&[name.to_string()]);
    }

    fn one(&self) -> Option<String> {
        self.fm.one().map(|i| self.fm.rows[i].name.clone())
    }

    fn dir(&self) -> String {
        self.fm.dir.to_string_lossy().into_owned()
    }
}

fn quiet(calls: Vec<String>) -> Vec<String> {
    calls.into_iter().filter(|c| !c.starts_with("Navigated(") && !c.starts_with("Sizes {")).collect()
}

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_DELETE: u32 = 0x2E;
const VK_F2: u32 = 0x71;
const VK_F4: u32 = 0x73;
const VK_F5: u32 = 0x74;
const VK_APPS: u32 = 0x5D;
const A: u32 = 0x41;
const C: u32 = 0x43;
const F: u32 = 0x46;
const H: u32 = 0x48;
const L: u32 = 0x4C;
const M: u32 = 0x4D;
const N: u32 = 0x4E;
const T_: u32 = 0x54;
const V: u32 = 0x56;
const W: u32 = 0x57;
const X: u32 = 0x58;

#[test]
fn enter_opens_a_folder_backspace_and_alt_arrows_walk_the_history() {
    let mut t = T::new("folder");
    assert_eq!(t.dir(), r"C:\");
    assert!(t.press(VK_RETURN)); // Projects is picked
    assert_eq!(t.dir(), r"C:\Projects");
    assert!(t.press(VK_BACK));
    assert_eq!(t.dir(), r"C:\");
    assert!(t.alt(VK_LEFT));
    assert_eq!(t.dir(), r"C:\Projects");
    assert!(t.alt(VK_RIGHT));
    assert_eq!(t.dir(), r"C:\");
    t.select("Projects");
    t.press(VK_RETURN);
    t.select("WinRust");
    t.press(VK_RETURN);
    assert_eq!(t.dir(), r"C:\Projects\WinRust");
    assert!(t.alt(VK_UP));
    assert_eq!(t.dir(), r"C:\Projects");
    assert!(t.fm.can_back() && !t.fm.can_forward());
}

#[test]
fn enter_on_a_file_opens_it_and_up_from_a_drive_root_is_this_pc() {
    let mut t = T::new("folder");
    t.select("config.toml");
    t.press(VK_RETURN);
    assert_eq!(t.calls(), [r#"Open("C:\\config.toml")"#]);
    assert_eq!(t.dir(), r"C:\", "a file does not navigate");
    t.press(VK_BACK);
    assert!(t.fm.at_pc(), "up from C:\\ lists the drives");
    assert_eq!(t.fm.rows.len(), 4);
    assert!(!t.fm.can_up(), "there is nothing above This PC");
    t.press(VK_BACK);
    assert!(t.fm.at_pc());
    // Enter on a drive opens it
    t.fm.select_names(&[r"D:\".to_string()]);
    t.press(VK_RETURN);
    assert_eq!(t.dir(), r"D:\");
}

#[test]
fn delete_cut_copy_paste_go_to_the_machine_as_the_shell_would_get_them() {
    let mut t = T::new("folder");
    t.select("Projects");
    t.press(VK_DELETE);
    t.ctrl(C);
    t.ctrl(X);
    t.ctrl(V);
    let calls = t.calls();
    assert_eq!(calls[0], r#"Delete { paths: ["C:\\Projects"], dir: "C:\\" }"#);
    assert_eq!(calls[1], r#"Clip { paths: ["C:\\Projects"], cut: false }"#);
    assert_eq!(calls[2], r#"Clip { paths: ["C:\\Projects"], cut: true }"#);
    assert_eq!(calls[3], r#"Paste("C:\\")"#);
    // several rows
    t.fm.list.click(0, false, false);
    t.fm.list.click(2, true, false);
    t.ctrl(C);
    assert_eq!(t.calls(), [r#"Clip { paths: ["C:\\PerfLogs", "C:\\Program Files (x86)"], cut: false }"#]);
    // nothing picked: nothing to delete or copy
    t.fm.list.sel.clear();
    t.press(VK_DELETE);
    t.ctrl(C);
    assert!(t.calls().is_empty());
}

#[test]
fn ctrl_n_makes_a_folder_and_opens_its_name_for_editing() {
    let mut t = T::new("folder");
    assert!(t.ctrl(N));
    assert_eq!(t.calls(), ["new_entry New folder"]);
    let r = t.fm.ren.as_ref().expect("the name is open for editing");
    assert_eq!(r.name, "New folder");
    assert_eq!(t.focus, Some(ID_REN));
    assert_eq!(t.one().as_deref(), Some("New folder"));
    // typing replaces the whole name (a folder's name has no extension to keep), Enter makes it so
    t.text("Docs");
    assert!(t.press(VK_RETURN));
    assert_eq!(t.calls(), ["rename New folder -> Docs"]);
    assert!(t.fm.ren.is_none());
    assert!(t.names().contains(&"Docs".to_string()) && !t.names().contains(&"New folder".to_string()));
    assert_eq!(t.one().as_deref(), Some("Docs"), "the renamed item stays picked");
}

#[test]
fn f2_renames_a_file_keeping_its_extension_selected_apart_and_refuses_bad_names() {
    let mut t = T::new("folder");
    t.select("config.toml");
    assert!(t.press(VK_F2));
    let r = t.fm.ren.as_ref().unwrap();
    assert_eq!((r.field.anchor, r.field.caret), (0, "config".len()), "the name without its extension is selected");
    // a name with a character Windows does not allow: the box stays, shakes, and says so
    t.fm.ren.as_mut().unwrap().field.set_text("a:b.toml");
    t.press(VK_RETURN);
    assert!(t.fm.ren.is_some() && t.fm.ren.as_ref().unwrap().bad == 1 && !t.toasts.is_empty());
    // a name that is taken
    t.fm.ren.as_mut().unwrap().field.set_text("INSTALL.log");
    t.press(VK_RETURN);
    assert_eq!(t.fm.ren.as_ref().unwrap().bad, 2);
    assert!(t.calls().is_empty(), "nothing was renamed");
    // Esc lets go
    t.press(VK_ESCAPE);
    assert!(t.fm.ren.is_none());
    assert!(t.calls().is_empty());
    // the same name again is no rename either
    t.press(VK_F2);
    t.press(VK_RETURN);
    assert!(t.fm.ren.is_none() && t.calls().is_empty());
    // not in a search
    t.fm.query = Some("x".into());
    t.press(VK_F2);
    assert!(t.fm.ren.is_none());
}

#[test]
fn tabs_open_close_and_cycle_like_the_old_window() {
    let mut t = T::new("folder");
    assert_eq!((t.fm.tabs.len(), t.fm.active), (2, 0));
    assert!(t.ctrl(T_));
    assert_eq!((t.fm.tabs.len(), t.fm.active), (3, 2), "a new tab on the same folder, and it is the one shown");
    assert_eq!(t.fm.tabs[2], PathBuf::from(r"C:\"));
    // Ctrl+Tab goes round
    t.ctrl(VK_TAB);
    assert_eq!(t.fm.active, 0);
    // the second tab holds Downloads
    t.fm.tab_select(1, &mut Cx::default());
    assert!(t.dir().ends_with("Downloads"));
    // Ctrl+W closes the shown one: its neighbour takes over
    t.ctrl(W);
    assert_eq!(t.fm.tabs.len(), 2);
    assert_eq!(t.fm.active, 1);
    // Ctrl+Shift+Tab goes back
    t.key(VK_TAB, true, true, false);
    assert_eq!(t.fm.active, 0);
    t.ctrl(W);
    t.ctrl(W);
    assert_eq!(t.fm.tabs.len(), 1, "the last one stays");
    t.ctrl(W);
    assert_eq!(t.fm.tabs.len(), 1);
}

#[test]
fn a_middle_click_on_a_folder_opens_it_in_a_new_tab_and_stays() {
    let mut t = T::new("folder");
    let i = t.row("Windows");
    t.click(row_id(ID_LIST, i), Button::Middle);
    assert_eq!(t.fm.tabs.len(), 3);
    assert_eq!(t.fm.tabs[2], PathBuf::from(r"C:\Windows"));
    assert_eq!(t.fm.active, 0, "the listing stays");
    assert_eq!(t.dir(), r"C:\");
    // on a file: nothing
    let f = t.row("config.toml");
    t.click(row_id(ID_LIST, f), Button::Middle);
    assert_eq!(t.fm.tabs.len(), 3);
    // a middle click on a tab closes it
    t.click(ID_TAB + 2, Button::Middle);
    assert_eq!(t.fm.tabs.len(), 2);
}

#[test]
fn ctrl_l_and_f4_edit_the_address_a_path_goes_there_and_question_mark_searches() {
    let mut t = T::new("folder");
    assert!(t.ctrl(L));
    assert!(t.fm.addr_edit && t.focus == Some(ID_ADDR));
    assert_eq!(t.fm.addr.text, r"C:\");
    // the keys now belong to the box: Backspace edits it, it does not go up
    t.fm.addr.set_text(r"C:\Projects\quipu");
    assert!(t.press(VK_RETURN));
    assert_eq!(t.dir(), r"C:\Projects\quipu");
    assert!(!t.fm.addr_edit);
    // F4 does the same
    assert!(t.press(VK_F4));
    assert!(t.fm.addr_edit);
    // a path that is not there: the box stays and shakes
    t.fm.addr.set_text(r"C:\no\such\folder");
    t.press(VK_RETURN);
    assert!(t.fm.addr_edit && t.fm.addr_bad);
    assert_eq!(t.dir(), r"C:\Projects\quipu");
    // Esc gives the old path back
    t.press(VK_ESCAPE);
    assert!(!t.fm.addr_edit);
    assert_eq!(t.fm.addr.text, r"C:\Projects\quipu");
    // "? terms" is a search
    t.press(VK_F4);
    t.fm.addr.set_text("? readme");
    t.press(VK_RETURN);
    assert_eq!(t.fm.query.as_deref(), Some("readme"));
    assert!(t.calls().contains(&"search readme".to_string()));
}

#[test]
fn backspace_in_the_address_box_edits_it_and_does_not_go_up() {
    let mut t = T::new("folder");
    t.press(VK_RETURN); // Projects
    t.ctrl(L);
    t.press(VK_BACK);
    assert_eq!(t.dir(), r"C:\Projects");
}

#[test]
fn ctrl_f_focuses_the_search_box_enter_searches_and_escape_clears() {
    let mut t = T::new("folder");
    assert!(t.ctrl(F));
    assert_eq!(t.focus, Some(ID_SEARCH));
    t.text("read");
    assert_eq!(t.fm.srch.text, "read");
    assert!(t.press(VK_RETURN));
    assert_eq!(t.fm.query.as_deref(), Some("read"));
    assert_eq!(t.fm.rows.len(), 2);
    // a hit's name is its whole path, shown as the file name
    assert_eq!(t.fm.shown_name(0), "README.md");
    assert!(t.fm.rows[0].name.starts_with(r"C:\Projects\"));
    // Enter on a hit opens the file by its path
    t.fm.list.click(0, false, false);
    t.press(VK_RETURN);
    assert!(t.calls().iter().any(|c| c.starts_with(r#"Open("C:\\Projects\\"#)));
    // Esc (the keys are the list's again) goes back to the folder
    t.press(VK_ESCAPE);
    assert!(t.fm.query.is_none());
    assert_eq!(t.fm.rows.len(), 10);
    assert!(t.fm.srch.text.is_empty());
}

#[test]
fn alt_enter_asks_for_properties_of_the_row_or_the_folder() {
    let mut t = T::new("folder");
    t.select("Projects");
    assert!(t.alt(VK_RETURN));
    t.fm.list.sel.clear();
    t.fm.list.cursor = None;
    t.alt(VK_RETURN);
    assert_eq!(t.calls(), [r#"Props("C:\\Projects")"#, r#"Props("C:\\")"#]);
}

#[test]
fn f5_reloads_and_keeps_the_selection_ctrl_h_flips_hidden_files() {
    let mut t = T::new("folder");
    t.select("Windows");
    let era = t.fm.era;
    assert!(t.press(VK_F5));
    assert!(t.fm.era > era, "the folder was listed again");
    assert_eq!(t.one().as_deref(), Some("Windows"));
    assert!(!t.fm.hidden);
    t.ctrl(H);
    assert!(t.fm.hidden);
    t.ctrl(H);
    assert!(!t.fm.hidden);
}

#[test]
fn ctrl_m_switches_between_the_table_and_the_tiles() {
    let mut t = T::new("folder");
    assert_eq!(t.fm.view, View::List);
    t.ctrl(M);
    assert_eq!(t.fm.view, View::Icons);
    t.ctrl(M);
    assert_eq!(t.fm.view, View::List);
}

#[test]
fn the_arrows_home_end_and_shift_move_and_extend_the_selection_ctrl_a_takes_all() {
    let mut t = T::new("folder");
    t.select("PerfLogs");
    t.press(VK_DOWN);
    assert_eq!(t.one().as_deref(), Some("Program Files"));
    t.key(VK_DOWN, false, true, false);
    t.key(VK_DOWN, false, true, false);
    assert_eq!(t.fm.list.sel.len(), 3, "Shift extends from where it started");
    t.press(VK_UP);
    assert_eq!(t.fm.list.sel.len(), 1);
    t.press(VK_END);
    assert_eq!(t.fm.list.cursor, Some(9));
    t.press(VK_HOME);
    assert_eq!(t.fm.list.cursor, Some(0));
    t.ctrl(A);
    assert_eq!(t.fm.list.sel.len(), 10);
    t.press(VK_ESCAPE);
    assert!(t.fm.list.sel.is_empty(), "Esc lets go of the selection");
}

#[test]
fn typing_a_name_jumps_to_it_and_space_is_a_quick_look_at_a_file() {
    let mut t = T::new("folder");
    t.fm.list.sel.clear();
    t.fm.list.cursor = None;
    t.text("w");
    assert_eq!(t.one().as_deref(), Some("Windows"));
    t.text("i");
    t.text("n");
    assert_eq!(t.one().as_deref(), Some("Windows"));
    // a file: Space opens it (and the space character that follows is not a letter to jump to)
    t.select("install.log");
    assert!(t.press(VK_SPACE));
    t.text(" ");
    assert_eq!(t.calls(), [r#"Open("C:\\install.log")"#]);
    assert_eq!(t.one().as_deref(), Some("install.log"));
    // a folder or nothing: Space shows or hides the preview
    t.select("Windows");
    let pv = t.fm.pv;
    t.press(VK_SPACE);
    assert_ne!(t.fm.pv, pv);
}

#[test]
fn a_click_on_a_header_sorts_the_same_column_again_turns_it_over() {
    let mut t = T::new("folder");
    assert_eq!(t.fm.sort, (0, true));
    t.click(ID_COLS + 3, Button::Left); // Size
    assert_eq!(t.fm.sort, (3, true), "ascending first, as before");
    // folders stay first, the files by size
    let files: Vec<_> = t.fm.rows.iter().filter(|r| !r.dir).map(|r| r.name.clone()).collect();
    assert_eq!(files, ["config.toml", "install.log", "appverifUI.dll"]);
    t.click(ID_COLS + 3, Button::Left);
    assert_eq!(t.fm.sort, (3, false));
    let files: Vec<_> = t.fm.rows.iter().filter(|r| !r.dir).map(|r| r.name.clone()).collect();
    assert_eq!(files, ["appverifUI.dll", "install.log", "config.toml"]);
    // the selection stays on its row when the rows move
    t.select("config.toml");
    t.click(ID_COLS + 1, Button::Left);
    assert_eq!(t.one().as_deref(), Some("config.toml"));
    assert_eq!(t.fm.sort.0, 1);
}

#[test]
fn group_by_date_makes_headings_that_are_no_rows() {
    let mut t = T::new("grouped");
    assert!(t.fm.group);
    assert_eq!(t.fm.rows.len(), 10, "the headings are not rows");
    assert_eq!(t.fm.heads.len(), 7);
    assert_eq!(t.fm.heads.iter().map(|h| h.count).sum::<usize>(), 10);
    assert_eq!(t.fm.heads[0].before, 0);
    // the keys and the selection know only rows
    t.press(VK_HOME);
    t.press(VK_DOWN);
    assert_eq!(t.fm.list.cursor, Some(1));
    // the icons view has no headings
    t.ctrl(M);
    assert!(t.fm.heads.is_empty());
}

#[test]
fn the_status_line_counts_folders_files_and_what_is_picked() {
    let mut t = T::new("folder");
    assert_eq!(t.fm.status(), format!("{}, {}  \u{00B7}  выбрано: 1", text::folders(7), text::files(3)));
    t.fm.list.sel.clear();
    assert_eq!(t.fm.status(), format!("{}, {}", text::folders(7), text::files(3)));
    t.select("config.toml");
    assert!(t.fm.status().ends_with("(3 KB)"));
    assert_eq!(text::folders(1), "1 папка");
    assert_eq!(text::folders(2), "2 папки");
    assert_eq!(text::folders(5), "5 папок");
    assert_eq!(text::folders(11), "11 папок");
    assert_eq!(text::folders(21), "21 папка");
    assert_eq!(text::files(22), "22 файла");
    let pc = T::new("this-pc");
    assert_eq!(pc.fm.status(), text::drives(4));
}

#[test]
fn a_right_click_on_a_row_without_the_bar_shows_the_windows_own_menu_and_its_rows_ask_the_machine() {
    let mut t = T::new("folder");
    t.fm.list.sel.clear();
    let i = t.row("config.toml");
    t.click(row_id(ID_LIST, i), Button::Right);
    assert_eq!(t.fm.sel_names(), ["config.toml"], "a right click picks the row it is on");
    let (tag, items) = t.menu.take().expect("a menu");
    assert_eq!(tag, TAG_ITEM);
    let has = |c: usize| items.iter().any(|m| m.value == Some(c));
    assert!(has(I_OPEN) && has(I_CUT) && has(I_COPY) && has(I_RENAME) && has(I_ZIP) && has(I_DELETE) && has(I_PROPS) && has(I_COPY_PATH));
    assert!(!has(I_OPEN_TAB), "only a folder opens in a tab");
    assert!(!has(I_EXTRACT));
    // a folder
    let j = t.row("Projects");
    t.click(row_id(ID_LIST, j), Button::Right);
    let (_, items) = t.menu.take().unwrap();
    assert!(items.iter().any(|m| m.value == Some(I_OPEN_TAB)));
    t.fm.item_command(I_DELETE, &mut Cx::default());
    t.fm.item_command(I_ZIP, &mut Cx::default());
    t.fm.item_command(I_COPY_PATH, &mut Cx::default());
    let calls = t.calls();
    assert_eq!(calls[0], r#"Delete { paths: ["C:\\Projects"], dir: "C:\\" }"#);
    assert_eq!(calls[1], r#"Zip { dir: "C:\\", paths: ["C:\\Projects"] }"#);
    assert_eq!(calls[2], r#"CopyPaths(["C:\\Projects"])"#);
}

/// The fake machine, with the bar there to draw the shell's menu.
struct Barred(Fake);

impl Backend for Barred {
    fn clock(&self) -> Clock {
        self.0.clock()
    }
    fn list(&self, d: &std::path::Path, o: &Opts) -> Option<Vec<Row>> {
        self.0.list(d, o)
    }
    fn sort(&self, r: &mut Vec<Row>, o: &Opts) {
        self.0.sort(r, o);
    }
    fn is_target(&self, p: &std::path::Path) -> bool {
        self.0.is_target(p)
    }
    fn bar(&self) -> bool {
        true
    }
    fn places(&self) -> Places {
        self.0.places()
    }
    fn search(&self, d: &std::path::Path, q: &str, o: &Opts) -> Found {
        self.0.search(d, q, o)
    }
    fn flag(&self, f: Flag) -> bool {
        self.0.flag(f)
    }
    fn set_flag(&self, f: Flag, on: bool) {
        self.0.set_flag(f, on);
    }
    fn new_entry(&self, d: &std::path::Path, f: bool) -> Option<String> {
        self.0.new_entry(d, f)
    }
    fn rename(&self, d: &std::path::Path, a: &str, b: &str) -> Result<(), String> {
        self.0.rename(d, a, b)
    }
    fn real_paths(&self, p: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
        self.0.real_paths(p)
    }
    fn facts(&self, p: &std::path::Path) -> Facts {
        self.0.facts(p)
    }
    fn job(&self) -> Option<JobView> {
        self.0.job()
    }
    fn busy(&self) -> bool {
        self.0.busy()
    }
    fn tool(&self, n: &str) -> bool {
        self.0.tool(n)
    }
    fn era(&self) -> u32 {
        self.0.era()
    }
    fn run(&self, o: isize, d: Do) {
        self.0.run(o, d);
    }
}

#[test]
fn a_right_click_on_a_row_asks_the_bar_when_there_is_one() {
    let barred = Arc::new(Barred(Fake::default()));
    let mut fm = Fm::new(barred.clone(), PathBuf::from(r"C:\"));
    let mut cx = Cx::default();
    fm.load(PathBuf::from(r"C:\"), true, false, &mut cx);
    fm.select_names(&["config.toml".to_string()]);
    let mut cx = Cx::default();
    fm.item_menu(None, &mut cx);
    assert!(cx.reqs.iter().all(|r| !matches!(r, Req::Menu { .. })), "the bar draws it, the window draws nothing");
    assert_eq!(quiet(barred.0.take_calls()), [r#"ItemMenu { dir: "C:\\", paths: ["C:\\config.toml"], at: None }"#]);
    // from the keyboard it opens at the picked row
    let mut cx = Cx::default();
    fm.item_menu(Some((300.0, 200.0)), &mut cx);
    assert!(quiet(barred.0.take_calls())[0].contains("at: Some((300.0, 200.0))"));
    // and the window's own menu comes when the bar did not answer
    let mut cx = Cx::default();
    fm.message(MSG_MENU_OWN, 0, 0, &mut cx);
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Menu { tag: TAG_ITEM, .. })));
}

#[test]
fn the_menu_key_opens_the_folders_menu_when_nothing_is_picked() {
    let mut t = T::new("folder");
    t.fm.list.sel.clear();
    t.press(VK_APPS);
    let (tag, _) = t.menu.take().unwrap();
    assert_eq!(tag, TAG_FOLDER);
    // with a row picked it is the row's
    t.select("config.toml");
    t.press(VK_APPS);
    assert_eq!(t.menu.take().map(|m| m.0), Some(TAG_ITEM));
}

#[test]
fn the_folders_menu_changes_the_view_the_sort_and_the_grouping() {
    let mut t = T::new("folder");
    t.click(ID_MORE, Button::Left);
    let (tag, items) = t.menu.take().unwrap();
    assert_eq!(tag, TAG_FOLDER);
    assert!(items.len() >= 6);
    let mut cx = Cx::default();
    t.fm.folder_command(F_VIEW_ICONS, &mut cx);
    assert_eq!(t.fm.view, View::Icons);
    t.fm.folder_command(F_VIEW_LIST, &mut cx);
    t.fm.folder_command(F_SORT + 3, &mut cx);
    t.fm.folder_command(F_DESC, &mut cx);
    assert_eq!(t.fm.sort, (3, false));
    t.fm.folder_command(F_GROUP, &mut cx);
    assert!(t.fm.group && t.fake.flag(Flag::Group), "the choice is kept for next time");
    assert!(!t.fm.heads.is_empty());
    t.fm.folder_command(F_FOLDERS_FIRST, &mut cx);
    assert!(!t.fm.folders_first && !t.fake.flag(Flag::FoldersFirst));
    t.fm.folder_command(F_NEW_TEXT, &mut cx);
    assert_eq!(t.fm.ren.as_ref().map(|r| r.name.as_str()), Some("New text document.txt"));
    assert_eq!(t.fm.ren.as_ref().map(|r| (r.field.anchor, r.field.caret)), Some((0, "New text document".len())));
    t.fm.folder_command(F_PASTE, &mut cx);
    t.fm.folder_command(F_TERMINAL, &mut cx);
    let calls = t.calls();
    assert!(calls.contains(&r#"Paste("C:\\")"#.to_string()) && calls.contains(&r#"Terminal("C:\\")"#.to_string()));
}

#[test]
fn a_drop_on_a_crumb_a_place_or_a_folder_row_moves_on_a_drive_and_copies_across() {
    let mut t = T::new("folder");
    // the frame's layout, by hand: where things are
    t.fm.geo.content = Rect::new(216.0, 98.0, 700.0, 600.0);
    t.fm.geo.crumbs = vec![(ID_CRUMB, Rect::new(180.0, 60.0, 60.0, 26.0), PathBuf::from(r"D:\"))];
    t.fm.geo.places = vec![(Rect::new(8.0, 200.0, 200.0, 32.0), PathBuf::from(r"C:\Users\Daniiar\Documents"))];
    let windows = t.row("Windows");
    t.fm.geo.rows = vec![(Rect::new(224.0, 400.0, 680.0, 36.0), windows)];
    let from = vec![r"C:\Temp\winhelper.exe".to_string()];
    let mut cx = Cx::default();
    // over empty space of the folder it came from: not a drop
    assert!(!t.fm.drag_over(&[r"C:\~x.txt".to_string()], 500.0, 300.0));
    // over a place on the same drive: Move
    assert!(t.fm.drag_over(&from, 20.0, 210.0));
    assert_eq!(t.fm.drag_effect(&from, false), DropFx::Move);
    assert_eq!(t.fm.drag_effect(&from, true), DropFx::Copy, "Ctrl copies");
    t.fm.drag_effect(&from, false);
    t.fm.drop(from.clone(), 20.0, 210.0, &mut cx);
    // over a crumb that is another drive: Copy
    assert!(t.fm.drag_over(&from, 200.0, 70.0));
    assert_eq!(t.fm.drag_effect(&from, false), DropFx::Copy);
    t.fm.drop(from.clone(), 200.0, 70.0, &mut cx);
    // over a folder row
    assert!(t.fm.drag_over(&from, 400.0, 410.0));
    assert!(matches!(t.fm.drop_hl, Some(DropAt::Row(..))));
    t.fm.drag_effect(&from, false);
    t.fm.drop(from.clone(), 400.0, 410.0, &mut cx);
    // a folder does not go into itself
    assert!(!t.fm.drag_over(&[r"C:\Windows".to_string()], 400.0, 410.0));
    let calls = t.calls();
    assert_eq!(calls[0], r#"DropOn { paths: ["C:\\Temp\\winhelper.exe"], to: "C:\\Users\\Daniiar\\Documents", copy: false }"#);
    assert_eq!(calls[1], r#"DropOn { paths: ["C:\\Temp\\winhelper.exe"], to: "D:\\", copy: true }"#);
    assert_eq!(calls[2], r#"DropOn { paths: ["C:\\Temp\\winhelper.exe"], to: "C:\\Windows", copy: false }"#);
    t.fm.drag_leave(&mut cx);
    assert!(t.fm.drop_hl.is_none());
}

#[test]
fn files_from_another_folder_dropped_on_the_list_land_in_this_folder() {
    let mut t = T::new("folder");
    t.fm.geo.content = Rect::new(216.0, 98.0, 700.0, 600.0);
    let from = vec![r"D:\Photos\IMG_2031.jpg".to_string()];
    assert!(t.fm.drag_over(&from, 500.0, 300.0));
    let mut cx = Cx::default();
    t.fm.drop(from, 500.0, 300.0, &mut cx);
    assert_eq!(t.calls(), [r#"DropOn { paths: ["D:\\Photos\\IMG_2031.jpg"], to: "C:\\", copy: true }"#]);
}

#[test]
fn dragging_rows_out_starts_one_drag_after_four_pixels_and_never_a_click() {
    let mut t = T::new("folder");
    let i = t.row("config.toml");
    let id = row_id(ID_LIST, i);
    let mut cx = Cx::default();
    t.fm.down(id, Button::Left, 300.0, 300.0, &mut cx);
    t.fm.drag(id, 302.0, 301.0, &mut cx);
    assert!(t.calls().is_empty(), "under four pixels it is still a click");
    t.fm.drag(id, 310.0, 301.0, &mut cx);
    t.fm.drag(id, 330.0, 305.0, &mut cx);
    assert_eq!(t.calls(), [r#"Drag(["C:\\config.toml"])"#], "one drag, not one per move");
    assert!(t.fm.dragging);
    t.fm.release(id, &mut cx);
    assert!(t.fm.dragging, "the drag runs on until the machine says it is over");
    t.fm.message(MSG_DRAG_DONE, 0, 0, &mut cx);
    assert!(!t.fm.dragging);
}

#[test]
fn a_press_on_a_picked_row_of_a_selection_waits_for_the_button() {
    let mut t = T::new("folder");
    t.fm.list.click(0, false, false);
    t.fm.list.click(1, true, false);
    let id = row_id(ID_LIST, 1);
    let mut cx = Cx::default();
    t.fm.down(id, Button::Left, 300.0, 300.0, &mut cx);
    assert_eq!(t.fm.list.sel.len(), 2, "it may be the start of dragging them all");
    t.fm.click(id, Button::Left, 300.0, 300.0, &mut cx);
    assert_eq!(t.fm.list.sel.len(), 1, "a click without a drag picks the one row");
}

#[test]
fn a_job_shows_in_the_status_line_and_escape_cancels_it() {
    let mut t = T::new("folder");
    assert!(t.fm.job.is_none());
    *t.fake.job.lock().unwrap() = Some(JobView { label: "x".into(), frac: Some(0.5) });
    t.message(MSG_TICK);
    assert_eq!(t.fm.job.as_ref().and_then(|j| j.frac), Some(0.5));
    t.press(VK_ESCAPE);
    assert_eq!(t.calls(), ["JobCancel"], "Esc stops the job before it clears anything");
    t.click(ID_JOB_CANCEL, Button::Left);
    assert_eq!(t.calls(), ["JobCancel"]);
    // when it is done the window lists the folder again
    *t.fake.job.lock().unwrap() = None;
    t.message(MSG_RELOAD);
    assert!(t.fm.job.is_none());
}

#[test]
fn folder_sizes_fill_the_size_column_when_they_arrive_and_stale_ones_are_dropped() {
    let mut t = T::new("folder");
    let before = t.fm.era;
    let mut cx = Cx::default();
    let msg = |era: u32, name: &str, n: u64| Box::into_raw(Box::new((era, name.to_string(), n))) as usize;
    t.fm.message(crate::WM_DIRSIZE, msg(before, "Windows", 12345), 0, &mut cx);
    let w = t.row("Windows");
    assert_eq!(t.fm.rows[w].size, 12345);
    t.fm.message(crate::WM_DIRSIZE, msg(before.wrapping_sub(1), "Temp", 999), 0, &mut cx);
    let tmp = t.row("Temp");
    assert_eq!(t.fm.rows[tmp].size, 0, "an answer for an earlier listing is dropped");
}

#[test]
fn the_sidebar_places_the_pc_and_drives_open_their_folders() {
    let mut t = T::new("folder");
    let k = t.fm.places.quick.iter().position(|p| p.title == "Pictures").unwrap();
    t.click(ID_PLACE + k as Id, Button::Left);
    assert!(t.dir().ends_with("Pictures"));
    t.click(ID_PC, Button::Left);
    assert!(t.fm.at_pc());
    let d = t.fm.places.drives.iter().position(|d| d.root == PathBuf::from(r"D:\")).unwrap();
    t.click(ID_DRIVE + d as Id, Button::Left);
    assert_eq!(t.dir(), r"D:\");
    // a middle click opens it in a tab, a right click asks for the place's menu
    t.click(ID_PLACE + k as Id, Button::Middle);
    assert_eq!(t.fm.tabs.last().map(|p| p.to_string_lossy().ends_with("Pictures")), Some(true));
    t.click(ID_PLACE, Button::Right);
    assert_eq!(t.menu.take().map(|m| m.0), Some(TAG_PLACE));
}

#[test]
fn pinning_a_folder_from_its_menu_is_asked_of_the_machine() {
    let mut t = T::new("folder");
    t.select("Temp");
    t.fm.item_command(I_PIN, &mut Cx::default());
    assert_eq!(t.calls(), [r#"Pin("C:\\Temp")"#]);
}

#[test]
fn the_toolbar_buttons_do_what_the_keys_do() {
    let mut t = T::new("folder");
    t.select("Projects");
    t.press(VK_RETURN);
    t.click(ID_BACK, Button::Left);
    assert_eq!(t.dir(), r"C:\");
    t.click(ID_FWD, Button::Left);
    assert_eq!(t.dir(), r"C:\Projects");
    t.click(ID_UP, Button::Left);
    assert_eq!(t.dir(), r"C:\");
    t.click(ID_VIEW + 1, Button::Left);
    assert_eq!(t.fm.view, View::Icons);
    t.click(ID_VIEW, Button::Left);
    assert_eq!(t.fm.view, View::List);
    let pv = t.fm.pv;
    t.click(ID_PV, Button::Left);
    assert_ne!(t.fm.pv, pv);
    t.click(ID_NEWTAB, Button::Left);
    assert_eq!(t.fm.tabs.len(), 3);
    t.click(ID_TAB, Button::Left);
    assert_eq!(t.fm.active, 0);
    t.click(ID_TAB_X + 2, Button::Left);
    assert_eq!(t.fm.tabs.len(), 2);
    // the pill's empty part edits the address
    t.click(ID_ADDR_PILL, Button::Left);
    assert!(t.fm.addr_edit);
}

#[test]
fn a_double_click_on_empty_space_goes_up_and_a_double_click_on_a_row_opens_it() {
    let mut t = T::new("folder");
    let i = t.row("Projects");
    let mut cx = Cx::default();
    t.fm.double(row_id(ID_LIST, i), &mut cx);
    assert_eq!(t.dir(), r"C:\Projects");
    t.fm.double(ID_LIST, &mut cx);
    assert_eq!(t.dir(), r"C:\");
}

#[test]
fn the_empty_folder_and_the_empty_search_have_a_button_that_makes_or_clears() {
    let mut t = T::new("empty");
    assert!(t.fm.rows.is_empty());
    t.click(ID_EMPTY_BTN, Button::Left);
    assert_eq!(t.calls(), ["new_entry New folder"]);
    let mut t = T::new("nothing-found");
    assert!(t.fm.searching() && t.fm.rows.is_empty());
    t.click(ID_EMPTY_BTN, Button::Left);
    assert!(!t.fm.searching());
}

#[test]
fn this_pc_sorts_its_drives_and_has_the_drives_menu() {
    let mut t = T::new("this-pc");
    assert_eq!(t.names(), [r"C:\", r"D:\", r"E:\", r"Z:\"]);
    t.click(ID_COLS + 1, Button::Left); // free space, the least first
    assert_eq!(t.names().first().map(String::as_str), Some(r"E:\"));
    // the drives and their rows stay in step
    assert_eq!(t.fm.drives[0].root, PathBuf::from(r"E:\"));
    t.fm.list.click(0, false, false);
    t.fm.item_menu(None, &mut Cx::default());
}

#[test]
fn the_first_listing_of_a_window_does_not_slide_but_the_next_ones_do() {
    let mut t = T::new("folder");
    assert!(!t.fm.nav_fresh && t.fm.nav_at == 0);
    t.fm.hwnd = 1; // the window exists now
    let mut cx = Cx::default();
    t.fm.load(PathBuf::from(r"C:\Windows"), true, false, &mut cx);
    assert!(t.fm.nav_fresh);
}

#[test]
fn eject_is_asked_of_the_machine_and_the_user_is_told_what_happens() {
    let mut t = T::new("folder");
    let k = t.fm.places.drives.iter().position(|d| d.removable).unwrap();
    t.click(ID_EJECT + k as Id, Button::Left);
    assert_eq!(t.calls(), [r#"Eject("E:\\")"#]);
    assert_eq!(t.toasts.len(), 1);
}

#[test]
fn a_folder_that_cannot_be_read_leaves_the_window_as_it_was() {
    let mut t = T::new("folder");
    let mut cx = Cx::default();
    t.fm.go(PathBuf::from(r"C:\no\such"), &mut cx);
    assert_eq!(t.dir(), r"C:\");
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Toast { .. })));
}

#[test]
fn a_new_tab_remembers_only_the_folder_like_before() {
    let mut t = T::new("folder");
    t.select("Windows");
    t.ctrl(T_);
    t.fm.tab_select(0, &mut Cx::default());
    assert_eq!(t.fm.tabs[2], PathBuf::from(r"C:\"));
    assert!(t.fm.list.sel.is_empty(), "no selection or scroll is kept per tab");
}

#[test]
fn drives_nearly_full_are_told_apart_by_their_percentage() {
    let t = T::new("folder");
    let z = t.fm.places.drives.iter().find(|d| d.root == PathBuf::from(r"Z:\")).unwrap();
    assert!(z.used_pct() >= 90);
    let c = t.fm.places.drives.iter().find(|d| d.root == PathBuf::from(r"C:\")).unwrap();
    assert!(c.used_pct() < 90);
}

/// Scrolls the list of `t` to `y` at once, as a user's wheel and a few frames would leave it.
fn scrolled_to(t: &mut T, y: f32) {
    t.fm.list.scroll.set_extent(10_000.0 * 37.0, 600.0);
    t.fm.list.scroll.scroll_to(y, 0);
    t.fm.list.scroll.settle();
}

fn many(t: &T) -> usize {
    t.fm.rows.len()
}

#[test]
fn a_refresh_of_the_same_folder_keeps_the_scroll_and_a_deleted_row_hands_the_pick_to_its_neighbour() {
    let mut t = T::new("many");
    assert_eq!(many(&t), 10_000);
    scrolled_to(&mut t, 5000.0 * 37.0);
    t.fm.list.shown = t.fm.list.cursor; // what the frames did
    t.select("file 05001.txt");
    t.fm.list.shown = t.fm.list.cursor;
    let at = t.fm.list.scroll.target();
    let i = t.row("file 05001.txt");
    // another program deletes it: the watcher's reload
    t.fake.fs.lock().unwrap().get_mut(&PathBuf::from(r"C:\Many")).unwrap().retain(|r| r.name != "file 05001.txt");
    t.press(VK_F5);
    assert_eq!(t.fm.list.scroll.target(), at, "the list stays where it was");
    assert_eq!(t.fm.list.sel.len(), 1);
    assert_eq!(t.one().as_deref(), Some(t.fm.rows[i].name.as_str()), "the next item took the deleted one's place");
    assert_eq!(t.fm.list.cursor, Some(i));
    // the last row deleted: the one before it
    let n = many(&t);
    t.fm.list.click(n - 1, false, false);
    let last = t.fm.rows[n - 1].name.clone();
    t.fake.fs.lock().unwrap().get_mut(&PathBuf::from(r"C:\Many")).unwrap().retain(|r| r.name != last);
    t.press(VK_F5);
    assert_eq!(t.fm.list.cursor, Some(n - 2));
    // a plain refresh keeps the pick and the offset
    let (sel, off) = (t.fm.sel_names(), t.fm.list.scroll.target());
    t.press(VK_F5);
    assert_eq!((t.fm.sel_names(), t.fm.list.scroll.target()), (sel, off));
    // another folder starts at the top
    let mut cx = Cx::default();
    t.fm.go(PathBuf::from(r"C:\Windows"), &mut cx);
    assert_eq!(t.fm.list.scroll.target(), 0.0);
}

#[test]
fn show_in_folder_asks_for_the_row_to_be_centred_when_it_is_off_screen() {
    let mut t = T::new("many");
    t.fm.reveal_names(&["file 05001.txt".to_string()]);
    assert!(t.fm.list.center_next && t.fm.list.shown.is_none());
    assert_eq!(t.one().as_deref(), Some("file 05001.txt"));
    // the host's frame: the list brings the row in, in the middle, with no glide
    let mut l = std::mem::take(&mut t.fm.list);
    l.scroll.set_extent(10_000.0 * 37.0, 600.0);
    let y = 5000.0 * 37.0;
    l.bring(y, 36.0, 0);
    assert!((l.scroll.offset(0) - (y + 18.0 - 300.0)).abs() < 0.5, "{}", l.scroll.offset(0));
    assert!(!l.center_next);
    // already in view: it stays
    let off = l.scroll.target();
    l.center_next = true;
    l.bring(off + 100.0, 36.0, 0);
    assert_eq!(l.scroll.target(), off);
}

#[test]
fn the_refresh_button_reloads_like_f5_and_keeps_the_pick() {
    let mut t = T::new("folder");
    t.select("Windows");
    let era = t.fm.era;
    t.click(ID_REFRESH, Button::Left);
    assert!(t.fm.era > era, "the folder was listed again");
    assert_eq!(t.one().as_deref(), Some("Windows"));
}
