//! The recording thread: owns COM/Media Foundation, the capture, the encoder and the microphone thread, keeps the frame
//! clock, and ends all of them in the reverse order whichever way it exits.
use std::{sync::{Arc, atomic::{AtomicBool, AtomicU32, Ordering}, mpsc::SyncSender}, thread, time::{Duration, Instant}};
use windows::Win32::{Media::MediaFoundation::{MFStartup, MFShutdown, MF_VERSION, MFSTARTUP_FULL}, System::{Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED}, Performance::{QueryPerformanceCounter, QueryPerformanceFrequency}}};
use super::{RecOptions, RecStats, capture::Capture, mic::{FRAME_BYTES, Mic, RATE}, writer::Writer};

const HNS: i64 = 10_000_000; // Media Foundation time unit, 100 ns

struct Com;
impl Drop for Com { fn drop(&mut self) { unsafe { CoUninitialize() } } }
struct MediaFoundation;
impl Drop for MediaFoundation { fn drop(&mut self) { unsafe { let _ = MFShutdown(); } } }

pub fn run(options: RecOptions, size: (u32, u32), stop: Arc<AtomicBool>, level: Arc<AtomicU32>, ready: SyncSender<Result<Instant, String>>) -> Result<RecStats, String> {
    let mut ready = Some(ready);
    let result = unsafe {
        (|| -> Result<RecStats, String> {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().map_err(|e| e.to_string())?;
            let _com = Com;
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| e.to_string())?;
            let _mf = MediaFoundation;
            run_inner(&options, size, &stop, level, &mut ready)
        })()
    };
    // an error before the clock started is the answer of Recorder::start
    if let (Some(ready), Err(error)) = (ready, &result) { let _ = ready.send(Err(error.clone())); }
    result
}

/// Puts the microphone's packets on the recording's timeline (sample index 0 = the instant of the first picture).
/// The device's own clock drifts a little against the system clock and a packet's QPC stamp jitters, so a packet is
/// appended where the previous one ended; only a gap or overlap beyond `TOLERANCE` (lost packets, a stalled device)
/// is made good with silence or by dropping samples. Sound and picture therefore never disagree by more than that.
#[derive(Default)]
pub struct Aligner { base: i64, pcm: Vec<u8>, started: bool }
const TOLERANCE: i64 = RATE * 30 / 1000;
impl Aligner {
    fn end(&self) -> i64 { self.base + (self.pcm.len() / FRAME_BYTES) as i64 }
    fn silence(&mut self, frames: i64) { self.pcm.resize(self.pcm.len() + frames as usize * FRAME_BYTES, 0); }
    /// `start`: the packet's first sample on the timeline, from its QPC stamp.
    pub fn push(&mut self, start: i64, data: &[u8]) {
        let frames = (data.len() / FRAME_BYTES) as i64;
        let mut skip = 0;
        if !self.started {
            if start + frames <= 0 { return; } // recorded before the first picture
            self.started = true;
            if start < 0 { skip = -start; } else { self.silence(start); }
        } else {
            let gap = start - self.end();
            if gap > TOLERANCE { self.silence(gap); } else if gap < -TOLERANCE { skip = (-gap).min(frames); }
        }
        self.pcm.extend_from_slice(&data[skip as usize * FRAME_BYTES..frames as usize * FRAME_BYTES]);
    }
    /// Silence up to `end` (the recording's last sample) when the microphone gave less.
    pub fn pad_to(&mut self, end: i64) { if end > self.end() { self.silence(end - self.end()); } }
    /// What is ready below sample `limit`, with the index of its first sample.
    pub fn take(&mut self, limit: i64) -> Option<(i64, Vec<u8>)> {
        let frames = (limit.min(self.end()) - self.base).max(0) as usize;
        if frames == 0 { return None; }
        let base = self.base;
        let rest = self.pcm.split_off(frames * FRAME_BYTES);
        let taken = std::mem::replace(&mut self.pcm, rest);
        self.base += frames as i64;
        Some((base, taken))
    }
}

struct Audio { mic: Option<Mic>, aligner: Aligner, origin: i64, failed: bool }
impl Audio {
    fn collect(&mut self) {
        let Some(mic) = &self.mic else { return };
        while let Ok(packet) = mic.packets.try_recv() {
            match packet {
                Ok(p) => {
                    let start = ((i128::from(p.qpc) - i128::from(self.origin)) * RATE as i128).div_euclid(10_000_000) as i64;
                    self.aligner.push(start, &p.data);
                }
                Err(_) => self.failed = true, // the device went away: the picture goes on, the rest of the sound is silence
            }
        }
    }
    unsafe fn write(&mut self, writer: &Writer, limit: i64) -> Result<(), String> {
        if let Some((base, pcm)) = self.aligner.take(limit) {
            let frames = (pcm.len() / FRAME_BYTES) as i64;
            let time = base * HNS / RATE;
            writer.audio(&pcm, time, (base + frames) * HNS / RATE - time).map_err(|e| format!("Cannot write the sound: {e}"))?;
        }
        Ok(())
    }
}

unsafe fn run_inner(options: &RecOptions, size: (u32, u32), stop: &AtomicBool, level: Arc<AtomicU32>, ready: &mut Option<SyncSender<Result<Instant, String>>>) -> Result<RecStats, String> {
    let fps = i64::from(options.fps);
    let mut pixels = vec![0u8; (size.0 as usize).checked_mul(size.1 as usize).and_then(|v| v.checked_mul(4)).ok_or("Recording area is too large")?];
    // Declared in the order they are released backwards: encoder first (finalized), then the capture and its D3D device.
    let capture = Capture::new(options.area, size.0, size.1)?;
    let mut writer = Writer::new(&options.out, size.0, size.1, options.fps, options.mic, Some(&capture.device))?;
    let started = (|| -> Result<(Option<Mic>, i64, Instant), String> {
        let mic = if options.mic { Some(Mic::start(level)?) } else { None };
        // the first picture before the clock starts, so the file does not open on a black frame
        let wait = Instant::now();
        while !capture.update(&mut pixels)? && wait.elapsed() < Duration::from_millis(1000) { thread::sleep(Duration::from_millis(10)); }
        let (mut counter, mut frequency) = (0, 0);
        QueryPerformanceFrequency(&mut frequency).map_err(|e| e.to_string())?;
        QueryPerformanceCounter(&mut counter).map_err(|e| e.to_string())?;
        Ok((mic, (i128::from(counter) * 10_000_000 / i128::from(frequency)) as i64, Instant::now()))
    })();
    let (mic, origin, start) = match started {
        Ok(v) => v,
        Err(e) => { drop(writer); let _ = std::fs::remove_file(&options.out); return Err(e); } // `mic`, if it was made, ended with the closure
    };
    if ready.take().unwrap().send(Ok(start)).is_err() { drop(writer); let _ = std::fs::remove_file(&options.out); return Err("Recorder was dropped during start".into()); }

    let mut audio = Audio { mic, aligner: Aligner::default(), origin, failed: false };
    let mut stats = RecStats { has_audio: options.mic, ..Default::default() };
    let (next, result) = pump(&Clock { fps, start, stop }, &mut pixels, &mut |p| capture.update(p), &writer, &mut audio, &mut stats);
    // Everything below runs on success and on error alike: the file is closed with what was recorded.
    if let Some(mic) = &mut audio.mic { mic.stop(); }
    audio.collect();
    let tail = if options.mic {
        audio.aligner.pad_to(next * RATE / fps);
        audio.write(&writer, next * RATE / fps)
    } else { Ok(()) };
    let finalized = writer.finish().map_err(|e| format!("Cannot finish the file: {e}"));
    stats.duration = Duration::from_nanos((next * HNS / fps * 100) as u64);
    stats.mic_lost = audio.failed;
    result.and(tail).and(finalized).map(|()| stats)
}

/// The frame clock. Picture k is for the instant k/fps after `start`; each pass writes one picture, shown until the tick
/// the clock has reached, so the file's timeline is the wall clock and a slow pass never stalls or shifts the sound.
/// Returns the tick the file ends at (the number of ticks written) and how the loop ended.
/// Frame rate, the instant of picture 0 and the stop flag.
struct Clock<'a> { fps: i64, start: Instant, stop: &'a AtomicBool }
unsafe fn pump(clock: &Clock, pixels: &mut [u8], grab: &mut dyn FnMut(&mut [u8]) -> Result<bool, String>, writer: &Writer, audio: &mut Audio, stats: &mut RecStats) -> (i64, Result<(), String>) {
    let Clock { fps, start, stop } = *clock;
    let tick = |k: i64| k * HNS / fps; // the time of picture k
    let mut next: i64 = 0; // the tick the next picture is for
    let result = (|| -> Result<(), String> {
        loop {
            if stop.load(Ordering::Acquire) && stats.frames > 0 { return Ok(()); }
            let now = (start.elapsed().as_nanos() * fps as u128 / 1_000_000_000) as i64;
            if now < next {
                let due = Duration::from_nanos((next as u128 * 1_000_000_000 / fps as u128) as u64);
                thread::sleep(due.saturating_sub(start.elapsed()).min(Duration::from_millis(5)));
                continue;
            }
            // `now - next` ticks passed while the previous picture was being made: that picture is shown for them too
            // (no stall, the timeline stays the wall clock) and they are counted as dropped
            if !grab(pixels)? { stats.repeated += 1; }
            stats.dropped += (now - next) as u64;
            writer.video(pixels, tick(next), tick(now + 1) - tick(next)).map_err(|e| format!("Cannot write the video: {e}"))?;
            stats.frames += 1;
            next = now + 1;
            audio.collect();
            audio.write(writer, next * RATE / fps)?;
        }
    })();
    (next, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pcm(frames: usize, v: u8) -> Vec<u8> { vec![v; frames * FRAME_BYTES] }

    #[test]
    fn audio_before_the_first_picture_is_cut_and_a_late_start_is_filled() {
        let mut a = Aligner::default();
        a.push(-2000, &pcm(1000, 1)); // entirely before
        assert!(a.take(10_000).is_none());
        a.push(-100, &pcm(480, 2)); // 100 samples before, 380 after
        a.push(380, &pcm(480, 3));
        let (base, data) = a.take(10_000).unwrap();
        assert_eq!((base, data.len() / FRAME_BYTES), (0, 860));
        assert_eq!((data[0], data[379 * 4], data[380 * 4]), (2, 2, 3));
        let mut late = Aligner::default();
        late.push(480, &pcm(480, 5)); // the first packet arrives half a frame in: silence before it
        let (_, data) = late.take(10_000).unwrap();
        assert_eq!((data.len() / FRAME_BYTES, data[0], data[480 * 4]), (960, 0, 5));
    }

    #[test]
    fn jitter_is_ignored_gaps_and_overlaps_are_repaired() {
        let mut a = Aligner::default();
        a.push(0, &pcm(480, 1));
        a.push(500, &pcm(480, 1)); // 20 samples late: jitter, appended as is
        assert_eq!(a.end(), 960);
        a.push(960 + 4800, &pcm(480, 2)); // 100 ms lost: silence
        assert_eq!(a.end(), 960 + 4800 + 480);
        a.push(a.end() - 4800, &pcm(480, 3)); // 100 ms back: dropped entirely
        assert_eq!(a.end(), 960 + 4800 + 480);
    }

    #[test]
    fn take_respects_the_limit_and_pad_fills_the_end() {
        let mut a = Aligner::default();
        a.push(0, &pcm(1000, 1));
        let (b0, d0) = a.take(600).unwrap();
        let (b1, d1) = a.take(600).unwrap_or((600, vec![]));
        assert_eq!((b0, d0.len() / 4, b1, d1.len()), (0, 600, 600, 0));
        a.pad_to(1600);
        let (b, d) = a.take(1600).unwrap();
        assert_eq!((b, d.len() / 4), (600, 1000));
        assert_eq!((d[0], d[399 * 4], d[400 * 4]), (1, 1, 0));
    }

    /// The clock on generated pictures (no capture, no microphone): a slow pass is counted and bridged, an unchanged screen
    /// repeats the picture, `stop` ends it within a frame, every tick is either written or skipped, and the file lasts as long.
    #[test]
    fn the_frame_clock_repeats_bridges_slow_passes_and_stops() {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            MFStartup(MF_VERSION, MFSTARTUP_FULL).unwrap();
            let path = std::env::temp_dir().join(format!("winrust-rec-clock-{}.mp4", std::process::id()));
            let mut writer = Writer::new(&path, 320, 180, 30, false, None).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stopper = stop.clone();
            let ender = thread::spawn(move || { thread::sleep(Duration::from_millis(1200)); stopper.store(true, Ordering::Release); });
            let mut calls = 0u32;
            let mut grab = |p: &mut [u8]| -> Result<bool, String> {
                calls += 1;
                if calls == 10 { thread::sleep(Duration::from_millis(150)); } // a slow pass: 4 ticks pass meanwhile
                for (i, b) in p.iter_mut().enumerate() { *b = (i as u32 + calls * 7) as u8; }
                Ok(!calls.is_multiple_of(3))
            };
            let mut audio = Audio { mic: None, aligner: Aligner::default(), origin: 0, failed: false };
            let mut stats = RecStats::default();
            let mut pixels = vec![0u8; 320 * 180 * 4];
            let (next, result) = pump(&Clock { fps: 30, start: Instant::now(), stop: &stop }, &mut pixels, &mut grab, &writer, &mut audio, &mut stats);
            ender.join().unwrap();
            result.unwrap();
            writer.finish().unwrap();
            drop(writer);
            let hns = super::super::writer::media_duration(&path);
            let _ = std::fs::remove_file(&path);
            assert_eq!(stats.frames + stats.dropped, next as u64, "{stats:?}");
            assert!((34..=38).contains(&next), "ticks {next}");
            assert!(stats.dropped >= 3 && stats.dropped <= 6, "{stats:?}");
            assert!(stats.repeated >= 8, "{stats:?}");
            let expected = next as u64 * 10_000_000 / 30;
            assert!(hns * 10 >= expected * 9 && hns * 10 <= expected * 11, "file {hns}, expected {expected}");
            MFShutdown().unwrap();
            CoUninitialize();
        }
    }
}
