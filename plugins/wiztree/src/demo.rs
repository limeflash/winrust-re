//! WinRust Disk in the states of the design (`--render state=<name>`, `--bench`, `--selftest`, the tests): drives and a tree made up to
//! look like the mockup's (DiskMap.html: its `drives()` and its `gen` (`made_up` here), ported), and a [`Fake`] backend that reads and deletes nothing.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use winrust_common::mft::{Rec, ROOT};
use winrust_ui::core::App;

use crate::{
    app::{Disk, DemoHook, Phase, Scan, ViewMode},
    backend::{Backend, DriveInfo},
    delete::How,
    model::{build, Category, DiskData, N_CATS},
};

/// What `--render-all` goes through: the states of the design's "Preview states" list (the others it names are these again:
/// "Done" and "None" are `drive-map`; "Normal" and "Maximized" are `drive-map` with `max`).
pub const STATES: [&str; 14] = [
    "drive-map", "inside-folder", "deep-folder", "list-view", "file-selected", "category-highlight", "context-menu", "delete-dialog", "deleted-toast", "recycled-toast",
    "scan-reading", "scan-frozen", "needs-admin", "not-ntfs",
];

fn gb(x: f64) -> u64 {
    (x * 1_073_741_824.0).round() as u64
}

/// A node of the made-up drive: a folder with its items, a folder of a size whose items are made up when it is entered, or a file.
struct N {
    name: String,
    size: u64,
    cat: Category,
    kids: Option<Vec<N>>,
    file: bool,
}

fn d(name: &str, size_gb: f64, cat: Category, kids: Option<Vec<N>>) -> N {
    N { name: name.into(), size: gb(size_gb), cat, kids, file: false }
}

fn fl(name: &str, size_gb: f64, cat: Category) -> N {
    N { name: name.into(), size: gb(size_gb), cat, kids: None, file: true }
}

fn drive_c() -> Vec<N> {
    use Category::*;
    vec![
        d("Users", 0.0, Documents, Some(vec![
            d("Daniiar", 0.0, Documents, Some(vec![
                d("Videos", 0.0, Video, Some(vec![d("Screen recordings", 34.2, Video, None), fl("render-final.mov", 22.4, Video), fl("Holiday 2025.mp4", 18.1, Video), d("OBS", 12.6, Video, None), fl("interview-raw.mkv", 6.3, Video), d("Clips", 2.9, Video, None)])),
                d("Downloads", 0.0, Other, Some(vec![fl("dataset-2026.zip", 22.1, Other), d("Movies", 18.4, Video, None), fl("ubuntu-26.04.iso", 6.1, System), fl("Win11_24H2.iso", 5.8, System), d("Installers", 4.6, Apps, None), fl("figma-export.zip", 2.3, Other), fl("wallpaper.jpg", 0.004, Pictures)])),
                d("AppData", 0.0, Apps, Some(vec![
                    d("Local", 0.0, Apps, Some(vec![d("Docker", 22.3, Code, None), d("Temp", 8.1, Other, None), d("npm-cache", 6.4, Code, None), d("Google", 4.5, Apps, None), d("NVIDIA", 1.6, Apps, None)])),
                    d("Roaming", 0.0, Apps, Some(vec![d("Telegram Desktop", 7.2, Apps, None), d("Code", 3.7, Code, None), d("discord", 2.1, Apps, None)])),
                ])),
                d("Pictures", 0.0, Pictures, Some(vec![d("Camera 2026", 24.2, Pictures, None), d("Lightroom", 11.6, Pictures, None), d("Screenshots", 2.4, Pictures, None), fl("mountains.jpg", 0.004, Pictures), fl("aurora.jpg", 0.004, Pictures)])),
                d("Projects", 0.0, Code, Some(vec![
                    d("duet", 14.8, Code, None),
                    d("quipu", 0.0, Code, Some(vec![d("node_modules", 8.1, Code, None), d(".git", 2.2, Code, None), d("dist", 1.4, Code, None), d("src", 0.3, Code, None)])),
                    d("WinRust", 0.0, Code, Some(vec![d("target", 3.8, Code, None), d(".git", 0.31, Code, None), d("plugins", 0.08, Code, None), d("docs", 0.04, Documents, None)])),
                ])),
                d("Documents", 9.4, Documents, None),
                d("Music", 5.4, Other, None),
                d("Desktop", 3.2, Documents, None),
                d("OneDrive", 1.9, Documents, None),
            ])),
            d("Public", 0.6, Documents, None),
            d("Default", 0.3, System, None),
        ])),
        d("Games", 0.0, Games, Some(vec![
            d("Call of Duty", 98.4, Games, None),
            d("Cyberpunk 2077", 66.2, Games, None),
            d("Steam", 0.0, Games, Some(vec![d("Baldur’s Gate 3", 14.6, Games, None), d("Counter-Strike 2", 3.4, Games, None), d("Hades II", 2.1, Games, None), d("shadercache", 1.9, Games, None)])),
        ])),
        d("Program Files", 0.0, Apps, Some(vec![d("Adobe", 18.6, Apps, None), d("JetBrains", 9.1, Apps, None), d("Microsoft Office", 6.2, Apps, None), d("NVIDIA Corporation", 2.4, Apps, None), d("Blender Foundation", 1.3, Apps, None), d("WinRust", 0.03, Apps, None), d("Common Files", 1.6, Apps, None)])),
        d("Windows", 0.0, System, Some(vec![d("WinSxS", 11.4, System, None), d("System32", 9.2, System, None), d("Installer", 4.1, System, None), d("SoftwareDistribution", 3.8, System, None), d("Fonts", 0.9, System, None), d("SysWOW64", 1.6, System, None)])),
        fl("pagefile.sys", 16.0, System),
        fl("hiberfil.sys", 12.8, System),
        d("ProgramData", 14.2, Apps, None),
        d("Program Files (x86)", 9.8, Apps, None),
        d("$Recycle.Bin", 4.6, Other, None),
        d("Temp", 2.1, Other, None),
        fl("swapfile.sys", 0.26, System),
    ]
}

fn drive_d() -> Vec<N> {
    use Category::*;
    vec![
        d("Media", 0.0, Video, Some(vec![d("Films", 412.0, Video, None), d("Series", 288.0, Video, None), d("RAW photos", 0.0, Pictures, Some(vec![d("2024", 61.0, Pictures, None), d("2025", 94.0, Pictures, None), d("2026", 52.0, Pictures, None)]))])),
        d("Backups", 0.0, Other, Some(vec![d("PC image 2026-10", 142.0, System, None), d("Phone", 38.0, Pictures, None)])),
        d("Steam Library", 96.0, Games, None),
        d("VMs", 0.0, Code, Some(vec![fl("ubuntu-dev.vhdx", 64.0, Code), fl("win-test.vhdx", 42.0, Code)])),
        d("Archive", 31.0, Documents, None),
    ]
}

fn drive_e() -> Vec<N> {
    use Category::*;
    vec![d("DCIM", 0.0, Pictures, Some(vec![d("100CANON", 18.2, Pictures, None), d("101CANON", 9.4, Pictures, None)])), d("Footage", 11.6, Video, None), fl("backup.zip", 1.3, Other)]
}

/// The made-up items of a folder that the mockup's `gen` makes: the same words, and its generator, a product of two 32-bit numbers
/// rounded as a JavaScript number rounds it.
fn made_up(node_size: u64, cat: Category, path: &[String]) -> Vec<N> {
    let mut h: u32 = 7;
    for ch in path.join("/").chars() {
        h = h.wrapping_mul(31).wrapping_add(ch as u32);
    }
    let mut rnd = move || {
        h = ((f64::from(h) * 1_103_515_245.0 + 12345.0) as u64 % (1u64 << 32)) as u32;
        f64::from(h >> 8) / 16_777_216.0
    };
    use Category::*;
    let (stems, dirs): (&[(&str, &str)], &[&str]) = match cat {
        Video => (&[("clip", "mp4"), ("take", "mov"), ("export", "mp4"), ("raw", "mkv")], &["2024", "2025", "Edits"]),
        Pictures => (&[("IMG_", "jpg"), ("DSC", "arw"), ("edit-", "psd"), ("IMG_", "heic")], &["Trips", "Family", "Work"]),
        Games => (&[("pak", "pak"), ("chunk", "bin"), ("shaders", "cache"), ("textures", "pak")], &["Content", "Binaries", "Saved"]),
        Apps => (&[("app", "dll"), ("resources", "pak"), ("data", "bin"), ("update", "msi")], &["bin", "resources", "locales"]),
        Code => (&[("pack-", "pack"), ("lib", "rlib"), ("bundle", "js"), ("layer", "tar")], &["objects", "cache", "deps"]),
        Documents => (&[("Report ", "docx"), ("Budget ", "xlsx"), ("Scan ", "pdf"), ("Notes ", "md")], &["Work", "Taxes", "Archive"]),
        System => (&[("cab", "cab"), ("amd64_", "dll"), ("update", "esd"), ("catalog", "cat")], &["Logs", "Backup", "Temp"]),
        Other => (&[("file", "bin"), ("archive", "zip"), ("backup", "bak"), ("data", "dat")], &["Misc", "Old"]),
    };
    let n = 7 + (rnd() * 7.0).floor() as usize;
    let w: Vec<f64> = (0..n).map(|_| rnd().powf(2.2) + 0.02).collect();
    let sum: f64 = w.iter().sum();
    (0..n)
        .map(|i| {
            let is_dir = i < 2 && node_size > 2_000_000_000;
            let t = stems[i % stems.len()];
            let name = if is_dir { dirs[i % dirs.len()].to_string() } else { format!("{}{}.{}", t.0, 1000 + (rnd() * 8999.0).floor() as u32, t.1) };
            N { name, size: (node_size as f64 * w[i] / sum).round() as u64, cat, kids: None, file: !is_dir }
        })
        .collect()
}

/// The records of a made-up drive, the category of each file, and the counts the mockup would show (its estimate for a folder it
/// makes up: a file to 450 KB, a folder to twelve files).
struct Made {
    recs: HashMap<u32, Rec>,
    cats: HashMap<u32, Category>,
    counts: HashMap<u32, (u32, u32)>,
    next: u32,
}

impl Made {
    fn add(&mut self, n: &N, parent: u32, path: &mut Vec<String>) -> (u32, u32) {
        let id = self.next;
        self.next += 1;
        path.push(n.name.clone());
        let r = if n.file {
            self.recs.insert(id, Rec { parent, name: n.name.clone(), dir: false, alloc: n.size, logical: n.size });
            self.cats.insert(id, n.cat);
            (1, 0)
        } else {
            self.recs.insert(id, Rec { parent, name: n.name.clone(), dir: true, alloc: 0, logical: 0 });
            match &n.kids {
                Some(kids) => {
                    let (mut files, mut dirs) = (0, 0);
                    for k in kids {
                        let (f, dd) = self.add(k, id, path);
                        (files, dirs) = (files + f, dirs + dd + u32::from(!k.file));
                    }
                    self.counts.insert(id, (files, dirs));
                    (files, dirs)
                }
                None => {
                    if n.size > 0 {
                        for k in made_up(n.size, n.cat, path) {
                            self.add(&k, id, path);
                        }
                    }
                    let est = ((n.size as f64 / 450_000.0).round() as u32).max(3);
                    let r = (est, ((f64::from(est) / 12.0).round() as u32).max(1));
                    self.counts.insert(id, r);
                    r
                }
            }
        };
        path.pop();
        r
    }
}

fn declared(n: &N) -> u64 {
    match &n.kids {
        Some(k) => k.iter().map(declared).sum(),
        None => n.size,
    }
}

/// The data of the made-up folders `kids` of a drive: the tree, a category for each file, the mockup's counts.
fn data_of(kids: &[N], letter: char) -> DiskData {
    let mut m = Made { recs: HashMap::new(), cats: HashMap::new(), counts: HashMap::new(), next: 16 };
    m.recs.insert(ROOT, Rec { parent: ROOT, name: ".".into(), dir: true, alloc: 0, logical: 0 });
    let (mut files, mut dirs) = (0, 0);
    for k in kids {
        let (f, dd) = m.add(k, ROOT, &mut vec![]);
        (files, dirs) = (files + f, dirs + dd + u32::from(!k.file));
    }
    m.counts.insert(ROOT, (files, dirs));
    let (cats, counts) = (m.cats, m.counts);
    let mut data = DiskData::analyse(build(m.recs), letter, |_, id, _| cats[&id]);
    data.set_counts(counts);
    data
}

/// The mockup's `walkCat`: what the legend adds up.
fn walk_cat(n: &N, depth: u32, acc: &mut [u64; N_CATS]) {
    match &n.kids {
        Some(k) if depth <= 3 => k.iter().for_each(|c| walk_cat(c, depth + 1, acc)),
        _ => acc[n.cat as usize] += declared(n),
    }
}

/// The data of a made-up drive (`C:`, `D:`, `E:`).
#[must_use]
pub fn mock_data(letter: char) -> DiskData {
    let kids = match letter {
        'D' => drive_d(),
        'E' => drive_e(),
        _ => drive_c(),
    };
    let mut data = data_of(&kids, letter);
    // The legend of the mockup stops looking three levels down and counts a folder there as what the folder is (its Docker as Apps,
    // not Code): the totals are its, so the legend reads as it does there.
    let mut totals = [0u64; N_CATS];
    for k in &kids {
        walk_cat(k, 1, &mut totals);
    }
    data.totals = totals;
    data
}

/// A drive of one folder with `items` folders in it, of sizes that fall off steeply: the most a map and a list are asked to show.
#[must_use]
pub fn dense(items: usize) -> Disk {
    let mut s = 99u64;
    let mut rnd = move || {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (s >> 33) as f64 / f64::from(1u32 << 31)
    };
    let kids: Vec<N> = (0..items).map(|i| d(&format!("Folder {i}"), 0.02 + 80.0 * rnd().powi(4), Category::ALL[i % 8], None)).collect();
    let data = data_of(&kids, 'C');
    let fake = Arc::new(Fake::new(true));
    let mut disk = Disk::new(fake as Arc<dyn Backend>, Some('C'));
    disk.drives = mock_drives(used_c());
    disk.data = Some(data);
    disk.phase = Phase::Ready;
    disk.max_tiles = crate::layout::MAX_ROWS;
    disk
}

/// A drive as a big one reads: `files` files in about a ninth as many folders, put together at random (a tree as deep as a random one
/// is), named and sized like what a drive holds. Classified the way a real scan is.
#[must_use]
pub fn synthetic(files: usize) -> DiskData {
    let mut s = 12345u64;
    let mut rnd = move || {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (s >> 33) as usize
    };
    let words = ["Users", "Program Files", "Windows", "Games", "steamapps", "node_modules", "target", "Pictures", "Videos", "Documents", "AppData", "Local", "Temp", "cache", "src", "build"];
    let exts = ["dll", "mp4", "jpg", "txt", "pak", "rs", "bin", "pdf", "zip", "mkv", "png", "json"];
    let dirs = files / 9 + 1;
    let mut recs: HashMap<u32, Rec> = HashMap::with_capacity(files + dirs + 1);
    recs.insert(ROOT, Rec { parent: ROOT, name: ".".into(), dir: true, alloc: 0, logical: 0 });
    let mut dir_ids: Vec<u32> = vec![ROOT];
    let mut next = 16u32;
    for i in 0..dirs {
        let parent = dir_ids[rnd() % dir_ids.len()];
        recs.insert(next, Rec { parent, name: format!("{}{}", words[rnd() % words.len()], i), dir: true, alloc: 0, logical: 0 });
        dir_ids.push(next);
        next += 1;
    }
    for i in 0..files {
        let parent = dir_ids[rnd() % dir_ids.len()];
        let len = ((rnd() % 2000 + 1) as u64 * 1024) >> (rnd() % 10);
        recs.insert(next, Rec { parent, name: format!("file{}-{}.{}", i, rnd() % 1000, exts[rnd() % exts.len()]), dir: false, alloc: len.div_ceil(4096) * 4096, logical: len });
        next += 1;
    }
    DiskData::read(build(recs), 'C')
}

/// What the made-up `C:` holds (made once).
fn used_c() -> u64 {
    static USED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *USED.get_or_init(|| mock_data('C').used())
}

/// The drives of the mockup; `used` is what the drive on screen holds (the others, what their folders add up to).
fn mock_drives(used_c: u64) -> Vec<DriveInfo> {
    let info = |letter, label: &str, fs: &str, total: f64, used: u64, removable| {
        let total = gb(total);
        DriveInfo { letter, label: label.into(), fs: fs.into(), total, free: total.saturating_sub(used), removable }
    };
    vec![
        info('C', "Local Disk", "NTFS", 953.9, used_c, false),
        info('D', "Data", "NTFS", 1863.0, drive_d().iter().map(declared).sum(), false),
        info('E', "USB Drive", "exFAT", 58.2, drive_e().iter().map(declared).sum(), true),
    ]
}

/// What a test or a render may look at afterwards, and a machine that has nothing to read, delete or elevate.
pub struct Fake {
    pub admin: bool,
    pub log: Mutex<Log>,
}

#[derive(Default)]
pub struct Log {
    pub scans: Vec<(char, bool)>,
    pub deletes: Vec<(Vec<PathBuf>, How, bool)>,
    /// The owner window each deletion was given.
    pub owners: Vec<isize>,
    pub reveals: Vec<String>,
    pub elevations: Vec<char>,
}

impl Fake {
    #[must_use]
    pub fn new(admin: bool) -> Self {
        Self { admin, log: Mutex::new(Log::default()) }
    }
}

impl Backend for Fake {
    fn drives(&self) -> Vec<DriveInfo> {
        mock_drives(used_c())
    }

    fn is_admin(&self) -> bool {
        self.admin
    }

    fn scan(&self, drive: char, slow: bool, progress: &mut dyn FnMut(u64, u64)) -> Result<DiskData, String> {
        self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).scans.push((drive, slow));
        progress(0, 100);
        progress(100, 100);
        Ok(mock_data(drive))
    }

    fn delete(&self, paths: &[PathBuf], how: How, confirmed: bool, _owner: isize) -> Result<(), String> {
        let mut log = self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        log.deletes.push((paths.to_vec(), how, confirmed));
        log.owners.push(_owner);
        drop(log);
        if confirmed { Ok(()) } else { Err("not confirmed".into()) }
    }

    fn reveal(&self, path: &str) {
        self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).reveals.push(path.to_string());
    }

    fn restart_elevated(&self, drive: char) -> bool {
        self.log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).elevations.push(drive);
        false
    }

    /// At once, on this thread: a render has no loop to wait for a worker in, a test wants the outcome.
    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        job();
    }
}

/// The window on the made-up drive `letter`, scanned, with `fake` as its machine.
#[must_use]
pub fn scanned(fake: Arc<Fake>, letter: char) -> Disk {
    let data = mock_data(letter);
    let be: Arc<dyn Backend> = fake;
    let mut disk = Disk::new(be, Some(letter));
    disk.drives = mock_drives(used_c());
    if let Some(i) = disk.drives.iter().position(|x| x.letter == letter) {
        disk.drive = i;
        disk.drives[i].free = disk.drives[i].total.saturating_sub(data.used());
    }
    disk.data = Some(data);
    disk.phase = Phase::Ready;
    disk
}

/// Goes down `names` from the root.
pub fn enter(disk: &mut Disk, names: &[&str]) {
    for name in names {
        let cur = disk.nav.current();
        let Some(d) = disk.data.as_ref() else { return };
        if let Some(id) = d.children(cur).into_iter().map(|c| c.0).find(|&i| d.name(i) == *name) {
            disk.nav.enter(id);
        }
    }
}

/// The id of the item `name` of the folder on screen.
#[must_use]
pub fn find(disk: &Disk, name: &str) -> Option<u32> {
    let d = disk.data.as_ref()?;
    d.children(disk.nav.current()).into_iter().map(|c| c.0).find(|&i| d.name(i) == name)
}

/// The window in the state `state` of the design.
pub fn demo(state: &str) -> Box<dyn App> {
    let state = if state.is_empty() { std::env::var("WIZTREE_DEMO").unwrap_or_default() } else { state.to_string() };
    Box::new(state_of(&state))
}

pub fn state_of(state: &str) -> Disk {
    let fake = Arc::new(Fake::new(state != "needs-admin"));
    let mut v = scanned(fake, if state == "not-ntfs" { 'E' } else { 'C' });
    const USER: [&str; 2] = ["Users", "Daniiar"];
    match state {
        "inside-folder" => enter(&mut v, &USER),
        "deep-folder" => enter(&mut v, &["Users", "Daniiar", "Projects", "quipu"]),
        "list-view" => {
            enter(&mut v, &USER);
            v.view = ViewMode::List;
        }
        "file-selected" => {
            enter(&mut v, &["Users", "Daniiar", "Videos"]);
            v.sel = find(&v, "render-final.mov");
        }
        "category-highlight" => v.pin_hl = Some(Category::Games),
        "context-menu" => v.demo = DemoHook::Menu(find(&v, "Games").unwrap_or(0)),
        "delete-dialog" | "deleted-toast" | "recycled-toast" => {
            enter(&mut v, &["Games"]);
            if let Some(id) = find(&v, "Call of Duty") {
                v.ask_delete(id);
            }
            if let Some(dlg) = v.dlg.as_mut() {
                dlg.how = if state == "recycled-toast" { How::RecycleBin } else { How::Permanently };
            }
            if state != "delete-dialog" {
                v.demo = DemoHook::Confirm;
            }
        }
        "scan-reading" | "scan-frozen" | "scan" => {
            let p = if state == "scan-frozen" { 0.62 } else { 0.38 };
            let total = 2_050_112u64;
            v.data = None;
            v.layout = None;
            v.phase = Phase::Scanning(Scan { epoch: 1, read: (p * total as f64) as u64, total, slow: false, since: std::time::Instant::now(), eta: Some(if state == "scan-frozen" { 1 } else { 2 }) });
        }
        _ => {}
    }
    if state == "not-ntfs" {
        v.data = None;
        v.layout = None;
        v.phase = Phase::Idle;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_made_up_drive_has_the_numbers_of_the_mockup() {
        let d = mock_data('C');
        let human = |b: u64| crate::look::human(b);
        assert_eq!(human(d.used()), "618 GB");
        let users = d.children(ROOT).iter().map(|c| c.0).find(|&i| d.name(i) == "Users").unwrap();
        assert_eq!(human(d.size(users)), "302 GB");
        let games = d.children(ROOT).iter().map(|c| c.0).find(|&i| d.name(i) == "Games").unwrap();
        assert_eq!(human(d.size(games)), "187 GB");
        assert_eq!(d.category(games), Category::Games);
        assert_eq!(d.counts(ROOT).0, 1_207_760, "the files the mockup says");
        // the legend's largest categories, as on its map: Games, Apps, Video, System
        let mut cats: Vec<_> = Category::ALL.iter().map(|c| (d.totals[*c as usize], *c)).collect();
        cats.sort_by_key(|c| std::cmp::Reverse(c.0));
        assert_eq!(cats.iter().take(4).map(|c| c.1).collect::<Vec<_>>(), vec![Category::Games, Category::Apps, Category::Video, Category::System]);
    }

    #[test]
    fn every_state_of_the_design_builds_and_the_made_up_data_is_the_same_each_time() {
        for s in STATES {
            let v = state_of(s);
            assert_eq!(App::title(&v), "WinRust Disk", "{s}");
        }
        let (a, b) = (mock_data('C'), mock_data('C'));
        assert_eq!((a.used(), a.tree.recs.len()), (b.used(), b.tree.recs.len()));
    }
}
