//! `topbar --selftest [<dir>] [bar|thumbs|tip|panels|memory|all]`: the DirectComposition path on real windows, without touching the
//! running taskbar. Everything happens on a stage in the top left corner of the screen (a plain opaque window, 1000 x 340 px,
//! topmost, never active) for a second or two. Each scene draws a frame twice, with the layered-window path (DIB, `blend`) and
//! with the composed one (windows, DirectComposition), captures the screen with `BitBlt` and compares the pixels; the click
//! regions are asked of `WindowFromPoint` on another thread (the way the mouse's hit test sees them). Prints one line per check
//! and writes the captured pictures into `<dir>`.
use super::*;
use render::{APPS, blend};
use ui::bar::{Live, Net, View};
use ui::gfx::{self, Gfx, Surface};
use winrust_common::style::{Side, Style};

const W: i32 = 1000;
const H: i32 = 340;
/// The stage's colour (BGR for GDI, the same in RGB for the reference).
const STAGE: u32 = 0x00_26_2a_20;

struct Stage {
    stage: HWND,
    out: String,
}

unsafe fn pump(ms: u32) {
    let end = GetTickCount64() + u64::from(ms);
    let mut m: MSG = zeroed();
    while GetTickCount64() < end {
        while PeekMessageW(&mut m, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    windows_sys::Win32::Graphics::Dwm::DwmFlush();
}

/// The screen's pixels of the stage, opaque.
unsafe fn capture() -> Vec<u32> {
    let mut s = Surface::new();
    s.grab(0, 0, W, H);
    s.pixels().to_vec()
}

fn reference() -> Vec<u32> {
    vec![0xff00_0000 | (STAGE & 255) << 16 | ((STAGE >> 8) & 255) << 8 | (STAGE >> 16) & 255; (W * H) as usize]
}

/// Max channel difference, pixels off by more than 2, pixels off by more than 8.
fn compare(a: &[u32], b: &[u32]) -> (u32, usize, usize) {
    let (mut max, mut n2, mut n8) = (0, 0, 0);
    for (&p, &q) in a.iter().zip(b) {
        let d = (0..3).map(|s| (((p >> (s * 8)) & 255) as i32 - ((q >> (s * 8)) & 255) as i32).unsigned_abs()).max().unwrap_or(0);
        max = max.max(d);
        n2 += usize::from(d > 2);
        n8 += usize::from(d > 8);
    }
    (max, n2, n8)
}

impl Stage {
    unsafe fn new(dir: &str) -> Self {
        let hi = GetModuleHandleW(null());
        let name = wide("topbar_selftest_stage");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: hi, hbrBackground: CreateSolidBrush(STAGE), lpszClassName: name.as_ptr(), ..zeroed() });
        let stage = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, name.as_ptr(), null(), WS_POPUP, 0, 0, W, H, null_mut(), null_mut(), hi, null());
        ShowWindow(stage, SW_SHOWNOACTIVATE);
        pump(120);
        Self { stage, out: dir.to_string() }
    }

    fn say(&self, line: &str) {
        println!("{line}");
    }

    unsafe fn png(&self, name: &str, px: &[u32]) {
        if let Ok(g) = Gfx::new(1.0) {
            let _ = g.save_png(&format!("{}\\{name}.png", self.out), W as u32, H as u32, px);
        }
    }

    /// What the mouse would find at each point: the window handle, asked on a thread of its own (a window that is transparent to
    /// the hit test is so only for other threads' input).
    unsafe fn who(&self, pts: &[(i32, i32)]) -> Vec<isize> {
        let pts = pts.to_vec();
        std::thread::spawn(move || pts.iter().map(|&(x, y)| unsafe { WindowFromPoint(POINT { x, y }) as isize }).collect()).join().unwrap_or_default()
    }

    /// Reports how a scene's picture compares to its reference.
    unsafe fn check(&self, name: &str, live: &[u32], want: &[u32]) {
        self.png(&format!("live-{name}"), live);
        self.png(&format!("ref-{name}"), want);
        let (max, n2, n8) = compare(live, want);
        self.say(&format!("{name:10} pixels: max channel difference {max:3}, {n2} pixels off by more than 2, {n8} by more than 8 (of {})", W * H));
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.stage) };
    }
}

fn apps_of(icon: &dyn Fn(&str) -> windows::Win32::UI::WindowsAndMessaging::HICON) -> Vec<barui::AppView> {
    (0..8)
        .map(|i| {
            let (_, color, wins, exe) = APPS[i % APPS.len()];
            barui::AppView { icon: icon(exe), wins, active: i == 1, flash: false, color: ui::gfx::Rgba::hex(color) }
        })
        .collect()
}

fn view(style: Style, side: Side, apps: Vec<barui::AppView>) -> View {
    View {
        style,
        side,
        size: 1,
        align_left: false,
        screen: (W as f32, H as f32),
        op: 0.85,
        apps,
        tray: Vec::new(),
        battery: Some((98, false)),
        vol: (72, false),
        net: Net::Wifi(85),
        lang: "RUS".into(),
        time: "03:57".into(),
        date: "11/26/2026".into(),
        unread: 0,
        dnd: false,
        rec: None,
    }
}

pub(crate) unsafe fn run(args: &[String]) {
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    SetProcessDPIAware();
    let dir = args.first().filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| ".".into());
    let _ = std::fs::create_dir_all(&dir);
    let which = args.iter().skip(1).find(|a| !a.starts_with("--")).map_or("all", String::as_str);
    SCALE.set(1.0);
    let st = Stage::new(&dir);
    let all = which == "all";
    if all || which == "messages" {
        messages(&st);
    }
    if which == "memory" {
        memory(&st);
    }
    if all || which == "panels" {
        panels(&st);
    }
    for style in [Style::Floating, Style::Classic] {
        if all || which == "bar" {
            bar(&st, style);
        }
    }
}

/// The bar: the same frames through a layered window's DIB and through a composed window; the click regions.
unsafe fn bar(st: &Stage, style: Style) {
    let icon = |path: &str| {
        let ic = if std::path::Path::new(path).exists() { item_icon(path, bar_icon_px()) } else { null_mut() };
        wicon(ic)
    };
    let v = view(style, Side::Bottom, apps_of(&icon));
    let frames = |bar: &mut barui::Bar| {
        let still = Live { now: 0, hot: NONE, press: NONE, ..Default::default() };
        bar.frame(&v, &still);
        let hot = Live { hot: 3, ..Live { now: 100_000, ..still } };
        bar.frame(&v, &hot);
        bar.frame(&v, &Live { now: 110_000, ..hot })
    };
    let tag = if style == Style::Floating { "bar-float" } else { "bar-flat" };

    // the reference: the layered-window path, blended over the stage like the screen does
    let mut want = reference();
    gfx::compose_off();
    let mut dib = barui::Bar::new(1.0).expect("the renderer");
    let p = frames(&mut dib);
    let (bar_s, shadow_s) = dib.surfaces();
    if let Some((x, y, w, _)) = p.shadow {
        blend(&mut want, (W, H), shadow_s.pixels(), w, (x, y), p.shadow_alpha);
    }
    blend(&mut want, (W, H), bar_s.pixels(), p.win.2, (p.win.0, p.win.1), p.alpha);

    // the composed path on real windows
    let tag = &format!("{tag}{}", if gfx::compose_windows() { "" } else { " (layered)" });
    let hi = GetModuleHandleW(null());
    let (cls, shc) = (wide("topbar_selftest_bar"), wide("topbar_selftest_shadow"));
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: hi, lpszClassName: cls.as_ptr(), ..zeroed() });
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: hi, lpszClassName: shc.as_ptr(), ..zeroed() });
    let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | gfx::window_ex();
    let bar_w = CreateWindowExW(ex, cls.as_ptr(), null(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), hi, null());
    let shadow_w = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | gfx::window_ex_through(), shc.as_ptr(), null(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), hi, null());
    gfx::through(whwnd(shadow_w));
    let mut live = barui::Bar::new(1.0).expect("the renderer");
    let p = frames(&mut live);
    live.present(whwnd(bar_w), whwnd(shadow_w));
    ShowWindow(bar_w, SW_SHOWNOACTIVATE);
    if p.shadow.is_some() {
        ShowWindow(shadow_w, SW_SHOWNOACTIVATE);
        SetWindowPos(shadow_w, bar_w, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    pump(250);
    let got = capture();
    st.check(tag, &got, &want);

    // who catches the mouse: the bar, the columns over the lifted icon, not the strip beside them, not the corners, not the shadow
    let (bx, by, bw, bh) = p.bar;
    let hot = live.screen_rect(3).expect("button 3");
    let (hx, top) = ((hot.0 + hot.2) / 2, by);
    let probes = [
        ((bx + bw / 2, by + bh - 6), bar_w as isize, "the bar's body"),
        ((hx, top - 5), bar_w as isize, "the strip over the lifted icon"),
        ((bx + 6, top - 5), 0, "the strip over a button at rest"),
        ((bx + 1, by + 1), if style == Style::Floating { 0 } else { bar_w as isize }, "the bar's corner"),
        ((bx + 150, by - 30), 0, "the shadow window above the bar"),
    ];
    let found = st.who(&probes.map(|p| p.0));
    for ((_, want_w, what), got_w) in probes.iter().zip(found) {
        let ours = got_w == bar_w as isize;
        let shadow = got_w == shadow_w as isize;
        let ok = if *want_w == 0 { !ours && !shadow } else { ours };
        st.say(&format!("{tag:10} click at {what}: {} ({})", if ok { "ok" } else { "WRONG" }, if ours { "the bar" } else if shadow { "the shadow window" } else { "a window below" }));
    }
    DestroyWindow(bar_w);
    DestroyWindow(shadow_w);
    pump(60);
}

/// The pixels the two pictures differ in, outside `skip` (rectangles: left, top, right, bottom).
fn compare_except(a: &[u32], b: &[u32], skip: &[(i32, i32, i32, i32)]) -> (u32, usize, usize) {
    let (mut max, mut n2, mut n8) = (0, 0, 0);
    for (i, (&p, &q)) in a.iter().zip(b).enumerate() {
        let (x, y) = (i as i32 % W, i as i32 / W);
        if skip.iter().any(|r| x >= r.0 && x < r.2 && y >= r.1 && y < r.3) {
            continue;
        }
        let d = (0..3).map(|s| (((p >> (s * 8)) & 255) as i32 - ((q >> (s * 8)) & 255) as i32).unsigned_abs()).max().unwrap_or(0);
        max = max.max(d);
        n2 += usize::from(d > 2);
        n8 += usize::from(d > 8);
    }
    (max, n2, n8)
}

/// One panel: the same frame through the layered-window path (`Popup` into DIBs, blended) and through the real panel host on
/// composed windows. `src` is a window whose thumbnail the panel's holes show (checked: left half red, right half blue).
unsafe fn panel_scene(st: &Stage, name: &str, anchor: ui::panel::Anchor, make: &dyn Fn() -> Box<dyn ui::panel::Content>, src: Option<isize>, moving: &[(i32, i32, i32, i32)]) {
    use ui::panel::{self, Popup};
    gfx::compose_off();
    let mut g = Gfx::new(1.0).expect("the renderer");
    let mut want = reference();
    let snap = want.clone();
    let mut grab = |x: i32, y: i32, w: i32, h: i32| Some((0..h).flat_map(|row| snap[((y + row) * W + x) as usize..((y + row) * W + x + w) as usize].iter().copied()).collect::<Vec<u32>>());
    let mut popup = Popup::new(&mut g, make(), anchor, 0, &mut grab);
    let _ = popup.frame(&mut g, 0);
    let shown = popup.frame(&mut g, 5000);
    if let Some((r, a)) = shown.scrim {
        blend(&mut want, (W, H), popup.scrim_surface().pixels(), r.2, (r.0, r.1), a);
    }
    let (panel_s, shade_s) = popup.surfaces();
    blend(&mut want, (W, H), shade_s.pixels(), shown.shadow.2, (shown.shadow.0, shown.shadow.1), shown.shadow_alpha);
    blend(&mut want, (W, H), panel_s.pixels(), shown.win.2, (shown.win.0, shown.win.1), shown.alpha);
    let holes: Vec<_> = popup.holes().iter().map(|h| h.rect).chain(moving.iter().copied()).collect();
    let (bx, by, bw, bh) = popup.rect();
    gfx::compose_windows();

    panel::open(make(), anchor);
    pump(1100);
    let got = capture();
    st.png(&format!("live-{name}"), &got);
    st.png(&format!("ref-{name}"), &want);
    let (max, n2, n8) = compare_except(&got, &want, &holes);
    st.say(&format!("{name:10} pixels: max channel difference {max:3}, {n2} pixels off by more than 2, {n8} by more than 8 (the holes of the glass and what moves by itself left out)"));

    // who catches the mouse: the box, not the room around it, not its corners, not its shadow
    let win = shown.win;
    let ours = panel::hwnd() as isize;
    let full = bw >= W && bh >= H;
    let probes = [
        ((bx + bw / 2, by + bh / 2), true, "the box"),
        ((bx - 2, by + bh / 2), full, "the room beside the box"),
        ((bx + 1, by + 1), full, "the box's corner"),
        ((bx + bw / 2, win.1 + win.3 + 6), full, "the shadow below the window"),
    ];
    let found = st.who(&probes.map(|p| p.0));
    for ((p, want_ours, what), w) in probes.iter().zip(found) {
        if p.0 < 0 || p.1 < 0 || p.0 >= W || p.1 >= H {
            st.say(&format!("{name:10} click at {what}: ({},{}) is off the stage, skipped", p.0, p.1));
            continue;
        }
        let ok = (w == ours) == *want_ours;
        st.say(&format!("{name:10} click at {what}: {} ({})", if ok { "ok" } else { "WRONG" }, if w == ours { "the panel" } else { "another window" }));
    }
    if let (Some(_), Some(h)) = (src, holes.first()) {
        let at = |x: i32| got[((h.1 + h.3) / 2 * W + x) as usize] & 0x00ff_ffff;
        let (l, r) = (at(h.0 + (h.2 - h.0) / 4), at(h.0 + (h.2 - h.0) * 3 / 4));
        st.say(&format!("{name:10} thumbnail through the hole: left {l:06x} (red ff0000) right {r:06x} (blue 0000ff): {}", if (l >> 16) >= 0xfc && l & 0xffff <= 3 && (r & 255) >= 0xfc && r >> 8 <= 3 { "ok" } else { "WRONG" }));
    }
    panel::close_now();
    pump(120);
}

unsafe fn panels(st: &Stage) {
    use ui::panel::{self, Anchor};
    gfx::compose_windows(); // without it (WINRUST_ULW=1) the same scenes run on layered windows
    let hi = GetModuleHandleW(null());
    let cls = wide("topbar_selftest_bar");
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: hi, lpszClassName: cls.as_ptr(), ..zeroed() });
    let bar_w = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, cls.as_ptr(), null(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), hi, null());
    if !panel::init(hi, 1.0, bar_w, || {}) {
        st.say("panels: the panel windows could not be made");
        return;
    }
    panel::NO_FOCUS.store(true, Relaxed);
    let anchor = Anchor { side: Side::Bottom, button: (700, H - 48, 748, H - 6), bar: (0, H - 54, W, H), screen: (W, H) };
    let icon = |path: &str| {
        let ic = if std::path::Path::new(path).exists() { item_icon(path, bar_icon_px()) } else { null_mut() };
        wicon(ic)
    };

    // lang: a small glass panel with a shadow
    let lang = || -> Box<dyn ui::panel::Content> {
        let rows = [("RUS", "Русский", "Russian · ЙЦУКЕН", true), ("ENG", "English", "United States · QWERTY", false), ("TUR", "Türkçe", "Turkish Q", false)];
        let items = rows.iter().enumerate().map(|(i, &(code, name, sub, on))| ui::lang_popup::Layout { id: i as isize, code: code.into(), name: name.into(), sub: sub.into(), on }).collect();
        Box::new(ui::lang_popup::LangPanel::new(items, |_| {}, || {}))
    };
    panel_scene(st, "lang", anchor, &lang, None, &[]);

    // preview: cards with holes, a real window's thumbnail shows through one (the window stands right of the stage: not in the picture)
    let src_c = wide("topbar_selftest_src");
    unsafe extern "system" fn src_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_PAINT {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(h, &mut ps);
            let mut r: RECT = zeroed();
            GetClientRect(h, &mut r);
            let (red, blue) = (CreateSolidBrush(0x0000_00FF), CreateSolidBrush(0x00FF_0000));
            FillRect(dc, &RECT { left: 0, top: 0, right: r.right / 2, bottom: r.bottom }, red);
            FillRect(dc, &RECT { left: r.right / 2, top: 0, right: r.right, bottom: r.bottom }, blue);
            DeleteObject(red);
            DeleteObject(blue);
            EndPaint(h, &ps);
            return 0;
        }
        DefWindowProcW(h, m, w, l)
    }
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(src_proc), hInstance: hi, lpszClassName: src_c.as_ptr(), ..zeroed() });
    let src = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, src_c.as_ptr(), null(), WS_POPUP | WS_VISIBLE, W + 20, 20, 300, 180, null_mut(), null_mut(), hi, null());
    pump(150);
    let preview = || -> Box<dyn ui::panel::Content> {
        use ui::preview::{Card, PreviewPanel};
        let cards = vec![Card { hwnd: src as isize, title: "Source window".into(), src: (300, 180), minimized: false, current: true }];
        Box::new(PreviewPanel::new(cards, render::demo_icon(&icon, 0), ui::gfx::Rgba::hex(0x3cc4f2), false, |_| {}))
    };
    panel_scene(st, "preview", Anchor { button: (500, H - 48, 548, H - 6), ..anchor }, &preview, Some(src as isize), &[]);
    DestroyWindow(src);

    // Task view: the whole screen under a scrim, one box that is the stage
    let task = || render::demo_taskview(1.0, (W, H), 3, 2, &icon);
    panel_scene(st, "taskview", anchor, &task, None, &[]);
    // the power dialog: modal, the card in the middle of a scrim
    let power = || -> Box<dyn ui::panel::Content> { Box::new(ui::power_popup::PowerPanel::new(ui::power_popup::Kind::Restart, |_| {})) };
    panel_scene(st, "power", anchor, &power, None, &[(440, 25, 610, 205)]); // the countdown ring and its seconds run by the clock
    DestroyWindow(bar_w);
}

/// 20 openings and closings of each panel on real composed windows: what the process holds (working set, private bytes, video
/// memory) must not grow after the first cycle. `topbar --selftest <dir> memory`.
unsafe fn memory(st: &Stage) {
    use ui::panel::{self, Anchor};
    let (ws0, pv0) = render::memory();
    let composed = gfx::compose_windows();
    let hi = GetModuleHandleW(null());
    let cls = wide("topbar_selftest_bar");
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(DefWindowProcW), hInstance: hi, lpszClassName: cls.as_ptr(), ..zeroed() });
    let bar_w = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, cls.as_ptr(), null(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), hi, null());
    let _bar = barui::Bar::new(1.0).expect("the renderer"); // the bar's renderer, as the real one holds it
    let up = panel::init(hi, 1.0, bar_w, || {}) && ui::tip::init(hi, 1.0, (W, H)) && ui::toast::init(hi, 1.0, (W, H));
    panel::NO_FOCUS.store(true, Relaxed);
    let gpu = || gfx::gpu_memory().map_or(0.0, |g| g.0);
    let (ws1, pv1) = render::memory();
    st.say(&format!("memory ({}): process start {ws0:.0} / {pv0:.0} MB (working set / private); with the devices, the bar's renderer and the windows of the panels, tip and toast ({up}) {ws1:.0} / {pv1:.0} MB, video {:.0} MB", if composed { "composed" } else { "layered" }, gpu()));
    let anchor = Anchor { side: Side::Bottom, button: (700, H - 48, 748, H - 6), bar: (0, H - 54, W, H), screen: (W, H) };
    let icon = |path: &str| {
        let ic = if std::path::Path::new(path).exists() { item_icon(path, bar_icon_px()) } else { null_mut() };
        wicon(ic)
    };
    type Make<'a> = &'a dyn Fn() -> Box<dyn ui::panel::Content>;
    let lang: Make = &|| {
        let rows = [("RUS", "Русский", "Russian", true), ("ENG", "English", "United States", false)];
        let items = rows.iter().enumerate().map(|(i, &(code, name, sub, on))| ui::lang_popup::Layout { id: i as isize, code: code.into(), name: name.into(), sub: sub.into(), on }).collect();
        Box::new(ui::lang_popup::LangPanel::new(items, |_| {}, || {}))
    };
    let task: Make = &|| render::demo_taskview(1.0, (W, H), 3, 2, &icon);
    let power: Make = &|| Box::new(ui::power_popup::PowerPanel::new(ui::power_popup::Kind::Restart, |_| {}));
    let search: Make = &|| Box::new(ui::search_panel::SearchPanel::with_query(900.0, "", Vec::new(), Vec::new()));
    for (name, make) in [("lang", lang), ("taskview", task), ("power", power), ("search", search)] {
        let mut first = (0.0, 0.0, 0.0);
        for cycle in 1..=23 {
            panel::open(make(), anchor);
            pump(260);
            panel::close_now();
            pump(40);
            if cycle == 3 {
                // the first cycles warm things up (fonts, effect caches, the first surfaces); what must not grow is what follows
                let (ws, pv) = render::memory();
                first = (ws, pv, gpu());
            }
            if std::env::var_os("SELFTEST_SERIES").is_some() && [1, 2, 3, 5, 10, 15, 20].contains(&cycle) {
                let (ws, pv) = render::memory();
                st.say(&format!("  {name} cycle {cycle}: {ws:.1} / {pv:.1} MB, video {:.1}", gpu()));
            }
        }
        let (ws, pv) = render::memory();
        let (dws, dpv, dgpu) = (ws - first.0, pv - first.1, gpu() - first.2);
        let ok = dpv.abs() <= 2.0 && dgpu.abs() <= 1.0;
        st.say(&format!(
            "memory {name:9} after cycle 3: {:.1} / {:.1} MB, video {:.1} MB; after 23: {ws:.1} / {pv:.1} MB, video {:.1} MB; growth {dws:+.1} / {dpv:+.1} MB, video {dgpu:+.1} MB: {}",
            first.0,
            first.1,
            first.2,
            gpu(),
            if ok { "ok" } else { "GROWS" }
        ));
    }
    DestroyWindow(bar_w);
}

static PAINTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe extern "system" fn count_paint(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == WM_PAINT {
        PAINTS.fetch_add(1, Relaxed);
        ValidateRect(h, null());
        return 0;
    }
    if m == crate::pacing::FRAME {
        if crate::pacing::received(h as isize) {
            FRAMES.fetch_add(1, Relaxed);
        }
        return 0;
    }
    DefWindowProcW(h, m, w, l)
}

/// The bar redraws on `InvalidateRect` (WM_PAINT): does a window without a redirection bitmap still get it, and the frame
/// messages of the pacing thread?
unsafe fn messages(st: &Stage) {
    gfx::compose_windows();
    let hi = GetModuleHandleW(null());
    let cls = wide("topbar_selftest_paint");
    RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(count_paint), hInstance: hi, lpszClassName: cls.as_ptr(), ..zeroed() });
    let w = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | gfx::window_ex(), cls.as_ptr(), null(), WS_POPUP, 0, 0, 200, 40, null_mut(), null_mut(), hi, null());
    ShowWindow(w, SW_SHOWNOACTIVATE);
    pump(100);
    let before = PAINTS.load(Relaxed);
    InvalidateRect(w, null(), 0);
    pump(100);
    let painted = PAINTS.load(Relaxed) - before;
    crate::pacing::set(w as isize, true);
    let frames = FRAMES.load(Relaxed);
    pump(500);
    let frames = FRAMES.load(Relaxed) - frames;
    crate::pacing::set(w as isize, false);
    st.say(&format!("messages   frames the ticker asked for in 500 ms: {frames} ({})", if gfx::composed_anywhere() { "on every tick of the screen" } else { "60 Hz: the layered windows" }));
    st.say(&format!("messages   WM_PAINT after InvalidateRect on a window without a redirection bitmap: {} ({painted})", if painted > 0 { "ok" } else { "WRONG" }));
    DestroyWindow(w);
}