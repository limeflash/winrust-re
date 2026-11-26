//! Windows.Graphics.Capture of one monitor, cropped to the recording area on the GPU and read back as top-down BGRA.
use windows::{core::{factory, Interface}, Graphics::{Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession}, DirectX::{DirectXPixelFormat, Direct3D11::IDirect3DDevice}}, Win32::{Foundation::{HMODULE, RECT}, Graphics::{Direct3D::D3D_DRIVER_TYPE_HARDWARE, Direct3D11::*, Dxgi::{IDXGIDevice, Common::*}, Gdi::*}, System::WinRT::{Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess}, Graphics::Capture::IGraphicsCaptureItemInterop}}};

pub struct Capture {
    pub device: ID3D11Device,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    context: ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    area: D3D11_BOX,
    width: usize,
    height: usize,
}

fn err<E: std::fmt::Display>(what: &'static str) -> impl FnOnce(E) -> String { move |e| format!("{what}: {e}") }

impl Capture {
    pub unsafe fn new(area: (i32, i32, i32, i32), width: u32, height: u32) -> Result<Self, String> {
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) { return Err("Screen capture is not supported on this system".into()); }
        let rect = RECT { left: area.0, top: area.1, right: area.2, bottom: area.3 };
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONULL);
        if monitor.0.is_null() { return Err("Recording area is not on a monitor".into()); }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() { return Err("Cannot inspect recording monitor".into()); }
        let bounds = info.rcMonitor;
        if rect.left < bounds.left || rect.top < bounds.top || rect.right > bounds.right || rect.bottom > bounds.bottom {
            return Err("Recording area must fit within one monitor".into());
        }
        let mut device = None;
        let mut context = None;
        // VIDEO_SUPPORT: the same device goes to Media Foundation's DXGI device manager for a hardware encoder
        let mut made = D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context));
        if made.is_err() {
            made = D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context));
        }
        made.map_err(err("Direct3D device"))?;
        let device = device.ok_or("No D3D11 device")?;
        let context = context.ok_or("No D3D11 context")?;
        // the capture thread and Media Foundation's encoder threads share this context
        let multithread: ID3D11Multithread = context.cast().map_err(err("Direct3D multithread"))?;
        let _ = multithread.SetMultithreadProtected(true);
        let dxgi: IDXGIDevice = device.cast().map_err(err("DXGI device"))?;
        let winrt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi).and_then(|d| d.cast()).map_err(err("WinRT Direct3D device"))?;
        let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(err("Capture interop"))?;
        let item: GraphicsCaptureItem = interop.CreateForMonitor(monitor).map_err(err("Capture item"))?;
        let size = item.Size().map_err(err("Capture item size"))?;
        let left = (rect.left - bounds.left) as u32;
        let top = (rect.top - bounds.top) as u32;
        if i64::from(left) + i64::from(width) > i64::from(size.Width) || i64::from(top) + i64::from(height) > i64::from(size.Height) {
            return Err("Recording area is larger than the captured monitor".into());
        }
        let desc = D3D11_TEXTURE2D_DESC { Width: width, Height: height, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_B8G8R8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_STAGING, CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32, ..Default::default() };
        let mut staging = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging)).map_err(err("Staging texture"))?;
        let staging = staging.ok_or("Cannot create staging texture")?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).map_err(err("Capture frame pool"))?;
        let session = match pool.CreateCaptureSession(&item) {
            Ok(session) => session,
            Err(e) => { let _ = pool.Close(); return Err(format!("Capture session: {e}")); }
        };
        // from here the Drop of `capture` closes the session and the pool on any failure
        let capture = Self { device, pool, session, context, staging, area: D3D11_BOX { left, top, front: 0, right: left + width, bottom: top + height, back: 1 }, width: width as usize, height: height as usize };
        let _ = capture.session.SetIsBorderRequired(false); // only honoured when the user allowed it; the border is not recorded anyway
        capture.session.StartCapture().map_err(err("Start capture"))?;
        Ok(capture)
    }

    /// Copies the newest captured frame into `pixels` (top-down BGRA, width*height*4 bytes).
    /// Ok(false): the screen has not changed since the last call, `pixels` keeps the previous picture.
    pub unsafe fn update(&self, pixels: &mut [u8]) -> Result<bool, String> {
        let mut latest = None;
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            if let Some(old) = latest.replace(frame) { let _ = old.Close(); } // the pool has two buffers: give the older one back at once
        }
        let Some(frame) = latest else { return Ok(false) };
        let result = self.copy(&frame, pixels);
        let _ = frame.Close();
        result
    }

    unsafe fn copy(&self, frame: &Direct3D11CaptureFrame, pixels: &mut [u8]) -> Result<bool, String> {
        let content = frame.ContentSize().map_err(err("Frame size"))?;
        if (content.Width as u32) < self.area.right || (content.Height as u32) < self.area.bottom { return Ok(false); } // the monitor's mode changed under us
        let surface = frame.Surface().map_err(err("Frame surface"))?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(err("Frame interface"))?;
        let texture: ID3D11Texture2D = access.GetInterface().map_err(err("Frame texture"))?;
        self.context.CopySubresourceRegion(&self.staging, 0, 0, 0, 0, &texture, 0, Some(&self.area));
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        self.context.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(err("Map frame"))?;
        for y in 0..self.height {
            std::ptr::copy_nonoverlapping((mapped.pData as *const u8).add(y * mapped.RowPitch as usize), pixels.as_mut_ptr().add(y * self.width * 4), self.width * 4);
        }
        self.context.Unmap(&self.staging, 0);
        Ok(true)
    }
}
impl Drop for Capture { fn drop(&mut self) { let _ = self.session.Close(); let _ = self.pool.Close(); } }
