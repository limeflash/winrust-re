//! What the file manager asks of the machine: listing a folder, the places in the left pane, the shell's context menu, the clipboard, file
//! operations, thumbnails. The window talks to a [`Backend`], so a render, a benchmark, a self-test and a test run it on a fake that
//! touches nothing (`demo::Fake`, which records what it was asked); only [`Real`] reads the disk and changes it.
//!
//! Everything that can run a modal loop (the shell's file operation, a drag, a message box, the bar's menu while it waits) is not done
//! inside the window's own call: [`Real`] queues it (`live::defer`) and the window runs it once its message is handled, with nothing
//! of the window borrowed, then tells the window to list the folder again.
use std::path::{Path, PathBuf};

use super::look::{DriveKind, IconKind};
use crate::Row;

/// How a listing is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    /// 0 name, 1 date modified, 2 type, 3 size (the columns of the window).
    pub col: usize,
    pub asc: bool,
    pub hidden: bool,
    pub group: bool,
    pub folders_first: bool,
    /// The icons view: its rows carry no date headings.
    pub icons: bool,
}

/// "Now" for the dates of a listing: FILETIME ticks (UTC) and the offset of the local time in ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub now: i64,
    pub off: i64,
}

/// A place of the left pane.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub title: String,
    pub path: PathBuf,
    pub icon: IconKind,
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DriveInfo {
    pub root: PathBuf,
    /// "Local Disk (C:)".
    pub label: String,
    pub kind: DriveKind,
    pub fs: String,
    pub total: u64,
    pub free: u64,
    pub removable: bool,
}

impl DriveInfo {
    /// How full the drive is, 0 to 100.
    #[must_use]
    pub fn used_pct(&self) -> u32 {
        if self.total == 0 { 0 } else { ((1.0 - self.free as f64 / self.total as f64) * 100.0).round() as u32 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Places {
    pub quick: Vec<Place>,
    pub drives: Vec<DriveInfo>,
}

/// What a search found.
pub struct Found {
    pub rows: Vec<Row>,
    /// What the line over the results adds ("stopped early", the other layout's query).
    pub more: String,
    /// The rust-search service answered (otherwise this folder and below were walked).
    pub indexed: bool,
}

/// What the preview pane shows of an item beyond its row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// A folder's folders and files, `+` when it has more than is counted.
    pub contains: Option<(usize, usize, bool)>,
    /// A folder's size when the backend knows it (the window otherwise shows what the Size column was filled with).
    pub size: Option<u64>,
    pub dims: Option<(u32, u32)>,
}

/// A copy, packing or unpacking in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct JobView {
    pub label: String,
    /// 0..1; none while the size is not known yet.
    pub frac: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    /// "Group by date" (a file in the settings folder, as before).
    Group,
    /// "Folders first" (on unless the old marker file is there).
    FoldersFirst,
}

/// What the window asks to have done.
#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    /// A file: its default app, or the photo viewer for a picture.
    Open(PathBuf),
    /// "Run as administrator".
    RunAs(PathBuf),
    Clip { paths: Vec<PathBuf>, cut: bool },
    Paste(PathBuf),
    Delete { paths: Vec<PathBuf>, dir: PathBuf },
    Props(PathBuf),
    /// The bar's menu for these (the shell's items drawn by the bar); the window shows its own when there is none.
    ItemMenu { dir: PathBuf, paths: Vec<PathBuf>, at: Option<(f32, f32)> },
    /// Starts an OLE drag of the paths (the window sees the drop on itself as a drop).
    Drag(Vec<PathBuf>),
    DropOn { paths: Vec<PathBuf>, to: PathBuf, copy: bool },
    Zip { dir: PathBuf, paths: Vec<PathBuf> },
    Extract { dir: PathBuf, paths: Vec<PathBuf> },
    CopyPaths(Vec<PathBuf>),
    Pin(PathBuf),
    Terminal(PathBuf),
    DiskUsage(PathBuf),
    /// Folder sizes for the Size column, one at a time in the background.
    Sizes { dir: PathBuf, names: Vec<String> },
    /// A picture of the file (its thumbnail, or its icon) from a worker.
    Thumb { path: PathBuf, icon: bool, size: i32 },
    Eject(PathBuf),
    JobCancel,
    /// The folder shown changed: other programs that ask the shell what this window shows are told.
    Navigated(PathBuf),
    /// The window exists: it takes the messages the host does not know.
    Start,
    /// Registers the window as a shell window (after the first frame: Windows' shell is asked, which takes a moment).
    Register,
}

pub trait Backend: Send + Sync {
    fn clock(&self) -> Clock;
    /// The rows of `dir` (date headings included when `o.group`), None when it cannot be read.
    fn list(&self, dir: &Path, o: &Opts) -> Option<Vec<Row>>;
    /// Puts `rows` (a listing without date headings) in the order of `o`, with the date headings when `o.group`.
    fn sort(&self, rows: &mut Vec<Row>, o: &Opts);
    /// A folder (or something inside an archive) the address bar may open.
    fn is_target(&self, p: &Path) -> bool;
    /// The bar is there to draw the shell's menu of an item.
    fn bar(&self) -> bool;
    fn places(&self) -> Places;
    fn search(&self, dir: &Path, q: &str, o: &Opts) -> Found;
    fn flag(&self, f: Flag) -> bool;
    fn set_flag(&self, f: Flag, on: bool);
    /// Makes "New folder" / "New text document.txt" in `dir`; the name it got.
    fn new_entry(&self, dir: &Path, folder: bool) -> Option<String>;
    fn rename(&self, dir: &Path, old: &str, new: &str) -> Result<(), String>;
    /// `paths` as files on disk: things inside an archive are taken out of it first.
    fn real_paths(&self, paths: &[PathBuf]) -> Result<Vec<PathBuf>, String>;
    fn facts(&self, path: &Path) -> Facts;
    fn job(&self) -> Option<JobView>;
    /// A copy, packing or unpacking runs (only one at a time).
    fn busy(&self) -> bool;
    /// An app of ours is installed ("photoview", "wiztree").
    fn tool(&self, name: &str) -> bool;
    /// The listing counter: a new listing makes what workers were asked for before stale.
    fn era(&self) -> u32;
    fn run(&self, owner: isize, d: Do);
}

// ---------------------------------------------------------------------------------------------------------------------
// the machine

pub struct Real;

impl Backend for Real {
    fn clock(&self) -> Clock {
        use windows_sys::Win32::{Foundation::FILETIME, Storage::FileSystem::FileTimeToLocalFileTime};
        let now = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() / 100) as i64) + 116_444_736_000_000_000;
        let (utc, mut local) = (FILETIME { dwLowDateTime: now as u32, dwHighDateTime: (now >> 32) as u32 }, FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 });
        // SAFETY: two locals.
        let off = if unsafe { FileTimeToLocalFileTime(&utc, &mut local) } != 0 { ((local.dwHighDateTime as i64) << 32 | local.dwLowDateTime as i64) - now } else { 0 };
        Clock { now, off }
    }

    fn list(&self, dir: &Path, o: &Opts) -> Option<Vec<Row>> {
        use std::sync::atomic::Ordering::Relaxed;
        crate::SORT_COL.store(o.col, Relaxed);
        crate::SORT_ASC.store(o.asc, Relaxed);
        crate::SHOW_HIDDEN.store(o.hidden, Relaxed);
        crate::THUMBS.store(o.icons, Relaxed);
        let rows = crate::rows_of(&dir.to_path_buf());
        if rows.is_some() {
            crate::GEN.fetch_add(1, Relaxed);
        }
        rows
    }

    fn sort(&self, rows: &mut Vec<Row>, o: &Opts) {
        if o.group {
            crate::bucket_refresh();
        }
        crate::sort_rows_with(rows, o.col, o.asc, !o.folders_first, o.group);
        crate::add_headers_with(rows, o.group);
    }

    fn is_target(&self, p: &Path) -> bool {
        p.is_dir() || crate::zipx::split_zip(p).is_some()
    }

    fn bar(&self) -> bool {
        super::live::bar()
    }

    fn places(&self) -> Places {
        super::live::places()
    }

    fn search(&self, dir: &Path, q: &str, _o: &Opts) -> Found {
        crate::search_rows(dir, q)
    }

    fn flag(&self, f: Flag) -> bool {
        match f {
            Flag::Group => crate::group_marker().exists(),
            Flag::FoldersFirst => !crate::mixed_marker().exists(),
        }
    }

    fn set_flag(&self, f: Flag, on: bool) {
        let (k, present) = match f {
            Flag::Group => (crate::group_marker(), on),
            Flag::FoldersFirst => (crate::mixed_marker(), !on),
        };
        if present {
            if let Some(d) = k.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&k, "1");
        } else {
            let _ = std::fs::remove_file(&k);
        }
    }

    fn new_entry(&self, dir: &Path, folder: bool) -> Option<String> {
        use winrust_common::t;
        let base = if folder {
            t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹")
        } else {
            t!("Новый текстовый документ.txt", "New text document.txt", ja: "新しいテキスト ドキュメント.txt", pt: "Novo documento de texto.txt", zh: "新建文本文档.txt")
        };
        crate::new_entry_named(dir, base, folder)
    }

    fn rename(&self, dir: &Path, old: &str, new: &str) -> Result<(), String> {
        std::fs::rename(dir.join(old), dir.join(new)).map_err(|e| e.to_string())
    }

    fn real_paths(&self, paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
        crate::real_paths(paths)
    }

    fn facts(&self, path: &Path) -> Facts {
        let mut f = Facts::default();
        if path.is_dir() && crate::zipx::split_zip(path).is_none() {
            // enough to say "3 folders, 12 files"; a folder of a million entries is not read to its end
            const CAP: usize = 5000;
            let (mut d, mut n, mut cut) = (0, 0, false);
            for (i, e) in std::fs::read_dir(path).into_iter().flatten().flatten().enumerate() {
                if i >= CAP {
                    cut = true;
                    break;
                }
                if e.file_type().is_ok_and(|t| t.is_dir()) { d += 1 } else { n += 1 }
            }
            f.contains = Some((d, n, cut));
        }
        f
    }

    fn job(&self) -> Option<JobView> {
        crate::jobs::progress().map(|(label, frac)| JobView { label, frac })
    }

    fn busy(&self) -> bool {
        crate::jobs::busy()
    }

    fn tool(&self, name: &str) -> bool {
        crate::tool(name).is_some()
    }

    fn era(&self) -> u32 {
        crate::GEN.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn run(&self, owner: isize, d: Do) {
        super::live::run(owner, d);
    }
}
