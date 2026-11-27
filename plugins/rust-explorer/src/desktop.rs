use super::*;
use winrust_common::deskmenu::{self, Cmd, IconSize, Request, Sort};

const CLR_NONE: isize = 0xFFFF_FFFF; // list background / text background: let the parent's wallpaper show through
thread_local! { static WALL: std::cell::Cell<(isize, i32, i32)> = const { std::cell::Cell::new((0, 0, 0)) }; } // (HBITMAP, w, h)
/// The virtual screen (every monitor): x, y, width, height. The desktop window covers it so each monitor gets its wallpaper.
pub(crate) fn virt() -> (i32, i32, i32, i32) {
    unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) }
}
/// The desktop's icon list covers the whole desktop window, every monitor, so marquee selection, drops and icon drags work
/// wherever the pointer is (and over the gaps between monitors of different sizes, where no icon can stand). Where icons may
/// stand is `desk::Grid`: a work area per monitor.
pub(crate) fn place_desk_list(list: HWND) {
    let (_, _, vw, vh) = virt();
    unsafe { MoveWindow(list, 0, 0, vw, vh, 1) };
}
/// Renders the wallpaper for the current monitor layout (at start and when monitors change), freeing the old one.
pub(crate) fn reload_wall() {
    let (vx, vy, cw, ch) = virt();
    let old = WALL.replace((wall::load(vx, vy, cw, ch), cw, ch)).0;
    if old != 0 { unsafe { windows_sys::Win32::Graphics::Gdi::DeleteObject(old as _) }; }
}
/// The wallpaper changed (Settings, the Photo viewer): rs-shell, which owns the conversion, makes the new BMP off the UI
/// thread (a second or two), then the window paints it (WM_WALL). Without rs-shell next to us nothing changes, as before.
pub(crate) fn wallpaper_changed(h: HWND) {
    let h = h as isize;
    std::thread::spawn(move || {
        use std::os::windows::process::CommandExt;
        let Some(exe) = env::current_exe().ok().map(|e| e.with_file_name("rs-shell.exe")).filter(|e| e.exists()) else { return };
        let made = std::process::Command::new(exe).arg("--wallpaper").creation_flags(0x0800_0000).status().is_ok_and(|s| s.success()); // CREATE_NO_WINDOW
        if made { unsafe { PostMessageW(h as HWND, WM_WALL, 0, 0) }; }
    });
}
/// Where icons may stand on one monitor (screen pixels): the monitor's rectangle and its icon area, the work area the bar left
/// less a small margin. A bar that reserves a strip gets a gap past it; the dock style reserves nothing but floats over the
/// bottom edge, so it gets a fixed gap while topbar runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Area { pub rect: (i32, i32, i32, i32), pub work: (i32, i32, i32, i32) }

/// The icon area of a monitor from its rectangle and its work area (`rcWork`, which the bar sets per monitor).
pub(crate) fn area_of(rect: (i32, i32, i32, i32), work: (i32, i32, i32, i32), dock_floats: bool) -> Area {
    let reserved = rect.3 - work.3;
    let bottom = if reserved > 0 { reserved + ui::sc(4) } else if dock_floats { ui::sc(52) } else { ui::sc(8) };
    Area { rect, work: (work.0, work.1 + ui::sc(8), work.2, rect.3 - bottom) }
}

/// Every monitor's icon area, the primary first (it is where icons start, and where "arrange automatically" fills), the others
/// left to right.
pub(crate) fn areas() -> Vec<Area> {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, lp: isize) -> i32 {
        let mut mi: MONITORINFO = unsafe { std::mem::zeroed() };
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(m, &mut mi) } != 0 {
            let (r, k) = (mi.rcMonitor, mi.rcWork);
            unsafe { (*(lp as *mut Vec<(bool, Area)>)).push((mi.dwFlags & 1 != 0 /* MONITORINFOF_PRIMARY */, Area { rect: (r.left, r.top, r.right, r.bottom), work: (k.left, k.top, k.right, k.bottom) })) };
        }
        1
    }
    let mut v: Vec<(bool, Area)> = Vec::new();
    unsafe { EnumDisplayMonitors(ptr::null_mut(), ptr::null(), Some(each), &mut v as *mut _ as isize) };
    let dock = unsafe { !FindWindowW(w("topbar").as_ptr(), ptr::null()).is_null() } && winrust_common::style::style() == winrust_common::style::Style::Dock;
    v.sort_by_key(|&(primary, a)| (!primary, a.rect.0, a.rect.1));
    let mut out: Vec<Area> = v.into_iter().map(|(_, a)| area_of(a.rect, a.work, dock)).collect();
    if out.is_empty() { // no answer: the primary screen alone
        let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        out.push(area_of((0, 0, sw, sh), (0, 0, sw, sh), dock));
    }
    out
}
/// "Hide desktop icons" is a marker file, so it survives restarts.
pub(crate) fn icons_marker() -> PathBuf { PathBuf::from(env::var_os("APPDATA").unwrap_or_default()).join(r"WinRust\desktop-icons-hidden.txt") }
/// The old "small icons" marker: still read while there is no view file.
pub(crate) fn small_marker() -> PathBuf { icons_marker().with_file_name("small-icons") }
fn view_file() -> PathBuf { icons_marker().with_file_name("desktop-view.txt") }

/// How the desktop's icons are ordered and sized (what the bar's desktop menu changes), kept in `desktop-view.txt`
/// (`sort=name|date|size`, `size=small|medium|large`, `auto=0|1`). Without the file the old marker decides the size (small,
/// else the old default 32 px = medium), the order is by name and nothing arranges itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct View { pub sort: Sort, pub size: IconSize, pub auto: bool }

impl View {
    pub fn parse(text: &str, small_marker: bool) -> View {
        let mut v = View { size: if small_marker { IconSize::Small } else { IconSize::Medium }, ..View::default() };
        for (k, val) in text.lines().filter_map(|l| l.split_once('=')) {
            match k.trim() {
                "sort" => if let Some(s) = Sort::parse(val) { v.sort = s },
                "size" => if let Some(s) = IconSize::parse(val) { v.size = s },
                "auto" => v.auto = val.trim() == "1",
                _ => {}
            }
        }
        v
    }
    pub fn to_text(self) -> String { format!("sort={}\nsize={}\nauto={}\n", self.sort.name(), self.size.name(), u8::from(self.auto)) }
    /// The old menu's "Small icons / Large icons" item: small, and back to the old default.
    pub fn toggled_small(self) -> View { View { size: if self.size == IconSize::Small { IconSize::Medium } else { IconSize::Small }, ..self } }
}

thread_local! { static VIEW: std::cell::Cell<Option<View>> = const { std::cell::Cell::new(None) }; }
/// The current view: read once, then kept (the desktop window's thread is the only one that changes it).
pub(crate) fn view() -> View {
    VIEW.with(|c| c.get().unwrap_or_else(|| {
        let v = View::parse(&fs::read_to_string(view_file()).unwrap_or_default(), small_marker().exists());
        c.set(Some(v));
        v
    }))
}
pub(crate) fn set_view(v: View) {
    VIEW.with(|c| c.set(Some(v)));
    let f = view_file();
    if let Some(d) = f.parent() { let _ = fs::create_dir_all(d); }
    let _ = fs::write(f, v.to_text());
    let _ = fs::remove_file(small_marker()); // the file says it now
}
/// The column `sort_rows` orders by: 0 name, 1 modified, 3 size.
pub(crate) const fn sort_col(s: Sort) -> usize { [0, 1, 3][s as usize] }
/// The icon grid of a size: cell width and height before scaling.
pub(crate) const fn spacing(s: IconSize) -> (i32, i32) { [(76, 64), (88, 96), (104, 112)][s as usize] }

/// The system's 48 px image list (`SHIL_EXTRALARGE`); its handle is the interface pointer, and the list lives as long as the process.
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn extra_large_icons() -> Option<usize> {
    use windows_sys::{core::GUID, Win32::UI::Shell::SHGetImageList};
    let iid = GUID::from_u128(0x46EB5926_582E_4017_9FDF_E8998DAA0950); // IID_IImageList
    let mut p: *mut std::ffi::c_void = ptr::null_mut();
    (SHGetImageList(2, &iid, &mut p) >= 0 && !p.is_null()).then_some(p as usize)
}

/// Desktop icon size: the system's 16, 32 or 48 px icons with a grid to match. Same icon indexes in all three.
#[allow(unsafe_op_in_unsafe_fn)]
pub(crate) unsafe fn apply_icon_size(list: HWND) {
    let size = view().size;
    let mut fi: SHFILEINFOW = std::mem::zeroed();
    let mut sys = |flag| SHGetFileInfoW(w("C:\\").as_ptr(), 0, &mut fi, size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX | flag);
    let il = match size {
        IconSize::Small => sys(SHGFI_SMALLICON),
        IconSize::Medium => sys(SHGFI_LARGEICON),
        IconSize::Large => extra_large_icons().unwrap_or_else(|| sys(SHGFI_LARGEICON)),
    };
    put(list, LVM_SETIMAGELIST, LVSIL_NORMAL as usize, il as isize);
    let (cx, cy) = spacing(size);
    put(list, LVM_SETICONSPACING, 0, ((ui::sc(cy) as isize) << 16) | ui::sc(cx) as isize);
}
pub(crate) fn icons_hidden() -> bool { icons_marker().exists() }
pub(crate) fn set_icons_hidden(hide: bool) {
    let m = icons_marker();
    if hide { if let Some(d) = m.parent() { let _ = fs::create_dir_all(d); } let _ = fs::write(&m, "1"); } else { let _ = fs::remove_file(&m); }
    let list = ST.with_borrow(|st| st.list);
    unsafe { ShowWindow(list, if hide { SW_HIDE } else { SW_SHOW }) };
}

/// The drive letter of a path that starts with one (`C:\...`): the drive WinRust Disk is asked to scan.
pub(crate) fn drive_of(p: &Path) -> Option<char> {
    let s = p.to_string_lossy();
    let mut c = s.chars();
    match (c.next(), c.next()) { (Some(d), Some(':')) if d.is_ascii_alphabetic() => Some(d.to_ascii_uppercase()), _ => None }
}

/// Right click on the desktop: asks the bar to draw the design's menu at the pointer, with the state it has to show.
/// `false` (no bar, or one that cannot) = the caller shows its own GDI menu.
pub(crate) fn ask_bar() -> bool {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut pt) }; // screen pixels: the bar opens the menu on the monitor this point is on
    desk::clicked((pt.x, pt.y));
    let v = view();
    let drive = tool("wiztree").and_then(|_| drive_of(&cur_dir()));
    deskmenu::ask(&Request { at: (pt.x, pt.y), sort: v.sort, size: v.size, auto: v.auto, shown: !icons_hidden(), drive })
}

/// What the bar's menu chose. The segments and the toggles carry the value they set.
pub(crate) fn command(h: HWND, cmd: Cmd) {
    let (list, dir, mut v) = (ST.with_borrow(|st| st.list), cur_dir(), view());
    let drop_cells = || { let _ = fs::remove_file(desk::pos_file()); }; // the next load lays every icon out again
    match cmd {
        // the new icons appear where the menu was opened, on whichever monitor that was
        Cmd::NewFolder => { desk::want_near_click(); new_entry(&dir, t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹"), true) }
        Cmd::NewText => { desk::want_near_click(); new_entry(&dir, t!("Новый текстовый документ.txt", "New text document.txt", ja: "新しいテキスト ドキュメント.txt", pt: "Novo documento de texto.txt", zh: "新建文本文档.txt"), false) }
        Cmd::Paste => { desk::want_near_click(); paste(h, &dir) }
        Cmd::Sort(s) => { v.sort = s; set_view(v); SORT_COL.store(sort_col(s), Relaxed); SORT_ASC.store(true, Relaxed); drop_cells(); }
        Cmd::Size(s) => { v.size = s; set_view(v); unsafe { apply_icon_size(list) }; drop_cells(); } // the grid changes with the size
        Cmd::Auto(on) => {
            v.auto = on;
            set_view(v);
            if !on { desk::save(list); } // switched off: the icons stay where they are
        }
        Cmd::Show(on) => return set_icons_hidden(!on),
        Cmd::Refresh => {}
        Cmd::Disk => { // wiztree takes the drive letter and asks for admin rights itself
            if let (Some(exe), Some(d)) = (tool("wiztree"), drive_of(&dir)) { let _ = std::process::Command::new(exe).arg(format!("{d}:")).spawn(); }
            return;
        }
        Cmd::Properties => return properties(&dir),
    }
    reload(h);
}

#[cfg(test)]
mod view_tests {
    use super::*;

    #[test]
    fn without_a_view_file_the_old_marker_decides_the_size() {
        assert_eq!(View::parse("", false), View { sort: Sort::Name, size: IconSize::Medium, auto: false }); // the old default: 32 px
        assert_eq!(View::parse("", true).size, IconSize::Small); // the old small-icons marker
        assert_eq!(View::parse("garbage\n=\nsort\n", false), View::default());
    }

    #[test]
    fn the_view_file_wins_and_round_trips() {
        let v = View { sort: Sort::Size, size: IconSize::Large, auto: true };
        assert_eq!(View::parse(&v.to_text(), true), v); // even over the marker
        assert_eq!(View::parse("size=small\nauto=1\nsort=date", false), View { sort: Sort::Date, size: IconSize::Small, auto: true });
        assert_eq!(View::parse("size=huge\nsort=type\nauto=yes", true), View { size: IconSize::Small, ..View::default() }); // unknown values are left alone
    }

    #[test]
    fn sizes_map_to_the_systems_icons_and_the_grid_widens_with_them() {
        assert_eq!(IconSize::ALL.map(|s| s.px()), [16, 32, 48]);
        let g = IconSize::ALL.map(spacing);
        assert_eq!(g[0], (76, 64), "as the old small icons");
        assert_eq!(g[1], (88, 96), "as the old large icons");
        assert!(g[1].0 < g[2].0 && g[1].1 < g[2].1);
        assert_eq!(Sort::ALL.map(sort_col), [0, 1, 3]);
    }

    #[test]
    fn the_old_menus_size_item_goes_between_small_and_the_default() {
        let v = View::default();
        assert_eq!(v.toggled_small().size, IconSize::Small);
        assert_eq!(v.toggled_small().toggled_small().size, IconSize::Medium);
        assert_eq!(View { size: IconSize::Large, ..v }.toggled_small().size, IconSize::Small);
    }

    #[test]
    fn disk_usage_scans_the_drive_the_desktop_folder_is_on() {
        assert_eq!(drive_of(Path::new(r"C:\Users\a\Desktop")), Some('C'));
        assert_eq!(drive_of(Path::new(r"d:\x")), Some('D'));
        assert_eq!(drive_of(Path::new(r"\\server\share\Desktop")), None);
        assert_eq!(drive_of(Path::new("")), None);
    }
}

/// Wallpaper for the desktop window.
pub(crate) mod wall {
    use windows_sys::Win32::{Foundation::RECT, Graphics::Gdi::*, UI::WindowsAndMessaging::{IMAGE_BITMAP, LR_LOADFROMFILE, LoadImageW}};

    /// The part of a bw x bh picture that "fill" shows on a cw x ch screen: centered, cropped to the screen's aspect.
    /// (x, y, w, h) in the picture.
    pub fn fill_src(bw: i32, bh: i32, cw: i32, ch: i32) -> (i32, i32, i32, i32) {
        let (sw, sh) = if bw as i64 * ch as i64 > bh as i64 * cw as i64 { (bh * cw / ch.max(1), bh) } else { (bw, bw * ch / cw.max(1)) };
        ((bw - sw) / 2, (bh - sh) / 2, sw, sh)
    }

    /// Every monitor's rectangle, relative to the virtual screen's top-left (vx, vy).
    fn monitors(vx: i32, vy: i32) -> Vec<RECT> {
        unsafe extern "system" fn each(_: HMONITOR, _: HDC, r: *mut RECT, lp: isize) -> i32 {
            unsafe { (*(lp as *mut Vec<RECT>)).push(*r) };
            1
        }
        let mut v: Vec<RECT> = Vec::new();
        unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut v as *mut _ as isize) };
        v.iter().map(|r| RECT { left: r.left - vx, top: r.top - vy, right: r.right - vx, bottom: r.bottom - vy }).collect()
    }

    /// The BMP for the whole virtual screen (vx, vy, cw x ch): scaled with "fill" onto each monitor on its own, so a second
    /// monitor of another size or aspect gets a whole picture too. 0 = no wallpaper file.
    pub fn load(vx: i32, vy: i32, cw: i32, ch: i32) -> isize {
        let Ok(base) = std::env::var("LOCALAPPDATA") else { return 0 };
        let path: Vec<u16> = format!(r"{base}\rs-shell\wallpaper.bmp").encode_utf16().chain([0]).collect();
        unsafe {
            let src = LoadImageW(std::ptr::null_mut(), path.as_ptr(), IMAGE_BITMAP, 0, 0, LR_LOADFROMFILE);
            if src.is_null() { return 0; }
            let mut bm: BITMAP = std::mem::zeroed();
            GetObjectW(src, size_of::<BITMAP>() as i32, &mut bm as *mut _ as _);
            let screen = GetDC(std::ptr::null_mut());
            let dst = CreateCompatibleBitmap(screen, cw, ch);
            let (sdc, ddc) = (CreateCompatibleDC(screen), CreateCompatibleDC(screen));
            let (o1, o2) = (SelectObject(sdc, src), SelectObject(ddc, dst));
            PatBlt(ddc, 0, 0, cw, ch, BLACKNESS); // the gaps between monitors of different heights
            SetStretchBltMode(ddc, HALFTONE);
            let mut mons = monitors(vx, vy);
            if mons.is_empty() { mons.push(RECT { left: 0, top: 0, right: cw, bottom: ch }); }
            for m in mons {
                let (mw, mh) = (m.right - m.left, m.bottom - m.top);
                let (sx, sy, sw, sh) = fill_src(bm.bmWidth, bm.bmHeight, mw, mh);
                StretchBlt(ddc, m.left, m.top, mw, mh, sdc, sx, sy, sw, sh, SRCCOPY);
            }
            SelectObject(sdc, o1); SelectObject(ddc, o2);
            DeleteDC(sdc); DeleteDC(ddc); ReleaseDC(std::ptr::null_mut(), screen); DeleteObject(src);
            dst as isize
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn fill_crops_to_the_screen_aspect() {
            assert_eq!(super::fill_src(1920, 1080, 1920, 1080), (0, 0, 1920, 1080));
            assert_eq!(super::fill_src(1920, 1080, 1080, 1080), (420, 0, 1080, 1080)); // square screen: the sides go
            assert_eq!(super::fill_src(1920, 1080, 2560, 1080), (0, 135, 1920, 810)); // ultrawide: top and bottom go
            assert_eq!(super::fill_src(100, 100, 0, 0).2, 100); // no division by zero
        }
    }

    pub fn paint(dc: HDC, bmp: isize, cw: i32, ch: i32) {
        unsafe {
            let m = CreateCompatibleDC(dc);
            let old = SelectObject(m, bmp as _);
            BitBlt(dc, 0, 0, cw, ch, m, 0, 0, SRCCOPY);
            SelectObject(m, old);
            DeleteDC(m);
        }
    }
}

/// The desktop's own logic: icons stay where the user puts them (positions saved per name), dropping one onto a folder
/// icon moves it into the folder, and a host window for Wallpaper Engine (it draws behind the icons).
pub(crate) mod desk {
    #![allow(unsafe_op_in_unsafe_fn)]
    use super::*;
    use std::{cell::{Cell, RefCell}, collections::{HashMap, HashSet}, time::Instant};
    use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HDC, ScreenToClient};

    pub const SPAWN_WORKERW: u32 = 0x052C; // what wallpaper apps send to "Progman" to get a window behind the icons
    const KEY: u32 = 0x0001_0001; // rgb(1, 0, 1): the list's background colour, made see-through while a live wallpaper runs

    thread_local! {
        static DRAG_FROM: Cell<Option<(i32, i32)>> = const { Cell::new(None) }; // our own icon drag: where it started (list coords)
        static DROP_AT: Cell<Option<(i32, i32)>> = const { Cell::new(None) }; // ...and where it was dropped (screen coords)
        static WORKERW: Cell<isize> = const { Cell::new(0) };
        static LIVE: Cell<bool> = const { Cell::new(false) }; // a live wallpaper is running in WORKERW
        /// The layout the icons were last put on: the virtual screen's top left (list coordinates start there) and every
        /// monitor's icon area. What the saved positions are relative to, and what a change of monitors or bar carries icons from.
        static LAST: RefCell<((i32, i32), Vec<Area>)> = const { RefCell::new(((0, 0), Vec::new())) };
        static CLICK: Cell<(i32, i32)> = const { Cell::new((0, 0)) }; // where the desktop menu was last opened (screen coords)
        static WANT: Cell<Option<((i32, i32), Instant)>> = const { Cell::new(None) }; // where the next new icons should appear
    }

    /// The live-wallpaper window follows the desktop's size (monitors added, removed or resized).
    pub unsafe fn size_workerw(cw: i32, ch: i32) {
        let ww = WORKERW.get() as HWND;
        if !ww.is_null() && IsWindow(ww) != 0 {
            MoveWindow(ww, 0, 0, cw, ch, 1);
        }
    }

    pub fn pos_file() -> PathBuf {
        PathBuf::from(env::var_os("APPDATA").unwrap_or_default()).join(r"rust-explorer\desktop-icons.txt")
    }

    /// Where the desktop menu was opened: what "New folder" and the others from it are placed near.
    pub fn clicked(at: (i32, i32)) { CLICK.set(at); }
    pub fn want_near_click() { want_near(CLICK.get()); }
    /// The icons that appear next (dropped files, new items) go to the free cells nearest to `at` (screen coords), whichever
    /// monitor it is on. Good for a few seconds: a drop whose files never come must not place something unrelated later.
    pub fn want_near(at: (i32, i32)) { WANT.set(Some((at, Instant::now()))); }
    fn wanted() -> Option<(i32, i32)> { WANT.get().filter(|(_, t)| t.elapsed().as_secs() < 15).map(|(p, _)| p) }

    /// Saved icon positions: name -> (x, y) in pixels from the primary monitor's icon area corner (so the file written before
    /// there was a second monitor still means the same cells; a monitor left of or above the primary has negative ones).
    fn saved() -> HashMap<String, (i32, i32)> { parse_saved(&fs::read_to_string(pos_file()).unwrap_or_default()) }

    fn parse_saved(text: &str) -> HashMap<String, (i32, i32)> {
        text.lines().filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.to_string()))
        }).map(|(x, y, n): (i32, i32, String)| (n, (x, y))).collect()
    }

    fn saved_text<'a>(items: impl Iterator<Item = (&'a str, (i32, i32))>) -> String {
        items.map(|(n, (x, y))| format!("{x}\t{y}\t{n}\n")).collect()
    }

    fn names() -> Vec<String> { ST.with_borrow(|st| st.rows.iter().map(|r| r.name.clone()).collect()) }

    /// The primary monitor's icon area corner of the last layout: the zero of the saved positions.
    fn origin() -> (i32, i32) { LAST.with_borrow(|l| l.1.first().map_or((0, 0), |a| (a.work.0, a.work.1))) }

    /// Icon i's position in screen coordinates (the list's own start at the virtual screen's top left).
    fn pos(list: HWND, i: usize) -> (i32, i32) {
        let (x, y) = raw_pos(list, i);
        let (vx, vy, ..) = virt();
        (x + vx, y + vy)
    }

    fn raw_pos(list: HWND, i: usize) -> (i32, i32) {
        let mut p = POINT { x: 0, y: 0 };
        put(list, LVM_GETITEMPOSITION, i, &mut p as *mut _ as isize);
        (p.x, p.y)
    }

    fn set_pos(list: HWND, i: usize, (x, y): (i32, i32)) {
        let (vx, vy, ..) = virt();
        let p = POINT { x: x - vx, y: y - vy };
        put(list, LVM_SETITEMPOSITION32, i, &p as *const _ as isize);
    }

    /// Writes every icon's position (after a move or a fresh layout). "Arrange automatically" keeps none: the layout is the
    /// sort order, so the positions from before stay put for when it is switched off (which writes them anew).
    pub fn save(list: HWND) {
        if view().auto { return; }
        let (ox, oy) = origin();
        let names = names();
        let lines = saved_text(names.iter().enumerate().map(|(i, n)| { let (x, y) = pos(list, i); (n.as_str(), (x - ox, y - oy)) }));
        let f = pos_file();
        let _ = fs::create_dir_all(f.parent().unwrap_or(&f));
        let _ = fs::write(f, lines);
    }

    /// A grid cell (slot): monitor (index into the grid's monitors), column, row.
    type Slot = (usize, i32, i32);

    /// One monitor's part of the grid: its icon area and how many cells fit in it.
    struct Mon { area: Area, cols: i32, rows: i32 }

    /// The icon grid: one cell size for all (the list has one DPI, the primary's), and per monitor a block of cells from the
    /// top left of its icon area, so an icon stands on a monitor (never in the gap between two) and the primary comes first.
    struct Grid { cx: i32, cy: i32, mons: Vec<Mon> }

    /// Squared distance from a point to a rectangle (0 inside it).
    fn gap2(r: (i32, i32, i32, i32), (x, y): (i32, i32)) -> i64 {
        let (dx, dy) = ((r.0 - x).max(0).max(x - r.2) as i64, (r.1 - y).max(0).max(y - r.3) as i64);
        dx * dx + dy * dy
    }
    fn d2(a: (i32, i32), b: (i32, i32)) -> i64 { let (dx, dy) = ((a.0 - b.0) as i64, (a.1 - b.1) as i64); dx * dx + dy * dy }

    impl Grid {
        /// The grid for the monitors as they are now; remembers them as the layout in force (see `LAST`).
        fn of(list: HWND) -> Grid {
            let s = put(list, LVM_GETITEMSPACING, 0, 0) as u32;
            let (cx, cy) = (((s & 0xFFFF) as i32).max(1), ((s >> 16) as i32).max(1));
            let (vx, vy, ..) = virt();
            let areas = areas();
            LAST.replace(((vx, vy), areas.clone()));
            Grid::new(cx, cy, &areas)
        }
        fn new(cx: i32, cy: i32, areas: &[Area]) -> Grid {
            let mons = areas.iter().map(|&area| {
                let w = area.work;
                Mon { area, cols: ((w.2 - w.0) / cx).max(1), rows: ((w.3 - w.1) / cy).max(1) }
            }).collect();
            Grid { cx, cy, mons }
        }
        fn all(&self) -> impl Iterator<Item = Slot> + '_ {
            self.mons.iter().enumerate().flat_map(|(m, mo)| (0..mo.cols).flat_map(move |c| (0..mo.rows).map(move |r| (m, c, r))))
        }
        fn at(&self, (m, c, r): Slot) -> (i32, i32) { let w = self.mons[m].area.work; (w.0 + c * self.cx, w.1 + r * self.cy) }
        /// The cell nearest to a position, free or not: on the monitor it is on, else on the nearest one (a gap between
        /// monitors of different sizes, a monitor that is gone), at its nearest edge cell.
        fn snap(&self, (x, y): (i32, i32)) -> Slot {
            let cell = |m: usize, mo: &Mon| {
                let w = mo.area.work;
                (m, (x - w.0 + self.cx / 2).div_euclid(self.cx).clamp(0, mo.cols - 1), (y - w.1 + self.cy / 2).div_euclid(self.cy).clamp(0, mo.rows - 1))
            };
            self.mons.iter().enumerate().map(|(m, mo)| cell(m, mo)).min_by_key(|&c| d2(self.at(c), (x, y))).unwrap_or((0, 0, 0))
        }
        /// The cell a position is on (within half a cell of it); None if it is off every monitor's cells.
        fn on(&self, p: (i32, i32)) -> Option<Slot> {
            let c = self.snap(p);
            let a = self.at(c);
            ((a.0 - p.0).abs() * 2 <= self.cx && (a.1 - p.1).abs() * 2 <= self.cy).then_some(c)
        }
        /// Free cell nearest to `want`; with no wish the first free one: the primary monitor's, going down the columns, like
        /// Windows (the next monitor's only when the primary is full).
        fn free(&self, taken: &HashSet<Slot>, want: Option<(i32, i32)>) -> Slot {
            let all = self.all().filter(|c| !taken.contains(c));
            match want {
                Some(p) => all.min_by_key(|&c| d2(self.at(c), p)),
                None => all.min(),
            }
            .unwrap_or((0, 0, 0))
        }
        /// Where an icon at `p` of the layout `old` goes when the layout is this one: with its monitor's icon area, so the
        /// bar coming or growing keeps every icon on its cell. A monitor that is gone or changed size (no monitor of this
        /// grid has its rectangle) leaves the position as it is, for `snap` to put on the nearest cell.
        fn carry(&self, old: &[Area], p: (i32, i32)) -> (i32, i32) {
            let Some(o) = old.iter().min_by_key(|a| gap2(a.work, p)) else { return p };
            match self.mons.iter().find(|m| m.area.rect == o.rect) {
                Some(m) => (p.0 + m.area.work.0 - o.work.0, p.1 + m.area.work.1 - o.work.1),
                None => p,
            }
        }
    }

    /// Fills the (non-virtual) desktop list with `n` rows and puts every icon on its saved cell, new ones on free cells.
    pub fn fill(list: HWND, n: usize) {
        for i in 0..n {
            let mut it: LVITEMW = unsafe { std::mem::zeroed() };
            it.mask = LVIF_TEXT | LVIF_IMAGE;
            it.iItem = i as i32;
            it.pszText = usize::MAX as *mut u16; // LPSTR_TEXTCALLBACKW: text and icon come from LVN_GETDISPINFO
            it.iImage = -1; // I_IMAGECALLBACK
            put(list, LVM_INSERTITEMW, 0, &it as *const _ as isize);
        }
        let g = Grid::of(list);
        let (ox, oy) = origin();
        let auto = view().auto;
        let old: HashMap<String, (i32, i32)> = if auto { HashMap::new() } else { saved().into_iter().map(|(n, (x, y))| (n, (x + ox, y + oy))).collect() };
        let names = names();
        let near = if auto { None } else { wanted() };
        for (i, p) in plan(&g, &names, &old, near).into_iter().enumerate() { set_pos(list, i, p); }
        if near.is_some() && names.iter().any(|n| !old.contains_key(n)) { WANT.set(None); } // the new icons came: used up
        save(list);
    }

    /// The monitors or the bar changed (a monitor came, went or resized; the bar came, went or changed its strip): every icon
    /// goes with its monitor's icon area, or when its monitor is gone to the nearest free cell of another.
    pub fn relayout(list: HWND) {
        let names = names();
        if put(list, LVM_GETITEMCOUNT, 0, 0) as usize != names.len() { return; } // not loaded yet: the first fill lays out
        let (old_v, old) = LAST.take();
        let g = Grid::of(list);
        let cur: HashMap<String, (i32, i32)> = if view().auto { HashMap::new() } else {
            names.iter().enumerate().map(|(i, n)| { let (x, y) = raw_pos(list, i); (n.clone(), g.carry(&old, (x + old_v.0, y + old_v.1))) }).collect()
        };
        for (i, p) in plan(&g, &names, &cur, None).into_iter().enumerate() { set_pos(list, i, p); }
        save(list);
    }

    /// Where each icon goes (screen pixels, in the order of `names`): on its saved cell when that is on a monitor's grid and
    /// nobody else has it; the icons whose position is off every grid (a gap between monitors, a monitor that went away) or
    /// whose cell is taken on the nearest free cells to where they were; new ones (`near`: where they should appear, such as
    /// the point of a drop) on the free cells nearest to it, else the first free cells going down the primary monitor's columns.
    /// With nothing saved (arranging automatically) that is the sort order, column after column.
    fn plan(g: &Grid, names: &[String], old: &HashMap<String, (i32, i32)>, near: Option<(i32, i32)>) -> Vec<(i32, i32)> {
        let mut taken = HashSet::new();
        let mut out = vec![(0, 0); names.len()];
        let (mut moved, mut fresh) = (vec![], vec![]);
        for (i, name) in names.iter().enumerate() {
            match old.get(name) {
                Some(&p) => match g.on(p).filter(|c| !taken.contains(c)) {
                    Some(c) => { taken.insert(c); out[i] = g.at(c); }
                    None => moved.push((i, p)),
                },
                None => fresh.push(i),
            }
        }
        for (i, p) in moved {
            let c = g.free(&taken, Some(p));
            taken.insert(c);
            out[i] = g.at(c);
        }
        for i in fresh {
            let c = g.free(&taken, near);
            taken.insert(c);
            out[i] = g.at(c);
        }
        out
    }

    // ---------- moving icons ----------

    /// Our own drag of desktop icons starts (`from` in list coordinates).
    pub fn drag_start(from: (i32, i32)) { DRAG_FROM.set(Some(from)); DROP_AT.set(None); }
    pub fn dragging() -> bool { DRAG_FROM.get().is_some() }
    /// Called by the drop target: the drag ended on the desktop itself at screen point `at`.
    pub fn dropped_here(at: (i32, i32)) { DROP_AT.set(Some(at)); }

    /// After the drag: on a folder icon (not one of the dragged) the selection moves into that folder; anywhere else the
    /// icons move by the drag distance (on any monitor), each to the nearest free cell. true = handled here.
    pub fn drag_end(h: HWND, list: HWND) -> bool {
        let Some(from) = DRAG_FROM.take() else { return false };
        // normally our drop target recorded where it ended; over a live wallpaper OLE never reaches it (the wallpaper window of
        // another process is under the pointer), so then: the pointer is over our desktop window = the drop was on the desktop
        let at = DROP_AT.take().or_else(|| unsafe {
            let mut c = POINT { x: 0, y: 0 };
            GetCursorPos(&mut c);
            (GetAncestor(WindowFromPoint(c), GA_ROOT) == h).then_some((c.x, c.y))
        });
        let Some(at) = at else { return false };
        let mut pt = POINT { x: at.0, y: at.1 };
        unsafe { ScreenToClient(list, &mut pt) };
        let (_, dir, paths) = selected();
        let mut sel = vec![];
        let mut i = -1isize;
        loop {
            i = put(list, LVM_GETNEXTITEM, i as usize, LVNI_SELECTED as isize);
            if i < 0 { break; }
            sel.push(i as usize);
        }
        // onto a folder icon
        let mut ht: LVHITTESTINFO = unsafe { std::mem::zeroed() };
        ht.pt = pt;
        let tgt = put(list, LVM_HITTEST, 0, &mut ht as *mut _ as isize);
        if tgt >= 0 && !sel.contains(&(tgt as usize)) {
            let target = ST.with_borrow(|st| st.rows.get(tgt as usize).filter(|r| r.dir).map(|r| st.dir.join(&r.name)));
            if let Some(t) = target {
                let _ = sh::fileop(h as _, sh::Op::Move, &paths, &t);
                let _ = dir;
                unsafe { PostMessageW(h, WM_APP, 0, 0) }; // reload: the moved icons are gone from here
                return true;
            }
        }
        let g = Grid::of(list);
        if view().auto { // nothing stays where it was dropped: everything goes back to the order of the sort
            for (i, p) in plan(&g, &names(), &HashMap::new(), None).into_iter().enumerate() { set_pos(list, i, p); }
            return true;
        }
        let n = put(list, LVM_GETITEMCOUNT, 0, 0).max(0) as usize;
        let mut taken: HashSet<Slot> = (0..n).filter(|i| !sel.contains(i)).map(|i| g.snap(pos(list, i))).collect();
        let (dx, dy) = (pt.x - from.0, pt.y - from.1);
        for &i in &sel {
            let (x, y) = pos(list, i);
            let c = g.free(&taken, Some((x + dx, y + dy))); // dropped between monitors or off them: the nearest cell there is
            taken.insert(c);
            set_pos(list, i, g.at(c));
        }
        save(list);
        true
    }

    // ---------- Wallpaper Engine (and other live wallpapers) ----------
    //
    // They look for the "Progman" window (that is our desktop's class), send it 0x052C and put their window into the
    // "WorkerW" that appears: Windows 11 24H2 has it as a child of Progman, under the icons. Ours is the same, and while it
    // holds a wallpaper the icon list turns see-through (colour key) so the wallpaper shows between the icons.

    pub unsafe fn spawn_workerw(h: HWND) {
        if WORKERW.get() != 0 && IsWindow(WORKERW.get() as HWND) != 0 { return; }
        let mut r: RECT = std::mem::zeroed();
        GetClientRect(h, &mut r);
        let ww = CreateWindowExW(0, w("WorkerW").as_ptr(), ptr::null(), WS_CHILD | WS_VISIBLE, 0, 0, r.right, r.bottom, h, 0 as _, GetModuleHandleW(ptr::null()), ptr::null());
        SetWindowPos(ww, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); // under the icon list
        WORKERW.set(ww as isize);
    }

    /// Every couple of seconds: a wallpaper window inside WorkerW = live mode on, none = back to the picture.
    pub unsafe fn check_live(list: HWND) {
        let ww = WORKERW.get() as HWND;
        let live = !ww.is_null() && IsWindow(ww) != 0 && !GetWindow(ww, GW_CHILD).is_null();
        if live == LIVE.replace(live) { return; }
        // the list stays "transparent" (its background is what the desktop paints: the key colour while live, see erase()),
        // so it keeps drawing the label shadows; the colour key then cuts the background out
        let ex = GetWindowLongPtrW(list, GWL_EXSTYLE);
        if live {
            SetWindowLongPtrW(list, GWL_EXSTYLE, ex | WS_EX_LAYERED as isize);
            SetLayeredWindowAttributes(list, KEY, 0, LWA_COLORKEY);
        } else {
            SetWindowLongPtrW(list, GWL_EXSTYLE, ex & !(WS_EX_LAYERED as isize));
        }
        InvalidateRect(list, ptr::null(), 1);
    }

    /// The desktop's background: the key colour while a live wallpaper shows through, else the picture. false = none to paint.
    pub unsafe fn erase(dc: HDC, h: HWND) -> bool {
        let (bmp, cw, ch) = WALL.get();
        if LIVE.get() {
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(h, &mut r);
            let b = CreateSolidBrush(KEY);
            FillRect(dc, &r, b);
            DeleteObject(b);
        } else if bmp != 0 {
            wall::paint(dc, bmp, cw, ch);
        } else {
            return false;
        }
        true
    }

    /// WorkerW without a wallpaper in it shows the picture wallpaper, like the desktop behind it.
    pub unsafe extern "system" fn workerw_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if m == WM_ERASEBKGND {
            let (bmp, cw, ch) = WALL.get();
            if bmp != 0 { wall::paint(wp as _, bmp, cw, ch); } else { let mut r: RECT = std::mem::zeroed(); GetClientRect(h, &mut r); FillRect(wp as _, &r, windows_sys::Win32::Graphics::Gdi::GetSysColorBrush(1) as _); }
            return 1;
        }
        DefWindowProcW(h, m, wp, lp)
    }

    #[cfg(test)]
    mod tests {
        use super::{Area, Grid, area_of, parse_saved, plan, saved_text};
        use std::collections::{HashMap, HashSet};

        /// A monitor of size w x h at (x, y) whose icon area is the whole of it (no bar).
        fn mon(x: i32, y: i32, w: i32, h: i32) -> Area { Area { rect: (x, y, x + w, y + h), work: (x, y, x + w, y + h) } }
        /// Cells of 100 x 90, so a 300 x 180 monitor holds 3 x 2.
        fn grid(areas: &[Area]) -> Grid { Grid::new(100, 90, areas) }

        #[test]
        fn grid_cells() {
            let g = grid(&[mon(0, 0, 500, 360)]); // 5 x 4
            assert_eq!(g.snap((0, 0)), (0, 0, 0));
            assert_eq!(g.on((149, 44)), Some((0, 1, 0))); // rounds to the nearest cell
            assert_eq!(g.on((900, 0)), None); // off the grid
            assert_eq!(g.snap((900, 0)), (0, 4, 0)); // ... and its nearest edge cell
            assert_eq!(g.at((0, 4, 3)), (400, 270));
            let mut taken: HashSet<_> = [(0, 0, 0), (0, 0, 1)].into();
            assert_eq!(g.free(&taken, None), (0, 0, 2)); // down the first column first
            taken.insert((0, 2, 2));
            assert_eq!(g.free(&taken, Some((200, 180))), (0, 2, 1)); // nearest free to the wish: cells are taller than wide, so above it (ties: lowest)
        }

        fn names(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

        #[test]
        fn saved_cells_are_kept_and_the_rest_fills_the_free_ones() {
            let g = grid(&[mon(0, 0, 300, 180)]); // 3 x 2
            let old: HashMap<_, _> = [("b".to_string(), (200, 90)), ("gone".to_string(), (0, 0)), ("off".to_string(), (900, 900))].into();
            // b keeps (2, 1); "off" (its position is off the grid) takes the free cell nearest to where it was, a and c (new) the
            // first free ones down the columns
            let p = plan(&g, &names(&["a", "b", "c", "off"]), &old, None);
            assert_eq!(p, vec![(0, 0), (200, 90), (0, 90), (100, 90)]);
        }

        #[test]
        fn arranging_automatically_ignores_the_saved_cells_and_follows_the_order_down_the_columns() {
            let g = grid(&[mon(0, 0, 300, 180)]);
            let p = plan(&g, &names(&["a", "b", "c", "d"]), &HashMap::new(), None);
            assert_eq!(p, vec![(0, 0), (0, 90), (100, 0), (100, 90)]);
            // the same names with a different sort order: a different layout, whatever was saved
            let p = plan(&g, &names(&["d", "c", "b", "a"]), &[("a".to_string(), (200, 90))].into(), None);
            assert_eq!(p[3], (200, 90)); // not auto: a keeps its cell
            assert_eq!(plan(&g, &names(&["d", "c", "b", "a"]), &HashMap::new(), None)[3], (100, 90)); // auto: it is the fourth
        }

        #[test]
        fn two_monitors_side_by_side_of_different_heights_leave_a_gap_no_icon_stands_in() {
            // primary 500 x 360 at the origin, the second 300 x 180 right of it, its top aligned: below it is a gap
            let g = grid(&[mon(0, 0, 500, 360), mon(500, 0, 300, 180)]);
            assert_eq!(g.on((500, 0)), Some((1, 0, 0))); // the second monitor's first cell
            assert_eq!(g.on((700, 90)), Some((1, 2, 1)));
            assert_eq!(g.on((600, 270)), None); // in the gap under the second monitor
            assert_eq!(g.snap((600, 270)), (1, 1, 1)); // nearest is the second monitor's bottom row (it is closer than the primary)
            assert_eq!(g.snap((450, 300)), (0, 4, 3)); // ... but here the primary's
            assert_eq!(g.at((1, 2, 1)), (700, 90));
            // new icons start on the primary, and go on to the second monitor only when it is full
            let many: Vec<String> = (0..21).map(|i| format!("n{i}")).collect();
            let p = plan(&g, &many, &HashMap::new(), None);
            assert_eq!(p[19], (400, 270)); // the primary's last cell (5 x 4 = 20 cells)
            assert_eq!(p[20], (500, 0)); // the 21st: the second monitor
        }

        #[test]
        fn a_monitor_above_the_primary_and_one_left_of_it_have_negative_coordinates() {
            let g = grid(&[mon(0, 0, 300, 180), mon(0, -180, 300, 180), mon(-300, 0, 300, 180)]);
            assert_eq!(g.on((0, -90)), Some((1, 0, 1)));
            assert_eq!(g.on((-100, 0)), Some((2, 2, 0)));
            // a point between the two outer monitors' corners lands on a cell of a real monitor
            for p in [(-50, -50), (310, 200), (-400, 400), (150, -500)] {
                let c = g.snap(p);
                let a = g.at(c);
                assert!(g.all().any(|x| x == c), "{p:?} -> {c:?} is no cell");
                assert!(g.mons.iter().any(|m| a.0 >= m.area.work.0 && a.0 < m.area.work.2 && a.1 >= m.area.work.1 && a.1 < m.area.work.3));
            }
        }

        #[test]
        fn icons_of_a_removed_monitor_snap_to_the_nearest_free_cells_of_the_rest() {
            let both = grid(&[mon(0, 0, 300, 180), mon(300, 0, 300, 180)]);
            let names = names(&["a", "b", "c", "d"]);
            // a and b on the second monitor (x >= 300), c and d on the primary
            let old: HashMap<_, _> = [("a", (300, 0)), ("b", (400, 90)), ("c", (0, 0)), ("d", (100, 0))].map(|(n, p)| (n.to_string(), p)).into();
            assert_eq!(plan(&both, &names, &old, None), vec![(300, 0), (400, 90), (0, 0), (100, 0)]); // unchanged while both are there
            let one = grid(&[mon(0, 0, 300, 180)]); // the second one is unplugged
            let p = plan(&one, &names, &old, None);
            assert_eq!(p[2], (0, 0));
            assert_eq!(p[3], (100, 0)); // the ones that stayed keep their cells
            assert_eq!(p[0], (200, 0)); // a: the nearest free cell to (300, 0) is the primary's right column, top
            assert_eq!(p[1], (200, 90)); // b: next to it
            assert_eq!(p.iter().collect::<HashSet<_>>().len(), 4, "no two icons on one cell");
        }

        #[test]
        fn the_bar_changing_moves_the_icons_with_their_monitor_and_a_gone_monitor_does_not() {
            let before = [mon(0, 0, 300, 180), mon(300, 0, 300, 180)];
            // a bar appears on the primary: its icon area starts 40 px lower; the second monitor is unchanged
            let mut after = before;
            after[0].work.1 += 40;
            let g = grid(&after);
            assert_eq!(g.carry(&before, (100, 90)), (100, 130)); // on the primary: down with the area, so the same cell
            assert_eq!(g.carry(&before, (400, 0)), (400, 0)); // on the second monitor: where it was
            // a monitor that changed size is no longer there: the position is left for `snap`
            let g = grid(&[mon(0, 0, 300, 180), mon(300, 0, 200, 180)]);
            assert_eq!(g.carry(&before, (400, 90)), (400, 90));
            assert_eq!(g.carry(&[], (5, 6)), (5, 6));
        }

        #[test]
        fn new_icons_appear_near_the_drop_point_on_any_monitor() {
            let g = grid(&[mon(0, 0, 300, 180), mon(300, 0, 300, 180)]);
            let old: HashMap<_, _> = [("a".to_string(), (400, 90))].into();
            // two dropped files at the second monitor's (410, 10): a is on a cell near it, they take the free ones around it
            let p = plan(&g, &names(&["a", "x", "y"]), &old, Some((410, 10)));
            assert_eq!(p[0], (400, 90));
            assert_eq!(p[1], (400, 0));
            assert_eq!(p[2], (500, 0)); // then the next nearest
            // without a wish they go to the primary
            assert_eq!(plan(&g, &names(&["a", "x"]), &old, None)[1], (0, 0));
        }

        #[test]
        fn the_bar_strips_shape_each_monitors_icon_area() {
            // no bar: a small gap at the top and bottom
            let a = area_of((1920, 0, 3840, 1080), (1920, 0, 3840, 1080), false);
            assert_eq!(a.work, (1920, 8, 3840, 1072));
            // a bar reserving 40 px on top, another 36 px at the bottom
            let a = area_of((0, 0, 2560, 1440), (0, 40, 2560, 1404), false);
            assert_eq!(a.work, (0, 48, 2560, 1400));
            // the dock floats over the bottom edge without reserving: a fixed gap
            let a = area_of((0, 0, 1920, 1080), (0, 0, 1920, 1080), true);
            assert_eq!(a.work, (0, 8, 1920, 1028));
            // a side bar's strip is outside the work area already
            assert_eq!(area_of((0, 0, 1920, 1080), (60, 0, 1920, 1080), false).work.0, 60);
        }

        #[test]
        fn positions_survive_the_file_both_ways_with_negative_coordinates_and_odd_names() {
            let items = [("a.txt", (0, 0)), ("New folder (2)", (-300, 90)), ("tab\u{e9} \u{65e5}\u{672c}.lnk", (4000, -180))];
            let text = saved_text(items.iter().map(|&(n, p)| (n, p)));
            let back = parse_saved(&text);
            assert_eq!(back.len(), 3);
            for (n, p) in items { assert_eq!(back[n], p); }
            // a file from before the second monitor read the same way, and a garbled line is skipped
            assert_eq!(parse_saved("76\t64\tx\nnot a line\n\t\t\n")["x"], (76, 64));
            // and the cells come out the same after the round trip
            let g = grid(&[mon(0, 0, 500, 360), mon(500, 0, 300, 180)]);
            let ns = names(&["p", "q"]);
            let old: HashMap<_, _> = [("p".to_string(), (200, 90)), ("q".to_string(), (700, 90))].into();
            let first = plan(&g, &ns, &old, None);
            let again = parse_saved(&saved_text(ns.iter().zip(&first).map(|(n, &p)| (n.as_str(), p))));
            assert_eq!(plan(&g, &ns, &again, None), first);
        }
    }
}

use windows::Win32::{System::Com::CoTaskMemFree, UI::Shell::{FOLDERID_Desktop, FOLDERID_PublicDesktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT}};
use std::ffi::c_void;

/// [user Desktop, public Desktop] (the user one may be redirected, e.g. to OneDrive).
pub(crate) fn desktop_dirs() -> Vec<PathBuf> {
        [&FOLDERID_Desktop, &FOLDERID_PublicDesktop].iter().filter_map(|id| unsafe {
            let p = SHGetKnownFolderPath(*id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const c_void));
            s.map(PathBuf::from)
        }).collect()
    }

#[allow(unsafe_op_in_unsafe_fn)]
pub(crate) unsafe fn setup_list(h: HWND, list: HWND, inst: HINSTANCE, fi: &mut SHFILEINFOW) {
                    SORT_COL.store(sort_col(view().sort), Relaxed); // the order the bar's menu chose last time
                    // system large icons; indexes are shared with the small list, so icon_of() works unchanged
                    let big = SHGetFileInfoW(w("C:\\").as_ptr(), 0, fi, size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX | SHGFI_LARGEICON);
                    put(list, LVM_SETIMAGELIST, LVSIL_NORMAL as usize, big as isize);
                    apply_icon_size(list);
                    // transparent background (the desktop paints it) and shadowed labels, readable on light wallpapers too
                    let ex = LVS_EX_TRANSPARENTBKGND | 0x0080_0000; // LVS_EX_TRANSPARENTSHADOWTEXT
                    put(list, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as usize, ex as isize);
                    for msg in [LVM_SETBKCOLOR, LVM_SETTEXTBKCOLOR] { put(list, msg, 0, CLR_NONE); }
                    put(list, LVM_SETTEXTCOLOR, 0, 0x00FF_FFFF);
                    reload_wall();
                    // grey (not ClearType) smoothing: on a live wallpaper the labels sit on a colour key, where ClearType leaves coloured fringes
                    put(list, WM_SETFONT, windows_sys::Win32::Graphics::Gdi::CreateFontW(-ui::sc(13), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 4, 0, w(ui::ui_face()).as_ptr()) as usize, 1);
                    // like explorer's desktop: wallpaper apps look for a SHELLDLL_DefView under Progman
                    CreateWindowExW(0, w("SHELLDLL_DefView").as_ptr(), ptr::null(), WS_CHILD, 0, 0, 0, 0, h, 0 as _, inst, ptr::null());
                    SetTimer(h, 7, 2000, None); // live wallpaper on / off (desk::check_live)
}

pub(crate) fn register_resident() {
                    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos};
                    let mut dirs: Vec<PathBuf> = [&FOLDERID_Desktop, &FOLDERID_Downloads, &FOLDERID_Documents, &FOLDERID_Pictures, &FOLDERID_Music, &FOLDERID_Videos].iter().filter_map(|id| sh::known_dir(id)).collect();
                    dirs.extend(ui::pins());
                    shellwin::register_resident(&dirs);
}

#[allow(unsafe_op_in_unsafe_fn)]
pub(crate) unsafe fn create_window(inst: HINSTANCE, bg: windows_sys::Win32::Graphics::Gdi::HBRUSH) {
            // the whole screen: it paints the wallpaper too; the icon list sits above the bar (desk_top / desk_bottom)
            // class "Progman" like explorer's desktop, so Wallpaper Engine & co. find it; title "rs-desktop" tells it apart
            // from explorer's ("Program Manager") for rs-shell
            let pm = w("Progman");
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst, lpszClassName: pm.as_ptr(), hCursor: LoadCursorW(0 as _, IDC_ARROW), hIcon: LoadIconW(inst, 1 as _), hbrBackground: bg, ..std::mem::zeroed() });
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(desk::workerw_proc), hInstance: inst, lpszClassName: w("WorkerW").as_ptr(), ..std::mem::zeroed() });
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: inst, lpszClassName: w("SHELLDLL_DefView").as_ptr(), ..std::mem::zeroed() });
            let (vx, vy, vw, vh) = virt(); // every monitor: the wallpaper reaches the secondary ones too (#67)
            let d = CreateWindowExW(WS_EX_TOOLWINDOW, pm.as_ptr(), w("rs-desktop").as_ptr(), WS_POPUP, vx, vy, vw, vh, 0 as _, 0 as _, inst, ptr::null());
            ChangeWindowMessageFilterEx(d, deskmenu::cmd_msg(), MSGFLT_ALLOW, ptr::null_mut()); // the bar may run elevated, we do not: UIPI would drop its choice
            SetWindowPos(d, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

