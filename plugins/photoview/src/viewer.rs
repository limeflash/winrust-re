//! The photo viewer's window: the state of what is open, and how the mockup (PhotoViewer.html) draws it. It is an [`App`] of
//! `winrust-ui`: it never sees a window, so `--render`, `--bench` and the tests run it with none.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
    time::SystemTime,
};

use winrust_ui::{
    anim::{self, CubicBezier, EASE, LINEAR, SPRING},
    core::App,
    frame::{Button, Cursor, Cx, Frame, Id, Mods},
    gfx::{Align, Canvas, Font, Picture, Rect, Rgba, Sample, Xf},
    widgets::{
        button::{self, Kind},
        caption::{Caption, caption},
        look as ui_look,
        overlay::MenuItem,
    },
};

mod demo;
pub use demo::{STATES, demo};

use crate::{
    decode::{self},
    decoder::{Decoded, Decoder, Meta},
    folder::{Folder, Hidden, is_pic},
    look, shell,
    view::{Geo, Zoom, strip_layout, zoom_label},
};

// ids of the window's items (0 and up); `1` is what the host's self-test and benchmark open as the menu
pub const ID_MORE: Id = 1;
pub const ID_INFO: Id = 2;
pub const ID_DELETE: Id = 3;
pub const ID_ROTATE: Id = 4;
const ID_ZOOM_OUT: Id = 5;
const ID_ZOOM: Id = 6;
const ID_ZOOM_IN: Id = 7;
const ID_STAGE: Id = 10;
const ID_PREV: Id = 11;
const ID_NEXT: Id = 12;
const ID_BROWSE: Id = 13;
const ID_ERR_SHOW: Id = 14;
const ID_ERR_NEXT: Id = 15;
const ID_PANEL: Id = 16;
const ID_WALL: Id = 20;
const ID_SHOW: Id = 21;
const ID_SWATCH: Id = 30;
const ID_THUMB: Id = 1000;

// `Cx::menu`, `Cx::toast`: what comes back as `App::command`
const TAG_MENU: u32 = 1;
const TAG_UNDO: u32 = 2;

// what a worker thread posts (`WM_APP + n`)
const MSG_READY: u32 = 0x8000 + 1;
const MSG_COPIED: u32 = 0x8000 + 2;
const MSG_RECYCLE: u32 = 0x8000 + 3;

// the keys of the tweens (an item's own use its id)
const K_ZOOM: i32 = 9001;
const K_OX: i32 = 9002;
const K_OY: i32 = 9003;
const K_ROT: i32 = 9004;
const K_SLIDE: i32 = 9005; // and the next
const K_HUD: i32 = 9007; // and the next
const K_CARD: i32 = 9009;
const K_DROP: i32 = 9010;
const K_SCROLL: i32 = 9011;

const CAPTION: f32 = 48.0;
const INFO_W: f32 = 288.0;
const STRIP_H: f32 = 76.0;
/// The toast stays 4.2 s; a picture that was deleted goes to the Recycle Bin a moment after it has gone.
const TOAST_MS: u64 = 4200;
const HUD_MS: u64 = 1100;
const NEAR: usize = 1;

/// The picture put on the GPU, and what the window keeps of how it was made.
struct Gpu {
    pic: Picture,
    /// The small blurred copy behind the picture (its ambient light).
    ambient: Option<Picture>,
    /// Pixels (the picture's own, or less when it was cut down to what the GPU takes).
    w: u32,
    h: u32,
    meta: Meta,
    palette: [u32; 5],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Right,
    Left,
    Fade,
}

/// The mockup's `slide` animation: a picture arriving.
struct Slide {
    dir: Dir,
    shot: u32,
    fresh: bool,
}

struct Hud {
    text: String,
    shot: u32,
    fresh: bool,
    until: u64,
}

struct Pending {
    hidden: Hidden,
    /// When the toast is gone and the picture goes to the Recycle Bin.
    due: u64,
}

struct Drag {
    from: (f32, f32),
    zoom: Zoom,
}

/// What the picture on the stage is, or why there is none.
enum Cur {
    /// No file is open.
    None,
    /// The file is being decoded.
    Loading,
    Ready,
    Failed,
}

type Bin = Arc<dyn Fn(&Hidden) -> Result<(), String> + Send + Sync>;

pub struct Viewer {
    /// Side effects on the user's machine (the Recycle Bin, the clipboard, the wallpaper, Explorer) happen only when this is set; a
    /// render, a benchmark and a test run with it off.
    live: bool,
    hwnd: Arc<AtomicIsize>,
    posted: Arc<AtomicBool>,
    dec: Decoder,
    folder: Option<Folder>,
    where_: String,
    /// Pictures finished off the screen thread, waiting for the next frame to put them on the GPU.
    inbox: Vec<(PathBuf, Decoded)>,
    gpu: HashMap<PathBuf, Gpu>,
    thumbs: HashMap<PathBuf, (Arc<crate::decoder::Thumb>, Picture)>,
    /// The picture on the stage; the new one's file may still be decoding while the old one stays.
    shown: Option<PathBuf>,
    /// Quarter turns the user gave each picture, in degrees (they add up: turning on goes on round, never back).
    rot: HashMap<PathBuf, i32>,
    zm: Option<Zoom>,
    info: bool,
    strip: bool,
    drop_on: bool,
    hud: Option<Hud>,
    hud_shots: u32,
    slide: Option<Slide>,
    slide_shots: u32,
    arrive: Dir,
    pending: Option<Pending>,
    bin: Bin,
    commits: Vec<std::thread::JoinHandle<()>>,
    failed: Arc<Mutex<Vec<(Hidden, String)>>>,
    drag: Option<Drag>,
    click_at: (f32, f32),
    info_scroll: f32,
    info_max: f32,
    win: (f32, f32),
    scale: f32,
    last_stage: (f32, f32),
    /// The next frame puts the picture where it belongs at once (a window resized, a picture that has just come, a drag).
    snap: bool,
    card_fresh: bool,
    card_for: Option<PathBuf>,
    drop_fresh: bool,
    wants: bool,
    /// What `--render` clicks first (the state of the design needs a menu or a toast open).
    demo_clicks: Vec<Id>,
    title_sent: String,
}

fn hex(c: u32) -> String {
    format!("#{:06x}", c & 0xff_ffff)
}

fn color(c: u32) -> Rgba {
    Rgba::new(((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, 1.0)
}

fn fmt_size(b: u64) -> String {
    if b >= 1_048_576 { format!("{:.1} MB", b as f64 / 1_048_576.0) } else { format!("{} KB", (b as f64 / 1024.0).round() as u64) }
}

/// `Today, 07:58`, or `Nov 12, 2026, 23:41`: the mockup's way to write when a file was changed.
fn fmt_modified(t: SystemTime) -> String {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let local = |t: SystemTime| -> Option<SYSTEMTIME> {
        let ticks = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_nanos() / 100 + 116_444_736_000_000_000;
        let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
        // SAFETY: plain conversions into locals.
        unsafe {
            let (mut utc, mut loc): (SYSTEMTIME, SYSTEMTIME) = (std::mem::zeroed(), std::mem::zeroed());
            (FileTimeToSystemTime(&ft, &mut utc) != 0 && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut loc) != 0).then_some(loc)
        }
    };
    let Some(m) = local(t) else { return "—".into() };
    let now = local(SystemTime::now());
    if now.is_some_and(|n| (n.wYear, n.wMonth, n.wDay) == (m.wYear, m.wMonth, m.wDay)) {
        return format!("Today, {:02}:{:02}", m.wHour, m.wMinute);
    }
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m.wMonth as usize).clamp(1, 12) - 1];
    format!("{month} {}, {}, {:02}:{:02}", m.wDay, m.wYear, m.wHour, m.wMinute)
}

fn kind_of(name: &str) -> String {
    let ext = name.rsplit('.').next().unwrap_or("").to_uppercase();
    match ext.as_str() {
        "JPG" | "JPEG" | "JFIF" => "JPEG image".into(),
        "PNG" => "PNG image".into(),
        "HEIC" => "HEIC image".into(),
        "WEBP" => "WebP image".into(),
        "GIF" => "GIF image".into(),
        _ => format!("{ext} file"),
    }
}

/// Why a picture cannot be shown, in the mockup's words.
fn error_text(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("heic" | "heif") => "HEIC pictures need the HEIF Image Extension from Microsoft Store. You can also convert it to JPEG on your phone.",
        Some("avif") => "AVIF pictures need the AV1 Video Extension from Microsoft Store.",
        _ => "This file is damaged or in a format the viewer doesn’t support.",
    }
}

/// An animation that is started at will: 0 to 1 over `dur`. The tween is put back to 0 first when `fresh`.
fn play(f: &mut Frame<'_>, key: i32, fresh: bool, dur: u32, ease: CubicBezier) -> f32 {
    if fresh {
        f.tween(key, 0, 0.0, 0, LINEAR);
    }
    f.tween(key, 0, 1.0, dur, ease)
}

impl Viewer {
    /// The viewer on `path` (a picture or a folder); no picture open when there is none. Decoding starts at once.
    pub fn open(path: Option<&Path>, live: bool) -> Self {
        let hwnd = Arc::new(AtomicIsize::new(0));
        let posted = Arc::new(AtomicBool::new(false));
        let dec = {
            let (hwnd, posted) = (hwnd.clone(), posted.clone());
            Decoder::new(move || {
                let h = hwnd.load(Ordering::Relaxed);
                // one message at a time: the window picks up everything that is ready when it answers
                if h != 0 && !posted.swap(true, Ordering::AcqRel) {
                    winrust_ui::window::post(h, MSG_READY, 0, 0);
                }
            })
        };
        let bin: Bin = if live { Arc::new(Folder::commit) } else { Arc::new(|_| Ok(())) };
        let mut v = Self {
            live,
            hwnd,
            posted,
            dec,
            folder: None,
            where_: String::new(),
            inbox: Vec::new(),
            gpu: HashMap::new(),
            thumbs: HashMap::new(),
            shown: None,
            rot: HashMap::new(),
            zm: None,
            info: true,
            strip: true,
            drop_on: false,
            hud: None,
            hud_shots: 0,
            slide: None,
            slide_shots: 0,
            arrive: Dir::Fade,
            pending: None,
            bin,
            commits: Vec::new(),
            failed: Arc::new(Mutex::new(Vec::new())),
            drag: None,
            click_at: (0.0, 0.0),
            info_scroll: 0.0,
            info_max: 0.0,
            win: (1160.0, 740.0),
            scale: 1.0,
            last_stage: (0.0, 0.0),
            snap: true,
            card_fresh: true,
            card_for: None,
            drop_fresh: true,
            wants: true,
            demo_clicks: Vec::new(),
            title_sent: String::new(),
        };
        if let Some(p) = path {
            v.set_folder(Folder::open(p));
        }
        v
    }

    fn set_folder(&mut self, f: Option<Folder>) {
        self.where_ = f.as_ref().and_then(|f| f.current()).and_then(|e| e.path.parent()).map(|p| p.display().to_string()).unwrap_or_default();
        self.folder = f;
        self.wants = true;
        self.want();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // what is open

    fn n(&self) -> usize {
        self.folder.as_ref().map_or(0, Folder::len)
    }

    fn index(&self) -> usize {
        self.folder.as_ref().map_or(0, Folder::index)
    }

    fn current(&self) -> Option<&crate::folder::Entry> {
        self.folder.as_ref().and_then(|f| f.current())
    }

    fn cur_state(&self) -> Cur {
        let Some(e) = self.current() else { return Cur::None };
        if self.gpu.contains_key(&e.path) {
            return Cur::Ready;
        }
        if self.dec.error(&e.path).is_some() { Cur::Failed } else { Cur::Loading }
    }

    /// The picture that is drawn: the current one when it is on the GPU, else the one drawn before (until the new one is there).
    fn shown_gpu(&self) -> Option<(&Path, &Gpu)> {
        let p = self.shown.as_deref()?;
        self.gpu.get(p).map(|g| (p, g))
    }

    fn rot_of(&self, p: &Path) -> i32 {
        self.rot.get(p).copied().unwrap_or(0)
    }

    /// The upright picture's size in DIPs at 100 % for the scale the window is drawn at.
    fn geo(&self) -> Option<Geo> {
        let (p, g) = self.shown_gpu()?;
        let (w, h) = (g.w as f32 / self.scale, g.h as f32 / self.scale);
        let pic = if self.rot_of(p).rem_euclid(180) == 90 { (h, w) } else { (w, h) };
        Some(Geo { stage: self.stage_size(), pic })
    }

    fn info_shown(&self) -> bool {
        self.info && self.n() > 0
    }

    fn strip_shown(&self) -> bool {
        self.strip && self.n() > 1
    }

    fn stage_size(&self) -> (f32, f32) {
        let (w, h) = self.win;
        (w - if self.info_shown() { INFO_W } else { 0.0 }, h - CAPTION - if self.strip_shown() { STRIP_H } else { 0.0 })
    }

    fn stage_rect(&self) -> Rect {
        let (w, h) = self.stage_size();
        Rect::new(0.0, CAPTION, w, h)
    }

    /// Tells the decoder what is wanted: the shown picture, those next to it, the thumbnails round it.
    fn want(&mut self) {
        let Some(f) = self.folder.as_ref() else { return };
        let Some(cur) = f.current() else { return };
        let (i, es) = (f.index(), f.entries());
        let near: Vec<PathBuf> = (1..=NEAR).flat_map(|d| [i.checked_sub(d), Some(i + d)]).flatten().filter_map(|j| es.get(j)).map(|e| e.path.clone()).collect();
        let half = (self.stage_size().0 / 2.0 / 40.0).ceil() as usize + 2;
        let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(es.len()));
        let mut strip: Vec<usize> = (lo..hi).collect();
        strip.sort_by_key(|j| j.abs_diff(i));
        let strip: Vec<PathBuf> = if self.strip_shown() { strip.into_iter().map(|j| es[j].path.clone()).collect() } else { Vec::new() };
        self.dec.want(&cur.path, &near, &strip);
        // the GPU keeps what is wanted and what is on the stage
        let keep: Vec<PathBuf> = std::iter::once(cur.path.clone()).chain(near).chain(self.shown.clone()).collect();
        self.gpu.retain(|p, _| keep.contains(p));
        self.thumbs.retain(|p, _| strip.contains(p));
        self.wants = false;
    }

    fn title_text(&self) -> String {
        self.current().map_or_else(|| "Photo viewer".into(), |e| format!("{} - Photo viewer", e.name))
    }

    fn sync_title(&mut self, cx: &mut Cx) {
        let t = self.title_text();
        if t != self.title_sent {
            cx.set_title(t.clone());
            self.title_sent = t;
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // doing things

    fn go(&mut self, to: usize, cx: &mut Cx) {
        let Some(f) = self.folder.as_mut() else { return };
        let from = f.index();
        f.go(to);
        let now = f.index();
        if now == from {
            return;
        }
        self.arrive = if now > from { Dir::Right } else { Dir::Left };
        self.arrived(cx);
    }

    /// Another picture is the current one: the zoom starts over, the decoder is told.
    fn arrived(&mut self, cx: &mut Cx) {
        self.zm = None;
        self.drag = None;
        self.info_scroll = 0.0;
        self.want();
        self.sync_title(cx);
        cx.invalidate();
    }

    fn hud(&mut self, text: String, cx: &mut Cx) {
        self.hud_shots += 1;
        self.hud = Some(Hud { text, shot: self.hud_shots, fresh: true, until: anim::now_ms() + HUD_MS });
        self.rearm(cx);
    }

    /// The one timer of the window is set for what is due first: the zoom hint's end, a deleted picture's turn to go.
    fn rearm(&mut self, cx: &mut Cx) {
        let now = anim::now_ms();
        let due = [self.hud.as_ref().map(|h| h.until), self.pending.as_ref().map(|p| p.due)].into_iter().flatten().min();
        if let Some(d) = due {
            cx.wake_in(d.saturating_sub(now).max(1) as u32);
        }
    }

    fn zoom_by(&mut self, k: f32, at: Option<(f32, f32)>, cx: &mut Cx) {
        let Some(geo) = self.geo() else { return };
        let now = geo.shown(self.zm).z;
        self.zoom_to(now * k, at, cx);
    }

    fn zoom_to(&mut self, to: f32, at: Option<(f32, f32)>, cx: &mut Cx) {
        let Some(geo) = self.geo() else { return };
        self.zm = geo.zoom_to(self.zm, to, at);
        let text = self.zm.map_or_else(|| "Fit".to_string(), |z| format!("{}%", (z.z * 100.0).round()));
        self.hud(text, cx);
        cx.invalidate();
    }

    fn toggle_fit(&mut self, at: Option<(f32, f32)>, cx: &mut Cx) {
        let Some(geo) = self.geo() else { return };
        if self.zm.is_some() || geo.fit() >= 1.0 {
            let was_fit = self.zm.is_none();
            self.zm = None;
            self.hud(if geo.fit() >= 1.0 && was_fit { "100%" } else { "Fit" }.into(), cx);
        } else {
            self.zoom_to(1.0, at, cx);
        }
        cx.invalidate();
    }

    fn rotate(&mut self, deg: i32, cx: &mut Cx) {
        let Some(p) = self.shown.clone().filter(|p| self.gpu.contains_key(p)) else { return };
        *self.rot.entry(p).or_insert(0) += deg;
        self.zm = None;
        cx.invalidate();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // deleting: the picture leaves the list, goes to the Recycle Bin when its toast is gone

    fn delete(&mut self, cx: &mut Cx) {
        if self.current().is_none() {
            return;
        }
        self.flush();
        let Some(h) = self.folder.as_mut().and_then(Folder::hide_current) else { return };
        self.dec.forget(&h.entry.path);
        self.gpu.remove(&h.entry.path);
        let name = h.entry.name.clone();
        self.pending = Some(Pending { hidden: h, due: anim::now_ms() + TOAST_MS + 200 });
        self.arrive = Dir::Fade;
        self.arrived(cx);
        cx.toast_at(format!("Moved {name} to the Recycle Bin"), Some(TAG_UNDO), if self.strip_shown() { 92.0 } else { 18.0 });
        self.rearm(cx);
    }

    /// The picture that was deleted goes to the Recycle Bin now (on a thread of its own: the shell takes its time).
    fn flush(&mut self) {
        let Some(p) = self.pending.take() else { return };
        let (bin, failed, hwnd) = (self.bin.clone(), self.failed.clone(), self.hwnd.clone());
        self.commits.retain(|c| !c.is_finished());
        self.commits.push(std::thread::spawn(move || {
            if let Err(e) = bin(&p.hidden) {
                failed.lock().unwrap().push((p.hidden, e));
                let h = hwnd.load(Ordering::Relaxed);
                if h != 0 {
                    winrust_ui::window::post(h, MSG_RECYCLE, 0, 0);
                }
            }
        }));
    }

    fn undo(&mut self, cx: &mut Cx) {
        let Some(p) = self.pending.take() else { return };
        if let Some(f) = self.folder.as_mut() {
            f.undo(p.hidden);
        }
        self.arrive = Dir::Fade;
        self.arrived(cx);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the menu and the buttons

    fn menu_items(&self) -> Vec<MenuItem> {
        vec![
            MenuItem::new("Set as wallpaper").icon(look::ICON_WALL),
            MenuItem::new("Copy picture").icon(look::ICON_COPY).kbd("Ctrl+C"),
            MenuItem::new("Copy path").icon(look::ICON_LINK),
            MenuItem::new("Show in folder").icon(look::ICON_FOLDER),
            MenuItem::sep(),
            MenuItem::new("Rotate left").icon(look::ICON_ROTATE_LEFT).kbd("Shift+R"),
            MenuItem::new("Move to Recycle Bin").icon(look::ICON_TRASH).kbd("Del"),
        ]
    }

    fn open_menu(&mut self, at: Option<(f32, f32)>, cx: &mut Cx) {
        if self.current().is_none() {
            return;
        }
        // the mockup's: right edge 98 DIPs from the window's, 46 down; a context menu stands where the pointer is
        let anchor = at.map_or(Rect::new(self.win.0 - 98.0 - 232.0, 46.0, 232.0, 0.0), |(x, y)| Rect::new(x, y, 0.0, 0.0));
        cx.menu(TAG_MENU, self.menu_items(), anchor);
    }

    fn set_wallpaper(&mut self, cx: &mut Cx) {
        let Some(p) = self.current().map(|e| e.path.clone()) else { return };
        let ok = !self.live || shell::set_wallpaper(&p);
        cx.toast_at(if ok { "Set as desktop wallpaper" } else { "Couldn’t set the wallpaper" }, None, self.toast_bottom());
    }

    fn show_in_folder(&mut self, cx: &mut Cx) {
        let Some(p) = self.current().map(|e| e.path.clone()) else { return };
        if self.live {
            shell::show_in_folder(&p);
        } else {
            cx.toast_at(format!("Opening {}", self.where_), None, self.toast_bottom());
        }
    }

    fn copy_path(&mut self, cx: &mut Cx) {
        let Some(p) = self.current().map(|e| e.path.display().to_string()) else { return };
        if self.live {
            cx.set_clipboard_text(p);
        }
        cx.toast_at("Path copied", None, self.toast_bottom());
    }

    fn copy_picture(&mut self, cx: &mut Cx) {
        let Some(p) = self.current().map(|e| e.path.clone()) else { return };
        if !self.live {
            return cx.toast_at("Picture copied", None, self.toast_bottom());
        }
        // the pixels were let go of when they went to the GPU: they are made again, off the window's thread
        let hwnd = self.hwnd.clone();
        std::thread::spawn(move || {
            decode::com_init();
            let ok = decode::decode_model(&p).is_ok_and(|d| shell::copy_picture(&d.px, d.w, d.h));
            winrust_ui::window::post(hwnd.load(Ordering::Relaxed), MSG_COPIED, usize::from(ok), 0);
        });
    }

    fn copy_color(&mut self, i: usize, cx: &mut Cx) {
        let Some(c) = self.shown_gpu().map(|(_, g)| g.palette[i.min(4)]) else { return };
        if self.live {
            cx.set_clipboard_text(hex(c));
        }
        cx.toast_at(format!("Copied {}", hex(c)), None, self.toast_bottom());
    }

    fn toast_bottom(&self) -> f32 {
        if self.strip_shown() { 92.0 } else { 18.0 }
    }

    fn open_path(&mut self, p: &Path, cx: &mut Cx) {
        let Some(f) = Folder::open(p) else {
            self.drop_on = false;
            return cx.toast_at("That isn’t a picture", None, self.toast_bottom());
        };
        self.flush();
        self.gpu.clear();
        self.thumbs.clear();
        self.inbox.clear();
        self.shown = None;
        self.rot.clear();
        self.drop_on = false;
        self.arrive = Dir::Fade;
        self.set_folder(Some(f));
        self.arrived(cx);
    }

    fn next(&mut self, d: isize, cx: &mut Cx) {
        let to = (self.index() as isize + d).clamp(0, self.n().max(1) as isize - 1) as usize;
        self.go(to, cx);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the GPU: what the decoder finished goes onto it, one frame at a time

    /// Puts what the workers finished on the GPU. The pixels go with it: the window keeps no copy, a 24 MP photo is 96 MB.
    fn absorb(&mut self, c: &mut Canvas<'_>) {
        let Some(f) = self.folder.as_ref() else { return };
        let (i, es) = (f.index(), f.entries());
        let mut wanted: Vec<&Path> = es.get(i).map(|e| e.path.as_path()).into_iter().collect();
        wanted.extend([i.checked_sub(1), Some(i + 1)].into_iter().flatten().filter_map(|j| es.get(j)).map(|e| e.path.as_path()));
        for p in &wanted {
            if let Some(d) = self.dec.take(p) {
                self.inbox.push((p.to_path_buf(), d));
            }
        }
        // a picture made on the WARP device the first frames use would be lost at the swap to the GPU
        if !winrust_ui::gfx::hardware_pending() {
            let any = !self.inbox.is_empty();
            for (p, d) in std::mem::take(&mut self.inbox) {
                if let Some(g) = upload(c, d) {
                    self.gpu.insert(p, g);
                }
            }
            if any {
                // the driver keeps scratch memory from the upload of a picture of 96 MB: let it go
                winrust_ui::gfx::trim();
            }
        }
        // a lost device took them: they are decoded again
        let stale: Vec<PathBuf> = self.gpu.iter().filter(|(_, g)| c.picture_stale(&g.pic)).map(|(p, _)| p.clone()).collect();
        for p in stale {
            self.gpu.remove(&p);
            self.dec.redo(&p);
        }
        let es = self.folder.as_ref().map(|f| f.entries()).unwrap_or_default();
        for e in es {
            if let Some(t) = self.dec.thumb(&e.path) {
                let fresh = self.thumbs.get(&e.path).is_none_or(|(old, pic)| !Arc::ptr_eq(old, &t) || c.picture_stale(pic));
                if fresh && let Some(pic) = c.pixels(&t.px, t.w as i32, t.h as i32) {
                    self.thumbs.insert(e.path.clone(), (t, pic));
                }
            }
        }
    }

    /// The current picture is on the GPU: it goes on the stage with its slide. A failed one clears the stage.
    fn settle(&mut self) {
        match (self.cur_state(), self.current().map(|e| e.path.clone())) {
            (Cur::Ready, Some(p)) if self.shown.as_ref() != Some(&p) => {
                self.shown = Some(p);
                self.snap = true;
                self.slide_shots += 1;
                self.slide = Some(Slide { dir: std::mem::replace(&mut self.arrive, Dir::Fade), shot: self.slide_shots, fresh: true });
            }
            (Cur::None | Cur::Failed, _) => self.shown = None,
            _ => {}
        }
        if self.wants {
            self.want();
        }
    }
}

/// The picture on the GPU, and the small blurred one that lights the stage behind it.
fn upload(c: &mut Canvas<'_>, d: Decoded) -> Option<Gpu> {
    let pic = c.pixels(&d.px, d.w as i32, d.h as i32)?;
    let (sw, sh, px) = &d.small;
    // the mockup's blur(70px) of a picture 120 % of a stage about 1400 DIPs wide: the small copy is blurred by the same share of itself
    let sigma = 70.0 * (*sw as f32 / c.scale()) / 1400.0;
    let ambient = c.blurred(px, *sw as i32, *sh as i32, sigma, 1.4);
    Some(Gpu { pic, ambient, w: d.w, h: d.h, meta: d.meta, palette: d.palette })
}

// ---------------------------------------------------------------------------------------------------------------------
// the App

impl App for Viewer {
    fn title(&self) -> String {
        self.title_text()
    }

    fn min_size(&self) -> (f32, f32) {
        (640.0, 440.0)
    }

    fn start(&mut self, hwnd: isize) {
        self.hwnd.store(hwnd, Ordering::Relaxed);
    }

    fn demo_clicks(&self) -> Vec<Id> {
        self.demo_clicks.clone()
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (w, h) = f.size;
        let scale = c.scale();
        if self.win != (w, h) || (self.scale - scale).abs() > 1e-3 {
            self.win = (w, h);
            self.scale = scale;
            self.snap = true;
            self.wants = true;
        }
        self.absorb(c);
        self.settle();

        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, look::WIN);
        let stage = self.stage_rect();
        if self.info_shown() {
            self.draw_info(c, f, Rect::new(w - INFO_W, CAPTION, INFO_W, h - CAPTION));
        }
        if self.strip_shown() {
            self.draw_strip(c, f, Rect::new(0.0, h - STRIP_H, stage.w, STRIP_H));
        }
        f.clipped(c, stage, |c, f| self.draw_stage(c, f, stage));
        self.draw_caption(c, f, w);
    }

    fn click(&mut self, id: Id, b: Button, x: f32, y: f32, cx: &mut Cx) {
        self.click_at = (x, y);
        match id {
            ID_MORE => self.open_menu(None, cx),
            ID_INFO => {
                self.info = !self.info;
                self.snap = true;
                self.wants = true;
            }
            ID_DELETE => self.delete(cx),
            ID_ROTATE => self.rotate(90, cx),
            ID_ZOOM_OUT => self.zoom_by(1.0 / 1.25, None, cx),
            ID_ZOOM_IN => self.zoom_by(1.25, None, cx),
            ID_ZOOM => self.toggle_fit(None, cx),
            ID_PREV => self.next(-1, cx),
            ID_NEXT | ID_ERR_NEXT => self.next(1, cx),
            ID_BROWSE => {
                let picked = if self.live {
                    shell::browse(cx.hwnd())
                } else {
                    demo::folder("pictures").map(|d| d.join("."))
                };
                if let Some(p) = picked {
                    self.open_path(&p, cx);
                }
            }
            ID_WALL => self.set_wallpaper(cx),
            ID_SHOW | ID_ERR_SHOW => self.show_in_folder(cx),
            ID_STAGE if b == Button::Right => self.open_menu(Some((x, y)), cx),
            i if (ID_SWATCH..ID_SWATCH + 5).contains(&i) => self.copy_color((i - ID_SWATCH) as usize, cx),
            i if i >= ID_THUMB => self.go((i - ID_THUMB) as usize, cx),
            _ => {}
        }
        self.sync_title(cx);
        cx.invalidate();
    }

    fn double(&mut self, id: Id, cx: &mut Cx) {
        if id == ID_STAGE {
            let st = self.stage_rect();
            let (x, y) = self.click_at;
            self.toggle_fit(Some((x - st.x, y - st.y)), cx);
        }
    }

    fn down(&mut self, id: Id, b: Button, x: f32, y: f32, _cx: &mut Cx) {
        if id == ID_STAGE && b == Button::Left
            && let (Some(z), Some(geo)) = (self.zm, self.geo())
        {
            self.drag = Some(Drag { from: (x, y), zoom: Zoom { z: z.z, ..geo.shown(self.zm) } });
        }
    }

    fn drag(&mut self, id: Id, x: f32, y: f32, cx: &mut Cx) {
        let (Some(d), Some(geo)) = (self.drag.as_ref(), self.geo()) else { return };
        if id != ID_STAGE {
            return;
        }
        let (ox, oy) = geo.clamp(d.zoom.z, d.zoom.ox + (x - d.from.0), d.zoom.oy + (y - d.from.1));
        self.zm = Some(Zoom { z: d.zoom.z, ox, oy });
        cx.cursor(Cursor::Move);
        cx.invalidate();
    }

    fn release(&mut self, _id: Id, cx: &mut Cx) {
        if self.drag.take().is_some() {
            cx.invalidate();
        }
    }

    fn wheel_at(&mut self, _id: Option<Id>, x: f32, y: f32, _dx: f32, dy: f32, _m: Mods, cx: &mut Cx) {
        if self.info_shown() && x >= self.win.0 - INFO_W && y >= CAPTION {
            self.info_scroll = (self.info_scroll - dy * 54.0).clamp(0.0, self.info_max);
            return cx.invalidate();
        }
        let st = self.stage_rect();
        if self.shown_gpu().is_some() && st.contains(x, y) {
            self.zoom_by(1.18_f32.powf(dy), Some((x - st.x, y - st.y)), cx);
        }
    }

    fn key(&mut self, vk: u32, m: Mods, cx: &mut Cx) -> bool {
        const BACK: u32 = 0x08;
        const ESC: u32 = 0x1B;
        const SPACE: u32 = 0x20;
        const PGUP: u32 = 0x21;
        const PGDN: u32 = 0x22;
        const END: u32 = 0x23;
        const HOME: u32 = 0x24;
        const LEFT: u32 = 0x25;
        const RIGHT: u32 = 0x27;
        const DEL: u32 = 0x2E;
        let key = |k: char| vk == k as u32;
        if m.ctrl {
            if key('C') {
                self.copy_picture(cx);
                return true;
            }
            return false;
        }
        match vk {
            ESC => {
                if self.zm.is_some() {
                    self.zm = None;
                    cx.invalidate();
                } else {
                    cx.close_window();
                }
            }
            RIGHT | SPACE | PGDN => self.next(1, cx),
            LEFT | BACK | PGUP => self.next(-1, cx),
            HOME => self.go(0, cx),
            END => self.go(self.n().saturating_sub(1), cx),
            DEL => self.delete(cx),
            0x31 | 0x61 => self.zoom_to(1.0, None, cx),
            0x30 | 0x60 => {
                self.zm = None;
                self.hud("Fit".into(), cx);
            }
            _ if key('F') => {
                self.zm = None;
                self.hud("Fit".into(), cx);
            }
            0xBB | 0x6B => self.zoom_by(1.25, None, cx),
            0xBD | 0x6D => self.zoom_by(1.0 / 1.25, None, cx),
            _ if key('R') => self.rotate(if m.shift { -90 } else { 90 }, cx),
            _ if key('I') => {
                self.info = !self.info;
                self.snap = true;
                self.wants = true;
            }
            _ => return false,
        }
        self.sync_title(cx);
        cx.invalidate();
        true
    }

    fn command(&mut self, tag: u32, choice: usize, cx: &mut Cx) {
        match (tag, choice) {
            (TAG_UNDO, _) => self.undo(cx),
            (TAG_MENU, 0) => self.set_wallpaper(cx),
            (TAG_MENU, 1) => self.copy_picture(cx),
            (TAG_MENU, 2) => self.copy_path(cx),
            (TAG_MENU, 3) => self.show_in_folder(cx),
            (TAG_MENU, 4) => self.rotate(-90, cx),
            (TAG_MENU, 5) => self.delete(cx),
            _ => {}
        }
        self.sync_title(cx);
        cx.invalidate();
    }

    fn message(&mut self, msg: u32, wp: usize, _lp: isize, cx: &mut Cx) {
        match msg {
            MSG_READY => self.posted.store(false, Ordering::Release),
            MSG_COPIED => cx.toast_at(if wp != 0 { "Picture copied" } else { "Couldn’t copy the picture" }, None, self.toast_bottom()),
            MSG_RECYCLE => {
                let failed = std::mem::take(&mut *self.failed.lock().unwrap());
                for (h, _) in failed {
                    let name = h.entry.name.clone();
                    if let Some(f) = self.folder.as_mut() {
                        f.put_back(h);
                    }
                    cx.toast_at(format!("Couldn’t move {name} to the Recycle Bin"), None, self.toast_bottom());
                }
                self.wants = true;
            }
            _ => {}
        }
        cx.invalidate();
    }

    fn wake(&mut self, cx: &mut Cx) {
        let now = anim::now_ms();
        if self.hud.as_ref().is_some_and(|h| now >= h.until) {
            self.hud = None;
        }
        if self.pending.as_ref().is_some_and(|p| now >= p.due) {
            self.flush();
        }
        self.rearm(cx);
        cx.invalidate();
    }

    fn drag_over(&mut self, paths: &[String], _x: f32, _y: f32) -> bool {
        let ok = paths.iter().any(|p| is_pic(Path::new(p)) || Path::new(p).is_dir());
        if ok && !self.drop_on {
            self.drop_fresh = true;
        }
        self.drop_on = ok;
        ok
    }

    fn drag_leave(&mut self, _cx: &mut Cx) {
        self.drop_on = false;
    }

    fn drop(&mut self, paths: Vec<String>, _x: f32, _y: f32, cx: &mut Cx) {
        self.drop_on = false;
        match paths.iter().map(Path::new).find(|p| is_pic(p) || p.is_dir()) {
            Some(p) => self.open_path(p, cx),
            None => cx.toast_at("That isn’t a picture", None, self.toast_bottom()),
        }
        self.sync_title(cx);
    }

    fn close(&mut self, _cx: &mut Cx) -> bool {
        // what was deleted goes to the Recycle Bin before the window is gone
        self.flush();
        for c in self.commits.drain(..) {
            let _ = c.join();
        }
        true
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// drawing: the caption

/// A square button of the caption (`.tb`): the icon, a plate that comes up under the pointer, `on` when it is pressed in.
#[allow(clippy::too_many_arguments)]
fn tool(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, icon: Option<&'static str>, tip: &'static str, on: bool, danger: bool) {
    let hot = f.focusable(id, r);
    f.hand();
    f.tip(tip);
    c.transformed(r.cx(), r.cy(), 1.0 - 0.08 * hot.press, 0.0, 0.0, |c| {
        let plate = if danger {
            Rgba::css(229, 72, 77, 0.18 * hot.hover)
        } else {
            Rgba::white(if on { 0.12_f32.max(0.09 * hot.hover) } else { 0.09 * hot.hover })
        };
        c.fill(r, 9.0, plate);
        let ink = if danger {
            ui_look::T2.mix(ui_look::RED, hot.hover)
        } else if on {
            ui_look::T1
        } else {
            ui_look::T2.mix(ui_look::T1, hot.hover)
        };
        match icon {
            Some(d) => c.icon(d, (r.cx(), r.cy()), 16.0, 1.9, ink, 0.0),
            // the three dots of "More": filled circles of 1.6 in a 24 box
            None => {
                for dx in [-4.33, 0.0, 4.33] {
                    c.circle(r.cx() + dx, r.cy(), 1.07, ink);
                }
            }
        }
    });
    if hot.focus {
        c.ring(r.inset(-3.0), 12.0, 2.0, ui_look::focus_ring().fade(0.9));
    }
}

impl Viewer {
    fn draw_caption(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32) {
        let n = self.n();
        let name = self.current().map_or_else(|| "Photo viewer".to_string(), |e| e.name.clone());
        let pos = if n > 0 { format!("{} of {n}", self.index() + 1) } else { String::new() };
        let has_pic = self.shown_gpu().is_some();
        let label = match self.geo() {
            Some(g) if has_pic => zoom_label(self.zm, g.fit()),
            _ => "—".to_string(),
        };
        let zf = Font::new(12.0, 600).tabular();
        let zc_w = (c.measure(&label, zf).ceil() + 16.0).max(58.0);
        let group_w = zc_w + 68.0;
        let tools_w = if n > 0 { group_w + 187.0 } else { 0.0 };
        let cap = Caption {
            bg: look::BAR,
            title: &name,
            sub: &pos,
            glyph: look::GLYPH,
            tile: [Rgba::hex(0xffb46b), Rgba::hex(0xe86a8e), Rgba::hex(0x7b6cf0)],
            max: f.maximized(),
            tools_w,
        };
        let tools = caption(c, f, Rect::new(0.0, 0.0, w, CAPTION), &cap);
        if n == 0 {
            return;
        }
        // the zoom group: - 100 % +, in a plate of its own
        let g = Rect::new(tools.x, 8.0, group_w, 32.0);
        c.fill(g, 10.0, Rgba::white(0.05));
        let small = |c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, icon: &'static str, tip: &'static str| {
            let hot = f.focusable(id, r);
            f.hand();
            f.tip(tip);
            c.transformed(r.cx(), r.cy(), 1.0 - 0.08 * hot.press, 0.0, 0.0, |c| {
                c.fill(r, 8.0, Rgba::white(0.09 * hot.hover));
                c.icon(icon, (r.cx(), r.cy()), 15.0, 2.0, ui_look::T2.mix(ui_look::T1, hot.hover), 0.0);
            });
            if hot.focus {
                c.ring(r.inset(-3.0), 11.0, 2.0, ui_look::focus_ring().fade(0.9));
            }
        };
        small(c, f, ID_ZOOM_OUT, Rect::new(g.x + 2.0, 10.0, 30.0, 28.0), look::ICON_MINUS, "Zoom out (−)");
        let zr = Rect::new(g.x + 34.0, 10.0, zc_w, 28.0);
        let hot = f.focusable(ID_ZOOM, zr);
        f.hand();
        f.tip("Fit ↔ 100% (F, 1)");
        c.fill(zr, 8.0, Rgba::white(0.1 * hot.hover));
        c.text(&label, zr, zf, ui_look::T1, Align::Center);
        if hot.focus {
            c.ring(zr.inset(-3.0), 11.0, 2.0, ui_look::focus_ring().fade(0.9));
        }
        small(c, f, ID_ZOOM_IN, Rect::new(zr.x + zr.w + 2.0, 10.0, 30.0, 28.0), look::ICON_PLUS, "Zoom in (+)");
        // a hairline, then the four buttons, 10 apart
        let sep = g.x + g.w + 10.0 + 4.0;
        c.fill(Rect::new(sep, 14.0, 1.0, 20.0), 0.0, Rgba::white(0.08));
        let x0 = sep + 1.0 + 4.0 + 10.0;
        let more = f.menu_open();
        tool(c, f, ID_ROTATE, Rect::new(x0, 8.0, 32.0, 32.0), Some(look::ICON_ROTATE), "Rotate (R)", false, false);
        tool(c, f, ID_INFO, Rect::new(x0 + 42.0, 8.0, 32.0, 32.0), Some(look::ICON_INFO), "Details (I)", self.info, false);
        tool(c, f, ID_DELETE, Rect::new(x0 + 84.0, 8.0, 32.0, 32.0), Some(look::ICON_TRASH), "Move to Recycle Bin (Del)", false, true);
        tool(c, f, ID_MORE, Rect::new(x0 + 126.0, 8.0, 32.0, 32.0), None, "More", more, false);
    }

    // -------------------------------------------------------------------------------------------------------------
    // the stage

    fn draw_stage(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, st: Rect) {
        c.fill(st, 0.0, look::STAGE);
        f.item(ID_STAGE, st);
        if self.zm.is_some() {
            f.hand();
        }
        let over = matches!(f.hover, Some(ID_STAGE | ID_PREV | ID_NEXT));
        let state = self.cur_state();
        let cur = self.current().map(|e| (e.path.clone(), e.name.clone()));
        if self.card_for != cur.as_ref().map(|c| c.0.clone()) {
            self.card_for = cur.as_ref().map(|c| c.0.clone());
            self.card_fresh = true;
        }
        self.draw_picture(c, f, st);
        match (&state, &cur) {
            (Cur::Failed, Some((_, name))) => self.draw_error(c, f, st, name),
            (Cur::None, _) if !self.drop_on => self.draw_empty(c, f, st),
            _ => {}
        }
        if self.shown_gpu().is_some() {
            self.draw_nav(c, f, st, over);
            self.draw_navigator(c, st);
        }
        self.draw_hud(c, f, st);
        if self.drop_on {
            self.draw_drop(c, f, st);
        }
    }

    fn draw_picture(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, st: Rect) {
        let (Some(geo), Some(p)) = (self.geo(), self.shown.clone()) else { return };
        let target = geo.shown(self.zm);
        let stage_changed = self.last_stage != (st.w, st.h);
        self.last_stage = (st.w, st.h);
        // a window being resized or a picture being dragged does not trail behind: only the user's own moves glide
        let dur = if self.snap || self.drag.is_some() || stage_changed { 0 } else { 340 };
        self.snap = false;
        let (z, ox, oy) = (f.tween(K_ZOOM, 0, target.z, dur, SPRING), f.tween(K_OX, 0, target.ox, dur, SPRING), f.tween(K_OY, 0, target.oy, dur, SPRING));
        let rot = f.tween(K_ROT, 0, self.rot_of(&p) as f32, dur, SPRING);
        // the slide: 0.42 s from the side (or a fade), and what makes the stage's frames while it runs
        let (t, dir) = match self.slide.as_mut() {
            Some(s) => {
                let t = play(f, K_SLIDE + (s.shot % 2) as i32, s.fresh, 420, SPRING);
                s.fresh = false;
                (t, s.dir)
            }
            None => (1.0, Dir::Fade),
        };
        if t >= 0.999 {
            self.slide = None;
        }
        let (dx, k) = match dir {
            Dir::Right => (36.0, 0.985),
            Dir::Left => (-36.0, 0.985),
            Dir::Fade => (0.0, 0.97),
        };
        let (dx, k) = (dx * (1.0 - t), k + (1.0 - k) * t);
        let Some(g) = self.gpu.get(&p) else { return };
        let s = self.scale;
        if let Some(a) = &g.ambient {
            // `left/top -10 %, 120 % x 120 %, object-fit: cover, opacity .32`
            let b = Rect::new(st.x - 0.1 * st.w, st.y - 0.1 * st.h, 1.2 * st.w, 1.2 * st.h);
            let m = (b.w / a.w).max(b.h / a.h);
            c.picture_in(a, Rect::new(b.cx() - a.w * m / 2.0, b.cy() - a.h * m / 2.0, a.w * m, a.h * m), 0.0, 0.32, Sample::Linear);
        }
        let (iw, ih) = (g.w as f32 / s * z, g.h as f32 / s * z);
        let r = Rect::new(st.cx() + ox - iw / 2.0, st.cy() + oy - ih / 2.0, iw, ih);
        let (sin, cos) = (rot.to_radians().sin().abs(), rot.to_radians().cos().abs());
        let (bw, bh) = (iw * cos + ih * sin, iw * sin + ih * cos);
        let bound = Rect::new(r.cx() - bw / 2.0, r.cy() - bh / 2.0, bw, bh);
        let sample = if z < 0.999 { Sample::Smooth } else if z >= 2.0 { Sample::Nearest } else { Sample::Linear };
        let covers = bound.x <= st.x && bound.y <= st.y && bound.x + bound.w >= st.x + st.w && bound.y + bound.h >= st.y + st.h;
        c.faded(t, |c| {
            c.transformed(st.cx(), st.cy(), k, dx, 0.0, |c| {
                if !covers {
                    c.shadow(bound, 3.0, 20.0, 60.0, Rgba::new(0.0, 0.0, 0.0, 0.45));
                }
                c.picture_in(&g.pic, r, rot, 1.0, sample);
            });
        });
    }

    /// The round arrows at the edges: they come up while the pointer is over the stage, and are not there at the ends of the list.
    fn draw_nav(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, st: Rect, over: bool) {
        let (i, n) = (self.index(), self.n());
        for (id, enabled, x, d) in [(ID_PREV, i > 0, st.x + 16.0, look::ICON_PREV), (ID_NEXT, i + 1 < n, st.x + st.w - 16.0 - 44.0, look::ICON_NEXT)] {
            let op = f.tween(id, 3, f32::from(over && enabled), 200, EASE);
            if !enabled {
                continue;
            }
            let r = Rect::new(x, st.cy() - 22.0, 44.0, 44.0);
            let hot = f.item(id, r);
            f.hand();
            if op <= 0.003 {
                continue;
            }
            c.faded(op, |c| {
                c.transformed(r.cx(), r.cy(), 1.0 - 0.08 * hot.press, 0.0, 0.0, |c| {
                    c.outside(r, 22.0, |c| c.shadow(r, 22.0, 8.0, 24.0, Rgba::new(0.0, 0.0, 0.0, 0.4)));
                    c.fill(r, 22.0, Rgba::css(20, 21, 26, 0.66).mix(Rgba::css(30, 31, 38, 0.86), hot.hover));
                    c.ring(r.inset(-1.0), 23.0, 1.0, Rgba::white(0.12));
                    c.icon(d, (r.cx(), r.cy()), 18.0, 2.4, Rgba::pure_white(1.0), 0.0);
                });
            });
        }
    }

    /// The small picture at the corner with the part in view outlined (when the picture is bigger than the stage).
    fn draw_navigator(&self, c: &mut Canvas<'_>, st: Rect) {
        let (Some(geo), Some((p, g))) = (self.geo(), self.shown_gpu()) else { return };
        let Some(n) = geo.navigator(self.zm, self.rot_of(p).rem_euclid(180) != 0) else { return };
        let r = Rect::new(st.x + st.w - 16.0 - n.w, st.y + st.h - 16.0 - n.h, n.w, n.h);
        c.outside(r, 8.0, |c| c.shadow(r, 8.0, 10.0, 30.0, Rgba::new(0.0, 0.0, 0.0, 0.5)));
        c.masked(r, 8.0, Xf::NONE, |c| {
            c.picture_in(&g.pic, r, 0.0, 0.75, Sample::Smooth);
            let v = Rect::new(r.x + n.x, r.y + n.y, n.vw, n.vh);
            // `box-shadow: 0 0 0 999px rgba(0,0,0,.45)` round the outline: the rest of the small picture is dimmed
            let dim = Rgba::new(0.0, 0.0, 0.0, 0.45);
            c.fill(Rect::new(r.x, r.y, r.w, v.y - r.y), 0.0, dim);
            c.fill(Rect::new(r.x, v.y + v.h, r.w, r.y + r.h - v.y - v.h), 0.0, dim);
            c.fill(Rect::new(r.x, v.y, v.x - r.x, v.h), 0.0, dim);
            c.fill(Rect::new(v.x + v.w, v.y, r.x + r.w - v.x - v.w, v.h), 0.0, dim);
            c.ring(v, 3.0, 1.5, Rgba::pure_white(1.0));
        });
        c.ring(r.inset(-1.0), 9.0, 1.0, Rgba::white(0.18));
    }

    /// The zoom in the middle of the stage: it comes in over 14 % of 1.1 s, stays, and goes.
    fn draw_hud(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, st: Rect) {
        let Some(h) = self.hud.as_mut() else { return };
        let t = play(f, K_HUD + (h.shot % 2) as i32, h.fresh, HUD_MS as u32, LINEAR);
        h.fresh = false;
        let (op, k) = if t < 0.14 {
            let e = EASE.ease(t / 0.14);
            (e, 0.92 + 0.08 * e)
        } else if t < 0.78 {
            (1.0, 1.0)
        } else {
            let e = EASE.ease(((t - 0.78) / 0.22).min(1.0));
            (1.0 - e, 1.0 - 0.02 * e)
        };
        let font = Font::new(15.0, 600).tabular();
        let tw = c.measure(&h.text, font).ceil();
        let r = Rect::new((st.cx() - (tw + 28.0) / 2.0).round(), (st.cy() - 17.0).round(), tw + 28.0, 34.0);
        let text = h.text.clone();
        c.faded(op, |c| {
            c.transformed(st.cx(), st.cy(), k, 0.0, 0.0, |c| {
                c.fill(r, 12.0, Rgba::css(16, 17, 22, 0.78));
                c.text(&text, r, font, look::ON_DARK, Align::Center);
            });
        });
    }

    /// A card in the middle of the stage: it comes up 8 DIPs over 0.4 s.
    fn card(&mut self, f: &mut Frame<'_>) -> (f32, f32) {
        let t = play(f, K_CARD, self.card_fresh, 400, SPRING);
        self.card_fresh = false;
        (t, 8.0 * (1.0 - t))
    }

    fn draw_error(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, st: Rect, name: &str) {
        let (op, dy) = self.card(f);
        let msg = error_text(name);
        let (tf, mf) = (Font::new(16.0, 600), Font::new(13.0, 400));
        let lines = c.lines(msg, mf, 420.0).max(1);
        let text_h = 19.0 + 6.0 + lines as f32 * 19.5;
        let total = 72.0 + 14.0 + text_h + 14.0 + 4.0 + 34.0;
        let top = (st.cy() - total / 2.0).round();
        let x = st.cx();
        let can_next = self.index() + 1 < self.n();
        c.faded(op, |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, dy, |c| {
                let tile = Rect::new(x - 36.0, top, 72.0, 72.0);
                c.fill(tile, 20.0, Rgba::white(0.05));
                c.ring(tile, 20.0, 1.0, Rgba::white(0.08));
                c.icon(look::ICON_BAD, (tile.cx(), tile.cy()), 30.0, 1.7, ui_look::T3, 0.0);
                let ty = top + 72.0 + 14.0;
                let title = format!("Can’t open {name}");
                c.text_fit(&title, Rect::new(x - 210.0, ty, 420.0, 19.0), tf, ui_look::T1, Align::Center);
                for (i, line) in wrap_lines(c, msg, mf, 420.0).iter().enumerate() {
                    c.text(line, Rect::new(x - 210.0, ty + 19.0 + 6.0 + i as f32 * 19.5, 420.0, 19.5), mf, ui_look::T2, Align::Center);
                }
                let by = ty + text_h + 14.0 + 4.0;
                let lw = |c: &mut Canvas<'_>, l: &str| c.measure(l, Font::new(13.0, 600)).ceil() + 28.0;
                let (w1, w2) = (lw(c, "Show in folder"), lw(c, "Next picture"));
                let total_w = w1 + if can_next { 8.0 + w2 } else { 0.0 };
                let bx = x - total_w / 2.0;
                button::button(c, f, ID_ERR_SHOW, Rect::new(bx, by, w1, 34.0), "Show in folder", Kind::Quiet);
                if can_next {
                    button::button(c, f, ID_ERR_NEXT, Rect::new(bx + w1 + 8.0, by, w2, 34.0), "Next picture", Kind::Accent);
                }
            });
        });
    }

    fn draw_empty(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, st: Rect) {
        let (op, dy) = self.card(f);
        let total = 80.0 + 16.0 + (21.0 + 6.0 + 16.0) + 16.0 + 36.0;
        let top = (st.cy() - total / 2.0).round();
        let x = st.cx();
        c.faded(op, |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, dy, |c| {
                // two pictures on top of each other, the front one lit
                let (ox, oy) = (x - 48.0, top);
                let back = Rect::new(ox + 8.0, oy + 10.0, 64.0, 52.0);
                c.rotated(back.cx(), back.cy(), -9.0, |c| {
                    c.shadow(back, 10.0, 8.0, 20.0, Rgba::new(0.0, 0.0, 0.0, 0.4));
                    c.fill(back, 10.0, Rgba::pair(0x2a2b33, 0x2a2b33));
                });
                let front = Rect::new(ox + 22.0, oy + 6.0, 66.0, 54.0);
                c.rotated(front.cx(), front.cy(), 6.0, |c| {
                    c.shadow(front, 10.0, 10.0, 26.0, Rgba::new(0.0, 0.0, 0.0, 0.5));
                    c.gradient_angle(front, 10.0, 160.0, &[(0.0, Rgba::hex(0xffb46b)), (0.55, Rgba::hex(0xe86a8e)), (1.0, Rgba::hex(0x7b6cf0))]);
                    c.masked(front, 10.0, Xf { cx: front.cx(), cy: front.cy(), k: 1.0, dx: 0.0, dy: 0.0 }, |c| {
                        c.glyph("M0 44l18-16 12 10 10-8 26 20v4H0z", (front.x + 12.0, front.y + 12.0), 24.0, Rgba::new(20.0 / 255.0, 14.0 / 255.0, 40.0 / 255.0, 0.55));
                    });
                    c.circle(front.x + 48.0, front.y + 16.0, 5.0, Rgba::pure_white(0.85));
                    c.ring(front, 10.0, 3.0, Rgba::pure_white(0.9));
                });
                let ty = top + 80.0 + 16.0;
                c.text("No picture open", Rect::new(x - 250.0, ty, 500.0, 21.0), Font::new(17.0, 600), ui_look::T1, Align::Center);
                c.text("Open a picture with this program, or drop one here", Rect::new(x - 250.0, ty + 27.0, 500.0, 16.0), ui_look::BODY, ui_look::T2, Align::Center);
                let bw = c.measure("Browse Pictures", Font::new(13.0, 600)).ceil() + 32.0;
                button::button(c, f, ID_BROWSE, Rect::new(x - bw / 2.0, ty + 43.0 + 16.0, bw, 36.0), "Browse Pictures", Kind::Accent);
            });
        });
    }

    /// "Drop to open": a dashed frame over the stage while a file is dragged over the window.
    fn draw_drop(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, st: Rect) {
        let t = play(f, K_DROP, std::mem::take(&mut self.drop_fresh), 200, SPRING);
        let r = Rect::new(st.x + 14.0, st.y + 14.0, st.w - 28.0, st.h - 28.0);
        let acc = ui_look::accent();
        c.faded(t, |c| {
            c.transformed(r.cx(), r.cy(), 0.98 + 0.02 * t, 0.0, 0.0, |c| {
                c.fill(r, 18.0, look::STAGE.mix(acc, 0.08));
                c.dashed_round(r, 18.0, 2.0, 3.0, 3.0, acc.fade(0.8));
                let top = r.cy() - 44.0;
                c.icon(look::ICON_DROP, (r.cx(), top + 17.0), 34.0, 1.8, acc, 0.0);
                c.text("Drop to open", Rect::new(r.x, top + 44.0, r.w, 18.0), Font::new(15.0, 600), ui_look::T1, Align::Center);
                c.text("JPEG, PNG, WebP, GIF, BMP", Rect::new(r.x, top + 72.0, r.w, 16.0), ui_look::BODY, ui_look::T2, Align::Center);
            });
        });
    }

    // -------------------------------------------------------------------------------------------------------------
    // the filmstrip

    fn draw_strip(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        c.fill(r, 0.0, look::BAR);
        c.fill(Rect::new(r.x, r.y - 1.0, r.w, 1.0), 0.0, Rgba::white(0.06));
        let Some(fol) = self.folder.as_ref() else { return };
        let (cur, es) = (fol.index(), fol.entries());
        let half = (r.w / 2.0 / 40.0).ceil() as usize + 2;
        let (lo, hi) = (cur.saturating_sub(half), (cur + half + 1).min(es.len()));
        let cells: Vec<f32> = es[lo..hi]
            .iter()
            .map(|e| {
                let ar = self.thumbs.get(&e.path).map_or(1.3, |(t, _)| (t.w as f32 / t.h as f32).clamp(0.75, 1.6));
                (46.0 * ar).round()
            })
            .collect();
        let xs = strip_layout(&cells, cur - lo, r.w, 16.0, 8.0);
        let acc = ui_look::accent();
        f.clipped(c, r, |c, f| {
            for (k, j) in (lo..hi).enumerate() {
                let e = &es[j];
                let b = Rect::new(r.x + xs[k], r.y + 15.0, cells[k], 46.0);
                if b.x + b.w < r.x || b.x > r.x + r.w {
                    continue;
                }
                let id = ID_THUMB + j as Id;
                let on = j == cur;
                let hot = f.focusable(id, b);
                f.hand();
                f.tip(e.name.clone());
                let cur_t = f.tween(id, 3, f32::from(on), 280, SPRING);
                let (k_s, dy) = (1.0 + 0.08 * cur_t, -2.0 * hot.hover);
                let op = if on { 1.0 } else { 0.55 + 0.45 * hot.hover };
                let xf = Xf { cx: b.cx(), cy: b.cy(), k: k_s, dx: 0.0, dy };
                c.faded(op, |c| {
                    c.transformed(b.cx(), b.cy(), k_s, 0.0, dy, |c| {
                        if on {
                            c.outside(b, 8.0, |c| c.shadow(b, 8.0, 6.0, 16.0, Rgba::new(0.0, 0.0, 0.0, 0.45)));
                        }
                        c.fill(b, 8.0, look::THUMB_BG);
                    });
                    match self.thumbs.get(&e.path) {
                        Some((_, pic)) => c.masked(b, 8.0, xf, |c| c.transformed(b.cx(), b.cy(), k_s, 0.0, dy, |c| c.picture_in(pic, b, 0.0, 1.0, Sample::Smooth))),
                        None if self.dec.thumb_failed(&e.path) => c.transformed(b.cx(), b.cy(), k_s, 0.0, dy, |c| c.icon(look::ICON_BAD_SMALL, (b.cx(), b.cy()), 16.0, 1.8, ui_look::T3, 0.0)),
                        None => {}
                    }
                    c.transformed(b.cx(), b.cy(), k_s, 0.0, dy, |c| {
                        if on {
                            c.ring(b.inset(-2.0), 10.0, 2.0, acc);
                        } else {
                            c.ring(b, 8.0, 1.0, Rgba::pure_white(0.06));
                        }
                    });
                });
                if hot.focus {
                    c.ring(b.inset(-3.0), 11.0, 2.0, ui_look::focus_ring().fade(0.9));
                }
            }
        });
    }

    // -------------------------------------------------------------------------------------------------------------
    // the details panel

    fn facts(&self) -> Vec<(&'static str, String)> {
        let Some(e) = self.current() else { return Vec::new() };
        let g = self.gpu.get(&e.path);
        let dash = || "—".to_string();
        let (dims, mp) = g.map_or((dash(), dash()), |g| {
            (format!("{} × {}", g.meta.width, g.meta.height), format!("{:.1} MP", f64::from(g.meta.width) * f64::from(g.meta.height) / 1e6))
        });
        let mut v = vec![("Dimensions", dims), ("Resolution", mp), ("Size", fmt_size(e.size)), ("Modified", fmt_modified(e.modified)), ("Folder", self.where_.clone())];
        if let Some(m) = g.map(|g| &g.meta) {
            for (k, val) in [("Taken", m.taken.clone()), ("Camera", m.camera.clone()), ("Lens", m.lens.clone()), ("Exposure", m.exposure.clone()), ("ISO", m.iso.map(|i| i.to_string()))] {
                v.extend(val.map(|s| (k, s)));
            }
        }
        v
    }

    fn draw_info(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, r: Rect) {
        c.fill(r, 0.0, look::BAR);
        c.fill(Rect::new(r.x - 1.0, r.y, 1.0, r.h), 0.0, Rgba::white(0.06));
        f.item(ID_PANEL, r);
        let Some(e) = self.current() else { return };
        let (name, kind, has_pic) = (e.name.clone(), kind_of(&e.name), self.gpu.contains_key(&e.path));
        let facts = self.facts();
        let palette = self.gpu.get(&e.path).map(|g| g.palette);
        let scroll = f.tween(K_SCROLL, 0, self.info_scroll, 150, EASE);
        let (x, w) = (r.x + 18.0, r.w - 36.0);
        let name_f = Font::new(15.0, 600);
        let lines = c.lines(&name, name_f, w).clamp(1, 4);
        let mut y = r.y + 18.0 - scroll;
        f.clipped(c, r, |c, f| {
            c.text_wrapped(&name, Rect::new(x, y, w, lines as f32 * 19.5), name_f, ui_look::T1, 19.5, 4);
            y += lines as f32 * 19.5 + 4.0;
            c.text(&kind, Rect::new(x, y, w, 16.0), ui_look::BODY, ui_look::T3, Align::Start);
            y += 16.0 + 18.0;
            let ff = Font::new(12.5, 400).tabular();
            for (k, v) in &facts {
                c.text(k, Rect::new(x, y, 92.0, 15.0), ff, ui_look::T3, Align::Start);
                c.text_fit(v, Rect::new(x + 102.0, y, w - 102.0, 15.0), ff, ui_look::T1, Align::End);
                y += 15.0 + 9.0;
            }
            y += 18.0 - 9.0;
            if let Some(pal) = palette {
                c.fill(Rect::new(x, y, w, 1.0), 0.0, Rgba::white(0.06));
                y += 1.0 + 16.0;
                c.text("Colors", Rect::new(x, y, 120.0, 16.0), Font::new(13.0, 600), ui_look::T1, Align::Start);
                c.text("Click to copy", Rect::new(x + w - 120.0, y, 120.0, 16.0), Font::new(12.0, 400), ui_look::T3, Align::End);
                y += 16.0 + 10.0;
                let sw = (w - 4.0 * 8.0) / 5.0;
                for (i, col) in pal.iter().enumerate() {
                    let b = Rect::new(x + i as f32 * (sw + 8.0), y, sw, 36.0);
                    let hot = f.item(ID_SWATCH + i as Id, b);
                    f.hand();
                    f.tip(hex(*col));
                    c.transformed(b.cx(), b.cy(), 1.0 + 0.06 * hot.hover, 0.0, -2.0 * hot.hover, |c| {
                        c.fill(b, 9.0, color(*col));
                        c.ring(b, 9.0, 1.0, Rgba::pure_white(0.12));
                    });
                }
                y += 36.0 + 18.0;
            }
            c.fill(Rect::new(x, y, w, 1.0), 0.0, Rgba::white(0.06));
            y += 1.0 + 16.0;
            if has_pic {
                wide_button(c, f, ID_WALL, Rect::new(x, y, w, 34.0), look::ICON_WALL, "Set as wallpaper", false);
                y += 34.0 + 8.0;
            }
            wide_button(c, f, ID_SHOW, Rect::new(x, y, w, 34.0), look::ICON_FOLDER, "Show in folder", true);
            y += 34.0;
        });
        self.info_max = (y + scroll + 18.0 - r.y - r.h).max(0.0);
        self.info_scroll = self.info_scroll.min(self.info_max);
    }
}

/// A button of the details panel: icon and label in the middle; `quiet` has a ring instead of a plate.
fn wide_button(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, icon: &'static str, label: &str, ring: bool) {
    let hot = f.focusable(id, r);
    f.hand();
    c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
        if ring {
            c.fill(r, 10.0, Rgba::white(0.04 * hot.hover));
            c.ring(r, 10.0, 1.0, Rgba::white(0.1 + 0.06 * hot.hover));
        } else {
            c.fill(r, 10.0, Rgba::white(0.08 + 0.05 * hot.hover));
        }
        let ink = if ring { ui_look::T2 } else { ui_look::T1 };
        let font = Font::new(13.0, 500);
        let tw = c.measure(label, font).ceil();
        let x0 = r.cx() - (15.0 + 8.0 + tw) / 2.0;
        c.icon(icon, (x0 + 7.5, r.cy()), 15.0, 1.9, ink, 0.0);
        c.text(label, Rect::new(x0 + 23.0, r.y, tw + 2.0, r.h), font, ink, Align::Start);
    });
    if hot.focus {
        c.ring(r.inset(-3.0), 13.0, 2.0, ui_look::focus_ring().fade(0.9));
    }
}

/// The lines `text` breaks into at `width`, for text drawn line by line (centred paragraphs).
fn wrap_lines(c: &mut Canvas<'_>, text: &str, font: Font, width: f32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match out.last_mut() {
            Some(l) if c.measure(&format!("{l} {word}"), font) <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => out.push(word.to_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use winrust_ui::frame::Req;

    fn folder_of(names: &[&str], tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photoview-viewer-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in names {
            std::fs::write(dir.join(n), crate::decoder::tests::png(2)).unwrap();
        }
        dir
    }

    /// A viewer on `dir` whose Recycle Bin is a list of the names it was asked to take (nothing is deleted).
    fn viewer(dir: &Path, first: &str) -> (Viewer, Arc<Mutex<Vec<String>>>) {
        let mut v = Viewer::open(Some(&dir.join(first)), false);
        let taken = Arc::new(Mutex::new(Vec::new()));
        let t = taken.clone();
        v.bin = Arc::new(move |h: &Hidden| {
            t.lock().unwrap().push(h.entry.name.clone());
            Ok(())
        });
        (v, taken)
    }

    fn name(v: &Viewer) -> String {
        v.current().map(|e| e.name.clone()).unwrap_or_default()
    }

    fn toasts(cx: &Cx) -> Vec<(String, Option<u32>)> {
        cx.reqs.iter().filter_map(|r| if let Req::Toast { text, undo, .. } = r { Some((text.clone(), *undo)) } else { None }).collect()
    }

    #[test]
    fn a_deleted_picture_goes_to_the_recycle_bin_only_when_its_toast_is_gone() {
        let dir = folder_of(&["a.png", "b.png", "c.png"], "del");
        let (mut v, taken) = viewer(&dir, "b.png");
        let mut cx = Cx::default();
        assert!(v.key(0x2E, Mods::default(), &mut cx), "Del is used");
        assert_eq!(name(&v), "c.png", "the next picture is shown at once");
        assert_eq!(toasts(&cx), [("Moved b.png to the Recycle Bin".to_string(), Some(TAG_UNDO))], "the toast offers Undo");
        assert!(taken.lock().unwrap().is_empty(), "nothing has gone to the Recycle Bin while the toast is up");
        // the toast is gone: the timer fires
        v.pending.as_mut().unwrap().due = 0;
        v.wake(&mut Cx::default());
        v.commits.drain(..).for_each(|c| c.join().unwrap());
        assert_eq!(*taken.lock().unwrap(), ["b.png"]);
        assert!(v.pending.is_none());
        drop(v); // joins the workers: they hold the files
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn undo_brings_the_picture_back_and_nothing_is_deleted() {
        let dir = folder_of(&["a.png", "b.png", "c.png"], "undo");
        let (mut v, taken) = viewer(&dir, "b.png");
        v.delete(&mut Cx::default());
        v.command(TAG_UNDO, 0, &mut Cx::default());
        assert_eq!((name(&v), v.n()), ("b.png".to_string(), 3), "Undo shows it again where it was");
        v.close(&mut Cx::default());
        assert!(taken.lock().unwrap().is_empty(), "the Recycle Bin was never asked");
        drop(v); // joins the workers: they hold the files
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_next_delete_and_closing_the_window_send_the_waiting_picture_on() {
        let dir = folder_of(&["a.png", "b.png", "c.png", "d.png"], "next");
        let (mut v, taken) = viewer(&dir, "a.png");
        v.delete(&mut Cx::default()); // a waits
        v.delete(&mut Cx::default()); // b is deleted: a goes now
        for c in v.commits.drain(..) {
            c.join().unwrap();
        }
        assert_eq!(*taken.lock().unwrap(), ["a.png"]);
        assert_eq!((v.n(), name(&v)), (2, "c.png".to_string()));
        assert!(v.close(&mut Cx::default()), "closing is allowed");
        let mut got = taken.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["a.png", "b.png"], "closing sent the second one");
        drop(v); // joins the workers: they hold the files
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_picture_the_bin_refused_comes_back() {
        let dir = folder_of(&["a.png", "b.png"], "refuse");
        let (mut v, _) = viewer(&dir, "a.png");
        v.bin = Arc::new(|_: &Hidden| Err("locked".to_string()));
        v.delete(&mut Cx::default());
        v.flush();
        for c in v.commits.drain(..) {
            c.join().unwrap();
        }
        let mut cx = Cx::default();
        v.message(MSG_RECYCLE, 0, 0, &mut cx);
        assert_eq!(v.n(), 2, "the file is in the list again");
        assert_eq!(toasts(&cx)[0].0, "Couldn’t move a.png to the Recycle Bin");
        drop(v); // joins the workers: they hold the files
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_keys_of_the_old_viewer_still_work() {
        let dir = folder_of(&["a.png", "b.png", "c.png"], "keys");
        let (mut v, _) = viewer(&dir, "a.png");
        let key = |v: &mut Viewer, vk: u32| v.key(vk, Mods::default(), &mut Cx::default());
        assert!(key(&mut v, 0x27) && name(&v) == "b.png", "Right");
        assert!(key(&mut v, 0x20) && name(&v) == "c.png", "Space");
        assert!(key(&mut v, 0x22) && name(&v) == "c.png", "PgDn at the end stays (the list does not wrap)");
        assert!(key(&mut v, 0x24) && name(&v) == "a.png", "Home");
        assert!(key(&mut v, 0x23) && name(&v) == "c.png", "End");
        assert!(key(&mut v, 0x08) && name(&v) == "b.png", "Backspace");
        assert!(key(&mut v, 0x21) && name(&v) == "a.png", "PgUp");
        assert!(key(&mut v, 0x25) && name(&v) == "a.png", "Left at the start stays");
        assert!(!key(&mut v, 0x41), "a key it does not know is left to the window");
        // Esc leaves the zoom first and closes the window second
        v.zm = Some(Zoom { z: 2.0, ox: 0.0, oy: 0.0 });
        let mut cx = Cx::default();
        v.key(0x1B, Mods::default(), &mut cx);
        assert!(v.zm.is_none() && !cx.reqs.iter().any(|r| matches!(r, Req::Close)));
        let mut cx = Cx::default();
        v.key(0x1B, Mods::default(), &mut cx);
        assert!(cx.reqs.iter().any(|r| matches!(r, Req::Close)), "Esc closes the window");
        drop(v); // joins the workers: they hold the files
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_dragged_file_is_welcome_only_when_it_is_a_picture() {
        let dir = folder_of(&["a.png"], "drag");
        let (mut v, _) = viewer(&dir, "a.png");
        assert!(!v.drag_over(&["C:\\x\\notes.txt".to_string()], 0.0, 0.0) && !v.drop_on);
        assert!(v.drag_over(&[dir.join("a.png").display().to_string()], 0.0, 0.0) && v.drop_on);
        v.drag_leave(&mut Cx::default());
        assert!(!v.drop_on);
        let mut cx = Cx::default();
        v.drop(vec!["C:\\x\\notes.txt".into()], 0.0, 0.0, &mut cx);
        assert_eq!(toasts(&cx)[0].0, "That isn’t a picture");
        drop(v); // joins the workers: they hold the files
        std::fs::remove_dir_all(dir).unwrap();
    }
}
