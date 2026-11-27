//! What WinRust Disk knows of a drive, with no window: the tree of a scan, the squarified layout, the categories of the design's
//! legend, the path the user went down, and what deleting something takes off the totals.
use std::collections::HashMap;
use winrust_common::mft::{path_of, totals, Rec, ROOT};

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

/// The id of the tile that stands for all the items too small for a tile of their own.
pub const AGG: u32 = u32::MAX;

/// Squarified treemap: `items` (id, size) biggest first, laid out in the rectangle (x, y, w, h). An item of no size gets no cell.
pub fn squarify(items: &[(u32, u64)], (mut x, mut y, mut w, mut h): (f64, f64, f64, f64)) -> Vec<(u32, [f64; 4])> {
    let items: Vec<(u32, u64)> = items.iter().copied().filter(|i| i.1 > 0).collect();
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

// ---------------------------------------------------------------------------------------------------------------------
// categories

/// The legend of the design (`CATS` of DiskMap.html), in the order of its code. Music and archives are "other" there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Category {
    Video,
    Pictures,
    Games,
    Apps,
    Code,
    Documents,
    System,
    Other,
}

pub const N_CATS: usize = 8;

impl Category {
    pub const ALL: [Category; N_CATS] = [Self::Video, Self::Pictures, Self::Games, Self::Apps, Self::Code, Self::Documents, Self::System, Self::Other];

    #[must_use]
    pub fn label(self) -> &'static str {
        ["Video", "Pictures", "Games", "Apps", "Code", "Documents", "System", "Other"][self as usize]
    }

    #[must_use]
    pub fn from_u8(n: u8) -> Self {
        Self::ALL.get(n as usize).copied().unwrap_or(Self::Other)
    }
}

/// What the folders on the way to an entry say about it, so that a drive of a million files is classified in one pass (a folder's
/// context is its parent's plus its own name) and a single path with `category` gives the same answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ctx(u8);

const SYS: u8 = 1;
const GAMES: u8 = 2;
const CODE: u8 = 4;
const APPS: u8 = 8;
/// The last part was `appdata`, and the last two were `appdata\local`: what `appdata\local\programs` needs to be seen.
const IN_APPDATA: u8 = 16;
const IN_LOCAL: u8 = 32;

impl Ctx {
    /// The context after one more part of the path.
    #[must_use]
    pub fn push(self, part: &str) -> Ctx {
        let p = part.to_lowercase();
        let mut f = self.0 & (SYS | GAMES | CODE | APPS);
        match p.as_str() {
            "windows" => f |= SYS,
            "steamapps" | "steam library" | "steamlibrary" | "epic games" | "gog games" | "gog galaxy" => f |= GAMES,
            ".git" | "node_modules" | "target" => f |= CODE,
            "program files" | "program files (x86)" => f |= APPS,
            "programs" if self.0 & IN_LOCAL != 0 => f |= APPS,
            "local" if self.0 & IN_APPDATA != 0 => f |= IN_LOCAL,
            "appdata" => f |= IN_APPDATA,
            _ => {}
        }
        Ctx(f)
    }

    /// The context of a path (`C:\Users\x`): every part of it.
    #[cfg(test)]
    #[must_use]
    pub fn of(path: &str) -> Ctx {
        path.split(['\\', '/']).filter(|s| !s.is_empty()).fold(Ctx::default(), |c, part| c.push(part))
    }
}

/// The category of an entry whose own path (its name included) has context `ctx`.
#[must_use]
pub fn classify(ctx: Ctx, name: &str, dir: bool) -> Category {
    use Category::*;
    let lower = name.to_lowercase();
    if ctx.0 & SYS != 0 || lower.starts_with('$') || matches!(lower.as_str(), "pagefile.sys" | "hiberfil.sys" | "swapfile.sys" | "system volume information") {
        return System;
    }
    if ctx.0 & GAMES != 0 {
        return Games;
    }
    if ctx.0 & CODE != 0 {
        return Code;
    }
    if ctx.0 & APPS != 0 {
        return Apps;
    }
    if dir {
        return Other;
    }
    match lower.rsplit_once('.').map_or("", |(_, e)| e) {
        "pak" | "vpk" | "bsa" | "ba2" | "pck" | "wad" => Games,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" | "mpg" | "mpeg" | "flv" => Video,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "tif" | "tiff" | "raw" | "arw" | "cr2" | "cr3" | "nef" | "dng" | "psd" => Pictures,
        "rs" | "c" | "h" | "cpp" | "hpp" | "cs" | "py" | "js" | "ts" | "tsx" | "jsx" | "java" | "go" | "swift" | "html" | "css" | "json" | "toml" | "yaml" | "yml" => Code,
        "pdf" | "txt" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "rtf" | "md" | "csv" => Documents,
        _ => Other,
    }
}

/// The category of the entry `name` in the folder `path` (`C:\Users\x`).
#[cfg(test)]
#[must_use]
pub fn category(path: &str, name: &str, dir: bool) -> Category {
    classify(Ctx::of(path).push(name), name, dir)
}

// ---------------------------------------------------------------------------------------------------------------------
// a scanned drive

/// A drive after its scan: the tree, what category each entry is (a folder's is the one that most of its bytes are in), how many
/// files and folders are under each folder, and the legend's totals.
pub struct DiskData {
    pub tree: Tree,
    pub drive: char,
    /// By record number; "other" for a number that is no entry.
    cat: Vec<u8>,
    /// Folder -> (files, folders) below it.
    counts: HashMap<u32, (u32, u32)>,
    pub totals: [u64; N_CATS],
}

/// What taking an entry off the tree took off the totals, so that Undo can put it back.
pub struct Removed {
    id: u32,
    parent: u32,
    size: u64,
    files: u32,
    dirs: u32,
    cats: [u64; N_CATS],
}

impl Removed {
    #[cfg(test)]
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }
}

/// The folders from `from` up to the root, `from` included.
fn up_from(recs: &HashMap<u32, Rec>, from: u32) -> impl Iterator<Item = u32> + '_ {
    let mut next = Some(from);
    let mut n = 0;
    std::iter::from_fn(move || {
        let id = next.take().filter(|_| n < 128)?;
        n += 1;
        if id != ROOT {
            next = recs.get(&id).map(|r| r.parent);
        }
        Some(id)
    })
}

impl DiskData {
    /// Classifies every file with `file_cat` (a path-based rule for a real drive, the design's own for the demo) and counts.
    pub fn analyse(tree: Tree, drive: char, mut file_cat: impl FnMut(&HashMap<u32, Rec>, u32, &Rec) -> Category) -> Self {
        let max = tree.recs.keys().copied().max().unwrap_or(0) as usize;
        let mut cat = vec![Category::Other as u8; max + 1];
        let mut sums: HashMap<u32, [u64; N_CATS]> = HashMap::new();
        let mut counts: HashMap<u32, (u32, u32)> = HashMap::new();
        let mut all = [0u64; N_CATS];
        for (&id, r) in &tree.recs {
            if id == ROOT || !tree.tot.contains_key(&id) {
                continue;
            }
            if r.dir {
                for a in up_from(&tree.recs, r.parent) {
                    counts.entry(a).or_default().1 += 1;
                }
                counts.entry(id).or_default();
                continue;
            }
            let c = file_cat(&tree.recs, id, r);
            cat[id as usize] = c as u8;
            all[c as usize] += r.alloc;
            for a in up_from(&tree.recs, r.parent) {
                sums.entry(a).or_default()[c as usize] += r.alloc;
                counts.entry(a).or_default().0 += 1;
            }
        }
        for (id, s) in &sums {
            // the biggest share; a tie goes to the earlier category, so a drive reads the same twice
            let best = (0..N_CATS).rev().max_by_key(|&k| s[k]).unwrap_or(N_CATS - 1);
            // (a folder the table lost is named by its children and is no record: nothing to colour)
            if let Some(slot) = cat.get_mut(*id as usize) {
                *slot = best as u8;
            }
        }
        Self { tree, drive, cat, counts, totals: all }
    }

    /// A drive read from disk: the category of each file is told by its path and its name.
    pub fn read(tree: Tree, drive: char) -> Self {
        let mut memo: HashMap<u32, Ctx> = HashMap::new();
        Self::analyse(tree, drive, move |recs, _id, r| {
            let ctx = dir_ctx(recs, &mut memo, r.parent);
            classify(ctx.push(&r.name), &r.name, false)
        })
    }

    /// Counts that are not the tree's own (a made-up drive says what the mockup says).
    pub fn set_counts(&mut self, counts: HashMap<u32, (u32, u32)>) {
        self.counts = counts;
    }

    #[must_use]
    pub fn category(&self, id: u32) -> Category {
        Category::from_u8(self.cat.get(id as usize).copied().unwrap_or(Category::Other as u8))
    }

    #[must_use]
    pub fn size(&self, id: u32) -> u64 {
        self.tree.tot.get(&id).copied().unwrap_or(0)
    }

    /// The bytes in use on the drive.
    #[must_use]
    pub fn used(&self) -> u64 {
        self.size(ROOT)
    }

    /// Files and folders below `id` (a file has none).
    #[must_use]
    pub fn counts(&self, id: u32) -> (u32, u32) {
        self.counts.get(&id).copied().unwrap_or((0, 0))
    }

    #[must_use]
    pub fn is_dir(&self, id: u32) -> bool {
        id == ROOT || self.tree.recs.get(&id).is_some_and(|r| r.dir)
    }

    #[must_use]
    pub fn name(&self, id: u32) -> &str {
        self.tree.recs.get(&id).map_or("?", |r| r.name.as_str())
    }

    /// `C:\Users\x`; the drive's own is `C:\`.
    #[must_use]
    pub fn path(&self, id: u32) -> String {
        path_of(&self.tree.recs, id, self.drive).0
    }

    /// The folders from the root down to `id`, `id` included (the root is first).
    #[must_use]
    pub fn trail(&self, id: u32) -> Vec<u32> {
        let mut t: Vec<u32> = up_from(&self.tree.recs, id).collect();
        if t.last() != Some(&ROOT) {
            t.push(ROOT);
        }
        t.reverse();
        t
    }

    /// The items of a folder, biggest first: (id, bytes).
    #[must_use]
    pub fn children(&self, dir: u32) -> Vec<(u32, u64)> {
        self.tree.kids.get(&dir).into_iter().flatten().map(|&id| (id, self.size(id))).collect()
    }

    /// The `n` biggest items of a folder (at most 300: what the list shows).
    #[must_use]
    pub fn largest(&self, dir: u32, n: usize) -> Vec<(u32, u64)> {
        self.tree.kids.get(&dir).into_iter().flatten().take(n.min(300)).filter_map(|id| self.tree.tot.get(id).map(|&size| (*id, size))).collect()
    }

    /// What is under `id` by category (the files themselves), if it is a whole tree: `id` hangs from the root by a chain of at most 128
    /// folders, and below it nothing comes back to a folder already seen nor goes deeper than 128. A broken scan (a parent that is its
    /// own child) is none, and nothing of it is taken off or deleted.
    fn subtree_cats(&self, id: u32) -> Option<[u64; N_CATS]> {
        let mut up = id;
        for _ in 0..128 {
            if up == ROOT {
                break;
            }
            up = self.tree.recs.get(&up)?.parent;
        }
        if up != ROOT {
            return None;
        }
        let mut cats = [0u64; N_CATS];
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![(id, 0u32)];
        while let Some((n, depth)) = stack.pop() {
            if depth > 128 || !seen.insert(n) {
                return None;
            }
            match self.tree.kids.get(&n) {
                Some(ks) => stack.extend(ks.iter().map(|&k| (k, depth + 1))),
                None => cats[self.category(n) as usize] += self.size(n),
            }
        }
        Some(cats)
    }

    /// `remove` would take `id`: it is a whole tree (see `subtree_cats`).
    #[must_use]
    pub fn can_remove(&self, id: u32) -> bool {
        id != ROOT && self.tree.recs.contains_key(&id) && self.subtree_cats(id).is_some()
    }

    /// Takes `id` and everything under it off the tree and off every total above it (the folder, its parents, the drive, the
    /// legend). None for the root, for what is not here and for what is not a whole tree.
    pub fn remove(&mut self, id: u32) -> Option<Removed> {
        let parent = self.tree.recs.get(&id).filter(|_| id != ROOT)?.parent;
        let pos = self.tree.kids.get(&parent)?.iter().position(|&k| k == id)?;
        let size = self.size(id);
        let cats = self.subtree_cats(id)?;
        let (files, dirs) = if self.is_dir(id) { (self.counts(id).0, self.counts(id).1 + 1) } else { (1, 0) };
        self.tree.kids.get_mut(&parent)?.remove(pos);
        self.shift(parent, size, (files, dirs), &cats, false);
        Some(Removed { id, parent, size, files, dirs, cats })
    }

    /// Puts back what `remove` took.
    pub fn restore(&mut self, r: &Removed) {
        let at = {
            let ks = self.tree.kids.entry(r.parent).or_default();
            ks.iter().position(|k| self.tree.tot.get(k).copied().unwrap_or(0) < r.size).unwrap_or(ks.len())
        };
        self.tree.kids.entry(r.parent).or_default().insert(at, r.id);
        self.shift(r.parent, r.size, (r.files, r.dirs), &r.cats, true);
    }

    fn shift(&mut self, parent: u32, size: u64, (files, dirs): (u32, u32), cats: &[u64; N_CATS], add: bool) {
        let chain: Vec<u32> = up_from(&self.tree.recs, parent).collect();
        for a in chain {
            let t = self.tree.tot.entry(a).or_default();
            *t = if add { t.saturating_add(size) } else { t.saturating_sub(size) };
            let c = self.counts.entry(a).or_default();
            *c = if add { (c.0 + files, c.1 + dirs) } else { (c.0.saturating_sub(files), c.1.saturating_sub(dirs)) };
        }
        for (k, t) in self.totals.iter_mut().enumerate() {
            *t = if add { t.saturating_add(cats[k]) } else { t.saturating_sub(cats[k]) };
        }
    }
}

/// The context of folder `id`: its parent's and its own name, remembered.
fn dir_ctx(recs: &HashMap<u32, Rec>, memo: &mut HashMap<u32, Ctx>, id: u32) -> Ctx {
    let mut chain = vec![];
    let mut cur = id;
    while cur != ROOT && !memo.contains_key(&cur) && chain.len() < 128 {
        let Some(r) = recs.get(&cur) else { break };
        chain.push(cur);
        cur = r.parent;
    }
    let mut ctx = if cur == ROOT { Ctx::default() } else { memo.get(&cur).copied().unwrap_or_default() };
    for &d in chain.iter().rev() {
        ctx = ctx.push(&recs[&d].name);
        memo.insert(d, ctx);
    }
    ctx
}

// ---------------------------------------------------------------------------------------------------------------------
// where the user is

/// The folders from the drive's root down to the one on the screen.
#[derive(Default)]
pub struct Nav {
    stack: Vec<u32>,
}

impl Nav {
    #[must_use]
    pub fn new() -> Self {
        Self { stack: vec![ROOT] }
    }

    pub fn enter(&mut self, id: u32) {
        if self.stack.is_empty() {
            self.stack.push(ROOT);
        }
        if self.current() != id {
            self.stack.push(id);
        }
    }

    /// The path is `trail` (root first), as `DiskData::trail` makes it.
    pub fn set(&mut self, trail: &[u32]) {
        self.stack = if trail.first() == Some(&ROOT) { trail.to_vec() } else { vec![ROOT] };
    }

    /// One level up; the folder that was left, None at the root.
    pub fn up(&mut self) -> Option<u32> {
        if self.stack.len() > 1 { self.stack.pop() } else { None }
    }

    /// Back to the `n`th folder of the path (0 is the root); the folder that was left, if the path got shorter.
    pub fn to(&mut self, n: usize) -> Option<u32> {
        let left = self.stack.get(n + 1).copied();
        self.stack.truncate(n + 1);
        left
    }

    #[must_use]
    pub fn current(&self) -> u32 {
        self.stack.last().copied().unwrap_or(ROOT)
    }

    /// The path, root first (the root is `ROOT`; the app names it after the drive).
    #[must_use]
    pub fn path(&self) -> &[u32] {
        &self.stack
    }

    #[must_use]
    pub fn at_root(&self) -> bool {
        self.stack.len() <= 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(parent: u32, name: &str, dir: bool, alloc: u64) -> Rec {
        Rec { parent, name: name.into(), dir, alloc, logical: alloc }
    }

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

    #[test]
    fn an_item_of_no_size_gets_no_cell_and_does_not_break_the_others() {
        let cells = squarify(&[(1, 10), (2, 0), (3, 5)], (0.0, 0.0, 30.0, 10.0));
        assert_eq!(cells.iter().map(|c| c.0).collect::<Vec<_>>(), vec![1, 3]);
        assert!(cells.iter().all(|c| c.1.iter().all(|v| v.is_finite())));
        assert!(squarify(&[], (0.0, 0.0, 10.0, 10.0)).is_empty());
        assert!(squarify(&[(1, 5)], (0.0, 0.0, 0.0, 10.0)).is_empty());
    }

    #[test]
    fn classification_table() {
        use Category::*;
        let cases = [
            ("C:\\SteamLibrary\\steamapps", "a.mp4", Games),
            ("C:\\Epic Games", "x", Games),
            ("D:\\GOG Games", "x", Games),
            ("C:\\", "x.pak", Games),
            ("C:\\", "x.vpk", Games),
            ("C:\\", "x.bsa", Games),
            ("C:\\", "x.ba2", Games),
            ("C:\\", "x.mp4", Video),
            ("C:\\", "x.mkv", Video),
            ("C:\\", "x.avi", Video),
            ("C:\\", "x.mov", Video),
            ("C:\\Program Files", "x.exe", Apps),
            ("C:\\Program Files (x86)\\Adobe", "x.dll", Apps),
            ("C:\\Users\\x\\AppData\\Local\\Programs", "x", Apps),
            ("C:\\Users\\x\\AppData\\Local\\Programs\\Foo", "x.dll", Apps),
            ("C:\\Users\\x\\AppData\\Local", "x.dll", Other),
            ("C:\\Users\\x\\AppData\\Roaming\\Programs", "x.dll", Other),
            ("C:\\Windows", "x", System),
            ("C:\\Windows\\System32", "x.mp4", System),
            ("C:\\", "pagefile.sys", System),
            ("C:\\", "hiberfil.sys", System),
            ("C:\\", "$MFT", System),
            ("C:\\", "System Volume Information", System),
            ("C:\\", "x.jpg", Pictures),
            ("C:\\", "x.png", Pictures),
            ("C:\\", "x.webp", Pictures),
            ("C:\\", "DSC0001.ARW", Pictures),
            ("C:\\", "x.rs", Code),
            ("C:\\", "x.py", Code),
            ("C:\\", "x.cpp", Code),
            ("C:\\.git", "x", Code),
            ("C:\\node_modules", "x", Code),
            ("C:\\target", "x", Code),
            ("C:\\", "x.pdf", Documents),
            ("C:\\", "x.docx", Documents),
            ("C:\\", "x.txt", Documents),
            ("C:\\", "x.mp3", Other),
            ("C:\\", "x.zip", Other),
            ("C:\\", "x.iso", Other),
            ("C:\\", "x.unknown", Other),
            ("C:\\", "noextension", Other),
        ];
        for (p, n, want) in cases {
            assert_eq!(category(p, n, false), want, "{p} {n}");
        }
        // a folder is told by its name and its place, never by what it looks like
        assert_eq!(category("C:\\", "Videos.mp4", true), Other);
        assert_eq!(category("C:\\", "Windows", true), System);
        assert_eq!(category("C:\\Program Files", "Adobe", true), Apps);
    }

    fn drive() -> Tree {
        let mut r = HashMap::new();
        r.insert(ROOT, rec(ROOT, ".", true, 0));
        r.insert(10, rec(ROOT, "Windows", true, 0));
        r.insert(11, rec(10, "kernel.dll", false, 50));
        r.insert(12, rec(ROOT, "Users", true, 0));
        r.insert(13, rec(12, "me", true, 0));
        r.insert(14, rec(13, "film.mkv", false, 400));
        r.insert(15, rec(13, "pic.png", false, 100));
        r.insert(16, rec(13, "notes.txt", false, 10));
        r.insert(17, rec(ROOT, "Steam Library", true, 0));
        r.insert(18, rec(17, "game.bin", false, 300));
        r.insert(19, rec(12, "empty", true, 0));
        build(r)
    }

    #[test]
    fn a_whole_drive_is_classified_in_one_pass_the_same_as_path_by_path() {
        let d = DiskData::read(drive(), 'C');
        let want = |id: u32| {
            let r = &d.tree.recs[&id];
            let path = path_of(&d.tree.recs, r.parent, 'C').0;
            category(&path, &r.name, false)
        };
        for id in [11, 14, 15, 16, 18] {
            assert_eq!(d.category(id), want(id), "{}", d.name(id));
        }
        assert_eq!((d.category(11), d.category(14), d.category(18)), (Category::System, Category::Video, Category::Games));
        // a folder is the category that most of its bytes are in; counts and the legend's totals add up (a folder of no bytes is none)
        assert_eq!((d.category(13), d.category(12)), (Category::Video, Category::Video));
        assert_eq!(d.counts(ROOT), (5, 4));
        assert_eq!((d.counts(13), d.counts(12)), ((3, 0), (3, 1)));
        assert_eq!(d.totals[Category::Video as usize], 400);
        assert_eq!(d.totals.iter().sum::<u64>(), d.used());
        assert_eq!(d.used(), 860);
    }

    #[test]
    fn removing_and_restoring_take_it_off_and_put_it_back_on_every_total() {
        let mut d = DiskData::read(drive(), 'C');
        let before = (d.used(), d.size(12), d.counts(ROOT), d.totals, d.children(13));
        let gone = d.remove(14).unwrap();
        assert_eq!((gone.id(), gone.size()), (14, 400));
        assert_eq!((d.used(), d.size(13), d.size(12)), (460, 110, 110));
        assert_eq!(d.counts(ROOT), (4, 4));
        assert_eq!(d.totals[Category::Video as usize], 0);
        assert_eq!(d.children(13), vec![(15, 100), (16, 10)]);
        // a folder takes its whole content with it
        let all = d.remove(13).unwrap();
        assert_eq!((d.used(), d.counts(12), d.counts(ROOT)), (350, (0, 0), (2, 3)));
        d.restore(&all);
        assert_eq!((d.used(), d.counts(12)), (460, (2, 1)));
        d.restore(&gone);
        assert_eq!((d.used(), d.size(12), d.counts(ROOT), d.totals, d.children(13)), before);
        assert!(d.remove(ROOT).is_none() && d.remove(999).is_none());
        assert!(d.remove(14).is_some() && d.remove(14).is_none(), "a second removal of the same thing is nothing");
    }

    #[test]
    fn a_broken_chain_of_parents_is_refused_and_does_not_hang() {
        // two folders that are each other's parent, as a damaged file table can say, with a file in one of them
        let mut r = HashMap::new();
        r.insert(ROOT, rec(ROOT, ".", true, 0));
        r.insert(20, rec(21, "a", true, 0));
        r.insert(21, rec(20, "b", true, 0));
        r.insert(22, rec(21, "f.bin", false, 10));
        let mut d = DiskData::read(build(r), 'C');
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let before = d.size(21);
            let can = (d.can_remove(20), d.can_remove(22));
            let gone = d.remove(20).is_some();
            let _ = tx.send((can, gone, d.size(21) == before));
        });
        let got = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("remove did not come back: a cycle hangs it");
        assert_eq!(got, ((false, false), false, true), "refused, and nothing was taken off");
        // a tree that is too deep to be followed to the root is refused as well
        let mut r = HashMap::new();
        r.insert(ROOT, rec(ROOT, ".", true, 0));
        for i in 0..200u32 {
            r.insert(100 + i, rec(if i == 0 { ROOT } else { 99 + i }, "d", true, 0));
        }
        r.insert(900, rec(299, "f.bin", false, 5));
        let d = DiskData::read(build(r), 'C');
        assert!(!d.can_remove(900) && !d.can_remove(299), "too deep to follow to the root");
    }

    #[test]
    fn navigation_goes_down_and_up_and_back_to_any_folder_of_the_path() {
        let d = DiskData::read(drive(), 'C');
        let mut nav = Nav::new();
        assert!(nav.at_root() && nav.up().is_none());
        nav.enter(12);
        nav.enter(13);
        nav.enter(13); // the same folder again is not a step
        assert_eq!((nav.current(), nav.path()), (13, &[ROOT, 12, 13][..]));
        assert_eq!(nav.up(), Some(13));
        nav.enter(13);
        assert_eq!(nav.to(0), Some(12), "the crumb of the root leaves the first folder");
        assert!(nav.at_root());
        assert_eq!(nav.to(0), None);
        assert_eq!(d.largest(ROOT, 2), vec![(12, 510), (17, 300)]);
        assert_eq!(d.largest(13, 1), vec![(14, 400)]);
        assert!(d.largest(19, 5).is_empty());
        assert_eq!(d.trail(14), vec![ROOT, 12, 13, 14]);
        assert_eq!(d.trail(ROOT), vec![ROOT]);
        nav.set(&d.trail(13));
        assert_eq!((nav.current(), nav.path().len()), (13, 3));
    }
}
