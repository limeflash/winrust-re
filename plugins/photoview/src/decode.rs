// Decodes any picture Windows has a codec for into top-down opaque BGRA, rotated by its EXIF orientation,
// transparent pixels composited over `bg` (BGR).
use windows::{
    Win32::{Foundation::GENERIC_READ, Graphics::Imaging::*, System::Com::*},
    core::*,
};

pub struct Img {
    pub w: i32,
    pub h: i32,
    pub bgra: Vec<u8>,
}

/// Call once on every thread that decodes.
pub fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// EXIF orientation (1..8) -> the WIC transform that makes the picture upright.
fn transform(orientation: u16) -> i32 {
    match orientation {
        2 => 8,      // mirrored
        3 => 2,      // 180
        4 => 16,     // mirrored vertically
        5 => 1 | 8,  // transpose
        6 => 1,      // 90 clockwise
        7 => 3 | 8,  // transverse
        8 => 3,      // 90 counter-clockwise
        _ => 0,
    }
}

pub fn decode(path: &str, bg: [u8; 3]) -> Result<Img> {
    unsafe {
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let dec = f.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let orientation = frame
            .GetMetadataQueryReader()
            .and_then(|q| {
                let mut v = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::default();
                q.GetMetadataByName(w!("/app1/ifd/{ushort=274}"), &mut v)?;
                u16::try_from(&v)
            })
            .unwrap_or(1);
        let conv = f.CreateFormatConverter()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut src: IWICBitmapSource = conv.cast()?;
        let t = transform(orientation);
        if t != 0 {
            let r = f.CreateBitmapFlipRotator()?;
            r.Initialize(&src, WICBitmapTransformOptions(t))?;
            src = r.cast()?;
        }
        let (mut w, mut h) = (0u32, 0u32);
        src.GetSize(&mut w, &mut h)?;
        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        src.CopyPixels(std::ptr::null(), w * 4, &mut bgra)?;
        for p in bgra.as_chunks_mut::<4>().0 {
            let inv = 255 - p[3] as u32; // premultiplied: result = colour + bg * (1 - alpha)
            for c in 0..3 {
                p[c] = (p[c] as u32 + bg[c] as u32 * inv / 255).min(255) as u8;
            }
            p[3] = 255;
        }
        Ok(Img { w: w as i32, h: h as i32, bgra })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x1 PNG, left pixel opaque red, right pixel fully transparent: the right one must come out as the background.
    #[test]
    fn decodes_png_and_composites_alpha_over_background() {
        // 8-bit RGBA, no interlace; IDAT built by hand (stored deflate block) so the test needs no image file.
        fn crc(d: &[u8]) -> u32 {
            let mut c = !0u32;
            for &b in d {
                c ^= b as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        fn chunk(out: &mut Vec<u8>, ty: &[u8], data: &[u8]) {
            out.extend((data.len() as u32).to_be_bytes());
            let mut body = ty.to_vec();
            body.extend(data);
            out.extend(&body);
            out.extend(crc(&body).to_be_bytes());
        }
        let raw = [0u8, 255, 0, 0, 255, 0, 0, 0, 0]; // filter 0, red opaque, transparent
        let (mut a, mut b) = (1u32, 0u32);
        for &x in &raw {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        let mut z = vec![0x78, 0x01, 0x01, raw.len() as u8, 0, !(raw.len() as u8), 0xFF];
        z.extend(raw);
        z.extend(((b << 16) | a).to_be_bytes());
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        chunk(&mut png, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        chunk(&mut png, b"IDAT", &z);
        chunk(&mut png, b"IEND", &[]);
        let dir = std::env::temp_dir().join(format!("photoview-decode-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("test.png");
        std::fs::write(&p, png).unwrap();
        com_init();
        let img = decode(p.to_str().unwrap(), [10, 20, 30]).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!((img.w, img.h), (2, 1));
        assert_eq!(img.bgra[..4], [0, 0, 255, 255]); // BGRA of opaque red
        assert_eq!(img.bgra[4..8], [10, 20, 30, 255]); // the background, as given (BGR)
    }
}

/// Decode without compositing, preserving premultiplied alpha for the new renderer.
pub fn decode_model(path: &std::path::Path) -> Result<crate::decoder::Decoded> {
    use crate::decoder::{Decoded, Meta, palette};
    unsafe {
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let dec = f.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let reader = frame.GetMetadataQueryReader().ok();
        let query = |name: &str| -> Option<windows::Win32::System::Com::StructuredStorage::PROPVARIANT> {
            let mut v = Default::default();
            reader.as_ref()?.GetMetadataByName(&HSTRING::from(name), &mut v).ok()?; Some(v)
        };
        let string = |names: &[&str]| -> Option<String> {
            names.iter().find_map(|name| { let v = query(name)?; windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc(&v).ok().and_then(|p| { let s = p.to_string().ok(); CoTaskMemFree(Some(p.0.cast())); s }).filter(|s| !s.is_empty()) })
        };
        let number = |names: &[&str]| -> Option<u32> {
            names.iter().find_map(|name| { let v = query(name)?; u32::try_from(&v).ok().or_else(|| u16::try_from(&v).ok().map(u32::from)) })
        };
        let orientation = number(&["/app1/ifd/{ushort=274}", "/ifd/{ushort=274}"]).unwrap_or(1) as u16;
        let conv = f.CreateFormatConverter()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut src: IWICBitmapSource = conv.cast()?;
        let t = transform(orientation);
        if t != 0 { let r = f.CreateBitmapFlipRotator()?; r.Initialize(&src, WICBitmapTransformOptions(t))?; src = r.cast()?; }
        let (mut w, mut h) = (0, 0); src.GetSize(&mut w, &mut h)?;
        let (original_w, original_h) = (w, h);
        if w.max(h) > 16384 {
            let scale = 16384.0 / w.max(h) as f64;
            w = ((w as f64 * scale).round() as u32).max(1); h = ((h as f64 * scale).round() as u32).max(1);
            let s = f.CreateBitmapScaler()?; s.Initialize(&src, w, h, WICBitmapInterpolationModeFant)?; src = s.cast()?;
        }
        let mut bytes = vec![0u8; w as usize * h as usize * 4]; src.CopyPixels(std::ptr::null(), w * 4, &mut bytes)?;
        let px: Vec<u32> = bytes.as_chunks::<4>().0.iter().map(|p| u32::from_le_bytes(*p)).collect();
        let meta = Meta { width: original_w, height: original_h, format: path.extension().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default(),
            taken: string(&["/app1/ifd/exif/{ushort=36867}", "/ifd/exif/{ushort=36867}"]),
            camera: string(&["/app1/ifd/{ushort=272}", "/ifd/{ushort=272}"]),
            lens: string(&["/app1/ifd/exif/{ushort=42036}", "/ifd/exif/{ushort=42036}"]),
            exposure: string(&["/app1/ifd/exif/{ushort=33434}", "/ifd/exif/{ushort=33434}"]),
            iso: number(&["/app1/ifd/exif/{ushort=34855}", "/ifd/exif/{ushort=34855}"]),
            size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) };
        let palette = palette(&px, w, h);
        Ok(Decoded { w, h, px, meta, palette })
    }
}

