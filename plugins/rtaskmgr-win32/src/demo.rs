//! The Task Manager in the states of the design (`--render state=<name>`, `--bench`, `--selftest`, the tests): the processes, services,
//! startup entries, users and charts of TaskManager.html made up into a [`Data`], and a [`Fake`] machine that reads and changes nothing
//! of this computer. Every action the window can take lands in the fake's log instead of on a process, a service or a registry key.
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use winrust_ui::core::App;

use crate::{
    app::{ID_ACT, SWITCH_COL, Tm},
    backend::{Machine, TrayItem},
    boost,
    data::{CAP, Data, Proc},
    draw::{AppM, MemView, UDot, UserM},
    look::{self, Tile},
    model::{Page, Row},
    net,
    perfview::{GLayout, GraphSpec, PerfItem, PerfKind, Rgb},
    res::Res,
    startup, store, suspect,
    sys::Svc,
};

/// What `--render-all` goes through: the states of the design's "Preview states" list, and every page.
pub const STATES: [&str; 32] = [
    "processes", "performance", "users", "speed", "settings", "alert", "no-alert", "process-menu", "more-menu", "end-tree", "no-results", "history-cleared", "no-rules",
    "history", "startup", "details", "services", "rules", "slow", "slow-app", "slow-drv", "slow-ok", "slow-checking", "perf-memory", "perf-disk", "perf-network", "perf-gpu", "processes-expanded", "users-collapsed", "speed-open",
    "toast", "tray-question",
];

/// What a state of the design needs by hand after the window is up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hook {
    None,
    /// The context menu of the picked row, where the design puts it.
    ProcessMenu,
    /// "End process tree" of the picked row, asked.
    EndTree,
    /// The "More" button of the head, pressed.
    MoreMenu,
    /// The window's close, asked of a program that has never been asked what to do with it.
    CloseAsk,
}

/// The extras the demo has that the real window gets from the machine.
#[derive(Clone)]
pub struct Demo {
    pub mem: MemView,
    pub users: Vec<UserM>,
    pub processes: usize,
    pub threads: u32,
    pub svc_total: usize,
    /// The tiles of the Users page: (value, sub, share).
    pub tiles: Vec<(String, String, f32)>,
    pub hook: Hook,
    /// The Details page lists every process, not only the first of each app (the benchmark's big table).
    pub details_all: bool,
}

impl Demo {
    /// What the real window gets from the registry and the event log, as the design shows it.
    pub fn post_rows(&self, page: Page, rows: &mut Vec<Row>) {
        if page == Page::Details && !self.details_all {
            let lead = LEAD_PIDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            rows.retain(|r| r.pids.first().is_some_and(|p| lead.contains(p)));
        }
        if page == Page::Startup {
            for r in rows.iter_mut() {
                let cost = match r.cells.first().map(String::as_str) {
                    Some("WinRust") => "0.3 s",
                    Some("Discord") => "2.1 s",
                    Some("Spotify") => "1.4 s",
                    Some("Telegram Desktop") => "0.6 s",
                    Some("NVIDIA Broadcast") => "0.9 s",
                    Some("Steam Client Bootstrapper") => "3.2 s",
                    Some("Claude") => "91.9 s",
                    Some("Cardinal") => "86.2 s",
                    Some("BotForge Panel") => "50.2 s",
                    _ => "1.8 s",
                };
                r.cells[3] = format!("+{cost}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the made-up apps

struct P {
    id: &'static str,
    exe: &'static str,
    desc: &'static str,
    company: &'static str,
    n: usize,
    cpu: f32,
    mem_mb: f32,
    disk: f32,
    gpu: f32,
    tile: Tile,
    kids: &'static [&'static str],
}

const T_CLAUDE: Tile = look::tile(0xd9835f, 0x9c4f33, "M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6L5.6 18.4");
const T_EDGE: Tile = look::tile(0x35b8f0, 0x1c8f6a, "M4 14a8 8 0 1 1 16 0c0 3-3 4-6 4s-4-2-4-4h10");
const T_SPOTIFY: Tile = look::tile(0x38c76a, 0x147a3c, "M5 9c5-2 10-1 14 1M6 13c4-1 8-1 11 1M7 17c3-1 6-1 8 0");
const T_DISCORD: Tile = look::tile(0x7a85f5, 0x4a52c4, "M6 8c3-2 9-2 12 0l1 8c-3 2-11 2-14 0zM9 12h.01M15 12h.01");
const T_TELEGRAM: Tile = look::tile(0x4cc3f2, 0x1f8ad0, "M20 4L3 11l6 2 2 6 3-4 5 3z");
const T_EXPLORER: Tile = look::tile(0xf2b84c, 0xc9852a, "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z");
const T_OUTLOOK: Tile = look::tile(0x3d8fe8, 0x1f56b0, "M4 6h16v12H4zM4 7l8 6 8-6");
const T_TASKMGR: Tile = look::tile(0x62a0ea, 0x2f5fc8, "M3 12h4l2.5-6 4 12 2.5-6H21");

fn apps() -> Vec<P> {
    vec![
        P { id: "claude", exe: "claude.exe", desc: "Claude", company: "Anthropic", n: 18, cpu: 0.4, mem_mb: 4003.0, disk: 0.0, gpu: 0.0, tile: T_CLAUDE, kids: &["Claude", "Claude Helper (GPU)", "Claude Helper (Renderer)", "Claude Helper (Renderer)", "Claude Helper (Utility)"] },
        P { id: "edge", exe: "msedge.exe", desc: "Microsoft Edge", company: "Microsoft Corporation", n: 11, cpu: 0.2, mem_mb: 1486.0, disk: 0.0, gpu: 0.0, tile: T_EDGE, kids: &["Microsoft Edge", "GPU Process", "Tab: Lemxy/WinRust · GitHub", "Tab: heydamir/unslop · GitHub", "Utility: Network Service"] },
        P { id: "spotify", exe: "Spotify.exe", desc: "Spotify", company: "Spotify AB", n: 7, cpu: 0.1, mem_mb: 1029.0, disk: 0.1, gpu: 0.0, tile: T_SPOTIFY, kids: &["Spotify", "Spotify (GPU)", "Spotify (Renderer)"] },
        P { id: "discord", exe: "Discord.exe", desc: "Discord", company: "Discord Inc.", n: 6, cpu: 0.1, mem_mb: 932.0, disk: 0.0, gpu: 1.0, tile: T_DISCORD, kids: &["Discord", "Discord (GPU)", "Discord (Renderer)", "Discord (Voice)"] },
        P { id: "svchost", exe: "svchost.exe", desc: "Service Host", company: "Microsoft Corporation", n: 40, cpu: 0.1, mem_mb: 879.0, disk: 0.0, gpu: 0.0, tile: look::T_SYS, kids: &[] },
        P { id: "rustc", exe: "rustc.exe", desc: "rustc", company: "The Rust Project", n: 3, cpu: 12.2, mem_mb: 503.0, disk: 0.0, gpu: 0.0, tile: look::T_RUST, kids: &["rustc · winrust-topbar", "rustc · common", "rustc · build script"] },
        P { id: "telegram", exe: "Telegram.exe", desc: "Telegram Desktop", company: "Telegram FZ-LLC", n: 1, cpu: 0.0, mem_mb: 437.0, disk: 0.0, gpu: 0.0, tile: T_TELEGRAM, kids: &[] },
        P { id: "explorer", exe: "rust-explorer.exe", desc: "WinRust Explorer", company: "WinRust contributors", n: 2, cpu: 0.0, mem_mb: 418.0, disk: 0.0, gpu: 0.0, tile: T_EXPLORER, kids: &["WinRust Explorer — Local Disk (C:)", "WinRust Explorer — Downloads"] },
        P { id: "powershell", exe: "powershell.exe", desc: "Windows PowerShell", company: "Microsoft Corporation", n: 5, cpu: 0.0, mem_mb: 379.0, disk: 0.0, gpu: 0.0, tile: look::T_PS, kids: &["Windows PowerShell", "Console Window Host", "cargo watch"] },
        P { id: "nvidia", exe: "nvcontainer.exe", desc: "NVIDIA Container", company: "NVIDIA Corporation", n: 2, cpu: 0.1, mem_mb: 232.0, disk: 0.0, gpu: 2.0, tile: look::T_NV, kids: &["NVIDIA Container", "NVIDIA Container"] },
        P { id: "defender", exe: "MsMpEng.exe", desc: "Antimalware Service", company: "Microsoft Corporation", n: 1, cpu: 0.0, mem_mb: 165.0, disk: 0.1, gpu: 0.0, tile: look::T_SHIELD, kids: &[] },
        P { id: "outlook", exe: "olk.exe", desc: "Microsoft Outlook", company: "Microsoft Corporation", n: 1, cpu: 0.0, mem_mb: 80.0, disk: 0.0, gpu: 0.0, tile: T_OUTLOOK, kids: &[] },
        P { id: "taskmgr", exe: "rtaskmgr-win32.exe", desc: "Task Manager (Rust)", company: "WinRust contributors", n: 2, cpu: 0.3, mem_mb: 60.6, disk: 0.0, gpu: 0.0, tile: T_TASKMGR, kids: &["Task Manager (Rust)", "Task Manager helper"] },
        P { id: "cargo", exe: "cargo.exe", desc: "cargo", company: "The Rust Project", n: 2, cpu: 0.0, mem_mb: 58.6, disk: 0.0, gpu: 0.0, tile: look::T_BOX, kids: &["cargo build", "cargo metadata"] },
        P { id: "registry", exe: "Registry", desc: "Registry", company: "Microsoft Corporation", n: 1, cpu: 0.0, mem_mb: 54.4, disk: 0.1, gpu: 0.0, tile: look::T_SYS, kids: &[] },
        P { id: "topbar", exe: "topbar.exe", desc: "WinRust Bar", company: "WinRust contributors", n: 1, cpu: 0.1, mem_mb: 18.0, disk: 0.0, gpu: 0.0, tile: look::T_WIN, kids: &[] },
        P { id: "search", exe: "rust-search.exe", desc: "WinRust Search", company: "WinRust contributors", n: 1, cpu: 0.0, mem_mb: 9.0, disk: 0.0, gpu: 0.0, tile: look::T_SYS, kids: &[] },
        P { id: "winhelper", exe: "winhelper.exe", desc: "winhelper.exe", company: "", n: 1, cpu: 0.8, mem_mb: 46.2, disk: 0.2, gpu: 0.0, tile: look::T_UNK, kids: &[] },
    ]
}

/// The tile of a made-up app, by the path the demo gave it (`demo:<id>`).
pub fn tile_of(path: &str) -> Option<Tile> {
    let mapped = DEMO_ICONS.lock().ok().and_then(|m| m.get(path).cloned());
    let id = path.strip_prefix("demo:").map(str::to_string).or_else(|| mapped.and_then(|m| m.strip_prefix("demo:").map(str::to_string)))?;
    let id = id.as_str();
    if let Some(t) = apps().into_iter().find(|p| p.id == id).map(|p| p.tile) {
        return Some(t);
    }
    Some(match id {
        "winrust" => look::T_WIN,
        "steam" => look::T_GAME,
        "onedrive" => look::T_CLOUD,
        "nvbroadcast" => look::T_NV,
        _ => look::T_APP,
    })
}

/// The path (`demo:<id>`) of the made-up app a history row names.
pub fn path_named(name: &str) -> Option<String> {
    let l = name.to_lowercase();
    apps().into_iter().find(|p| p.desc.to_lowercase() == l || p.id == l).map(|p| format!("demo:{}", p.id))
}

/// The pid of the first process of an app, as the design's Details page has it.
fn lead_pid(id: &str) -> Option<u32> {
    Some(match id {
        "claude" => 14820,
        "edge" => 9312,
        "spotify" => 11044,
        "rustc" => 20488,
        "explorer" => 6620,
        "topbar" => 4416,
        "taskmgr" => 22104,
        "svchost" => 1288,
        "search" => 5092,
        "winhelper" => 30212,
        _ => return None,
    })
}

/// The command line of an app's processes (the Details page shows it).
fn cmdline(id: &str) -> &'static str {
    match id {
        "claude" => r#""C:\Users\Daniiar\AppData\Local\AnthropicClaude\claude.exe""#,
        "edge" => r#""C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --profile-directory=Default"#,
        "spotify" => r#""C:\Users\Daniiar\AppData\Roaming\Spotify\Spotify.exe""#,
        "rustc" => r"rustc --crate-name topbar --edition=2021 plugins\topbar\src\main.rs",
        "explorer" => r#""C:\Program Files\WinRust\rust-explorer.exe""#,
        "topbar" => r#""C:\Program Files\WinRust\topbar.exe" --tray"#,
        "taskmgr" => r#""C:\Program Files\WinRust\rtaskmgr-win32.exe""#,
        "svchost" => r"C:\Windows\system32\svchost.exe -k LocalServiceNetworkRestricted -p",
        "search" => r#""C:\Program Files\WinRust\rust-search.exe" --service"#,
        "winhelper" => r"C:\Users\Daniiar\AppData\Local\Temp\winhelper.exe -k 4f2a",
        _ => "",
    }
}

fn series(seed: f32, base: f32, amp: f32, n: usize) -> VecDeque<f32> {
    // the design's `series`: 31 points of three waves, here as many as a minute holds
    (0..n)
        .map(|k| {
            let i = k as f32 * 30.0 / (n as f32 - 1.0);
            (base + amp * ((i * 0.7 + seed).sin() * 0.5 + (i * 1.9 + seed * 2.0).sin() * 0.3 + (i * 0.23).sin() * 0.2)).max(0.0)
        })
        .collect()
}

/// The data of the design: 103 processes of 16 apps, the services, startup entries, history and rules of the pages.
pub fn data() -> Data {
    let cfg = store::Settings {
        to_tray: true,
        tray_asked: true,
        theme: 2,
        sort: 3, // the design lists the processes by memory
        rules: vec![store::Rule { proc: "chrome.exe".into(), cpu: 50.0, secs: 60, act: store::RuleAct::Lower, hits: 2 }],
        ..store::Settings::default()
    };
    let mut d = Data::blank(cfg);
    d.is_admin = false;
    d.rules = d.cfg.rules.clone();
    let mut pid = 40_000u32;
    let mut lead_pids: Vec<u32> = Vec::new();
    let mut svc_pid: HashMap<String, u32> = HashMap::new();
    for a in apps() {
        let path = format!("demo:{}", a.id);
        d.res.seed(&path, Res { icon: 0, desc: a.desc.to_string(), company: a.company.to_string() });
        let wsum = (a.n * (a.n + 1) / 2) as f32;
        let lead = lead_pid(a.id).unwrap_or(pid + 4);
        for i in 0..a.n {
            let w = (a.n - i) as f32 / wsum;
            pid += 4;
            let this = match (i, a.id) {
                (0, _) => lead,
                (1..=4, "svchost") => [2840, 3412, 1880, 2016][i - 1],
                _ => pid,
            };
            let title = if a.id == "svchost" || a.n == 1 { None } else { Some(a.kids.get(i).map_or_else(|| format!("{} Helper", a.desc), |k| (*k).to_string())) };
            let reasons = match a.id {
                "winhelper" => vec![suspect::Reason { sev: 3, text: "Unsigned".into() }, suspect::Reason { sev: 3, text: "runs from %TEMP%".into() }, suspect::Reason { sev: 3, text: "opens outbound connections".into() }],
                "powershell" if i == 0 => vec![suspect::Reason { sev: 2, text: "Started with an encoded command".into() }],
                _ => vec![],
            };
            if !cmdline(a.id).is_empty() {
                d.cmds.insert(this, cmdline(a.id).to_string());
            }
            if i == 0 && lead_pid(a.id).is_some() {
                lead_pids.push(this);
            }
            d.procs.push(Proc {
                pid: this,
                ppid: if i == 0 { 4 } else { lead },
                name: a.exe.to_string(),
                exe: Some(path.clone()),
                cpu: a.cpu * w,
                avg: a.cpu * w,
                mem: (f64::from(a.mem_mb) * 1_048_576.0 * f64::from(w)) as u64,
                disk: f64::from(a.disk) * 1_048_576.0 * f64::from(w),
                net: 0.0,
                gpu: if i == 0 { a.gpu } else { 0.0 },
                reasons,
                title,
                user: "Daniiar".into(),
            });
            if a.id == "svchost" && (1..=4).contains(&i) {
                let name = ["Audiosrv", "Dnscache", "WlanSvc", "BthServ"][i - 1].to_string();
                d.svc_pids.insert(this, vec![name.clone()]);
                svc_pid.insert(name, this);
            }
        }
        if a.id == "search" {
            d.suspended.insert(lead); // the design's Details page lists it as Suspended
        }
        if a.id == "rustc" || a.id == "taskmgr" {
            for p in d.procs.iter().filter(|p| p.name == a.exe) {
                d.efficiency.insert(p.pid);
            }
        }
    }
    // the services page, in the design's order
    for (name, display, running) in [
        ("WSearch", "Windows Search", false),
        ("rs-launcher", "WinRust search service", true),
        ("Audiosrv", "Windows Audio", true),
        ("BthServ", "Bluetooth Support Service", true),
        ("Dnscache", "DNS Client", true),
        ("WlanSvc", "WLAN AutoConfig", true),
        ("wuauserv", "Windows Update", false),
        ("Spooler", "Print Spooler", true),
        ("SysMain", "SysMain", false),
    ] {
        let pid = match name {
            "rs-launcher" => 5092,
            "Spooler" => 3104,
            n => svc_pid.get(n).copied().unwrap_or(0),
        };
        d.svcs.push(Svc { name: name.into(), display: display.into(), pid: if running { pid } else { 0 }, running });
    }
    // the startup page
    for (name, publisher, tile_id, cmd, on) in [
        ("WinRust", "WinRust contributors", "winrust", r#""C:\Program Files\WinRust\topbar.exe" --tray"#, true),
        ("Discord", "Discord Inc.", "discord", r"%LocalAppData%\Discord\Update.exe --processStart Discord.exe", true),
        ("Spotify", "Spotify AB", "spotify", r"%AppData%\Spotify\Spotify.exe --autostart --minimized", true),
        ("Telegram Desktop", "Telegram FZ-LLC", "telegram", r"%AppData%\Telegram Desktop\Telegram.exe -autostart", true),
        ("NVIDIA Broadcast", "NVIDIA Corporation", "nvbroadcast", r#""C:\Program Files\NVIDIA Corporation\NVIDIA Broadcast\NVIDIA Broadcast UI.exe""#, true),
        ("Steam Client Bootstrapper", "Valve Corporation", "steam", r#""C:\Program Files (x86)\Steam\steam.exe" -silent"#, false),
        ("Microsoft OneDrive", "Microsoft Corporation", "onedrive", r"%LocalAppData%\Microsoft\OneDrive\OneDrive.exe /background", false),
        ("Claude", "Anthropic", "claude", r#""C:\Users\Daniiar\AppData\Local\AnthropicClaude\claude.exe""#, true),
    ] {
        let item = startup::Item { name: name.into(), command: cmd.into(), source: 0, enabled: on };
        if let Some(exe) = crate::model::exe_from_command(cmd) {
            d.res.seed(&exe, Res { icon: 0, desc: name.into(), company: publisher.into() });
            DEMO_ICONS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(exe, format!("demo:{tile_id}"));
        }
        d.startup.push(item);
    }
    // two entries of no publisher and no icon of their own (the What's slow page tells them by a letter)
    for (name, cmd) in [("Cardinal", r#""C:\Program Files\Cardinal\cardinal.exe""#), ("BotForge Panel", r#""C:\Program Files\BotForge\botforge.exe""#)] {
        d.startup.push(startup::Item { name: name.into(), command: cmd.into(), source: 0, enabled: true });
    }
    // what Windows logged of its last five startups (the design's numbers) and what delayed them
    d.boot = startup::BootLog {
        boots: vec![("2026-11-20 13:39".into(), 88_000, 44_000), ("2026-11-17 09:05".into(), 59_000, 31_000), ("2026-11-14 10:12".into(), 71_000, 36_000), ("2026-11-11 08:47".into(), 126_000, 60_000), ("2026-11-09 09:20".into(), 139_000, 70_000)],
        slow: vec![
            ("msmpeng.exe".into(), "Microsoft Defender Antivirus".into(), 273_000),
            ("claude.exe".into(), "Claude".into(), 91_900),
            ("cardinal.exe".into(), "Cardinal".into(), 86_200),
            ("botforge.exe".into(), "BotForge Panel \u{2014} Steam Account Management".into(), 50_200),
            ("tslgame.exe".into(), "tslgame.exe".into(), 39_800),
        ],
    };
    // what the apps usually use (the real window has no such week of history)
    let mb = |v: u64| v * 1_048_576;
    for (k, mem, cpu) in [("claude.exe", 3740, 0.0), ("msedge.exe", 1620, 0.0), ("spotify.exe", 955, 0.0), ("discord.exe", 380, 0.0), ("rustc.exe", 0, 10.0)] {
        d.usual.insert(k.into(), crate::data::Usual { mem: mb(mem), cpu });
    }
    (d.dpc_pct, d.intr_pct) = (0.7, 0.1);
    // the history page: a row a minute
    let tops = ["Claude", "rustc", "Microsoft Edge", "Claude", "Spotify", "cargo", "Claude", "Discord", "Microsoft Edge", "Antimalware Service", "Claude", "rustc", "Telegram Desktop", "Claude"];
    for (i, top) in tops.iter().enumerate() {
        let cpu = (14.0 + 10.0 * (i as f32 * 1.3).sin() + if *top == "rustc" { 30.0 } else { 0.0 }).round();
        let mem = (39.0 + 3.0 * (i as f32 * 0.6).sin()).round();
        let io = (180.0 + 140.0 * (i as f32 * 2.1).sin().abs()).round();
        let net = (40.0 + 90.0 * (i as f32 * 1.7).cos().abs()).round();
        d.hist.push(vec![format!("2026-11-27 16:{:02}", 58 - i), format!("{cpu}"), format!("{mem}"), format!("{io}"), format!("{net}"), (*top).to_string()]);
    }
    // the charts
    d.series.cpu = series(1.0, 16.0, 14.0, CAP);
    d.series.mem = series(2.0, 39.0, 3.0, CAP);
    d.series.disk = series(3.0, 4.0, 6.0, CAP);
    d.series.rx = series(4.0, 1000.0, 1200.0, CAP);
    d.series.tx = series(6.0, 150.0, 120.0, CAP);
    d.series.gpu = series(5.0, 5.0, 6.0, CAP);
    // the newest point of every chart is the number the page's headline says
    for (q, v) in [(&mut d.series.cpu, 14.0), (&mut d.series.mem, 39.0), (&mut d.series.disk, 2.0), (&mut d.series.rx, 1400.0), (&mut d.series.gpu, 2.0)] {
        if let Some(last) = q.back_mut() {
            *last = v;
        }
    }
    d.cpu_total = 14.0;
    d.mem_pct = 39.0;
    d.sample.disk_active = 2.0;
    d.sample.gpu_total = 2.0;
    d.ticks = 5;
    LEAD_PIDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone_from(&lead_pids);
    d
}

static LEAD_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static DEMO_ICONS: std::sync::LazyLock<Mutex<HashMap<String, String>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// What the Performance page shows in the design: the five devices and their numbers.
pub fn perf_items(d: &Data) -> Vec<PerfItem<'_>> {
    fn g<'a>(label: &str, right: &str, q: &'a VecDeque<f32>, ymax: f32) -> GraphSpec<'a> {
        GraphSpec { label: label.into(), right: right.into(), series: vec![(q, Rgb(0, 0, 0))], ymax }
    }
    #[allow(clippy::too_many_arguments)] // a made-up device: what the page shows of it, one value to a word
    fn it<'a>(kind: PerfKind, title: &str, l1: &str, l2: &str, graphs: Vec<GraphSpec<'a>>, big_title: &str, device: &str, stats: &[(&str, &str)]) -> PerfItem<'a> {
        PerfItem {
        kind,
        title: title.into(),
        line1: l1.into(),
        line2: l2.into(),
        graphs,
        layout: GLayout::Single,
        big_title: big_title.into(),
        device: device.into(),
        stats: stats.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
        stat_cols: 2,
        facts: vec![],
        composition: None,
        }
    }
    vec![
        it(PerfKind::Cpu, "CPU", "14%  3.21 GHz", "", vec![g("% utilization", "100%", &d.series.cpu, 100.0)], "CPU", "AMD Ryzen 7 7840HS", &[("Utilization", "14%"), ("Speed", "3.21 GHz"), ("Processes", "186"), ("Up time", "2:07:41:12")]),
        it(PerfKind::Mem, "Memory", "12.4 / 32 GB", "39%", vec![g("Memory usage", "32 GB", &d.series.mem, 100.0)], "Memory", "32 GB DDR5", &[("In use", "12.4 GB"), ("Available", "19.6 GB"), ("Committed", "18 / 36 GB"), ("Cached", "9.1 GB")]),
        it(PerfKind::Disk(0), "Disk 0 (C:)", "SSD", "2%", vec![g("Active time", "100%", &d.series.disk, 100.0)], "Disk 0 (C:)", "NVMe SSD 1 TB", &[("Active time", "2%"), ("Read", "0.4 MB/s"), ("Write", "1.1 MB/s"), ("Response", "0.3 ms")]),
        it(PerfKind::Nic(0), "Wi-Fi", "S 0.2 · R 1.4 Mbps", "", vec![g("Throughput", "10 Mbps", &d.series.rx, 10_000.0)], "Wi-Fi", "Wi-Fi 6E adapter", &[("Send", "0.2 Mbps"), ("Receive", "1.4 Mbps"), ("Signal", "Excellent"), ("IPv4", "192.168.1.24")]),
        it(PerfKind::Gpu(0), "GPU 0", "Radeon 780M", "2%", vec![g("3D", "100%", &d.series.gpu, 100.0)], "GPU 0", "AMD Radeon 780M", &[("Utilization", "2%"), ("Shared memory", "1.2 GB"), ("Driver", "32.0.1"), ("Temperature", "44 °C")]),
    ]
}

fn users() -> Vec<UserM> {
    let a = |name: &str, id: &str, mem: f64, cpu: f32, disk: f64, pids: &[u32]| AppM { name: name.into(), icon: Some(format!("demo:{id}")), cpu, mem: (mem * 1_048_576.0) as u64, disk: disk * 1_048_576.0, pids: pids.to_vec() };
    vec![
        UserM {
            key: "dan".into(),
            name: "Daniiar".into(),
            sub: "Active · local account · since 08:02".into(),
            dot: UDot::Active,
            procs: 146,
            cpu: 12.9,
            mem: (10.6 * 1_073_741_824.0) as u64,
            disk: 0.2 * 1_048_576.0,
            apps: vec![
                a("Claude", "claude", 4003.0, 0.4, 0.0, &[1004]),
                a("Microsoft Edge", "edge", 1486.0, 0.2, 0.0, &[1076]),
                a("Spotify", "spotify", 1029.0, 0.1, 0.1, &[1120]),
                a("Discord", "discord", 932.0, 0.1, 0.0, &[1160]),
                a("Telegram Desktop", "telegram", 437.0, 0.0, 0.0, &[1360]),
                a("WinRust Explorer", "explorer", 418.0, 0.0, 0.0, &[1380]),
                a("Microsoft Outlook", "outlook", 80.0, 0.0, 0.0, &[1420]),
                a("Task Manager (Rust)", "taskmgr", 61.0, 0.3, 0.0, &[1480]),
            ],
        },
        UserM { key: "guest".into(), name: "Guest".into(), sub: "Locked · signed in at 11:40".into(), dot: UDot::Locked, procs: 9, cpu: 0.0, mem: 212 * 1_048_576, disk: 0.0, apps: vec![] },
    ]
}

/// The window for `state` of the design, on a machine that is made up.
#[must_use]
pub fn demo(state: &str) -> Box<dyn App> {
    let (tm, _fake) = build(state);
    Box::new(tm)
}

/// The window and its fake, for the tests and the benchmark that look at what the fake was asked.
#[must_use]
pub fn build(state: &str) -> (Tm, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let d = data();
    let dm = Demo {
        mem: MemView { total: 32 * 1_073_741_824, used: (12.4 * 1_073_741_824.0) as u64, standby: (9.1 * 1_073_741_824.0) as u64, free: (10.5 * 1_073_741_824.0) as u64 },
        users: users(),
        processes: 186,
        threads: 2412,
        svc_total: 241,
        tiles: vec![("14%".into(), "all users".into(), 0.133), ("11.1 GB".into(), "of 32 GB".into(), 0.35), ("0.2 MB/s".into(), "C:".into(), 0.02), ("1.4 Mbps".into(), "Wi-Fi".into(), 0.03)],
        hook: Hook::None,
        details_all: false,
    };
    let page = match state {
        "performance" | "perf-memory" | "perf-disk" | "perf-network" | "perf-gpu" => Page::Performance,
        "users" | "users-collapsed" => Page::Users,
        "speed" | "speed-open" => Page::Boost,
        "settings" => Page::Settings,
        "history" | "history-cleared" => Page::History,
        "startup" => Page::Startup,
        "details" => Page::Details,
        "services" => Page::Services,
        "rules" | "no-rules" => Page::Rules,
        "insights" | "slow" | "slow-app" | "slow-drv" | "slow-ok" | "slow-checking" => Page::Insights,
        _ => Page::Processes,
    };
    let mut tm = Tm::new(fake.clone(), d, Some(dm), false, page);
    match state {
        "no-alert" => {
            tm.alert_off.insert("winhelper.exe".into());
        }
        "no-results" => {
            tm.search.set_text("zzzz");
            tm.rebuild();
        }
        "history-cleared" => {
            tm.d.hist.clear();
            tm.rebuild();
        }
        "no-rules" => {
            tm.d.rules.clear();
            tm.rebuild();
        }
        "more-menu" => {
            if let Some(d) = tm.demo.as_mut() {
                d.hook = Hook::MoreMenu;
            }
        }
        "process-menu" | "end-tree" => {
            tm.select_key("msedge.exe");
            if let Some(d) = tm.demo.as_mut() {
                d.hook = if state == "end-tree" { Hook::EndTree } else { Hook::ProcessMenu };
            }
        }
        "processes-expanded" => {
            tm.expanded.insert("rustc.exe".into());
            tm.rebuild();
        }
        "users-collapsed" => {}
        "perf-memory" => tm.perf_sel = PerfKind::Mem,
        "perf-disk" => tm.perf_sel = PerfKind::Disk(0),
        "perf-network" => tm.perf_sel = PerfKind::Nic(0),
        "perf-gpu" => tm.perf_sel = PerfKind::Gpu(0),
        "speed-open" => {
            tm.tw_open.insert(0);
        }
        "slow-app" => {
            // Discord's processes together hold 1.6 GB (it usually holds 380 MB)
            let sum: u64 = tm.d.procs.iter().filter(|p| p.name == "Discord.exe").map(|p| p.mem).sum();
            for p in tm.d.procs.iter_mut().filter(|p| p.name == "Discord.exe") {
                p.mem = (p.mem as f64 * 1638.0 * 1_048_576.0 / sum as f64) as u64;
            }
        }
        "slow-drv" => (tm.d.dpc_pct, tm.d.intr_pct) = (3.8, 0.4),
        "slow-ok" => tm.d.boot.boots[0] = ("2026-11-20 13:39".into(), 60_000, 30_000),
        "slow-checking" => tm.slow_chk = Some(crate::app::SlowCheck { serial: 1, born: None }),
        "toast" => tm.say("Efficiency mode on for rustc", look::DOT_OK),
        _ => {}
    }
    for p in [Page::Startup, Page::Details, Page::Services, Page::Rules] {
        tm.sorts.insert(p, (SWITCH_COL, false)); // no column: the order the data has
    }
    tm.rebuild();
    if page == Page::Processes && tm.sel.is_none() && matches!(state, "processes" | "alert" | "no-alert" | "more-menu" | "toast" | "processes-expanded") {
        tm.select_key("rustc.exe"); // the design opens with rustc picked
    }
    if matches!(page, Page::Users) && state != "users-collapsed" {
        tm.u_open.insert("dan".into());
    }
    if state == "tray-question" {
        tm.d.cfg.tray_asked = false;
        if let Some(d) = tm.demo.as_mut() {
            d.hook = Hook::CloseAsk;
        }
    }
    if page == Page::Boost {
        tm.rebuild();
    }
    if page == Page::Insights {
        tm.slow_at = "17:02".into(); // the design's time
        tm.rebuild();
    }
    (tm, fake)
}

/// The window on the Details page with `n` more made-up processes (the benchmark's table of hundreds of rows).
#[must_use]
pub fn big(n: usize) -> (Tm, Arc<Fake>) {
    let (mut tm, fake) = build("details");
    if let Some(dm) = tm.demo.as_mut() {
        dm.details_all = true;
    }
    for i in 0..n {
        tm.d.procs.push(Proc {
            pid: 100_000 + i as u32,
            ppid: 4,
            name: format!("worker-{i:04}.exe"),
            exe: None,
            cpu: (i % 7) as f32 * 0.1,
            avg: 0.0,
            mem: ((i % 97) as u64 + 1) * 3 * 1_048_576,
            disk: 0.0,
            net: 0.0,
            gpu: 0.0,
            reasons: vec![],
            title: None,
            user: "Daniiar".into(),
        });
    }
    tm.rebuild();
    (tm, fake)
}

impl Tm {
    /// Picks the row of the group whose processes have this exe name.
    pub fn select_key(&mut self, exe_lower: &str) {
        if let Some(i) = self.rows.iter().position(|r| r.key == exe_lower) {
            self.select_at(i);
        }
    }

    /// The items that `--render` clicks to open what the state of the design shows (see [`Hook`] and `App::demo_clicks`).
    pub fn demo_click_ids(&self) -> Vec<i32> {
        match self.demo.as_ref().map(|d| d.hook) {
            Some(Hook::ProcessMenu | Hook::EndTree | Hook::CloseAsk) => vec![crate::app::ID_DEMO],
            Some(Hook::MoreMenu) => vec![ID_ACT + 3],
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the machine that touches nothing

/// Every call the window made on the machine, in order, as a line of text.
#[derive(Default)]
pub struct Fake {
    pub calls: Mutex<Vec<String>>,
    /// The tray icon exists (a real run would have made one).
    pub tray: AtomicBool,
    pub hidden: AtomicBool,
    pub shown: AtomicUsize,
    pub tray_items: Mutex<Vec<TrayItem>>,
    pub boost: Mutex<HashMap<&'static str, boost::State>>,
    /// What the fake answers to a service or startup change: `Err(5)` is "access denied".
    pub deny: AtomicBool,
    /// The startup entries it was told to switch (by key): the next read of them shows it.
    pub startup: Mutex<HashMap<String, bool>>,
}

impl Fake {
    fn log(&self, s: String) {
        self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(s);
    }

    /// The log so far.
    #[must_use]
    pub fn log_of(&self) -> Vec<String> {
        self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// What was asked of the machine that changes it (every call except the reads and the window's own).
    #[must_use]
    pub fn changes(&self) -> Vec<String> {
        self.log_of().into_iter().filter(|l| !l.starts_with("read:") && !l.starts_with("window:")).collect()
    }
}

impl Machine for Fake {
    fn refresh(&self, d: &mut Data) -> Vec<String> {
        self.log("read: refresh".into());
        // the numbers drift a little, deterministically, so that a tick is a real change
        d.ticks += 1;
        let t = d.ticks as f32;
        for (q, base, amp) in [(&mut d.series.cpu, 16.0, 14.0), (&mut d.series.mem, 39.0, 3.0), (&mut d.series.disk, 4.0, 6.0), (&mut d.series.rx, 1000.0, 1200.0), (&mut d.series.gpu, 5.0, 6.0)] {
            if q.len() == CAP {
                q.pop_front();
            }
            q.push_back((base + amp * ((t * 0.7).sin() * 0.5 + (t * 1.9).sin() * 0.3)).max(0.0));
        }
        d.cpu_total = d.series.cpu.back().copied().unwrap_or(0.0);
        Vec::new()
    }
    fn load_startup(&self, d: &mut Data) {
        self.log("read: startup".into());
        let told = self.startup.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for it in &mut d.startup {
            if let Some(&on) = told.get(&it.key()) {
                it.enabled = on;
            }
        }
    }
    fn load_history(&self, _d: &mut Data) {
        self.log("read: history".into());
    }
    fn boost_state(&self, id: boost::Id, _admin: bool) -> boost::State {
        let m = self.boost.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        m.get(id.key()).cloned().unwrap_or_else(|| match id {
            boost::Id::MemAuto | boost::Id::Shell => boost::State::On,
            boost::Id::MemNow => boost::State::Info(String::new()),
            _ => boost::State::Off,
        })
    }
    fn boost_done(&self) -> Vec<String> {
        Vec::new()
    }
    fn self_autostart(&self) -> bool {
        false
    }
    fn system_dark(&self) -> bool {
        true
    }

    fn kill(&self, _d: &Data, pid: u32) -> bool {
        self.log(format!("kill {pid}"));
        true
    }
    fn set_priority(&self, pid: u32, class: u32) -> bool {
        self.log(format!("priority {pid} {class}"));
        true
    }
    fn priority_name(&self, _pid: u32) -> String {
        crate::sys::PRIORITIES[2].0.to_string()
    }
    fn suspend(&self, pid: u32, on: bool) -> bool {
        self.log(format!("suspend {pid} {on}"));
        true
    }
    fn set_efficiency(&self, pid: u32, on: bool) -> bool {
        self.log(format!("efficiency {pid} {on}"));
        true
    }
    fn set_affinity(&self, pid: u32, half: bool) -> bool {
        self.log(format!("affinity {pid} {half}"));
        true
    }
    fn service_ctl(&self, name: &str, start: bool) -> Result<(), u32> {
        self.log(format!("service {name} {}", if start { "start" } else { "stop" }));
        if self.deny.load(Ordering::Relaxed) { Err(5) } else { Ok(()) }
    }
    fn startup_set(&self, it: &startup::Item, on: bool) -> Result<(), u32> {
        self.log(format!("startup {} {on}", it.name));
        if self.deny.load(Ordering::Relaxed) {
            return Err(5);
        }
        self.startup.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(it.key(), on);
        Ok(())
    }
    fn boost_start(&self, id: boost::Id, on: bool, _donors: Vec<u32>) -> Result<(), String> {
        self.log(format!("boost {} {on}", id.key()));
        self.boost.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id.key(), if on { boost::State::On } else { boost::State::Off });
        Ok(())
    }
    fn reveal(&self, path: &str) {
        self.log(format!("reveal {path}"));
    }
    fn run_dialog(&self) {
        self.log("run dialog".into());
    }
    fn relaunch_admin(&self) -> bool {
        self.log("relaunch as administrator".into());
        false
    }
    fn set_self_autostart(&self, on: bool) -> bool {
        self.log(format!("autostart {on}"));
        true
    }
    fn etw_start(&self) -> Option<net::Session> {
        self.log("etw start".into());
        None
    }
    fn etw_stop(&self, _s: net::Session) {
        self.log("etw stop".into());
    }
    fn save_settings(&self, cfg: &store::Settings) {
        self.log(format!("save settings view={} to_tray={}", cfg.view, cfg.to_tray));
    }
    fn clear_history(&self) {
        self.log("clear history".into());
    }
    fn export(&self, file: &str, _text: &str) -> Result<String, String> {
        self.log(format!("export {file}"));
        Ok(format!("Desktop\\{file}"))
    }

    fn window_hidden(&self, _hwnd: isize) -> bool {
        self.hidden.load(Ordering::Relaxed)
    }
    fn show_window(&self, _hwnd: isize) {
        self.log("window: show".into());
        self.hidden.store(false, Ordering::Relaxed);
        self.shown.fetch_add(1, Ordering::Relaxed);
    }
    fn hide_window(&self, _hwnd: isize) {
        self.log("window: hide".into());
        self.hidden.store(true, Ordering::Relaxed);
    }
    fn tray_add(&self, _hwnd: isize) {
        self.log("window: tray add".into());
        self.tray.store(true, Ordering::Relaxed);
    }
    fn tray_menu(&self, items: Vec<TrayItem>) {
        *self.tray_items.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = items;
    }
    fn tray_balloon(&self, msg: &str) {
        self.log(format!("window: balloon {msg}"));
    }
    fn tray_remove(&self) {
        self.log("window: tray remove".into());
        self.tray.store(false, Ordering::Relaxed);
    }
}

