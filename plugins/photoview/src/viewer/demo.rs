//! The viewer in the states of the design (`--render state=<name>`, `--bench`, `--selftest`): a folder of pictures, a viewer that
//! touches nothing of the user's machine, and a wait for the workers so that the first frame already has its picture.
//!
//! The pictures come from `PHOTOVIEW_DEMO\{pictures,shots,broken}` when that variable names a folder, and otherwise from the
//! wallpapers Windows ships (`C:\Windows\Web\Wallpaper`). `PHOTOVIEW_OPEN=<file>` opens that picture instead (the benchmark of a big
//! photo).
use super::*;

/// What `--render-all` goes through: the states of the design's "Preview states" list.
pub const STATES: [&str; 15] = [
    "landscape", "portrait", "screenshot", "wallpaper", "fit", "zoom-100", "zoom-300", "rotated", "details-strip", "picture-only", "more-menu", "no-picture",
    "dragging", "unsupported", "just-deleted",
];

fn windows_wallpapers(sub: &str) -> Option<PathBuf> {
    let root = Path::new(r"C:\Windows\Web\Wallpaper");
    let d = root.join(match sub {
        "pictures" => "ThemeA",
        "shots" => "ThemeB",
        _ => return None,
    });
    d.is_dir().then_some(d)
}

/// A folder of pictures for the demo: `pictures`, `shots`, or `broken` (one picture the system has no codec for).
pub fn folder(name: &str) -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("PHOTOVIEW_DEMO") {
        let d = PathBuf::from(root).join(name);
        if d.is_dir() {
            return Some(d);
        }
    }
    if name != "broken" {
        return windows_wallpapers(name);
    }
    // a folder of its own in the temp dir: two real pictures and a HEIC that is not one
    let d = std::env::temp_dir().join("winrust-photoview-demo").join("broken");
    let src = windows_wallpapers("pictures")?;
    let mut jpgs = std::fs::read_dir(src).ok()?.flatten().map(|e| e.path()).filter(|p| is_pic(p));
    let _ = std::fs::create_dir_all(&d);
    if let (Some(a), Some(b)) = (jpgs.next(), jpgs.next()) {
        let _ = std::fs::copy(a, d.join("IMG_2031.jpg"));
        let _ = std::fs::copy(b, d.join("IMG_2040.png"));
    }
    let _ = std::fs::write(d.join("IMG_2041.heic"), b"not a picture: the codec is missing");
    Some(d)
}

/// The viewer for `state`, with its first picture ready.
pub fn demo(state: &str) -> Box<dyn App> {
    let (sub, at) = match state {
        "portrait" => ("pictures", 1),
        "screenshot" => ("shots", 3),
        "wallpaper" => ("pictures", 3),
        "unsupported" => ("broken", 2),
        _ => ("pictures", 2),
    };
    let open = std::env::var_os("PHOTOVIEW_OPEN").map(PathBuf::from);
    let dir = folder(sub);
    let mut v = if state == "no-picture" || state == "dragging" {
        Viewer::open(None, false)
    } else if let Some(p) = open.or_else(|| dir.and_then(|d| Folder::open(&d).and_then(|f| f.entries().get(at).or(f.entries().last()).map(|e| e.path.clone())))) {
        Viewer::open(Some(&p), false)
    } else {
        Viewer::open(None, false)
    };
    match state {
        "zoom-100" => v.zm = Some(Zoom { z: 1.0, ox: 0.0, oy: 0.0 }),
        "zoom-300" => v.zm = Some(Zoom { z: 3.0, ox: 900.0, oy: -300.0 }),
        "rotated" => {
            if let Some(p) = v.current().map(|e| e.path.clone()) {
                v.rot.insert(p, 90);
            }
        }
        "picture-only" => (v.info, v.strip) = (false, false),
        "dragging" => v.drop_on = true,
        "more-menu" => v.demo_clicks = vec![ID_MORE],
        "just-deleted" => v.demo_clicks = vec![ID_DELETE],
        _ => {}
    }
    // a render has no message loop to wait for the picture in; the host's benchmark and self-tests (state "") have, and want the
    // start the user gets
    if !state.is_empty() {
        v.wait_for_workers();
    }
    Box::new(v)
}

impl Viewer {
    /// Waits (up to 20 s) until the shown picture and the filmstrip's thumbnails are made: a render takes its picture at once, it
    /// cannot wait for a message.
    fn wait_for_workers(&mut self) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let Some(f) = self.folder.as_ref() else { return };
        let (cur, es) = (f.index(), f.entries());
        let path = es[cur].path.clone();
        let strip: Vec<PathBuf> = if self.strip_shown() { es.iter().map(|e| e.path.clone()).take(12).collect() } else { Vec::new() };
        while std::time::Instant::now() < end {
            if let Some(d) = self.dec.take(&path) {
                self.inbox.push((path.clone(), d));
            }
            let picture = !self.inbox.is_empty() || self.dec.error(&path).is_some();
            let thumbs = strip.iter().all(|p| self.dec.thumb(p).is_some() || self.dec.thumb_failed(p));
            if picture && thumbs {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
