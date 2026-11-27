// NTFS MFT reader shared by rs-du (console) and wiztree (GUI).
use std::{collections::HashMap, mem::size_of, ptr};
use crate::{t, tf};
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::{IO::DeviceIoControl, Ioctl::*}};

pub const MASK: u64 = 0xFFFF_FFFF_FFFF;
pub const ROOT: u32 = 5;

pub struct Rec { pub parent: u32, pub name: String, pub dir: bool, pub alloc: u64, pub logical: u64 }

pub fn rd(b: &[u8], o: usize, n: usize) -> Option<u64> {
    let s = b.get(o..o.checked_add(n)?)?;
    Some(s.iter().rev().fold(0u64, |a, &x| (a << 8) | x as u64))
}

/// Checks the "FILE" magic and undoes the update-sequence fixups. None for garbage or torn records.
pub fn fixup(b: &mut [u8]) -> Option<()> {
    if b.get(..4)? != b"FILE" { return None; }
    let (uo, uc) = (rd(b, 4, 2)? as usize, rd(b, 6, 2)? as usize);
    let seq = [*b.get(uo)?, *b.get(uo + 1)?];
    for i in 1..uc {
        let end = i * 512 - 2;
        if end + 2 > b.len() { break; }
        if b[end..end + 2] != seq { return None; } // torn record
        let (x, y) = (*b.get(uo + 2 * i)?, *b.get(uo + 2 * i + 1)?);
        b[end] = x;
        b[end + 1] = y;
    }
    Some(())
}

/// Data runs (lcn, clusters) of the first unnamed non-resident $DATA of a fixed-up record. Sparse runs have lcn 0.
pub fn data_runs(b: &[u8]) -> Option<Vec<(u64, u64)>> {
    let mut o = rd(b, 0x14, 2)? as usize;
    loop {
        let (ty, len) = (rd(b, o, 4)?, rd(b, o + 4, 4)? as usize);
        if ty == 0xFFFF_FFFF || len == 0 { return None; }
        if ty == 0x80 && rd(b, o + 8, 1)? != 0 && rd(b, o + 9, 1)? == 0 { break; }
        o += len;
    }
    let (mut p, mut lcn, mut runs) = (o + rd(b, o + 0x20, 2)? as usize, 0i64, vec![]);
    while let Some(&h) = b.get(p) {
        if h == 0 { break; }
        let (ln, on) = ((h & 15) as usize, (h >> 4) as usize);
        let clusters = rd(b, p + 1, ln)?;
        let off = rd(b, p + 1 + ln, on)?;
        let shift = 64 - 8 * on as u32;
        let delta = if on == 0 { 0 } else { ((off << shift) as i64) >> shift };
        lcn += delta;
        runs.push((if on == 0 { 0 } else { lcn as u64 }, clusters));
        p += 1 + ln + on;
    }
    Some(runs)
}

/// Parses one raw MFT record into (base record ref, what it holds). Extension records (base != 0) carry attributes
/// that did not fit in their base record (often $FILE_NAME or more $DATA); `merge` joins them. None for unused/garbage.
pub fn parse(b: &mut [u8]) -> Option<(u64, Rec)> {
    fixup(b)?;
    let flags = rd(b, 0x16, 2)?;
    let base = rd(b, 0x20, 8)? & MASK;
    if flags & 1 == 0 { return None; }
    let used = (rd(b, 0x18, 4)? as usize).min(b.len());
    let mut o = rd(b, 0x14, 2)? as usize;
    let (mut name, mut parent, mut ns) = (None::<String>, 0u32, 2u64);
    let (mut alloc, mut logical) = (0u64, 0u64);
    while o + 8 <= used {
        let ty = rd(b, o, 4)?;
        let len = rd(b, o + 4, 4)? as usize;
        if ty == 0xFFFF_FFFF || len == 0 || o + len > used { break; }
        let nonres = rd(b, o + 8, 1)? != 0;
        if ty == 0x30 && !nonres {
            let c = o + rd(b, o + 0x14, 2)? as usize;
            let n = rd(b, c + 0x41, 1)?;
            if name.is_none() || (ns == 2 && n != 2) { // prefer a non-DOS name
                let nl = rd(b, c + 0x40, 1)? as usize;
                let u16s: Vec<u16> = (0..nl).map(|i| rd(b, c + 0x42 + 2 * i, 2).unwrap_or(0) as u16).collect();
                name = Some(String::from_utf16_lossy(&u16s));
                parent = (rd(b, c, 8)? & MASK) as u32;
                ns = n;
            }
        } else if ty == 0x80 {
            if !nonres { logical += rd(b, o + 0x10, 4)?; }
            else if rd(b, o + 0x10, 8)? == 0 { alloc += rd(b, o + 0x28, 8)?; logical += rd(b, o + 0x30, 8)?; }
        }
        o += len;
    }
    Some((base, Rec { parent, name: name.unwrap_or_default(), dir: flags & 2 != 0, alloc, logical }))
}

pub fn merge(recs: &mut HashMap<u32, Rec>, id: u32, r: Rec) {
    match recs.entry(id) {
        std::collections::hash_map::Entry::Vacant(v) => { v.insert(r); }
        std::collections::hash_map::Entry::Occupied(o) => {
            let e = o.into_mut();
            if e.name.is_empty() { e.name = r.name; e.parent = r.parent; }
            e.dir |= r.dir;
            e.alloc += r.alloc;
            e.logical += r.logical;
        }
    }
}

fn parse_slot(recs: &mut HashMap<u32, Rec>, idx: u32, slot: &mut [u8]) {
    if let Some((base, r)) = parse(slot) { merge(recs, if base == 0 { idx } else { base as u32 }, r); }
}

/// The open volume, closed on every way out of `scan` (the error returns used to leave it open: a rescan leaked one each time).
struct Volume(HANDLE);
impl Drop for Volume {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// Tells `progress` how far a scan is, about 48 times in all: at the start, after every `step` records, and at the end.
struct Pace {
    total: u64,
    step: u64,
    next: u64,
    last: Option<u64>,
}

impl Pace {
    fn new(total: u64) -> Self {
        Self { total, step: total.div_ceil(48).max(1), next: 0, last: None }
    }

    /// `read` records are done: reports when a step has been passed (`read` may jump over a sparse hole).
    fn at(&mut self, read: u64, progress: &mut dyn FnMut(u64, u64)) {
        if read >= self.next {
            progress(read.min(self.total), self.total);
            self.last = Some(read.min(self.total));
            self.next = read.saturating_add(self.step);
        }
    }

    /// The scan ended after `read` records: the last report is the whole table, whatever was reported before.
    fn end(&mut self, read: u64, progress: &mut dyn FnMut(u64, u64)) {
        if self.last != Some(read.min(self.total)) {
            progress(read.min(self.total), self.total);
        }
    }
}

pub fn scan(drive: char) -> Result<HashMap<u32, Rec>, String> {
    scan_with(drive, &mut |_, _| {})
}

pub fn scan_with(drive: char, progress: &mut dyn FnMut(u64, u64)) -> Result<HashMap<u32, Rec>, String> {
    let path: Vec<u16> = format!("\\\\.\\{drive}:").encode_utf16().chain([0]).collect();
    let h = unsafe { CreateFileW(path.as_ptr(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, ptr::null(), OPEN_EXISTING, 0, ptr::null_mut()) };
    if h == INVALID_HANDLE_VALUE { return Err(tf!("не удалось открыть том {0}: (нужны права администратора), код {1}", "cannot open volume {0}: (run as administrator), error {1}", ja: "ボリューム {0}: を開けません (管理者として実行してください)、エラー {1}", pt: "não foi possível abrir o volume {0}: (execute como administrador), erro {1}", zh: "无法打开卷 {0}:（请以管理员身份运行），错误 {1}", drive, unsafe { GetLastError() })); }
    let _volume = Volume(h);
    let mut vd: NTFS_VOLUME_DATA_BUFFER = unsafe { std::mem::zeroed() };
    let mut n = 0u32;
    let ok = unsafe { DeviceIoControl(h, FSCTL_GET_NTFS_VOLUME_DATA, ptr::null(), 0, &mut vd as *mut _ as _, size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32, &mut n, ptr::null_mut()) };
    if ok == 0 { return Err(tf!("{0}: не NTFS (код {1})", "{0}: is not NTFS (error {1})", ja: "{0}: は NTFS ではありません (エラー {1})", pt: "{0}: não é NTFS (erro {1})", zh: "{0}: 不是 NTFS（错误 {1}）", drive, unsafe { GetLastError() })); }
    let (rs, cl) = (vd.BytesPerFileRecordSegment as usize, vd.BytesPerCluster as u64);
    // Record 0 is $MFT itself: its data runs say where the whole table lives, then we read it in big chunks.
    let mut out = vec![0u8; 12 + rs];
    let inp = NTFS_FILE_RECORD_INPUT_BUFFER { FileReferenceNumber: 0 };
    let ok = unsafe { DeviceIoControl(h, FSCTL_GET_NTFS_FILE_RECORD, &inp as *const _ as _, size_of::<NTFS_FILE_RECORD_INPUT_BUFFER>() as u32, out.as_mut_ptr() as _, out.len() as u32, &mut n, ptr::null_mut()) };
    if ok == 0 { return Err(tf!("не удалось прочитать запись $MFT, код {0}", "cannot read the $MFT record, error {0}", ja: "$MFT レコードを読めません、エラー {0}", pt: "não foi possível ler o registro $MFT, erro {0}", zh: "无法读取 $MFT 记录，错误 {0}", unsafe { GetLastError() })); }
    let rec0 = out[12..12 + rs].to_vec();
    // the ioctl already hands back a record with the fixups undone (raw reads below do not)
    let runs = data_runs(&rec0).ok_or_else(|| t!("в записи $MFT нет потоков $DATA", "no $DATA runs in the $MFT record", ja: "$MFT レコードに $DATA のランがありません", pt: "não há runs $DATA no registro $MFT", zh: "$MFT 记录里没有 $DATA 数据运行").to_string())?;
    let total = (vd.MftValidDataLength as u64) / rs as u64;
    let mut pace = Pace::new(total);
    pace.at(0, progress);
    let mut left = vd.MftValidDataLength as u64; // bytes of the table still to read
    let mut recs = HashMap::with_capacity(left as usize / rs / 2);
    let (mut idx, mut buf) = (0u32, vec![0u8; 4 << 20]);
    let (mut seen, mut magic) = (0u32, 0u32);
    for (lcn, clusters) in runs {
        let mut len = (clusters * cl).min(left);
        left -= len;
        if lcn == 0 { idx += (len / rs as u64) as u32; pace.at(u64::from(idx), progress); continue; } // sparse hole
        let mut pos = (lcn * cl) as i64;
        while len > 0 {
            let want = len.min(buf.len() as u64) as usize;
            let mut got = 0u32;
            let ok = unsafe { SetFilePointerEx(h, pos, ptr::null_mut(), FILE_BEGIN) != 0 && ReadFile(h, buf.as_mut_ptr() as _, want as u32, &mut got, ptr::null_mut()) != 0 };
            if !ok || got as usize != want { return Err(tf!("не удалось прочитать MFT на байте {0}, код {1}", "reading the MFT failed at byte {0}, error {1}", ja: "バイト {0} で MFT を読めません、エラー {1}", pt: "falha ao ler a MFT no byte {0}, erro {1}", zh: "读取 MFT 在字节 {0} 处失败，错误 {1}", pos, unsafe { GetLastError() })); }
            for c in buf[..want].chunks_exact_mut(rs) {
                seen += 1;
                magic += (&c[..4] == b"FILE") as u32;
                parse_slot(&mut recs, idx, c);
                idx += 1;
            }
            pace.at(u64::from(idx), progress);
            pos += want as i64;
            len -= want as u64;
        }
    }
    pace.end(u64::from(idx), progress);
    recs.retain(|_, r| !r.name.is_empty()); // no $FILE_NAME anywhere (e.g. attribute list in a record we never saw)
    eprintln!("mft: {seen} slots read, {magic} with FILE magic, {} named", recs.len());
    Ok(recs)
}

pub fn path_of(recs: &HashMap<u32, Rec>, mut id: u32, drive: char) -> (String, usize) {
    let mut parts = vec![];
    for _ in 0..128 {
        if id == ROOT { break; }
        let Some(r) = recs.get(&id) else { parts.push("?".into()); break };
        parts.push(r.name.clone());
        id = r.parent;
    }
    let d = parts.len();
    parts.reverse();
    (format!("{drive}:\\{}", parts.join("\\")), d)
}

pub fn human(b: u64) -> String {
    let (mut v, mut i) = (b as f64, 0);
    while v >= 1024.0 && i < 4 { v /= 1024.0; i += 1; }
    format!("{v:.1} {}", ["B", "KB", "MB", "GB", "TB"][i])
}

/// Bytes under every record (a folder = everything below it), summed up the parent chain. Skips root and $BadClus.
pub fn totals(recs: &HashMap<u32, Rec>, logical: bool) -> HashMap<u32, u64> {
    let mut tot: HashMap<u32, u64> = HashMap::new();
    for (&id, r) in recs {
        let s = if logical { r.logical } else { r.alloc };
        if s == 0 || id == ROOT || id == 8 { continue; } // 8 = $BadClus, a sparse file that claims the whole volume
        *tot.entry(id).or_default() += s;
        let mut p = r.parent;
        for _ in 0..128 { // the cap also breaks parent cycles
            *tot.entry(p).or_default() += s;
            if p == ROOT { break; }
            p = match recs.get(&p) { Some(x) => x.parent, None => break };
        }
    }
    tot
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn progress_is_about_48_steps_from_nothing_to_the_whole_table() {
        // a table of 2,050,112 records read in chunks of 4,096, with a sparse hole in the middle
        let total = 2_050_112u64;
        let (mut pace, mut seen) = (Pace::new(total), Vec::new());
        let mut note = |r: u64, t: u64| seen.push((r, t));
        pace.at(0, &mut note);
        let mut read = 0u64;
        while read < total {
            read = if read == 1_000_000 { 1_300_000 } else { (read + 4096).min(total) };
            pace.at(read, &mut note);
        }
        pace.end(read, &mut note);
        assert_eq!(seen.first(), Some(&(0, total)));
        assert_eq!(seen.last(), Some(&(total, total)));
        assert!((45..=52).contains(&seen.len()), "{} reports", seen.len());
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0 && w[0].1 == w[1].1), "never goes back");
        // a table of one record still says where it started and that it ended
        let (mut pace, mut one) = (Pace::new(1), Vec::new());
        pace.at(0, &mut |r, t| one.push((r, t)));
        pace.at(1, &mut |r, t| one.push((r, t)));
        pace.end(1, &mut |r, t| one.push((r, t)));
        assert_eq!(one.first(), Some(&(0, 1)));
        assert_eq!(one.last(), Some(&(1, 1)));
    }

    #[test]
    fn parsed_slots_match_with_and_without_progress() {
        fn run(progress: &mut dyn FnMut(u64, u64)) -> HashMap<u32, Rec> {
            let mut slots = vec![vec![0u8; 1024]; 3];
            for slot in &mut slots {
                slot[..4].copy_from_slice(b"FILE");
                slot[4..6].copy_from_slice(&48u16.to_le_bytes());
                slot[6..8].copy_from_slice(&1u16.to_le_bytes());
                slot[0x14..0x16].copy_from_slice(&56u16.to_le_bytes());
                slot[0x16..0x18].copy_from_slice(&1u16.to_le_bytes());
                slot[0x18..0x1c].copy_from_slice(&64u32.to_le_bytes());
                slot[56..60].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
            }
            let mut recs = HashMap::new();
            for (i, slot) in slots.iter_mut().enumerate() { parse_slot(&mut recs, i as u32, slot); progress(i as u64 + 1, 3); }
            recs
        }
        let a = run(&mut |_, _| {});
        let mut calls = Vec::new();
        let b = run(&mut |read, total| calls.push((read, total)));
        assert_eq!(a.len(), 3);
        for (id, r) in a { let other = &b[&id]; assert_eq!((&r.name, r.parent, r.dir, r.alloc, r.logical), (&other.name, other.parent, other.dir, other.alloc, other.logical)); }
        assert_eq!(calls.last(), Some(&(3, 3)));
    }
}
