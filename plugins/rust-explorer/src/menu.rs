//! The folder's own menu, painted in the window's palette (glyph, label, check mark, submenus) instead of the grey system one.
//! Each owner-drawn item starts with an MSAAMENUINFO, so screen readers still read its label.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::ui::{fill, font, pal, sc, text, ui_face};
use std::cell::RefCell;
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::{Controls::*, WindowsAndMessaging::*}};

#[repr(C)]
struct Entry {
    msaa_sig: u32, // MSAAMENUINFO: signature, length, text
    msaa_len: u32,
    msaa_text: *const u16,
    label: Vec<u16>,
    glyph: &'static str,
    checked: bool,
    sep: bool,
    sub: bool,
}

thread_local! {
    static ITEMS: RefCell<Vec<Box<Entry>>> = const { RefCell::new(Vec::new()) }; // alive while the menu is open
}

unsafe fn insert(m: HMENU, id: usize, label: &str, glyph: &'static str, checked: bool, sep: bool, sub: HMENU) {
    let label: Vec<u16> = label.encode_utf16().chain([0]).collect();
    let mut e = Box::new(Entry { msaa_sig: 0xAA0D_F00D, msaa_len: label.len() as u32 - 1, msaa_text: std::ptr::null(), label, glyph, checked, sep, sub: !sub.is_null() });
    e.msaa_text = e.label.as_ptr();
    let mut mi: MENUITEMINFOW = std::mem::zeroed();
    mi.cbSize = size_of::<MENUITEMINFOW>() as u32;
    mi.fMask = MIIM_FTYPE | MIIM_ID | MIIM_DATA | MIIM_STATE | if sub.is_null() { 0 } else { MIIM_SUBMENU };
    mi.fType = MFT_OWNERDRAW | if sep { MFT_SEPARATOR } else { 0 };
    mi.fState = if checked { MFS_CHECKED } else { 0 };
    mi.wID = id as u32;
    mi.hSubMenu = sub;
    mi.dwItemData = &*e as *const Entry as usize;
    InsertMenuItemW(m, GetMenuItemCount(m) as u32, 1, &mi);
    ITEMS.with_borrow_mut(|v| v.push(e));
}

pub unsafe fn item(m: HMENU, id: usize, glyph: &'static str, label: &str, checked: bool) {
    insert(m, id, label, glyph, checked, false, std::ptr::null_mut());
}

pub unsafe fn sep(m: HMENU) {
    insert(m, 0, "", "", false, true, std::ptr::null_mut());
}

/// Shows the menu at the pointer and returns the picked id (0: none). Destroys the menu.
pub unsafe fn track(h: HWND, m: HMENU) -> i32 {
    let bg = CreateSolidBrush(pal().bg);
    let mut info: MENUINFO = std::mem::zeroed();
    info.cbSize = size_of::<MENUINFO>() as u32;
    info.fMask = MIM_BACKGROUND | MIM_APPLYTOSUBMENUS;
    info.hbrBack = bg;
    SetMenuInfo(m, &info);
    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    let id = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, h, std::ptr::null());
    DestroyMenu(m);
    DeleteObject(bg as _);
    ITEMS.with_borrow_mut(|v| v.clear());
    id
}

/// The entry behind a menu message's item data, if it is one of ours (the shell's context menu draws its own).
fn ours<'a>(ctl: u32, data: usize) -> Option<&'a Entry> {
    if ctl != ODT_MENU {
        return None;
    }
    let p = ITEMS.with_borrow(|v| v.iter().map(|e| &**e as *const Entry).find(|&p| p as usize == data))?;
    Some(unsafe { &*p }) // boxed: stays put until `track` clears the list, after the menu is gone
}

/// WM_MEASUREITEM / WM_DRAWITEM for our items; false: not ours.
pub unsafe fn message(h: HWND, msg: u32, lp: LPARAM) -> bool {
    if msg == WM_MEASUREITEM {
        let mi = &mut *(lp as *mut MEASUREITEMSTRUCT);
        let Some(e) = ours(mi.CtlType, mi.itemData) else { return false };
        if e.sep {
            (mi.itemWidth, mi.itemHeight) = (0, sc(9) as u32);
            return true;
        }
        let dc = GetDC(h);
        let f = font(14, 400, ui_face()) as HFONT;
        let old = SelectObject(dc, f as _);
        let mut size: SIZE = std::mem::zeroed();
        GetTextExtentPoint32W(dc, e.label.as_ptr(), e.msaa_len as i32, &mut size);
        SelectObject(dc, old);
        DeleteObject(f as _);
        ReleaseDC(h, dc);
        (mi.itemWidth, mi.itemHeight) = ((size.cx + sc(76)) as u32, sc(34) as u32);
        return true;
    }
    let di = &*(lp as *const DRAWITEMSTRUCT);
    let Some(e) = ours(di.CtlType, di.itemData) else { return false };
    let (dc, r) = (di.hDC, di.rcItem);
    fill(dc, r, pal().bg, 0);
    if e.sep {
        let y = (r.top + r.bottom) / 2;
        fill(dc, RECT { left: r.left + sc(12), top: y, right: r.right - sc(12), bottom: y + 1 }, pal().line, 0);
        return true;
    }
    if di.itemState & ODS_SELECTED != 0 {
        fill(dc, RECT { left: r.left + sc(4), top: r.top + sc(1), right: r.right - sc(4), bottom: r.bottom - sc(1) }, pal().hover, sc(8));
    }
    SetBkMode(dc, TRANSPARENT as i32);
    let (f_txt, f_glyph) = (font(14, 400, ui_face()) as HFONT, font(15, 400, winrust_common::style::icon_face()) as HFONT);
    let old = SelectObject(dc, f_glyph as _);
    text(dc, e.glyph, RECT { left: r.left + sc(12), right: r.left + sc(36), ..r }, pal().dim, DT_CENTER);
    let right = RECT { left: r.right - sc(32), right: r.right - sc(8), ..r };
    if e.checked {
        text(dc, "\u{E73E}", right, pal().accent, DT_CENTER);
    }
    if e.sub {
        text(dc, "\u{E76C}", right, pal().dim, DT_CENTER);
    }
    SelectObject(dc, f_txt as _);
    let label = String::from_utf16_lossy(&e.label[..e.msaa_len as usize]);
    text(dc, &label, RECT { left: r.left + sc(44), right: r.right - sc(30), ..r }, pal().text, DT_LEFT);
    SelectObject(dc, old);
    DeleteObject(f_txt as _);
    DeleteObject(f_glyph as _);
    if e.sub {
        ExcludeClipRect(dc, r.left, r.top, r.right, r.bottom); // keeps the system from painting its own arrow over ours
    }
    true
}
