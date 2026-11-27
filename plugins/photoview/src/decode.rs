//! Decodes any picture Windows has a codec for (WIC: JPEG, PNG, GIF, BMP, TIFF; WebP, HEIC and AVIF when the codec is installed) into
//! premultiplied BGRA, upright by its EXIF orientation: what Direct2D takes as it is. Used on the worker threads of [`crate::decoder`].
use crate::decoder::{Decoded, Meta, Thumb, palette};
use windows::{
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::*,
        System::Com::{StructuredStorage::PROPVARIANT, *},
    },
    core::*,
};

/// The longest side Direct2D takes in a bitmap.
const MAX_SIDE: u32 = 16384;
/// A picture of more pixels than this is shown scaled down (a 100 MP panorama is 400 MB as pixels).
const MAX_PIXELS: u64 = 100_000_000;
/// The height of a filmstrip thumbnail in device pixels: twice the 46 DIPs of the design, so it is sharp at 200 %.
pub const THUMB_H: u32 = 92;
/// The longest side of the small copy a picture's ambient light is blurred from.
const SMALL: u32 = 64;

/// Call once on every thread that decodes.
pub fn com_init() {
    // SAFETY: COM for WIC on this thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// EXIF orientation (1..8) -> the WIC transform that makes the picture upright.
fn transform(orientation: u16) -> WICBitmapTransformOptions {
    WICBitmapTransformOptions(match orientation {
        2 => 8,     // mirrored
        3 => 2,     // 180
        4 => 16,    // mirrored vertically
        5 => 1 | 8, // transpose
        6 => 1,     // 90 clockwise
        7 => 3 | 8, // transverse
        8 => 3,     // 90 counter-clockwise
        _ => 0,
    })
}

/// A picture file opened: its first frame, and the factory that made it.
struct Opened {
    factory: IWICImagingFactory,
    frame: IWICBitmapFrameDecode,
    reader: Option<IWICMetadataQueryReader>,
}

fn open(path: &std::path::Path) -> Result<Opened> {
    // SAFETY: WIC objects made and used on this thread.
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let dec = factory.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let reader = frame.GetMetadataQueryReader().ok();
        Ok(Opened { factory, frame, reader })
    }
}

impl Opened {
    fn query(&self, name: &str) -> Option<PROPVARIANT> {
        let mut v = PROPVARIANT::default();
        // SAFETY: the reader fills the variant.
        unsafe { self.reader.as_ref()?.GetMetadataByName(&HSTRING::from(name), &mut v).ok()? };
        Some(v)
    }

    fn string(&self, names: &[&str]) -> Option<String> {
        names.iter().find_map(|name| {
            let v = self.query(name)?;
            // SAFETY: the string the call allocates is copied and freed.
            let s = unsafe {
                let p = windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc(&v).ok()?;
                let s = p.to_string().ok();
                CoTaskMemFree(Some(p.0.cast()));
                s
            };
            s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        })
    }

    fn number(&self, names: &[&str]) -> Option<u32> {
        names.iter().find_map(|name| {
            let v = self.query(name)?;
            u32::try_from(&v).ok().or_else(|| u16::try_from(&v).ok().map(u32::from))
        })
    }

    fn orientation(&self) -> u16 {
        self.number(&["/app1/ifd/{ushort=274}", "/ifd/{ushort=274}"]).unwrap_or(1) as u16
    }

    /// The exposure time: a rational in the metadata, the numerator in the low half of a 64-bit number.
    fn exposure(&self) -> Option<String> {
        let v = ["/app1/ifd/exif/{ushort=33434}", "/ifd/exif/{ushort=33434}"].iter().find_map(|n| u64::try_from(&self.query(n)?).ok())?;
        exposure_text(v as u32, (v >> 32) as u32)
    }
}

/// `1/250 s`, `0.5 s`, `2 s`: how a camera's shutter time reads.
pub fn exposure_text(num: u32, den: u32) -> Option<String> {
    if num == 0 || den == 0 {
        return None;
    }
    let secs = f64::from(num) / f64::from(den);
    Some(if secs < 0.4 {
        format!("1/{} s", (1.0 / secs).round() as u32)
    } else if secs.fract() < 0.05 {
        format!("{} s", secs.round() as u32)
    } else {
        format!("{secs:.1} s")
    })
}

/// `2026:11:12 23:41:00` (EXIF) as `Nov 12, 2026, 23:41`, the way the details panel writes dates.
pub fn exif_date(s: &str) -> Option<String> {
    let (d, t) = s.trim_end_matches('\0').split_once(' ')?;
    let mut ds = d.split(':').map(|n| n.trim().parse::<u32>().ok());
    let (y, m, day) = (ds.next()??, ds.next()??, ds.next()??);
    let mut ts = t.split(':').map(|n| n.trim().parse::<u32>().ok());
    let (hh, mm) = (ts.next()??, ts.next()??);
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].get(m.checked_sub(1)? as usize)?;
    (y > 0 && (1..=31).contains(&day)).then(|| format!("{month} {day}, {y}, {hh:02}:{mm:02}"))
}

/// The whole picture: pixels, size, what its file says about it, the five colours, and the small copy for the ambient light.
pub fn decode_model(path: &std::path::Path) -> Result<Decoded> {
    // SAFETY: WIC objects made and used on this thread; the pixels are copied into a buffer of exactly the size asked for.
    unsafe {
        let o = open(path)?;
        let orientation = o.orientation();
        let conv = o.factory.CreateFormatConverter()?;
        conv.Initialize(&o.frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut src: IWICBitmapSource = conv.cast()?;
        let t = transform(orientation);
        if t.0 != 0 {
            let r = o.factory.CreateBitmapFlipRotator()?;
            r.Initialize(&src, t)?;
            src = r.cast()?;
        }
        let (mut w, mut h) = (0, 0);
        src.GetSize(&mut w, &mut h)?;
        if w == 0 || h == 0 {
            return Err(Error::from(windows::Win32::Foundation::E_FAIL));
        }
        let (original_w, original_h) = (w, h);
        let k = (f64::from(MAX_SIDE) / f64::from(w.max(h))).min((MAX_PIXELS as f64 / (f64::from(w) * f64::from(h))).sqrt()).min(1.0);
        if k < 1.0 {
            w = ((f64::from(w) * k).round() as u32).max(1);
            h = ((f64::from(h) * k).round() as u32).max(1);
            let s = o.factory.CreateBitmapScaler()?;
            s.Initialize(&src, w, h, WICBitmapInterpolationModeFant)?;
            src = s.cast()?;
        }
        let mut px = vec![0u32; w as usize * h as usize];
        src.CopyPixels(std::ptr::null(), w * 4, std::slice::from_raw_parts_mut(px.as_mut_ptr().cast::<u8>(), px.len() * 4))?;
        let meta = Meta {
            width: original_w,
            height: original_h,
            taken: o.string(&["/app1/ifd/exif/{ushort=36867}", "/ifd/exif/{ushort=36867}"]).map(|s| exif_date(&s).unwrap_or(s)),
            camera: o.string(&["/app1/ifd/{ushort=272}", "/ifd/{ushort=272}"]),
            lens: o.string(&["/app1/ifd/exif/{ushort=42036}", "/ifd/exif/{ushort=42036}"]),
            exposure: o.exposure(),
            iso: o.number(&["/app1/ifd/exif/{ushort=34855}", "/ifd/exif/{ushort=34855}"]),
        };
        let palette = palette(&px, w, h);
        let small = small_copy(&px, w, h);
        Ok(Decoded { w, h, px, meta, palette, small })
    }
}

/// `(w, h, premultiplied BGRA)`.
pub type Small = (u32, u32, Vec<u32>);

/// The picture at most `SMALL` pixels on its long side, each pixel the mean of a grid of samples of the area it stands for.
pub fn small_copy(px: &[u32], w: u32, h: u32) -> Small {
    let k = (f64::from(SMALL) / f64::from(w.max(h))).min(1.0);
    let (sw, sh) = (((f64::from(w) * k).round() as u32).max(1), ((f64::from(h) * k).round() as u32).max(1));
    let (w64, h64) = (u64::from(w), u64::from(h));
    let mut out = Vec::with_capacity((sw * sh) as usize);
    for y in 0..sh {
        let y0 = u64::from(y) * h64 / u64::from(sh);
        let y1 = (u64::from(y + 1) * h64 / u64::from(sh)).max(y0 + 1);
        for x in 0..sw {
            let x0 = u64::from(x) * w64 / u64::from(sw);
            let x1 = (u64::from(x + 1) * w64 / u64::from(sw)).max(x0 + 1);
            let (nx, ny) = ((x1 - x0).min(6), (y1 - y0).min(6));
            let mut sum = [0u32; 4];
            for j in 0..ny {
                for i in 0..nx {
                    let (sx, sy) = (x0 + (i * 2 + 1) * (x1 - x0) / (nx * 2), y0 + (j * 2 + 1) * (y1 - y0) / (ny * 2));
                    let p = px[(sy.min(h64 - 1) * w64 + sx.min(w64 - 1)) as usize];
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += (p >> (8 * c)) & 255;
                    }
                }
            }
            let n = (nx * ny) as u32;
            out.push(sum.iter().enumerate().fold(0u32, |acc, (c, s)| acc | (s / n) << (8 * c)));
        }
    }
    (sw, sh, out)
}

/// A filmstrip thumbnail: `THUMB_H` high, as wide as the design's cell for the picture's shape (46 × 0.75..1.6 DIPs), the picture
/// cut to it from the middle. WIC scales while it decodes (a JPEG is read at a fraction of its size), so this costs a few ms.
pub fn decode_thumb(path: &std::path::Path) -> Result<Thumb> {
    // SAFETY: WIC objects made and used on this thread.
    unsafe {
        let o = open(path)?;
        let t = transform(o.orientation());
        let (mut fw, mut fh) = (0, 0);
        o.frame.GetSize(&mut fw, &mut fh)?;
        if fw == 0 || fh == 0 {
            return Err(Error::from(windows::Win32::Foundation::E_FAIL));
        }
        let turned = t.0 & 1 != 0; // a quarter turn swaps the sides
        let (w, h) = if turned { (fh, fw) } else { (fw, fh) };
        let (cw, ch) = thumb_cell(w, h);
        let k = (f64::from(cw) / f64::from(w)).max(f64::from(ch) / f64::from(h));
        let (sw, sh) = (((f64::from(w) * k).ceil() as u32).max(cw), ((f64::from(h) * k).ceil() as u32).max(ch));
        let scaler = o.factory.CreateBitmapScaler()?;
        let (pw, ph) = if turned { (sh, sw) } else { (sw, sh) };
        scaler.Initialize(&o.frame, pw, ph, WICBitmapInterpolationModeFant)?;
        let conv = o.factory.CreateFormatConverter()?;
        conv.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut src: IWICBitmapSource = conv.cast()?;
        if t.0 != 0 {
            let r = o.factory.CreateBitmapFlipRotator()?;
            r.Initialize(&src, t)?;
            src = r.cast()?;
        }
        let rect = WICRect { X: ((sw - cw) / 2) as i32, Y: ((sh - ch) / 2) as i32, Width: cw as i32, Height: ch as i32 };
        let mut px = vec![0u32; (cw * ch) as usize];
        src.CopyPixels(&rect, cw * 4, std::slice::from_raw_parts_mut(px.as_mut_ptr().cast::<u8>(), px.len() * 4))?;
        Ok(Thumb { w: cw, h: ch, px })
    }
}

/// The size of a thumbnail's cell for a picture of `w` × `h`: the design's `46 * clamp(w / h, .75, 1.6)` DIPs wide, doubled.
pub fn thumb_cell(w: u32, h: u32) -> (u32, u32) {
    let ar = (f64::from(w) / f64::from(h.max(1))).clamp(0.75, 1.6);
    ((f64::from(THUMB_H) * ar).round() as u32, THUMB_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_and_dates_read_like_a_camera_writes_them() {
        assert_eq!(exposure_text(1, 250).as_deref(), Some("1/250 s"));
        assert_eq!(exposure_text(10, 30).as_deref(), Some("1/3 s"));
        assert_eq!(exposure_text(5, 10).as_deref(), Some("0.5 s"));
        assert_eq!(exposure_text(2, 1).as_deref(), Some("2 s"));
        assert_eq!(exposure_text(0, 1), None);
        assert_eq!(exif_date("2026:11:12 23:41:07").as_deref(), Some("Nov 12, 2026, 23:41"));
        assert_eq!(exif_date("0000:00:00 00:00:00"), None);
        assert_eq!(exif_date("yesterday"), None);
    }

    #[test]
    fn thumbnail_cells_follow_the_design() {
        assert_eq!(thumb_cell(2400, 1600), (138, 92), "3:2 is 1.5 x 46 = 69 DIPs");
        assert_eq!(thumb_cell(1200, 1800), (69, 92), "a portrait is no narrower than .75");
        assert_eq!(thumb_cell(3000, 1000), (147, 92), "a panorama is no wider than 1.6");
    }

    #[test]
    fn the_small_copy_is_a_mean_of_the_picture() {
        // left half red, right half blue, 200 x 100
        let px: Vec<u32> = (0..100).flat_map(|_| (0..200).map(|x| if x < 100 { 0xffff0000 } else { 0xff0000ff })).collect();
        let (w, h, s) = small_copy(&px, 200, 100);
        assert_eq!((w, h, s.len()), (64, 32, 64 * 32));
        assert_eq!(s[0], 0xffff0000);
        assert_eq!(s[63], 0xff0000ff);
        assert_eq!(s[31], 0xffff0000, "the last column of the left half is still pure red");
        let (w, h, s) = small_copy(&[0xff112233; 6], 3, 2);
        assert_eq!((w, h, s), (3, 2, vec![0xff112233; 6]), "a small picture stays as it is");
    }
}
