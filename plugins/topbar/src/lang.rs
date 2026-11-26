//! topbar: lang (split out of main.rs; shares its items through `super::*`).
use super::*;

/// Next keyboard layout for the app window. Posted to its focused control (that is where Chrome, Office... look for it);
/// timer 7 checks it took and otherwise presses Win+Space, the system switcher (games, terminals ignore the message).
/// The installed keyboard layouts and the one the app in front uses.
pub(crate) unsafe fn layouts() -> (Vec<HKL>, HKL) {
    let mut l: [HKL; 32] = [null_mut(); 32];
    let n = GetKeyboardLayoutList(32, l.as_mut_ptr()).clamp(0, 32) as usize;
    (l[..n].to_vec(), GetKeyboardLayout(GetWindowThreadProcessId(app_window(), null_mut())))
}

/// A layout's language as Windows names it in the user's language ("Русский (Россия)"), and its short code ("RUS").
pub(crate) unsafe fn layout_names(k: HKL) -> (String, String) {
    let id = k as usize as u32 & 0xffff;
    let get = |what: u32| {
        let mut b = [0u16; 96];
        let n = GetLocaleInfoW(id, what, b.as_mut_ptr(), 96).max(1) as usize - 1;
        String::from_utf16_lossy(&b[..n])
    };
    let short = get(3).to_uppercase(); // LOCALE_SABBREVLANGNAME
    (get(2), if short == "ENU" { "ENG".into() } else { short }) // LOCALE_SLOCALIZEDDISPLAYNAME
}

/// The registry key of a layout under `HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts` (its KLID), from the layout
/// handle: the low word is the language, the high word the layout (equal to the language for the default one). A high word
/// with `F` as its top digit (a second layout of the same language) is looked up by its "Layout Id" value instead: `None`.
fn klid(hkl: u32) -> Option<String> {
    let (lo, hi) = (hkl & 0xffff, hkl >> 16);
    match hi {
        0xf000..=0xffff => None,
        0 => Some(format!("0000{lo:04X}")),
        h if h == lo => Some(format!("0000{lo:04X}")),
        h => Some(format!("{h:04X}{lo:04X}")),
    }
}

/// A string value under `HKLM\...\Keyboard Layouts\<klid>`.
unsafe fn layout_value(klid: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW};
    let path = wide(&format!("SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\{klid}"));
    let mut key: HKEY = null_mut();
    if RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) != 0 {
        return None;
    }
    let mut buf = [0u16; 256];
    let mut len = size_of_val(&buf) as u32;
    let r = RegQueryValueExW(key, wide(name).as_ptr(), null(), null_mut(), buf.as_mut_ptr().cast(), &mut len);
    RegCloseKey(key);
    let n = (len as usize / 2).min(buf.len());
    (r == 0).then(|| String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').to_string()).filter(|s| !s.is_empty())
}

/// The layout's name in the user's language ("US", "Russian"), from the registry: its resource-string display name when that
/// loads, else the plain "Layout Text".
unsafe fn layout_text(k: HKL) -> String {
    let hkl = k as usize as u32;
    let id = klid(hkl).unwrap_or_else(|| {
        // a second layout of a language: the key whose "Layout Id" is the high word's low 12 bits
        let lid = format!("{:04X}", (hkl >> 16) & 0x0fff);
        let lang = format!("{:04X}", hkl & 0xffff);
        (0xa000u32..=0xa0ffu32)
            .map(|n| format!("{n:04X}{lang}"))
            .find(|c| layout_value(c, "Layout Id").is_some_and(|v| v.eq_ignore_ascii_case(&lid)))
            .unwrap_or_default()
    });
    if id.is_empty() {
        return String::new();
    }
    if let Some(res) = layout_value(&id, "Layout Display Name") {
        let mut b = [0u16; 128];
        if SHLoadIndirectString(wide(&res).as_ptr(), b.as_mut_ptr(), 128, null_mut()) >= 0 {
            let n = b.iter().position(|&c| c == 0).unwrap_or(0);
            if n > 0 {
                return String::from_utf16_lossy(&b[..n]);
            }
        }
    }
    layout_value(&id, "Layout Text").unwrap_or_default()
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// "Language · layout": just the layout when its name already says the language ("Turkish Q").
fn sub_line(lang: &str, layout: &str) -> String {
    let (l, k) = (lang.to_lowercase(), layout.to_lowercase());
    match (layout.is_empty(), k.contains(&l) || l.contains(&k)) {
        (true, _) => lang.to_string(),
        (false, true) => layout.to_string(),
        (false, false) => format!("{lang} \u{b7} {layout}"),
    }
}

/// What the keyboard layout panel lists: every installed layout, the current one marked.
pub(crate) unsafe fn layout_rows() -> Vec<ui::lang_popup::Layout> {
    let (list, cur) = layouts();
    list.iter()
        .map(|&k| {
            let id = k as usize as u32 & 0xffff;
            let get = |what: u32| {
                let mut b = [0u16; 96];
                let n = GetLocaleInfoW(id, what, b.as_mut_ptr(), 96).max(1) as usize - 1;
                String::from_utf16_lossy(&b[..n])
            };
            // LOCALE_SNATIVELANGUAGENAME: the language in its own tongue; LOCALE_SLOCALIZEDLANGUAGENAME: in the user's
            let (name, local) = (capital(&get(0x04)), capital(&get(0x6f)));
            ui::lang_popup::Layout { id: k as isize, code: layout_names(k).1, name, sub: sub_line(&local, &layout_text(k)), on: k == cur }
        })
        .collect()
}

fn pick_layout(id: isize) {
    // SAFETY: asks the app in front to switch to one of the installed layouts.
    unsafe { set_lang(BAR.load(Relaxed) as HWND, id as HKL) }
}

fn language_settings() {
    // SAFETY: opens a settings URI.
    unsafe { open(&wide("ms-settings:regionlanguage")) }
}

/// The language button was clicked: open the keyboard layout panel above it, or close it.
pub(crate) unsafe fn lang_toggle() {
    ui::panel::toggle(ui::panel::Id::Lang, || {
        let a = anchor(LANG)?;
        Some((Box::new(ui::lang_popup::LangPanel::new(layout_rows(), pick_layout, language_settings)), a))
    });
}

/// The window in front is one of the bar's own (Start, a flyout).
pub(crate) unsafe fn own_foreground() -> bool {
    let mut pid = 0u32;
    GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
    pid == std::process::id()
}

/// The next layout (or the previous one with `back`, Win+Shift+Space) for the app in front.
pub(crate) unsafe fn next_lang(bar: HWND, back: bool) {
    let own = own_foreground(); // our Start in front: switch this thread (app_window() is the app behind it)
    let (l, cur) = if own { (layouts().0, GetKeyboardLayout(0)) } else { layouts() };
    if l.len() < 2 {
        return; // one layout: nothing to switch to
    }
    let i = l.iter().position(|&k| k == cur);
    let n = l.len();
    let next = l[match (i, back) { (Some(i), false) => (i + 1) % n, (Some(i), true) => (i + n - 1) % n, (None, _) => 0 }];
    if own {
        ActivateKeyboardLayout(next, 0);
        InvalidateRect(bar, null(), 0);
    } else {
        set_lang(bar, next);
    }
}

/// Asks the app in front to switch to layout `next` (the bar's own thread cannot change another app's language).
pub(crate) unsafe fn set_lang(bar: HWND, next: HKL) {
    let w = app_window();
    let tid = GetWindowThreadProcessId(w, null_mut());
    let cur = GetKeyboardLayout(tid);
    let mut gi: GUITHREADINFO = zeroed();
    gi.cbSize = size_of::<GUITHREADINFO>() as u32;
    let target = if GetGUIThreadInfo(tid, &mut gi) != 0 && !gi.hwndFocus.is_null() { gi.hwndFocus } else { w };
    PostMessageW(target, WM_INPUTLANGCHANGEREQUEST, 0, next as isize);
    LANG_TRY.set((tid, cur as usize, next as usize));
    SetTimer(bar, 7, 250, None);
}

pub(crate) unsafe fn lang_name() -> String {
    let tid = GetWindowThreadProcessId(app_window(), null_mut());
    let id = GetKeyboardLayout(tid) as usize as u32 & 0xffff;
    let mut b = [0u16; 16];
    let n = GetLocaleInfoW(id, 3, b.as_mut_ptr(), 16).max(1) as usize - 1; // LOCALE_SABBREVLANGNAME
    let s = String::from_utf16_lossy(&b[..n]).to_uppercase();
    if s == "ENU" { "ENG".into() } else { s }
}

thread_local! {
    pub(crate) static SIG: Cell<(u64, u32)> = const { Cell::new((0, 0)) }; // (hash of the last painted state, ticks since the last repaint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn klid_of_a_layout_handle() {
        assert_eq!(klid(0x0419_0419).as_deref(), Some("00000419")); // the default layout of Russian
        assert_eq!(klid(0x0409_0409).as_deref(), Some("00000409"));
        assert_eq!(klid(0x0002_0409).as_deref(), Some("00020409")); // United States-International
        assert_eq!(klid(0xf0a8_0419), None); // looked up by its Layout Id
    }

    #[test]
    fn the_sub_line_does_not_say_the_language_twice() {
        assert_eq!(sub_line("Turkish", "Turkish Q"), "Turkish Q");
        assert_eq!(sub_line("English", "US"), "English \u{b7} US");
        assert_eq!(sub_line("Russian", "Russian"), "Russian");
        assert_eq!(sub_line("Russian", ""), "Russian");
        assert_eq!(capital("русский"), "Русский");
    }
}
