//! Vertical scrolling with no window: where the content is, where it is gliding to, and the bar's thumb with the pointer holding it.
//! [`super::scroll`] draws the bar and tells it where the track is; the tests here do that by hand.
use crate::anim::EASE;
use crate::gfx::Rect;
/// Vertical scrolling in device-independent pixels; wheel deltas follow Windows sign conventions.
#[derive(Clone, Debug, Default)]
pub struct Scroll {
    target: f32,
    from: f32,
    start_ms: u64,
    dur_ms: u32,
    content: f32,
    view: f32,
    /// Where the track of the bar is in the window (`y`, height), DIPs: written by the widget while it draws.
    pub track: (f32, f32),
    /// The viewport in the window (DIPs), written by the widget too: the wheel scrolls what the pointer is over.
    pub area: Rect,
    /// Counts the glides begun, so that the frame loop can be kept running for each (the host draws frames while a tween runs).
    serial: u32,
    /// How far down the thumb the pointer holds it, while it does.
    grab: Option<f32>,
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
            self.serial = self.serial.wrapping_add(1);
        }
    }
    /// How many glides have begun.
    pub fn serial(&self) -> u32 {
        self.serial
    }
    /// Goes where it was going at once (the user asked for no animation).
    pub fn settle(&mut self) {
        self.from = self.target;
        self.dur_ms = 0;
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
    /// Thumb geometry at the offset shown at time `now` (not the one it glides to), with a 20-pixel minimum.
    pub fn thumb(&self, track_h: f32, now: u64) -> (f32, f32) {
        let track = track_h.max(0.0);
        let h = if self.content <= 0.0 {
            track
        } else {
            (track * self.view / self.content).clamp(20.0_f32.min(track), track)
        };
        let y = if self.max() > 0.0 {
            (track - h) * self.offset(now) / self.max()
        } else {
            0.0
        };
        (y, h)
    }
    /// Stops where it is shown now (a pointer takes hold of it).
    pub fn stop(&mut self, now: u64) {
        self.target = self.offset(now);
        self.from = self.target;
        self.dur_ms = 0;
    }
    /// The content can be scrolled.
    pub fn scrollable(&self) -> bool {
        self.max() > 0.0
    }
    /// The button went down at window `y` on the bar's track: on the thumb it takes hold of it, beside it the content goes a page
    /// toward `y`. False when there is no bar there.
    pub fn press(&mut self, y: f32, now: u64) -> bool {
        let (ty, th) = self.track;
        if th <= 0.0 || !self.scrollable() {
            return false;
        }
        let (t0, h) = self.thumb(th, now);
        let top = ty + t0;
        if y >= top && y <= top + h {
            self.stop(now);
            self.grab = Some(y - top);
        } else if y < top {
            self.page_up(now);
        } else {
            self.page_down(now);
        }
        true
    }
    /// The pointer moved with the thumb held: the content follows it at once.
    pub fn drag_to(&mut self, y: f32, now: u64) {
        let Some(grab) = self.grab else { return };
        let (ty, th) = self.track;
        let (_, h) = self.thumb(th, now);
        let travel = (th - h).max(0.0);
        if travel > 0.0 {
            self.target = ((y - grab - ty) / travel).clamp(0.0, 1.0) * self.max();
            self.from = self.target;
            self.dur_ms = 0;
        }
    }
    pub fn release(&mut self) {
        self.grab = None;
    }
    /// The thumb is held.
    pub fn dragging(&self) -> bool {
        self.grab.is_some()
    }
    /// How far the content is scrolled when it has glided there.
    pub fn target(&self) -> f32 {
        self.target
    }
    pub fn content(&self) -> f32 {
        self.content
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
        assert_eq!(s.thumb(100.0, 0), (0.0, 20.0));
        s.track = (10.0, 100.0);
        // the thumb is 20 high on a track of 100 that starts at y = 10: press its middle, pull it 40 down
        assert!(s.press(20.0, 0));
        assert!(s.dragging());
        s.drag_to(60.0, 0);
        assert_eq!(s.offset(0), 450.0);
        assert_eq!(s.thumb(100.0, 0), (40.0, 20.0));
        s.drag_to(5000.0, 0);
        assert_eq!(s.offset(0), 900.0, "past the end of the track: the end of the content");
        s.drag_to(-5000.0, 0);
        assert_eq!(s.offset(0), 0.0);
        s.drag_to(60.0, 0);
        s.release();
        s.drag_to(10.0, 0);
        assert_eq!(s.offset(0), 450.0, "a thumb that is let go does not follow");
        s.page_down(1);
        assert_eq!(s.offset(151), 550.0);
        s.page_up(151);
        assert_eq!(s.offset(301), 450.0);
        s.ensure_visible(800.0, 20.0, 301);
        assert_eq!(s.offset(451), 720.0);
    }

    #[test]
    fn the_track_pages_and_the_thumb_follows_what_is_shown() {
        let mut s = Scroll::default();
        s.set_extent(1000.0, 100.0);
        s.track = (0.0, 100.0);
        s.scroll_to(450.0, 0);
        // half way through the glide the thumb is where the content is, not where it is going
        let mid = s.offset(75);
        assert!(mid > 0.0 && mid < 450.0);
        assert_eq!(s.thumb(100.0, 75).0, 80.0 * mid / 900.0);
        assert_eq!(s.thumb(100.0, 500).0, 40.0);
        // a press on the track beside the thumb is a page; on it, a hold that stops the glide
        s.scroll_to(0.0, 1000);
        s.set_extent(1000.0, 100.0);
        assert!(s.press(90.0, 2000));
        assert!(!s.dragging(), "below the thumb: a page, not a hold");
        assert_eq!(s.target(), 100.0);
        assert!(s.press(15.0, 3000) && s.dragging());
        s.release();
        let mut none = Scroll::default();
        none.set_extent(50.0, 100.0);
        none.track = (0.0, 100.0);
        assert!(!none.press(5.0, 0), "content that fits has no bar to press");
    }
}
