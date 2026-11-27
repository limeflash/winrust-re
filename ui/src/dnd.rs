//! Files dragged onto an application window: an OLE drop target that tells the app what is over it ([`App::drag_over`], from the first
//! pixel of the drag to the drop) and what was dropped ([`App::drop`]). The list of files is read from the drag once, when it
//! enters; a drag that carries no files is refused (the cursor shows the "no" sign).

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, POINT, POINTL};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, OleInitialize, ReleaseStgMedium, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::core::{Ref, Result, implement};

use crate::anim;
use crate::core::DropFx;
use crate::window::{event, with};

/// The paths a drag carries (`CF_HDROP`); empty when it carries none.
fn hdrop_paths(o: &IDataObject) -> Vec<String> {
    let fmt = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    // SAFETY: the global memory of the medium is read while it is held, and released once.
    unsafe {
        let Ok(mut med) = o.GetData(&fmt) else { return Vec::new() };
        let hd = HDROP(med.u.hGlobal.0);
        let n = DragQueryFileW(hd, u32::MAX, None);
        let out = (0..n)
            .map(|i| {
                let mut buf = vec![0u16; DragQueryFileW(hd, i, None) as usize + 1];
                let len = DragQueryFileW(hd, i, Some(&mut buf)) as usize;
                String::from_utf16_lossy(&buf[..len])
            })
            .collect();
        ReleaseStgMedium(&mut med);
        out
    }
}

#[implement(IDropTarget)]
struct Target {
    hwnd: isize,
    paths: RefCell<Vec<String>>,
}

impl Target {
    /// The point of the screen (`POINTL`) in DIPs of the window's client area.
    fn client(&self, pt: &POINTL) -> (f32, f32) {
        let mut p = POINT { x: pt.x, y: pt.y };
        // SAFETY: a conversion of a local point for our own window.
        unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(HWND(self.hwnd as *mut _), &mut p).ok().ok() };
        let s = with(self.hwnd as _, |h| h.core.scale).unwrap_or(1.0);
        (p.x as f32 / s, p.y as f32 / s)
    }

    fn over(&self, k: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) {
        let (x, y) = self.client(pt);
        let paths = self.paths.borrow();
        let mut ok = false;
        let mut fx = DropFx::Copy;
        if !paths.is_empty() {
            event(self.hwnd as _, |h| {
                ok = h.core.drag_over(&paths, x, y);
                fx = h.core.app.drag_effect(&paths, k.0 & 8 != 0); // MK_CONTROL
                None
            });
        }
        // SAFETY: OLE gives a valid pointer to the effect.
        unsafe { *effect = if !ok { DROPEFFECT_NONE } else if fx == DropFx::Move { DROPEFFECT_MOVE } else { DROPEFFECT_COPY } };
    }
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(&self, o: Ref<'_, IDataObject>, k: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        *self.paths.borrow_mut() = o.as_ref().map(hdrop_paths).unwrap_or_default();
        self.over(k, pt, effect);
        Ok(())
    }

    fn DragOver(&self, k: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        self.over(k, pt, effect);
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.paths.borrow_mut().clear();
        event(self.hwnd as _, |h| {
            h.core.drag_leave(anim::now_ms());
            None
        });
        Ok(())
    }

    fn Drop(&self, o: Ref<'_, IDataObject>, k: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let paths = o.as_ref().map(hdrop_paths).unwrap_or_default();
        self.paths.borrow_mut().clear();
        let (x, y) = self.client(pt);
        let got = !paths.is_empty();
        let mut fx = DropFx::Copy;
        // the app is told after OLE has the answer: it may show a dialog, which must not hold the drag source
        event(self.hwnd as _, |h| {
            h.core.drag_leave(anim::now_ms());
            if got {
                fx = h.core.app.drag_effect(&paths, k.0 & 8 != 0);
                h.core.files(paths, x, y, anim::now_ms());
            }
            None
        });
        // SAFETY: OLE gives a valid pointer to the effect.
        unsafe { *effect = if !got { DROPEFFECT_NONE } else if fx == DropFx::Move { DROPEFFECT_MOVE } else { DROPEFFECT_COPY } };
        Ok(())
    }
}

/// Makes `hwnd` take dropped files. OLE is started for the thread (it keeps the apartment the thread has).
pub fn accept(hwnd: isize) {
    // SAFETY: OLE start-up and the registration of a window of ours.
    unsafe {
        let _ = OleInitialize(None);
        let t: IDropTarget = Target { hwnd, paths: RefCell::new(Vec::new()) }.into();
        let _ = RegisterDragDrop(HWND(hwnd as *mut _), &t);
    }
}

/// The window no longer takes drops (called when it goes).
pub fn refuse(hwnd: isize) {
    // SAFETY: a window of ours.
    unsafe {
        let _ = RevokeDragDrop(HWND(hwnd as *mut _));
    }
}
