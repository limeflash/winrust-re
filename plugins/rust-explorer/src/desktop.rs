use super::*;
use winrust_common::deskmenu::{self, Cmd, IconSize, Request, Sort};

const CLR_NONE: isize = 0xFFFF_FFFF; // list background / text background: let the parent's wallpaper show through
thread_local! { static WALL: std::cell::Cell<(isize, i32, i32)> = const { std::cell::Cell::new((0, 0, 0)) }; } // (HBITMAP, w, h)
/// The virtual screen (every monitor): x, y, width, height. The desktop window covers it so each monitor gets its wallpaper.
pub(crate) fn virt() -> (i32, i32, i32, i32) {
    unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) }
}
/// The desktop's icon list stays on the primary monitor (at screen 0,0, i.e. -vx, -vy inside the desktop window), between the bar's strips.
pub(crate) fn place_desk_list(list: HWND) {
    let (vx, vy, ..) = virt();
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    unsafe { MoveWindow(list, -vx, -vy + desk_top(), sw, (sh - desk_top() - desk_bottom()).max(0), 1) };
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
/// What the bar reserves at the top and bottom of the screen: the work area topbar sets when there is no explorer.
fn work_strips() -> (i32, i32) {
    let mut r: RECT = unsafe { std::mem::zeroed() };
    if unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut _ as *mut _, 0) } == 0 { return (0, 0); }
    (r.top.max(0), (unsafe { GetSystemMetrics(SM_CYSCREEN) } - r.bottom).max(0))
}
/// Margins of the desktop's icon list, from the bar's real size: its reserved strip (any height, top or bottom).
/// The dock style reserves nothing but floats over the bottom edge, so it gets a fixed gap while topbar runs.
fn desk_top() -> i32 { work_strips().0 + ui::sc(8) }
fn desk_bottom() -> i32 {
    let reserved = work_strips().1;
    if reserved > 0 { return reserved + ui::sc(4); }
    let bar_up = unsafe { !FindWindowW(w("topbar").as_ptr(), ptr::null()).is_null() };
    if bar_up && winrust_common::style::style() == winrust_common::style::Style::Dock { ui::sc(52) } else { ui::sc(8) }
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
    unsafe { GetCursorPos(&mut pt) };
    let v = view();
    let drive = tool("wiztree").and_then(|_| drive_of(&cur_dir()));
    deskmenu::ask(&Request { at: (pt.x, pt.y), sort: v.sort, size: v.size, auto: v.auto, shown: !icons_hidden(), drive })
}

/// What the bar's menu chose. The segments and the toggles carry the value they set.
pub(crate) fn command(h: HWND, cmd: Cmd) {
    let (list, dir, mut v) = (ST.with_borrow(|st| st.list), cur_dir(), view());
    let drop_cells = || { let _ = fs::remove_file(desk::pos_file()); }; // the next load lays every icon out again
    match cmd {
        Cmd::NewFolder => new_entry(&dir, t!("Новая папка", "New folder", ja: "新しいフォルダー", pt: "Nova pasta", zh: "新建文件夹"), true),
        Cmd::NewText => new_entry(&dir, t!("Новый текстовый документ.txt", "New text document.txt", ja: "新しいテキスト ドキュメント.txt", pt: "Novo documento de texto.txt", zh: "新建文本文档.txt"), false),
        Cmd::Paste => paste(h, &dir),
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
    use std::{cell::Cell, collections::{HashMap, HashSet}};
    use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, HDC, ScreenToClient};

    pub const SPAWN_WORKERW: u32 = 0x052C; // what wallpaper apps send to "Progman" to get a window behind the icons
    const KEY: u32 = 0x0001_0001; // rgb(1, 0, 1): the list's background colour, made see-through while a live wallpaper runs

    thread_local! {
        static DRAG_FROM: Cell<Option<(i32, i32)>> = const { Cell::new(None) }; // our own icon drag: where it started (list coords)
        static DROP_AT: Cell<Option<(i32, i32)>> = const { Cell::new(None) }; // ...and where it was dropped (screen coords)
        static WORKERW: Cell<isize> = const { Cell::new(0) };
        static LIVE: Cell<bool> = const { Cell::new(false) }; // a live wallpaper is running in WORKERW
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

    /// Saved icon positions: name -> (x, y) in list pixels.
    fn saved() -> HashMap<String, (i32, i32)> {
        fs::read_to_string(pos_file()).unwrap_or_default().lines().filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.to_string()))
        }).map(|(x, y, n): (i32, i32, String)| (n, (x, y))).collect()
    }

    fn names() -> Vec<String> { ST.with_borrow(|st| st.rows.iter().map(|r| r.name.clone()).collect()) }

    fn pos(list: HWND, i: usize) -> (i32, i32) {
        let mut p = POINT { x: 0, y: 0 };
        put(list, LVM_GETITEMPOSITION, i, &mut p as *mut _ as isize);
        (p.x, p.y)
    }

    fn set_pos(list: HWND, i: usize, (x, y): (i32, i32)) {
        let p = POINT { x, y };
        put(list, LVM_SETITEMPOSITION32, i, &p as *const _ as isize);
    }

    /// Writes every icon's position (after a move or a fresh layout). "Arrange automatically" keeps none: the layout is the
    /// sort order, so the positions from before stay put for when it is switched off (which writes them anew).
    pub fn save(list: HWND) {
        if view().auto { return; }
        let lines: String = names().iter().enumerate().map(|(i, n)| { let (x, y) = pos(list, i); format!("{x}\t{y}\t{n}\n") }).collect();
        let f = pos_file();
        let _ = fs::create_dir_all(f.parent().unwrap_or(&f));
        let _ = fs::write(f, lines);
    }

    /// The icon grid: cell size, and how many columns / rows fit.
    struct Grid { cx: i32, cy: i32, cols: i32, rows: i32 }

    impl Grid {
        fn of(list: HWND) -> Grid {
            let s = put(list, LVM_GETITEMSPACING, 0, 0) as u32;
            let (cx, cy) = (((s & 0xFFFF) as i32).max(1), ((s >> 16) as i32).max(1));
            let mut r: RECT = unsafe { std::mem::zeroed() };
            unsafe { GetClientRect(list, &mut r) };
            if r.bottom <= 0 {
                // the first fill runs in WM_CREATE, before WM_SIZE gives the list its size: the size it is about to get
                r = unsafe { RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) - desk_top() - desk_bottom() } };
            }
            Grid { cx, cy, cols: (r.right / cx).max(1), rows: (r.bottom / cy).max(1) }
        }
        /// Nearest cell to a position; None if it is off the grid (screen got smaller).
        fn cell(&self, (x, y): (i32, i32)) -> Option<(i32, i32)> {
            let c = ((x + self.cx / 2).div_euclid(self.cx), (y + self.cy / 2).div_euclid(self.cy));
            (c.0 >= 0 && c.1 >= 0 && c.0 < self.cols && c.1 < self.rows).then_some(c)
        }
        fn at(&self, (c, r): (i32, i32)) -> (i32, i32) { (c * self.cx, r * self.cy) }
        /// Free cell nearest to `want`; with no wish the first free one going down the columns, like Windows.
        fn free(&self, taken: &HashSet<(i32, i32)>, want: Option<(i32, i32)>) -> (i32, i32) {
            let all = (0..self.cols).flat_map(|c| (0..self.rows).map(move |r| (c, r))).filter(|c| !taken.contains(c));
            match want {
                Some((wc, wr)) => all.min_by_key(|&(c, r)| (c - wc).pow(2) + (r - wr).pow(2)),
                None => all.min(),
            }
            .unwrap_or((0, 0))
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
        let old = if view().auto { HashMap::new() } else { saved() };
        for (i, p) in plan(&Grid::of(list), &names(), &old).into_iter().enumerate() { set_pos(list, i, p); }
        save(list);
    }

    /// Where each icon goes (list pixels, in the order of `names`): on its saved cell when that is on the grid and nobody else
    /// has it, the others on the first free cells going down the columns. With nothing saved (arranging automatically) that is
    /// the sort order, column after column.
    fn plan(g: &Grid, names: &[String], old: &HashMap<String, (i32, i32)>) -> Vec<(i32, i32)> {
        let mut taken = HashSet::new();
        let mut out = vec![(0, 0); names.len()];
        let mut later = vec![];
        for (i, name) in names.iter().enumerate() {
            match old.get(name).and_then(|&p| g.cell(p)).filter(|c| !taken.contains(c)) {
                Some(c) => { taken.insert(c); out[i] = g.at(c); }
                None => later.push(i),
            }
        }
        for i in later {
            let c = g.free(&taken, None);
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
    /// icons move by the drag distance, snapped to the grid, each to the nearest free cell. true = handled here.
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
            for (i, p) in plan(&g, &names(), &HashMap::new()).into_iter().enumerate() { set_pos(list, i, p); }
            return true;
        }
        let n = put(list, LVM_GETITEMCOUNT, 0, 0).max(0) as usize;
        let mut taken: HashSet<(i32, i32)> = (0..n).filter(|i| !sel.contains(i)).filter_map(|i| g.cell(pos(list, i))).collect();
        let (dx, dy) = (pt.x - from.0, pt.y - from.1);
        for &i in &sel {
            let (x, y) = pos(list, i);
            let want = g.cell((x + dx, y + dy)).unwrap_or_else(|| {
                // dragged off the grid: the nearest edge cell
                (((x + dx) / g.cx).clamp(0, g.cols - 1), ((y + dy) / g.cy).clamp(0, g.rows - 1))
            });
            let c = if taken.contains(&want) { g.free(&taken, Some(want)) } else { want };
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
        use super::{Grid, plan};
        use std::collections::{HashMap, HashSet};

        #[test]
        fn grid_cells() {
            let g = Grid { cx: 100, cy: 90, cols: 5, rows: 4 };
            assert_eq!(g.cell((0, 0)), Some((0, 0)));
            assert_eq!(g.cell((149, 44)), Some((1, 0))); // rounds to the nearest cell
            assert_eq!(g.cell((900, 0)), None); // off the grid
            let mut taken: HashSet<_> = [(0, 0), (0, 1)].into();
            assert_eq!(g.free(&taken, None), (0, 2)); // down the first column first
            taken.insert((2, 2));
            assert_eq!(g.free(&taken, Some((2, 2))), (1, 2)); // nearest free to the wish (ties: lowest)
        }

        fn names(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

        #[test]
        fn saved_cells_are_kept_and_the_rest_fills_the_free_ones() {
            let g = Grid { cx: 100, cy: 90, cols: 3, rows: 2 };
            let old: HashMap<_, _> = [("b".to_string(), (200, 90)), ("gone".to_string(), (0, 0)), ("off".to_string(), (900, 900))].into();
            // b keeps (2, 1); a and c (new) and "off" (its cell is off the grid now) take the free cells down the columns
            let p = plan(&g, &names(&["a", "b", "c", "off"]), &old);
            assert_eq!(p, vec![(0, 0), (200, 90), (0, 90), (100, 0)]);
        }

        #[test]
        fn arranging_automatically_ignores_the_saved_cells_and_follows_the_order_down_the_columns() {
            let g = Grid { cx: 100, cy: 90, cols: 3, rows: 2 };
            let p = plan(&g, &names(&["a", "b", "c", "d"]), &HashMap::new());
            assert_eq!(p, vec![(0, 0), (0, 90), (100, 0), (100, 90)]);
            // the same names with a different sort order: a different layout, whatever was saved
            let p = plan(&g, &names(&["d", "c", "b", "a"]), &[("a".to_string(), (200, 90))].into());
            assert_eq!(p[3], (200, 90)); // not auto: a keeps its cell
            assert_eq!(plan(&g, &names(&["d", "c", "b", "a"]), &HashMap::new())[3], (100, 90)); // auto: it is the fourth
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

