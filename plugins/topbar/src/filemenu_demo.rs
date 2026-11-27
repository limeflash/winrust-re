//! The states of the mockup's "File context menu" board as demo data: what the shell would have given for each target, with the
//! third-party items and their small bitmaps the mockup draws (7-Zip, Microsoft Defender, Cursor, DashBeam, Git, NVIDIA). For
//! `topbar --render-panel <png> filemenu state=<name>` and the tests; the machine's own shell is never asked.
use winrust_common::filemenu::{Icon, ShellItem, Sub, fl, rq};

use crate::filemenu::{self as model, Facts, HeadIcon, Target};

/// A 16 px bitmap like a program gives its menu item: a rounded square in its colour with a lighter mark.
#[must_use]
pub fn bitmap(body: u32, mark: u32) -> Icon {
    let mut px = vec![0u32; 256];
    let inside = |x: i32, y: i32| {
        let (dx, dy) = ((x.min(15 - x) - 3).min(0), (y.min(15 - y) - 3).min(0));
        dx * dx + dy * dy <= 9
    };
    for y in 0..16 {
        for x in 0..16 {
            if inside(x, y) {
                px[(y * 16 + x) as usize] = if (5..=10).contains(&x) && (5..=10).contains(&y) { 0xff00_0000 | mark } else { 0xff00_0000 | body };
            }
        }
    }
    Icon { w: 16, h: 16, px }
}

fn with(mut i: ShellItem, icon: Option<Icon>) -> ShellItem {
    i.icon = icon;
    i
}

fn it(id: u32, verb: &str, label: &str) -> ShellItem {
    ShellItem::new(id, verb, label)
}

fn sub(label: &str, kids: Vec<ShellItem>) -> ShellItem {
    ShellItem { sub: Sub::Items(kids), ..it(0, "", label) }
}

fn cursor() -> Icon {
    bitmap(0x0f0f10, 0xe8e8e8)
}
fn dash() -> Icon {
    bitmap(0x0d2b1d, 0x35e08a)
}
fn zip7() -> Icon {
    bitmap(0x111111, 0xffffff)
}
fn defender() -> Icon {
    bitmap(0x1f6fd6, 0xffffff)
}
fn git() -> Icon {
    bitmap(0xf05133, 0xffffff)
}
fn nvidia() -> Icon {
    bitmap(0x76b900, 0x0b0b0b)
}

/// The submenus Windows fills when they are opened, as they stand once filled.
fn open_with(apps: &[(&str, Icon)]) -> ShellItem {
    let mut kids: Vec<ShellItem> = apps.iter().enumerate().map(|(i, (n, ic))| with(it(100 + i as u32, "", n), Some(ic.clone()))).collect();
    kids.push(ShellItem::sep());
    kids.push(it(98, "openas", "Choose another app…"));
    kids.push(with(it(99, "", "Search the Microsoft Store"), Some(bitmap(0x2b7cd3, 0xffffff))));
    sub("Open with", kids)
}

fn send_to() -> ShellItem {
    let ic = |c: u32| Some(bitmap(c, 0xffffff));
    sub(
        "Send to",
        vec![with(it(30, "", "Bluetooth device"), ic(0x2a6fdb)), with(it(31, "", "Desktop (create shortcut)"), ic(0x4b87c9)), with(it(32, "", "Documents"), ic(0xc9a43a)), with(it(33, "", "Mail recipient"), ic(0x3f78c4)), with(it(34, "", "USB Drive (E:)"), ic(0x6a7080))],
    )
}

fn seven_zip(base: &str) -> ShellItem {
    let z = |id: u32, l: &str| with(it(id, "", l), Some(zip7()));
    sub(
        "7-Zip",
        vec![z(110, "Open archive"), z(111, "Extract files…"), z(112, "Extract Here"), z(113, "Test archive"), ShellItem::sep(), z(114, "Add to archive…"), z(115, &format!("Add to “{base}.7z”")), z(116, &format!("Add to “{base}.zip”")), z(117, "Compress and email…")],
    )
}

fn tail(first_open: ShellItem, base: &str, extra: Vec<ShellItem>, with_open_with: bool) -> Vec<ShellItem> {
    let mut v = vec![first_open];
    v.extend(extra);
    v.extend([
        it(40, "pintohomefile", "Add to Favorites"),
        with(it(41, "Send with DashBeam", "Send with DashBeam"), Some(dash())),
        with(seven_zip(base), Some(zip7())),
        with(it(42, "", "Scan with Microsoft Defender…"), Some(defender())),
    ]);
    if with_open_with {
        v.push(open_with(&[("Notepad", bitmap(0x4a90d9, 0xffffff)), ("Visual Studio Code", bitmap(0x2c8be6, 0xffffff)), ("Cursor", cursor()), ("Microsoft Edge", bitmap(0x2fa3d8, 0xffffff))]));
    }
    v.extend([
        ShellItem::sep(),
        sub("Give access to", vec![it(50, "", "Remove access"), it(51, "", "Specific people…")]),
        it(60, "copyaspath", "Copy as path"),
        it(61, "Windows.ModernShare", "Share"),
        it(62, "PreviousVersions", "Restore previous versions"),
        ShellItem::sep(),
        send_to(),
        ShellItem::sep(),
        it(25, "cut", "Cut"),
        it(26, "copy", "Copy"),
        ShellItem::sep(),
        it(17, "link", "Create shortcut"),
        it(18, "delete", "Delete"),
    ]);
    v.extend([ShellItem::sep(), it(20, "properties", "Properties")]);
    v
}

fn open_default(label: &str) -> ShellItem {
    ShellItem { flags: fl::DEFAULT, ..it(1, "open", label) }
}

/// One state of the board.
pub struct Demo {
    pub facts: Facts,
    pub items: Vec<ShellItem>,
    pub head: HeadIcon,
    /// Where the pointer was, DIPs of the mockup's 1440 x 900 screen.
    pub at: (f32, f32),
    pub more: bool,
    /// The row whose submenu is open, and the row that is lit.
    pub sub: Option<usize>,
    pub foc: Option<usize>,
}

fn facts(target: Target, name: &str, meta: &str, stem: &str, flags: u32) -> Facts {
    Facts { target, name: name.into(), meta: meta.into(), stem: stem.into(), start_pinned: false, bar_pinned: false, bin_items: 0, extract: false, flags }
}

/// The state named `name` (`text`, `app`, `image`, `folder`, `several`, `openwith`, `more`, `corner`, `bin`, `archive`, `drive`).
#[must_use]
pub fn demo(name: &str) -> Option<Demo> {
    let desk = rq::DESKTOP | rq::CAN_RENAME;
    let plate = |d: &'static str, a: u32, b: u32, ink: u32| HeadIcon::Plate(d, a, b, ink);
    let text = |more: bool, sub: Option<usize>| {
        let items = tail(open_default("Open"), "New text document", vec![it(33, "edit", "Edit"), with(it(34, "Cursor", "Open with Cursor"), Some(cursor()))], true);
        Demo { facts: facts(Target::Text, "New text document.txt", "Text document · 2 KB", "New text document", desk), items, head: plate(model::DOC, 0xeaf2ff, 0xeaf2ff, 0x2557c4), at: (96.0, 600.0), more, sub, foc: sub }
    };
    Some(match name {
        "text" => text(false, None),
        "openwith" => text(false, Some(2)),
        "more" => Demo { at: (96.0, 470.0), ..text(true, None) },
        "app" => {
            let items = tail(open_default("Open"), "Cursor", vec![it(33, "runas", "Run as administrator"), it(34, "", "Troubleshoot compatibility"), it(35, "opencontaining", "Open file location")], true);
            Demo { facts: facts(Target::App, "Cursor", "Shortcut · app", "Cursor", desk), items, head: plate("M5 3l14 9-14 9z", 0x0f0f10, 0x0f0f10, 0xe8e8e8), at: (150.0, 170.0), more: false, sub: None, foc: None }
        }
        "image" => {
            let extra = vec![it(33, "setdesktopwallpaper", "Set as desktop background"), it(34, "rotate90", "Rotate right"), it(35, "rotate270", "Rotate left"), with(it(36, "", "Edit with NVIDIA Canvas"), Some(nvidia()))];
            let items = tail(open_default("Open"), "mountains", extra, true);
            Demo { facts: facts(Target::Image, "mountains.jpg", "JPG image · 3.6 MB", "mountains", desk), items, head: plate(model::IMG, 0x3a6ea5, 0x6aa0d8, 0xffffff), at: (150.0, 275.0), more: false, sub: None, foc: None }
        }
        "corner" => {
            let extra = vec![it(33, "setdesktopwallpaper", "Set as desktop background"), it(34, "rotate90", "Rotate right"), it(35, "rotate270", "Rotate left")];
            let items = tail(open_default("Open"), "Screenshot 2026-11-27 093112", extra, true);
            Demo { facts: facts(Target::Image, "Screenshot 2026-11-27 093112.png", "PNG image · 1.9 MB", "Screenshot 2026-11-27 093112", desk), items, head: plate(model::IMG, 0x3a6ea5, 0x6aa0d8, 0xffffff), at: (1392.0, 812.0), more: false, sub: None, foc: None }
        }
        "folder" => {
            let mut items = tail(open_default("Open"), "Projects", vec![with(it(33, "git_gui", "Open Git GUI here"), Some(git())), with(it(34, "git_shell", "Open Git Bash here"), Some(git())), with(it(35, "Cursor", "Open with Cursor"), Some(cursor()))], false);
            items.retain(|i| i.verb != "Windows.ModernShare");
            Demo { facts: facts(Target::Folder, "Projects", "Folder · 4 items", "Projects", rq::TAB | rq::CAN_RENAME), items, head: plate(model::FOLDER_OPEN, 0xf6c453, 0xd49a2a, 0x4a3200), at: (930.0, 300.0), more: false, sub: None, foc: None }
        }
        "several" => {
            let items = tail(open_default("Open"), "New text document", vec![], false);
            let mut f = facts(Target::Several, "3 items", "12.4 MB · New text document.txt, rtaskmgr-Processes.txt…", "", rq::DESKTOP);
            f.name = model::items_text(3);
            Demo { facts: f, items, head: plate(model::COPY, 0x2b2c33, 0x2b2c33, 0xa3a9b8), at: (96.0, 600.0), more: false, sub: None, foc: None }
        }
        "archive" => {
            let items = tail(open_default("Open"), "logs-1771793152", vec![it(33, "", "Extract All…")], true);
            let mut f = facts(Target::Archive, "logs-1771793152.zip", "Compressed folder · 4.8 MB", "logs-1771793152", desk);
            f.extract = true;
            Demo { facts: f, items, head: plate("M12 3v2M12 7v2M12 11v2M10 15h4v4h-4z", 0xf6c453, 0xd49a2a, 0x5a3d00), at: (96.0, 170.0), more: false, sub: None, foc: None }
        }
        "bin" => {
            let items = vec![open_default("Open"), it(2, "empty", "Empty Recycle Bin"), ShellItem::sep(), with(it(3, "pintohome", "Pin to Quick access"), None), it(4, "properties", "Properties")];
            let mut f = facts(Target::Bin, "Recycle Bin", "14 items · 860 MB", "", rq::DESKTOP);
            f.bin_items = 14;
            Demo { facts: f, items, head: plate(model::BIN, 0x2b2c33, 0x2b2c33, 0xa3a9b8), at: (160.0, 70.0), more: false, sub: None, foc: None }
        }
        "drive" => {
            let items = vec![open_default("Open"), it(2, "properties", "Properties")];
            Demo { facts: facts(Target::Drive, "USB Drive (E:)", "17.7 GB free of 58.2 GB", "", rq::TAB), items, head: plate(model::DRIVE, 0x2b2c33, 0x2b2c33, 0xa3a9b8), at: (700.0, 470.0), more: false, sub: None, foc: None }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filemenu::{Do, Kind, Row, Under, build};
    use winrust_common::filemenu::Request;

    const STATES: [&str; 11] = ["text", "openwith", "more", "app", "image", "corner", "folder", "several", "archive", "bin", "drive"];

    fn ids(items: &[ShellItem], out: &mut Vec<u32>) {
        for i in items {
            out.push(i.id);
            if let Sub::Items(v) = &i.sub {
                ids(v, out);
            }
        }
    }

    fn acts(rows: &[Row]) -> Vec<(String, Do)> {
        rows.iter().filter(|r| r.is_item()).map(|r| (r.label.clone(), r.act.clone())).collect()
    }

    #[test]
    fn every_state_of_the_board_builds_and_points_only_at_items_the_shell_gave() {
        for name in STATES {
            let d = demo(name).unwrap_or_else(|| panic!("{name}"));
            let mut have = vec![];
            ids(&d.items, &mut have);
            let m = build(&d.facts, &d.items, d.head);
            assert!(!m.head.name.is_empty() && !m.rows.is_empty(), "{name}");
            // no rule at either end or twice in a row, in the rows and in the whole list
            for list in [&m.rows, &m.full] {
                assert!(list.first().is_some_and(|r| r.is_item() || r.act == Do::Less), "{name}: starts with a rule");
                assert!(list.last().is_some_and(Row::is_item), "{name}: ends with a rule");
                assert!(list.windows(2).all(|w| w[0].is_item() || w[1].kind != Kind::Sep || w[0].kind == Kind::Caption), "{name}: two rules");
            }
            fn walk(rows: &[Row], have: &[u32], name: &str) {
                for r in rows {
                    if let Do::Shell(id) = r.act {
                        assert!(have.contains(&id), "{name}: {} runs item {id}, which the shell did not give", r.label);
                    }
                    if let Some(Under::Items(v)) = &r.sub {
                        walk(v, have, name);
                    }
                }
            }
            walk(&m.rows, &have, name);
            walk(&m.full, &have, name);
            for q in &m.quick {
                if let Do::Shell(id) = q.act {
                    assert!(have.contains(&id), "{name}: tile {}", q.tip);
                }
            }
        }
    }

    #[test]
    fn the_boards_rows_by_kind_are_the_designs() {
        let rows = |n: &str| {
            let d = demo(n).unwrap();
            let m = build(&d.facts, &d.items, d.head);
            (m.quick.len(), m.rows.iter().map(|r| if r.is_item() { r.act.clone() } else { Do::Nothing }).collect::<Vec<_>>(), m.rows.len())
        };
        // tiles: Cut Copy Rename Share Delete; a folder has no Share; several items have no Rename; a bin has none
        assert_eq!(rows("text").0, 5);
        assert_eq!(rows("folder").0, 4);
        assert_eq!(rows("several").0, 4);
        assert_eq!(rows("bin").0, 0);
        // text: Open Edit OpenWith - CopyPath Zip SendTo - More Properties
        assert_eq!(rows("text").2, 10);
        // an app: Run, Run as administrator, Open file location - Pin Start, Pin bar - Copy as path - More, Properties
        assert_eq!(rows("app").2, 11);
        // an image adds the wallpaper and the two turns
        assert_eq!(rows("image").2, 12);
        // the Recycle Bin: Open, Empty - Properties
        assert_eq!(rows("bin").2, 4);
        assert_eq!(rows("drive").2, 4, "Open, Open in new tab, rule, Properties");
        assert!(demo("nothing").is_none());
    }

    #[test]
    fn a_request_through_the_wire_gives_the_same_menu_as_the_items_directly() {
        for name in STATES {
            let d = demo(name).unwrap();
            let direct = build(&d.facts, &d.items, HeadIcon::Plate(model::DOC, 0, 0, 0));
            let req = Request { session: 9, at: (10, 20), flags: d.facts.flags, paths: vec![r"C:\Users\Me\Desktop\x".into()], items: d.items.clone() };
            let back = Request::decode(&req.encode()).unwrap_or_else(|| panic!("{name}: decode"));
            let via = build(&d.facts, &back.items, HeadIcon::Plate(model::DOC, 0, 0, 0));
            assert_eq!(acts(&direct.rows), acts(&via.rows), "{name}");
            assert_eq!(acts(&direct.full), acts(&via.full), "{name}");
            let bitmaps = |rows: &[Row]| rows.iter().filter(|r| matches!(r.pic, crate::filemenu::Pic::Bitmap(_))).count();
            assert_eq!(bitmaps(&direct.full), bitmaps(&via.full), "{name}: the programs' bitmaps made the trip");
        }
    }

    #[test]
    fn the_third_party_items_of_the_text_file_keep_their_bitmaps_in_the_whole_list() {
        let d = demo("text").unwrap();
        let m = build(&d.facts, &d.items, d.head);
        for name in ["Open with Cursor", "Send with DashBeam", "7-Zip", "Scan with Microsoft Defender…"] {
            let r = m.full.iter().find(|r| r.label == name).unwrap_or_else(|| panic!("{name}"));
            assert!(matches!(r.pic, crate::filemenu::Pic::Bitmap(_)), "{name} has its own icon");
        }
        // Open with and Send to are the design's own rows in the main list, with the shell's submenus behind them
        let Some(Under::Items(apps)) = m.rows.iter().find(|r| r.act == Do::Nothing && r.sub.is_some()).and_then(|r| r.sub.clone()) else { panic!("a submenu") };
        assert!(apps.iter().any(|r| r.label == "Notepad" && matches!(r.pic, crate::filemenu::Pic::Bitmap(_))));
        assert!(apps.iter().any(|r| r.label == "Choose another app…"), "{:?}", apps.iter().map(|r| &r.label).collect::<Vec<_>>());
    }
}
