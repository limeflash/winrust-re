//! The pictures of one folder in natural order (`img2` before `img10`), the one shown, and a deletion that can still be taken
//! back: a picture is hidden from the list first and goes to the Recycle Bin only when the viewer commits it.
use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    time::SystemTime,
};
use windows::{
    Win32::{System::Com::*, UI::Shell::*},
    core::PCWSTR,
};

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: SystemTime,
}

pub struct Folder {
    entries: Vec<Entry>,
    index: usize,
}

/// A picture taken out of the list, with the place it had.
#[derive(Clone, Debug)]
pub struct Hidden {
    pub entry: Entry,
    position: usize,
}

pub fn is_pic(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| crate::EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

#[link(name = "shlwapi")]
unsafe extern "system" {
    fn StrCmpLogicalW(a: *const u16, b: *const u16) -> i32;
}

fn wide(p: &std::ffi::OsStr) -> Vec<u16> {
    p.encode_wide().chain([0]).collect()
}

/// Explorer's order: digits compare as numbers, case does not matter.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (wide(a.as_ref()), wide(b.as_ref()));
    // SAFETY: both strings are NUL-terminated and live for the call.
    unsafe { StrCmpLogicalW(a.as_ptr(), b.as_ptr()).cmp(&0) }
}

impl Folder {
    /// The pictures next to `path` (a picture), or those in it (a folder, the first one shown). `None` when there is none.
    pub fn open(path: &Path) -> Option<Self> {
        let path = std::path::absolute(path).ok()?; // `x.jpg` has the parent "": the folder is the current one
        let is_dir = path.is_dir();
        if !is_dir && (!path.is_file() || !is_pic(&path)) {
            return None;
        }
        let dir = if is_dir { path.as_path() } else { path.parent()? };
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| {
                let e = e.ok()?;
                let path = e.path();
                let m = e.metadata().ok()?;
                if !m.is_file() || !is_pic(&path) {
                    return None;
                }
                Some(Entry { name: e.file_name().to_string_lossy().into_owned(), path, size: m.len(), modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH) })
            })
            .collect();
        entries.sort_by(|a, b| natural(&a.name, &b.name));
        if entries.is_empty() {
            return None;
        }
        let index = if is_dir { 0 } else { entries.iter().position(|e| e.path.as_os_str().eq_ignore_ascii_case(path.as_os_str()))? };
        Some(Self { entries, index })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.index)
    }

    /// Shows picture `i` (the last one if `i` is past the end). The list does not wrap: the first picture has nothing before it.
    pub fn go(&mut self, i: usize) {
        self.index = i.min(self.entries.len().saturating_sub(1));
    }

    /// Takes the shown picture out of the list. The next one is shown; after the last one, the one before it.
    pub fn hide_current(&mut self) -> Option<Hidden> {
        if self.index >= self.entries.len() {
            return None;
        }
        let position = self.index;
        let entry = self.entries.remove(position);
        self.index = self.index.min(self.entries.len().saturating_sub(1));
        Some(Hidden { entry, position })
    }

    /// Undo: the picture is back where it was and is shown again.
    pub fn undo(&mut self, h: Hidden) {
        let i = h.position.min(self.entries.len());
        self.entries.insert(i, h.entry);
        self.index = i;
    }

    /// The picture could not be moved to the Recycle Bin after all: it is back in the list, and what is shown stays shown.
    pub fn put_back(&mut self, h: Hidden) {
        let i = h.position.min(self.entries.len());
        let was_empty = self.entries.is_empty();
        self.entries.insert(i, h.entry);
        if !was_empty && i <= self.index {
            self.index += 1;
        }
    }

    /// Moves the hidden picture to the Recycle Bin (it can be restored from there). Blocks until the shell is done: call it from a
    /// worker thread. The shell's file operation needs an apartment of its own, so it gets a thread of its own too.
    pub fn commit(h: &Hidden) -> Result<(), String> {
        let path = h.entry.path.clone();
        std::thread::spawn(move || {
            // SAFETY: the file operation is made, used and released on this thread.
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|e| e.to_string())?;
                let result = (|| -> windows::core::Result<()> {
                    let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
                    op.SetOperationFlags(FOF_ALLOWUNDO | FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOFX_RECYCLEONDELETE)?;
                    let name = wide(path.as_os_str());
                    let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None)?;
                    op.DeleteItem(&item, None)?;
                    op.PerformOperations()?;
                    if op.GetAnyOperationsAborted()?.as_bool() {
                        return Err(windows::core::Error::new(windows::core::HRESULT(0x8000_4004u32 as i32), "Recycle operation aborted"));
                    }
                    Ok(())
                })();
                CoUninitialize();
                result.map_err(|e| e.to_string())
            }
        })
        .join()
        .map_err(|_| "Recycle worker panicked".to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(names: &[&str], tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photoview-folder-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        dir
    }

    fn names(f: &Folder) -> Vec<&str> {
        f.entries().iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn natural_order_and_the_start_picture() {
        let dir = dir_with(&["img10.png", "IMG2.png", "img1.jpg", "other.txt"], "order");
        let mut f = Folder::open(&dir).unwrap();
        assert_eq!(names(&f), ["img1.jpg", "IMG2.png", "img10.png"], "2 before 10, case ignored, text files left out");
        assert_eq!(f.index(), 0);
        assert_eq!(Folder::open(&dir.join("IMG2.png")).unwrap().index(), 1, "a picture is shown, not the first one");
        f.go(99);
        assert_eq!(f.index(), 2, "the list does not wrap");
        f.go(0);
        assert_eq!(f.current().unwrap().name, "img1.jpg");
        assert!(Folder::open(&dir.join("other.txt")).is_none(), "a text file opens nothing");
        assert!(Folder::open(&dir.join("gone.png")).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hide_undo_and_put_back_keep_the_place() {
        let dir = dir_with(&["a.png", "b.png", "c.png", "d.png"], "hide");
        let mut f = Folder::open(&dir.join("b.png")).unwrap();
        let h = f.hide_current().unwrap();
        assert_eq!((names(&f), f.current().unwrap().name.as_str()), (vec!["a.png", "c.png", "d.png"], "c.png"), "the next picture is shown");
        f.undo(h);
        assert_eq!((f.index(), f.current().unwrap().name.as_str(), f.len()), (1, "b.png", 4), "Undo shows the picture again where it was");

        f.go(3);
        let last = f.hide_current().unwrap();
        assert_eq!(f.current().unwrap().name, "c.png", "after the last picture, the one before it (not the first)");
        f.put_back(last);
        assert_eq!((f.current().unwrap().name.as_str(), f.len()), ("c.png", 4), "put back after the shown one: the shown one stays");
        f.go(2);
        let before = f.clone_hidden_for_test(0);
        f.put_back(before);
        assert_eq!(f.current().unwrap().name, "c.png", "put back before the shown one: it is pushed along and still shown");

        let mut one = Folder::open(&dir.join("a.png")).unwrap();
        while one.hide_current().is_some() {}
        assert!(one.len() == 0 && one.current().is_none() && one.hide_current().is_none(), "an empty folder shows nothing");
        std::fs::remove_dir_all(dir).unwrap();
    }

    impl Folder {
        /// A copy of the entry at `at` as a `Hidden` that was taken from there (the list keeps it too: a test fixture).
        fn clone_hidden_for_test(&mut self, at: usize) -> Hidden {
            let mut h = self.entries[at].clone();
            h.name = format!("x-{}", h.name);
            Hidden { entry: h, position: at }
        }
    }

    #[test]
    #[ignore = "puts a temp file in the real Recycle Bin: run with --ignored"]
    fn commits_temp_file_to_recycle_bin() {
        let dir = dir_with(&["recycle.png"], "bin");
        let file = dir.join("recycle.png");
        let mut folder = Folder::open(&file).unwrap();
        Folder::commit(&folder.hide_current().unwrap()).unwrap();
        assert!(!file.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
