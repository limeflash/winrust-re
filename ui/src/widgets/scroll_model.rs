use crate::anim::EASE;
/// Vertical scrolling in device-independent pixels; wheel deltas follow Windows sign conventions.
#[derive(Clone, Debug, Default)]
pub struct Scroll {
    target: f32,
    from: f32,
    start_ms: u64,
    dur_ms: u32,
    content: f32,
    view: f32,
}
impl Scroll {
    fn max(&self) -> f32 {
        (self.content - self.view).max(0.0)
    }
    pub fn set_extent(&mut self, content: f32, view: f32) {
        self.content = content.max(0.0);
        self.view = view.max(0.0);
        self.target = self.target.clamp(0.0, self.max());
        self.from = self.from.clamp(0.0, self.max());
    }
    pub fn offset(&self, now: u64) -> f32 {
        if self.dur_ms == 0 {
            return self.target;
        }
        let t = now.saturating_sub(self.start_ms) as f32 / self.dur_ms as f32;
        self.from + (self.target - self.from) * EASE.ease(t)
    }
    pub fn animating(&self, now: u64) -> bool {
        self.dur_ms > 0
            && now.saturating_sub(self.start_ms) < u64::from(self.dur_ms)
            && self.from != self.target
    }
    pub fn scroll_to(&mut self, y: f32, now: u64) {
        let y = y.clamp(0.0, self.max());
        if y != self.target {
            self.from = self.offset(now);
            self.target = y;
            self.start_ms = now;
            self.dur_ms = 150;
        }
    }
    pub fn wheel(&mut self, delta: f32, now: u64, line_h: f32) {
        let d = -delta / 120.0 * 3.0 * line_h;
        if delta.abs() < 120.0 {
            self.target = (self.offset(now) + d).clamp(0.0, self.max());
            self.from = self.target;
            self.dur_ms = 0;
        } else {
            self.scroll_to(self.target + d, now);
        }
    }
    pub fn ensure_visible(&mut self, top: f32, h: f32, now: u64) {
        if top < self.target {
            self.scroll_to(top, now);
        } else if top + h > self.target + self.view {
            self.scroll_to(top + h - self.view, now);
        }
    }
    pub fn page_up(&mut self, now: u64) {
        self.scroll_to(self.target - self.view, now);
    }
    pub fn page_down(&mut self, now: u64) {
        self.scroll_to(self.target + self.view, now);
    }
    /// Thumb geometry at the target offset, with a 20-pixel minimum.
    pub fn thumb(&self, track_h: f32) -> (f32, f32) {
        let track = track_h.max(0.0);
        let h = if self.content <= 0.0 {
            track
        } else {
            (track * self.view / self.content).clamp(20.0_f32.min(track), track)
        };
        let y = if self.max() > 0.0 {
            (track - h) * self.target / self.max()
        } else {
            0.0
        };
        (y, h)
    }
    /// Drag delta in track pixels; track height is supplied for the mapping.
    pub fn drag_thumb(&mut self, dy: f32, track_h: f32) {
        let (_, h) = self.thumb(track_h);
        let travel = (track_h - h).max(0.0);
        if travel > 0.0 {
            self.target = (self.target + dy * self.max() / travel).clamp(0.0, self.max());
            self.from = self.target;
            self.dur_ms = 0;
        }
    }
    pub fn view(&self) -> f32 {
        self.view
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wheel_animation_and_bounds() {
        let mut s = Scroll::default();
        s.set_extent(1000.0, 100.0);
        s.wheel(-120.0, 10, 10.0);
        assert_eq!(s.offset(10), 0.0);
        assert!(s.animating(10));
        assert_eq!(s.offset(160), 30.0);
        s.wheel(-60.0, 160, 10.0);
        assert_eq!(s.offset(160), 45.0);
        assert!(!s.animating(160));
        s.scroll_to(2000.0, 200);
        assert_eq!(s.offset(350), 900.0);
        s.set_extent(50.0, 100.0);
        assert_eq!(s.offset(350), 0.0);
    }
    #[test]
    fn thumb_and_navigation() {
        let mut s = Scroll::default();
        s.set_extent(1000.0, 100.0);
        assert_eq!(s.thumb(100.0), (0.0, 20.0));
        s.drag_thumb(40.0, 100.0);
        assert_eq!(s.offset(0), 450.0);
        assert_eq!(s.thumb(100.0), (40.0, 20.0));
        s.page_down(1);
        assert_eq!(s.offset(151), 550.0);
        s.page_up(151);
        assert_eq!(s.offset(301), 450.0);
        s.ensure_visible(800.0, 20.0, 301);
        assert_eq!(s.offset(451), 720.0);
    }
}
