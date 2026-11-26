//! WASAPI shared-mode capture of the default microphone on its own thread: 48 kHz stereo 16-bit PCM packets stamped with the
//! device's QPC time (100 ns), plus the peak meter. The thread owns every COM object it makes and ends them before it exits.
use std::{sync::{Arc, atomic::{AtomicBool, AtomicU32, Ordering}, mpsc::{self, Receiver}}, thread::{self, JoinHandle}, time::{Duration, Instant}};
use windows::Win32::{Media::Audio::*, System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED}};

pub const RATE: i64 = 48_000;
pub const FRAME_BYTES: usize = 4; // stereo, 16 bit

pub struct Packet { pub qpc: u64, pub data: Vec<u8> }
pub struct Mic {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Unbounded: the recording thread drains it after every video frame, and a lost packet would be a hole in the sound.
    pub packets: Receiver<Result<Packet, String>>,
}

fn err<E: std::fmt::Display>(what: &'static str) -> impl FnOnce(E) -> String { move |e| format!("Microphone {what}: {e}") }

impl Mic {
    pub fn start(level: Arc<AtomicU32>) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (send, packets) = mpsc::channel();
        let (ready, init) = mpsc::sync_channel(1);
        let thread = thread::Builder::new().name("rec-microphone".into()).spawn(move || unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let started = std::cell::Cell::new(false);
            let result = (|| -> Result<(), String> {
                let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err("enumerator"))?;
                let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole).map_err(err("device"))?;
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(err("client"))?;
                let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 2, nSamplesPerSec: RATE as u32, nAvgBytesPerSec: RATE as u32 * FRAME_BYTES as u32, nBlockAlign: FRAME_BYTES as u16, wBitsPerSample: 16, cbSize: 0 };
                // the audio engine converts the device's own format to ours
                client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, 1_000_000, 0, &format, None).map_err(err("format"))?;
                let capture: IAudioCaptureClient = client.GetService().map_err(err("capture service"))?;
                client.Start().map_err(err("start"))?;
                if ready.send(Ok(())).is_err() { let _ = client.Stop(); return Ok(()); }
                started.set(true);
                let mut peak = 0.0f32;
                let mut last_meter = Instant::now();
                let outcome = (|| -> Result<(), String> {
                    while !flag.load(Ordering::Acquire) {
                        while capture.GetNextPacketSize().map_err(err("read"))? != 0 {
                            let mut ptr = std::ptr::null_mut();
                            let mut frames = 0;
                            let mut flags = 0;
                            let mut qpc = 0;
                            capture.GetBuffer(&mut ptr, &mut frames, &mut flags, None, Some(&mut qpc)).map_err(err("read"))?;
                            let mut data = vec![0u8; frames as usize * FRAME_BYTES];
                            if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 == 0 && !ptr.is_null() {
                                std::ptr::copy_nonoverlapping(ptr, data.as_mut_ptr(), data.len());
                                for s in data.as_chunks::<2>().0 { peak = peak.max(i16::from_le_bytes(*s).unsigned_abs() as f32 / 32768.0); }
                            }
                            capture.ReleaseBuffer(frames).map_err(err("read"))?;
                            let _ = send.send(Ok(Packet { qpc, data }));
                        }
                        if last_meter.elapsed() >= Duration::from_millis(50) {
                            level.store(peak.to_bits(), Ordering::Relaxed);
                            peak = 0.0;
                            last_meter = Instant::now();
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    Ok(())
                })();
                let _ = client.Stop();
                outcome
            })();
            if let Err(e) = result {
                // before the start was reported: start() returns it; after: the recording thread sees it among the packets
                if started.get() { let _ = send.send(Err(e)); } else { let _ = ready.send(Err(e)); }
            }
            level.store(0f32.to_bits(), Ordering::Relaxed);
            if com { CoUninitialize(); }
        }).map_err(|e| e.to_string())?;
        match init.recv() {
            Ok(Ok(())) => Ok(Self { stop, thread: Some(thread), packets }),
            Ok(Err(e)) => { let _ = thread.join(); Err(e) },
            Err(_) => { let _ = thread.join(); Err("Microphone thread ended during start".into()) },
        }
    }
    pub fn stop(&mut self) { self.stop.store(true, Ordering::Release); if let Some(thread) = self.thread.take() { let _ = thread.join(); } }
}
impl Drop for Mic { fn drop(&mut self) { self.stop(); } }
