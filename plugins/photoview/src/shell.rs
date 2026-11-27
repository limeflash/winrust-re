//! What the viewer asks of the system: the wallpaper, the clipboard, Explorer, the file dialog. Nothing here runs in a test or a
//! render (the viewer's `live` flag keeps them out); each is one call, and says in its comment what it does to the user's machine.
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
    },
    UI::{
        Controls::Dialogs::{GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW},
        WindowsAndMessaging::{SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW},
    },
};

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    std::os::windows::ffi::OsStrExt::encode_wide(s).chain([0]).collect()
}

/// Makes `path` the desktop picture of the user (`SPI_SETDESKWALLPAPER`, written to the registry and broadcast with
/// `WM_SETTINGCHANGE`). With Explorer as the shell that is all. rs-shell's desktop (rust-explorer `--desktop`) paints its own copy,
/// `%LOCALAPPDATA%\rs-shell\wallpaper.bmp`, which rs-shell makes from the registry value once at its start and rust-explorer reads
/// once: neither listens for this change, so there the new wallpaper shows after the next sign-in.
pub fn set_wallpaper(path: &Path) -> bool {
    let w = wide(path.as_os_str());
    // SAFETY: the path is NUL-terminated and lives for the call.
    unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, w.as_ptr() as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) != 0 }
}

/// Opens the folder of `path` in the file manager with the file selected.
pub fn show_in_folder(path: &Path) {
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
    let w = wide(path.as_os_str());
    // SAFETY: an item id list made from a path and freed after the call; COM is up on the UI thread.
    unsafe {
        let pidl = ILCreateFromPathW(windows::core::PCWSTR(w.as_ptr()));
        if !pidl.is_null() {
            let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
            ILFree(Some(pidl));
        }
    }
}

/// Puts a picture on the clipboard as a device-independent bitmap (`CF_DIB`): premultiplied BGRA pixels, top row first, `w` × `h`.
/// Transparent parts come out white. Replaces what the clipboard held. Returns whether it worked.
pub fn copy_picture(px: &[u32], w: u32, h: u32) -> bool {
    let (w, h) = (w as usize, h as usize);
    if px.len() < w * h || w == 0 || h == 0 {
        return false;
    }
    let header = 40usize;
    let bytes = header + w * h * 4;
    // SAFETY: the memory is written while locked, and either given to the clipboard (which owns it then) or freed.
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if mem.is_null() {
            return false;
        }
        let p = GlobalLock(mem).cast::<u8>();
        if p.is_null() {
            GlobalFree(mem);
            return false;
        }
        let head = std::slice::from_raw_parts_mut(p, header);
        head.fill(0);
        head[0..4].copy_from_slice(&40u32.to_le_bytes());
        head[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        head[8..12].copy_from_slice(&(h as i32).to_le_bytes()); // positive: the rows are bottom first
        head[12..14].copy_from_slice(&1u16.to_le_bytes());
        head[14..16].copy_from_slice(&32u16.to_le_bytes());
        let body = std::slice::from_raw_parts_mut(p.add(header).cast::<u32>(), w * h);
        for (y, row) in body.chunks_exact_mut(w).enumerate() {
            let src = &px[(h - 1 - y) * w..(h - y) * w];
            for (d, s) in row.iter_mut().zip(src) {
                let inv = 255 - (s >> 24);
                let over_white = |c: u32| (((s >> c) & 255) + inv).min(255) << c;
                *d = over_white(16) | over_white(8) | over_white(0);
            }
        }
        GlobalUnlock(mem);
        if OpenClipboard(null_mut()) == 0 {
            GlobalFree(mem);
            return false;
        }
        EmptyClipboard();
        let ok = !SetClipboardData(8, mem).is_null(); // CF_DIB
        if !ok {
            GlobalFree(mem);
        }
        CloseClipboard();
        ok
    }
}

/// The file dialog for "Browse Pictures": one picture, `None` if it is cancelled.
pub fn browse(owner: isize) -> Option<PathBuf> {
    let filter: Vec<u16> = "Pictures\0*.jpg;*.jpeg;*.jfif;*.png;*.bmp;*.gif;*.tif;*.tiff;*.webp;*.heic;*.avif\0All files\0*.*\0\0".encode_utf16().collect();
    let mut file = vec![0u16; 4096];
    // SAFETY: the structure and its buffers live for the call; the dialog is modal over our window.
    unsafe {
        let mut o: OPENFILENAMEW = std::mem::zeroed();
        o.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        o.hwndOwner = owner as HWND;
        o.lpstrFilter = filter.as_ptr();
        o.lpstrFile = file.as_mut_ptr();
        o.nMaxFile = file.len() as u32;
        o.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut o) == 0 {
            return None;
        }
    }
    let n = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..n])))
}
