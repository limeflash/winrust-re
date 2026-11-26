//! One monitor's picture, cropped to the recording area on the GPU and read back as top-down BGRA. Windows.Graphics.Capture
//! first; where Windows refuses it ("Could not capture the given monitor": a machine whose shell is not Explorer, like
//! WinRust's own), DXGI Desktop Duplication of the output that holds the area, with the pointer drawn in.
use std::{cell::{Cell, RefCell}, time::{Duration, Instant}};
use windows::{core::{factory, Interface}, Graphics::{Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession}, DirectX::{DirectXPixelFormat, Direct3D11::IDirect3DDevice}}, Win32::{Foundation::{HMODULE, RECT}, Graphics::{Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN}, Direct3D11::*, Dxgi::{CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, Common::*}, Gdi::*}, System::WinRT::{Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess}, Graphics::Capture::IGraphicsCaptureItemInterop}, UI::WindowsAndMessaging::{CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, ICONINFO}}};

/// How long a duplication that cannot be made again (a secure desktop, a mode switch that does not end) is waited for.
const LOST_FOR: Duration = Duration::from_secs(15);

enum Source {
    Wgc { pool: Direct3D11CaptureFramePool, session: GraphicsCaptureSession },
    /// The duplication goes away when the display mode changes or a full-screen app takes the output; made again then.
    /// `lost`: since when the duplication could not be made again (the recording ends with an error after `LOST_FOR`: the rest of
    /// the file would be one still picture reported as a recording).
    Dup { output: IDXGIOutput1, dup: RefCell<Option<IDXGIOutputDuplication>>, lost: Cell<Option<Instant>> },
}

pub struct Capture {
    pub device: ID3D11Device,
    source: Source,
    context: ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    area: D3D11_BOX,
    /// The area's top left corner on the screen (where the pointer is drawn from).
    origin: (i32, i32),
    width: usize,
    height: usize,
}

fn err<E: std::fmt::Display>(what: &'static str) -> impl FnOnce(E) -> String { move |e| format!("{what}: {e}") }

/// A device that can also feed Media Foundation's hardware encoder (VIDEO_SUPPORT), on `adapter` or the default one.
unsafe fn device(adapter: Option<&IDXGIAdapter>) -> Result<(ID3D11Device, ID3D11DeviceContext), String> {
    let kind: D3D_DRIVER_TYPE = if adapter.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
    let (mut device, mut context) = (None, None);
    let mut made = unsafe { D3D11CreateDevice(adapter, kind, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) };
    if made.is_err() {
        made = unsafe { D3D11CreateDevice(adapter, kind, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) };
    }
    made.map_err(err("Direct3D device"))?;
    let (device, context) = (device.ok_or("No D3D11 device")?, context.ok_or("No D3D11 context")?);
    // the capture thread and Media Foundation's encoder threads share this context
    let multithread: ID3D11Multithread = context.cast().map_err(err("Direct3D multithread"))?;
    let _ = unsafe { multithread.SetMultithreadProtected(true) };
    Ok((device, context))
}

impl Capture {
    pub unsafe fn new(area: (i32, i32, i32, i32), width: u32, height: u32) -> Result<Self, String> {
        // the area is in physical pixels, and so must be every monitor rectangle compared with it
        unsafe { windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(-4isize as _) }; // per monitor v2
        let rect = RECT { left: area.0, top: area.1, right: area.2, bottom: area.3 };
        match unsafe { Self::wgc(rect, width, height) } {
            Ok(c) => Ok(c),
            Err(first) => unsafe { Self::dup(rect, width, height) }.map_err(|e| format!("{first}; Desktop Duplication: {e}")),
        }
    }

    unsafe fn wgc(rect: RECT, width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            if !GraphicsCaptureSession::IsSupported().unwrap_or(false) { return Err("Screen capture is not supported on this system".into()); }
            let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONULL);
            if monitor.0.is_null() { return Err("Recording area is not on a monitor".into()); }
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() { return Err("Cannot inspect recording monitor".into()); }
            let bounds = info.rcMonitor;
            if rect.left < bounds.left || rect.top < bounds.top || rect.right > bounds.right || rect.bottom > bounds.bottom {
                return Err("Recording area must fit within one monitor".into());
            }
            let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(err("Capture interop"))?;
            let item: GraphicsCaptureItem = interop.CreateForMonitor(monitor).map_err(err("Capture item"))?;
            let size = item.Size().map_err(err("Capture item size"))?;
            let (left, top) = ((rect.left - bounds.left) as u32, (rect.top - bounds.top) as u32);
            if i64::from(left) + i64::from(width) > i64::from(size.Width) || i64::from(top) + i64::from(height) > i64::from(size.Height) {
                return Err("Recording area is larger than the captured monitor".into());
            }
            let (device, context) = device(None)?;
            let staging = staging(&device, width, height)?;
            let dxgi: IDXGIDevice = device.cast().map_err(err("DXGI device"))?;
            let winrt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi).and_then(|d| d.cast()).map_err(err("WinRT Direct3D device"))?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).map_err(err("Capture frame pool"))?;
            let session = match pool.CreateCaptureSession(&item) {
                Ok(session) => session,
                Err(e) => { let _ = pool.Close(); return Err(format!("Capture session: {e}")); }
            };
            // from here the Drop of `capture` closes the session and the pool on any failure
            let capture = Self { device, source: Source::Wgc { pool, session }, context, staging, area: area_box(left, top, width, height), origin: (rect.left, rect.top), width: width as usize, height: height as usize };
            if let Source::Wgc { session, .. } = &capture.source {
                let _ = session.SetIsBorderRequired(false); // only honoured when the user allowed it; the border is not recorded anyway
                session.StartCapture().map_err(err("Start capture"))?;
            }
            Ok(capture)
        }
    }

    unsafe fn dup(rect: RECT, width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            // the output that holds the area, and the adapter that drives it (a duplication is made on that adapter's device)
            let f: IDXGIFactory1 = CreateDXGIFactory1().map_err(err("DXGI factory"))?;
            let mut found = None;
            let mut i = 0;
            while let Ok(a) = f.EnumAdapters1(i) {
                let mut j = 0;
                while let Ok(o) = a.EnumOutputs(j) {
                    let d = o.GetDesc().map_err(err("Output"))?.DesktopCoordinates;
                    if rect.left >= d.left && rect.top >= d.top && rect.right <= d.right && rect.bottom <= d.bottom {
                        found = Some((a.clone(), o, d));
                    }
                    j += 1;
                }
                i += 1;
            }
            let (adapter, output, d): (IDXGIAdapter1, _, RECT) = found.ok_or("The recording area is not on one output")?;
            let (device, context) = device(Some(&adapter.cast().map_err(err("Adapter"))?))?;
            let staging = staging(&device, width, height)?;
            let output: IDXGIOutput1 = output.cast().map_err(err("Output 1.1"))?;
            let dup = output.DuplicateOutput(&device).map_err(err("Duplicate output"))?;
            let (left, top) = ((rect.left - d.left) as u32, (rect.top - d.top) as u32);
            Ok(Self { device, source: Source::Dup { output, dup: RefCell::new(Some(dup)), lost: Cell::new(None) }, context, staging, area: area_box(left, top, width, height), origin: (rect.left, rect.top), width: width as usize, height: height as usize })
        }
    }

    /// Copies the newest captured frame into `pixels` (top-down BGRA, width*height*4 bytes).
    /// Ok(false): the screen has not changed since the last call, `pixels` keeps the previous picture.
    pub unsafe fn update(&self, pixels: &mut [u8]) -> Result<bool, String> {
        match &self.source {
            Source::Wgc { pool, .. } => unsafe {
                let mut latest = None;
                while let Ok(frame) = pool.TryGetNextFrame() {
                    if let Some(old) = latest.replace(frame) { let _ = old.Close(); } // the pool has two buffers: give the older one back at once
                }
                let Some(frame) = latest else { return Ok(false) };
                let result = self.copy_wgc(&frame, pixels);
                let _ = frame.Close();
                result
            },
            Source::Dup { output, dup, lost } => unsafe {
                let mut slot = dup.borrow_mut();
                if slot.is_none() {
                    *slot = output.DuplicateOutput(&self.device).ok(); // still switching modes: the next tick tries again
                    if slot.is_some() {
                        lost.set(None);
                    } else {
                        let since = lost.get().unwrap_or_else(Instant::now);
                        lost.set(Some(since));
                        if since.elapsed() > LOST_FOR {
                            return Err("The screen can no longer be captured".into());
                        }
                    }
                }
                let Some(d) = slot.as_ref() else { return Ok(false) };
                let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut res: Option<IDXGIResource> = None;
                match d.AcquireNextFrame(0, &mut info, &mut res) {
                    Ok(()) => {
                        // a frame that only moved the pointer has no new picture; the pointer is drawn on every frame anyway
                        let result = if info.LastPresentTime == 0 {
                            Ok(false)
                        } else {
                            res.ok_or("No desktop image".to_string()).and_then(|r| r.cast::<ID3D11Texture2D>().map_err(err("Desktop texture"))).and_then(|t| self.read(&t, pixels))
                        };
                        let _ = d.ReleaseFrame();
                        let changed = result?;
                        self.pointer(pixels);
                        Ok(changed || info.LastMouseUpdateTime != 0)
                    }
                    Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => Ok(false),
                    Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => { *slot = None; Ok(false) }
                    Err(e) => Err(format!("Desktop frame: {e}")),
                }
            },
        }
    }

    unsafe fn copy_wgc(&self, frame: &Direct3D11CaptureFrame, pixels: &mut [u8]) -> Result<bool, String> {
        let content = frame.ContentSize().map_err(err("Frame size"))?;
        if (content.Width as u32) < self.area.right || (content.Height as u32) < self.area.bottom { return Ok(false); } // the monitor's mode changed under us
        let surface = frame.Surface().map_err(err("Frame surface"))?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(err("Frame interface"))?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(err("Frame texture"))?;
        unsafe { self.read(&texture, pixels) }
    }

    /// The area of `texture` into `pixels`, through the staging texture.
    unsafe fn read(&self, texture: &ID3D11Texture2D, pixels: &mut [u8]) -> Result<bool, String> {
        unsafe {
            self.context.CopySubresourceRegion(&self.staging, 0, 0, 0, 0, texture, 0, Some(&self.area));
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(err("Map frame"))?;
            for y in 0..self.height {
                std::ptr::copy_nonoverlapping((mapped.pData as *const u8).add(y * mapped.RowPitch as usize), pixels.as_mut_ptr().add(y * self.width * 4), self.width * 4);
            }
            self.context.Unmap(&self.staging, 0);
        }
        Ok(true)
    }

    /// Draws the pointer into the picture (a duplicated desktop comes without it): the pixels under it go through a small DIB
    /// that Windows draws the cursor on, then back.
    unsafe fn pointer(&self, pixels: &mut [u8]) {
        const BOX: i32 = 128; // the largest pointer Windows draws (accessibility sizes) fits
        unsafe {
            let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
            if GetCursorInfo(&mut ci).is_err() || ci.flags.0 & CURSOR_SHOWING.0 == 0 || ci.hCursor.is_invalid() {
                return;
            }
            let mut ii = ICONINFO::default();
            if GetIconInfo(ci.hCursor.into(), &mut ii).is_err() {
                return;
            }
            let _ = DeleteObject(ii.hbmMask.into());
            if !ii.hbmColor.is_invalid() {
                let _ = DeleteObject(ii.hbmColor.into());
            }
            let (x0, y0) = (ci.ptScreenPos.x - ii.xHotspot as i32 - self.origin.0, ci.ptScreenPos.y - ii.yHotspot as i32 - self.origin.1);
            let (w, h) = (self.width as i32, self.height as i32);
            if x0 >= w || y0 >= h || x0 + BOX <= 0 || y0 + BOX <= 0 {
                return;
            }
            let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: BOX, biHeight: -BOX, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let Ok(dib) = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return;
            };
            let old = SelectObject(dc, dib.into());
            let bits = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (BOX * BOX * 4) as usize);
            let rows = |f: &mut dyn FnMut(usize, usize, usize)| {
                for y in y0.max(0)..(y0 + BOX).min(h) {
                    let (xa, xb) = (x0.max(0), (x0 + BOX).min(w));
                    f(((y * w + xa) * 4) as usize, (((y - y0) * BOX + (xa - x0)) * 4) as usize, ((xb - xa) * 4) as usize);
                }
            };
            rows(&mut |p, b, n| bits[b..b + n].copy_from_slice(&pixels[p..p + n]));
            let _ = DrawIconEx(dc, 0, 0, ci.hCursor.into(), 0, 0, 0, None, DI_NORMAL);
            rows(&mut |p, b, n| pixels[p..p + n].copy_from_slice(&bits[b..b + n]));
            SelectObject(dc, old);
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(dc);
        }
    }
}

unsafe fn staging(device: &ID3D11Device, width: u32, height: u32) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC { Width: width, Height: height, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_B8G8R8A8_UNORM, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_STAGING, CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32, ..Default::default() };
    let mut staging = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(err("Staging texture"))?;
    staging.ok_or_else(|| "Cannot create staging texture".to_string())
}

const fn area_box(left: u32, top: u32, width: u32, height: u32) -> D3D11_BOX {
    D3D11_BOX { left, top, front: 0, right: left + width, bottom: top + height, back: 1 }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Source::Wgc { pool, session } = &self.source {
            let _ = session.Close();
            let _ = pool.Close();
        }
    }
}
