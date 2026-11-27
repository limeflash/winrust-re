//! What the review of the deletion path found, each as a test that fails without its fix: the order in which answers, toasts and a new
//! tree arrive, and what the deleter is handed. Same machinery as `tests.rs`, plus a machine whose workers run when the test says.
use std::{path::PathBuf, sync::Arc};

use winrust_common::mft::ROOT;
use winrust_ui::{
    core::App,
    frame::{Button, Cx, Id},
};

use crate::{
    app::*,
    backend::{Backend, DriveInfo},
    delete::How,
    demo::{enter, find, scanned, Fake},
    model::DiskData,
};

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

fn click(d: &mut Disk, id: Id, b: Button) {
    d.click(id, b, 100.0, 100.0, &mut Cx::default());
    d.ensure_layout(MAP);
}

fn deletes(f: &Fake) -> usize {
    f.log.lock().unwrap().deletes.len()
}

/// A machine whose workers run when the test says so, in the order it says, and whose shell can be told to refuse.
struct Defer {
    inner: Fake,
    jobs: std::sync::Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    refuse: bool,
}

impl Defer {
    fn new(refuse: bool) -> Arc<Self> {
        Arc::new(Self { inner: Fake::new(true), jobs: std::sync::Mutex::new(Vec::new()), refuse })
    }

    /// Runs the `n`th job that has been given and not run (counting from 0).
    fn run(&self, n: usize) {
        let job = self.jobs.lock().unwrap().remove(n);
        job();
    }

    fn waiting(&self) -> usize {
        self.jobs.lock().unwrap().len()
    }
}

impl Backend for Defer {
    fn drives(&self) -> Vec<DriveInfo> {
        self.inner.drives()
    }
    fn is_admin(&self) -> bool {
        true
    }
    fn scan(&self, drive: char, slow: bool, progress: &mut dyn FnMut(u64, u64)) -> Result<DiskData, String> {
        self.inner.scan(drive, slow, progress)
    }
    fn delete(&self, paths: &[PathBuf], how: How, confirmed: bool, owner: isize) -> Result<(), String> {
        let r = self.inner.delete(paths, how, confirmed, owner);
        if self.refuse { Err("access denied".into()) } else { r }
    }
    fn reveal(&self, _: &str) {}
    fn restart_elevated(&self, _: char) -> bool {
        false
    }
    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        self.jobs.lock().unwrap().push(job);
    }
}

fn deferred(refuse: bool) -> (Disk, Arc<Defer>) {
    let be = Defer::new(refuse);
    let mut d = Disk::new(be.clone() as Arc<dyn Backend>, Some('C'));
    d.data = Some(crate::demo::mock_data('C'));
    d.phase = Phase::Ready;
    d.ensure_layout(MAP);
    (d, be)
}

/// Delete `name` of the level on screen to the Recycle Bin through the dialog.
fn bin(d: &mut Disk, name: &str) {
    d.ask_delete(find(d, name).unwrap());
    click(d, ID_DLG_OK, Button::Left);
}

fn for_good(d: &mut Disk, name: &str) {
    d.ask_delete(find(d, name).unwrap());
    click(d, ID_DLG_OPT, Button::Left);
    click(d, ID_DLG_OK, Button::Left);
}

fn said(d: &Disk) -> Option<(String, bool)> {
    d.toast.as_ref().map(|t| (t.text.clone(), t.undo))
}

#[test]
fn the_toast_with_undo_stays_while_its_deletion_is_armed_and_what_else_there_is_to_say_waits() {
    let (mut d, f) = disk();
    let users = find(&d, "Users").unwrap();
    go(&mut d, &["Games"]);
    bin(&mut d, "Call of Duty");
    let used = d.data.as_ref().unwrap().used();
    let armed = Some(("Call of Duty moved to the Recycle Bin".to_string(), true));
    assert_eq!(said(&d), armed);
    // a refusal of the dialog, Copy path: neither takes the Undo away
    d.ask_delete(users);
    d.say_for_test("Path copied");
    assert_eq!(said(&d), armed, "the Undo is still there to be clicked");
    assert_eq!(deletes(&f), 0);
    // Undo is what it says, and then what waited is said (the newest)
    click(&mut d, ID_TOAST_UNDO, Button::Left);
    assert!(d.data.as_ref().unwrap().used() > used, "it is back");
    assert_eq!(said(&d), Some(("Path copied".to_string(), false)));
    assert_eq!(deletes(&f), 0);
    // when the bin gets it instead, the words wait for that
    bin(&mut d, "Call of Duty");
    d.say_for_test("Path copied");
    assert_eq!(said(&d), armed);
    d.flush_now();
    assert_eq!(said(&d), Some(("Path copied".to_string(), false)), "once it is sent");
    assert_eq!(deletes(&f), 1);
}

#[test]
fn the_answer_of_another_deletion_is_not_taken_for_the_one_in_flight() {
    let (mut d, be) = deferred(false);
    go(&mut d, &["Games"]);
    // the bin's job goes out, and then a deletion for good: the bin's answer comes first
    bin(&mut d, "Call of Duty");
    d.flush_now();
    for_good(&mut d, "Cyberpunk 2077");
    assert_eq!((be.waiting(), d.busy().is_some()), (2, true));
    be.run(0);
    d.take_events();
    assert!(d.busy().is_some(), "the job for good is still the one in flight");
    be.run(0);
    d.take_events();
    assert_eq!((find(&d, "Cyberpunk 2077"), d.busy()), (None, None), "and its answer is heard");
    assert_eq!(said(&d), Some(("66.2 GB freed · Cyberpunk 2077 deleted".to_string(), false)));
}

#[test]
fn a_rescan_sends_what_waits_for_its_toast_and_no_old_undo_or_answer_reaches_the_new_tree() {
    // Undo after a rescan
    let (mut d, f) = disk();
    go(&mut d, &["Games"]);
    let whole = d.data.as_ref().unwrap().used();
    bin(&mut d, "Call of Duty");
    d.start_scan();
    d.take_events();
    assert_eq!(deletes(&f), 1, "it went to the bin, against the tree it was taken from");
    assert!(d.toast.is_none(), "and there is no Undo to click");
    click(&mut d, ID_TOAST_UNDO, Button::Left);
    assert_eq!(d.data.as_ref().unwrap().used(), whole, "the new tree is not given the old one's folder again");
    // the shell's refusal of that, after the new tree is in
    let (mut d, be) = deferred(true);
    go(&mut d, &["Games"]);
    bin(&mut d, "Call of Duty");
    d.start_scan();
    assert_eq!(be.waiting(), 2);
    be.run(1); // the scan is answered first
    d.take_events();
    let whole = d.data.as_ref().unwrap().used();
    be.run(0); // then the bin says no
    d.take_events();
    assert_eq!(d.data.as_ref().unwrap().used(), whole, "its folder is not restored into a tree it was never in");
    assert!(d.toast.as_ref().is_some_and(|t| !t.ok && t.text.contains("Recycle Bin")), "but the user is told");
}

#[test]
fn another_drive_does_not_take_a_deletion_for_good_in_flight_for_its_own() {
    let (mut d, be) = deferred(false);
    go(&mut d, &["Games"]);
    let (free_c, free_d) = (d.drives[0].free, d.drives[1].free);
    let size = d.data.as_ref().unwrap().size(find(&d, "Call of Duty").unwrap());
    for_good(&mut d, "Call of Duty");
    assert!(d.key(0x09, Default::default(), &mut Cx::default()), "Tab: the next drive");
    be.run(1); // D: is read
    d.take_events();
    let used_d = d.data.as_ref().unwrap().used();
    be.run(0); // and then C:'s deletion is done
    d.take_events();
    assert_eq!(d.data.as_ref().unwrap().used(), used_d, "nothing is taken off D:");
    assert_eq!((d.drives[0].free, d.drives[1].free), (free_c + size, free_d), "what was freed is C:'s");
}

#[test]
fn a_path_with_a_folder_lost_on_the_way_says_so_and_is_not_asked_about() {
    let (mut d, f) = disk();
    let rec = |parent, name: &str, dir, alloc| winrust_common::mft::Rec { parent, name: name.into(), dir, alloc, logical: alloc };
    let mut r = std::collections::HashMap::new();
    r.insert(ROOT, rec(ROOT, ".", true, 0));
    r.insert(10, rec(999, "lost", true, 0)); // its parent is not in the table
    r.insert(11, rec(10, "f.bin", false, 500));
    d.data = Some(DiskData::read(crate::model::build(r), 'C'));
    d.ask_delete(11);
    assert!(d.dlg.is_none() && deletes(&f) == 0);
    let text = said(&d).map(|s| s.0).unwrap_or_default();
    assert!(text.contains("incomplete") && !text.contains("Windows needs"), "{text}");
}

#[test]
fn the_shells_own_questions_are_modal_to_our_window() {
    let (mut d, f) = disk();
    d.start(4242); // the window exists (a number: nothing is posted anywhere that matters)
    go(&mut d, &["Games"]);
    bin(&mut d, "Call of Duty");
    d.flush_now();
    for_good(&mut d, "Cyberpunk 2077");
    d.take_events();
    assert_eq!(f.log.lock().unwrap().owners, vec![4242, 4242]);
}
