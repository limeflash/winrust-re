//! topbar: shot (split out of main.rs; shares its items through `super::*`).
use super::*;

/// PrtSc without explorer: the whole desktop (all monitors) to the clipboard and to Pictures\Screenshots\<date time>.png.
/// The system's own capture overlay (ms-screenclip:) does not come up without explorer as the shell. Returns what was done.
/// `topbar --shot` does the same from a command line.
pub(crate) unsafe fn screenshot() -> Option<(Saved, (u32, u32))> {
    let prev = windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(-4isize as _); // real pixels of every monitor (#77)
    let (x, y, w, h) = (GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79)); // virtual screen
    if w <= 0 || h <= 0 {
        windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(prev);
        return None;
    }
    let (scr, mem) = (GetDC(null_mut()), CreateCompatibleDC(GetDC(null_mut())));
    let bmp = CreateCompatibleBitmap(scr, w, h);
    let old = SelectObject(mem, bmp);
    BitBlt(mem, 0, 0, w, h, scr, x, y, SRCCOPY | 0x4000_0000); // CAPTUREBLT: layered windows (the wallpaper, overlays) too
    SelectObject(mem, old);
    DeleteDC(mem);
    ReleaseDC(null_mut(), scr);
    windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(prev);
    save_bitmap(bmp, true, true).map(|s| (s, (w as u32, h as u32)))
}

/// What `save_bitmap` did.
pub(crate) struct Saved {
    /// The file's name (it is in Pictures\Screenshots when `saved`).
    pub name: String,
    pub saved: bool,
    pub copied: bool,
}

const PNG_ENCODER: windows_sys::core::GUID = windows_sys::core::GUID { data1: 0x557c_f406, data2: 0x1a04, data3: 0x11d3, data4: [0x9a, 0x73, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e] };

/// Writes a bitmap as a PNG file. The bitmap stays the caller's.
pub(crate) unsafe fn write_png(bmp: HBITMAP, path: &std::path::Path) -> bool {
    use windows_sys::Win32::Graphics::GdiPlus::{GdipCreateBitmapFromHBITMAP, GdipDisposeImage, GdipSaveImageToFile, GpBitmap, GpImage};
    let mut gb: *mut GpBitmap = null_mut();
    let mut done = false;
    if GdipCreateBitmapFromHBITMAP(bmp, null_mut(), &mut gb) == 0 && !gb.is_null() {
        done = GdipSaveImageToFile(gb as *mut GpImage, wide(&path.to_string_lossy()).as_ptr(), &PNG_ENCODER, null()) == 0;
        GdipDisposeImage(gb as *mut GpImage);
    }
    done
}

/// Saves a screen bitmap as Pictures\Screenshots\Screenshot <date time>.png (`save`) and puts it on the clipboard (`clip`, which
/// then owns it). The bitmap is the caller's no longer. Returns what was done, `None` when neither worked.
pub(crate) unsafe fn save_bitmap(bmp: HBITMAP, clip: bool, save: bool) -> Option<Saved> {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    let mut t: SYSTEMTIME = zeroed();
    GetLocalTime(&mut t);
    let name = format!("Screenshot {:04}-{:02}-{:02} {:02}-{:02}-{:02}.png", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
    let dir = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(r"Pictures\Screenshots");
    let _ = std::fs::create_dir_all(&dir);
    let saved = save && write_png(bmp, &dir.join(&name));
    let mut copied = false;
    if clip && OpenClipboard(null_mut()) != 0 {
        EmptyClipboard();
        copied = !SetClipboardData(2, bmp as _).is_null(); // CF_BITMAP: the clipboard owns the bitmap from here on
        CloseClipboard();
    }
    if !copied {
        DeleteObject(bmp);
    }
    (saved || copied).then_some(Saved { name, saved, copied })
}

/// The snip tool finished (or failed): the notifications say so. Runs on the bar's thread.
pub(crate) unsafe fn clip_notice(bar: HWND) {
    let _ = bar;
    snip::tell();
}

/// PrtSc pressed: take the screenshot; a notification says it was saved.
pub(crate) unsafe fn screenshot_notice(bar: HWND) {
    let _ = bar;
    match screenshot() {
        Some((s, size)) => notify::screenshot_done(s.saved.then_some(s.name.as_str()), Some(size), s.copied, None),
        None => notify::screenshot_failed(),
    }
}
