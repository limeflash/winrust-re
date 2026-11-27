//! "Show more options" for a selection Windows' own menu was too slow for.
//!
//! Windows builds the whole menu in one `QueryContextMenu` on one thread, so one handler that waits for a process that is not
//! there (the two Pin handlers wait for explorer.exe) takes the whole menu with it. Here the list is made the way Windows makes
//! it, but one handler at a time and each on a thread of its own with a budget of its own:
//!
//! * the handlers are read from the registry, as the shell reads them: `shellex\ContextMenuHandlers` of the selection's
//!   association keys (its ProgID such as `lnkfile`, the extension, `SystemFileAssociations\<ext>`, `*`, `AllFilesystemObjects`,
//!   `Directory` / `Folder` for folders), less the ones Windows has blocked, the two Pin handlers while explorer.exe is not
//!   running (our own Pin rows cover them) and the ones that hung before;
//! * each handler is created, given the selection (`IShellExtInit::Initialize`) and asked for its items in a command number range
//!   of its own, on a worker (an STA thread) of its own; a handler that has not answered in [`PART_BUDGET`] is given up (its thread
//!   stays where it is, it and any later call to it are forgotten);
//! * the shell's own commands (Cut, Copy, Create shortcut, Delete, Properties and the static verbs of the association keys) come
//!   from the default context menu made over copies of the keys' `shell` branches only, so it loads no handlers of its own;
//! * what has a submenu (Send to, Open with, 7-Zip) is opened, and the list is laid flat: the bar draws two levels, and this list
//!   is the second. The submenu's name stands as a dimmed heading over its items.
//!
//! Running an item goes to the worker that owns its command number, which gives the shell its offset in its own range.
use super::*;
use std::collections::HashMap;
use windows::Win32::{
    Foundation::ERROR_SUCCESS,
    System::{
        Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, IDataObject},
        Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS},
        Registry::{
            HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_READ, REG_OPTION_VOLATILE, RRF_RT_ANY, RRF_RT_REG_SZ, RegCloseKey, RegCopyTreeW,
            RegCreateKeyExW, RegDeleteTreeW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
        },
    },
    UI::Shell::Common::ITEMIDLIST,
};
use windows::Win32::System::{Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, IDropTarget}, SystemServices::MODIFIERKEYS_FLAGS};

/// Command numbers a part may use: the default menu has 1..=0x100, the handlers the ranges after it.
pub(super) const RANGE: u32 = 0x100;
/// How long a handler has to give its items, all of them started together.
pub(super) const PART_BUDGET: Duration = Duration::from_millis(300);
/// How long the submenus of what came have to fill.
pub(super) const FILL_BUDGET: Duration = Duration::from_millis(600);
/// The number of the dimmed heading of a flattened submenu (it can never be chosen).
const HEADING: u32 = 0x7FF0;
/// Handlers asked at most, so the ranges (126 of them) are enough.
const MAX_PARTS: usize = 120;

/// "Pin to Start" (`exefile`) and the shortcut's own handler (`lnkfile`: Pin to taskbar, ...): both ask explorer.exe.
const PIN_START: u128 = 0x470C0EBD_5D73_4D58_9CED_E91E22E23282;
const SHELL_LINK: u128 = 0x00021401_0000_0000_C000_000000000046;
/// Windows' "Send to" handler: it fills its submenu through a window the default menu gives it, and asked on its own it stays
/// empty; the entries of the SendTo folder are read and the selection dropped on one instead ([`build_send_to`]).
const SEND_TO: u128 = 0x7BA4C740_9E81_11CF_99D3_00AA004AE837;

// ---------------------------------------------------------------------------------------------------------------------
// the registry, as far as this needs it

fn w16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// An open key, closed with it.
struct Key(HKEY);

impl Key {
    fn open(root: HKEY, path: &str) -> Option<Key> {
        let mut k = HKEY::default();
        // SAFETY: a NUL-terminated path; the key is ours when it opened.
        unsafe { RegOpenKeyExW(root, PCWSTR(w16(path).as_ptr()), None, KEY_READ, &mut k) }.is_ok().then_some(Key(k))
    }

    fn class(path: &str) -> Option<Key> {
        Self::open(HKEY_CLASSES_ROOT, path)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: our own key, closed once.
        unsafe { let _ = RegCloseKey(self.0); }
    }
}

/// A string value of `k` (`sub` below it, `name`; none: the default value).
fn value(k: HKEY, sub: Option<&str>, name: Option<&str>) -> Option<String> {
    let (sub, name) = (sub.map(w16), name.map(w16));
    let p = |v: &Option<Vec<u16>>| v.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr()));
    let mut buf = [0u16; 1024];
    let mut cb = (buf.len() * 2) as u32;
    // SAFETY: the buffer is as long as the call is told.
    let r = unsafe { RegGetValueW(k, p(&sub), p(&name), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut cb)) };
    if r != ERROR_SUCCESS { return None; }
    Some(String::from_utf16_lossy(&buf[..(cb as usize / 2).min(buf.len())]).trim_end_matches('\0').to_string())
}

fn subkeys(k: HKEY) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 256];
        let mut n = buf.len() as u32;
        // SAFETY: the buffer is as long as the call is told.
        let r = unsafe { RegEnumKeyExW(k, i, Some(PWSTR(buf.as_mut_ptr())), &mut n, None, None, None, None) };
        if r != ERROR_SUCCESS { break; }
        out.push(String::from_utf16_lossy(&buf[..n as usize]));
    }
    out
}

/// `{470C0EBD-5D73-4d58-9CED-E91E22E23282}` as a number.
fn parse_guid(s: &str) -> Option<u128> {
    let t: String = s.trim().trim_matches(['{', '}']).chars().filter(|c| *c != '-').collect();
    (t.len() == 32).then(|| u128::from_str_radix(&t, 16).ok()).flatten()
}

fn fmt_guid(g: u128) -> String {
    format!("{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}", g >> 96, (g >> 80) & 0xFFFF, (g >> 64) & 0xFFFF, (g >> 48) & 0xFFFF, g & 0xFFFF_FFFF_FFFF)
}

/// Whether Windows has blocked the handler (Shell Extensions\Blocked).
fn blocked(g: u128) -> bool {
    let name = w16(&fmt_guid(g));
    let sub = w16(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked");
    // SAFETY: NUL-terminated names; only whether the value is there is asked.
    unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, PCWSTR(sub.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_ANY, None, None, None) }.is_ok()
}

fn explorer_running() -> bool {
    // SAFETY: a snapshot of the process list, walked and closed here.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return true };
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = false;
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            let n = &e.szExeFile[..e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len())];
            if String::from_utf16_lossy(n).eq_ignore_ascii_case("explorer.exe") {
                found = true;
                break;
            }
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
        found
    }
}

/// The handlers that hung (they are not asked again this session).
static HUNG: Mutex<Option<HashSet<u128>>> = Mutex::new(None);

fn with_hung<T>(f: impl FnOnce(&mut HashSet<u128>) -> T) -> T {
    f(HUNG.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(HashSet::new))
}

// ---------------------------------------------------------------------------------------------------------------------
// which handlers

/// The keys under HKEY_CLASSES_ROOT whose handlers and verbs belong to this selection, most specific first.
fn assoc_keys(paths: &[PathBuf]) -> Vec<String> {
    let kind = kind_of(paths);
    let strs = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    if kind == "<dir>" {
        return strs(&["Directory", "Folder", "AllFilesystemObjects"]);
    }
    if kind.contains("<dir>") {
        return strs(&["AllFilesystemObjects"]);
    }
    if kind.contains('+') || kind.is_empty() {
        return strs(&["*", "AllFilesystemObjects"]);
    }
    let mut out = Vec::new();
    let progid = Key::class(&kind).and_then(|k| value(k.0, None, None)).filter(|p| !p.is_empty() && Key::class(p).is_some());
    let perceived = Key::class(&kind).and_then(|k| value(k.0, None, Some("PerceivedType")));
    out.extend(progid.clone());
    out.push(kind.clone());
    out.push(format!(r"SystemFileAssociations\{kind}"));
    out.extend(perceived.filter(|p| !p.is_empty()).map(|p| format!(r"SystemFileAssociations\{p}")));
    if progid.is_none() { out.push("Unknown".into()); }
    out.extend(strs(&["*", "AllFilesystemObjects"]));
    out
}

/// A context menu handler of an association key.
#[derive(Clone, Debug)]
struct Spec {
    name: String,
    clsid: u128,
    /// The key it was found under (what the shell gives it as its ProgID key).
    key: String,
}

/// The handlers of `keys`, each CLSID once, less those `skip` says no to.
fn handlers(keys: &[String], skip: &dyn Fn(u128) -> bool) -> Vec<Spec> {
    let mut out: Vec<Spec> = Vec::new();
    for key in keys {
        let Some(root) = Key::class(&format!(r"{key}\shellex\ContextMenuHandlers")) else { continue };
        for name in subkeys(root.0) {
            // the key's value is the handler's CLSID; a key that is itself a CLSID needs no value
            let Some(clsid) = value(root.0, Some(&name), None).as_deref().and_then(parse_guid).or_else(|| parse_guid(&name)) else { continue };
            if out.iter().any(|s| s.clsid == clsid) || skip(clsid) { continue; }
            out.push(Spec { name, clsid, key: key.clone() });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// asking one handler, and the default menu

/// The folder `p` is in, as an item id list (`None`: there is none).
fn parent_pidl(p: &Path) -> Option<*mut ITEMIDLIST> {
    let parent = p.parent()?;
    let mut pidl = std::ptr::null_mut();
    // SAFETY: a NUL-terminated path; the list is the caller's to free.
    unsafe { SHParseDisplayName(PCWSTR(w16(&parent.to_string_lossy()).as_ptr()), None, &mut pidl, 0, None) }.ok()?;
    Some(pidl)
}

/// One handler's items, asked for in its own range. Runs on the handler's worker.
fn build_handler(sp: &Spec, paths: &[PathBuf], first: u32) -> Option<(Box<dyn Backend>, Vec<ShellItem>)> {
    // SAFETY: COM calls on this worker's own apartment, on objects that stay on it.
    unsafe {
        let data: IDataObject = crate::sh::ui(HWND::default(), paths).ok()?;
        let init: IShellExtInit = CoCreateInstance(&GUID::from_u128(sp.clsid), None, CLSCTX_ALL).ok()?;
        let key = Key::class(&sp.key)?;
        let folder = parent_pidl(paths.first()?);
        let r = init.Initialize(folder.map(|p| p as *const ITEMIDLIST), &data, Some(key.0));
        if let Some(p) = folder { CoTaskMemFree(Some(p as *const c_void)); }
        r.ok()?;
        let cm: IContextMenu = init.cast().ok()?;
        let menu_h = CreatePopupMenu().ok()?;
        if cm.QueryContextMenu(menu_h, 0, first, first + RANGE - 1, CMF_NORMAL).is_err() {
            let _ = DestroyMenu(menu_h);
            return None;
        }
        let mut s = Session::new(cm, menu_h, paths, first);
        s.on_drop.push(Box::new(move || drop(key)));
        let (mut src, m) = s.src();
        let items = menu::read(m.0, &mut src);
        (!items.is_empty()).then(|| (Box::new(s) as Box<dyn Backend>, items))
    }
}

/// The shell's own commands: the default context menu over copies of the keys' `shell` branches (the static verbs) and no handlers
/// (it reads those from `shellex`, which the copies do not have). Runs on its worker.
fn build_defaults(paths: &[PathBuf], keys: &[String]) -> Option<(Box<dyn Backend>, Vec<ShellItem>)> {
    static N: AtomicU32 = AtomicU32::new(0);
    // SAFETY: COM and registry calls on this worker's own apartment, on keys made here and deleted with the session.
    unsafe {
        let mut abs: Vec<*mut ITEMIDLIST> = Vec::new();
        let free = |v: &[*mut ITEMIDLIST]| v.iter().for_each(|p| CoTaskMemFree(Some(*p as *const c_void)));
        for p in paths {
            let mut pidl = std::ptr::null_mut();
            if SHParseDisplayName(PCWSTR(w16(&p.to_string_lossy()).as_ptr()), None, &mut pidl, 0, None).is_err() {
                free(&abs);
                return None;
            }
            abs.push(pidl);
        }
        let parent: IShellFolder = SHBindToParent(*abs.first()?, None).ok()?;
        let mut kids: Vec<*mut ITEMIDLIST> = abs.iter().map(|&p| ILFindLastID(p)).collect();
        // the keys: a volatile copy of each `shell` branch
        let root = format!(r"Software\WinRustTmp\{}-{}", std::process::id(), N.fetch_add(1, Relaxed));
        let mut made: Vec<Key> = Vec::new();
        for (i, a) in keys.iter().enumerate() {
            let Some(shell) = Key::class(&format!(r"{a}\shell")) else { continue };
            let (mut top, mut dst) = (HKEY::default(), HKEY::default());
            let at = w16(&format!(r"{root}\{i}"));
            if RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(at.as_ptr()), None, PCWSTR::null(), REG_OPTION_VOLATILE, KEY_ALL_ACCESS, None, &mut top, None) != ERROR_SUCCESS { continue; }
            let top = Key(top);
            if RegCreateKeyExW(top.0, windows::core::w!("shell"), None, PCWSTR::null(), REG_OPTION_VOLATILE, KEY_ALL_ACCESS, None, &mut dst, None) == ERROR_SUCCESS {
                let dst = Key(dst);
                let _ = RegCopyTreeW(shell.0, PCWSTR::null(), dst.0);
            }
            made.push(top);
        }
        let hk: Vec<HKEY> = made.iter().map(|k| k.0).collect();
        let dc = DEFCONTEXTMENU {
            hwnd: HWND::default(),
            pcmcb: std::mem::ManuallyDrop::new(None),
            pidlFolder: std::ptr::null_mut(),
            psf: std::mem::ManuallyDrop::new(Some(parent)),
            cidl: kids.len() as u32,
            apidl: kids.as_mut_ptr(),
            punkAssociationInfo: std::mem::ManuallyDrop::new(None),
            cKeys: hk.len() as u32,
            aKeys: if hk.is_empty() { std::ptr::null() } else { hk.as_ptr() },
        };
        let cm: Result<IContextMenu> = SHCreateDefaultContextMenu(&dc);
        free(&abs);
        let cm = cm.ok()?;
        let menu_h = CreatePopupMenu().ok()?;
        if cm.QueryContextMenu(menu_h, 0, 1, RANGE, CMF_NORMAL).is_err() {
            let _ = DestroyMenu(menu_h);
            return None;
        }
        let mut s = Session::new(cm, menu_h, paths, 1);
        // the keys stay for as long as the shell may read the static verbs from them (when one is run), then they go
        s.on_drop.push(Box::new(move || {
            drop(made);
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(w16(&root).as_ptr()));
        }));
        let (mut src, m) = s.src();
        let items = menu::read(m.0, &mut src);
        (!items.is_empty()).then(|| (Box::new(s) as Box<dyn Backend>, items))
    }
}

/// "Send to": the entries of the SendTo folder, and what runs one (the selection is dropped on it, as Windows does).
struct SendTo {
    folder: IShellFolder,
    kids: Vec<*mut ITEMIDLIST>,
    first: u32,
    paths: Vec<PathBuf>,
}

impl Drop for SendTo {
    fn drop(&mut self) {
        // SAFETY: the lists came from the folder's enumerator and are ours.
        for p in &self.kids { unsafe { CoTaskMemFree(Some(*p as *const c_void)) }; }
    }
}

impl Backend for SendTo {
    fn fill(&mut self, _: u32) -> Option<Vec<ShellItem>> {
        None
    }
    fn reread(&mut self) -> Vec<(u32, Vec<ShellItem>)> {
        Vec::new()
    }
    fn watching(&self) -> bool {
        false
    }
    fn run(&mut self, h: HWND, id: u32, _session: u32) -> bool {
        let Some(&child) = id.checked_sub(self.first).and_then(|i| self.kids.get(i as usize)) else { return false };
        // SAFETY: the folder's own drop target for one of its items, and a data object of the selection; the drop is what the shell does
        // for a click in its Send to menu.
        unsafe {
            let Ok(dt) = self.folder.GetUIObjectOf::<IDropTarget>(h, &[child as *const ITEMIDLIST], None) else { return false };
            let Ok(data) = crate::sh::ui::<IDataObject>(h, &self.paths) else { return false };
            let pt = windows::Win32::Foundation::POINTL { x: 0, y: 0 };
            let mut effect = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;
            if dt.DragEnter(&data, MODIFIERKEYS_FLAGS(0), pt, &mut effect).is_ok() {
                let mut effect = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;
                let _ = dt.Drop(&data, MODIFIERKEYS_FLAGS(0), pt, &mut effect);
            }
        }
        false
    }
}

/// The Send to entries (Windows' own "Compressed folder" is left out: our Compress to ZIP is the one), under a heading.
fn build_send_to(paths: &[PathBuf], first: u32) -> Option<(Box<dyn Backend>, Vec<ShellItem>)> {
    use windows::Win32::UI::Shell::{FOLDERID_SendTo, KF_FLAG_DEFAULT, SHGDN_NORMAL, SHGetDesktopFolder, SHGetKnownFolderPath, StrRetToBufW};
    // SAFETY: COM calls on this worker's own apartment; the lists are freed with the backend.
    unsafe {
        let dir = SHGetKnownFolderPath(&FOLDERID_SendTo, KF_FLAG_DEFAULT, None).ok()?;
        let mut pidl = std::ptr::null_mut();
        let parsed = SHParseDisplayName(PCWSTR(dir.0), None, &mut pidl, 0, None);
        CoTaskMemFree(Some(dir.0 as *const c_void));
        parsed.ok()?;
        let folder: windows::core::Result<IShellFolder> = SHGetDesktopFolder().and_then(|d| d.BindToObject(pidl, None));
        CoTaskMemFree(Some(pidl as *const c_void));
        let folder = folder.ok()?;
        let mut en: Option<IEnumIDList> = None;
        folder.EnumObjects(HWND::default(), (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0) as u32, &mut en).ok().ok()?;
        let en = en?;
        let zip = crate::sh::system_zip_name();
        let (mut kids, mut items) = (Vec::new(), vec![ShellItem::new(HEADING, "", "Send to")]);
        items[0].flags = fl::DISABLED;
        loop {
            let mut one = [std::ptr::null_mut::<ITEMIDLIST>()];
            let mut got = 0u32;
            if en.Next(&mut one, Some(&mut got)).is_err() || got == 0 { break; }
            let mut name = [0u16; 260];
            let mut sr = windows::Win32::UI::Shell::Common::STRRET::default();
            let label = (folder.GetDisplayNameOf(one[0], SHGDN_NORMAL, &mut sr).is_ok() && StrRetToBufW(&mut sr, Some(one[0] as *const ITEMIDLIST), &mut name).is_ok())
                .then(|| String::from_utf16_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())]));
            match label {
                Some(l) if !l.is_empty() && !zip.as_ref().is_some_and(|z| z.eq_ignore_ascii_case(&l)) && kids.len() < RANGE as usize => {
                    items.push(ShellItem::new(first + kids.len() as u32, "", &l));
                    kids.push(one[0]);
                }
                _ => CoTaskMemFree(Some(one[0] as *const c_void)),
            }
        }
        (!kids.is_empty()).then(|| (Box::new(SendTo { folder, kids, first, paths: paths.to_vec() }) as Box<dyn Backend>, items))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// putting the parts together

pub(super) type Builder = Box<dyn FnOnce() -> Option<(Box<dyn Backend>, Vec<ShellItem>)> + Send>;

/// Something that gives items in a range of its own: a handler, or the default menu.
pub(super) struct Part {
    /// What it is, for remembering that it hung (the handler's CLSID; 0 for the default menu).
    pub tag: u128,
    pub name: String,
    /// Its range: `first..first + RANGE`.
    pub first: u32,
    pub build: Builder,
}

/// The list as the parts of a menu came: each in order, a rule between them, and what runs an item.
struct Composite {
    owners: Vec<(u32, u32, Worker)>,
}

impl Backend for Composite {
    fn fill(&mut self, _: u32) -> Option<Vec<ShellItem>> {
        None // the list is flat: nothing is left to open
    }
    fn reread(&mut self) -> Vec<(u32, Vec<ShellItem>)> {
        Vec::new()
    }
    fn watching(&self) -> bool {
        false
    }
    fn run(&mut self, h: HWND, id: u32, session: u32) -> bool {
        if let Some((_, _, w)) = self.owners.iter().find(|(a, b, _)| (*a..=*b).contains(&id)) {
            // the owner tells the window when it is done
            w.send(Cmd::Run(h.0 as isize, id, session));
        }
        true
    }
}

/// `items` with every submenu that was opened laid flat: a dimmed heading with its name, then its items, rules around.
fn flatten(items: &[ShellItem], subs: &HashMap<u32, Vec<ShellItem>>) -> Vec<ShellItem> {
    fn rule(out: &mut Vec<ShellItem>) {
        if out.last().is_some_and(|l| !l.is_sep()) { out.push(ShellItem::sep()); }
    }
    let mut out: Vec<ShellItem> = Vec::new();
    for it in items {
        if let filemenu::Sub::Lazy(h) = it.sub {
            let kids: Vec<&ShellItem> = subs.get(&h).map(|v| v.iter().filter(|c| c.is_sep() || (c.id != 0 && !c.has_sub())).collect()).unwrap_or_default();
            if kids.iter().all(|c| c.is_sep()) { continue; }
            rule(&mut out);
            let mut head = ShellItem::new(HEADING, "", &it.label);
            head.flags = fl::DISABLED;
            head.icon = it.icon.clone();
            out.push(head);
            let mut started = false;
            for c in kids {
                if !c.is_sep() {
                    started = true;
                    out.push(c.clone());
                } else if started {
                    rule(&mut out);
                }
            }
            rule(&mut out);
        } else if it.is_sep() {
            rule(&mut out);
        } else if !it.has_sub() {
            out.push(it.clone());
        }
    }
    while out.last().is_some_and(ShellItem::is_sep) { out.pop(); }
    out
}

/// The finished list: an item that came twice (a shortcut's handler brings the target's menu, with its own Open) stays where it
/// came first, a heading with nothing under it goes, and rules neither double nor stand at an end.
fn finish(items: Vec<ShellItem>) -> Vec<ShellItem> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut out: Vec<ShellItem> = Vec::new();
    for it in items {
        let heading = it.id == HEADING && it.disabled();
        if !it.is_sep() && !heading && !seen.insert((it.verb.to_lowercase(), it.label.to_lowercase())) { continue; }
        if it.is_sep() && out.last().is_none_or(|l| l.is_sep()) { continue; }
        // a heading whose items were all duplicates is followed by a rule
        if it.is_sep() && out.last().is_some_and(|l| l.id == HEADING && l.disabled()) { out.pop(); continue; }
        out.push(it);
    }
    while out.last().is_some_and(|l| l.is_sep() || (l.id == HEADING && l.disabled())) { out.pop(); }
    out
}

/// Starts every part together; those that have not given their items within `budget` of now are given up (`hung` is told: tag and
/// name), those that have get their submenus opened for `fill` and are laid flat after `head`.
pub(super) fn compose(parts: Vec<Part>, head: Vec<ShellItem>, budget: Duration, fill: Duration, hung: &mut dyn FnMut(u128, &str)) -> Option<(Box<dyn Backend>, Vec<ShellItem>)> {
    struct Kept {
        first: u32,
        w: Worker,
        items: Vec<ShellItem>,
    }
    let started: Vec<(u128, String, u32, Worker)> = parts.into_iter().map(|p| (p.tag, p.name, p.first, spawn(p.build))).collect();
    let end = Instant::now() + budget;
    let mut kept: Vec<Kept> = Vec::new();
    for (tag, name, first, w) in started {
        match await_built(&w, end.saturating_duration_since(Instant::now())) {
            Built::Ready(items) => kept.push(Kept { first, w, items }),
            Built::Failed => {}
            Built::Slow => hung(tag, &name),
        }
    }
    // open the submenus (Send to, Open with, 7-Zip...) and wait for what they hold, but not for long
    let mut want: Vec<(usize, u32)> = Vec::new();
    for (i, k) in kept.iter().enumerate() {
        for it in &k.items {
            if let filemenu::Sub::Lazy(h) = it.sub {
                k.w.send(Cmd::Fill(h));
                want.push((i, h));
            }
        }
    }
    let mut got: HashMap<(usize, u32), Vec<ShellItem>> = HashMap::new();
    let (end, mut changed_at) = (Instant::now() + fill, Instant::now());
    while !want.is_empty() && Instant::now() < end {
        let mut changed = false;
        for (i, k) in kept.iter().enumerate() {
            while let Ok(r) = k.w.rep.try_recv() {
                if let Reply::Sub(h, items) = r {
                    got.insert((i, h), items);
                    changed = true;
                }
            }
        }
        if changed { changed_at = Instant::now(); }
        if want.iter().all(|k| got.contains_key(k)) && changed_at.elapsed() >= Duration::from_millis(100) { break; }
        std::thread::sleep(Duration::from_millis(15));
    }
    let mut out = head;
    let mut owners = Vec::new();
    for (i, k) in kept.into_iter().enumerate() {
        let subs: HashMap<u32, Vec<ShellItem>> = got.iter().filter(|((j, _), _)| *j == i).map(|((_, h), v)| (*h, v.clone())).collect();
        let block = flatten(&k.items, &subs);
        if block.is_empty() { continue; }
        if out.last().is_some_and(|l| !l.is_sep()) { out.push(ShellItem::sep()); }
        out.extend(block);
        owners.push((k.first, k.first + RANGE - 1, k.w));
    }
    let out = finish(out);
    (!out.is_empty()).then(|| (Box::new(Composite { owners }) as Box<dyn Backend>, out))
}

/// The whole list for `paths`: what the builder of the "Show more options" worker is. Runs on that worker.
pub(super) fn build_more(_hw: isize, paths: Vec<PathBuf>) -> Option<(Box<dyn Backend>, Vec<ShellItem>)> {
    let explorer = explorer_running();
    let keys = assoc_keys(&paths);
    let specs = handlers(&keys, &|g| g == SEND_TO || blocked(g) || with_hung(|h| h.contains(&g)) || (!explorer && (g == PIN_START || g == SHELL_LINK)));
    let mut parts: Vec<Part> = Vec::new();
    for (k, sp) in specs.into_iter().take(MAX_PARTS).enumerate() {
        let first = RANGE + 1 + k as u32 * RANGE;
        let ps = paths.clone();
        parts.push(Part { tag: sp.clsid, name: sp.name.clone(), first, build: Box::new(move || build_handler(&sp, &ps, first)) });
    }
    // Send to: the SendTo folder's own entries, after the handlers' (Windows puts it before Cut)
    let ps = paths.clone();
    let send_first = RANGE + 1 + parts.len() as u32 * RANGE;
    parts.push(Part { tag: 0, name: "sendto".into(), first: send_first, build: Box::new(move || build_send_to(&ps, send_first)) });
    // the shell's own commands go last, as in Windows' menu (Cut, Copy, Create shortcut, Delete, Properties)
    let (ps, ks) = (paths.clone(), keys);
    parts.push(Part { tag: 0, name: "defaults".into(), first: 1, build: Box::new(move || build_defaults(&ps, &ks)) });
    // Open and Run as administrator are ours: the verbs the shell has for them are static, and only a program's own key has them
    let head = own_items(&paths).into_iter().filter(|i| matches!(i.verb.as_str(), "open" | "runas")).collect();
    compose(parts, head, PART_BUDGET, FILL_BUDGET, &mut |tag, _| {
        if tag != 0 { with_hung(|h| _ = h.insert(tag)); }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handler of the test: its items, what its submenus hold, and what it was asked to run (never anything real).
    struct Fake {
        subs: HashMap<u32, Vec<ShellItem>>,
        log: Arc<Mutex<Vec<u32>>>,
    }

    impl Backend for Fake {
        fn fill(&mut self, h: u32) -> Option<Vec<ShellItem>> { self.subs.get(&h).cloned() }
        fn reread(&mut self) -> Vec<(u32, Vec<ShellItem>)> { Vec::new() }
        fn watching(&self) -> bool { false }
        fn run(&mut self, _: HWND, id: u32, _: u32) -> bool {
            self.log.lock().unwrap().push(id);
            true
        }
    }

    fn item(id: u32, verb: &str, label: &str) -> ShellItem {
        ShellItem::new(id, verb, label)
    }

    /// A part that answers after `delay` (`None`: never) with `items`, its submenus from `subs`.
    fn part(tag: u128, first: u32, delay: Option<Duration>, items: Option<Vec<ShellItem>>, subs: Vec<(u32, Vec<ShellItem>)>, log: &Arc<Mutex<Vec<u32>>>) -> Part {
        let log = Arc::clone(log);
        Part {
            tag,
            name: format!("part{tag}"),
            first,
            build: Box::new(move || {
                match delay {
                    Some(d) => std::thread::sleep(d),
                    None => loop { std::thread::park(); },
                }
                let items = items?;
                Some((Box::new(Fake { subs: subs.into_iter().collect(), log }) as Box<dyn Backend>, items))
            }),
        }
    }

    fn labels(items: &[ShellItem]) -> Vec<String> {
        items.iter().map(|i| if i.is_sep() { "-".into() } else { i.label.clone() }).collect()
    }

    #[test]
    fn a_handler_that_hangs_or_is_slow_is_given_up_and_the_others_make_the_list() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let lazy = |h| ShellItem { sub: filemenu::Sub::Lazy(h), ..item(0, "sendto", "Send to") };
        let parts = vec![
            part(1, 0x101, Some(Duration::ZERO), Some(vec![item(0x101, "open", "Open with Cursor"), lazy(1)]), vec![(1, vec![item(0x102, "", "Desktop"), item(0x103, "", "Documents")])], &log),
            part(2, 0x201, None, Some(vec![item(0x201, "", "Hangs")]), vec![], &log),
            part(3, 0x301, Some(Duration::from_secs(5)), Some(vec![item(0x301, "", "Too slow")]), vec![], &log),
            part(4, 0x401, Some(Duration::ZERO), None, vec![], &log),
            part(5, 0x501, Some(Duration::from_millis(100)), Some(vec![item(0x501, "", "Pinned elsewhere")]), vec![], &log),
            part(0, 1, Some(Duration::ZERO), Some(vec![item(25, "cut", "Cut"), item(18, "delete", "Delete")]), vec![], &log),
        ];
        let mut hung = Vec::new();
        let t = Instant::now();
        let (mut be, items) = compose(parts, vec![item(0x8001, "open", "Open")], Duration::from_millis(300), Duration::from_millis(500), &mut |tag, _| hung.push(tag)).expect("a list");
        let took = t.elapsed();
        assert!(took < Duration::from_millis(1200), "{took:?}: the budgets are the most it waits");
        assert_eq!(hung, [2, 3], "the one that never answers and the one that answers too late, and no other");
        assert_eq!(labels(&items), ["Open", "-", "Open with Cursor", "-", "Send to", "Desktop", "Documents", "-", "Pinned elsewhere", "-", "Cut", "Delete"], "{items:?}");
        assert!(items.iter().all(|i| !i.has_sub()), "the list is flat");
        assert!(items.iter().find(|i| i.label == "Send to").unwrap().disabled(), "a submenu's name is a dimmed heading");
        // running goes to the owner of the number, whatever its range's offset: nothing real is run
        be.run(HWND::default(), 0x103, 1);
        be.run(HWND::default(), 18, 1);
        be.run(HWND::default(), 0x201, 1); // the abandoned part's range: nobody owns it
        let end = Instant::now() + Duration::from_secs(3);
        while log.lock().unwrap().len() < 2 && Instant::now() < end { std::thread::sleep(Duration::from_millis(10)); }
        let mut ran = log.lock().unwrap().clone();
        ran.sort_unstable();
        assert_eq!(ran, [18, 0x103]);
    }

    #[test]
    fn handlers_that_all_fail_leave_only_our_own_rows_and_nothing_at_all_leaves_nothing() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let got = compose(vec![part(1, 0x101, Some(Duration::ZERO), None, vec![], &log)], vec![item(0x8001, "open", "Open")], Duration::from_millis(200), Duration::from_millis(100), &mut |_, _| {});
        assert_eq!(labels(&got.unwrap().1), ["Open"]);
        assert!(compose(vec![], vec![], Duration::from_millis(50), Duration::from_millis(50), &mut |_, _| {}).is_none());
    }

    #[test]
    fn a_submenu_with_nothing_in_it_leaves_no_heading_and_rules_never_double() {
        let lazy = ShellItem { sub: filemenu::Sub::Lazy(3), ..item(0, "", "Give access to") };
        let items = vec![item(1, "", "A"), ShellItem::sep(), lazy.clone(), ShellItem::sep(), item(2, "", "B"), ShellItem::sep()];
        let flat = flatten(&items, &HashMap::new());
        assert_eq!(labels(&flat), ["A", "-", "B"]);
        let mut subs = HashMap::new();
        subs.insert(3, vec![ShellItem::sep(), item(5, "", "X"), ShellItem::sep(), ShellItem::sep(), item(6, "", "Y")]);
        assert_eq!(labels(&flatten(&items, &subs)), ["A", "-", "Give access to", "X", "-", "Y", "-", "B"]);
    }

    #[test]
    fn what_came_twice_stays_where_it_came_first_and_no_heading_or_rule_is_left_alone() {
        let head = |l: &str| ShellItem { flags: fl::DISABLED, ..item(HEADING, "", l) };
        let list = vec![item(1, "open", "Open"), ShellItem::sep(), item(5, "open", "OPEN"), head("7-Zip"), item(6, "open", "Open"), ShellItem::sep(), item(7, "", "Copy as path"), ShellItem::sep(), head("Send to")];
        assert_eq!(labels(&finish(list)), ["Open", "-", "Copy as path"]);
    }

    #[test]
    fn handler_clsids_are_read_and_written_as_windows_does() {
        assert_eq!(parse_guid("{470C0EBD-5D73-4d58-9CED-E91E22E23282}"), Some(PIN_START));
        assert_eq!(parse_guid("{00021401-0000-0000-C000-000000000046}"), Some(SHELL_LINK));
        assert_eq!(parse_guid("OpenContainingFolderMenu"), None);
        assert_eq!(fmt_guid(PIN_START), "{470C0EBD-5D73-4D58-9CED-E91E22E23282}");
        assert_eq!(fmt_guid(SHELL_LINK), "{00021401-0000-0000-C000-000000000046}");
    }

    #[test]
    fn the_association_keys_of_a_selection_are_read_from_the_registry() {
        let txt = assoc_keys(&[PathBuf::from(r"C:\x\a.TXT")]);
        assert!(txt.contains(&".txt".to_string()) && txt.contains(&"AllFilesystemObjects".to_string()) && txt.contains(&"*".to_string()), "{txt:?}");
        assert!(assoc_keys(&[PathBuf::from(r"C:\x\a.lnk")]).first().is_some_and(|k| k == "lnkfile"), "a shortcut's ProgID comes first");
        let dir = std::env::temp_dir();
        assert_eq!(assoc_keys(std::slice::from_ref(&dir)), ["Directory", "Folder", "AllFilesystemObjects"]);
        assert_eq!(assoc_keys(&[PathBuf::from("a.txt"), PathBuf::from("b.png")]), ["*", "AllFilesystemObjects"]);
        // the handlers of a shortcut on this machine, less what is skipped: each CLSID once, the skipped ones absent
        let all = handlers(&["lnkfile".into(), "AllFilesystemObjects".into(), "*".into()], &|_| false);
        let some = handlers(&["lnkfile".into(), "AllFilesystemObjects".into(), "*".into()], &|g| g == PIN_START || g == SHELL_LINK);
        assert!(all.len() >= some.len() && !some.iter().any(|s| s.clsid == PIN_START || s.clsid == SHELL_LINK));
        let mut ids: Vec<u128> = all.iter().map(|s| s.clsid).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
    }

    /// The whole list of a real temp file, built the way the desktop builds it for a slow shell (handler by handler, with budgets)
    /// and only read: nothing is run. It never waits more than 30 s.
    #[test]
    fn a_real_list_of_a_temp_file_is_made_handler_by_handler_and_nothing_is_run() {
        crate::sh::init();
        let d = std::env::temp_dir().join(format!("wr-more-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("note.txt");
        std::fs::write(&f, b"x").unwrap();
        let w = spawn({
            let ps = vec![f.clone()];
            move || build_more(0, ps)
        });
        let built = await_built(&w, Duration::from_secs(30));
        let _ = std::fs::remove_dir_all(&d);
        let Built::Ready(items) = built else { panic!("a list in 30 s: {built:?}") };
        let verbs: Vec<&str> = items.iter().map(|i| i.verb.as_str()).collect();
        assert!(verbs.contains(&"properties") && verbs.contains(&"copy") && verbs.contains(&"cut") && verbs.contains(&"delete"), "the shell's own commands: {verbs:?}");
        assert!(items.iter().all(|i| !i.has_sub()), "flat");
        assert!(items.iter().any(|i| i.verb == "open"), "ours: {verbs:?}");
    }
}
