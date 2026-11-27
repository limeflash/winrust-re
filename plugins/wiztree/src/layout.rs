//! Where the tiles of a folder stand on the map: the mockup's `renderVals` (squarify, the labels, the sub-tiles of the big ones) in
//! Rust. It depends on the folder, the size of the map and the data, so it is made once for them and kept; a frame only draws it.
use crate::look::{human, pct};
use crate::model::{squarify, Category, DiskData, AGG};
use winrust_ui::gfx::Rect;

/// The room round the tiles, and the gap between them (`PAD`, `GAP`).
pub const PAD: f32 = 8.0;
pub const GAP: f32 = 6.0;
/// The tiles of a level; the rest are one tile of "N smaller items".
pub const MAX_TILES: usize = 60;
/// The items the list shows.
pub const MAX_ROWS: usize = 300;

/// A part of a big folder's tile: what is in it, to its own scale.
pub struct Sub {
    /// In the tile's own coordinates (its top left corner is 0, 0).
    pub r: Rect,
    pub radius: f32,
    pub name: String,
    pub label: bool,
    /// `sqrt(size / the biggest sub-tile's)`: how bright it is.
    pub k: f32,
}

pub struct Tile {
    /// The record; `AGG` for the tile of the small items.
    pub id: u32,
    /// What squarify gave it, with the gap: where a zoom starts from or ends at.
    pub slot: Rect,
    /// The tile, in map coordinates.
    pub r: Rect,
    pub radius: f32,
    pub name: String,
    /// The second line of the label (empty when it does not fit).
    pub size_line: String,
    pub dir: bool,
    pub cat: Category,
    /// The size against the biggest tile's: the tile's colour.
    pub frac: f32,
    pub label: bool,
    pub font: f32,
    pub subs: Vec<Sub>,
    /// The tile's colour for the mode it was last drawn in (the mix in oklab is not free, and a hover redraws every tile).
    pub bg: std::cell::Cell<Option<(bool, winrust_ui::gfx::Rgba)>>,
}

/// One row of the list and one of "Biggest here".
pub struct Item {
    pub id: u32,
    pub bytes: u64,
    pub cat: Category,
    pub dir: bool,
    pub name: String,
    /// The files below it (a file has none).
    pub files: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    /// Which state of the data it was made from.
    pub epoch: u64,
    pub folder: u32,
    pub map: (i32, i32),
    pub max_tiles: usize,
}

pub struct Layout {
    pub key: Key,
    pub tiles: Vec<Tile>,
    /// The biggest items of the folder (at most `MAX_ROWS`), biggest first.
    pub items: Vec<Item>,
    /// The folder's own size.
    pub size: u64,
}

fn item(d: &DiskData, id: u32, bytes: u64) -> Item {
    let dir = d.is_dir(id);
    Item { id, bytes, cat: d.category(id), dir, name: d.name(id).to_string(), files: dir.then(|| d.counts(id).0) }
}

impl Layout {
    /// The level `folder` of `d` on a map of `map` DIPs.
    #[must_use]
    pub fn build(d: &DiskData, epoch: u64, folder: u32, map: (f32, f32), max_tiles: usize) -> Layout {
        let key = Key { epoch, folder, map: (map.0.round() as i32, map.1.round() as i32), max_tiles };
        let kids = d.children(folder);
        let size = d.size(folder);
        let items: Vec<Item> = kids.iter().take(MAX_ROWS).map(|&(id, b)| item(d, id, b)).collect();
        let mut shown: Vec<(u32, u64)> = kids.iter().take(max_tiles).copied().collect();
        let rest = kids.len().saturating_sub(max_tiles);
        if rest > 0 {
            shown.push((AGG, kids[max_tiles..].iter().map(|k| k.1).sum()));
        }
        let top = shown.first().map_or(1, |s| s.1).max(1);
        let rects = squarify(&shown, (f64::from(PAD), f64::from(PAD), f64::from(map.0 - 2.0 * PAD), f64::from(map.1 - 2.0 * PAD)));
        let mut tiles = Vec::with_capacity(rects.len());
        for (id, [x0, y0, x1, y1]) in rects {
            let slot = Rect::new(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32);
            let (w, h) = ((slot.w - GAP).max(0.0), (slot.h - GAP).max(0.0));
            let r = Rect::new(slot.x + GAP / 2.0, slot.y + GAP / 2.0, w, h);
            let bytes = shown.iter().find(|s| s.0 == id).map_or(0, |s| s.1);
            let agg = id == AGG;
            let dir = !agg && d.is_dir(id);
            let big = w > 150.0 && h > 110.0 && dir;
            let (mut subs, mut via) = (Vec::new(), String::new());
            if big {
                (subs, via) = subtiles(d, id, w, h);
            }
            let label = w > 64.0 && h > 34.0;
            let mut size_line = String::new();
            if label && h > 48.0 {
                size_line = human(bytes);
                if w > 120.0 {
                    size_line.push_str(" · ");
                    size_line.push_str(&pct(bytes, size));
                }
                if !via.is_empty() && w > 200.0 {
                    size_line.push_str(" · mostly in ");
                    size_line.push_str(&via);
                }
            }
            tiles.push(Tile {
                id,
                slot,
                r,
                radius: (w.min(h) / 5.0).clamp(4.0, 12.0),
                name: if agg { format!("{rest} smaller items") } else { d.name(id).to_string() },
                size_line,
                dir,
                cat: if agg { Category::Other } else { d.category(id) },
                frac: bytes as f32 / top as f32,
                label,
                font: if w > 180.0 && h > 80.0 { 13.5 } else { 12.0 },
                subs,
                bg: std::cell::Cell::new(None),
            });
        }
        Layout { key, tiles, items, size }
    }

    #[must_use]
    pub fn tile(&self, id: u32) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.id == id)
    }
}

/// The sub-tiles of the folder `id`, whose tile is `w` × `h`: its biggest items, down one or two levels while one folder holds
/// more than four fifths of it (and then the way down, "a › b").
fn subtiles(d: &DiskData, id: u32, w: f32, h: f32) -> (Vec<Sub>, String) {
    let (mut node, mut via) = (id, String::new());
    let mut ks = d.children(node);
    for _ in 0..2 {
        let sum: u64 = ks.iter().map(|k| k.1).sum();
        match ks.first() {
            Some(&(first, v)) if d.is_dir(first) && sum > 0 && v as f64 / sum as f64 > 0.8 => {
                if !via.is_empty() {
                    via.push_str(" › ");
                }
                via.push_str(d.name(first));
                node = first;
                ks = d.children(node);
            }
            _ => break,
        }
    }
    ks.truncate(18);
    let top = ks.first().map_or(1, |k| k.1).max(1);
    let subs = squarify(&ks, (8.0, 50.0, f64::from(w - 16.0), f64::from(h - 58.0)))
        .into_iter()
        .map(|(sid, [x0, y0, x1, y1])| {
            let (sw, sh) = ((x1 - x0) as f32, (y1 - y0) as f32);
            let bytes = ks.iter().find(|k| k.0 == sid).map_or(0, |k| k.1);
            Sub {
                r: Rect::new(x0 as f32, y0 as f32, (sw - 4.0).max(0.0), (sh - 4.0).max(0.0)),
                radius: (sw.min(sh) / 5.0).clamp(3.0, 8.0),
                name: d.name(sid).to_string(),
                label: sw > 84.0 && sh > 32.0,
                k: (bytes as f32 / top as f32).sqrt(),
            }
        })
        .collect();
    (subs, via)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::build;
    use std::collections::HashMap;
    use winrust_common::mft::{Rec, ROOT};

    fn data(n: u32) -> DiskData {
        let mut r = HashMap::new();
        let rec = |parent, name: &str, dir, alloc| Rec { parent, name: name.into(), dir, alloc, logical: alloc };
        r.insert(ROOT, rec(ROOT, ".", true, 0));
        r.insert(10, rec(ROOT, "Games", true, 0));
        r.insert(11, rec(10, "Call of Duty", true, 0));
        r.insert(12, rec(11, "pak0.pak", false, 90_000_000_000));
        r.insert(13, rec(11, "pak1.pak", false, 8_000_000_000));
        for i in 0..n {
            r.insert(100 + i, rec(ROOT, &format!("f{i}.bin"), false, 1_000_000 + u64::from(i)));
        }
        DiskData::read(build(r), 'C')
    }

    #[test]
    fn the_tiles_fill_the_map_and_the_small_ones_make_one_tile() {
        let d = data(100);
        let l = Layout::build(&d, 1, ROOT, (820.0, 592.0), MAX_TILES);
        assert_eq!(l.tiles.len(), MAX_TILES + 1);
        let agg = l.tiles.last().unwrap();
        assert_eq!((agg.id, agg.name.as_str()), (AGG, "41 smaller items"));
        let area: f32 = l.tiles.iter().map(|t| t.slot.w * t.slot.h).sum();
        assert!((area - (820.0 - 16.0) * (592.0 - 16.0)).abs() < 5.0, "{area}");
        assert_eq!(l.items.len(), 101, "the list has them all, up to 300");
        // the biggest tile is the folder, its colour is the strongest, and it shows what is in it
        let games = &l.tiles[0];
        assert_eq!((games.id, games.frac, games.dir, games.cat), (10, 1.0, true, Category::Games));
        assert!(games.label && games.size_line.starts_with("91.3 GB · 100%"), "{}", games.size_line);
        assert!(games.size_line.ends_with("mostly in Call of Duty"), "{}", games.size_line);
        assert_eq!(games.subs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["pak0.pak", "pak1.pak"]);
        assert!(games.subs[0].k == 1.0 && games.subs[1].k < 0.5);
        // a small tile has no label, a tile of a file no sub-tiles
        assert!(l.tiles.iter().any(|t| !t.label) && l.tiles.iter().filter(|t| !t.dir).all(|t| t.subs.is_empty()));
    }

    #[test]
    fn a_folder_of_no_size_has_no_tiles_and_a_tiny_map_has_none_that_overflow() {
        let d = data(0);
        assert!(Layout::build(&d, 1, 12, (820.0, 592.0), MAX_TILES).tiles.is_empty());
        let l = Layout::build(&d, 1, ROOT, (20.0, 20.0), MAX_TILES);
        assert!(l.tiles.iter().all(|t| t.r.w >= 0.0 && t.r.h >= 0.0 && t.r.x.is_finite()));
    }
}
