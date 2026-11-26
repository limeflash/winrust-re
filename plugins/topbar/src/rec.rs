//! Screen recording: Windows.Graphics.Capture of the monitor, cropped to the area, into an MP4 (H.264 + AAC) through the
//! Media Foundation sink writer; the microphone is WASAPI on its own thread. Everything runs on threads of its own
//! (`rec-video`, `rec-microphone`); `Recorder` only holds a stop flag and the join handle.
use std::{path::PathBuf, sync::{Arc, atomic::{AtomicBool, AtomicU32, Ordering}, mpsc}, thread::{self, JoinHandle}, time::{Duration, Instant}};

mod writer;
mod capture;
mod mic;
mod runtime;

pub struct RecOptions {
    /// What to record, screen pixels (left, top, right, bottom); may span part of one monitor. Odd sizes lose their last pixel.
    pub area: (i32, i32, i32, i32),
    /// Mix in the default microphone.
    pub mic: bool,
    /// The .mp4 to write.
    pub out: PathBuf,
    /// Frames per second (30 or 60).
    pub fps: u32,
}

pub struct Recorder {
    out: PathBuf,
    started: Instant,
    stop: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    worker: Option<JoinHandle<Result<RecStats, String>>>,
}

/// What a finished recording contains.
#[derive(Default, Clone, Debug)]
pub struct RecStats {
    /// Pictures written to the file.
    pub frames: u64,
    /// Of them: the screen had not changed, the previous picture was written again.
    pub repeated: u64,
    /// Ticks of the frame clock that had to be skipped because the previous picture took longer than one frame
    /// (that picture then stays on screen for them in the file; the clock never stalls).
    pub dropped: u64,
    /// Length of the video (and of the sound, which is padded to it).
    pub duration: Duration,
    pub has_audio: bool,
    /// The microphone stopped working during the recording: the rest of the sound is silence.
    pub mic_lost: bool,
}

fn even_size(area: (i32, i32, i32, i32)) -> Result<(u32, u32), String> {
    let width = i64::from(area.2) - i64::from(area.0);
    let height = i64::from(area.3) - i64::from(area.1);
    if width < 2 || height < 2 || width > i64::from(u32::MAX) || height > i64::from(u32::MAX) {
        return Err("Recording area must be at least 2 by 2 pixels".into());
    }
    Ok(((width as u32) & !1, (height as u32) & !1))
}

impl Recorder {
    pub fn start(o: RecOptions) -> Result<Recorder, String> {
        let size = even_size(o.area)?;
        if o.fps != 30 && o.fps != 60 {
            return Err("Recording frame rate must be 30 or 60 fps".into());
        }
        let out = o.out.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_stop = stop.clone();
        let worker_level = level.clone();
        let worker = thread::Builder::new().name("rec-video".into()).spawn(move || runtime::run(o, size, worker_stop, worker_level, sender)).map_err(|e| e.to_string())?;
        match receiver.recv() {
            Ok(Ok(started)) => Ok(Self { out, started, stop, level, worker: Some(worker) }),
            // the worker has released everything and removed the file it had begun when it answered; join waits for that
            Ok(Err(error)) => { let _ = worker.join(); Err(error) },
            Err(error) => { let _ = worker.join(); Err(format!("Recording thread failed to start: {error}")) },
        }
    }

    /// Time since the first picture.
    pub fn elapsed(&self) -> Duration { self.started.elapsed() }
    /// The microphone's peak over the last ~50 ms, 0..1 (0 without a microphone).
    pub fn mic_level(&self) -> f32 { f32::from_bits(self.level.load(Ordering::Relaxed)) }
    /// Ends the recording and finishes the file.
    pub fn stop(self) -> Result<PathBuf, String> { self.stop_with_stats().map(|(path, _)| path) }
    /// `stop` plus what the file contains. On an error while recording the file is finished with what was recorded and kept.
    pub fn stop_with_stats(mut self) -> Result<(PathBuf, RecStats), String> {
        let stats = self.finish()?;
        Ok((self.out.clone(), stats))
    }
    fn finish(&mut self) -> Result<RecStats, String> {
        self.stop.store(true, Ordering::Release);
        match self.worker.take() {
            Some(worker) => worker.join().map_err(|_| "Recording thread panicked".to_string())?,
            None => Ok(RecStats::default()),
        }
    }
}

impl Drop for Recorder { fn drop(&mut self) { let _ = self.finish(); } }

pub fn default_path() -> PathBuf {
    use windows::Win32::{System::Com::CoTaskMemFree, UI::Shell::{FOLDERID_Videos, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath}};
    let videos = unsafe {
        match SHGetKnownFolderPath(&FOLDERID_Videos, KNOWN_FOLDER_FLAG(0), None) {
            Ok(path) => {
                let result = path.to_string().ok().map(PathBuf::from);
                CoTaskMemFree(Some(path.0.cast()));
                result
            }
            Err(_) => None,
        }
    }.unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Videos"));
    let dir = videos.join("Captures");
    let _ = std::fs::create_dir_all(&dir);
    let mut time = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut time) };
    recording_path(&videos, &time)
}

fn recording_path(videos: &std::path::Path, time: &windows_sys::Win32::Foundation::SYSTEMTIME) -> PathBuf {
    videos.join("Captures").join(format!("Recording {:04}-{:02}-{:02} {:02}{:02}{:02}.mp4", time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond))
}

/// `topbar.exe --rec-selftest <out.mp4> [mic]`: 3 s of the primary monitor's top-left 640x360 through the real capture path
/// (and the microphone only with `mic`); prints what was written and also saves it next to the video as <out.mp4>.txt.
pub fn selftest(args: &[String]) -> bool {
    use windows::Win32::{Foundation::POINT, Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY}};
    let Some(out) = args.first().map(PathBuf::from) else {
        println!("usage: topbar.exe --rec-selftest <out.mp4> [mic]");
        return false;
    };
    let mic = args.get(1).is_some_and(|a| a == "mic");
    let report = (|| -> Result<String, String> {
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !unsafe { GetMonitorInfoW(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY), &mut info) }.as_bool() { return Err("no primary monitor".into()); }
        let (left, top) = (info.rcMonitor.left, info.rcMonitor.top);
        let began = Instant::now();
        let recorder = Recorder::start(RecOptions { area: (left, top, left + 640, top + 360), mic, out, fps: 30 })?;
        let started_in = began.elapsed();
        let mut peak = 0.0f32;
        while recorder.elapsed() < Duration::from_secs(3) { peak = peak.max(recorder.mic_level()); thread::sleep(Duration::from_millis(20)); }
        let stopped = Instant::now();
        let (path, stats) = recorder.stop_with_stats()?;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(format!("file {}
size {size} bytes
frames written {}
repeated (screen unchanged) {}
dropped (late ticks) {}
video duration {:.3} s
start took {:.0} ms, stop took {:.0} ms
microphone {} (peak {:.2}{})",
            path.display(), stats.frames, stats.repeated, stats.dropped, stats.duration.as_secs_f64(), started_in.as_secs_f64() * 1000.0, stopped.elapsed().as_secs_f64() * 1000.0,
            if stats.has_audio { "recorded" } else { "off" }, peak, if stats.mic_lost { ", LOST during the recording" } else { "" }))
    })();
    let (ok, text) = match report { Ok(t) => (true, t), Err(e) => (false, format!("FAILED: {e}")) };
    println!("{text}");
    if let Some(out) = args.first() { let _ = std::fs::write(format!("{out}.txt"), &text); }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_sizes_are_even() {
        assert_eq!(even_size((10, 20, 651, 381)).unwrap(), (640, 360));
        assert!(even_size((0, 0, 1, 10)).is_err());
    }

    #[test]
    fn default_recording_path_shape() {
        let path = recording_path(std::path::Path::new("Videos"), &windows_sys::Win32::Foundation::SYSTEMTIME::default());
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "Captures");
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("Recording ") && name.ends_with(".mp4"));
        assert_eq!(name.len(), 31);
    }
}

