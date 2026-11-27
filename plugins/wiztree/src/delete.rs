//! Deleting for WinRust Disk: through the shell's `IFileOperation`, as the file manager does, never `remove_dir_all`. To the Recycle
//! Bin or for good; and nothing at all unless the caller says the user confirmed it (the "How to delete" dialog).
use std::{
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
};
use windows::{
    core::PCWSTR,
    Win32::{
        System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED},
        UI::Shell::{FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    RecycleBin,
    Permanently,
}

/// What may be handed to the shell: an absolute path of something inside a drive, which no part of is a `?` (what a broken scan
/// shows for a folder it lost). Never a drive's root.
fn allowed(p: &Path) -> bool {
    let mut parts = p.components();
    matches!(parts.next(), Some(Component::Prefix(_))) && matches!(parts.next(), Some(Component::RootDir)) && parts.clone().next().is_some() && parts.all(|c| matches!(c, Component::Normal(n) if n != "?"))
}

/// What Windows or every account needs, refused even when confirmed (this program runs as administrator): the system folders at a
/// drive's top, the NTFS `$` files there, a whole user profile, and the folders right inside `Windows` (System32, WinSxS…). What
/// is inside them can still go: a cache, a game, a download.
// ponytail: the names are checked on every drive, not only the system one; a `D:\Users` of a data drive is refused too
fn protected(p: &Path) -> bool {
    let names: Vec<String> = p.components().filter_map(|c| match c {
        Component::Normal(n) => Some(n.to_string_lossy().to_lowercase()),
        _ => None,
    }).collect();
    match names.as_slice() {
        [top] => top.starts_with('$') || matches!(top.as_str(), "windows" | "program files" | "program files (x86)" | "programdata" | "users" | "recovery" | "boot" | "system volume information"),
        [first, _] => first == "users" || first == "windows",
        _ => false,
    }
}

/// A path the deleter accepts: see [`allowed`] and [`protected`]. The app asks before it shows the dialog.
pub fn deletable(p: &Path) -> bool {
    allowed(p) && !protected(p)
}

/// Deletes `paths`. Blocks until the shell is done, so call it from a worker thread. `confirmed` is the dialog's answer: without it
/// nothing is touched. Fails when any path is not [`deletable`], before the shell has been asked about any, and when something is
/// still there afterwards.
pub fn delete(paths: &[PathBuf], how: How, confirmed: bool) -> Result<(), String> {
    if !confirmed {
        return Err("Deletion requires confirmation".into());
    }
    if paths.is_empty() || !paths.iter().all(|p| allowed(p)) {
        return Err("Not a path that can be deleted".into());
    }
    if paths.iter().any(|p| protected(p)) {
        return Err("Windows needs it".into());
    }
    let paths = paths.to_vec();
    // COM wants its own apartment: a thread that is only that
    std::thread::spawn(move || unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|e| e.to_string())?;
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = Com;
        let fo: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        // To the bin the shell may still ask ("too big for the Recycle Bin: delete it for good?"): a question that is not answered
        // for the user, so there is no FOF_NOCONFIRMATION there. For good, the dialog of this program was the question.
        let flags = match how {
            How::RecycleBin => FOF_ALLOWUNDO | FOF_SILENT | FOF_NOERRORUI | FOFX_RECYCLEONDELETE,
            How::Permanently => FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
        };
        fo.SetOperationFlags(flags).map_err(|e| e.to_string())?;
        for p in &paths {
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).map_err(|e| e.to_string())?;
            fo.DeleteItem(&item, None).map_err(|e| e.to_string())?;
        }
        fo.PerformOperations().map_err(|e| e.to_string())?;
        if fo.GetAnyOperationsAborted().map_err(|e| e.to_string())?.as_bool() {
            return Err("Deletion was aborted".into());
        }
        if paths.iter().any(|p| p.symlink_metadata().is_ok()) {
            return Err("Some items could not be deleted".into());
        }
        Ok(())
    })
    .join()
    .map_err(|_| "Deletion thread failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own in the temp directory: the only place a test deletes in.
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!("winrust-disk-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&p).unwrap();
        p
    }

    #[test]
    fn nothing_is_deleted_without_confirmation() {
        let d = temp();
        let p = d.join("file");
        std::fs::write(&p, b"hello").unwrap();
        for how in [How::Permanently, How::RecycleBin] {
            assert!(delete(std::slice::from_ref(&p), how, false).is_err());
            assert!(p.exists());
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_file_and_a_folder_are_deleted_for_good_when_confirmed() {
        let d = temp();
        let (f, dir) = (d.join("file"), d.join("folder"));
        std::fs::write(&f, b"hello").unwrap();
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("inner").join("x.bin"), [0u8; 1000]).unwrap();
        let keep = d.join("keep");
        std::fs::write(&keep, b"stays").unwrap();
        delete(&[f.clone(), dir.clone()], How::Permanently, true).unwrap();
        assert!(!f.exists() && !dir.exists());
        assert!(keep.exists(), "what was not asked for stays");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn what_is_not_a_path_inside_a_drive_is_refused_before_the_shell_hears_of_it() {
        let d = temp();
        let keep = d.join("keep");
        std::fs::write(&keep, b"stays").unwrap();
        // a drive's root, the drive without a slash, a relative path, a lost folder, a parent step, nothing at all
        for bad in ["C:\\", "C:", "keep", "C:\\?\\x.bin", "C:\\a\\..\\b", ""] {
            assert!(delete(&[PathBuf::from(bad)], How::Permanently, true).is_err(), "{bad:?}");
        }
        assert!(delete(&[], How::Permanently, true).is_err());
        // one bad path among good ones: none is deleted
        assert!(delete(&[keep.clone(), PathBuf::from("C:\\")], How::Permanently, true).is_err());
        assert!(keep.exists());
        assert!(allowed(&keep) && allowed(Path::new("D:\\x")) && !allowed(Path::new("D:\\")));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn what_windows_needs_is_refused_even_when_confirmed_but_not_what_is_inside() {
        for bad in ["C:\\Windows", "C:\\windows\\System32", "C:\\Windows\\WinSxS", "C:\\Program Files", "C:\\Program Files (x86)", "C:\\ProgramData", "C:\\Users", "C:\\Users\\Daniiar", "C:\\$MFT", "C:\\System Volume Information", "C:\\Recovery"] {
            assert!(!deletable(Path::new(bad)), "{bad}");
            // refused before the shell hears of it: these paths are never touched
            assert_eq!(delete(&[PathBuf::from(bad)], How::Permanently, true), Err("Windows needs it".into()), "{bad}");
        }
        for ok in ["C:\\Windows\\SoftwareDistribution\\Download", "C:\\Users\\Daniiar\\Downloads", "C:\\Program Files\\Some Game", "C:\\Games", "D:\\Videos\\a.mkv"] {
            assert!(deletable(Path::new(ok)), "{ok}");
        }
    }

    /// Touches the real Recycle Bin of the user: run by hand (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn a_file_goes_to_the_recycle_bin() {
        let d = temp();
        let p = d.join("file");
        std::fs::write(&p, b"hello").unwrap();
        delete(std::slice::from_ref(&p), How::RecycleBin, true).unwrap();
        assert!(!p.exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}
