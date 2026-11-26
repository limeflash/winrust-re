// Own flyouts instead of the stock Windows ones: the bar's menus. One popup window; what it shows depends on `Kind`.
// (Start, search, quick settings, Wi-Fi and the calendar are panels of `ui::panel` now.)
use super::*;
use std::sync::atomic::Ordering::Relaxed;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Menu, // right-click menu of the bar
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Menu(u8),
}

pub static PREVIEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false); // `--preview`: stays open without focus

thread_local! {
    static FLY: Cell<Option<Kind>> = const { Cell::new(None) };
    static FH: Cell<HWND> = const { Cell::new(null_mut()) };
    static OVER: Cell<Option<Act>> = const { Cell::new(None) };
    static MENU_X: Cell<i32> = const { Cell::new(0) };
    static FIN: Cell<(i32, i32)> = const { Cell::new((0, 0)) }; // where the open flyout rests
    static T0: Cell<u64> = const { Cell::new(0) }; // when it started opening
    static GHOST: Cell<(HWND, u64, (i32, i32))> = const { Cell::new((null_mut(), 0, (0, 0))) }; // the closing flyout: window, start, position
}

pub fn is_open(k: Kind) -> bool {
    FLY.get() == Some(k)
}

pub fn any_open() -> bool {
    FLY.get().is_some()
}

// ---------- geometry ----------

fn size(k: Kind) -> (i32, i32) {
    match k {
        Kind::Menu => (MW, menu_rows().1),
    }
}

fn r(l: i32, t: i32, rr: i32, b: i32) -> RECT {
    RECT { left: sc(l), top: sc(t), right: sc(rr), bottom: sc(b) }
}

fn rects(k: Kind) -> Vec<(RECT, Act)> {
    let (w, _) = size(k);
    match k {
        Kind::Menu => {
            menu_rows().0.into_iter().enumerate().map(|(i, (t, b))| (r(6, t, w - 6, b), Act::Menu(i as u8))).collect()
        }
    }
}

fn act_at(k: Kind, x: i32, y: i32) -> Option<(RECT, Act)> {
    rects(k).into_iter().rev().find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
}

// ---------- open / close ----------

pub fn init(hi: HINSTANCE) {
    unsafe {
        let class = wide("topbar_fly");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: hi, hCursor: LoadCursorW(null_mut(), IDC_ARROW), lpszClassName: class.as_ptr(), ..zeroed() };
        RegisterClassW(&wc);
    }
}

/// One row of a menu flyout: a label (with a window's icon when it stands for a window), a dim keyboard hint on the right,
/// a dot when it is the current choice, red when it destroys something, a separator line above it when `sep`.
pub struct MItem {
    pub icon: HICON,
    pub label: String,
    pub hint: &'static str,
    pub on: bool,
    pub danger: bool,
    pub sep: bool,
}

impl MItem {
    pub fn new(label: impl Into<String>) -> Self {
        MItem { icon: null_mut(), label: label.into(), hint: "", on: false, danger: false, sep: false }
    }
    pub fn hint(mut self, h: &'static str) -> Self {
        self.hint = h;
        self
    }
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
    pub fn sep(mut self) -> Self {
        self.sep = true;
        self
    }
}

type Pick = Box<dyn FnOnce(usize)>;
thread_local! {
    static MITEMS: RefCell<Vec<MItem>> = const { RefCell::new(Vec::new()) };
    static MHEAD: RefCell<Option<(HICON, &'static str, String, String)>> = const { RefCell::new(None) }; // icon (or a glyph when null), title, subtitle
    static MPICK: RefCell<Option<Pick>> = const { RefCell::new(None) };
}
const MW: i32 = 272; // menu width
const MROW: i32 = 38;
const MHEAD_H: i32 = 58;

/// Menu flyout above the bar, centered on screen x `cx`. `pick` gets the index of the chosen item (not called on cancel).
/// A right click while a menu is open replaces it.
pub fn menu(cx: i32, head: Option<(HICON, &'static str, String, String)>, items: Vec<MItem>, pick: impl FnOnce(usize) + 'static) {
    if is_open(Kind::Menu) {
        close_now();
    }
    MITEMS.set(items);
    MHEAD.set(head);
    MPICK.set(Some(Box::new(pick)));
    MENU_X.set(if vert() { cx } else { cx - sc(MW) / 2 }); // along a side bar it holds the anchor y
    toggle(Kind::Menu);
}

/// (top, bottom) of each menu item in logical px, and the panel height.
fn menu_rows() -> (Vec<(i32, i32)>, i32) {
    let mut y = if MHEAD.with_borrow(|h| h.is_some()) { MHEAD_H + 9 } else { 6 };
    let rows = MITEMS.with_borrow(|m| {
        m.iter()
            .map(|it| {
                if it.sep {
                    y += 9;
                }
                y += MROW;
                (y - MROW, y)
            })
            .collect()
    });
    (rows, y + 6)
}

/// Starts the fade-out of the open flyout (it stays as a ghost window until the fade ends).
pub fn close() {
    let h = FH.replace(null_mut());
    FLY.set(None);
    unsafe { InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0) }; // the bar's open button goes out
    if h.is_null() {
        return;
    }
    unsafe {
        kill_ghost();
        GHOST.set((h, now_ms(), FIN.get()));
        KillTimer(h, 1);
        KillTimer(h, 2);
        SetTimer(h, 3, 15, None);
    }
}

/// Closes without the fade (another flyout replaces it, or a fullscreen app needs the screen).
pub fn close_now() {
    close();
    unsafe { kill_ghost() };
}

unsafe fn kill_ghost() {
    let (g, _, _) = GHOST.replace((null_mut(), 0, (0, 0)));
    if !g.is_null() {
        DestroyWindow(g);
    }
}

/// One frame of a flyout fade: opening rises up from the bar while fading in, closing fades out while sinking back down.
unsafe fn anim_step(hw: HWND) {
    let now = now_ms();
    let (g, gt0, gpos) = GHOST.get();
    if hw == g {
        let t = (now.saturating_sub(gt0) as f32 / 150.0).min(1.0);
        let q = t * t;
        SetLayeredWindowAttributes(hw, 0, ((1.0 - q) * 255.0) as u8, LWA_ALPHA);
        SetWindowPos(hw, null_mut(), gpos.0, gpos.1 + (q * sc(10) as f32) as i32, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        if t >= 1.0 {
            kill_ghost();
        }
    } else if hw == FH.get() {
        let t = (now.saturating_sub(T0.get()) as f32 / 220.0).min(1.0);
        let p = 1.0 - (1.0 - t).powi(3);
        SetLayeredWindowAttributes(hw, 0, (p * 255.0) as u8, LWA_ALPHA);
        let (x, y) = FIN.get();
        SetWindowPos(hw, null_mut(), x, y + ((1.0 - p) * sc(14) as f32) as i32, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        if t >= 1.0 {
            KillTimer(hw, 3);
        }
    } else {
        KillTimer(hw, 3);
    }
}

pub fn toggle(k: Kind) {
    // The panel counts as open only while its window is alive: Alt+F4 (or anything else destroying the window behind our
    // back) leaves the state saying "open", and then every Win press would only "close" a dead window and never open a new
    // one — the button looks dead until the bar restarts.
    if FLY.get() == Some(k) && unsafe { IsWindow(FH.get()) } != 0 {
        return close();
    }
    // No debounce here: a Win press right after the panel closed (or while it is still fading out) must open it. The bar is
    // WS_EX_NOACTIVATE, so the click that closes the panel never comes back as a second toggle of its own.
    close();
    unsafe {
        HV.with_borrow_mut(|m| m.retain(|k, _| *k < 1_000_000)); // flyout keys are >= 1_000_000: no stale glow from the last panel
        OVER.set(None);
        let (w, h) = (sc(size(k).0), sc(size(k).1));
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let x = if vert() { side_x(w) } else { match k {
            Kind::Menu => MENU_X.get().min(sw - w - sc(8)).max(sc(8)),
        } };
        let class = wide("topbar_fly");
        let fy = if vert() && k == Kind::Menu { (MENU_X.get() - h / 2).clamp(sc(8), GetSystemMetrics(SM_CYSCREEN) - h - sc(8)) } else { pop_y(h) }; // above the bar (below it at the top of the screen)
        let hw = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED, class.as_ptr(), null(), WS_POPUP, x, fy + sc(14), w, h, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        SetLayeredWindowAttributes(hw, 0, 0, LWA_ALPHA); // starts invisible, anim_step fades it in
        FIN.set((x, fy));
        T0.set(now_ms());
        SetWindowRgn(hw, CreateRoundRectRgn(0, 0, w + 1, h + 1, rr(16), rr(16)), 1);
        FH.set(hw);
        FLY.set(Some(k));
        InvalidateRect(BAR.load(Relaxed) as HWND, null(), 0); // the bar's button lights up
        ShowWindow(hw, SW_SHOW);
        force_fg(hw);
        SetTimer(hw, 1, 1000, None);
        SetTimer(hw, 3, 15, None);
    }
}

// ---------- actions ----------

unsafe fn run_act(a: Act) {
    match a {
        Act::Menu(i) => {
            close();
            if let Some(pick) = MPICK.take() {
                pick(i as usize);
            }
        }
    }
}

// ---------- painting ----------

fn inset(rc: RECT, d: i32) -> RECT {
    RECT { left: rc.left + d, top: rc.top + d, right: rc.right - d, bottom: rc.bottom - d }
}

unsafe fn paint(hw: HWND, k: Kind) {
    let mut ps: PAINTSTRUCT = zeroed();
    let wdc = BeginPaint(hw, &mut ps);
    let mut cr: RECT = zeroed();
    GetClientRect(hw, &mut cr);
    BUSY.set(false);
    let (dc, bmp) = (CreateCompatibleDC(wdc), CreateCompatibleBitmap(wdc, cr.right, cr.bottom));
    let old = SelectObject(dc, bmp);
    fill(dc, cr, mix(pal().bg, pal().text, 0.12), sc(16));
    fill(dc, inset(cr, 1), pal().bg, sc(16));
    SetBkMode(dc, TRANSPARENT as i32);
    let (face, gface) = (wide(ui_face()), wide(winrust_common::style::icon_face()));
    let font = |f: &[u16], px: i32, wt: i32| CreateFontW(-sc(px), 0, 0, 0, wt, 0, 0, 0, 1, 0, 0, 5, 0, f.as_ptr());
    let (f_txt, f_small, f_glyph, f_title, f_big) = (font(&face, 13, 400), font(&face, 12, 400), font(&gface, 16, 400), font(&face, 15, 600), font(&face, 34, 600));
    let (f_semi, f_head, f_gs, f_label) = (font(&face, 13, 600), font(&face, 18, 700), font(&gface, 11, 400), font(&face, 12, 600));
    let of = SelectObject(dc, f_txt);
    let (w, _) = size(k);
    let over = OVER.get();
    let hot = |a: Act| over == Some(a);
    // hover highlight that fades; the key is the element's position (flyout keys start at 1_000_000)
    let g = |a: Act, rc: RECT, from: u32, to: u32, rad: i32| glow(dc, rc.top * 4096 + rc.left + 1_000_000, hot(a), rc, from, to, rad);

    match k {
        Kind::Menu => {
            // header: the app's icon on an accent-tinted tile, its name, and what is open
            MHEAD.with_borrow(|hd| {
                if let Some((ic, glyph, title, sub)) = hd {
                    let tile = r(14, 12, 48, 46);
                    fill(dc, tile, mix(pal().bg, pal().accent, 0.16), sc(10));
                    if !ic.is_null() {
                        DrawIconEx(dc, tile.left + sc(5), tile.top + sc(5), *ic, sc(24), sc(24), 0, null_mut(), DI_NORMAL);
                    } else {
                        SelectObject(dc, f_glyph); // no app icon (Style, Language): the menu's own symbol
                        text(dc, glyph, tile, pal().accent, DT_CENTER);
                    }
                    SelectObject(dc, f_title);
                    text(dc, title, r(60, 10, w - 14, 32), pal().text, DT_LEFT);
                    SelectObject(dc, f_small);
                    text(dc, sub, r(60, 30, w - 14, 48), pal().dim, DT_LEFT);
                    fill(dc, r(12, MHEAD_H + 4, w - 12, MHEAD_H + 5), pal().line, 0);
                }
            });
            let rs = rects(k);
            MITEMS.with_borrow(|items| {
                for (rc, a) in rs.iter() {
                    let Act::Menu(i) = *a;
                    let it = &items[i as usize];
                    if it.sep {
                        fill(dc, RECT { left: rc.left + sc(6), top: rc.top - sc(5), right: rc.right - sc(6), bottom: rc.top - sc(5) + 1 }, pal().line, 0);
                    }
                    let col = if it.danger { BAD } else { pal().text };
                    g(*a, *rc, pal().bg, if it.danger { mix(pal().bg, BAD, 0.22) } else { pal().hover }, sc(8));
                    // plain text rows: a window's own icon is the only picture; the current choice gets an accent dot
                    let mut lx = rc.left + sc(14);
                    if !it.icon.is_null() {
                        DrawIconEx(dc, lx, (rc.top + rc.bottom) / 2 - sc(8), it.icon, sc(16), sc(16), 0, null_mut(), DI_NORMAL);
                        lx += sc(26);
                    }
                    let mut rx = rc.right - sc(14);
                    if it.on {
                        let (cy, d) = ((rc.top + rc.bottom) / 2, sc(4));
                        fill(dc, RECT { left: rx - 2 * d, top: cy - d, right: rx, bottom: cy + d }, pal().accent, 2 * d);
                        rx -= sc(18);
                    }
                    SelectObject(dc, f_small);
                    if !it.hint.is_empty() {
                        text(dc, it.hint, RECT { left: lx, right: rx, ..*rc }, mix(pal().dim, pal().bg, 0.25), DT_RIGHT);
                    }
                    SelectObject(dc, f_txt);
                    text(dc, &it.label, RECT { left: lx, right: rx - match it.hint.chars().count() { 0 => 0, 1..=2 => sc(20), _ => sc(96) }, ..*rc }, if it.on { pal().accent } else { col }, DT_LEFT);
                }
            });
        }
    }

    BitBlt(wdc, 0, 0, cr.right, cr.bottom, dc, 0, 0, SRCCOPY);
    SelectObject(dc, of);
    for f in [f_txt, f_small, f_glyph, f_title, f_big, f_semi, f_head, f_gs, f_label] {
        DeleteObject(f);
    }
    SelectObject(dc, old);
    DeleteObject(bmp);
    DeleteDC(dc);
    if BUSY.get() {
        SetTimer(hw, 2, 16, None); // next animation frame
    } else {
        KillTimer(hw, 2);
    }
    EndPaint(hw, &ps);
}

// ---------- window proc ----------

unsafe extern "system" fn proc(hw: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_TIMER && wp == 3 {
        anim_step(hw); // also for the fading-out ghost window
        return 0;
    }
    if hw != FH.get() {
        return DefWindowProcW(hw, msg, wp, lp); // ghost or a window still being created
    }
    let Some(k) = FLY.get() else { return DefWindowProcW(hw, msg, wp, lp) };
    let (x, y) = ((lp & 0xffff) as i16 as i32, (lp >> 16 & 0xffff) as i16 as i32);
    match msg {
        WM_PAINT => paint(hw, k),
        WM_ERASEBKGND => return 1,
        WM_ACTIVATE if wp & 0xffff == 0 && !PREVIEW.load(Relaxed) => {
            close();
        }
        WM_TIMER => {
            InvalidateRect(hw, null(), 0);
        }
        WM_MOUSEMOVE => {
            let o = act_at(k, x, y).map(|a| a.1);
            if OVER.replace(o) != o {
                InvalidateRect(hw, null(), 0);
            }
        }
        WM_LBUTTONDOWN => {
            if let Some((_, a)) = act_at(k, x, y) {
                run_act(a);
            }
            if FLY.get().is_some() {
                InvalidateRect(hw, null(), 0);
            }
        }
        WM_CHAR if wp == 27 => {
            close();
            return 0;
        }
        _ => return DefWindowProcW(hw, msg, wp, lp),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{init, is_open, toggle, close_now, Kind, FLY, FH, GetModuleHandleW, SetProcessDPIAware, null, null_mut, DestroyWindow, IsWindow};

    /// A bar menu always opens: a press right after a close is not dropped, and a state left saying "open" with no
    /// window behind it (CreateWindowExW failed) does not turn the toggle into a close-only no-op.
    #[test]
    fn a_menu_always_opens() {
        unsafe {
            SetProcessDPIAware();
            init(GetModuleHandleW(null()));
            toggle(Kind::Menu);
            assert!(is_open(Kind::Menu), "the first press opens the menu");
            toggle(Kind::Menu);
            assert!(!is_open(Kind::Menu), "the second press closes it");
            toggle(Kind::Menu);
            assert!(is_open(Kind::Menu), "a press right after the close opens it again");
            let live = FH.get();
            assert!(!live.is_null() && IsWindow(live) != 0);
            DestroyWindow(live);
            close_now();
            // the window is gone but the state still says "open" (what a failed creation or a kill behind our back leaves)
            FLY.set(Some(Kind::Menu));
            FH.set(null_mut());
            toggle(Kind::Menu);
            assert!(is_open(Kind::Menu) && IsWindow(FH.get()) != 0, "the press opens a real window");
            close_now();
        }
    }
}
