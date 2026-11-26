//! topbar: the host of the screenshot picker (Win+Shift+S) and the screen recording (Win+Shift+R), split out of main.rs.
//!
//! What it shows is `ui::snip` (the model and the painting). Here are the windows and the input, on a thread of its own with its own
//! message loop (so a busy bar cannot stall it, like the old clipper): one overlay window per monitor with the frozen picture of
//! it, the countdown disc, and for a recording the frame round the area and the floating control. All of them are DirectComposition
//! windows (layered ones with `WINRUST_ULW=1`) and all of them are kept out of what is captured (`WDA_EXCLUDEFROMCAPTURE`), so a
//! recording never contains the picker, the frame or the control.
//!
//! The thread lives while something is on screen (a picker, a countdown, a recording) and ends after it. The bar and the keyboard hook
//! talk to it through [`open`], [`toggle_rec`], [`stop_rec`] and [`cancel`]; what it finished comes back as [`Outcome`]s for the bar's
//! thread, which owns the notifications.
use super::*;
use crate::ui::{
    self,
    gfx::{self, Gfx, Picture, Surface},
    snip::{self as model, Act, Ctl, Hit, IRect, Key, Kind, Mode, Mon, Opts, RecView, Snip, Win},
};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicU64},
    time::Duration,
};
use windows_sys::Win32::{
    Graphics::Dwm::{DwmFlush, DwmGetWindowAttribute},
    UI::{
        HiDpi::{GetDpiForMonitor, SetThreadDpiAwarenessContext},
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent},
        WindowsAndMessaging::{
            GetCursorPos, GetWindowTextW, IDC_CROSS, IDC_HAND, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, SetCursor,
            SetWindowDisplayAffinity,
        },
    },
};

const WM_CMD: u32 = WM_APP + 1;
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
const T_COUNT: usize = 1;
const T_REC: usize = 2;
const T_FLASH: usize = 3;
const T_END: usize = 4;
const T_RECEND: usize = 5;
/// Room around the control's box for its shadow, DIPs.
const PAD: f32 = 70.0;

// ---------------------------------------------------------------------------------------------------------------------
// talking to the thread

enum Cmd {
    /// Runs a closure on the thread (the live smoke test drives the host with it).
    Run(Box<dyn FnOnce(&mut Host) + Send>),
    Open(Kind),
    ToggleRec,
    StopRec,
    Cancel,
}

struct Link {
    hwnd: isize,
    starting: bool,
    queue: VecDeque<Cmd>,
}

static LINK: Mutex<Link> = Mutex::new(Link { hwnd: 0, starting: false, queue: VecDeque::new() });
/// A countdown is running: Esc cancels it (its disc takes no keys; the keyboard hook asks for the cancel).
pub static COUNTING: AtomicBool = AtomicBool::new(false);
/// `GetTickCount64` at the start of the recording in progress, 0 when none.
static REC_SINCE: AtomicU64 = AtomicU64::new(0);
static OUTCOMES: Mutex<Vec<Outcome>> = Mutex::new(Vec::new());

/// A fake desktop for the live smoke test (`--render-panel shot-live`): the next picker opens on it instead of on the real screen.
struct Stage {
    mons: Vec<Mon>,
    wins: Vec<Win>,
    frozen: Vec<Vec<u32>>,
}

static STAGE: Mutex<Option<Stage>> = Mutex::new(None);
/// The smoke test is running: its windows stay out of the focus and are not hidden from capture (the test reads them back).
static STAGED: AtomicBool = AtomicBool::new(false);

/// What came of a capture, for the bar's thread to tell the user.
pub enum Outcome {
    Shot { name: Option<String>, copied: bool, size: (u32, u32), preview: Option<String>, notify: bool },
    ShotFailed,
    Rec { path: String, secs: u64, bytes: u64, thumb: Option<String>, notify: bool },
    RecFailed(String),
}

fn send(c: Cmd) {
    let mut l = LINK.lock().unwrap_or_else(|e| e.into_inner());
    l.queue.push_back(c);
    if l.hwnd != 0 {
        // SAFETY: a message to the thread's control window; it is cleared (under this lock) before the window goes.
        unsafe { PostMessageW(l.hwnd as HWND, WM_CMD, 0, 0) };
    } else if !l.starting {
        l.starting = true;
        drop(l);
        if thread::Builder::new().name("snip".into()).spawn(thread_main).is_err() {
            LINK.lock().unwrap_or_else(|e| e.into_inner()).starting = false;
        }
    }
}

/// Opens the picker (Win+Shift+S: a screenshot, Win+Shift+R: a recording); open already, it switches to that kind.
pub fn open(kind: Kind) {
    send(Cmd::Open(kind));
}

/// Win+Shift+R and the menu: stops the recording, or opens the picker for one.
pub fn toggle_rec() {
    send(Cmd::ToggleRec);
}

pub fn stop_rec() {
    send(Cmd::StopRec);
}

/// Closes the picker or the countdown.
pub fn cancel() {
    send(Cmd::Cancel);
}

/// Milliseconds the recording in progress has run, `None` when nothing is being recorded.
#[must_use]
pub fn rec_ms() -> Option<u64> {
    let at = REC_SINCE.load(Relaxed);
    // SAFETY: a plain clock read.
    (at != 0).then(|| unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }.saturating_sub(at))
}

/// The recording's time as the bar and the control write it.
#[must_use]
pub fn rec_time() -> Option<String> {
    rec_ms().map(|ms| model::clock_text(ms / 1000))
}

pub fn recording() -> bool {
    REC_SINCE.load(Relaxed) != 0
}

/// Takes what the thread finished (the bar's thread tells the user).
pub fn outcomes() -> Vec<Outcome> {
    std::mem::take(&mut *OUTCOMES.lock().unwrap_or_else(|e| e.into_inner()))
}

fn outcome(o: Outcome) {
    OUTCOMES.lock().unwrap_or_else(|e| e.into_inner()).push(o);
    // SAFETY: a message to the bar's window.
    unsafe { PostMessageW(BAR.load(Relaxed) as HWND, SNIP, 2, 0) };
}

fn opts_file() -> PathBuf {
    appdata_file("topbar-snip.txt")
}

fn load_opts() -> Opts {
    std::fs::read_to_string(opts_file()).map(|t| Opts::parse(&t)).unwrap_or_default()
}

fn save_opts(o: &Opts) {
    if STAGED.load(Relaxed) {
        return; // the smoke test leaves the user's choices alone
    }
    let _ = std::fs::write(opts_file(), o.to_text());
}

// ---------------------------------------------------------------------------------------------------------------------
// the screen

/// The monitors, device pixels of the virtual desktop (this thread is per-monitor DPI aware).
unsafe fn monitors() -> Vec<Mon> {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HMONITOR, MONITORINFO};
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> i32 {
        let out = &mut *(lp as *mut Vec<Mon>);
        let mut mi: MONITORINFO = zeroed();
        mi.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut mi) != 0 {
            let (mut dx, mut dy) = (96u32, 96u32);
            if GetDpiForMonitor(m, 0, &mut dx, &mut dy) != 0 || dx == 0 {
                dx = 96;
            }
            let r = mi.rcMonitor;
            out.push(Mon { rect: IRect::new(r.left, r.top, r.right, r.bottom), scale: dx as f32 / 96.0 });
        }
        1
    }
    let mut v: Vec<Mon> = Vec::new();
    EnumDisplayMonitors(null_mut(), null(), Some(each), &mut v as *mut _ as LPARAM);
    v.sort_by_key(|m| (m.rect.l, m.rect.t));
    v
}

/// The windows a picker can take, the topmost first.
unsafe fn windows_on_screen() -> Vec<Win> {
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> i32 {
        let out = &mut *(lp as *mut Vec<Win>);
        if IsWindowVisible(h) == 0 || IsIconic(h) != 0 {
            return 1;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) != 0 && ex & WS_EX_APPWINDOW == 0 {
            return 1;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == windows_sys::Win32::System::Threading::GetCurrentProcessId() {
            return 1; // the bar and its panels
        }
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(h, 14, (&mut cloaked as *mut u32).cast(), 4); // DWMWA_CLOAKED: on another virtual desktop, a hidden store app
        if cloaked != 0 {
            return 1;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(h, class.as_mut_ptr(), 64) as usize;
        if matches!(String::from_utf16_lossy(&class[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return 1;
        }
        let mut rc: RECT = zeroed();
        if DwmGetWindowAttribute(h, 9, (&mut rc as *mut RECT).cast(), size_of::<RECT>() as u32) != 0 && GetWindowRect(h, &mut rc) == 0 {
            return 1; // DWMWA_EXTENDED_FRAME_BOUNDS: the visible frame, without the invisible resize borders
        }
        if rc.right - rc.left < 40 || rc.bottom - rc.top < 30 {
            return 1;
        }
        let mut t = [0u16; 128];
        let n = GetWindowTextW(h, t.as_mut_ptr(), 128).max(0) as usize;
        out.push(Win { rect: IRect::new(rc.left, rc.top, rc.right, rc.bottom), title: String::from_utf16_lossy(&t[..n]) });
        1
    }
    let mut v: Vec<Win> = Vec::new();
    EnumWindows(Some(each), &mut v as *mut _ as LPARAM);
    v
}

/// The monitors as pictures, before anything of ours is on them.
fn freeze(mons: &[Mon]) -> Vec<Vec<u32>> {
    mons.iter()
        .map(|m| {
            let mut s = Surface::new();
            s.grab(m.rect.l, m.rect.t, m.rect.w(), m.rect.h());
            s.pixels().to_vec()
        })
        .collect()
}

/// The rectangle `r` of the picture of the monitor `mon`, as pixels (`w * h`).
fn crop(px: &[u32], mon: IRect, r: IRect) -> (Vec<u32>, i32, i32) {
    let w = mon.w() as usize;
    let (cw, ch) = (r.w().max(0) as usize, r.h().max(0) as usize);
    let mut out = Vec::with_capacity(cw * ch);
    for y in 0..ch {
        let row = (r.t - mon.t) as usize + y;
        let from = row * w + (r.l - mon.l) as usize;
        match px.get(from..from + cw) {
            Some(s) => out.extend_from_slice(s),
            None => out.resize(out.len() + cw, 0xff00_0000),
        }
    }
    (out, cw as i32, ch as i32)
}

/// A bitmap of opaque `w * h` pixels (a DIB section; the clipboard or the PNG encoder takes it from here).
unsafe fn bitmap_of(px: &[u32], w: i32, h: i32) -> HBITMAP {
    let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..zeroed() }, ..zeroed() };
    let mut bits = null_mut();
    let bmp = CreateDIBSection(null_mut(), &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if !bmp.is_null() && !bits.is_null() {
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits.cast::<u32>(), (w * h) as usize);
    }
    bmp
}

fn temp_png(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("winrust-snip");
    let _ = std::fs::create_dir_all(&dir);
    // previews and cover pictures only live as long as the notification that shows them: anything a day old is stale (once per run)
    static SWEPT: std::sync::Once = std::sync::Once::new();
    SWEPT.call_once(|| sweep_old(&dir, Duration::from_secs(24 * 3600)));
    dir.join(format!("{tag}-{}.png", unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }))
}

/// Deletes the files of `dir` last written more than `age` ago.
fn sweep_old(dir: &std::path::Path, age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|t| t > age) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Saves what was taken and tells the bar (its own thread: the encoder and the clipboard take a moment).
fn deliver(px: Vec<u32>, (w, h): (i32, i32), opts: Opts, copy_only: bool) {
    thread::spawn(move || unsafe {
        let bmp = bitmap_of(&px, w, h);
        if bmp.is_null() {
            return outcome(Outcome::ShotFailed);
        }
        let (clip, mut save) = (opts.clip || copy_only, opts.save && !copy_only);
        if !clip && !save {
            save = true; // both switched off: it must go somewhere
        }
        // a preview for the notification when no file is saved (the file itself is its own preview)
        let preview = (!save && opts.notify).then(|| temp_png("preview")).filter(|p| shot::write_png(bmp, p));
        match shot::save_bitmap(bmp, clip, save) {
            Some(s) => outcome(Outcome::Shot { name: s.saved.then_some(s.name), copied: s.copied, size: (w as u32, h as u32), preview: preview.map(|p| p.to_string_lossy().into_owned()), notify: opts.notify }),
            None => outcome(Outcome::ShotFailed),
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// windows

struct View {
    hwnd: HWND,
    mon: usize,
    gfx: Gfx,
    surf: Surface,
    pic: Option<Picture>,
    shown: bool,
}

/// An overlay window of one monitor, made but not shown.
unsafe fn make_window(class: &str, ex: u32, r: IRect, interactive: bool) -> Option<HWND> {
    let name = wide(class);
    let hi = GetModuleHandleW(null());
    ui::panel::register_class(hi, &name, Some(pick_proc), false);
    let h = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | ex | if interactive { gfx::window_ex() } else { gfx::window_ex_through() }, name.as_ptr(), null(), WS_POPUP, r.l, r.t, r.w(), r.h(), null_mut(), null_mut(), hi, null());
    if h.is_null() {
        return None;
    }
    if !STAGED.load(Relaxed) {
        SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
    }
    if !interactive {
        gfx::through(ui::panel::whwnd(h));
    }
    Some(h)
}

unsafe fn show(h: HWND, activate: bool) {
    SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | if activate { 0 } else { SWP_NOACTIVATE });
}

impl View {
    fn new(m: Mon, mon: usize, class: &str, ex: u32, interactive: bool) -> Option<Self> {
        let gfx = Gfx::new(m.scale).ok()?; // before the window: a failure here must not leave a topmost window behind
        // SAFETY: window creation on this thread.
        let hwnd = unsafe { make_window(class, ex, m.rect, interactive)? };
        Some(Self { hwnd, mon, gfx, surf: Surface::composed(), pic: None, shown: false })
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: our own window; the composition surface goes with `surf`.
        unsafe { DestroyWindow(self.hwnd) };
    }
}

struct Pick {
    snip: Snip,
    views: Vec<View>,
    frozen: Vec<Vec<u32>>,
    last_click: (u64, (i32, i32)),
    /// A capture was made: the flash plays and the picker closes by itself; input is ignored.
    busy: bool,
    hidden: bool,
}

impl Pick {
    /// Takes no input but Esc: a capture is under way, or a countdown runs over it.
    fn locked(&self) -> bool {
        self.busy || self.snip.counting
    }
}

struct Count {
    left: model::Countdown,
    since: u64,
    then: Then,
    view: View,
}

#[derive(Clone, Copy)]
enum Then {
    Shot(IRect, bool),
    Rec(IRect),
}

struct Rec {
    recorder: Option<rec::Recorder>,
    area: IRect,
    mon: Mon,
    last: String,
    frame: View,
    pill: View,
    mic: bool,
    thumb: Option<String>,
    notify: bool,
    since: u64,
    leaving: Option<u64>,
    level: [f32; 4],
    hot: bool,
    down: bool,
    ticks: u32,
}

struct Host {
    ctl: HWND,
    pick: Option<Pick>,
    count: Option<Count>,
    rec: Option<Rec>,
    /// A recording is being started on a thread of its own (the microphone and the first picture can take a while): the flag is
    /// set when Esc cancels it. The overlay thread stays alive meanwhile.
    starting: Option<std::sync::Arc<AtomicBool>>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static CURSOR: Cell<usize> = const { Cell::new(0) };
}

fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|h| h.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn now() -> u64 {
    ui::anim::now_ms()
}

fn thread_main() {
    // SAFETY: this thread is the overlay's: it makes its windows, runs their messages and ends when none is left.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        SetThreadDpiAwarenessContext(-4isize as _); // real pixels on every monitor, whatever its scale (#77)
        gfx::compose_windows();
        let hi = GetModuleHandleW(null());
        let name = wide("topbar_snip_ctl");
        ui::panel::register_class(hi, &name, Some(ctl_proc), false);
        let ctl = CreateWindowExW(0, name.as_ptr(), null(), 0, 0, 0, 0, 0, HWND_MESSAGE, null_mut(), hi, null());
        if ctl.is_null() {
            let mut l = LINK.lock().unwrap_or_else(|e| e.into_inner());
            l.starting = false;
            l.queue.clear();
            return;
        }
        HOST.set(Some(Host { ctl, pick: None, count: None, rec: None, starting: None }));
        {
            let mut l = LINK.lock().unwrap_or_else(|e| e.into_inner());
            l.hwnd = ctl as isize;
            l.starting = false;
        }
        drain();
        let mut m: MSG = zeroed();
        loop {
            // nothing left on screen: leave, unless a command slipped in (checked under the lock the senders take)
            if with(|h| h.idle()).unwrap_or(true) {
                let mut l = LINK.lock().unwrap_or_else(|e| e.into_inner());
                if l.queue.is_empty() {
                    l.hwnd = 0;
                    break;
                }
            }
            if GetMessageW(&mut m, null_mut(), 0, 0) <= 0 {
                break;
            }
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        HOST.set(None); // the windows go before the thread's devices do
        DestroyWindow(ctl);
    }
}

/// Runs what was asked of the thread.
fn drain() {
    loop {
        let c = LINK.lock().unwrap_or_else(|e| e.into_inner()).queue.pop_front();
        let Some(c) = c else { return };
        with(|h| unsafe { h.command(c) });
    }
}

unsafe extern "system" fn ctl_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CMD => {
            drain();
            0
        }
        m if m == crate::pacing::FRAME => {
            if crate::pacing::received(hwnd as isize) {
                with(|h| h.frame());
            }
            0
        }
        WM_TIMER => {
            let id = wp;
            with(|h| h.timer(id));
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: a plain query into a local.
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

impl Host {
    fn idle(&self) -> bool {
        self.pick.is_none() && self.count.is_none() && self.rec.is_none() && self.starting.is_none()
    }

    /// Wants a frame at the screen's next tick.
    fn request(&self) {
        crate::pacing::set(self.ctl as isize, true);
    }

    unsafe fn command(&mut self, c: Cmd) {
        match c {
            Cmd::Run(f) => f(self),
            Cmd::Open(k) => self.open(k),
            Cmd::ToggleRec if self.rec.as_ref().is_some_and(|r| r.leaving.is_none()) => self.stop_rec(),
            Cmd::ToggleRec => self.open(Kind::Rec),
            Cmd::StopRec => self.stop_rec(),
            Cmd::Cancel => {
                if let Some(cancelled) = &self.starting {
                    cancelled.store(true, Relaxed); // the recording that is starting is thrown away when it is up
                    if let Some(p) = &mut self.pick {
                        p.snip.counting = false;
                    }
                }
                if self.count.is_some() {
                    self.drop_count();
                }
                self.cancel_pick();
            }
        }
    }

    unsafe fn open(&mut self, kind: Kind) {
        if self.starting.is_some() {
            return;
        }
        if self.count.is_some() {
            return self.command(Cmd::Cancel); // a second press of the key during the countdown cancels it
        }
        let t = now();
        if let Some(p) = &mut self.pick {
            if !p.busy {
                p.snip.set_kind(kind, t);
                self.request();
            }
            return;
        }
        let staged = STAGED.load(Relaxed);
        let (mons, wins, frozen) = match STAGE.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(s) => (s.mons, s.wins, s.frozen),
            None => {
                let mons = monitors();
                let wins = windows_on_screen();
                let frozen = freeze(&mons); // before any window of ours is up: this is the picture the picker works on
                (mons, wins, frozen)
            }
        };
        if mons.is_empty() {
            return;
        }
        let mut opts = load_opts();
        opts.kind = kind;
        let at = if staged { 0 } else { model::mon_at(&mons, cursor_pos()) };
        let mut views = Vec::new();
        for (i, m) in mons.iter().enumerate() {
            if let Some(v) = View::new(*m, i, "topbar_snip", 0, true) {
                views.push(v);
            }
        }
        if views.len() != mons.len() {
            return;
        }
        let mut snip = Snip::new(mons, wins, opts, at, t);
        if !staged {
            snip.move_to(cursor_pos(), t); // the guides are on the pointer from the first frame
        }
        CURSOR.set(IDC_CROSS as usize);
        self.pick = Some(Pick { snip, views, frozen, last_click: (0, (0, 0)), busy: false, hidden: false });
        self.frame();
        if let Some(p) = &self.pick {
            for v in &p.views {
                show(v.hwnd, v.mon == at && !staged);
            }
            if !staged {
                super::force_fg(p.views[at].hwnd);
            }
        }
        self.request();
    }

    unsafe fn cancel_pick(&mut self) {
        let Some(p) = &mut self.pick else { return };
        if p.busy {
            return;
        }
        save_opts(&p.snip.opts);
        self.end_pick(true);
    }

    /// Takes the picker away: with its fade (`animate`), or at once.
    unsafe fn end_pick(&mut self, animate: bool) {
        let Some(p) = &mut self.pick else { return };
        if animate && !p.hidden {
            p.busy = true;
            p.snip.close(now());
            SetTimer(self.ctl, T_END, 190, None);
            self.request();
        } else {
            self.pick = None;
            gfx::trim();
        }
    }

    fn paint_pick(&mut self, t: u64) {
        let Some(p) = &mut self.pick else { return };
        if p.hidden {
            return;
        }
        let alpha = (255.0 * p.snip.opacity(t)).round() as u8;
        for (vi, v) in p.views.iter_mut().enumerate() {
            let m = p.snip.mons[v.mon];
            let px = p.frozen[vi].as_slice();
            let (snip, mon) = (&p.snip, v.mon);
            let _ = v.gfx.paint(&mut v.surf, m.rect.w(), m.rect.h(), |c| {
                if v.pic.as_ref().is_none_or(|pic| !c.is_current(pic)) {
                    v.pic = c.pixels(px, m.rect.w(), m.rect.h());
                }
                snip.paint(c, mon, v.pic.as_ref(), t);
            });
            let _ = v.surf.present(ui::panel::whwnd(v.hwnd), m.rect.l, m.rect.t, alpha);
        }
        gfx::commit();
        p.snip.dirty = false;
    }

    /// One frame of whatever is on screen.
    fn frame(&mut self) {
        let t = now();
        let mut busy = false;
        if let Some(p) = &self.pick
            && (p.snip.dirty || p.snip.busy(t))
        {
            self.paint_pick(t);
        }
        if let Some(p) = &self.pick {
            busy |= p.snip.busy(t) || p.snip.dirty;
        }
        if let Some(c) = &mut self.count {
            let sz = 180.0;
            let px = (sz * c.view.gfx.scale()).round() as i32;
            let (n, since) = (c.left.shown(), c.since);
            let _ = c.view.gfx.paint(&mut c.view.surf, px, px, |cv| model::paint_count(cv, sz, n, since, t));
            if let Some(m) = self.pick.as_ref().map(|p| p.snip.mons[c.view.mon]) {
                let (x, y) = (m.rect.l + (m.rect.w() - px) / 2, m.rect.t + (m.rect.h() - px) / 2);
                let _ = c.view.surf.present(ui::panel::whwnd(c.view.hwnd), x, y, 255);
                gfx::commit();
                if !c.view.shown {
                    c.view.shown = true;
                    // SAFETY: showing our own window without activating it.
                    unsafe { show(c.view.hwnd, false) };
                }
            }
            busy |= t.saturating_sub(c.since) < 450;
        }
        crate::pacing::set(self.ctl as isize, busy);
    }

    unsafe fn timer(&mut self, id: usize) {
        let t = now();
        match id {
            T_COUNT => {
                if let Some(c) = &mut self.count {
                    if c.left.tick() {
                        KillTimer(self.ctl, T_COUNT);
                        return self.fire();
                    }
                    self.request();
                }
            }
            T_FLASH => {
                KillTimer(self.ctl, T_FLASH);
                if let Some(p) = &mut self.pick {
                    p.snip.close(t);
                }
                SetTimer(self.ctl, T_END, 190, None);
                self.request();
            }
            T_END => {
                KillTimer(self.ctl, T_END);
                self.pick = None;
                gfx::trim();
            }
            T_REC => self.rec_tick(t),
            T_RECEND => {
                KillTimer(self.ctl, T_RECEND);
                KillTimer(self.ctl, T_REC);
                self.rec = None;
                gfx::trim();
            }
            _ => {}
        }
        if self.pick.is_none() && self.count.is_none() {
            crate::pacing::set(self.ctl as isize, false); // the control repaints on its own timer, not on the screen's frames
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the picker's input

    /// The control under a point (the layout of the monitor the point is on).
    fn hit_at(&mut self, pt: (i32, i32)) -> Hit {
        let Some(p) = &mut self.pick else { return Hit::Canvas };
        let mi = model::mon_at(&p.snip.mons, pt);
        let m = p.snip.mons[mi];
        let Some(vi) = p.views.iter().position(|v| v.mon == mi) else { return Hit::Canvas };
        let g = &mut p.views[vi].gfx;
        let lay = p.snip.layout(mi, &mut |s, f| g.measure(s, f));
        lay.hit((pt.0 - m.rect.l) as f32 / m.scale, (pt.1 - m.rect.t) as f32 / m.scale)
    }

    unsafe fn on_move(&mut self, pt: (i32, i32)) {
        let hit = self.hit_at(pt);
        let t = now();
        let Some(p) = &mut self.pick else { return };
        if p.locked() {
            return;
        }
        p.snip.point_at(if let Hit::Ctl(c) = hit { Some(c) } else { None }, t);
        p.snip.move_to(pt, t);
        CURSOR.set(cursor_for(&p.snip, pt, hit));
        self.request();
    }

    unsafe fn on_press(&mut self, pt: (i32, i32)) {
        let hit = self.hit_at(pt);
        let t = now();
        let Some(p) = &mut self.pick else { return };
        if p.locked() {
            return;
        }
        let double = matches!(hit, Hit::Canvas)
            && t.saturating_sub(p.last_click.0) < 500
            && (pt.0 - p.last_click.1.0).abs() < 5
            && (pt.1 - p.last_click.1.1).abs() < 5
            && p.snip.editable()
            && p.snip.target().is_some_and(|s| s.contains(pt));
        p.last_click = (t, pt);
        let act = if double { Act::Capture { copy_only: false } } else { p.snip.press(pt, hit, t) };
        self.request();
        self.apply(act);
    }

    unsafe fn on_release(&mut self) {
        if let Some(p) = self.pick.as_mut().filter(|p| !p.locked()) {
            p.snip.release(now());
            self.request();
        }
    }

    unsafe fn on_key(&mut self, vk: u32) {
        let held = |k: u16| GetKeyState(i32::from(k)) < 0;
        if held(VK_CONTROL) || held(VK_MENU) || held(VK_LWIN) || held(VK_RWIN) {
            return;
        }
        let key = match vk {
            0x1B => Key::Esc,
            0x0D => Key::Enter,
            0x41..=0x5A => Key::Char(vk as u8 as char),
            _ => return,
        };
        let Some(p) = &mut self.pick else { return };
        if p.busy || (p.locked() && key != Key::Esc) {
            return;
        }
        let act = p.snip.key(key, now());
        self.request();
        self.apply(act);
    }

    unsafe fn apply(&mut self, act: Act) {
        match act {
            Act::None => {}
            Act::Cancel => self.command(Cmd::Cancel),
            Act::Capture { copy_only } => self.capture(copy_only),
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // taking it

    unsafe fn capture(&mut self, copy_only: bool) {
        let Some(p) = &mut self.pick else { return };
        let Some(sel) = p.snip.target() else { return };
        let opts = p.snip.opts;
        save_opts(&opts);
        p.snip.opts_open = false;
        if opts.kind == Kind::Rec {
            let sel = sel.even();
            return if opts.wait() > 0 { self.begin_count(opts.wait(), Then::Rec(sel)) } else { self.start_rec(sel) };
        }
        if copy_only || opts.wait() == 0 {
            self.shoot(sel, copy_only);
        } else {
            self.begin_count(opts.wait(), Then::Shot(sel, copy_only));
        }
    }

    /// Takes `sel` from the frozen picture and plays the flash.
    unsafe fn shoot(&mut self, sel: IRect, copy_only: bool) {
        let Some(p) = &mut self.pick else { return };
        let mi = model::mon_at(&p.snip.mons, (sel.l, sel.t));
        let mon = p.snip.mons[mi].rect;
        let Some(sel) = sel.intersect(mon) else { return };
        let (px, w, h) = crop(&p.frozen[mi], mon, sel);
        deliver(px, (w, h), p.snip.opts, copy_only);
        p.busy = true;
        p.snip.flash(now());
        SetTimer(self.ctl, T_FLASH, 330, None);
        self.request();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the countdown

    unsafe fn begin_count(&mut self, secs: u8, then: Then) {
        let Some(p) = &mut self.pick else { return };
        let at = match then {
            Then::Shot(r, _) | Then::Rec(r) => model::mon_at(&p.snip.mons, (r.l, r.t)),
        };
        let m = p.snip.mons[at];
        let Some(view) = View::new(m, at, "topbar_snip_count", 0, false) else { return };
        if matches!(then, Then::Shot(..)) {
            // a screenshot is taken of the live screen when the count ends: the picker steps aside, so the screen can be arranged
            for v in &p.views {
                ShowWindow(v.hwnd, SW_HIDE);
            }
            p.hidden = true;
        } else {
            // a recording starts from what is on the screen anyway (the picker is kept out of it): it stays, as in the mockup
            p.snip.counting = true;
            p.snip.dirty = true;
        }
        self.count = Some(Count { left: model::Countdown::new(secs), since: now(), then, view });
        COUNTING.store(true, Relaxed);
        SetTimer(self.ctl, T_COUNT, 1000, None);
        self.frame();
    }

    unsafe fn drop_count(&mut self) {
        KillTimer(self.ctl, T_COUNT);
        COUNTING.store(false, Relaxed);
        if self.count.take().is_some() {
            self.pick = None;
            gfx::trim();
        }
    }

    /// The countdown reached zero.
    unsafe fn fire(&mut self) {
        let Some(c) = self.count.take() else { return };
        COUNTING.store(false, Relaxed);
        let then = c.then;
        ShowWindow(c.view.hwnd, SW_HIDE);
        drop(c);
        DwmFlush();
        DwmFlush(); // the disc is off the screen before anything is taken from it
        match then {
            Then::Rec(sel) => self.start_rec(sel),
            Then::Shot(sel, copy_only) => {
                let Some(p) = &mut self.pick else { return };
                // what the screen is now, not what it was when the picker opened
                p.frozen = freeze(&p.snip.mons);
                for v in &mut p.views {
                    v.pic = None;
                }
                p.hidden = false;
                p.busy = false;
                self.shoot(sel, copy_only);
                self.frame();
                if let Some(p) = &self.pick {
                    for v in &p.views {
                        show(v.hwnd, false);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the recording

    unsafe fn start_rec(&mut self, area: IRect) {
        if self.rec.is_some() {
            return self.end_pick(false);
        }
        let Some(p) = &mut self.pick else { return };
        let mi = model::mon_at(&p.snip.mons, (area.l, area.t));
        let (m, opts) = (p.snip.mons[mi], p.snip.opts);
        let Some(area) = area.intersect(m.rect).map(IRect::even) else { return self.end_pick(false) };
        let thumb = {
            let (px, w, h) = crop(&p.frozen[mi], m.rect, area);
            let path = temp_png("thumb");
            let bmp = bitmap_of(&px, w, h);
            let ok = !bmp.is_null() && shot::write_png(bmp, &path);
            if !bmp.is_null() {
                DeleteObject(bmp);
            }
            ok.then(|| path.to_string_lossy().into_owned())
        };
        // Recorder::start waits for the microphone and the first picture: not on the overlay's thread, or a stuck device would freeze
        // the picker (Esc included). It stays up, locked, until the recording is there; Esc throws the start away.
        p.snip.counting = true;
        p.snip.dirty = true;
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        self.starting = Some(cancelled.clone());
        let options = rec::RecOptions { area: (area.l, area.t, area.r, area.b), mic: opts.mic, out: rec::default_path(), fps: 30 };
        let out = options.out.clone();
        let spawned = thread::Builder::new().name("rec-start".into()).spawn(move || {
            let recorder = rec::Recorder::start(options);
            send(Cmd::Run(Box::new(move |h| unsafe { h.rec_started(recorder, out, cancelled, (area, m, opts.mic, opts.notify), thumb) })));
        });
        if spawned.is_err() {
            self.starting = None;
            outcome(Outcome::RecFailed("Cannot start the recording thread".into()));
            self.end_pick(false);
        }
        self.request();
    }

    /// The answer of the start thread: the recording is up (or not) and the picker goes.
    unsafe fn rec_started(&mut self, recorder: Result<rec::Recorder, String>, out: PathBuf, cancelled: std::sync::Arc<AtomicBool>, (area, m, mic, notify): (IRect, Mon, bool, bool), thumb: Option<String>) {
        self.starting = None;
        let recorder = match recorder {
            Ok(r) if !cancelled.load(Relaxed) => r,
            Ok(r) => {
                // Esc came first: end it and take its file away, off this thread
                thread::spawn(move || {
                    let _ = r.stop();
                    let _ = std::fs::remove_file(out);
                });
                return self.end_pick(false);
            }
            Err(e) => {
                if !cancelled.load(Relaxed) {
                    outcome(Outcome::RecFailed(e));
                }
                return self.end_pick(false);
            }
        };
        if !self.show_rec(Some(recorder), area, m, (mic, notify), thumb) {
            return self.end_pick(false);
        }
        self.end_pick(true);
    }

    /// The frame and the control of a recording that has begun (`recorder` is `None` only in the smoke test).
    unsafe fn show_rec(&mut self, recorder: Option<rec::Recorder>, area: IRect, m: Mon, (mic, notify): (bool, bool), thumb: Option<String>) -> bool {
        let mi = 0;
        let (Some(frame), Some(pill)) = (View::new(m, mi, "topbar_snip_frame", 0, false), View::new(m, mi, "topbar_snip_pill", WS_EX_NOACTIVATE, true)) else {
            if let Some(r) = recorder {
                let _ = r.stop();
            }
            return false;
        };
        let t = now();
        // SAFETY: a plain clock read.
        REC_SINCE.store(windows_sys::Win32::System::SystemInformation::GetTickCount64().max(1), Relaxed);
        self.rec = Some(Rec { recorder, area, mon: m, last: String::new(), frame, pill, mic, thumb, notify, since: t, leaving: None, level: [0.0; 4], hot: false, down: false, ticks: 0 });
        SetTimer(self.ctl, T_REC, 33, None);
        self.rec_tick(t);
        PostMessageW(BAR.load(Relaxed) as HWND, SNIP, 4, 0);
        true
    }

    unsafe fn stop_rec(&mut self) {
        let Some(r) = &mut self.rec else { return };
        if r.leaving.is_some() {
            return;
        }
        r.leaving = Some(now());
        REC_SINCE.store(0, Relaxed);
        PostMessageW(BAR.load(Relaxed) as HWND, SNIP, 4, 0);
        if let Some(recorder) = r.recorder.take() {
            let (thumb, notify) = (r.thumb.clone(), r.notify);
            thread::spawn(move || match recorder.stop_with_stats() {
                Ok((path, stats)) => {
                    let bytes = std::fs::metadata(&path).map_or(0, |m| m.len());
                    outcome(Outcome::Rec { path: path.to_string_lossy().into_owned(), secs: stats.duration.as_secs().max(1), bytes, thumb, notify });
                }
                Err(e) => outcome(Outcome::RecFailed(e)),
            });
        }
        SetTimer(self.ctl, T_RECEND, 190, None);
    }

    /// Paints the frame and the control (the clock of the recording, the meter) and puts them where they belong.
    fn rec_tick(&mut self, t: u64) {
        let Some(r) = &mut self.rec else { return };
        let (m, k) = (r.mon, r.mon.scale);
        let text = rec_time().unwrap_or_else(|| r.last.clone());
        r.last.clone_from(&text);
        let level = r.recorder.as_ref().map_or(0.0, rec::Recorder::mic_level);
        // the bars rise at once and fall by degrees
        let target = model::meter_bars(level, (t % 100_000) as f32 / 1000.0);
        for (l, to) in r.level.iter_mut().zip(target) {
            *l = if to > *l { to } else { *l * 0.8 };
        }
        let view = RecView {
            time: text,
            meter: r.mic.then_some(r.level),
            hot: f32::from(r.hot),
            down: r.down,
            since: r.since,
            leaving: r.leaving,
            window: false,
            full: r.area == m.rect,
        };
        // the frame: the recorded area and a margin, kept to the monitor
        let margin = (8.0 * k).round() as i32;
        let fr = IRect::new(r.area.l - margin, r.area.t - margin, r.area.r + margin, r.area.b + margin).intersect(m.rect).unwrap_or(m.rect);
        let area_dip = r.area.dip(fr, k);
        let _ = r.frame.gfx.paint(&mut r.frame.surf, fr.w(), fr.h(), |c| model::paint_rec_frame(c, area_dip, &view, t));
        let _ = r.frame.surf.present(ui::panel::whwnd(r.frame.hwnd), fr.l, fr.t, 255);
        // the control: a box with room for its shadow, cut to the control itself so the rest does not catch the mouse
        let g = &mut r.pill.gfx;
        let width = model::pill_width(&mut |s, f| g.measure(s, f), r.mic, &view.time);
        let (wd, ht) = (((width + 2.0 * PAD) * k).round() as i32, ((44.0 + 2.0 * PAD) * k).round() as i32);
        let top = model::pill_top(r.area.dip(m.rect, k), m.rect.h() as f32 / k, r.area == m.rect);
        let (x, y) = (m.rect.l + ((m.rect.w() - (width * k).round() as i32) / 2) - (PAD * k).round() as i32, m.rect.t + (top * k).round() as i32 - (PAD * k).round() as i32);
        let _ = r.pill.gfx.paint(&mut r.pill.surf, wd, ht, |c| model::paint_pill(c, (PAD, PAD), &view, t));
        let _ = r.pill.surf.present(ui::panel::whwnd(r.pill.hwnd), x, y, 255);
        let (px, py) = ((PAD * k).round() as i32, (PAD * k).round() as i32);
        gfx::shape(ui::panel::whwnd(r.pill.hwnd), Some(&[gfx::Rgn { x: px, y: py, w: (width * k).round() as i32, h: (44.0 * k).round() as i32, r: (22.0 * k).round() as i32 }]));
        gfx::commit();
        for v in [&mut r.frame, &mut r.pill] {
            if !v.shown {
                v.shown = true;
                // SAFETY: showing our own windows without activating them.
                unsafe { show(v.hwnd, false) };
            }
        }
        r.ticks += 1;
        if r.ticks % 2 == 0 && r.leaving.is_none() {
            // SAFETY: a message to the bar's window (its time and its pulsing dot repaint)
            unsafe { PostMessageW(BAR.load(Relaxed) as HWND, SNIP, 4, 0) };
        }
    }

    /// The pointer over the recording control (client coordinates of its window, device pixels).
    unsafe fn pill_pointer(&mut self, x: i32, y: i32, down: Option<bool>) -> bool {
        let Some(r) = &mut self.rec else { return false };
        let m = r.mon;
        let g = &mut r.pill.gfx;
        let s = model::stop_rect(&mut |t, f| g.measure(t, f), r.mic, &r.last);
        let (dx, dy) = (x as f32 / m.scale - PAD, y as f32 / m.scale - PAD);
        let over = s.contains(dx, dy);
        let was = r.hot;
        r.hot = over;
        let mut stop = false;
        match down {
            Some(true) => r.down = over,
            Some(false) => {
                stop = r.down && over;
                r.down = false;
            }
            None => {}
        }
        if was != r.hot || down.is_some() {
            let t = now();
            self.rec_tick(t);
        }
        stop
    }
}

fn cursor_for(s: &Snip, pt: (i32, i32), hit: Hit) -> usize {
    use model::Grip;
    let c = match hit {
        Hit::Canvas => match s.opts.mode {
            Mode::Region => match s.grip_at(pt).filter(|_| s.editable()) {
                Some(Grip::Move) => IDC_SIZEALL,
                Some(Grip::N | Grip::S) => IDC_SIZENS,
                Some(Grip::E | Grip::W) => IDC_SIZEWE,
                Some(Grip::Nw | Grip::Se) => IDC_SIZENWSE,
                Some(Grip::Ne | Grip::Sw) => IDC_SIZENESW,
                None => IDC_CROSS,
            },
            Mode::Window => IDC_HAND,
            Mode::Full => IDC_ARROW,
        },
        _ => IDC_ARROW,
    };
    c as usize
}

/// The picker's overlay windows, the countdown's disc, the frame and the control share one procedure.
unsafe extern "system" fn pick_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let client = || ((lp & 0xffff) as i16 as i32, ((lp >> 16) & 0xffff) as i16 as i32);
    match msg {
        WM_ERASEBKGND => 1,
        WM_SETCURSOR => {
            let c = CURSOR.get();
            let mine = with(|h| h.pick.as_ref().is_some_and(|p| p.views.iter().any(|v| v.hwnd == hwnd))).unwrap_or(false);
            if mine && c != 0 {
                SetCursor(LoadCursorW(null_mut(), c as *const u16));
            } else {
                SetCursor(LoadCursorW(null_mut(), IDC_ARROW));
            }
            1
        }
        WM_MOUSEACTIVATE => {
            let pill = with(|h| h.rec.as_ref().is_some_and(|r| r.pill.hwnd == hwnd)).unwrap_or(false);
            if pill { 3 } else { 1 } // MA_NOACTIVATE for the control, MA_ACTIVATE for the picker
        }
        WM_MOUSEMOVE => {
            let is_pill = with(|h| h.rec.as_ref().is_some_and(|r| r.pill.hwnd == hwnd)).unwrap_or(false);
            if is_pill {
                let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
                let (x, y) = client();
                with(|h| h.pill_pointer(x, y, None));
            } else {
                let p = cursor_pos();
                with(|h| h.on_move(p));
            }
            0
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            with(|h| h.pill_pointer(-10_000, -10_000, None));
            0
        }
        WM_LBUTTONDOWN => {
            let is_pill = with(|h| h.rec.as_ref().is_some_and(|r| r.pill.hwnd == hwnd)).unwrap_or(false);
            if is_pill {
                SetCapture(hwnd);
                let (x, y) = client();
                with(|h| h.pill_pointer(x, y, Some(true)));
            } else {
                SetCapture(hwnd);
                let p = cursor_pos();
                with(|h| h.on_press(p));
            }
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            let is_pill = with(|h| h.rec.as_ref().is_some_and(|r| r.pill.hwnd == hwnd)).unwrap_or(false);
            if is_pill {
                let (x, y) = client();
                if with(|h| h.pill_pointer(x, y, Some(false))).unwrap_or(false) {
                    stop_rec();
                }
            } else {
                with(|h| h.on_release());
            }
            0
        }
        WM_RBUTTONDOWN => {
            let mine = with(|h| h.pick.as_ref().is_some_and(|p| p.views.iter().any(|v| v.hwnd == hwnd))).unwrap_or(false);
            if mine {
                cancel();
            }
            0
        }
        WM_KEYDOWN => {
            with(|h| h.on_key(wp as u32));
            0
        }
        WM_ACTIVATE if wp & 0xffff == 0 => {
            // another program took the focus: a picker that cannot hear Esc any more goes away
            let other = lp as HWND;
            let lost = with(|h| h.pick.as_ref().is_some_and(|p| !p.hidden && !p.busy && p.views.iter().any(|v| v.hwnd == hwnd) && p.views.iter().all(|v| v.hwnd != other))).unwrap_or(false);
            if lost {
                cancel();
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the notifications of the outcomes (the bar's thread)

/// The bar's thread: tells the user what the capture thread finished.
pub(crate) fn tell() {
    for o in outcomes() {
        match o {
            Outcome::Shot { name, copied, size, preview, notify } => {
                if notify {
                    notify::screenshot_done(name.as_deref(), Some(size), copied, preview);
                }
            }
            Outcome::ShotFailed => notify::screenshot_failed(),
            Outcome::Rec { path, secs, bytes, thumb, notify } => {
                if notify {
                    notify::recording_saved(&path, secs, bytes, thumb);
                }
            }
            Outcome::RecFailed(e) => notify::recording_failed(&e),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `--render-panel shot|shot-window|shot-full|shot-opts|shot-count|rec-pick|rec-live`: the picker and the recording into PNGs

/// A desktop to work on: a bright wallpaper with three light windows, so the dimming of the picker shows (1:1 at `k`).
pub(crate) fn demo_screen(w: i32, h: i32, k: f32) -> Vec<u32> {
    let mut px: Vec<u32> = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
            let c = |a: f32, b: f32| (a + (b - a) * (0.55 * x + 0.45 * y)) as u32;
            0xff00_0000 | c(70.0, 196.0) << 16 | c(130.0, 110.0) << 8 | c(220.0, 214.0)
        })
        .collect();
    let mut rect = |x0: f32, y0: f32, x1: f32, y1: f32, col: u32| {
        for y in ((y0 * k) as i32).max(0)..((y1 * k) as i32).min(h) {
            for x in ((x0 * k) as i32).max(0)..((x1 * k) as i32).min(w) {
                px[(y * w + x) as usize] = col;
            }
        }
    };
    // back to front: the picker's list is topmost first
    for (n, &(_, x, y, ww, hh)) in DEMO_WINDOWS.iter().enumerate().rev() {
        rect(x, y, x + ww, y + hh, 0xfff4_f5f9);
        rect(x, y, x + ww, y + 36.0, 0xffdf_e3ec);
        rect(x + 14.0, y + 12.0, x + 60.0, y + 24.0, 0xffb4_bccd);
        let hero = [0xff4a_7be0_u32, 0xffe0_6a4a, 0xff3f_b07a][n % 3];
        rect(x + 24.0, y + 64.0, x + ww * 0.5, y + hh * 0.5, hero);
        rect(x + ww * 0.58, y + 64.0, x + ww - 24.0, y + 84.0, 0xffc9_cfdc);
        rect(x + ww * 0.58, y + 100.0, x + ww * 0.85, y + 116.0, 0xffdc_e0ea);
        rect(x + 24.0, y + hh * 0.62, x + ww - 24.0, y + hh * 0.62 + 18.0, 0xffd2_d7e3);
        rect(x + 24.0, y + hh * 0.62 + 34.0, x + ww * 0.7, y + hh * 0.62 + 50.0, 0xffe2_e6ef);
    }
    px
}

const DEMO_WINDOWS: [(&str, f32, f32, f32, f32); 3] = [("Terminal", 900.0, 60.0, 380.0, 300.0), ("File manager", 520.0, 240.0, 860.0, 560.0), ("Photo viewer", 60.0, 130.0, 760.0, 470.0)];

/// The demo's windows as a picker would list them (topmost first).
pub(crate) fn demo_windows(k: f32) -> Vec<Win> {
    DEMO_WINDOWS
        .into_iter()
        .map(|(t, x, y, w, h)| Win { rect: IRect::new((x * k) as i32, (y * k) as i32, ((x + w) * k) as i32, ((y + h) * k) as i32), title: t.into() })
        .collect()
}

/// Renders one picker state to `file` (device pixels, the monitor is 1440 x 900 DIPs at `scale`).
pub(crate) unsafe fn render(args: &[String]) {
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let (Some(file), Some(what)) = (args.first(), args.get(1)) else { return };
    let get = |k: &str| args.iter().skip(2).find_map(|a| a.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
    let scale: f32 = get("scale").and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let t: u64 = get("t").and_then(|v| v.parse().ok()).unwrap_or(5000);
    let lang = get("lang").unwrap_or("en");
    i18n::set_lang(i18n::Lang::parse(lang));
    i18n::style::set_accent(i18n::style::Accent::parse(get("accent").unwrap_or("blue")));
    let (w, h) = ((1440.0 * scale).round() as i32, (900.0 * scale).round() as i32);
    let mon = Mon { rect: IRect::new(0, 0, w, h), scale };
    let mut gfx = Gfx::new(scale).expect("the renderer");
    let screen = demo_screen(w, h, scale);
    let base = 1_000_000u64;
    let mut surf = Surface::new();
    let flag = |n: &str| args.iter().skip(2).any(|a| a == n);
    let region = |x: f32, y: f32, ww: f32, hh: f32| IRect::new((x * scale) as i32, (y * scale) as i32, ((x + ww) * scale) as i32, ((y + hh) * scale) as i32);
    let mut opts = Opts { kind: if flag("rec") || what.starts_with("rec") { Kind::Rec } else { Kind::Shot }, ..Opts::default() };
    for a in args.iter().skip(2) {
        match a.as_str() {
            "window" => opts.mode = Mode::Window,
            "full" => opts.mode = Mode::Full,
            "noclip" => opts.clip = false,
            "nomic" => opts.mic = false,
            "delay" => opts.delay = 3,
            _ => {}
        }
    }
    let mut snip = Snip::new(vec![mon], demo_windows(scale), opts, 0, base);
    snip.opts.mode = opts.mode;
    let pic_px = screen.clone();
    let mut pic: Option<Picture> = None;
    let present = |gfx: &mut Gfx, surf: &mut Surface, snip: &Snip, t: u64, pic: &mut Option<Picture>| {
        let _ = gfx.paint(surf, w, h, |c| {
            if pic.is_none() {
                *pic = c.pixels(&pic_px, w, h);
            }
            snip.paint(c, 0, pic.as_ref(), t);
        });
    };
    let mut out: Vec<u32>;
    let mut size = (w, h);
    match what.as_str() {
        "shot" | "shot-window" | "shot-full" | "shot-opts" | "rec-pick" => {
            match what.as_str() {
                "shot-window" => {
                    snip.set_mode(Mode::Window, base);
                    snip.move_to(((get("x").and_then(|v| v.parse::<f32>().ok()).unwrap_or(620.0) * scale) as i32, (get("y").and_then(|v| v.parse::<f32>().ok()).unwrap_or(340.0) * scale) as i32), base);
                }
                "shot-full" => snip.set_mode(Mode::Full, base),
                _ => {
                    if !flag("empty") {
                        let (x, y, ww, hh) = (452.0, 188.0, 520.0, 330.0);
                        let r = if what == "rec-pick" { region(300.0, 120.0, 840.0, 560.0) } else { region(x, y, ww, hh) };
                        let p = (r.l, r.t);
                        snip.press(p, Hit::Canvas, base);
                        snip.move_to((r.r, r.b), base + 1);
                        snip.release(base + 2);
                    } else if let Some(p) = get("cur").and_then(|v| v.split_once(',')).and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))) {
                        snip.move_to(((p.0 * scale) as i32, (p.1 * scale) as i32), base);
                    }
                    if what == "shot-opts" {
                        snip.press((0, 0), Hit::Ctl(Ctl::Gear), base + 3);
                    }
                }
            }
            if let Some(i) = get("hover").and_then(|v| v.parse::<usize>().ok()) {
                let c = [Ctl::Kind(Kind::Shot), Ctl::Kind(Kind::Rec), Ctl::Mode(Mode::Region), Ctl::Mode(Mode::Window), Ctl::Mode(Mode::Full), Ctl::Gear, Ctl::Close, Ctl::Cancel, Ctl::Copy, Ctl::Go][i.min(9)];
                snip.point_at(Some(c), base + 3);
            }
            if flag("flash") {
                snip.flash(base + 4);
            }
            if flag("rec-kind") {
                snip.set_kind(Kind::Rec, base + 4);
            }
            present(&mut gfx, &mut surf, &snip, base + t, &mut pic);
            out = surf.pixels().to_vec();
            if let Some(f) = get("dump") {
                let lay = snip.layout(0, &mut |s, f| gfx.measure(s, f));
                let text: String = lay.items.iter().map(|(c, r)| format!("{c:?} {:.1},{:.1},{:.1},{:.1}\n", r.x, r.y, r.w, r.h)).chain([format!("tool {:?}\npop {:?}\nbar {:?}\nlabel {:?}\n", lay.tool, lay.pop, lay.bar, lay.label)]).collect();
                let _ = std::fs::write(f, text);
            }
        }
        "shot-count" => {
            // what the screen shows during a countdown: the desktop and the disc
            out = screen.clone();
            let sz = 180.0;
            let px = (sz * scale).round() as i32;
            let mut disc = Surface::new();
            let _ = gfx.paint(&mut disc, px, px, |c| model::paint_count(c, sz, get("n").and_then(|v| v.parse().ok()).unwrap_or(3), base, base + t.min(5000)));
            crate::render::blend(&mut out, (w, h), disc.pixels(), px, ((w - px) / 2, (h - px) / 2), 255);
        }
        "rec-live" => {
            out = screen.clone();
            let area = region(get("ax").and_then(|v| v.parse().ok()).unwrap_or(300.0), get("ay").and_then(|v| v.parse().ok()).unwrap_or(120.0), 840.0, 560.0);
            let secs: u64 = get("secs").and_then(|v| v.parse().ok()).unwrap_or(12);
            let view = RecView { time: model::clock_text(secs), meter: (!flag("nomic")).then(|| model::meter_bars(get("level").and_then(|v| v.parse().ok()).unwrap_or(0.6), 0.3)), hot: f32::from(flag("hot")), down: false, since: base, leaving: None, window: flag("window"), full: false };
            let m = Mon { rect: IRect::new(0, 0, w, h), scale };
            let margin = (8.0 * scale).round() as i32;
            let fr = IRect::new(area.l - margin, area.t - margin, area.r + margin, area.b + margin);
            let mut s1 = Surface::new();
            let ad = area.dip(fr, scale);
            let _ = gfx.paint(&mut s1, fr.w(), fr.h(), |c| model::paint_rec_frame(c, ad, &view, base + t));
            crate::render::blend(&mut out, (w, h), s1.pixels(), fr.w(), (fr.l, fr.t), 255);
            let width = model::pill_width(&mut |s, f| gfx.measure(s, f), !flag("nomic"), "0:00");
            let (wd, ht) = (((width + 2.0 * PAD) * scale).round() as i32, ((44.0 + 2.0 * PAD) * scale).round() as i32);
            let top = model::pill_top(area.dip(m.rect, scale), 900.0, false);
            let mut s2 = Surface::new();
            let _ = gfx.paint(&mut s2, wd, ht, |c| model::paint_pill(c, (PAD, PAD), &view, base + t));
            let x = ((1440.0 - width) / 2.0 * scale).round() as i32 - (PAD * scale).round() as i32;
            let y = (top * scale).round() as i32 - (PAD * scale).round() as i32;
            crate::render::blend(&mut out, (w, h), s2.pixels(), wd, (x, y), 255);
        }
        _ => return,
    }
    if let Some(rect) = get("crop").and_then(|v| {
        let n: Vec<f32> = v.split(',').filter_map(|p| p.parse().ok()).collect();
        (n.len() == 4).then(|| ((n[0] * scale) as i32, (n[1] * scale) as i32, (n[2] * scale) as i32, (n[3] * scale) as i32))
    }) {
        let (x0, y0, x1, y1) = (rect.0.max(0), rect.1.max(0), rect.2.min(w), rect.3.min(h));
        let mut c = Vec::new();
        for y in y0..y1 {
            c.extend_from_slice(&out[(y * w + x0) as usize..(y * w + x1) as usize]);
        }
        size = (x1 - x0, y1 - y0);
        out = c;
    }
    if gfx.save_png(file, size.0 as u32, size.1 as u32, &out).is_err() {
        let _ = std::fs::write(format!("{file}.err.txt"), "could not write the png");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// `--render-panel shot-live <report.txt>`: the real windows on a stage

/// The pixel at a screen point as (r, g, b).
unsafe fn pixel(x: i32, y: i32) -> (i32, i32, i32) {
    let dc = GetDC(null_mut());
    let c = GetPixel(dc, x, y);
    ReleaseDC(null_mut(), dc);
    ((c & 255) as i32, ((c >> 8) & 255) as i32, ((c >> 16) & 255) as i32)
}

fn near(a: (i32, i32, i32), b: (i32, i32, i32), tol: i32) -> bool {
    (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol && (a.2 - b.2).abs() <= tol
}

/// Opens the picker on a fake 480 x 300 desktop in the top left corner of the screen (never active, each phase gone within two
/// seconds), drives it with the model's own input handlers and reads pixels back: that the thread starts, the composed windows
/// show what was painted, a selection brightens its inside, the recording's frame and control come up where they should, and that
/// the thread ends when everything is closed. Nothing is captured, saved or put on the clipboard, and no input is sent.
pub(crate) unsafe fn smoke(args: &[String]) {
    use std::sync::mpsc;
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    SetThreadDpiAwarenessContext(-4isize as _);
    let mut report = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| report.push(format!("{} {name}: {detail}", if ok { "ok  " } else { "FAIL" }));
    let mon = Mon { rect: IRect::new(0, 0, 480, 300), scale: 1.0 };
    let (red, blue) = ((200, 60, 60), (60, 60, 200));
    let frozen: Vec<u32> = (0..480 * 300).map(|i| if i % 480 < 240 { 0xff00_0000 | (200 << 16) | (60 << 8) | 60 } else { 0xff00_0000 | (60 << 16) | (60 << 8) | 200 }).collect();
    STAGED.store(true, Relaxed);
    *STAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Stage { mons: vec![mon], wins: Vec::new(), frozen: vec![frozen] });
    open(Kind::Shot);
    thread::sleep(Duration::from_millis(900));
    let class = wide("topbar_snip");
    check("the picker window exists", !FindWindowW(class.as_ptr(), null()).is_null(), String::new());
    // the scrim over the picture: .5 of (5, 7, 12) over the colour
    let dimmed = |c: (i32, i32, i32)| ((c.0 + 5) / 2, (c.1 + 7) / 2, (c.2 + 12) / 2);
    let (l, r) = (pixel(40, 200), pixel(440, 200));
    check("the frozen picture shows, dimmed", near(l, dimmed(red), 8) && near(r, dimmed(blue), 8), format!("left {l:?} want {:?}, right {r:?} want {:?}", dimmed(red), dimmed(blue)));
    let bar = pixel(388, 40); // glass between the mode buttons and the gear: the 480-wide stage is narrower than the toolbar, which then keeps its right end on it
    check("the toolbar is drawn over it", bar.0 < 60 && bar.1 < 60 && bar.2 < 70, format!("{bar:?}"));
    // drag a selection through the handlers
    let (tx, rx) = mpsc::channel();
    send(Cmd::Run(Box::new(move |h| unsafe {
        h.on_move((100, 100));
        h.on_press((100, 100));
        h.on_move((300, 220));
        h.on_release();
        let ready = h.pick.as_ref().map(|p| (p.snip.ready(), p.snip.target()));
        let lay = h.pick.as_mut().map(|p| {
            let g = &mut p.views[0].gfx;
            p.snip.layout(0, &mut |t, f| g.measure(t, f))
        });
        let _ = tx.send((ready, lay.and_then(|l| l.rect_of(Ctl::Go))));
    })));
    thread::sleep(Duration::from_millis(500));
    let (ready, go) = rx.recv_timeout(Duration::from_secs(2)).unwrap_or((None, None));
    check("the drag made a selection", ready == Some((true, Some(IRect::new(100, 100, 300, 220)))), format!("{ready:?}"));
    let inside = pixel(150, 160);
    check("its inside is not dimmed", near(inside, red, 6), format!("{inside:?} want {red:?}"));
    if let Some(g) = go {
        let at = pixel(g.x as i32 + 4, (g.y + g.h / 2.0) as i32);
        check("the Done button is in the accent colour", at.2 > at.0 + 40, format!("{at:?} at {:.0},{:.0}", g.x, g.y));
    } else {
        check("the Done button has a place", false, "no layout".into());
    }
    cancel();
    thread::sleep(Duration::from_millis(500));
    check("the picker is gone", FindWindowW(class.as_ptr(), null()).is_null(), String::new());
    // the recording's frame and control, without a recorder
    let area = IRect::new(60, 60, 420, 240);
    let (tx, rx) = mpsc::channel();
    send(Cmd::Run(Box::new(move |h| unsafe {
        let _ = tx.send(h.show_rec(None, area, mon, (true, false), None));
    })));
    thread::sleep(Duration::from_millis(700));
    check("the recording windows came up", rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false), String::new());
    let ring = pixel(58, 150);
    check("the frame's ring is red just outside the area", near(ring, (229, 72, 77), 24), format!("{ring:?}"));
    // the control: centred at the top, its Stop button is red
    let mut gfx = Gfx::new(1.0).expect("the renderer");
    let width = model::pill_width(&mut |s, f| gfx.measure(s, f), true, "0:00");
    let stop = model::stop_rect(&mut |s, f| gfx.measure(s, f), true, "0:00");
    let x0 = ((480.0 - width) / 2.0).round();
    let at = pixel((x0 + stop.x + 3.0) as i32, (16.0 + stop.cy()) as i32);
    check("the control's Stop button is red", near(at, (229, 72, 77), 24), format!("{at:?} at {:.0},{:.0}", x0 + stop.x + 3.0, 16.0 + stop.cy()));
    check("a recording shows on the bar's clock source", recording(), String::new());
    send(Cmd::StopRec);
    thread::sleep(Duration::from_millis(500));
    check("the recording state is cleared", !recording(), String::new());
    check("the control is gone", FindWindowW(wide("topbar_snip_pill").as_ptr(), null()).is_null(), String::new());
    thread::sleep(Duration::from_millis(100));
    check("the thread has ended", LINK.lock().unwrap_or_else(|e| e.into_inner()).hwnd == 0, String::new());
    STAGED.store(false, Relaxed);
    let text = report.join("\n");
    println!("{text}");
    if let Some(f) = args.first() {
        let _ = std::fs::write(f, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_takes_the_rows_and_columns_asked_for() {
        let px: Vec<u32> = (0..8 * 6).collect();
        let (c, w, h) = crop(&px, IRect::new(0, 0, 8, 6), IRect::new(2, 1, 5, 4));
        assert_eq!((w, h, c.len()), (3, 3, 9));
        assert_eq!((c[0], c[2], c[8]), (8 + 2, 8 + 4, 3 * 8 + 4));
        // a monitor that is not at the origin: the crop is in desktop coordinates
        let (c, _, _) = crop(&px, IRect::new(-8, 0, 0, 6), IRect::new(-6, 2, -4, 3));
        assert_eq!(c, vec![2 * 8 + 2, 2 * 8 + 3]);
    }

    #[test]
    fn the_demo_desktop_lists_its_windows_topmost_first() {
        let wins = demo_windows(1.0);
        assert_eq!(wins[0].title, "Terminal");
        assert_eq!(model::window_at(&wins, (950, 100)), Some(0));
        assert_eq!(model::window_at(&wins, (600, 300)), Some(1));
    }
}
