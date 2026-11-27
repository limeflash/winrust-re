use std::{
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    time::SystemTime,
};
use windows::{
    Win32::{System::Com::*, UI::Shell::*},
    core::PCWSTR,
};

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
pub struct Hidden {
    entry: Entry,
    position: usize,
}

pub fn is_pic(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| crate::EXTS.contains(&e.to_ascii_lowercase().as_str()))
}
#[link(name = "shlwapi")]
unsafe extern "system" {
    fn StrCmpLogicalW(a: *const u16, b: *const u16) -> i32;
}
fn wide(p: &std::ffi::OsStr) -> Vec<u16> {
    p.encode_wide().chain([0]).collect()
}
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let a = wide(a.as_ref());
    let b = wide(b.as_ref());
    unsafe { StrCmpLogicalW(a.as_ptr(), b.as_ptr()).cmp(&0) }
}
impl Folder {
    pub fn open(path: &Path) -> Option<Self> {
        let is_dir = path.is_dir();
        if !is_dir && (!path.is_file() || !is_pic(path)) {
            return None;
        }
        let dir = if is_dir { path } else { path.parent()? };
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| {
                let e = e.ok()?;
                let path = e.path();
                let m = e.metadata().ok()?;
                if !m.is_file() || !is_pic(&path) {
                    return None;
                }
                Some(Entry {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path,
                    size: m.len(),
                    modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                })
            })
            .collect();
        entries.sort_by(|a, b| natural(&a.name, &b.name));
        if entries.is_empty() {
            return None;
        }
        let index = if is_dir {
            0
        } else {
            entries
                .iter()
                .position(|e| e.path.as_os_str().eq_ignore_ascii_case(path.as_os_str()))?
        };
        Some(Self { entries, index })
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.index)
    }
    pub fn go(&mut self, i: usize) {
        self.index = i.min(self.entries.len().saturating_sub(1));
    }
    pub fn next(&mut self) {
        if !self.entries.is_empty() {
            self.index = (self.index + 1) % self.entries.len();
        }
    }
    pub fn prev(&mut self) {
        if !self.entries.is_empty() {
            self.index = (self.index + self.entries.len() - 1) % self.entries.len();
        }
    }
    pub fn hide_current(&mut self) -> Option<Hidden> {
        if self.entries.is_empty() {
            return None;
        }
        let position = self.index;
        let entry = self.entries.remove(position);
        if self.index >= self.entries.len() {
            self.index = 0;
        }
        Some(Hidden { entry, position })
    }
    pub fn undo(&mut self, h: Hidden) {
        let i = h.position.min(self.entries.len());
        self.entries.insert(i, h.entry);
        self.index = i;
    }
    pub fn commit(h: Hidden) -> Result<(), String> {
        // IFileOperation requires an STA; isolate it from the caller's COM apartment.
        std::thread::spawn(move || unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(|e| e.to_string())?;
            let result = (|| -> windows::core::Result<()> {
                let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
                op.SetOperationFlags(
                    FOF_ALLOWUNDO
                        | FOF_SILENT
                        | FOF_NOCONFIRMATION
                        | FOF_NOERRORUI
                        | FOFX_RECYCLEONDELETE,
                )?;
                let name = wide(h.entry.path.as_os_str());
                let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None)?;
                op.DeleteItem(&item, None)?;
                op.PerformOperations()?;
                if op.GetAnyOperationsAborted()?.as_bool() {
                    return Err(windows::core::Error::new(
                        windows::core::HRESULT(0x80004004u32 as i32),
                        "Recycle operation aborted",
                    ));
                }
                Ok(())
            })();
            CoUninitialize();
            result.map_err(|e| e.to_string())
        })
        .join()
        .map_err(|_| "Recycle worker panicked".to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_navigation_and_undo() {
        let dir = std::env::temp_dir().join(format!(
            "photoview-folder-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["img10.png", "IMG2.png", "img1.jpg", "other.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let mut f = Folder::open(&dir).unwrap();
        assert_eq!(
            f.entries()
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["img1.jpg", "IMG2.png", "img10.png"]
        );
        assert_eq!(Folder::open(&dir.join("IMG2.png")).unwrap().index(), 1);
        f.prev();
        assert_eq!(f.index(), 2);
        f.next();
        assert_eq!(f.index(), 0);
        f.go(1);
        let h = f.hide_current().unwrap();
        assert_eq!(f.current().unwrap().name, "img10.png");
        f.undo(h);
        assert_eq!(f.index(), 1);
        assert_eq!(f.current().unwrap().name, "IMG2.png");
        assert_eq!(f.entries().len(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod recycle_test {
    #[test]
    fn commits_temp_file_to_recycle_bin() {
        let dir = std::env::temp_dir().join(format!(
            "photoview-recycle-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("recycle.png");
        std::fs::write(&file, b"test").unwrap();
        let mut folder = super::Folder::open(&file).unwrap();
        super::Folder::commit(folder.hide_current().unwrap()).unwrap();
        assert!(!file.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
