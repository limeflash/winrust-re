//! The desktop backgrounds of the Personalization page for the real system (`os::Live` calls these): the picture that is the wallpaper now
//! (its thumbnail decoded by WIC on a worker thread), the design's presets (Dusk, Sea, Sand, Graphite), and what picking a preset does:
//! the preset is drawn once into a PNG of the primary display's size in `%LOCALAPPDATA%\WinRust\wallpapers\<id>.png` (reused while the
//! file is there and the size matches) and set the way the picker sets a picture (SPI_SETDESKWALLPAPER, the desktop follows it).
//! The pure parts (the gradient, the file's size) are tested; the rest runs only in the real window.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use windows::{
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
            WICDecodeMetadataCacheOnDemand, WICRect,
        },
        System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
    },
    core::HSTRING,
};
use windows_sys::Win32::{System::Registry::HKEY_CURRENT_USER, UI::WindowsAndMessaging::*};

use crate::{
    Fill, Wall, display,
    os::{Note, Ping},
    sys::{self, wide},
};

/// A thumbnail: premultiplied BGRA, `w` × `h` device pixels.
#[derive(Debug, PartialEq, Eq)]
pub struct Thumb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

/// The size a tile's picture is decoded to (the tile is 92 × 60 DIPs; three times that stays sharp up to 300 %).
const THUMB: (u32, u32) = (276, 180);

/// The presets of the design: id, name, the stops of `linear-gradient(160deg, ...)` (position 0..1, 0xRRGGBB); one stop is a plain colour.
pub const PRESETS: [(&str, &str, &[(f32, u32)]); 4] = [
    ("dusk", "Dusk", &[(0.0, 0x1e2a4a), (0.6, 0x4a2a5e), (1.0, 0xc2546a)]),
    ("sea", "Sea", &[(0.0, 0x0b3d4f), (0.55, 0x1f7a8c), (1.0, 0x9fd3c7)]),
    ("sand", "Sand", &[(0.0, 0x3b2f2a), (0.55, 0x8c6a4f), (1.0, 0xe7c9a0)]),
    ("graphite", "Graphite", &[(0.0, 0x2a2c33)]),
];

/// The preset as the tile draws it.
fn fill_of(stops: &[(f32, u32)]) -> Fill {
    match stops {
        [(_, c)] => Fill::Solid(*c),
        [(_, a), (m, b), (_, z)] => Fill::Gradient(*a, *b, *z, *m),
        _ => Fill::Solid(0x2a2c33),
    }
}

/// `linear-gradient(160deg, stops)` over `w` × `h` pixels (CSS: the line runs 160 degrees clockwise from "up", as long as the box's
/// projection on it), as premultiplied BGRA with the alpha 255.
#[must_use]
pub fn gradient_pixels(w: u32, h: u32, stops: &[(f32, u32)]) -> Vec<u32> {
    let rad = 160f32.to_radians();
    let (dx, dy) = (rad.sin(), -rad.cos());
    let len = (w as f32 * dx.abs() + h as f32 * dy.abs()).max(1.0);
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let chan = |c: u32, s: u32| ((c >> s) & 255) as f32;
    let at = |t: f32| -> u32 {
        let t = t.clamp(0.0, 1.0);
        let i = stops.iter().position(|s| s.0 >= t).unwrap_or(stops.len() - 1);
        let (a, b) = if i == 0 { (stops[0], stops[0]) } else { (stops[i - 1], stops[i]) };
        let k = if b.0 > a.0 { ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0) } else { 0.0 };
        let m = |s: u32| (chan(a.1, s) + (chan(b.1, s) - chan(a.1, s)) * k).round() as u32;
        0xff00_0000 | m(16) << 16 | m(8) << 8 | m(0)
    };
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            px.push(at(((x as f32 + 0.5 - cx) * dx + (y as f32 + 0.5 - cy) * dy) / len + 0.5));
        }
    }
    px
}

/// The pixel size a PNG file says it has (its IHDR), read without decoding.
#[must_use]
pub fn png_size(path: &std::path::Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut b = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut b).ok()?;
    (b[..8] == [0x89, b'P', b'N', b'G', 13, 10, 26, 10]).then(|| (u32::from_be_bytes([b[16], b[17], b[18], b[19]]), u32::from_be_bytes([b[20], b[21], b[22], b[23]])))
}

fn factory() -> Option<IWICImagingFactory> {
    // SAFETY: COM on this thread (a worker has none yet: the call is harmless when it already has).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()
    }
}

/// Writes premultiplied BGRA pixels as a PNG.
pub fn write_png(path: &std::path::Path, w: u32, h: u32, px: &[u32]) -> bool {
    let go = || -> Option<()> {
        let f = factory()?;
        // SAFETY: WIC objects made and used here; the pixels outlive the call.
        unsafe {
            let enc = f.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
            let stream = f.CreateStream().ok()?;
            stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), 0x4000_0000).ok()?; // GENERIC_WRITE
            enc.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
            let (mut frame, mut props) = (None, None);
            enc.CreateNewFrame(&mut frame, &mut props).ok()?;
            let frame = frame?;
            frame.Initialize(props.as_ref()).ok()?;
            frame.SetSize(w, h).ok()?;
            let mut fmt = GUID_WICPixelFormat32bppPBGRA;
            frame.SetPixelFormat(&mut fmt).ok()?;
            frame.WritePixels(h, w * 4, std::slice::from_raw_parts(px.as_ptr().cast::<u8>(), px.len() * 4)).ok()?;
            frame.Commit().ok()?;
            enc.Commit().ok()
        }
    };
    go().is_some()
}

/// The picture at `path` cut to cover a `THUMB` box from its middle (WIC scales while it decodes).
fn decode_thumb(path: &str) -> Option<Thumb> {
    let f = factory()?;
    // SAFETY: WIC objects made and used on this thread.
    unsafe {
        let dec = f.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let (mut iw, mut ih) = (0u32, 0u32);
        frame.GetSize(&mut iw, &mut ih).ok()?;
        if iw == 0 || ih == 0 {
            return None;
        }
        let (tw, th) = THUMB;
        let k = (tw as f32 / iw as f32).max(th as f32 / ih as f32);
        let (sw, sh) = (((iw as f32 * k).ceil() as u32).max(tw), ((ih as f32 * k).ceil() as u32).max(th));
        let scaler = f.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, sw, sh, WICBitmapInterpolationModeFant).ok()?;
        let conv = f.CreateFormatConverter().ok()?;
        conv.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let rect = WICRect { X: ((sw - tw) / 2) as i32, Y: ((sh - th) / 2) as i32, Width: tw as i32, Height: th as i32 };
        let mut px = vec![0u32; (tw * th) as usize];
        conv.CopyPixels(&rect, tw * 4, std::slice::from_raw_parts_mut(px.as_mut_ptr().cast::<u8>(), px.len() * 4)).ok()?;
        Some(Thumb { w: tw, h: th, px })
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the real system

const DESKTOP: &str = r"Control Panel\Desktop";

fn dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("WinRust").join("wallpapers"))
}

fn preset_file(id: &str) -> Option<PathBuf> {
    Some(dir()?.join(format!("{id}.png")))
}

/// The wallpaper's file now: the registry's, else the one Windows keeps transcoded (a slideshow or a solid colour has no file of its own).
fn current_path() -> Option<String> {
    let p = sys::string_long(HKEY_CURRENT_USER, DESKTOP, "WallPaper").filter(|p| !p.is_empty() && std::path::Path::new(p).is_file());
    p.or_else(|| {
        let t = PathBuf::from(std::env::var_os("APPDATA")?).join(r"Microsoft\Windows\Themes\TranscodedWallpaper");
        t.is_file().then(|| t.to_string_lossy().into_owned())
    })
}

/// Which preset the file at `path` is.
fn preset_of(path: &str) -> Option<usize> {
    PRESETS.iter().position(|(id, ..)| preset_file(id).is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(path)))
}

type Cache = Mutex<HashMap<String, Option<Arc<Thumb>>>>;

fn cache() -> &'static Cache {
    static C: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// The key of a thumbnail: the file and when it last changed.
fn stamp(path: &str) -> String {
    let t = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    format!("{path}|{t}")
}

/// What the page's name line says: the preset's name, or the picture's file name.
pub fn name() -> String {
    match current_path() {
        Some(p) => preset_of(&p).map_or_else(|| p.rsplit('\\').next().unwrap_or_default().to_string(), |i| PRESETS[i].1.to_string()),
        None => String::new(),
    }
}

/// The tiles: the picture that is the wallpaper (selected; its thumbnail once the worker has it), then the presets, the current one marked.
pub fn walls() -> Vec<Wall> {
    let cur = current_path();
    let on_preset = cur.as_deref().and_then(preset_of);
    let mut v = Vec::new();
    if let (Some(p), None) = (&cur, on_preset) {
        let pic = cache().lock().ok().and_then(|c| c.get(&stamp(p)).cloned().flatten());
        v.push(Wall { label: p.rsplit('\\').next().unwrap_or_default().into(), fill: Fill::Solid(0x22232a), on: true, pic });
    }
    v.extend(PRESETS.iter().enumerate().map(|(i, (_, name, stops))| Wall { label: (*name).into(), fill: fill_of(stops), on: on_preset == Some(i), pic: None }));
    v
}

/// Decodes the thumbnail of the current wallpaper off the window's thread; `done` says it is there.
pub fn prepare(done: Ping) {
    let Some(p) = current_path().filter(|p| preset_of(p).is_none()) else { return };
    let key = stamp(&p);
    if cache().lock().is_ok_and(|c| c.contains_key(&key)) {
        return;
    }
    std::thread::spawn(move || {
        let t = decode_thumb(&p).map(Arc::new);
        if let Ok(mut c) = cache().lock() {
            c.retain(|k, _| !k.starts_with(&p)); // an older picture of the same file goes
            c.insert(key, t);
        }
        done();
    });
}

/// Picks tile `k` of `walls()`: a preset is drawn (once) and set; the picture itself is already the wallpaper.
pub fn select(k: usize, done: Note) {
    let cur_is_picture = current_path().is_some_and(|p| preset_of(&p).is_none());
    let Some(i) = k.checked_sub(usize::from(cur_is_picture)).filter(|&i| i < PRESETS.len()) else { return };
    std::thread::spawn(move || {
        let (id, name, stops) = PRESETS[i];
        let Some(file) = preset_file(id) else { return done(t!("Не удалось сохранить картинку", "Could not save the picture").into()) };
        // the primary display's size in pixels (the desktop fills every display with it)
        let (w, h) = display::layout().iter().find(|m| m.primary).map_or((1920, 1080), |m| (m.w.max(1) as u32, m.h.max(1) as u32));
        if png_size(&file) != Some((w, h)) {
            let _ = std::fs::create_dir_all(file.parent().unwrap_or(&file));
            if !write_png(&file, w, h, &gradient_pixels(w, h, stops)) {
                return done(t!("Не удалось сохранить картинку", "Could not save the picture").into());
            }
        }
        // "fill": the picture covers the display; what the desktop was set to before (centred, tiled) would show a frame
        sys::set_hkcu_string(DESKTOP, "WallpaperStyle", "10");
        sys::set_hkcu_string(DESKTOP, "TileWallpaper", "0");
        let mut path = wide(&file.to_string_lossy());
        // SAFETY: the same call the picker makes; the desktop reads the new file on SPIF_SENDCHANGE.
        let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, path.as_mut_ptr().cast(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) } != 0;
        done(if ok { tf!("Фон: {}", "Background: {}", name) } else { t!("Windows не приняла картинку", "Windows did not take the picture").into() });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gradient_runs_160_degrees_and_a_single_stop_is_flat() {
        let px = gradient_pixels(40, 20, &[(0.0, 0xff0000), (1.0, 0x0000ff)]);
        assert_eq!(px.len(), 800);
        let top_left = px[0];
        let bottom_right = px[799];
        assert!(((top_left >> 16) & 255) > 230 && (top_left & 255) < 25, "{top_left:x}: starts red");
        assert!((bottom_right & 255) > 230 && ((bottom_right >> 16) & 255) < 25, "{bottom_right:x}: ends blue");
        assert_eq!(top_left >> 24, 255);
        let flat = gradient_pixels(8, 8, &[(0.0, 0x2a2c33)]);
        assert!(flat.iter().all(|&p| p == 0xff2a_2c33));
        // 160 degrees points down and a little to the right: the top right is darker than the bottom left only by a little, both mid-way
        let mid = |p: u32| (p & 255) as i32;
        assert!((mid(px[39]) - mid(px[760])).abs() < 90, "the line is mostly vertical");
    }

    #[test]
    fn a_preset_is_written_once_and_its_size_is_read_back() {
        let dir = std::env::temp_dir().join("winrust-settings-wall-test");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("t.png");
        let _ = std::fs::remove_file(&f);
        assert!(png_size(&f).is_none());
        let (id, _, stops) = PRESETS[1];
        assert_eq!(id, "sea");
        assert!(write_png(&f, 64, 36, &gradient_pixels(64, 36, stops)));
        assert_eq!(png_size(&f), Some((64, 36)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reads only, by hand: `cargo test -p rust-settings --release --offline -- --ignored print_walls --nocapture` (select() is never called).
    #[test]
    #[ignore = "reads the real machine"]
    fn print_walls() {
        println!("current {:?}, name {:?}", current_path(), name());
        let (tx, rx) = std::sync::mpsc::channel();
        prepare(Box::new(move || drop(tx.send(()))));
        let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        for w in walls() {
            println!("{} on={} thumb={:?}", w.label, w.on, w.pic.as_ref().map(|t| (t.w, t.h, t.px[t.px.len() / 2] & 0xffffff)));
        }
        println!("primary {:?}", display::layout().iter().find(|m| m.primary).map(|m| (m.w, m.h)));
    }
}
