//! Timing curves and transitions for UI animation.

/// The clock every animation reads, in milliseconds: `GetTickCount64` as it was at the first call, plus the precise time since.
/// `GetTickCount64` itself moves in steps of the system timer tick (15.6 ms), so a motion paced at the screen's 360 Hz would
/// stand still for five frames and then jump; this moves with every frame. The values stay in `GetTickCount64`'s domain, so
/// a zero kept for "long ago" and the comparisons with it mean what they did.
#[must_use]
pub fn now_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static BASE: OnceLock<(u64, Instant)> = OnceLock::new();
    // SAFETY: plain query.
    let (tick, at) = BASE.get_or_init(|| (unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }, Instant::now()));
    tick + at.elapsed().as_millis() as u64
}

/// A CSS cubic-bezier timing curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}
impl CubicBezier {
    /// Creates a curve from its two control points.
    #[must_use]
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }
    /// Evaluates the timing curve at a clamped progress value.
    #[must_use]
    pub fn ease(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 || t == 1.0 {
            return t;
        }
        fn sample(a: f32, b: f32, s: f32) -> f32 {
            ((1.0 - 3.0 * b + 3.0 * a) * s + (3.0 * b - 6.0 * a)) * s * s + 3.0 * a * s
        }
        let mut s = t;
        for _ in 0..8 {
            let x = sample(self.x1, self.x2, s) - t;
            if x.abs() < 1e-6 {
                return sample(self.y1, self.y2, s);
            }
            let slope = (3.0 * (1.0 - 3.0 * self.x2 + 3.0 * self.x1) * s
                + 2.0 * (3.0 * self.x2 - 6.0 * self.x1))
                * s
                + 3.0 * self.x1;
            if slope.abs() < 1e-6 {
                break;
            }
            s -= x / slope;
        }
        let (mut lo, mut hi) = (0.0, 1.0);
        while hi - lo > 1e-6 {
            s = (lo + hi) * 0.5;
            if sample(self.x1, self.x2, s) < t {
                lo = s;
            } else {
                hi = s;
            }
        }
        sample(self.y1, self.y2, (lo + hi) * 0.5)
    }
}
/// Constant speed.
pub const LINEAR: CubicBezier = CubicBezier::new(0.0, 0.0, 1.0, 1.0);
/// Standard CSS ease.
pub const EASE: CubicBezier = CubicBezier::new(0.25, 0.1, 0.25, 1.0);
/// Panels and bar movement.
pub const SNAP: CubicBezier = CubicBezier::new(0.2, 0.9, 0.1, 1.0);
/// Hover plates, icons and tooltips.
pub const SPRING: CubicBezier = CubicBezier::new(0.22, 1.0, 0.36, 1.0);
/// Quick settings and search box.
pub const POP: CubicBezier = CubicBezier::new(0.16, 1.0, 0.3, 1.0);
/// Closing panels.
pub const EXIT: CubicBezier = CubicBezier::new(0.5, 0.0, 0.75, 0.0);
/// A window minimizing to the bar.
pub const SINK: CubicBezier = CubicBezier::new(0.55, 0.0, 0.75, 0.2);
/// A transition on one scalar property; timestamps are milliseconds.
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    from: f32,
    to: f32,
    start: u64,
    dur: u32,
    delay: u32,
    ease: CubicBezier,
}
impl Transition {
    /// Creates a settled transition.
    #[must_use]
    pub const fn new(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: 0,
            dur: 0,
            delay: 0,
            ease: LINEAR,
        }
    }
    /// Returns the property value at the supplied time.
    #[must_use]
    pub fn value(&self, now: u64) -> f32 {
        if self.dur == 0 {
            return self.to;
        }
        let begin = self.start.saturating_add(u64::from(self.delay));
        if now < begin {
            return self.from;
        }
        let elapsed = now - begin;
        if elapsed >= u64::from(self.dur) {
            return self.to;
        }
        lerp(
            self.from,
            self.to,
            self.ease.ease(elapsed as f32 / self.dur as f32),
        )
    }
    /// Returns the destination value.
    #[must_use]
    pub fn target(&self) -> f32 {
        self.to
    }
    /// Retargets from the current value, returning whether the target changed.
    pub fn set(&mut self, to: f32, now: u64, dur: u32, delay: u32, ease: CubicBezier) -> bool {
        if (to - self.to).abs() <= 1e-6 {
            return false;
        }
        *self = Self {
            from: self.value(now),
            to,
            start: now,
            dur,
            delay,
            ease,
        };
        true
    }
    /// Immediately sets the value.
    pub fn snap(&mut self, to: f32) {
        *self = Self::new(to);
    }
    /// Returns whether the transition has yet to end, including its delay.
    #[must_use]
    pub fn running(&self, now: u64) -> bool {
        self.dur > 0
            && now
                < self
                    .start
                    .saturating_add(u64::from(self.delay))
                    .saturating_add(u64::from(self.dur))
    }
}
/// Linearly interpolates between two values.
#[must_use]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
/// Applies the user's motion preference to a duration and delay.
#[must_use]
pub fn timing(mode: winrust_common::style::Anim, dur_ms: u32, delay_ms: u32) -> (u32, u32) {
    match mode {
        winrust_common::style::Anim::Full => (dur_ms, delay_ms),
        winrust_common::style::Anim::Reduced => (dur_ms.min(80), 0),
        winrust_common::style::Anim::Off => (0, 0),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_clock_moves_with_every_frame_not_with_the_timer_tick() {
        let (mut last, mut steps, mut max_step) = (now_ms(), 0u32, 0u64);
        let end = std::time::Instant::now() + std::time::Duration::from_millis(120);
        while std::time::Instant::now() < end {
            let t = now_ms();
            assert!(t >= last, "the clock never goes back");
            if t > last {
                steps += 1;
                max_step = max_step.max(t - last);
            }
            last = t;
        }
        // GetTickCount64 would step about 8 times in 120 ms (15.6 ms each); this one steps every millisecond
        assert!(steps > 60 && max_step <= 3, "{steps} steps, the largest {max_step} ms");
    }

    #[test]
    fn curves() {
        for c in [LINEAR, EASE, SNAP, SPRING, POP, EXIT, SINK] {
            assert_eq!(c.ease(0.0), 0.0);
            assert_eq!(c.ease(1.0), 1.0);
        }
        assert!((EASE.ease(0.5) - 0.8024).abs() < 0.001);
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            assert!((LINEAR.ease(t) - t).abs() < 1e-5);
            if i > 0 {
                for c in [SNAP, SPRING] {
                    assert!(c.ease(t) >= c.ease((i - 1) as f32 / 100.0));
                }
            }
        }
    }
    #[test]
    fn transitions() {
        let mut t = Transition::new(0.0);
        assert!(t.set(1.0, 100, 100, 20, LINEAR));
        assert_eq!(t.value(110), 0.0);
        assert_eq!(t.value(220), 1.0);
        let v = t.value(170);
        assert!(t.set(0.0, 170, 100, 0, LINEAR));
        assert!((t.value(170) - v).abs() < 1e-6);
        assert!(!t.set(0.0, 180, 100, 0, EASE));
    }
    #[test]
    fn preferences() {
        use winrust_common::style::Anim;
        assert_eq!(timing(Anim::Full, 100, 10), (100, 10));
        assert_eq!(timing(Anim::Reduced, 100, 10), (80, 0));
        assert_eq!(timing(Anim::Reduced, 20, 10), (20, 0));
        assert_eq!(timing(Anim::Off, 100, 10), (0, 0));
    }
}
