use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};

pub struct Meta {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub taken: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub exposure: Option<String>,
    pub iso: Option<u32>,
    pub size: u64,
}
pub struct Decoded {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
    pub meta: Meta,
    pub palette: [u32; 5],
}
pub struct Thumb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}
#[derive(Default)]
struct State {
    stopped: bool,
    generation: u64,
    pictures: Vec<PathBuf>,
    strip: Vec<PathBuf>,
    decoded: HashMap<PathBuf, Arc<Decoded>>,
    thumbs: HashMap<PathBuf, Arc<Thumb>>,
    order: VecDeque<PathBuf>,
    errors: HashMap<PathBuf, String>,
    seen: HashSet<PathBuf>,
}
pub struct Decoder {
    shared: Arc<(Mutex<State>, Condvar)>,
    worker: Option<JoinHandle<()>>,
}
impl Decoder {
    pub fn new(ready: impl Fn() + Send + 'static) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let s = shared.clone();
        let worker = thread::spawn(move || unsafe {
            let initialized = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            )
            .is_ok();
            loop {
                let (path, picture, generation) = {
                    let (lock, cv) = &*s;
                    let mut st = lock.lock().unwrap();
                    loop {
                        if st.stopped {
                            if initialized {
                                windows::Win32::System::Com::CoUninitialize();
                            }
                            return;
                        }
                        let p = st
                            .pictures
                            .iter()
                            .find(|p| !st.decoded.contains_key(*p) && !st.errors.contains_key(*p))
                            .cloned();
                        let t = st
                            .strip
                            .iter()
                            .find(|p| {
                                !st.thumbs.contains_key(*p)
                                    && !st.errors.contains_key(*p)
                                    && !st.seen.contains(*p)
                            })
                            .cloned();
                        if let Some(p) = p {
                            break (p, true, st.generation);
                        }
                        if let Some(p) = t {
                            break (p, false, st.generation);
                        }
                        st = cv.wait(st).unwrap();
                    }
                };
                let result = crate::decode::decode_model(&path);
                let mut st = s.0.lock().unwrap();
                if st.stopped {
                    continue;
                }
                if generation != st.generation
                    && !(if picture {
                        st.pictures.contains(&path)
                    } else {
                        st.strip.contains(&path)
                    })
                {
                    continue;
                }
                match result {
                    Ok(image) => {
                        if picture {
                            st.decoded.insert(path, Arc::new(image));
                        } else {
                            st.seen.insert(path.clone());
                            let thumb = Arc::new(make_thumb(&image));
                            st.thumbs.insert(path.clone(), thumb);
                            st.order.retain(|p| p != &path);
                            st.order.push_back(path);
                            while st.order.len() > 256 {
                                if let Some(p) = st.order.pop_front() {
                                    st.thumbs.remove(&p);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let msg = if path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("heic"))
                        {
                            format!("HEIC needs the HEIF Image Extensions: {e}")
                        } else {
                            e.to_string()
                        };
                        st.errors.insert(path, msg);
                    }
                }
                drop(st);
                ready();
            }
        });
        Self {
            shared,
            worker: Some(worker),
        }
    }
    pub fn want(&self, current: &Path, neighbours: &[PathBuf], strip: &[PathBuf]) {
        let mut st = self.shared.0.lock().unwrap();
        st.generation += 1;
        st.pictures = std::iter::once(current.to_path_buf())
            .chain(
                neighbours
                    .iter()
                    .filter(|p| p.as_path() != current)
                    .take(2)
                    .cloned(),
            )
            .collect();
        st.strip = strip.to_vec();
        st.seen.clear();
        let pics: HashSet<_> = st.pictures.iter().cloned().collect();
        let wanted: HashSet<_> = st.pictures.iter().chain(st.strip.iter()).cloned().collect();
        st.decoded.retain(|p, _| pics.contains(p));
        st.errors.retain(|p, _| wanted.contains(p));
        self.shared.1.notify_one();
    }
    pub fn picture(&self, p: &Path) -> Option<Arc<Decoded>> {
        self.shared.0.lock().unwrap().decoded.get(p).cloned()
    }
    pub fn thumb(&self, p: &Path) -> Option<Arc<Thumb>> {
        self.shared.0.lock().unwrap().thumbs.get(p).cloned()
    }
    pub fn error(&self, p: &Path) -> Option<String> {
        self.shared.0.lock().unwrap().errors.get(p).cloned()
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().stopped = true;
        self.shared.1.notify_one();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn make_thumb(image: &Decoded) -> Thumb {
    let scale = (92.0 / image.w as f64).max(46.0 / image.h as f64);
    let sw = 92.0 / scale;
    let sh = 46.0 / scale;
    let x0 = (image.w as f64 - sw) / 2.0;
    let y0 = (image.h as f64 - sh) / 2.0;
    let mut px = Vec::with_capacity(92 * 46);
    for y in 0..46 {
        for x in 0..92 {
            let sx = (x0 + (x as f64 + 0.5) / scale).floor() as usize;
            let sy = (y0 + (y as f64 + 0.5) / scale).floor() as usize;
            px.push(
                image.px[sy.min(image.h as usize - 1) * image.w as usize
                    + sx.min(image.w as usize - 1)],
            );
        }
    }
    Thumb { w: 92, h: 46, px }
}
pub(crate) fn palette(px: &[u32], w: u32, h: u32) -> [u32; 5] {
    let mut samples = Vec::with_capacity(4096);
    for y in 0..64 {
        for x in 0..64 {
            let p = px[((y * h / 64) * w + x * w / 64) as usize];
            let a = p >> 24;
            if let Some(a) = std::num::NonZeroU32::new(a) {
                let r = ((p >> 16 & 255) * 255 / a).min(255);
                let g = ((p >> 8 & 255) * 255 / a).min(255);
                let b = ((p & 255) * 255 / a).min(255);
                samples.push((r << 16) | (g << 8) | b);
            }
        }
    }
    let mut boxes = vec![samples];
    while boxes.len() < 5 {
        let candidate = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .filter_map(|(i, b)| {
                let ranges: Vec<u32> = [16, 8, 0]
                    .iter()
                    .map(|s| {
                        let (min, max) = b.iter().fold((255, 0), |(lo, hi), p| {
                            let c = p >> s & 255;
                            (lo.min(c), hi.max(c))
                        });
                        max - min
                    })
                    .collect();
                let (axis, range) = ranges.iter().enumerate().max_by_key(|(_, r)| *r)?;
                if *range == 0 {
                    None
                } else {
                    Some((i, axis, *range * b.len() as u32))
                }
            })
            .max_by_key(|x| x.2);
        let Some((i, axis, _)) = candidate else { break };
        let mut b = boxes.swap_remove(i);
        let shift = [16, 8, 0][axis];
        b.sort_unstable_by_key(|p| p >> shift & 255);
        let right = b.split_off(b.len() / 2);
        boxes.push(b);
        boxes.push(right);
    }
    boxes.sort_by_key(|b| std::cmp::Reverse(b.len()));
    let mut out = [0; 5];
    for (i, b) in boxes.iter().enumerate() {
        if !b.is_empty() {
            let n = b.len() as u64;
            let sums = b.iter().fold([0u64; 3], |mut a, p| {
                a[0] += (p >> 16 & 255) as u64;
                a[1] += (p >> 8 & 255) as u64;
                a[2] += (p & 255) as u64;
                a
            });
            out[i] = ((sums[0] / n) << 16 | (sums[1] / n) << 8 | (sums[2] / n)) as u32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, ty: &[u8], data: &[u8]) {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = ty.to_vec();
            body.extend(data);
            out.extend(&body);
            let mut c = !0u32;
            for b in body {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        0xedb88320 ^ (c >> 1)
                    } else {
                        c >> 1
                    };
                }
            }
            out.extend((!c).to_be_bytes());
        }
        let raw = [0u8, 255, 0, 0, 255, 0, 0, 255, 255];
        let (mut a, mut b) = (1u32, 0u32);
        for x in raw {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        let mut z = vec![
            0x78,
            0x01,
            0x01,
            raw.len() as u8,
            0,
            !(raw.len() as u8),
            255,
        ];
        z.extend(raw);
        z.extend(((b << 16) | a).to_be_bytes());
        let mut p = vec![137, 80, 78, 71, 13, 10, 26, 10];
        chunk(&mut p, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        chunk(&mut p, b"IDAT", &z);
        chunk(&mut p, b"IEND", &[]);
        p
    }
    #[test]
    fn palette_finds_both_colours() {
        let mut pixels = vec![0xffff0000; 2048];
        pixels.extend(vec![0xff0000ff; 2048]);
        let p = palette(&pixels, 64, 64);
        assert!(p.contains(&0xff0000));
        assert!(p.contains(&0x0000ff));
    }
    #[test]
    fn worker_delivers_current_and_thumbnail_and_joins() {
        let dir = std::env::temp_dir().join(format!(
            "photoview-worker-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.png");
        let n = dir.join("b.png");
        std::fs::write(&p, png()).unwrap();
        std::fs::write(&n, png()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let d = Decoder::new(move || {
            let _ = tx.send(());
        });
        d.want(&p, std::slice::from_ref(&n), std::slice::from_ref(&p));
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(d.picture(&p).is_some());
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while d.thumb(&p).is_none() && std::time::Instant::now() < end {
            rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        }
        let img = d.picture(&p).unwrap();
        assert_eq!((img.w, img.h), (2, 1));
        assert_eq!(img.px, [0xffff0000, 0xff0000ff]);
        let t = d.thumb(&p).unwrap();
        assert_eq!((t.w, t.h, t.px.len()), (92, 46, 92 * 46));
        drop(d);
        assert!(rx.recv_timeout(std::time::Duration::from_secs(1)).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
