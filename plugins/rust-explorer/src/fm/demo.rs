//! The made-up machine of the design (Explorer.v2.html): its folders, drives and pictures, a [`Fake`] backend that reads and changes nothing
//! and records every call, and the states of the mockup's "Preview states" as windows for `--render`, the benchmark and the tests.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering::Relaxed},
    },
};

use winrust_ui::core::App;

use super::{
    backend::{Backend, Clock, DriveInfo, Do, Facts, Flag, Found, JobView, Opts, Place, Places},
    look::{DriveKind, IconKind, Special},
    model::{Fm, Hook, Ren, View, pc_dir},
    text,
};
use crate::Row;

/// The states of the mockup, as `--render-all` goes through them.
pub const STATES: [&str; 17] = [
    "folder", "this-pc", "images", "empty", "search", "nothing-found", "large-icons", "grouped", "no-preview", "file-menu", "folder-menu", "renaming", "copying", "usb-ejected", "pictures-list", "deep", "many",
];

const GIB: f64 = 1_073_741_824.0;

fn now() -> Clock {
    Clock { now: text::ticks(2026, 11, 26, 17, 30) as i64, off: 0 }
}

fn when(s: &str) -> u64 {
    let (d, t) = s.split_once(' ').unwrap_or((s, "00:00"));
    let n = |x: &str| x.parse::<u32>().unwrap_or(0);
    let mut dp = d.split('-');
    let (y, m, day) = (dp.next().unwrap_or("2026").parse::<i32>().unwrap_or(2026), n(dp.next().unwrap_or("1")), n(dp.next().unwrap_or("1")));
    let mut tp = t.split(':');
    text::ticks(y, m, day, n(tp.next().unwrap_or("0")), n(tp.next().unwrap_or("0")))
}

fn dir_row(name: &str, m: &str) -> Row {
    Row { name: name.into(), dir: true, size: 0, time: when(m), icon: -1, hdr: false }
}

fn file_row(name: &str, m: &str, size: u64) -> Row {
    Row { name: name.into(), dir: false, size, time: when(m), icon: -1, hdr: false }
}

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

const HOME: &str = r"C:\Users\Daniiar";

/// The folders of the mockup.
fn tree() -> HashMap<PathBuf, Vec<Row>> {
    let (d, f) = (dir_row, file_row);
    let mut t: HashMap<PathBuf, Vec<Row>> = HashMap::new();
    let mut put = |k: &str, v: Vec<Row>| {
        t.insert(p(k), v);
    };
    put(r"C:\", vec![d("PerfLogs", "2024-04-01 00:26"), d("Program Files", "2026-11-26 07:59"), d("Program Files (x86)", "2026-11-13 00:05"), d("Projects", "2026-11-26 08:12"), d("Temp", "2026-11-26 16:40"), d("Users", "2026-10-31 05:11"), d("Windows", "2026-11-25 21:36"), f("appverifUI.dll", "2026-01-27 05:18", 112_640), f("config.toml", "2026-11-20 09:42", 2150), f("install.log", "2026-11-25 22:07", 18_400)]);
    put(r"C:\PerfLogs", vec![]);
    put(r"C:\Program Files", vec![d("WinRust", "2026-11-26 07:59"), d("NVIDIA Corporation", "2026-11-02 13:20"), d("Windows Defender", "2026-11-12 03:00"), d("Common Files", "2026-10-30 18:11")]);
    put(r"C:\Program Files\WinRust", vec![f("topbar.exe", "2026-11-26 07:59", 4_718_592), f("rust-explorer.exe", "2026-11-26 07:59", 3_355_443), f("rtaskmgr-win32.exe", "2026-11-26 07:59", 2_936_012), f("rust-search.exe", "2026-11-26 07:59", 1_887_436), f("plugins.toml", "2026-11-26 07:59", 1200)]);
    put(r"C:\Program Files (x86)", vec![d("Microsoft", "2026-11-13 00:05"), d("Steam", "2026-10-21 22:40")]);
    put(r"C:\Projects", vec![d("WinRust", "2026-11-26 08:12"), d("quipu", "2026-11-25 23:40"), d("duet", "2026-11-20 14:03"), d("dead-shelter", "2026-11-11 19:27"), f("notes.md", "2026-11-26 07:31", 6100)]);
    put(r"C:\Projects\WinRust", vec![d("plugins", "2026-11-26 08:12"), d("src", "2026-11-26 08:10"), d("common", "2026-11-25 22:01"), d("scripts", "2026-11-24 12:44"), f("Cargo.toml", "2026-11-26 08:02", 3100), f("README.md", "2026-11-25 18:20", 14_300), f("WinRust.exe", "2026-11-26 08:12", 8_178_892)]);
    put(r"C:\Projects\WinRust\plugins", vec![d("topbar", "2026-11-26 08:12"), d("rust-explorer", "2026-11-26 07:40"), d("rtaskmgr-win32", "2026-11-25 19:02"), d("rust-search", "2026-11-22 11:15"), d("rs-shell", "2026-11-20 16:48"), d("photoview", "2026-11-18 10:30")]);
    put(r"C:\Projects\quipu", vec![d("src", "2026-11-25 23:40"), f("package.json", "2026-11-25 23:12", 1800), f("README.md", "2026-11-24 10:05", 5200)]);
    put(r"C:\Projects\duet", vec![d("app", "2026-11-20 14:03"), f("design.fig", "2026-11-19 18:30", 24_117_248)]);
    put(r"C:\Projects\dead-shelter", vec![d("assets", "2026-11-11 19:27"), f("index.html", "2026-11-11 19:20", 4300), f("game.js", "2026-11-11 19:27", 88_000)]);
    put(r"C:\Temp", vec![f("winhelper.exe", "2026-11-26 16:40", 412_000), f("~setup.tmp", "2026-11-24 09:12", 2_400_000)]);
    put(r"C:\Users", vec![d("Daniiar", "2026-11-26 08:13"), d("Public", "2026-06-02 11:00")]);
    put(r"C:\Windows", vec![d("System32", "2026-11-25 21:36"), d("Fonts", "2026-11-03 08:00"), d("Temp", "2026-11-26 09:44"), f("explorer.exe", "2026-11-12 03:01", 5_452_595), f("notepad.exe", "2026-11-12 03:01", 360_448)]);
    put(r"D:\", vec![d("Games", "2026-11-21 20:10"), d("Backups", "2026-11-01 03:00"), d("Photos", "2026-10-18 12:30"), d("Videos", "2026-09-30 22:00"), f("Windows 11 24H2.iso", "2026-08-12 10:00", 6_657_199_308)]);
    put(r"D:\Games", vec![d("Steam Library", "2026-11-21 20:10")]);
    put(r"D:\Games\Steam Library", vec![]);
    put(r"D:\Backups", vec![f("system-2026-11-01.vhdx", "2026-11-01 03:00", 120_259_084_288), f("system-2026-10-01.vhdx", "2026-10-01 03:00", 118_111_600_640)]);
    put(r"D:\Photos", vec![f("IMG_2031.jpg", "2026-10-18 12:30", 5_033_164), f("IMG_2040.png", "2026-10-19 09:02", 3_145_728), f("IMG_2041.heic", "2026-10-19 09:05", 2_936_012)]);
    put(r"D:\Videos", vec![]);
    put(r"E:\", vec![d("Installers", "2026-11-18 14:00"), f("WinRust-0.5.9.zip", "2026-11-26 08:20", 9_017_753), f("slides.pdf", "2026-11-10 11:45", 2_936_012)]);
    put(r"E:\Installers", vec![f("rustup-init.exe", "2026-11-18 14:00", 9_437_184), f("Git-2.47.0-64-bit.exe", "2026-11-18 14:01", 66_060_288)]);
    put(r"Z:\", vec![d("Shared", "2026-11-25 10:00"), d("Design", "2026-11-26 09:30"), f("team-notes.md", "2026-11-26 09:12", 4200)]);
    put(r"Z:\Shared", vec![]);
    put(r"Z:\Design", vec![f("winrust-redesign.fig", "2026-11-26 09:30", 38_797_312)]);
    put(HOME, vec![d("Desktop", "2026-11-26 08:13"), d("Documents", "2026-11-25 18:44"), d("Downloads", "2026-11-26 08:15"), d("Music", "2026-10-02 12:00"), d("Pictures", "2026-11-26 07:44"), d("Videos", "2026-09-28 21:15")]);
    put(&format!(r"{HOME}\Desktop"), vec![f("WinRust.lnk", "2026-11-26 08:13", 1100), f("todo.txt", "2026-11-24 22:15", 640)]);
    put(&format!(r"{HOME}\Documents"), vec![d("Contracts", "2026-10-12 11:20"), f("Plan 2027.docx", "2026-11-25 18:44", 49_152), f("budget.xlsx", "2026-11-19 09:05", 31_744), f("NumBuster brief.pdf", "2026-11-03 15:40", 812_000)]);
    put(&format!(r"{HOME}\Documents\Contracts"), vec![f("lease.pdf", "2026-09-14 12:00", 420_000)]);
    put(&format!(r"{HOME}\Downloads"), vec![f("WinRust.exe", "2026-11-26 08:15", 8_178_892), f("wallpaper.jpg", "2026-11-26 07:58", 4_299_161), f("invoice-november.pdf", "2026-11-25 16:02", 225_280), f("setup-logs.zip", "2026-11-22 10:41", 1_363_148), f("Inter-4.0.zip", "2026-11-04 09:12", 5_872_025), f("screenshot-2026-10-29.png", "2026-10-29 14:20", 812_000)]);
    put(&format!(r"{HOME}\Music"), vec![]);
    put(&format!(r"{HOME}\Pictures"), vec![d("Screenshots", "2026-11-26 07:44"), f("aurora.jpg", "2026-11-12 23:41", 3_891_200), f("bloom.jpg", "2026-11-20 18:05", 1_820_160), f("mountains.jpg", "2026-11-14 20:10", 3_774_873), f("wallpaper.jpg", "2026-11-26 07:58", 4_299_161), f("waves.jpg", "2026-10-30 11:22", 2_457_600)]);
    put(&format!(r"{HOME}\Pictures\Screenshots"), vec![f("desktop.png", "2026-11-26 07:44", 1_520_640), f("file-manager.png", "2026-11-26 07:42", 1_118_208), f("settings.png", "2026-11-26 07:41", 1_187_840), f("task-manager.png", "2026-11-26 07:40", 1_443_840), f("terminal.png", "2026-11-26 07:39", 1_140_736)]);
    put(&format!(r"{HOME}\Videos"), vec![]);
    t
}

fn drive_list() -> Vec<DriveInfo> {
    let g = |x: f64| (x * GIB) as u64;
    vec![
        DriveInfo { root: p(r"C:\"), label: "Local Disk (C:)".into(), kind: DriveKind::Ssd, fs: "NTFS".into(), total: g(953.0), free: g(349.0), removable: false },
        DriveInfo { root: p(r"D:\"), label: "Data (D:)".into(), kind: DriveKind::Hdd, fs: "NTFS".into(), total: g(1863.0), free: g(1142.0), removable: false },
        DriveInfo { root: p(r"E:\"), label: "USB Drive (E:)".into(), kind: DriveKind::Usb, fs: "exFAT".into(), total: g(58.0), free: g(41.0), removable: true },
        DriveInfo { root: p(r"Z:\"), label: "Network Drive (Z:)".into(), kind: DriveKind::Net, fs: "SMB share".into(), total: g(2048.0), free: g(61.0), removable: false },
    ]
}

fn places(drives: Vec<DriveInfo>, pins: &[PathBuf]) -> Places {
    let sp = |t: &str, name: &str, s: Special| Place { title: t.into(), path: p(&format!(r"{HOME}\{name}")), icon: IconKind::Folder(Some(s)), pinned: false };
    let mut quick = vec![
        Place { title: "Home".into(), path: p(HOME), icon: IconKind::Home, pinned: false },
        sp("Desktop", "Desktop", Special::Desktop),
        sp("Downloads", "Downloads", Special::Downloads),
        sp("Documents", "Documents", Special::Documents),
        sp("Pictures", "Pictures", Special::Pictures),
        sp("Music", "Music", Special::Music),
        sp("Videos", "Videos", Special::Videos),
    ];
    for q in pins {
        quick.push(Place { title: q.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()), path: q.clone(), icon: IconKind::Folder(None), pinned: true });
    }
    Places { quick, drives }
}

/// A made-up photograph: a soft gradient, so that the thumbnails of a state have something to show. `seed` picks the colours.
fn picture(seed: u32, w: i32, h: i32) -> Vec<u32> {
    const SKY: [(u32, u32); 5] = [(0x2b3a67, 0xe8a87c), (0x1b4d3e, 0xa3d9a5), (0x3a2f5b, 0xf2b5d4), (0x14213d, 0x6ab0de), (0x5c2a2a, 0xf0c27b)];
    let (a, b) = SKY[seed as usize % SKY.len()];
    let mix = |x: u32, y: u32, t: f32| {
        let k = |s: u32| (((x >> s & 255) as f32) * (1.0 - t) + ((y >> s & 255) as f32) * t) as u32;
        k(16) << 16 | k(8) << 8 | k(0)
    };
    let mut px = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let t = y as f32 / h as f32;
            let ridge = (((x as f32 / w as f32) * 7.0 + seed as f32).sin() * 0.12 + 0.62).clamp(0.0, 1.0);
            let base = if t > ridge { mix(0x0e1118, 0x2a3140, (t - ridge) * 2.0) } else { mix(a, b, t / ridge.max(0.01)) };
            px[(y * w + x) as usize] = 0xff00_0000 | base;
        }
    }
    px
}

// ---------------------------------------------------------------------------------------------------------------------
// the machine that touches nothing

pub struct Fake {
    pub fs: Mutex<HashMap<PathBuf, Vec<Row>>>,
    pub drives: Mutex<Vec<DriveInfo>>,
    pub pins: Mutex<Vec<PathBuf>>,
    flags: Mutex<(bool, bool)>,
    pub job: Mutex<Option<JobView>>,
    /// Every call the window made on the machine, in order, as a line of text.
    pub calls: Mutex<Vec<String>>,
    era: AtomicU32,
}

impl Default for Fake {
    fn default() -> Self {
        Self { fs: Mutex::new(tree()), drives: Mutex::new(drive_list()), pins: Mutex::new(vec![p(r"C:\Projects")]), flags: Mutex::new((false, true)), job: Mutex::new(None), calls: Mutex::new(Vec::new()), era: AtomicU32::new(1) }
    }
}

impl Fake {
    fn note(&self, s: impl Into<String>) {
        self.calls.lock().unwrap().push(s.into());
    }

    /// The calls so far, and forgets them.
    pub fn take_calls(&self) -> Vec<String> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    fn size_of(&self, dir: &Path) -> u64 {
        let fs = self.fs.lock().unwrap();
        fn walk(fs: &HashMap<PathBuf, Vec<Row>>, d: &Path) -> u64 {
            fs.get(d).map_or(0, |v| v.iter().map(|r| if r.dir { walk(fs, &d.join(&r.name)) } else { r.size }).sum())
        }
        walk(&fs, dir)
    }
}

impl Backend for Fake {
    fn clock(&self) -> Clock {
        now()
    }

    fn list(&self, dir: &Path, o: &Opts) -> Option<Vec<Row>> {
        let rows = self.peek(dir, o)?;
        self.era.fetch_add(1, Relaxed);
        Some(rows)
    }

    fn peek(&self, dir: &Path, o: &Opts) -> Option<Vec<Row>> {
        let mut rows = self.fs.lock().unwrap().get(dir)?.clone();
        self.sort(&mut rows, o);
        Some(rows)
    }

    fn sort(&self, rows: &mut Vec<Row>, o: &Opts) {
        let c = now();
        crate::bucket_set(c.now, c.off);
        crate::sort_rows_with(rows, o.col, o.asc, !o.folders_first, o.group);
        crate::add_headers_with(rows, o.group);
    }

    fn is_target(&self, p: &Path) -> bool {
        self.fs.lock().unwrap().contains_key(p)
    }

    fn bar(&self) -> bool {
        false
    }

    fn places(&self) -> Places {
        places(self.drives.lock().unwrap().clone(), &self.pins.lock().unwrap())
    }

    fn search(&self, dir: &Path, q: &str, _o: &Opts) -> Found {
        self.note(format!("search {q}"));
        let q = q.to_lowercase();
        let fs = self.fs.lock().unwrap();
        let mut rows = Vec::new();
        fn walk(fs: &HashMap<PathBuf, Vec<Row>>, d: &Path, q: &str, depth: u32, out: &mut Vec<Row>) {
            let Some(v) = fs.get(d) else { return };
            for r in v {
                let at = d.join(&r.name);
                if r.name.to_lowercase().contains(q) {
                    out.push(Row { name: at.to_string_lossy().into_owned(), ..r.clone() });
                }
                if r.dir && depth < 6 {
                    walk(fs, &at, q, depth + 1, out);
                }
            }
        }
        walk(&fs, dir, &q, 0, &mut rows);
        Found { rows, more: String::new(), indexed: false }
    }

    fn flag(&self, f: Flag) -> bool {
        let g = self.flags.lock().unwrap();
        match f {
            Flag::Group => g.0,
            Flag::FoldersFirst => g.1,
        }
    }

    fn set_flag(&self, f: Flag, on: bool) {
        let mut g = self.flags.lock().unwrap();
        match f {
            Flag::Group => g.0 = on,
            Flag::FoldersFirst => g.1 = on,
        }
    }

    fn new_entry(&self, dir: &Path, folder: bool) -> Option<String> {
        let mut fs = self.fs.lock().unwrap();
        let base = if folder { "New folder" } else { "New text document.txt" };
        let list = fs.get_mut(dir)?;
        let mut name = base.to_string();
        let mut n = 2;
        while list.iter().any(|r| r.name.eq_ignore_ascii_case(&name)) {
            name = match base.rsplit_once('.') {
                Some((s, e)) if !folder => format!("{s} ({n}).{e}"),
                _ => format!("{base} ({n})"),
            };
            n += 1;
        }
        list.push(if folder { dir_row(&name, "2026-11-26 17:30") } else { file_row(&name, "2026-11-26 17:30", 0) });
        if folder {
            fs.insert(dir.join(&name), vec![]);
        }
        drop(fs);
        self.note(format!("new_entry {name}"));
        Some(name)
    }

    fn rename(&self, dir: &Path, old: &str, new: &str) -> Result<(), String> {
        self.note(format!("rename {old} -> {new}"));
        let mut fs = self.fs.lock().unwrap();
        let list = fs.get_mut(dir).ok_or("no such folder")?;
        let r = list.iter_mut().find(|r| r.name == old).ok_or("no such item")?;
        r.name = new.to_string();
        Ok(())
    }

    fn real_paths(&self, paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
        Ok(paths.to_vec())
    }

    fn facts(&self, path: &Path) -> Facts {
        let mut f = Facts::default();
        let fs = self.fs.lock().unwrap();
        if let Some(v) = fs.get(path) {
            let d = v.iter().filter(|r| r.dir).count();
            f.contains = Some((d, v.len() - d, false));
        }
        drop(fs);
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        f.dims = match name.as_str() {
            "aurora.jpg" | "mountains.jpg" | "waves.jpg" | "img_2031.jpg" => Some((2400, 1600)),
            "bloom.jpg" | "img_2040.png" => Some((1200, 1800)),
            "wallpaper.jpg" => Some((2560, 1440)),
            "desktop.png" | "screenshot-2026-10-29.png" => Some((1440, 900)),
            "task-manager.png" => Some((1360, 860)),
            _ => None,
        };
        if path.is_dir() || name.is_empty() || self.fs.lock().unwrap().contains_key(path) {
            f.size = Some(self.size_of(path));
        }
        f
    }

    fn job(&self) -> Option<JobView> {
        self.job.lock().unwrap().clone()
    }

    fn busy(&self) -> bool {
        self.job.lock().unwrap().is_some()
    }

    fn tool(&self, _name: &str) -> bool {
        true
    }

    fn era(&self) -> u32 {
        self.era.load(Relaxed)
    }

    fn run(&self, _owner: isize, d: Do) {
        self.note(format!("{d:?}"));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the states

/// A window in a state of the design, and the machine it runs on.
#[must_use]
pub fn build(state: &str) -> (Fm, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let mut fm = Fm::new(fake.clone(), p(r"C:\"));
    fm.flags_from_backend();
    fm.places = fake.places();
    fm.tabs = vec![p(r"C:\"), p(&format!(r"{HOME}\Downloads"))];
    let mut cx = winrust_ui::frame::Cx::default();
    let go = |fm: &mut Fm, d: &str, cx: &mut winrust_ui::frame::Cx| {
        fm.load(if d == "pc" { pc_dir() } else { p(d) }, true, false, cx);
        fm.tabs[0] = fm.dir.clone();
    };
    let pics = format!(r"{HOME}\Pictures");
    match state {
        "this-pc" => {
            fm.tabs = vec![pc_dir()];
            go(&mut fm, "pc", &mut cx);
        }
        "images" | "pictures-list" => {
            fm.tabs = vec![p(&pics)];
            go(&mut fm, &pics, &mut cx);
            fm.select_names(&["mountains.jpg".to_string()]);
        }
        "empty" => {
            fm.tabs = vec![p(&format!(r"{HOME}\Music"))];
            go(&mut fm, &format!(r"{HOME}\Music"), &mut cx);
        }
        "search" | "nothing-found" => {
            go(&mut fm, r"C:\", &mut cx);
            fm.srch.set_text(if state == "search" { "read" } else { "zzzz" });
            fm.run_search(if state == "search" { "read" } else { "zzzz" }, &mut cx);
        }
        "large-icons" => {
            fm.tabs = vec![p(&pics)];
            fm.view = View::Icons;
            go(&mut fm, &pics, &mut cx);
            fm.select_names(&["mountains.jpg".to_string()]);
        }
        "grouped" => {
            fm.group = true;
            fm.sort = (1, false);
            go(&mut fm, r"C:\", &mut cx);
            fm.select_names(&["Projects".to_string()]);
        }
        "deep" => {
            fm.tabs = vec![p(r"C:\Projects\WinRust\plugins")];
            go(&mut fm, r"C:\Projects\WinRust\plugins", &mut cx);
        }
        "many" => {
            // a folder of 10,000 items for the benchmark
            let rows: Vec<Row> = (0..10_000u32).map(|i| if i % 9 == 0 { dir_row(&format!("folder {i:05}"), "2026-11-20 10:00") } else { file_row(&format!("file {i:05}.txt"), "2026-11-21 11:30", 1000 + u64::from(i) * 37) }).collect();
            fake.fs.lock().unwrap().insert(p(r"C:\Many"), rows);
            go(&mut fm, r"C:\Many", &mut cx);
        }
        _ => {
            go(&mut fm, r"C:\", &mut cx);
            fm.select_names(&["Projects".to_string()]);
        }
    }
    match state {
        "no-preview" => {
            fm.pv = false;
            fm.select_names(&["Projects".to_string()]);
        }
        "file-menu" => {
            fm.select_names(&["Projects".to_string()]);
            fm.hook = Hook::ItemMenu;
        }
        "folder-menu" => {
            fm.list.sel.clear();
            fm.hook = Hook::FolderMenu;
        }
        "renaming" => {
            fm.select_names(&["Projects".to_string()]);
            if let Some(i) = fm.one() {
                fm.rename_begin(i, &mut cx);
            }
        }
        "copying" => {
            *fake.job.lock().unwrap() = Some(JobView { label: "Copying Temp".into(), frac: Some(0.46) });
            fm.job = fake.job.lock().unwrap().clone();
            fm.select_names(&["Projects".to_string()]);
        }
        "usb-ejected" => {
            fake.drives.lock().unwrap().retain(|d| d.root != p(r"E:\"));
            fm.places = fake.places();
            fm.select_names(&["Projects".to_string()]);
        }
        _ => {}
    }
    // the pictures of the state: a made-up photograph for every image in view
    let era = fm.era;
    let paths: Vec<(usize, PathBuf)> = (0..fm.rows.len()).filter(|&i| !fm.rows[i].dir && crate::is_pic(&fm.row_path(i))).map(|i| (i, fm.row_path(i))).collect();
    for (k, (_, path)) in paths.into_iter().enumerate() {
        fm.got_picture(era, path, false, (96, 72), picture(k as u32, 96, 72));
    }
    fm.title_sent.clone_from(&fm.title);
    let _ = Ren { name: String::new(), field: winrust_ui::widgets::field_model::TextField::new(), bad: 0 };
    (fm, fake)
}

/// What `--render`, `--bench` and `--selftest` run: the window in the state `state` ("" is the first one).
#[must_use]
pub fn demo(state: &str) -> Box<dyn App> {
    Box::new(build(state).0)
}
