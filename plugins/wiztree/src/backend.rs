//! What WinRust Disk asks of the machine: which drives there are, whether it may read a raw volume, the scans, the deletion, a
//! folder shown in the file manager, the UAC prompt. The window talks to a [`Backend`], so a render, a benchmark and a test run it
//! on a fake that touches nothing; only [`Real`] ever reads a drive or deletes a file.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};

use windows_sys::Win32::{
    Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW},
    UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};
use winrust_common::mft::{self, Rec, ROOT};

use crate::delete::{self, How};
use crate::model::{build, DiskData};

/// A drive in the title bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriveInfo {
    pub letter: char,
    /// "Local Disk", "Data", "USB Drive": the volume's name, or a name for what it is.
    pub label: String,
    /// "NTFS", "exFAT", ..
    pub fs: String,
    pub total: u64,
    pub free: u64,
    pub removable: bool,
}

impl DriveInfo {
    #[must_use]
    pub fn ntfs(&self) -> bool {
        self.fs.eq_ignore_ascii_case("NTFS")
    }
}

pub trait Backend: Send + Sync {
    fn drives(&self) -> Vec<DriveInfo>;
    /// Whether the program may open a raw volume (the file table).
    fn is_admin(&self) -> bool;
    /// Reads a whole drive. `slow` walks the folders instead of reading the NTFS file table. `progress(done, of)`. Blocks: call it
    /// from a worker.
    fn scan(&self, drive: char, slow: bool, progress: &mut dyn FnMut(u64, u64)) -> Result<DiskData, String>;
    /// See [`delete::delete`]; blocks too.
    fn delete(&self, paths: &[PathBuf], how: How, confirmed: bool) -> Result<(), String>;
    /// Shows `path` in the file manager.
    fn reveal(&self, path: &str);
    /// Starts this program again with administrator rights (the UAC prompt) for `drive`; whether the user agreed.
    fn restart_elevated(&self, drive: char) -> bool;
    /// Runs `job` on a thread of its own (a test backend runs it on the spot, so that it can look at the outcome).
    fn spawn(&self, job: Box<dyn FnOnce() + Send>);
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn from_wide(b: &[u16]) -> String {
    String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
}

/// The machine.
pub struct Real;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

impl Backend for Real {
    fn drives(&self) -> Vec<DriveInfo> {
        // SAFETY: plain queries with buffers of our own.
        let mask = unsafe { GetLogicalDrives() };
        ('A'..='Z')
            .filter(|c| mask & (1 << (*c as u8 - b'A')) != 0)
            .filter_map(|c| {
                let root = wide(&format!("{c}:\\"));
                let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
                if kind != DRIVE_FIXED && !(kind == DRIVE_REMOVABLE && c > 'B') {
                    return None;
                }
                let (mut free, mut total, mut any) = (0u64, 0u64, 0u64);
                // SAFETY: out-parameters are locals.
                if unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut free, &mut total, &mut any) } == 0 || total == 0 {
                    return None; // an empty card reader
                }
                let (mut name, mut fs) = ([0u16; 261], [0u16; 64]);
                // SAFETY: the buffers are as long as is said.
                unsafe { GetVolumeInformationW(root.as_ptr(), name.as_mut_ptr(), 261, null_mut(), null_mut(), null_mut(), fs.as_mut_ptr(), 64) };
                let label = from_wide(&name);
                let removable = kind == DRIVE_REMOVABLE;
                Some(DriveInfo { letter: c, label: if label.is_empty() { (if removable { "USB Drive" } else { "Local Disk" }).into() } else { label }, fs: from_wide(&fs), total, free, removable })
            })
            .collect()
    }

    fn is_admin(&self) -> bool {
        // SAFETY: a plain query.
        unsafe { IsUserAnAdmin() != 0 }
    }

    fn scan(&self, drive: char, slow: bool, progress: &mut dyn FnMut(u64, u64)) -> Result<DiskData, String> {
        let recs = if slow {
            let used = self.drives().into_iter().find(|d| d.letter == drive).map_or(0, |d| d.total.saturating_sub(d.free));
            walk(Path::new(&format!("{drive}:\\")), used, progress)
        } else {
            mft::scan_with(drive, progress)?
        };
        Ok(DiskData::read(build(recs), drive))
    }

    fn delete(&self, paths: &[PathBuf], how: How, confirmed: bool) -> Result<(), String> {
        delete::delete(paths, how, confirmed)
    }

    fn reveal(&self, path: &str) {
        winrust_common::reveal(path);
    }

    fn restart_elevated(&self, drive: char) -> bool {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        // SAFETY: NUL-terminated strings that live for the call.
        let r = unsafe { ShellExecuteW(null_mut(), wide("runas").as_ptr(), wide(&exe).as_ptr(), wide(&format!("{drive}:")).as_ptr(), null(), SW_SHOWNORMAL) };
        r as usize > 32
    }

    fn spawn(&self, job: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(job);
    }
}

/// The slow scan: every folder under `root` read with the ordinary calls, the way a program without administrator rights (or on a
/// drive that is not NTFS) has to. A folder it cannot open is left out. Links are not followed. `used` is how much of the drive is in
/// use, to say how far along it is: `progress(files so far, files there will be)`.
pub fn walk(root: &Path, used: u64, progress: &mut dyn FnMut(u64, u64)) -> HashMap<u32, Rec> {
    let mut recs = HashMap::new();
    recs.insert(ROOT, Rec { parent: ROOT, name: ".".into(), dir: true, alloc: 0, logical: 0 });
    // 8 is the record the MFT reader skips ($BadClus): the numbers start above the reserved ones
    let mut next = 16u32;
    let mut stack = vec![(root.to_path_buf(), ROOT)];
    let (mut files, mut bytes) = (0u64, 0u64);
    while let Some((dir, parent)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let id = next;
            next += 1;
            let name = e.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                recs.insert(id, Rec { parent, name, dir: true, alloc: 0, logical: 0 });
                stack.push((e.path(), id));
            } else {
                let len = e.metadata().map_or(0, |m| m.len());
                // ponytail: a 4 KB cluster for every drive; exFAT cards have bigger ones, the sizes are a little short there
                let alloc = len.div_ceil(4096) * 4096;
                recs.insert(id, Rec { parent, name, dir: false, alloc, logical: len });
                (files, bytes) = (files + 1, bytes + alloc);
                if files % 2000 == 0 {
                    let of = if bytes > 0 { (files as f64 * used as f64 / bytes as f64) as u64 } else { files };
                    progress(files, of.max(files + 1));
                }
            }
        }
    }
    progress(files, files);
    recs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slow_scan_reads_a_folder_with_the_sizes_of_its_files() {
        let d = std::env::temp_dir().join(format!("winrust-disk-walk-{}", std::process::id()));
        std::fs::create_dir_all(d.join("a").join("b")).unwrap();
        std::fs::write(d.join("one.bin"), [0u8; 5000]).unwrap();
        std::fs::write(d.join("a").join("two.bin"), [0u8; 100]).unwrap();
        std::fs::write(d.join("a").join("b").join("three.bin"), [0u8; 4096]).unwrap();
        std::fs::write(d.join("empty.txt"), b"").unwrap();
        let mut seen = Vec::new();
        let recs = walk(&d, 1_000_000, &mut |f, of| seen.push((f, of)));
        std::fs::remove_dir_all(&d).unwrap();
        let t = build(recs);
        assert_eq!(t.tot[&ROOT], 8192 + 4096 + 4096, "5000 -> two clusters, 100 -> one, 4096 -> one, nothing -> nothing");
        assert_eq!(t.kids[&ROOT].len(), 2, "the folder and the file of some size; the empty file is no tile");
        assert_eq!(seen.last(), Some(&(4, 4)), "it ends with the whole count");
        assert!(t.recs.keys().all(|&id| id == ROOT || id >= 16));
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_empty_scan_not_a_failure() {
        let recs = walk(Path::new("Z:\\no\\such\\folder\\winrust"), 0, &mut |_, _| {});
        assert_eq!(recs.len(), 1);
    }
}
