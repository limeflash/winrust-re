//! topbar: an app's own list of recent files, the one Windows keeps for its taskbar jump list (AutomaticDestinations).
//! (split out of main.rs; shares its items through `super::*`)
//!
//! `IApplicationDocumentLists` returns it for an AppUserModelID. An app that never set one has only an implicit id the shell
//! derives from its path; for those the exe path is tried too, and an app Windows keeps no list for simply shows no block.
use super::*;
use windows::Win32::UI::Shell::Common::IObjectArray;
use windows::Win32::UI::Shell::{ADLT_RECENT, ApplicationDocumentLists, IApplicationDocumentLists, IShellItem, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY};

pub(crate) struct Doc {
    pub name: String,
    pub path: String,
    pub folder: bool,
}

/// The recent files of the app `id` (an AppUserModelID or an exe path), the newest first, at most `max`.
pub(crate) unsafe fn recent(id: &str, max: u32) -> Vec<Doc> {
    if id.is_empty() {
        return Vec::new();
    }
    (|| -> Option<Vec<Doc>> {
        let lists: IApplicationDocumentLists = CoCreateInstance(&ApplicationDocumentLists, None, CLSCTX_INPROC_SERVER).ok()?;
        lists.SetAppID(PCWSTR(wide(id).as_ptr())).ok()?;
        let arr: IObjectArray = lists.GetList(ADLT_RECENT, max).ok()?;
        let mut out = Vec::new();
        for i in 0..arr.GetCount().ok()? {
            if let Ok(item) = arr.GetAt::<IShellItem>(i) {
                let text = |kind| item.GetDisplayName(kind).ok().and_then(|p| {
                    let s = p.to_string().ok();
                    windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
                    s
                });
                let path = text(SIGDN_FILESYSPATH).unwrap_or_default();
                let name = text(SIGDN_NORMALDISPLAY).filter(|n| !n.is_empty()).or_else(|| std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into()));
                if let (Some(name), false) = (name, path.is_empty()) {
                    let folder = std::path::Path::new(&path).is_dir();
                    out.push(Doc { name, path, folder });
                }
            }
        }
        Some(out)
    })()
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn explorer_has_a_list_and_a_junk_id_has_none() {
        unsafe {
            let _ = super::CoInitializeEx(None, super::COINIT_APARTMENTTHREADED);
            let junk = super::recent("no.such.app.id", 5);
            assert!(junk.is_empty());
            // the id is not case sensitive (the bar hands over the lower-cased AppUserModelID of a window)
            let names = |id| super::recent(id, 5).into_iter().map(|d| d.name).collect::<Vec<_>>();
            assert_eq!(names("Microsoft.Windows.Explorer"), names("microsoft.windows.explorer"));
        }
    }
}
