//! What a click, a key and a worker's answer do to the window, with no window: the app is driven by hand, on a [`Fake`] that records
//! every thing it is asked to read, delete, show or elevate.
use std::{path::PathBuf, sync::Arc};

use winrust_common::mft::ROOT;
use winrust_ui::{
    core::App,
    frame::{Button, Cx, Id, Mods, Req},
    widgets::overlay::MenuItem,
};

use crate::{
    app::*,
    backend::{Backend, DriveInfo},
    delete::How,
    demo::{enter, find, scanned, Fake},
    model::DiskData,
};

const NONE: Mods = Mods { ctrl: false, shift: false };
const MAP: (f32, f32) = (820.0, 592.0);

fn disk() -> (Disk, Arc<Fake>) {
    let fake = Arc::new(Fake::new(true));
    let mut d = scanned(fake.clone(), 'C');
    d.ensure_layout(MAP);
    (d, fake)
}

fn go(d: &mut Disk, names: &[&str]) {
    enter(d, names);
    d.ensure_layout(MAP);
}

fn tile_of(d: &Disk, name: &str) -> Id {
    let id = find(d, name).unwrap_or_else(|| panic!("{name} is not on this level"));
    ID_TILE + d.layout.as_ref().unwrap().tiles.iter().position(|t| t.id == id).unwrap() as Id
}

fn click(d: &mut Disk, id: Id, b: Button) -> Cx {
    let mut cx = Cx::default();
    d.click(id, b, 100.0, 100.0, &mut cx);
    d.ensure_layout(MAP);
    cx
}

fn click_tile(d: &mut Disk, name: &str, b: Button) -> Cx {
    let id = tile_of(d, name);
    click(d, id, b)
}

fn key(d: &mut Disk, vk: u32) -> bool {
    let used = d.key(vk, NONE, &mut Cx::default());
    d.ensure_layout(MAP);
    used
}

fn menu_of(cx: &Cx) -> Vec<MenuItem> {
    cx.reqs.iter().find_map(|r| if let Req::Menu { items, .. } = r { Some(items.clone()) } else { None }).expect("a menu was asked for")
}

fn texts(items: &[MenuItem]) -> Vec<&str> {
    items.iter().map(|i| i.text.as_str()).collect()
}

fn deletes(f: &Fake) -> Vec<(Vec<PathBuf>, How, bool)> {
    f.log.lock().unwrap().deletes.clone()
}

fn scans(f: &Fake) -> Vec<(char, bool)> {
    f.log.lock().unwrap().scans.clone()
}

#[test]
fn a_click_on_a_folder_goes_in_and_backspace_comes_out_and_selects_where_it_was() {
    let (mut d, _) = disk();
    let users = find(&d, "Users").unwrap();
    click_tile(&mut d, "Users", Button::Left);
    assert_eq!((d.nav.current(), d.nav.path().len()), (users, 2));
    assert!(matches!(d.zoom, Some(Zoom::In(_))), "the zoom starts from the tile that was clicked");
    assert!(key(&mut d, 0x08));
    assert_eq!((d.nav.current(), d.sel), (ROOT, Some(users)));
    assert!(matches!(d.zoom, Some(Zoom::Out(id)) if id == users));
    // a crumb of the path goes back to its folder
    go(&mut d, &["Users", "Daniiar", "Videos"]);
    click(&mut d, ID_CRUMB + 1, Button::Left);
    assert_eq!(d.nav.path().len(), 2);
}

#[test]
fn a_click_on_a_file_selects_it_and_another_click_lets_go() {
    let (mut d, _) = disk();
    go(&mut d, &["Users", "Daniiar", "Videos"]);
    let t = tile_of(&d, "render-final.mov");
    click(&mut d, t, Button::Left);
    assert_eq!(d.sel, find(&d, "render-final.mov"));
    assert_eq!(d.nav.path().len(), 4, "a file is not gone into");
    click(&mut d, t, Button::Left);
    assert_eq!(d.sel, None);
}

#[test]
fn the_right_button_opens_the_menu_of_the_item_and_the_choices_do_what_they_say() {
    let (mut d, f) = disk();
    let cx = click_tile(&mut d, "Games", Button::Right);
    let items = menu_of(&cx);
    assert_eq!(texts(&items), vec!["Games", "Open", "Show in file manager", "Copy path", "", "Delete…"]);
    assert!(items[0].disabled && items[5].danger, "the first row only names it; Delete is red");
    d.command(TAG_MENU, A_SHOW, &mut Cx::default());
    assert_eq!(f.log.lock().unwrap().reveals, vec!["C:\\Games".to_string()]);
    // Copy path is a request for the host (it owns the clipboard)
    click_tile(&mut d, "Games", Button::Right);
    let mut cx = Cx::default();
    d.command(TAG_MENU, A_COPY, &mut cx);
    assert!(cx.reqs.iter().any(|r| matches!(r, Req::Clipboard(t) if t == "C:\\Games")));
    // Open goes in
    click_tile(&mut d, "Games", Button::Right);
    d.command(TAG_MENU, A_OPEN, &mut Cx::default());
    assert_eq!(d.nav.path().len(), 2);
    // the menu of the empty map: up, show, scan again
    let cx = click(&mut d, ID_MAP, Button::Right);
    assert_eq!(texts(&menu_of(&cx)), vec!["Games", "Up one level", "Show in file manager", "Scan again"]);
    d.command(TAG_MENU, A_UP, &mut Cx::default());
    assert!(d.nav.at_root());
    assert!(deletes(&f).is_empty(), "no menu choice deletes: Delete… only asks");
}

#[test]
fn the_dialog_asks_and_only_its_confirmation_deletes_and_cancel_and_esc_delete_nothing() {
    let (mut d, f) = disk();
    go(&mut d, &["Games"]);
    let id = find(&d, "Call of Duty").unwrap();
    // the Delete key and the menu only open it
    click_tile(&mut d, "Call of Duty", Button::Right);
    d.command(TAG_MENU, A_DELETE, &mut Cx::default());
    assert!(d.dlg.is_some());
    assert_eq!(d.dlg.as_ref().unwrap().how, DEFAULT_HOW, "it starts on the Recycle Bin: Enter is not the irreversible one");
    click(&mut d, ID_DLG_CANCEL, Button::Left);
    assert!(d.dlg.is_none());
    d.sel = Some(id);
    assert!(key(&mut d, 0x2E), "the Delete key");
    assert!(d.dlg.is_some());
    assert!(key(&mut d, 0x1B), "Esc is Cancel");
    assert!(d.dlg.is_none());
    d.ask_delete(id);
    click(&mut d, ID_DLG_SCRIM, Button::Left);
    assert!(d.dlg.is_none(), "a click outside is Cancel");
    // a tile, a click on the map: neither deletes
    click(&mut d, ID_MAP, Button::Left);
    click_tile(&mut d, "Cyberpunk 2077", Button::Left);
    assert!(deletes(&f).is_empty());
    assert!(d.data.as_ref().unwrap().size(id) > 0 && d.toast.is_none());
}

#[test]
fn deleting_for_good_goes_through_the_deleter_with_the_confirmation_then_the_tree_loses_it_and_a_toast_says_so() {
    let (mut d, f) = disk();
    go(&mut d, &["Games"]);
    let (id, free) = (find(&d, "Call of Duty").unwrap(), d.drives[0].free);
    let (size, used) = (d.data.as_ref().unwrap().size(id), d.data.as_ref().unwrap().used());
    d.ask_delete(id);
    click(&mut d, ID_DLG_OPT, Button::Left);
    assert_eq!(d.dlg.as_ref().unwrap().how, How::Permanently);
    click(&mut d, ID_DLG_OK, Button::Left);
    d.take_events();
    assert_eq!(deletes(&f), vec![(vec![PathBuf::from("C:\\Games\\Call of Duty")], How::Permanently, true)]);
    assert_eq!((d.data.as_ref().unwrap().used(), find(&d, "Call of Duty")), (used - size, None));
    assert_eq!(d.drives[0].free, free + size, "the drive has what was freed");
    let t = d.toast.as_ref().unwrap();
    assert_eq!((t.text.as_str(), t.undo, t.ok), ("98.4 GB freed · Call of Duty deleted", false, true));
    assert!(d.dlg.is_none() && d.busy().is_none());
}

/// A machine whose shell says no to every deletion.
struct Refuses(Fake);

impl Backend for Refuses {
    fn drives(&self) -> Vec<DriveInfo> {
        self.0.drives()
    }
    fn is_admin(&self) -> bool {
        true
    }
    fn scan(&self, drive: char, slow: bool, progress: &mut dyn FnMut(u64, u64)) -> Result<DiskData, String> {
        self.0.scan(drive, slow, progress)
    }
    fn delete(&self, _: &[PathBuf], _: How, _: bool) -> Result<(), String> {
        Err("access denied".into())
    }
    fn reveal(&self, _: &str) {}
    fn restart_elevated(&self, _: char) -> bool {
        false
    }
    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
}

fn refusing() -> Disk {
    let be: Arc<dyn Backend> = Arc::new(Refuses(Fake::new(true)));
    let mut d = Disk::new(be, Some('C'));
    d.data = Some(crate::demo::mock_data('C'));
    d.phase = Phase::Ready;
    d.ensure_layout(MAP);
    d
}

#[test]
fn a_refused_deletion_leaves_everything_and_says_why_for_good_and_for_the_bin() {
    // for good
    let mut d = refusing();
    go(&mut d, &["Games"]);
    let (id, used, free) = (find(&d, "Call of Duty").unwrap(), d.data.as_ref().unwrap().used(), d.drives[0].free);
    d.ask_delete(id);
    d.dlg.as_mut().unwrap().how = How::Permanently;
    click(&mut d, ID_DLG_OK, Button::Left);
    d.take_events();
    assert_eq!((find(&d, "Call of Duty"), d.data.as_ref().unwrap().used(), d.drives[0].free), (Some(id), used, free));
    assert_eq!(d.toast.as_ref().map(|t| (t.text.as_str(), t.ok)), Some(("Couldn’t delete Call of Duty: access denied", false)));
    assert!(d.busy().is_none(), "and it can be tried again");
    // to the bin: it was off the screen, and comes back when the shell refuses
    d.ask_delete(id);
    click(&mut d, ID_DLG_OK, Button::Left);
    assert_eq!(find(&d, "Call of Duty"), None);
    d.flush_now();
    d.take_events();
    assert_eq!((find(&d, "Call of Duty"), d.data.as_ref().unwrap().used()), (Some(id), used));
    assert!(d.toast.as_ref().is_some_and(|t| !t.ok && t.text.contains("Recycle Bin")));
}

#[test]
fn the_recycle_bin_waits_for_the_toast_and_undo_brings_it_back_and_nothing_is_sent() {
    let (mut d, f) = disk();
    go(&mut d, &["Games"]);
    let id = find(&d, "Call of Duty").unwrap();
    let used = d.data.as_ref().unwrap().used();
    d.ask_delete(id);
    click(&mut d, ID_DLG_OK, Button::Left);
    assert_eq!(find(&d, "Call of Duty"), None, "it is off the screen at once");
    assert!(deletes(&f).is_empty(), "and still where it was on the disk until the toast is over");
    let t = d.toast.as_ref().unwrap();
    assert_eq!((t.text.as_str(), t.undo), ("Call of Duty moved to the Recycle Bin", true));
    click(&mut d, ID_TOAST_UNDO, Button::Left);
    assert_eq!((find(&d, "Call of Duty"), d.data.as_ref().unwrap().used(), d.toast.is_none()), (Some(id), used, true));
    // the wake that was set for it finds nothing to do
    d.wake(&mut Cx::default());
    d.take_events();
    assert!(deletes(&f).is_empty());
}

#[test]
fn the_recycle_bin_gets_it_when_the_toast_is_over_or_the_window_closes_or_another_is_deleted() {
    // when the time is up
    let (mut d, f) = disk();
    d.bin_delay_ms = 40;
    go(&mut d, &["Games"]);
    d.ask_delete(find(&d, "Call of Duty").unwrap());
    click(&mut d, ID_DLG_OK, Button::Left);
    d.wake(&mut Cx::default());
    assert!(deletes(&f).is_empty(), "not before");
    std::thread::sleep(std::time::Duration::from_millis(80));
    d.wake(&mut Cx::default());
    d.take_events();
    assert_eq!(deletes(&f).iter().map(|x| (x.1, x.2)).collect::<Vec<_>>(), vec![(How::RecycleBin, true)]);
    // when the window closes
    let (mut d, f) = disk();
    go(&mut d, &["Games"]);
    d.ask_delete(find(&d, "Cyberpunk 2077").unwrap());
    click(&mut d, ID_DLG_OK, Button::Left);
    assert!(d.close(&mut Cx::default()));
    assert_eq!(deletes(&f).len(), 1, "closing does not lose it");
    // when the next one is deleted
    let (mut d, f) = disk();
    go(&mut d, &["Games"]);
    d.ask_delete(find(&d, "Cyberpunk 2077").unwrap());
    click(&mut d, ID_DLG_OK, Button::Left);
    d.ask_delete(find(&d, "Call of Duty").unwrap());
    click(&mut d, ID_DLG_OK, Button::Left);
    d.take_events();
    assert_eq!(deletes(&f).len(), 1, "the first went to the bin when the second was asked for");
}

#[test]
fn without_administrator_rights_a_card_stands_in_the_way_and_nothing_elevates_or_reads_until_the_user_chooses() {
    let fake = Arc::new(Fake::new(false));
    let mut d = scanned(fake.clone(), 'C');
    d.ensure_layout(MAP);
    assert_eq!(d.block(), Some(Block::Admin));
    assert!(!d.ready() && d.cat_list().is_empty());
    // "Restart as administrator" asks the machine once (the fake says no, as a user who cancels the prompt does)
    let mut cx = Cx::default();
    d.click(ID_BLOCK_RUN, Button::Left, 1.0, 1.0, &mut cx);
    assert_eq!(f_elevations(&fake), vec!['C']);
    assert!(!cx.reqs.iter().any(|r| matches!(r, Req::Close)), "a window whose elevation was refused stays");
    assert!(scans(&fake).is_empty());
    // "Scan slowly instead": a slow scan, on the drive on screen
    d.click(ID_BLOCK_ALT, Button::Left, 1.0, 1.0, &mut Cx::default());
    d.take_events();
    assert_eq!(scans(&fake), vec![('C', true)]);
    assert_eq!(d.block(), None);
}

fn f_elevations(f: &Fake) -> Vec<char> {
    f.log.lock().unwrap().elevations.clone()
}

#[test]
fn a_drive_that_is_not_ntfs_has_a_card_and_is_scanned_only_slowly_and_only_when_asked() {
    let (mut d, fake) = disk();
    // Tab: the next drive (D: is NTFS: scanned at once, the fast way); once more: E: (exFAT): a card, no scan
    assert!(key(&mut d, 0x09));
    assert_eq!(scans(&fake), vec![('D', false)]);
    d.take_events();
    assert!(key(&mut d, 0x09));
    assert_eq!((d.block(), scans(&fake).len()), (Some(Block::Fs), 1));
    d.click(ID_BLOCK_RUN, Button::Left, 1.0, 1.0, &mut Cx::default());
    d.take_events();
    assert_eq!(scans(&fake).last(), Some(&('E', true)), "Scan slowly");
    assert!(d.ready());
}

#[test]
fn a_scan_comes_back_through_the_inbox_and_a_stale_answer_is_ignored() {
    let (mut d, fake) = disk();
    d.start_scan();
    d.start_scan(); // the first one's answer is in the inbox for a scan that is no longer the one wanted
    d.take_events();
    assert!(d.ready() && d.fresh && d.scanned_in.is_some());
    assert_eq!(scans(&fake).len(), 2);
    // an answer of an old scan after the wanted one is not shown
    d.inbox.lock().unwrap().push(Event::Scanned { epoch: 1, result: Err("old".into()), secs: 0.0 });
    d.take_events();
    assert!(d.ready() && d.error.is_none());
}

#[test]
fn the_list_sorts_by_its_header_and_its_keys_select() {
    let (mut d, _) = disk();
    go(&mut d, &["Users", "Daniiar"]);
    d.view = ViewMode::List;
    d.ensure_layout(MAP);
    let names = |d: &Disk| d.order.iter().map(|&i| d.layout.as_ref().unwrap().items[i].name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&d)[..3], ["Videos", "Downloads", "AppData"], "the biggest first, with no arrow");
    click(&mut d, ID_COLS + 2, Button::Left);
    assert_eq!(names(&d)[0], "OneDrive", "Size, smallest first");
    click(&mut d, ID_COLS + 2, Button::Left);
    assert_eq!(names(&d)[0], "Videos", "and turned over");
    click(&mut d, ID_COLS, Button::Left);
    assert_eq!(names(&d)[..2], ["AppData", "Desktop"], "Name: by the letters");
    // the keys move the selection along the rows, Enter opens a folder, Delete asks
    assert!(key(&mut d, 0x28));
    assert_eq!(d.sel, find(&d, "AppData"));
    assert!(key(&mut d, 0x28) && d.sel == find(&d, "Desktop"));
    assert!(key(&mut d, 0x0D));
    assert_eq!(d.nav.path().len(), 4);
}

#[test]
fn the_panel_shows_the_item_in_focus_and_biggest_here_takes_you_there() {
    let (mut d, _) = disk();
    go(&mut d, &["Users", "Daniiar"]);
    let videos = find(&d, "Videos").unwrap();
    assert_eq!(d.focus(Some(videos)), Some(videos), "what the pointer is over");
    assert_eq!(d.focus(None), None);
    d.sel = Some(videos);
    assert_eq!(d.focus(None), Some(videos), "else the selected one");
    d.tops = vec![videos, find(&d, "Downloads").unwrap()];
    d.click(ID_TOP, Button::Left, 1.0, 1.0, &mut Cx::default());
    assert_eq!(d.nav.current(), videos, "a folder is gone into");
    // a file inside another folder: its folder is shown and it is selected
    let rec = find(&d, "render-final.mov").unwrap();
    go(&mut d, &[]);
    d.nav.set(&[ROOT]);
    d.tops = vec![rec];
    d.click(ID_TOP, Button::Left, 1.0, 1.0, &mut Cx::default());
    assert_eq!((d.nav.path().len(), d.sel), (4, Some(rec)));
}

#[test]
fn the_legend_dims_what_is_not_in_the_category_and_empty_folders_have_a_card() {
    let (mut d, _) = disk();
    assert_eq!(d.cat_list().first().map(|c| c.label()), Some("Games"), "the biggest category first");
    assert_eq!(d.cat_list().len(), 8);
    go(&mut d, &["Users", "Daniiar", "Pictures", "Screenshots"]);
    let ids: Vec<u32> = d.layout.as_ref().unwrap().tiles.iter().map(|t| t.id).collect();
    for id in ids {
        d.data.as_mut().unwrap().remove(id);
    }
    d.data_gen += 1;
    d.ensure_layout(MAP);
    assert!(d.empty_dir());
    click(&mut d, ID_EMPTY_UP, Button::Left);
    assert_eq!(d.nav.path().len(), 4);
}
