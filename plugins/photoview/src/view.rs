//! Where the picture is on the stage: fit, zoom about a point, pan limits, the navigator and the filmstrip's place. All in DIPs, no
//! drawing, so it is checked by tests. It is the mockup's `renderVals` (PhotoViewer.html) in Rust.

/// The room a fitted picture leaves round it, each side.
pub const MARGIN: f32 = 24.0;
pub const MAX_ZOOM: f32 = 8.0;
/// The navigator's width.
pub const NAVI_W: f32 = 148.0;

/// A zoom beyond fit: the picture is `z` times its size (1 = one pixel of the picture on one device pixel) and its middle is moved
/// by `(ox, oy)` from the stage's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    pub z: f32,
    pub ox: f32,
    pub oy: f32,
}

/// The stage and the upright picture (its size in DIPs at 100 %).
#[derive(Clone, Copy, Debug)]
pub struct Geo {
    pub stage: (f32, f32),
    pub pic: (f32, f32),
}

/// What the navigator shows: its size, and the part of the picture in view as a rectangle of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Navi {
    pub w: f32,
    pub h: f32,
    pub x: f32,
    pub y: f32,
    pub vw: f32,
    pub vh: f32,
}

impl Geo {
    /// The zoom that shows the whole picture with `MARGIN` round it, never above 100 %.
    pub fn fit(&self) -> f32 {
        (((self.stage.0 - 2.0 * MARGIN) / self.pic.0).min((self.stage.1 - 2.0 * MARGIN) / self.pic.1)).clamp(0.01, 1.0)
    }

    /// The offset kept so that the picture never leaves the stage by more than its margin.
    pub fn clamp(&self, z: f32, ox: f32, oy: f32) -> (f32, f32) {
        let mx = ((self.pic.0 * z - self.stage.0) / 2.0 + MARGIN).max(0.0);
        let my = ((self.pic.1 * z - self.stage.1) / 2.0 + MARGIN).max(0.0);
        (ox.clamp(-mx, mx), oy.clamp(-my, my))
    }

    /// The zoom and offset the picture is shown with: those of `zm` (clamped to the stage), or fit.
    pub fn shown(&self, zm: Option<Zoom>) -> Zoom {
        match zm {
            Some(Zoom { z, ox, oy }) => {
                let (ox, oy) = self.clamp(z, ox, oy);
                Zoom { z, ox, oy }
            }
            None => Zoom { z: self.fit(), ox: 0.0, oy: 0.0 },
        }
    }

    /// Zooms to `to` about `at` (a point of the stage, from its top left corner; `None` is its middle): the point of the picture
    /// under it stays there. `None` back is fit (what the mockup calls `zm: null`): `to` is at or below fit.
    pub fn zoom_to(&self, zm: Option<Zoom>, to: f32, at: Option<(f32, f32)>) -> Option<Zoom> {
        let fit = self.fit();
        let now = self.shown(zm);
        let z = to.clamp(fit.min(0.1), MAX_ZOOM);
        if z <= fit + 1e-4 {
            return None;
        }
        let (cx, cy) = at.map_or((0.0, 0.0), |(px, py)| (px - self.stage.0 / 2.0, py - self.stage.1 / 2.0));
        let (ox, oy) = self.clamp(z, cx - (cx - now.ox) * z / now.z, cy - (cy - now.oy) * z / now.z);
        Some(Zoom { z, ox, oy })
    }

    /// The navigator: shown for a picture that is bigger than the stage, never for a turned one (the mockup draws the unturned picture).
    pub fn navigator(&self, zm: Option<Zoom>, turned: bool) -> Option<Navi> {
        let zm = zm.filter(|_| !turned)?;
        let z = self.shown(Some(zm));
        let (dw, dh) = (self.pic.0 * z.z, self.pic.1 * z.z);
        if dw <= self.stage.0 + 1.0 && dh <= self.stage.1 + 1.0 {
            return None;
        }
        let (l, t) = (self.stage.0 / 2.0 + z.ox - dw / 2.0, self.stage.1 / 2.0 + z.oy - dh / 2.0);
        let (w, h) = (NAVI_W, (NAVI_W * self.pic.1 / self.pic.0).round());
        let (fx, fy, fw, fh) = ((-l / dw).max(0.0), (-t / dh).max(0.0), (self.stage.0 / dw).min(1.0), (self.stage.1 / dh).min(1.0));
        Some(Navi { w, h, x: fx * w, y: fy * h, vw: fw * w, vh: fh * h })
    }
}

/// The label of the zoom button: `Fit · 34%` while the picture is smaller than its stage, `100%` when it fits at full size, the
/// percentage while zoomed.
pub fn zoom_label(zm: Option<Zoom>, fit: f32) -> String {
    match zm {
        Some(z) => format!("{}%", (z.z * 100.0).round()),
        None if fit >= 1.0 => "100%".into(),
        None => format!("Fit · {}%", (fit * 100.0).round()),
    }
}

/// Where the thumbnails stand: their left edges for cells of the given widths, `gap` apart, in a strip of `width` with `pad` on both
/// sides. They are centred when they fit; when they do not, the strip scrolls to bring `current` to the middle (not beyond the ends).
pub fn strip_layout(widths: &[f32], current: usize, width: f32, pad: f32, gap: f32) -> Vec<f32> {
    let total: f32 = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    let room = width - 2.0 * pad;
    let start = if total <= room {
        (width - total) / 2.0
    } else {
        let before: f32 = widths.iter().take(current).map(|w| w + gap).sum();
        let mid = before + widths.get(current).copied().unwrap_or(0.0) / 2.0;
        pad - (mid - room / 2.0).clamp(0.0, total - room)
    };
    let mut x = start;
    widths
        .iter()
        .map(|w| {
            let at = x;
            x += w + gap;
            at
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mockup's first picture on its default stage: 2400 x 1600 on 872 x 616.
    fn aurora() -> Geo {
        Geo { stage: (872.0, 616.0), pic: (2400.0, 1600.0) }
    }

    #[test]
    fn fit_is_what_the_mockup_prints() {
        let g = aurora();
        assert!((g.fit() - 0.3433).abs() < 1e-3, "{}", g.fit());
        assert_eq!(zoom_label(None, g.fit()), "Fit · 34%");
        assert_eq!(zoom_label(None, 1.0), "100%");
        assert_eq!(zoom_label(Some(Zoom { z: 3.0, ox: 0.0, oy: 0.0 }), 0.3), "300%");
        // a small picture is not enlarged, and a tiny stage does not make the zoom vanish
        assert_eq!(Geo { stage: (800.0, 600.0), pic: (100.0, 50.0) }.fit(), 1.0);
        assert!(Geo { stage: (30.0, 30.0), pic: (4000.0, 3000.0) }.fit() >= 0.01);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let g = aurora();
        // the pointer is 100 px right of and 50 px under the stage's middle
        let at = (872.0 / 2.0 + 100.0, 616.0 / 2.0 + 50.0);
        let z1 = g.zoom_to(None, 1.0, Some(at)).unwrap();
        // the picture point under the pointer before: ((100 - 0) / fit, (50 - 0) / fit); after: ((100 - ox) / 1, ...)
        let fit = g.fit();
        let (px, py) = (100.0 / fit, 50.0 / fit);
        assert!(((100.0 - z1.ox) - px * z1.z).abs() < 0.5 && ((50.0 - z1.oy) - py * z1.z).abs() < 0.5, "{z1:?}");
        // zooming out to fit or below is "fit", and the limits hold
        assert_eq!(g.zoom_to(Some(z1), 0.05, None), None);
        assert_eq!(g.zoom_to(None, 100.0, None).unwrap().z, MAX_ZOOM);
        // the middle stays the middle
        let m = g.zoom_to(None, 1.0, None).unwrap();
        assert_eq!((m.ox, m.oy), (0.0, 0.0));
    }

    #[test]
    fn the_picture_cannot_be_pushed_off_the_stage() {
        let g = aurora();
        let (mx, my) = g.clamp(1.0, 1e6, -1e6);
        assert_eq!((mx, my), ((2400.0 - 872.0) / 2.0 + 24.0, -((1600.0 - 616.0) / 2.0 + 24.0)));
        // a picture smaller than the stage cannot be moved at all (beyond the margin there is nothing to see)
        assert_eq!(g.clamp(0.2, 500.0, 500.0), (0.0, 0.0));
        let shown = g.shown(Some(Zoom { z: 3.0, ox: 1e9, oy: 0.0 }));
        assert_eq!(shown.ox, (2400.0 * 3.0 - 872.0) / 2.0 + 24.0);
    }

    #[test]
    fn the_navigator_shows_the_view() {
        let g = aurora();
        assert_eq!(g.navigator(None, false), None, "not zoomed");
        assert_eq!(g.navigator(Some(Zoom { z: 3.0, ox: 0.0, oy: 0.0 }), true), None, "not for a turned picture");
        let n = g.navigator(Some(Zoom { z: 3.0, ox: 0.0, oy: 0.0 }), false).unwrap();
        assert_eq!((n.w, n.h), (148.0, 99.0));
        // the view is the middle of the picture: stage / picture = 872 / 7200 of its width
        assert!((n.vw - 148.0 * 872.0 / 7200.0).abs() < 0.01 && (n.x - (148.0 - n.vw) / 2.0).abs() < 0.01, "{n:?}");
        // a picture that only just fits the stage has none
        assert_eq!(g.navigator(Some(Zoom { z: 0.36, ox: 0.0, oy: 0.0 }), false), None);
    }

    #[test]
    fn the_strip_is_centred_or_scrolls_to_the_current_picture() {
        let w = [69.0, 35.0, 69.0, 74.0, 69.0];
        let x = strip_layout(&w, 2, 872.0, 16.0, 8.0);
        assert_eq!(x[0], 262.0, "the mockup's first thumbnail");
        assert_eq!(x[1] - x[0], 77.0);
        // twenty thumbnails do not fit in 400: the current one (10th) is in the middle, the ends stop at the padding
        let many = [60.0; 20];
        let x = strip_layout(&many, 10, 400.0, 16.0, 8.0);
        assert!((x[10] + 30.0 - 200.0).abs() < 0.01, "{}", x[10]);
        assert_eq!(strip_layout(&many, 0, 400.0, 16.0, 8.0)[0], 16.0);
        let last = strip_layout(&many, 19, 400.0, 16.0, 8.0);
        assert!((last[19] + 60.0 - (400.0 - 16.0)).abs() < 0.01);
        assert!(strip_layout(&[], 0, 400.0, 16.0, 8.0).is_empty());
    }
}
