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

/// Saves a screen bitmap as Pictures\Screenshots\Screenshot <date time>.png (`save`) and puts it on the clipboard (`clip`).
/// The bitmap is deleted here. Returns what was done, `None` when neither worked.
pub(crate) unsafe fn save_bitmap(bmp: HBITMAP, clip: bool, save: bool) -> Option<Saved> {
    let mut t: SYSTEMTIME = zeroed();
    GetLocalTime(&mut t);
    let name = format!("Screenshot {:04}-{:02}-{:02} {:02}-{:02}-{:02}.png", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
    let dir = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(r"Pictures\Screenshots");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(&name);
    let saved = save && write_png(bmp, &path);
    let copied = clip && to_clipboard(bmp, saved.then_some(path.as_path()));
    DeleteObject(bmp);
    (saved || copied).then_some(Saved { name, saved, copied })
}

/// Puts a screen bitmap on the clipboard the way image editors and browsers read it: a PNG and a 32-bit DIB (Windows makes the
/// other formats from the DIB). A bare CF_BITMAP from a thread without a window was not something a Chromium-based app could
/// paste. `png` is the file the picture was saved to, if it was.
unsafe fn to_clipboard(bmp: HBITMAP, png: Option<&std::path::Path>) -> bool {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
    use windows_sys::Win32::{Foundation::GlobalFree, System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock}};
    let global = |bytes: &[u8]| {
        let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        let p = GlobalLock(g).cast::<u8>();
        if p.is_null() {
            GlobalFree(g);
            return null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        GlobalUnlock(g);
        g
    };
    // the pixels, 32-bit bottom-up, opaque (a screen has no transparency; a reader that honours alpha must not see a clear picture)
    let mut info: BITMAP = zeroed();
    if GetObjectW(bmp, size_of::<BITMAP>() as i32, (&raw mut info).cast()) == 0 {
        return false;
    }
    let (w, h) = (info.bmWidth, info.bmHeight.abs());
    let mut dib = vec![0u8; 40 + (w as usize) * (h as usize) * 4];
    let head = BITMAPINFOHEADER { biSize: 40, biWidth: w, biHeight: h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, biSizeImage: (w * h * 4) as u32, ..zeroed() };
    std::ptr::copy_nonoverlapping((&raw const head).cast::<u8>(), dib.as_mut_ptr(), 40);
    let dc = GetDC(null_mut());
    let rows = GetDIBits(dc, bmp, 0, h as u32, dib.as_mut_ptr().add(40).cast(), dib.as_mut_ptr().cast(), DIB_RGB_COLORS);
    ReleaseDC(null_mut(), dc);
    if rows == 0 {
        return false;
    }
    dib[40..].chunks_exact_mut(4).for_each(|p| p[3] = 255);
    // the PNG: the saved file's bytes, or a temporary file's
    let temp = std::env::temp_dir().join(format!("winrust-clip-{}.png", std::process::id()));
    let png_bytes = match png {
        Some(p) => std::fs::read(p).ok(),
        None => write_png(bmp, &temp).then(|| std::fs::read(&temp).ok()).flatten(),
    };
    let _ = std::fs::remove_file(&temp);
    // the bar's window owns what is on the clipboard (opened with no window, Windows may refuse the data)
    if OpenClipboard(BAR.load(Relaxed) as HWND) == 0 {
        return false;
    }
    EmptyClipboard();
    let g = global(&dib);
    let mut ok = !g.is_null() && !SetClipboardData(8, g).is_null(); // CF_DIB: the clipboard owns the memory from here on
    if !ok && !g.is_null() {
        GlobalFree(g);
    }
    if let Some(bytes) = png_bytes {
        let g = global(&bytes);
        let fmt = RegisterClipboardFormatW(wide("PNG").as_ptr());
        if !g.is_null() && !SetClipboardData(fmt, g).is_null() {
            ok = true;
        } else if !g.is_null() {
            GlobalFree(g);
        }
    }
    CloseClipboard();
    ok
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
