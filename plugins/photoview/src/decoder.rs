//! The decoding threads: one makes pictures (the shown one first, then its neighbours), one makes the filmstrip's thumbnails, so a
//! 24 MP photo being decoded never keeps a thumbnail waiting. The window asks with [`Decoder::want`], is told with the `ready`
//! callback (it posts a message: the callback runs on a worker), and picks a finished picture up with [`Decoder::take`]; from then on
//! the pixels are the window's (it puts them on the GPU and lets go of them), and the decoder does not keep a copy.
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};

/// What a picture's file says about it.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub width: u32,
    pub height: u32,
    pub taken: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub exposure: Option<String>,
    pub iso: Option<u32>,
}

/// A picture as it comes off a worker: premultiplied BGRA, upright.
pub struct Decoded {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
    pub meta: Meta,
    /// The five colours that make up the picture, `0xRRGGBB`, the most common first.
    pub palette: [u32; 5],
    /// A copy of at most 64 px on the long side: what the ambient light behind the picture is blurred from.
    pub small: crate::decode::Small,
}

/// A filmstrip thumbnail: cut to the cell of the design for the picture's shape.
pub struct Thumb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

/// How many thumbnails are kept (one is at most 150 KB).
const THUMBS: usize = 160;

#[derive(Default)]
struct State {
    stopped: bool,
    /// The shown picture, then the ones next to it: the order they are made in.
    pictures: Vec<PathBuf>,
    /// The thumbnails the strip shows now, the shown one's first.
    strip: Vec<PathBuf>,
    /// Made, not picked up yet.
    ready: HashMap<PathBuf, Decoded>,
    /// Picked up: the window holds the pixels.
    delivered: HashSet<PathBuf>,
    errors: HashMap<PathBuf, String>,
    thumbs: HashMap<PathBuf, Arc<Thumb>>,
    order: VecDeque<PathBuf>,
    bad_thumbs: HashSet<PathBuf>,
}

pub struct Decoder {
    shared: Arc<(Mutex<State>, Condvar)>,
    workers: Vec<JoinHandle<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Picture,
    Thumb,
}

impl Decoder {
    /// `ready` is called on a worker thread whenever a picture, a thumbnail or an error is there to be asked for.
    pub fn new(ready: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let ready = Arc::new(ready);
        let workers = [Kind::Picture, Kind::Thumb]
            .into_iter()
            .map(|kind| {
                let (shared, ready) = (shared.clone(), ready.clone());
                thread::spawn(move || work(kind, &shared, &*ready))
            })
            .collect();
        Self { shared, workers }
    }

    /// What is wanted now: the shown picture, those next to it, and the thumbnails of the strip. What is not in these lists any
    /// more is forgotten (pictures the window holds are its own business).
    pub fn want(&self, current: &Path, near: &[PathBuf], strip: &[PathBuf]) {
        let mut st = self.shared.0.lock().unwrap();
        st.pictures = std::iter::once(current.to_path_buf()).chain(near.iter().filter(|p| p.as_path() != current).take(2).cloned()).collect();
        st.strip = strip.to_vec();
        let pics: HashSet<_> = st.pictures.iter().cloned().collect();
        let strip: HashSet<_> = st.strip.iter().cloned().collect();
        st.ready.retain(|p, _| pics.contains(p));
        st.delivered.retain(|p| pics.contains(p));
        st.errors.retain(|p, _| pics.contains(p));
        st.bad_thumbs.retain(|p| strip.contains(p));
        self.shared.1.notify_all();
    }

    /// A finished picture, for the window to keep (once: the decoder does not make it again while it is wanted).
    pub fn take(&self, p: &Path) -> Option<Decoded> {
        let mut st = self.shared.0.lock().unwrap();
        let d = st.ready.remove(p)?;
        st.delivered.insert(p.to_path_buf());
        Some(d)
    }

    /// The picture could not be decoded: why, as the codec says it.
    pub fn error(&self, p: &Path) -> Option<String> {
        self.shared.0.lock().unwrap().errors.get(p).cloned()
    }

    pub fn thumb(&self, p: &Path) -> Option<Arc<Thumb>> {
        self.shared.0.lock().unwrap().thumbs.get(p).cloned()
    }

    /// The thumbnail could not be made (the picture is not one).
    pub fn thumb_failed(&self, p: &Path) -> bool {
        self.shared.0.lock().unwrap().bad_thumbs.contains(p)
    }

    /// The window lost the picture it held (the GPU was reset): it is made again.
    pub fn redo(&self, p: &Path) {
        let mut st = self.shared.0.lock().unwrap();
        st.delivered.remove(p);
        self.shared.1.notify_all();
    }

    /// The window forgets the picture (a deleted one): it is not made again if it was asked for.
    pub fn forget(&self, p: &Path) {
        let mut st = self.shared.0.lock().unwrap();
        st.ready.remove(p);
        st.thumbs.remove(p);
        st.pictures.retain(|q| q != p);
        st.strip.retain(|q| q != p);
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().stopped = true;
        self.shared.1.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

/// What a worker of `kind` does next, if anything.
fn next(kind: Kind, st: &State) -> Option<PathBuf> {
    match kind {
        Kind::Picture => st.pictures.iter().find(|p| !st.ready.contains_key(*p) && !st.delivered.contains(*p) && !st.errors.contains_key(*p)).cloned(),
        Kind::Thumb => st.strip.iter().find(|p| !st.thumbs.contains_key(*p) && !st.bad_thumbs.contains(*p)).cloned(),
    }
}

fn work(kind: Kind, shared: &(Mutex<State>, Condvar), ready: &(dyn Fn() + Send + Sync)) {
    crate::decode::com_init();
    let (lock, cv) = shared;
    loop {
        let path = {
            let mut st = lock.lock().unwrap();
            loop {
                if st.stopped {
                    return;
                }
                if let Some(p) = next(kind, &st) {
                    break p;
                }
                st = cv.wait(st).unwrap();
            }
        };
        let made = match kind {
            Kind::Picture => crate::decode::decode_model(&path).map(Made::Picture),
            Kind::Thumb => crate::decode::decode_thumb(&path).map(Made::Thumb),
        };
        let mut st = lock.lock().unwrap();
        if st.stopped {
            return;
        }
        // asked for something else while this was being made: the result is thrown away (the picture may come back to the list)
        let wanted = match kind {
            Kind::Picture => st.pictures.contains(&path),
            Kind::Thumb => st.strip.contains(&path),
        };
        if wanted {
            match made {
                Ok(Made::Picture(d)) => {
                    st.ready.insert(path, d);
                }
                Ok(Made::Thumb(t)) => {
                    st.order.retain(|p| p != &path);
                    st.order.push_back(path.clone());
                    st.thumbs.insert(path, Arc::new(t));
                    let mut turns = st.order.len();
                    while st.order.len() > THUMBS && turns > 0 {
                        turns -= 1;
                        if let Some(p) = st.order.pop_front() {
                            if st.strip.contains(&p) {
                                st.order.push_back(p); // on show now: the oldest of the others goes instead
                            } else {
                                st.thumbs.remove(&p);
                            }
                        }
                    }
                }
                Err(e) => match kind {
                    Kind::Picture => {
                        st.errors.insert(path, e.message());
                    }
                    Kind::Thumb => {
                        st.bad_thumbs.insert(path);
                    }
                },
            }
        }
        drop(st);
        ready();
    }
}

enum Made {
    Picture(Decoded),
    Thumb(Thumb),
}

/// Five colours of a picture by median cut over a 64 × 64 sample of it: `0xRRGGBB`, the colours of the biggest groups first.
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
                if *range == 0 { None } else { Some((i, axis, *range * b.len() as u32)) }
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
pub(crate) mod tests {
    use super::*;

    /// A PNG of `w` × 1 pixels: the first red, the rest blue (an uncompressed deflate block: no codec to trust but WIC's own reader).
    pub(crate) fn png(w: u32) -> Vec<u8> {
        fn chunk(out: &mut Vec<u8>, ty: &[u8], data: &[u8]) {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = ty.to_vec();
            body.extend(data);
            out.extend(&body);
            let mut c = !0u32;
            for b in body {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            out.extend((!c).to_be_bytes());
        }
        let mut raw = vec![0u8, 255, 0, 0, 255];
        for _ in 1..w {
            raw.extend([0, 0, 255, 255]);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for x in &raw {
            a = (a + *x as u32) % 65521;
            b = (b + a) % 65521;
        }
        let mut z = vec![0x78, 0x01, 0x01, raw.len() as u8, 0, !(raw.len() as u8), 255];
        z.extend(&raw);
        z.extend(((b << 16) | a).to_be_bytes());
        let mut p = vec![137, 80, 78, 71, 13, 10, 26, 10];
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend([0, 0, 0, 1, 8, 6, 0, 0, 0]);
        chunk(&mut p, b"IHDR", &ihdr);
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
        assert_eq!(palette(&[0xff102030; 4], 2, 2)[0], 0x102030, "one flat colour is one colour");
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photoview-worker-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait_for(d: &Decoder, rx: &std::sync::mpsc::Receiver<()>, mut done: impl FnMut(&Decoder) -> bool) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !done(d) {
            assert!(std::time::Instant::now() < end, "the worker did not deliver in 15 s");
            let _ = rx.recv_timeout(std::time::Duration::from_millis(200));
        }
    }

    #[test]
    fn the_workers_deliver_the_picture_once_and_a_thumbnail_and_join() {
        let dir = temp("deliver");
        let (a, b, bad) = (dir.join("a.png"), dir.join("b.png"), dir.join("bad.png"));
        std::fs::write(&a, png(2)).unwrap();
        std::fs::write(&b, png(2)).unwrap();
        std::fs::write(&bad, b"not a picture").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let d = Decoder::new(move || {
            let _ = tx.lock().unwrap().send(());
        });
        d.want(&a, std::slice::from_ref(&b), &[a.clone(), bad.clone()]);
        wait_for(&d, &rx, |d| d.thumb(&a).is_some() && d.thumb_failed(&bad));
        wait_for(&d, &rx, |d| d.error(&a).is_none() && d.shared.0.lock().unwrap().ready.contains_key(&b));
        let img = d.take(&a).expect("the shown picture is there");
        assert_eq!((img.w, img.h, img.px.as_slice()), (2, 1, [0xffff0000u32, 0xff0000ff].as_slice()));
        assert_eq!((img.meta.width, img.meta.height), (2, 1));
        assert!(d.take(&a).is_none(), "a picture is handed over once");
        d.want(&a, std::slice::from_ref(&b), std::slice::from_ref(&a));
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(d.take(&a).is_none(), "and not made again while it is wanted");
        let t = d.thumb(&a).unwrap();
        assert_eq!((t.w, t.h, t.px.len()), (147, 92, 147 * 92), "a 2:1 picture: the cell is capped at 1.6");
        assert!(d.thumb(&bad).is_none() && d.error(&bad).is_none());

        // a file that is not a picture, asked as one: an error, and the thumbnail is not tried again
        d.want(&bad, &[], &[]);
        wait_for(&d, &rx, |d| d.error(&bad).is_some());
        drop(d); // joins both workers
        std::fs::remove_dir_all(dir).unwrap();
    }
}
