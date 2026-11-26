//! The Media Foundation sink writer: H.264 video (hardware encoder when there is one) and optional AAC audio into one MP4.
//! Video comes in as top-down BGRA frames, audio as 48 kHz stereo 16-bit PCM; every sample carries its own time and duration.
use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{core::PCWSTR, Win32::{Graphics::Direct3D11::ID3D11Device, Media::MediaFoundation::*}};

pub struct Writer {
    writer: IMFSinkWriter,
    video: u32,
    audio: Option<u32>,
    width: usize,
    height: usize,
    finalized: bool,
}

fn pair(a: u32, b: u32) -> u64 { (u64::from(a) << 32) | u64::from(b) }

impl Writer {
    /// A hardware encoder through the DXGI device manager first, then, if that cannot be set up, Media Foundation's software
    /// encoder. A failed attempt leaves no file behind.
    pub unsafe fn new(path: &Path, width: u32, height: u32, fps: u32, audio: bool, device: Option<&ID3D11Device>) -> Result<Self, String> {
        let attempt = |device| Self::open(path, width, height, fps, audio, device).inspect_err(|_| { let _ = std::fs::remove_file(path); });
        match device {
            Some(device) => attempt(Some(device)).or_else(|_| attempt(None)),
            None => attempt(None),
        }.map_err(|e| format!("Cannot start the video encoder: {e}"))
    }

    pub(super) unsafe fn open(path: &Path, width: u32, height: u32, fps: u32, audio: bool, device: Option<&ID3D11Device>) -> windows::core::Result<Self> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 3)?;
        let attributes = attributes.unwrap();
        attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
        if let Some(device) = device {
            let mut token = 0;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
            let manager = manager.unwrap();
            manager.ResetDevice(device, token)?;
            attributes.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        }
        let writer = MFCreateSinkWriterFromURL(PCWSTR(wide.as_ptr()), None, &attributes)?;
        let output = MFCreateMediaType()?;
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        output.SetUINT32(&MF_MT_AVG_BITRATE, (u64::from(width) * u64::from(height) * u64::from(fps) / 3).clamp(2_000_000, 20_000_000) as u32)?;
        output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        output.SetUINT64(&MF_MT_FRAME_SIZE, pair(width, height))?;
        output.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1))?;
        output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
        let video = writer.AddStream(&output)?;
        let input = MFCreateMediaType()?;
        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        input.SetUINT64(&MF_MT_FRAME_SIZE, pair(width, height))?;
        input.SetUINT64(&MF_MT_FRAME_RATE, pair(fps, 1))?;
        input.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
        input.SetUINT32(&MF_MT_DEFAULT_STRIDE, width * 4)?; // positive: rows top-down, said outright because the converters disagree about the default
        writer.SetInputMediaType(video, &input, None)?;
        let audio = if audio {
            let out = MFCreateMediaType()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
            out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000)?;
            let stream = writer.AddStream(&out)?;
            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
            input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
            input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
            input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 192_000)?;
            writer.SetInputMediaType(stream, &input, None)?;
            Some(stream)
        } else { None };
        writer.BeginWriting()?;
        Ok(Self { writer, video, audio, width: width as usize, height: height as usize, finalized: false })
    }

    unsafe fn submit(&self, stream: u32, len: usize, time: i64, duration: i64, fill: impl FnOnce(*mut u8)) -> windows::core::Result<()> {
        let buffer = MFCreateMemoryBuffer(len.try_into().map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG))?)?;
        let mut data = std::ptr::null_mut();
        buffer.Lock(&mut data, None, None)?;
        fill(data);
        buffer.Unlock()?;
        buffer.SetCurrentLength(len as u32)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(time)?;
        sample.SetSampleDuration(duration)?;
        self.writer.WriteSample(stream, &sample)
    }
    /// One picture, top-down BGRA (width*height*4 bytes), shown from `time` for `duration` (100 ns units).
    pub unsafe fn video(&self, bgra: &[u8], time: i64, duration: i64) -> windows::core::Result<()> {
        if bgra.len() != self.width * self.height * 4 { return Err(windows::Win32::Foundation::E_INVALIDARG.into()); }
        self.submit(self.video, bgra.len(), time, duration, |data| std::ptr::copy_nonoverlapping(bgra.as_ptr(), data, bgra.len()))
    }
    /// PCM bytes (whole 4-byte frames) from `time` for `duration`.
    pub unsafe fn audio(&self, pcm: &[u8], time: i64, duration: i64) -> windows::core::Result<()> {
        let Some(stream) = self.audio else { return Err(windows::Win32::Foundation::E_UNEXPECTED.into()) };
        self.submit(stream, pcm.len(), time, duration, |data| std::ptr::copy_nonoverlapping(pcm.as_ptr(), data, pcm.len()))
    }
    pub unsafe fn finish(&mut self) -> windows::core::Result<()> {
        if self.finalized { return Ok(()); }
        self.finalized = true;
        self.writer.Finalize()
    }
}
impl Drop for Writer { fn drop(&mut self) { unsafe { let _ = self.finish(); } } }

/// The file's duration as Media Foundation's source reader reports it (100 ns).
#[cfg(test)]
pub(super) unsafe fn media_duration(path: &Path) -> u64 {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    reader_duration(&MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), None).unwrap())
}
#[cfg(test)]
unsafe fn reader_duration(reader: &IMFSourceReader) -> u64 {
    let duration = reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION).unwrap();
    assert_eq!(duration.Anonymous.Anonymous.vt, windows::Win32::System::Variant::VT_UI8);
    duration.Anonymous.Anonymous.Anonymous.uhVal
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{core::Interface, Win32::{Foundation::HMODULE, Graphics::{Direct3D::D3D_DRIVER_TYPE_HARDWARE, Direct3D11::*}, System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED}}};

    struct Temp(std::path::PathBuf);
    impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }

    unsafe fn device() -> ID3D11Device {
        let (mut device, mut context) = (None, None);
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)).unwrap();
        let _ = context.unwrap().cast::<ID3D11Multithread>().unwrap().SetMultithreadProtected(true);
        device.unwrap()
    }

    /// 2 s at 30 fps, 640x360: white-ish moving stripes on the upper half, black lower half, a 440 Hz sine; then reads the file back.
    unsafe fn round_trip(hardware: bool) {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        MFStartup(MF_VERSION, MFSTARTUP_FULL).unwrap();
        let path = Temp(std::env::temp_dir().join(format!("winrust-rec-test-{}-{hardware}.mp4", std::process::id())));
        {
            let device = hardware.then(|| device());
            let mut writer = Writer::open(&path.0, 640, 360, 30, true, device.as_ref()).unwrap();
            let mut frame = vec![0u8; 640 * 360 * 4];
            for n in 0..60usize {
                for y in 0..180usize {
                    for x in 0..640usize {
                        let p = (y * 640 + x) * 4;
                        frame[p..p + 4].copy_from_slice(&[((x + n * 3) % 256) as u8, 255, 255, 255]);
                    }
                }
                let start = n as i64 * 10_000_000 / 30;
                writer.video(&frame, start, (n as i64 + 1) * 10_000_000 / 30 - start).unwrap();
                let mut samples = Vec::with_capacity(1600 * 4);
                for i in 0..1600 {
                    let s = (((n * 1600 + i) as f32 * std::f32::consts::TAU * 440.0 / 48_000.0).sin() * 10000.0) as i16;
                    samples.extend_from_slice(&s.to_le_bytes());
                    samples.extend_from_slice(&s.to_le_bytes());
                }
                writer.audio(&samples, start, 1600 * 10_000_000 / 48_000).unwrap();
            }
            writer.finish().unwrap();
        }
        assert!(std::fs::metadata(&path.0).unwrap().len() > 5000);
        let wide: Vec<u16> = path.0.as_os_str().encode_wide().chain(Some(0)).collect();
        let reader = MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), None).unwrap();
        let (video, audio) = (MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32);
        let native = reader.GetNativeMediaType(video, 0).unwrap();
        assert_eq!(native.GetGUID(&MF_MT_SUBTYPE).unwrap(), MFVideoFormat_H264);
        assert_eq!(native.GetUINT64(&MF_MT_FRAME_SIZE).unwrap(), pair(640, 360));
        assert_eq!(reader.GetNativeMediaType(audio, 0).unwrap().GetGUID(&MF_MT_SUBTYPE).unwrap(), MFAudioFormat_AAC);
        let value = reader_duration(&reader);
        assert!((18_000_000..=22_000_000).contains(&value), "duration {value}");
        // the first picture decoded to NV12 (top-down): the bright half must be on top
        let nv12 = MFCreateMediaType().unwrap();
        nv12.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).unwrap();
        nv12.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).unwrap();
        reader.SetCurrentMediaType(video, None, &nv12).unwrap();
        let (mut sample, mut flags) = (None, 0);
        reader.ReadSample(video, 0, None, Some(&mut flags), None, Some(&mut sample)).unwrap();
        let buffer = sample.expect("a decoded frame").ConvertToContiguousBuffer().unwrap();
        let (mut data, mut len) = (std::ptr::null_mut(), 0);
        buffer.Lock(&mut data, None, Some(&mut len)).unwrap();
        let luma = std::slice::from_raw_parts(data, len as usize);
        let (top, bottom) = (luma[90 * 640 + 300], luma[270 * 640 + 300]);
        buffer.Unlock().unwrap();
        assert!(top > 150 && bottom < 60, "top {top}, bottom {bottom}: the picture is upside down");
        drop((buffer, reader));
        MFShutdown().unwrap();
        CoUninitialize();
    }

    #[test]
    fn synthetic_video_and_audio_round_trip_hardware_path() { unsafe { round_trip(true) } }
    #[test]
    fn synthetic_video_and_audio_round_trip_software_path() { unsafe { round_trip(false) } }

    #[test]
    fn a_failed_start_leaves_no_file() {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            MFStartup(MF_VERSION, MFSTARTUP_FULL).unwrap();
            let path = std::env::temp_dir().join("winrust-rec-no-such-dir").join("x.mp4");
            assert!(Writer::new(&path, 640, 360, 30, false, None).is_err());
            assert!(!path.exists());
            // an odd size is not accepted by the encoder; the writer must say so instead of writing garbage
            let odd = Temp(std::env::temp_dir().join(format!("winrust-rec-odd-{}.mp4", std::process::id())));
            assert!(Writer::new(&odd.0, 641, 359, 30, false, None).is_err());
            assert!(!odd.0.exists());
            MFShutdown().unwrap();
            CoUninitialize();
        }
    }
}
