//! What the desktop and its menus still use of the old GDI look: the system scale, a rounded fill and a line of text for the owner-drawn menus
//! (`menu.rs`), the switch to dark system menus, and the pins of Quick access. The file manager window is `fm` (winrust-ui).
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use std::cell::Cell;
use windows_sys::Win32::{Graphics::Gdi::*, System::LibraryLoader::{GetProcAddress, LoadLibraryW}};

// The colours come from the WinRust style the topbar gear picks (normal, terminal, dock, tiling): the same palette as the bar.
pub use winrust_common::style::{pal, ui_face};

thread_local! {
    static SCALE: Cell<f32> = const { Cell::new(1.0) };
}

pub fn init_scale() {
    unsafe {
        let dc = GetDC(ptr::null_mut());
        SCALE.set(GetDeviceCaps(dc, LOGPIXELSX as i32) as f32 / 96.0);
        ReleaseDC(ptr::null_mut(), dc);
    }
}

pub fn sc(v: i32) -> i32 { (v as f32 * SCALE.get()).round() as i32 }

pub fn font(px: i32, weight: i32, face: &str) -> isize {
    unsafe { CreateFontW(-sc(px), 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, w(face).as_ptr()) as isize }
}

/// Dark context menus / scrollbars for the whole process (uxtheme's unnamed exports 135 and 136). Missing = stays light.
pub fn dark_mode() {
    unsafe {
        let m = LoadLibraryW(w("uxtheme.dll").as_ptr());
        if m.is_null() { return; }
        if let Some(f) = GetProcAddress(m, 135usize as *const u8) { std::mem::transmute::<_, extern "system" fn(i32) -> i32>(f)(2); } // SetPreferredAppMode(ForceDark)
        if let Some(f) = GetProcAddress(m, 136usize as *const u8) { std::mem::transmute::<_, extern "system" fn()>(f)(); } // FlushMenuThemes
    }
}

/// GDI+ for anti-aliased rounded shapes (started once in main); fill() falls back to plain GDI without it.
pub fn gdiplus_start() {
    use windows_sys::Win32::Graphics::GdiPlus::{GdiplusStartup, GdiplusStartupInput};
    unsafe {
        let (mut tok, mut inp): (usize, GdiplusStartupInput) = (0, std::mem::zeroed());
        inp.GdiplusVersion = 1;
        GdiplusStartup(&mut tok, &inp, ptr::null_mut());
    }
}

/// Fills `r` (right / bottom exclusive) in `c`; `rad` > 0 rounds the corners (rad = corner diameter), anti-aliased.
pub(crate) unsafe fn fill(dc: HDC, r: RECT, c: u32, rad: i32) {
    use windows_sys::Win32::Graphics::GdiPlus::*;
    let (wd, ht) = ((r.right - r.left) as f32, (r.bottom - r.top) as f32);
    if wd <= 0.0 || ht <= 0.0 { return; }
    let mut g: *mut GpGraphics = ptr::null_mut();
    if rad <= 0 || GdipCreateFromHDC(dc, &mut g) != 0 || g.is_null() {
        let b = CreateSolidBrush(c);
        if rad <= 0 {
            FillRect(dc, &r, b);
        } else {
            let (ob, op) = (SelectObject(dc, b), SelectObject(dc, GetStockObject(NULL_PEN)));
            RoundRect(dc, r.left, r.top, r.right + 1, r.bottom + 1, rad, rad);
            SelectObject(dc, ob);
            SelectObject(dc, op);
        }
        DeleteObject(b);
        return;
    }
    GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
    GdipSetPixelOffsetMode(g, PixelOffsetModeHalf); // pixel i spans [i, i+1): straight edges stay crisp
    let (x, y, d) = (r.left as f32, r.top as f32, (rad as f32).min(wd).min(ht));
    let mut path: *mut GpPath = ptr::null_mut();
    GdipCreatePath(FillModeAlternate, &mut path);
    GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
    GdipAddPathArc(path, x + wd - d, y, d, d, 270.0, 90.0);
    GdipAddPathArc(path, x + wd - d, y + ht - d, d, d, 0.0, 90.0);
    GdipAddPathArc(path, x, y + ht - d, d, d, 90.0, 90.0);
    GdipClosePathFigure(path);
    let mut br: *mut GpSolidFill = ptr::null_mut();
    GdipCreateSolidFill(0xFF00_0000 | (c & 255) << 16 | (c & 0xFF00) | (c >> 16 & 255), &mut br); // COLORREF -> ARGB
    GdipFillPath(g, br as *mut GpBrush, path);
    GdipDeleteBrush(br as *mut GpBrush);
    GdipDeletePath(path);
    GdipDeleteGraphics(g);
}

pub(crate) unsafe fn text(dc: HDC, s: &str, mut r: RECT, c: u32, flags: u32) {
    SetTextColor(dc, c);
    DrawTextW(dc, w(s).as_ptr(), -1, &mut r, flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
}

/// Folders the user pinned to Quick access: %APPDATA%\WinRust\rust-explorer-pins.txt, one path per line.
fn pins_file() -> Option<PathBuf> { env::var_os("APPDATA").map(|a| PathBuf::from(a).join("WinRust").join("rust-explorer-pins.txt")) }

pub fn pins() -> Vec<PathBuf> {
    pins_file().and_then(|f| fs::read_to_string(f).ok()).map(|s| s.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect()).unwrap_or_default()
}

/// Pins `p` (or unpins it when already pinned).
pub fn toggle_pin(p: &Path) {
    let mut v = pins();
    if let Some(i) = v.iter().position(|x| x == p) { v.remove(i); } else { v.push(p.to_path_buf()); }
    if let Some(f) = pins_file() {
        if let Some(d) = f.parent() { let _ = fs::create_dir_all(d); }
        let _ = fs::write(f, v.iter().map(|x| x.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n"));
    }
}
