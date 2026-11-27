#![allow(dead_code)]
use std::{os::windows::ffi::OsStrExt, path::{Path, PathBuf}};
use windows::{core::PCWSTR, Win32::{System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED}, UI::Shell::{FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOFX_RECYCLEONDELETE}}};

#[derive(Clone, Copy)]
pub enum How { RecycleBin, Permanently }
fn size(path: &Path) -> Result<u64, String> {
    let m = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.file_type().is_symlink() { return Ok(m.len()); }
    let mut sum = 0u64;
    for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? { sum = sum.saturating_add(size(&entry.map_err(|e| e.to_string())?.path())?); }
    Ok(sum)
}
pub fn delete(paths: &[PathBuf], how: How, confirmed: bool) -> Result<u64, String> {
    if !confirmed { return Err("Deletion requires confirmation".into()); }
    let paths = paths.to_vec();
    std::thread::spawn(move || unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(|e| e.to_string())?;
        struct Com; impl Drop for Com { fn drop(&mut self) { unsafe { CoUninitialize() } } } let _com = Com;
        let mut bytes = 0u64;
        for p in &paths { bytes = bytes.saturating_add(size(p)?); }
        let fo: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let mut flags = FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI;
        if matches!(how, How::RecycleBin) { flags |= FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE; }
        fo.SetOperationFlags(flags).map_err(|e| e.to_string())?;
        for p in &paths {
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).map_err(|e| e.to_string())?;
            fo.DeleteItem(&item, None).map_err(|e| e.to_string())?;
        }
        fo.PerformOperations().map_err(|e| e.to_string())?;
        if fo.GetAnyOperationsAborted().map_err(|e| e.to_string())?.as_bool() { return Err("Deletion was aborted".into()); }
        if paths.iter().any(|p| p.exists()) { return Err("Some items could not be deleted".into()); }
        Ok(bytes)
    }).join().map_err(|_| "Deletion thread failed".to_string())?
}
#[cfg(test)] mod tests {
    use super::*;
    fn temp() -> PathBuf { let p = std::env::temp_dir().join(format!("winrust-disk-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())); std::fs::create_dir(&p).unwrap(); p }
    #[test] fn confirmation_required() { let d=temp(); let p=d.join("file"); std::fs::write(&p, b"hello").unwrap(); assert!(delete(std::slice::from_ref(&p), How::Permanently, false).is_err()); assert!(p.exists()); std::fs::remove_dir_all(d).unwrap(); }
    #[test] fn permanent_temp_file() { let d=temp(); let p=d.join("file"); std::fs::write(&p, b"hello").unwrap(); assert_eq!(delete(std::slice::from_ref(&p), How::Permanently, true).unwrap(), 5); assert!(!p.exists()); std::fs::remove_dir_all(d).unwrap(); }
    #[test] #[ignore] fn recycle_temp_file() { let d=temp(); let p=d.join("file"); std::fs::write(&p, b"hello").unwrap(); assert_eq!(delete(std::slice::from_ref(&p), How::RecycleBin, true).unwrap(), 5); assert!(!p.exists()); std::fs::remove_dir_all(d).unwrap(); }
}
