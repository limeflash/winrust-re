#![allow(dead_code)]
use std::collections::HashMap;
use winrust_common::mft::{Rec, ROOT, totals, path_of};

pub struct Tree {
    pub recs: HashMap<u32, Rec>,
    pub tot: HashMap<u32, u64>,
    pub kids: HashMap<u32, Vec<u32>>,
}

pub fn build(recs: HashMap<u32, Rec>) -> Tree {
    let tot = totals(&recs, false);
    let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&id, r) in &recs {
        if id != ROOT && tot.contains_key(&id) {
            kids.entry(r.parent).or_default().push(id);
        }
    }
    for v in kids.values_mut() {
        v.sort_by_key(|id| std::cmp::Reverse(tot[id]));
    }
    Tree { recs, tot, kids }
}

/// Squarified treemap: `items` (id, size) biggest first, laid out in the rectangle (x, y, w, h).
pub fn squarify(items: &[(u32, u64)], (mut x, mut y, mut w, mut h): (f64, f64, f64, f64)) -> Vec<(u32, [f64; 4])> {
    let total: f64 = items.iter().map(|i| i.1 as f64).sum();
    let mut out = vec![];
    if total <= 0.0 || w <= 0.0 || h <= 0.0 {
        return out;
    }
    let scale = w * h / total;
    let area = |k: usize| items[k].1 as f64 * scale;
    let mut i = 0;
    while i < items.len() {
        let side = w.min(h);
        let worst = |s: f64, mx: f64, mn: f64| (side * side * mx / (s * s)).max(s * s / (side * side * mn));
        let (mut j, mut s, mut mx, mut mn) = (i + 1, area(i), area(i), area(i));
        let mut cur = worst(s, mx, mn);
        while j < items.len() {
            let a = area(j);
            let next = worst(s + a, mx.max(a), mn.min(a));
            if next > cur {
                break;
            }
            (s, mx, mn, cur, j) = (s + a, mx.max(a), mn.min(a), next, j + 1);
        }
        if w >= h {
            // a column on the left
            let (cw, mut yy) = (s / h, y);
            for (k, item) in items.iter().enumerate().take(j).skip(i) {
                let ch = area(k) / cw;
                out.push((item.0, [x, yy, x + cw, yy + ch]));
                yy += ch;
            }
            x += cw;
            w -= cw;
        } else {
            // a row on top
            let (rh, mut xx) = (s / w, x);
            for (k, item) in items.iter().enumerate().take(j).skip(i) {
                let cw = area(k) / rh;
                out.push((item.0, [xx, y, xx + cw, y + rh]));
                xx += cw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treemap_fills_the_rect_and_keeps_size_ratios() {
        let items = [(1, 600), (2, 300), (3, 200), (4, 100), (5, 100)];
        let cells = squarify(&items, (0.0, 0.0, 400.0, 300.0));
        assert_eq!(cells.len(), 5);
        let a = |id: u32| {
            let r = cells.iter().find(|c| c.0 == id).unwrap().1;
            (r[2] - r[0]) * (r[3] - r[1])
        };
        let total: f64 = (1..=5).map(a).sum();
        assert!((total - 120_000.0).abs() < 1.0, "area {total}");
        assert!((a(1) / a(2) - 2.0).abs() < 1e-6);
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category { Games, Video, Apps, System, Pictures, Code, Documents, Music, Archives, Other }

pub fn category(path: &str, name: &str, dir: bool) -> Category {
    use Category::*;
    let path = format!("{}\\{}", path, name).replace('/', "\\").to_lowercase();
    let parts: Vec<_> = path.split('\\').filter(|s| !s.is_empty()).collect();
    let name = name.to_lowercase();
    let has = |s: &str| parts.contains(&s);
    if has("windows") || name.starts_with('$') || matches!(name.as_str(), "pagefile.sys" | "hiberfil.sys" | "swapfile.sys") { return System; }
    if has("steamapps") || has("steam library") || has("steamlibrary") || has("epic games") || has("gog games") || has("gog galaxy") { return Games; }
    if has(".git") || has("node_modules") || has("target") { return Code; }
    if has("program files") || has("program files (x86)") || path.contains("\\appdata\\local\\programs\\") || path.ends_with("\\appdata\\local\\programs") { return Apps; }
    if dir { return Other; }
    match name.rsplit_once('.').map_or("", |(_, e)| e) {
        "pak" | "vpk" | "bsa" | "ba2" | "pck" | "wad" => Games,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" => Video,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "tif" | "tiff" | "raw" => Pictures,
        "rs" | "c" | "h" | "cpp" | "hpp" | "cs" | "py" | "js" | "ts" | "tsx" | "jsx" | "java" | "go" | "swift" | "html" | "css" | "json" | "toml" | "yaml" | "yml" => Code,
        "pdf" | "txt" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "rtf" => Documents,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" => Music,
        "zip" | "7z" | "rar" | "iso" | "tar" | "gz" => Archives,
        _ => Other,
    }
}

#[derive(Default)]
pub struct Nav { stack: Vec<u32> }
impl Nav {
    pub fn new() -> Self { Self { stack: vec![ROOT] } }
    pub fn enter(&mut self, id: u32) { if self.stack.is_empty() { self.stack.push(ROOT); }
        if self.current() != id { self.stack.push(id); } }
    pub fn up(&mut self) { if self.stack.len() > 1 { self.stack.pop(); } }
    pub fn current(&self) -> u32 { self.stack.last().copied().unwrap_or(ROOT) }
    pub fn crumbs(&self, tree: &Tree) -> Vec<(u32, String)> {
        let mut result = vec![(ROOT, tree.recs.get(&ROOT).map_or_else(|| "\\".to_string(), |r| r.name.clone()))];
        result.extend(self.stack.iter().copied().filter(|&id| id != ROOT).filter_map(|id| tree.recs.get(&id).map(|r| (id, r.name.clone()))));
        result
    }
}
pub fn largest(tree: &Tree, dir: u32, n: usize) -> Vec<(u32, u64)> {
    tree.kids.get(&dir).into_iter().flatten().take(n.min(300)).filter_map(|id| tree.tot.get(id).map(|&size| (*id, size))).collect()
}
pub fn category_totals(tree: &Tree, recs: &HashMap<u32, Rec>) -> Vec<(Category, u64)> {
    let mut sums = std::collections::BTreeMap::new();
    for (&id, rec) in recs {
        if id == ROOT || id == 8 || rec.alloc == 0 || !tree.tot.contains_key(&id) { continue; }
        let path = path_of(recs, rec.parent, 'C').0;
        *sums.entry(category(&path, &rec.name, rec.dir)).or_insert(0u64) += rec.alloc;
    }
    sums.into_iter().collect()
}

#[cfg(test)]
mod model_tests {
    use super::*;
    #[test] fn classification_table() {
        use Category::*;
        let cases = [("C:\\SteamLibrary\\steamapps", "a.mp4", Games), ("C:\\Epic Games", "x", Games), ("D:\\GOG Games", "x", Games), ("C:\\", "x.pak", Games), ("C:\\", "x.vpk", Games), ("C:\\", "x.bsa", Games), ("C:\\", "x.ba2", Games), ("C:\\", "x.mp4", Video), ("C:\\", "x.mkv", Video), ("C:\\", "x.avi", Video), ("C:\\", "x.mov", Video), ("C:\\Program Files", "x.exe", Apps), ("C:\\Users\\x\\AppData\\Local\\Programs", "x", Apps), ("C:\\Windows", "x", System), ("C:\\", "pagefile.sys", System), ("C:\\", "hiberfil.sys", System), ("C:\\", "$MFT", System), ("C:\\", "x.jpg", Pictures), ("C:\\", "x.png", Pictures), ("C:\\", "x.webp", Pictures), ("C:\\", "x.rs", Code), ("C:\\", "x.py", Code), ("C:\\", "x.cpp", Code), ("C:\\.git", "x", Code), ("C:\\node_modules", "x", Code), ("C:\\target", "x", Code), ("C:\\", "x.pdf", Documents), ("C:\\", "x.docx", Documents), ("C:\\", "x.txt", Documents), ("C:\\", "x.mp3", Music), ("C:\\", "x.flac", Music), ("C:\\", "x.wav", Music), ("C:\\", "x.zip", Archives), ("C:\\", "x.7z", Archives), ("C:\\", "x.rar", Archives), ("C:\\", "x.iso", Archives), ("C:\\", "x.unknown", Other)];
        for (p,n,want) in cases { assert_eq!(category(p,n,false), want, "{p} {n}"); }
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn navigation_sizes_and_categories() {
        let mut records = HashMap::new();
        let rec = |parent, name: &str, dir, alloc| Rec { parent, name: name.into(), dir, alloc, logical: alloc };
        records.insert(ROOT, rec(ROOT, "C:", true, 0));
        records.insert(10, rec(ROOT, "folder", true, 0));
        records.insert(11, rec(10, "song.mp3", false, 20));
        records.insert(12, rec(10, "pic.png", false, 10));
        let tree = build(records);
        let mut nav = Nav::new(); nav.enter(10);
        assert_eq!(nav.current(), 10);
        assert_eq!(nav.crumbs(&tree), vec![(ROOT, "C:".into()), (10, "folder".into())]);
        assert_eq!(largest(&tree, 10, 1), vec![(11, 20)]);
        assert_eq!(category_totals(&tree, &tree.recs), vec![(Category::Pictures, 10), (Category::Music, 20)]);
        nav.up(); nav.up(); assert_eq!(nav.current(), ROOT);
    }
}
