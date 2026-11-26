//! Parsing SVG path data into absolute cubic segments.
use std::fmt;
/// A point in path coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    /// Horizontal coordinate.
    pub x: f32,
    /// Vertical coordinate.
    pub y: f32,
}
impl Pt {
    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    fn add(self, p: Self) -> Self {
        Self::new(self.x + p.x, self.y + p.y)
    }
    fn scale(self, k: f32) -> Self {
        Self::new(self.x * k, self.y * k)
    }
    fn reflect(self, p: Self) -> Self {
        self.scale(2.0).add(p.scale(-1.0))
    }
}
/// An absolute path segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    /// Start a subpath.
    Move(Pt),
    /// Draw a straight line.
    Line(Pt),
    /// Draw a cubic curve using two controls and an endpoint.
    Cubic(Pt, Pt, Pt),
    /// Close the current subpath.
    Close,
}
/// Invalid path data at a byte offset.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset of the error.
    pub at: usize,
}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid SVG path at byte {}", self.at)
    }
}
impl std::error::Error for ParseError {}
struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
}
impl Parser<'_> {
    fn sep(&mut self) {
        while self.i < self.bytes.len()
            && (self.bytes[self.i].is_ascii_whitespace() || self.bytes[self.i] == b',')
        {
            self.i += 1;
        }
    }
    fn number(&mut self) -> Result<f32, ParseError> {
        self.sep();
        let start = self.i;
        if self
            .bytes
            .get(self.i)
            .is_some_and(|c| *c == b'+' || *c == b'-')
        {
            self.i += 1;
        }
        let mut digits = 0;
        while self.bytes.get(self.i).is_some_and(u8::is_ascii_digit) {
            self.i += 1;
            digits += 1;
        }
        if self.bytes.get(self.i) == Some(&b'.') {
            self.i += 1;
            while self.bytes.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(ParseError { at: start });
        }
        if self
            .bytes
            .get(self.i)
            .is_some_and(|c| *c == b'e' || *c == b'E')
        {
            self.i += 1;
            if self
                .bytes
                .get(self.i)
                .is_some_and(|c| *c == b'+' || *c == b'-')
            {
                self.i += 1;
            }
            let e = self.i;
            while self.bytes.get(self.i).is_some_and(u8::is_ascii_digit) {
                self.i += 1;
            }
            if e == self.i {
                return Err(ParseError { at: self.i });
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.i])
            .map_err(|_| ParseError { at: start })?;
        let n = s.parse::<f32>().map_err(|_| ParseError { at: start })?;
        if !n.is_finite() {
            return Err(ParseError { at: start });
        }
        Ok(n)
    }
    fn flag(&mut self) -> Result<bool, ParseError> {
        self.sep();
        let at = self.i;
        match self.bytes.get(self.i) {
            Some(b'0') => {
                self.i += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.i += 1;
                Ok(true)
            }
            _ => Err(ParseError { at }),
        }
    }
    fn point(&mut self, base: Pt, relative: bool) -> Result<Pt, ParseError> {
        let p = Pt::new(self.number()?, self.number()?);
        Ok(if relative { base.add(p) } else { p })
    }
}
fn arc(
    out: &mut Vec<Seg>,
    start: Pt,
    end: Pt,
    radii: (f32, f32),
    rotation: f32,
    large: bool,
    sweep: bool,
) {
    if start == end {
        return;
    }
    let (mut rx, mut ry) = (f64::from(radii.0.abs()), f64::from(radii.1.abs()));
    if rx == 0.0 || ry == 0.0 {
        out.push(Seg::Line(end));
        return;
    }
    let phi = f64::from(rotation).to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx = f64::from(start.x - end.x) / 2.0;
    let dy = f64::from(start.y - end.y) / 2.0;
    let x = cos * dx + sin * dy;
    let y = -sin * dx + cos * dy;
    let lambda = x * x / (rx * rx) + y * y / (ry * ry);
    if lambda > 1.0 {
        let k = lambda.sqrt();
        rx *= k;
        ry *= k;
    }
    let numerator = (rx * rx * ry * ry - rx * rx * y * y - ry * ry * x * x).max(0.0);
    let denom = rx * rx * y * y + ry * ry * x * x;
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let coef = sign * (numerator / denom).sqrt();
    let cxp = coef * rx * y / ry;
    let cyp = -coef * ry * x / rx;
    let cx = cos * cxp - sin * cyp + f64::from(start.x + end.x) / 2.0;
    let cy = sin * cxp + cos * cyp + f64::from(start.y + end.y) / 2.0;
    let ux = (x - cxp) / rx;
    let uy = (y - cyp) / ry;
    let vx = (-x - cxp) / rx;
    let vy = (-y - cyp) / ry;
    let theta = uy.atan2(ux);
    let mut delta = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    let count = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize;
    let step = delta / count as f64;
    let map = |x: f64, y: f64| {
        Pt::new(
            (cx + rx * cos * x - ry * sin * y) as f32,
            (cy + rx * sin * x + ry * cos * y) as f32,
        )
    };
    for i in 0..count {
        let a = theta + i as f64 * step;
        let b = a + step;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let (sa, ca) = a.sin_cos();
        let (sb, cb) = b.sin_cos();
        let p1 = map(ca - k * sa, sa + k * ca);
        let p2 = map(cb + k * sb, sb - k * cb);
        let p = if i + 1 == count { end } else { map(cb, sb) };
        out.push(Seg::Cubic(p1, p2, p));
    }
}
/// Parses SVG 1.1 path data into absolute line and cubic segments.
pub fn parse(d: &str) -> Result<Vec<Seg>, ParseError> {
    let mut p = Parser {
        bytes: d.as_bytes(),
        i: 0,
    };
    let mut out = Vec::new();
    let mut cur = Pt::new(0.0, 0.0);
    let mut sub = cur;
    let mut cmd = 0u8;
    let mut previous = 0u8;
    let mut control = cur;
    let mut first = true;
    loop {
        p.sep();
        if p.i == p.bytes.len() {
            break;
        }
        if p.bytes[p.i].is_ascii_alphabetic() {
            cmd = p.bytes[p.i];
            p.i += 1;
            if !b"MmLlHhVvCcSsQqTtAaZz".contains(&cmd) {
                return Err(ParseError { at: p.i - 1 });
            }
            if first && cmd != b'M' && cmd != b'm' {
                return Err(ParseError { at: p.i - 1 });
            }
            if cmd == b'Z' || cmd == b'z' {
                out.push(Seg::Close);
                cur = sub;
                previous = cmd;
                cmd = 0;
                continue;
            }
        } else if cmd == 0 {
            return Err(ParseError { at: p.i });
        }
        let rel = cmd.is_ascii_lowercase();
        let upper = cmd.to_ascii_uppercase();
        match upper {
            b'M' => {
                cur = p.point(cur, rel)?;
                sub = cur;
                out.push(Seg::Move(cur));
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                cur = p.point(cur, rel)?;
                out.push(Seg::Line(cur));
            }
            b'H' => {
                let x = p.number()?;
                cur.x = if rel { cur.x + x } else { x };
                out.push(Seg::Line(cur));
            }
            b'V' => {
                let y = p.number()?;
                cur.y = if rel { cur.y + y } else { y };
                out.push(Seg::Line(cur));
            }
            b'C' => {
                let a = p.point(cur, rel)?;
                let b = p.point(cur, rel)?;
                let end = p.point(cur, rel)?;
                out.push(Seg::Cubic(a, b, end));
                control = b;
                cur = end;
            }
            b'S' => {
                let a = if matches!(previous, b'C' | b'S') {
                    cur.reflect(control)
                } else {
                    cur
                };
                let b = p.point(cur, rel)?;
                let end = p.point(cur, rel)?;
                out.push(Seg::Cubic(a, b, end));
                control = b;
                cur = end;
            }
            b'Q' | b'T' => {
                let q = if upper == b'Q' {
                    p.point(cur, rel)?
                } else if matches!(previous, b'Q' | b'T') {
                    cur.reflect(control)
                } else {
                    cur
                };
                let end = p.point(cur, rel)?;
                let a = cur.add(q.add(cur.scale(-1.0)).scale(2.0 / 3.0));
                let b = end.add(q.add(end.scale(-1.0)).scale(2.0 / 3.0));
                out.push(Seg::Cubic(a, b, end));
                control = q;
                cur = end;
            }
            b'A' => {
                let rx = p.number()?;
                let ry = p.number()?;
                let angle = p.number()?;
                let large = p.flag()?;
                let sweep = p.flag()?;
                let end = p.point(cur, rel)?;
                if rx < 0.0 || ry < 0.0 {
                    return Err(ParseError { at: p.i });
                }
                arc(&mut out, cur, end, (rx, ry), angle, large, sweep);
                cur = end;
            }
            _ => return Err(ParseError { at: p.i }),
        }
        previous = upper;
        first = false;
    }
    Ok(out)
}
/// Returns the bounds of all endpoints and control points.
#[must_use]
pub fn bounds(segs: &[Seg]) -> Option<(Pt, Pt)> {
    let mut result: Option<(Pt, Pt)> = None;
    let mut include = |p: Pt| {
        result = Some(match result {
            None => (p, p),
            Some((min, max)) => (
                Pt::new(min.x.min(p.x), min.y.min(p.y)),
                Pt::new(max.x.max(p.x), max.y.max(p.y)),
            ),
        });
    };
    for seg in segs {
        match *seg {
            Seg::Move(p) | Seg::Line(p) => include(p),
            Seg::Cubic(a, b, c) => {
                include(a);
                include(b);
                include(c);
            }
            Seg::Close => {}
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry() {
        let p = |x, y| Pt::new(x, y);
        assert_eq!(
            parse("M3.5 8.5h11v9h-11z").unwrap(),
            vec![
                Seg::Move(p(3.5, 8.5)),
                Seg::Line(p(14.5, 8.5)),
                Seg::Line(p(14.5, 17.5)),
                Seg::Line(p(3.5, 17.5)),
                Seg::Close
            ]
        );
        assert_eq!(
            parse("M12 19.5h.01").unwrap().last(),
            Some(&Seg::Line(p(12.01, 19.5)))
        );
        assert_eq!(
            parse("M.5.5 2-3").unwrap(),
            vec![Seg::Move(p(0.5, 0.5)), Seg::Line(p(2.0, -3.0))]
        );
        assert!(parse("M1 2 L").is_err());
        assert!(matches!(
            parse("M0 0Q10 0 10 10").unwrap()[1],
            Seg::Cubic(_, _, Pt { x: 10.0, y: 10.0 })
        ));
        let s = parse("M0 0 c1 0 2 0 3 0 s2 1 3 0").unwrap();
        assert!(matches!(s[2], Seg::Cubic(Pt { x: 4.0, y: 0.0 }, _, _)));
        let s = parse("M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z").unwrap();
        for seg in &s {
            if let Seg::Cubic(_, _, p) = seg {
                assert!(((p.x - 12.0).hypot(p.y - 12.0) - 3.0).abs() < 1e-3);
            }
        }
        let (a, b) = bounds(&s).unwrap();
        assert!(
            (a.x - 9.0).abs() < 0.5
                && (a.y - 9.0).abs() < 0.5
                && (b.x - 15.0).abs() < 0.5
                && (b.y - 15.0).abs() < 0.5
        );
        assert!(parse("M0 0a1 1 0 011 1").is_ok());
    }

    /// Every icon of the design parses and stays inside its 24×24 box.
    #[test]
    fn design_icons() {
        for d in [
            "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
            "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4",
            "M3.5 8.5h11v9h-11zM8 5.5h11v9",
            "M6 14l6-6 6 6",
            "M3 9.5a13 13 0 0 1 18 0M6 13a8.5 8.5 0 0 1 12 0M9 16.5a4 4 0 0 1 6 0M12 19.5h.01",
            "M4 9.5h3.5L12 6v12l-4.5-3.5H4zM15.5 9a4 4 0 0 1 0 6M18 6.5a8 8 0 0 1 0 11",
            "M4 12a8 8 0 1 0 2.5-5.8M4 4v5h5",
            "M12 3v9M6.3 6.8a8 8 0 1 0 11.4 0",
            "M12 21a9 9 0 1 1 9-9c0 2-1.5 3-3 3h-2a2 2 0 0 0-1.5 3.3A2 2 0 0 1 12 21zM7.5 11h.01M10 7.5h.01M14.5 7.5h.01",
            "M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5a8.5 8.5 0 1 0 10.7 10.7zM16.5 3.2l.55 1.25 1.25.55-1.25.55-.55 1.25-.55-1.25L14.7 5l1.25-.55zM20 8.2l.4.9.9.4-.9.4-.4.9-.4-.9-.9-.4.9-.4z",
            "M21 16v-2l-8-5V3.5a1.5 1.5 0 0 0-3 0V9l-8 5v2l8-2.5V19l-2 1.5V22l3.5-1 3.5 1v-1.5L13 19v-5.5z",
            "M1.75 1.25 h4.5 a1.75 1.75 0 0 1 0 3.5 h-4.5 a1.75 1.75 0 0 1 0 -3.5 z",
        ] {
            let segs = parse(d).unwrap_or_else(|e| panic!("{e}: {d}"));
            let (lo, hi) = bounds(&segs).unwrap();
            assert!([lo.x, lo.y, hi.x, hi.y].iter().all(|v| v.is_finite() && (-1.0..=25.0).contains(v)), "{d}");
        }
    }
}
