//! The preview pictures: PNGs embedded in the exe, decoded by WIC (the system's own decoder: no crate for it) into the premultiplied
//! pixels Direct2D takes, once, when a module is first looked at.
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

/// A picture as pixels.
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

/// Decodes a PNG (or any picture the system has a codec for). COM must be initialized on the thread; the window's thread is.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Pixels> {
    // SAFETY: WIC objects made and used on this thread; the stream reads `bytes` while they live.
    unsafe {
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = f.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let dec = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = f.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        conv.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || w > 8192 || h > 8192 {
            return None;
        }
        let mut px = vec![0u32; (w * h) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, std::slice::from_raw_parts_mut(px.as_mut_ptr().cast::<u8>(), px.len() * 4)).ok()?;
        Some(Pixels { w, h, px })
    }
}
