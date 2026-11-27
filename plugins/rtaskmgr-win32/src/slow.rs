//! What's slow: the numbers and the words of the page, made from what [`Data`] knows. A pure function of the data (`build`), so the
//! page is tested without a window; `draw/slow.rs` only paints it and `app.rs` only changes what it is made from.
//!
//! Three things can slow a PC down and the page tells about each: how Windows started (the event log: the last startups and the apps
//! that delayed them), what the apps use right now (against what they usually use, where that is known: `Data::usual`), and how long
//! the drivers hold the processor (DPC and interrupts).
use crate::{
    data::{CRITICAL, Data, group_key},
    model::{dec, exe_from_command},
    startup::Item,
};

/// The colour of a chip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Fine,
    Warn,
    /// Nothing to say good or bad about (no data).
    Quiet,
}

#[derive(Clone, Debug)]
pub struct Chip {
    pub text: String,
    pub tone: Tone,
}

/// An app of the "Apps right now" card.
#[derive(Clone, Debug)]
pub struct AppRow {
    pub name: String,
    /// The file whose icon it has.
    pub icon: Option<String>,
    pub metric: String,
    pub now: String,
    /// "usually 380 MB": only where it is known.
    pub usual: Option<String>,
    /// How far the bar goes (0..1), and where the mark of the usual stands.
    pub now_frac: f32,
    pub usual_frac: Option<f32>,
    pub bad: bool,
    /// "4× usual": how far over (rounded), 0 when it is not known.
    pub times: u32,
    pub why: String,
    pub pids: Vec<u32>,
    /// The row is about memory (else the processor).
    pub memory: bool,
}

#[derive(Clone, Debug)]
pub struct Apps {
    pub chip: Chip,
    pub verdict: String,
    pub bad: bool,
    pub rows: Vec<AppRow>,
    /// Some row says what is usual: the columns are about the comparison.
    pub compare: bool,
}

/// One of the two meters of the "Drivers" card.
#[derive(Clone, Debug)]
pub struct Meter {
    pub title: &'static str,
    pub sub: &'static str,
    pub val: String,
    /// Of the 5 % the scale ends at.
    pub frac: f32,
    /// At 2 % sound and video stutter.
    pub hot: bool,
}

#[derive(Clone, Debug)]
pub struct Drivers {
    pub chip: Chip,
    pub verdict: String,
    pub bad: bool,
    pub meters: [Meter; 2],
    pub note: &'static str,
}

/// A column of the chart of the last startups.
#[derive(Clone, Debug)]
pub struct Bar {
    pub label: String,
    pub ms: u32,
    /// Until the desktop (only the last startup is told apart).
    pub desk: u32,
}

/// A row of "What delays it".
#[derive(Clone, Debug)]
pub struct Delay {
    pub name: String,
    pub sub: String,
    /// The file whose icon it has (the startup entry's, or a running process of that name).
    pub icon: Option<String>,
    pub ms: u32,
    pub text: String,
    /// There is a startup entry to turn off (or on again).
    pub can: bool,
    pub on: bool,
    /// The entry's key (`startup::Item::key`).
    pub key: String,
    /// What stands where the button would, for what is not turned off here.
    pub keep: &'static str,
    pub keep_tip: &'static str,
}

#[derive(Clone, Debug)]
pub struct Boot {
    pub chip: Chip,
    pub verdict: String,
    pub bad: bool,
    /// Windows has told of its startups.
    pub have: bool,
    pub when: String,
    pub total: String,
    pub delta: Option<String>,
    pub desk: String,
    pub after: String,
    pub fast: String,
    pub fast_ms: u32,
    pub bars: Vec<Bar>,
    pub delays: Vec<Delay>,
}

#[derive(Clone, Debug)]
pub struct Slow {
    pub title: String,
    pub sub: String,
    /// How many of the three things are slow: the badge of the sidebar.
    pub n: usize,
    pub boot: Boot,
    pub apps: Apps,
    pub drivers: Drivers,
}

/// What the chart's scale is: the tallest column is this many milliseconds, as the design's `bMax = 150`.
pub const CHART_MS: u32 = 150_000;

// ---------------------------------------------------------------------------------------------------------------------
// words

fn secs(ms: u32) -> String {
    tf!("{} с", "{} s", ja: "{} 秒", pt: "{} s", zh: "{} 秒", (f64::from(ms) / 1000.0).round() as u64)
}

fn plus(ms: u32) -> String {
    let v = f64::from(ms) / 1000.0;
    if v >= 10.0 { tf!("+{} с", "+{} s", ja: "+{} 秒", pt: "+{} s", zh: "+{} 秒", v.round() as u64) } else { tf!("+{} с", "+{} s", ja: "+{} 秒", pt: "+{} s", zh: "+{} 秒", dec(v, 1)) }
}

fn parts(date: &str) -> Option<(u32, u32, u32, u32, u32)> {
    let n = |r: std::ops::Range<usize>| date.get(r)?.parse::<u32>().ok();
    Some((n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?))
}

/// 0 is Sunday.
fn weekday(y: u32, m: u32, d: u32) -> usize {
    const T: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m as usize - 1) % 12] + d) % 7) as usize
}

fn weekday_name(i: usize) -> &'static str {
    let names = t!(["вс", "пн", "вт", "ср", "чт", "пт", "сб"], ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"], ja: ["日", "月", "火", "水", "木", "金", "土"], pt: ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"], zh: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]);
    names[i % 7]
}

/// "Fri 20 Nov, 13:39", as the design says the time of the last startup.
pub fn when_text(date: &str) -> String {
    let Some((y, mo, d, h, mi)) = parts(date) else { return date.to_string() };
    let wd = weekday_name(weekday(y, mo, d));
    let months = t!(["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"], ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"], ja: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"], pt: ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"], zh: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"]);
    let mon = months[(mo as usize - 1) % 12];
    match winrust_common::lang!() {
        winrust_common::Lang::Ja => format!("{mon}{d}日({wd}) {h:02}:{mi:02}"),
        winrust_common::Lang::Zh => format!("{mon}{d}日 {wd} {h:02}:{mi:02}"),
        _ => format!("{wd} {d} {mon}, {h:02}:{mi:02}"),
    }
}

/// "Mon 9": the label under a column of the chart.
fn bar_label(date: &str) -> String {
    let Some((y, mo, d, _, _)) = parts(date) else { return date.to_string() };
    let wd = weekday_name(weekday(y, mo, d));
    match winrust_common::lang!() {
        winrust_common::Lang::Ja => format!("{d}日({wd})"),
        winrust_common::Lang::Zh => format!("{d}日 {wd}"),
        _ => format!("{wd} {d}"),
    }
}

fn memory_text(b: u64) -> String {
    crate::draw::fmt_mem_gb(b)
}

fn fine() -> Chip {
    Chip { text: t!("В порядке", "Fine", ja: "問題なし", pt: "Normal", zh: "正常").to_string(), tone: Tone::Fine }
}

fn warn(text: String) -> Chip {
    Chip { text, tone: Tone::Warn }
}

// ---------------------------------------------------------------------------------------------------------------------
// the startup

/// The startup entry that a name of the log (an exe's file name or a service's name, in lower case) is.
pub fn entry_of<'a>(items: &'a [Item], key: &str) -> Option<&'a Item> {
    items.iter().find(|it| if it.source == crate::startup::SERVICE { it.name.to_lowercase() == key } else { it.exe_name().as_deref() == Some(key) })
}

fn is_security(key: &str) -> bool {
    ["msmpeng", "defender", "mssense", "nissrv", "securityhealth"].iter().any(|k| key.contains(k))
}

fn delays_of(d: &Data) -> Vec<Delay> {
    d.boot
        .slow
        .iter()
        .take(5)
        .map(|(key, friendly, ms)| {
            let item = entry_of(&d.startup, key);
            let name = if friendly.is_empty() { key.clone() } else { friendly.clone() };
            let exe = item.and_then(|it| exe_from_command(&it.command));
            let company = exe.as_deref().and_then(|e| d.res.get(e)).map(|r| r.company).unwrap_or_default();
            let sub = match (item, company.is_empty()) {
                (Some(it), false) if it.enabled => tf!("{} · запускается с Windows", "{} · starts with Windows", ja: "{} · Windows と同時に起動", pt: "{} · inicia com o Windows", zh: "{} · 随 Windows 启动", company),
                (Some(it), true) if it.enabled => t!("Запускается с Windows", "Starts with Windows", ja: "Windows と同時に起動", pt: "Inicia com o Windows", zh: "随 Windows 启动").to_string(),
                (Some(_), _) => t!("Не будет запускаться с Windows", "Won’t start with Windows", ja: "Windows と同時に起動しません", pt: "Não vai iniciar com o Windows", zh: "不会随 Windows 启动").to_string(),
                (None, _) if is_security(key) => t!("Проверяет файлы, пока запускается Windows", "Scans files while Windows starts", ja: "Windows の起動中にファイルをスキャン", pt: "Verifica arquivos enquanto o Windows inicia", zh: "在 Windows 启动时扫描文件").to_string(),
                (None, _) => t!("Замедлило запуск Windows", "Slowed Windows startup", ja: "Windows の起動を遅くしました", pt: "Atrasou a inicialização do Windows", zh: "拖慢了 Windows 启动").to_string(),
            };
            let (keep, keep_tip) = if is_security(key) {
                (
                    t!("Защита Windows · оставить", "Windows security · keep on", ja: "Windows セキュリティ · オンのまま", pt: "Segurança do Windows · manter ligado", zh: "Windows 安全 · 保持开启"),
                    t!("Defender проверяет файлы при запуске, чтобы вас защитить. WinRust его не отключает.", "Defender checks files at startup to keep you safe. WinRust doesn’t turn it off.", ja: "Defender は起動時にファイルを確認して保護します。WinRust はこれをオフにしません。", pt: "O Defender verifica arquivos na inicialização para proteger você. O WinRust não o desativa.", zh: "Defender 会在启动时检查文件以保护你。WinRust 不会将其关闭。"),
                )
            } else {
                (
                    t!("Здесь не отключается", "Can’t be turned off here", ja: "ここではオフにできません", pt: "Não dá para desativar aqui", zh: "无法在此关闭"),
                    t!("Это не запись автозагрузки, поэтому её нечем отключить.", "This is not a startup entry, so there is nothing to turn off.", ja: "スタートアップの項目ではないため、オフにできるものがありません。", pt: "Isto não é um item de inicialização, então não há o que desativar.", zh: "这不是启动项，因此没有可关闭的内容。"),
                )
            };
            Delay {
                name,
                sub,
                icon: exe.or_else(|| running_exe(d, key)),
                ms: *ms,
                text: plus(*ms),
                can: item.is_some(),
                on: item.is_some_and(|it| it.enabled),
                key: item.map(Item::key).unwrap_or_default(),
                keep,
                keep_tip,
            }
        })
        .collect()
}

/// The exe of a running process of this file name (its icon stands for what the log names).
fn running_exe(d: &Data, key: &str) -> Option<String> {
    d.procs.iter().find(|p| p.name.to_lowercase() == key).and_then(|p| p.exe.clone())
}

fn boot_of(d: &Data) -> Boot {
    let log = &d.boot;
    let none = |verdict: String| Boot {
        chip: Chip { text: t!("Нет данных", "No data", ja: "データなし", pt: "Sem dados", zh: "无数据").to_string(), tone: Tone::Quiet },
        verdict,
        bad: false,
        have: false,
        when: String::new(),
        total: String::new(),
        delta: None,
        desk: String::new(),
        after: String::new(),
        fast: String::new(),
        fast_ms: 0,
        bars: Vec::new(),
        delays: Vec::new(),
    };
    let Some(last) = log.boots.first() else {
        return none(if d.is_admin {
            t!("Windows ещё не записала время запуска.", "Windows has not logged a startup yet.", ja: "Windows はまだ起動時間を記録していません。", pt: "O Windows ainda não registrou uma inicialização.", zh: "Windows 尚未记录启动时间。").to_string()
        } else {
            t!("Windows показывает время запуска только администратору.", "Windows shows its startup times only to an administrator.", ja: "Windows は起動時間を管理者にだけ表示します。", pt: "O Windows mostra os tempos de inicialização só a um administrador.", zh: "Windows 只向管理员显示启动时间。").to_string()
        });
    };
    let shown: Vec<&(String, u32, u32)> = log.boots.iter().take(5).rev().collect();
    let fast_ms = shown.iter().map(|b| b.1).min().unwrap_or(last.1);
    let delta = last.1.saturating_sub(fast_ms);
    let delays = delays_of(d);
    let left: Vec<&Delay> = delays.iter().filter(|x| x.can && x.on).collect();
    let slow_start = delta >= 10_000 && f64::from(last.1) >= f64::from(fast_ms) * 1.2;
    let bad = slow_start && !left.is_empty();
    let verdict = if bad {
        tf!("Прошлый запуск занял {} — на {} дольше самого быстрого. Основная часть — приложения, которые можно отключить.", "It took {} last time, {} longer than your fastest. Apps you can turn off cause most of it.", ja: "前回は {} かかり、最速より {} 長くなりました。大半はオフにできるアプリが原因です。", pt: "Levou {} da última vez, {} a mais que o seu mais rápido. A maior parte vem de apps que você pode desativar.", zh: "上次用了 {}，比最快的一次多 {}。主要原因是可以关闭的应用。", secs(last.1), secs(delta))
    } else if slow_start {
        t!("Вы отключили то, что тормозило запуск. Следующий должен быть быстрее.", "You turned off what slowed it. The next startup should be faster.", ja: "遅くしていたものをオフにしました。次回の起動は速くなるはずです。", pt: "Você desativou o que atrasava. A próxima inicialização deve ser mais rápida.", zh: "你已关闭拖慢启动的项目。下次启动应该会更快。").to_string()
    } else {
        t!("Запуск идёт с обычной скоростью.", "Startup time is normal.", ja: "起動時間は通常どおりです。", pt: "O tempo de inicialização está normal.", zh: "启动时间正常。").to_string()
    };
    let bars: Vec<Bar> = shown
        .iter()
        .enumerate()
        .map(|(i, b)| Bar { label: if i + 1 == shown.len() { t!("Последний", "Last", ja: "前回", pt: "Último", zh: "最近").to_string() } else { bar_label(&b.0) }, ms: b.1, desk: b.2 })
        .collect();
    Boot {
        chip: if bad { warn(t!("Медленно", "Slow", ja: "遅い", pt: "Lento", zh: "较慢").to_string()) } else { fine() },
        verdict,
        bad,
        have: true,
        when: when_text(&last.0),
        total: secs(last.1),
        delta: (bad || slow_start).then(|| plus(delta)),
        desk: secs(last.2),
        after: secs(last.1.saturating_sub(last.2)),
        fast: secs(fast_ms),
        fast_ms,
        bars,
        delays,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the apps

struct Group {
    key: String,
    name: String,
    icon: Option<String>,
    mem: u64,
    avg: f32,
    pids: Vec<u32>,
}

fn groups_of(d: &Data) -> Vec<Group> {
    let mut gs: Vec<Group> = Vec::new();
    for p in &d.procs {
        let key = group_key(&p.name);
        if CRITICAL.contains(&key.as_str()) {
            continue;
        }
        match gs.iter_mut().find(|g| g.key == key) {
            Some(g) => {
                g.mem += p.mem;
                g.avg += p.avg;
                g.pids.push(p.pid);
            }
            None => {
                // as the process list names it: what the file says it is, else its file name
                let desc = p.exe.as_deref().and_then(|e| d.res.get(e)).map(|r| r.desc).filter(|x| !x.is_empty());
                gs.push(Group { name: desc.unwrap_or_else(|| p.name.trim_end_matches(".exe").to_string()), icon: p.exe.clone(), key, mem: p.mem, avg: p.avg, pids: vec![p.pid] });
            }
        }
    }
    gs
}

fn apps_of(d: &Data) -> Apps {
    let gs = groups_of(d);
    let mut by_mem: Vec<&Group> = gs.iter().collect();
    by_mem.sort_by_key(|g| std::cmp::Reverse(g.mem));
    let mut picked: Vec<(&Group, bool)> = by_mem.iter().take(3).map(|g| (*g, true)).collect();
    if let Some(top) = gs.iter().filter(|g| g.avg >= 8.0 && !picked.iter().any(|(p, _)| p.key == g.key)).max_by(|a, b| a.avg.total_cmp(&b.avg)) {
        picked.push((top, false));
    }
    let top_mem = by_mem.first().map(|g| g.key.clone());
    let biggest = |mem: bool| picked.iter().filter(|(_, m)| *m == mem).map(|(g, _)| if mem { g.mem as f64 } else { f64::from(g.avg) }).fold(1e-9, f64::max);
    let (max_mem, max_cpu) = (biggest(true), biggest(false));
    let mut rows: Vec<AppRow> = picked
        .iter()
        .map(|(g, memory)| {
            let now = if *memory { g.mem as f64 } else { f64::from(g.avg) };
            let usual = d.usual.get(&g.key).map(|u| if *memory { u.mem as f64 } else { f64::from(u.cpu) }).filter(|u| *u > 0.0);
            let over = usual.map(|u| now / u);
            let pressure = usual.is_none() && *memory && d.mem_pct >= 80.0 && top_mem.as_deref() == Some(g.key.as_str());
            let heavy_cpu = usual.is_none() && !*memory && g.avg >= 50.0;
            let bad = over.is_some_and(|o| o >= 2.0 && (*memory || g.avg >= 10.0)) || pressure || heavy_cpu;
            let times = over.filter(|o| *o >= 2.0).map_or(0, |o| o.round() as u32);
            let fmt = |v: f64| if *memory { memory_text(v as u64) } else { format!("{}%", v.round() as u64) };
            let mx = usual.map_or(if *memory { max_mem } else { max_cpu }, |u| now.max(u) * 1.15);
            let why = if !bad {
                String::new()
            } else if times > 0 && *memory {
                t!("Занимает намного больше памяти, чем обычно. Перезапуск обычно возвращает её.", "Uses far more memory than usual. Restarting it usually gives the memory back.", ja: "いつもよりはるかに多くのメモリを使っています。再起動すると通常は元に戻ります。", pt: "Usa muito mais memória que o normal. Reiniciar costuma devolvê-la.", zh: "占用的内存远超平时。重启通常能把内存还回来。").to_string()
            } else if times > 0 {
                t!("Загружает ЦП намного сильнее, чем обычно за последние 30 с.", "Uses far more processor time than usual over the last 30 s.", ja: "直近 30 秒で、いつもよりはるかに多く CPU を使っています。", pt: "Usa muito mais CPU que o normal nos últimos 30 s.", zh: "最近 30 秒占用的 CPU 远超平时。").to_string()
            } else if *memory {
                tf!("Память занята на {:.0}%, и больше всего её у этого приложения.", "Memory is {:.0}% used and this app holds the most of it.", ja: "メモリ使用率は {:.0}% で、このアプリが最も多く使っています。", pt: "A memória está {:.0}% em uso e este app usa a maior parte.", zh: "内存已使用 {:.0}%，此应用占用最多。", d.mem_pct)
            } else {
                t!("Последние 30 с оно сильно загружает ЦП.", "It has kept the processor busy for the last 30 s.", ja: "直近 30 秒、CPU を大きく使い続けています。", pt: "Manteve a CPU ocupada nos últimos 30 s.", zh: "最近 30 秒一直占用大量 CPU。").to_string()
            };
            AppRow {
                name: g.name.clone(),
                icon: g.icon.clone(),
                metric: if *memory { t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存").to_string() } else { t!("ЦП · среднее за 30 с", "CPU · 30 s average", ja: "CPU · 30 秒平均", pt: "CPU · média de 30 s", zh: "CPU · 30 秒平均").to_string() },
                now: fmt(now),
                usual: usual.map(|u| tf!("обычно {}", "usually {}", ja: "通常 {}", pt: "normalmente {}", zh: "通常 {}", fmt(u))),
                now_frac: (now / mx).clamp(0.0, 1.0) as f32,
                usual_frac: usual.map(|u| (u / mx).clamp(0.0, 1.0) as f32),
                bad,
                times,
                why,
                pids: g.pids.clone(),
                memory: *memory,
            }
        })
        .collect();
    // what is wrong comes first
    rows.sort_by_key(|r| !r.bad);
    let bad: Vec<&AppRow> = rows.iter().filter(|r| r.bad).collect();
    let compare = rows.iter().any(|r| r.usual.is_some());
    let (chip, verdict) = if let Some(first) = bad.first() {
        let n = bad.len();
        let chip = warn(if n == 1 { t!("1 приложение", "1 app", ja: "1 個のアプリ", pt: "1 app", zh: "1 个应用").to_string() } else { tf!("Приложений: {}", "{} apps", ja: "{} 個のアプリ", pt: "{} apps", zh: "{} 个应用", n) });
        let verdict = if first.times > 0 {
            tf!("«{}» занимает намного больше обычного. Остальное в норме.", "{} uses far more than usual. Everything else is normal.", ja: "{} はいつもよりはるかに多く使っています。ほかは正常です。", pt: "{} usa muito mais que o normal. O resto está normal.", zh: "{} 的占用远超平时。其他一切正常。", first.name)
        } else if first.memory {
            tf!("Память почти заполнена, больше всего её у «{}».", "Memory is nearly full and {} holds the most of it.", ja: "メモリがほぼ満杯で、{} が最も多く使っています。", pt: "A memória está quase cheia e {} usa a maior parte.", zh: "内存几乎已满，{} 占用最多。", first.name)
        } else {
            tf!("«{}» сильно загружает ЦП.", "{} keeps the processor busy.", ja: "{} が CPU を大きく使っています。", pt: "{} mantém a CPU ocupada.", zh: "{} 占用了大量 CPU。", first.name)
        };
        (chip, verdict)
    } else if compare {
        (fine(), t!("Каждое приложение использует примерно столько же, сколько обычно.", "Every app uses about as much as it usually does.", ja: "どのアプリもいつもと同じくらいの使用量です。", pt: "Cada app usa quase o mesmo que costuma usar.", zh: "每个应用的占用都和平时差不多。").to_string())
    } else {
        (fine(), t!("Ни одно приложение сейчас не выделяется.", "No app stands out right now.", ja: "いま目立って重いアプリはありません。", pt: "Nenhum app se destaca agora.", zh: "目前没有明显占用过多的应用。").to_string())
    };
    Apps { chip, verdict, bad: !bad.is_empty(), rows, compare }
}

// ---------------------------------------------------------------------------------------------------------------------
// the drivers

fn drivers_of(d: &Data) -> Drivers {
    let (dpc, intr) = (d.dpc_pct, d.intr_pct);
    let meter = |title, sub, v: f32| Meter { title, sub, val: format!("{}%", dec(f64::from(v), 1)), frac: (v / 5.0).clamp(0.0, 1.0), hot: v >= 2.0 };
    let meters = [
        meter(t!("Работа драйверов (DPC)", "Driver work (DPC)", ja: "ドライバーの処理 (DPC)", pt: "Trabalho dos drivers (DPC)", zh: "驱动程序工作 (DPC)"), t!("Время, которое драйверы тратят на задачи устройств", "Time drivers spend finishing hardware tasks", ja: "ドライバーがハードウェアの処理に使う時間", pt: "Tempo que os drivers gastam concluindo tarefas de hardware", zh: "驱动程序完成硬件任务所用的时间"), dpc),
        meter(t!("Прерывания", "Interrupts", ja: "割り込み", pt: "Interrupções", zh: "中断"), t!("Время, которое уходит на ответы устройствам", "Time spent answering devices", ja: "デバイスへの応答に使う時間", pt: "Tempo gasto respondendo aos dispositivos", zh: "响应设备所用的时间"), intr),
    ];
    let bad = meters.iter().any(|m| m.hot);
    Drivers {
        chip: if bad { warn(t!("Подтормаживает", "Stutters", ja: "カクつく", pt: "Engasga", zh: "卡顿").to_string()) } else { fine() },
        verdict: if bad {
            t!("Драйверы держат ЦП так долго, что это вызывает подтормаживание.", "Drivers hold the CPU long enough to cause stutter.", ja: "ドライバーが CPU を長く占有し、カクつきの原因になっています。", pt: "Os drivers seguram a CPU por tempo suficiente para causar engasgos.", zh: "驱动程序占用 CPU 的时间足以造成卡顿。").to_string()
        } else {
            t!("Драйверы не задерживают ЦП.", "Drivers aren’t holding up the CPU.", ja: "ドライバーが CPU を妨げていません。", pt: "Os drivers não estão atrasando a CPU.", zh: "驱动程序没有拖累 CPU。").to_string()
        },
        bad,
        meters,
        note: t!(
            "Драйверы занимают ЦП настолько, что трещит звук, дёргается мышь и проседают кадры. Чаще всего виноваты драйверы Wi-Fi/сети, звука, видео или USB: обновите их или отключайте по очереди в диспетчере устройств.",
            "Drivers hold the CPU long enough to make audio crackle, the mouse jerk and frames drop. Usually Wi-Fi/network, audio, graphics or USB drivers: update them, or turn them off one by one in Device Manager.",
            ja: "ドライバーが CPU を長く占有し、音が途切れたりマウスがぎこちなくなったりフレームが落ちたりします。多くは Wi-Fi/ネットワーク、オーディオ、グラフィックス、USB のドライバーが原因です。更新するか、デバイス マネージャーで 1 つずつ無効にしてください。",
            pt: "Os drivers seguram a CPU por tempo suficiente para o áudio chiar, o mouse travar e os quadros caírem. Em geral são drivers de Wi-Fi/rede, áudio, vídeo ou USB: atualize-os ou desative-os um a um no Gerenciador de Dispositivos.",
            zh: "驱动程序占用 CPU 的时间过长，会导致音频爆音、鼠标卡顿和掉帧。通常是 Wi-Fi/网络、声卡、显卡或 USB 驱动：请更新它们，或在设备管理器中逐个禁用排查。"
        ),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// the page

/// The page for what `d` knows now.
pub fn build(d: &Data) -> Slow {
    let (boot, apps, drivers) = (boot_of(d), apps_of(d), drivers_of(d));
    let n = usize::from(boot.bad) + usize::from(apps.bad) + usize::from(drivers.bad);
    let left: Vec<&Delay> = boot.delays.iter().filter(|x| x.can && x.on).collect();
    let boot_part = || {
        let first = left.first().map_or("", |x| x.name.as_str());
        match left.len() {
            0 | 1 => tf!("{} сильнее всего замедляет запуск", "{} adds the most delay", ja: "{} が起動を最も遅くしています", pt: "{} é o que mais atrasa", zh: "{} 对启动的拖慢最多", first),
            2 => tf!("{} и ещё {} в автозагрузке сильнее всего замедляют запуск", "{} and {} more startup app add the most delay", ja: "{} とほか {} 件のスタートアップ アプリが起動を最も遅くしています", pt: "{} e mais {} app de inicialização atrasam mais", zh: "{} 等 {} 个启动应用对启动的拖慢最多", first, left.len() - 1),
            _ => tf!("{} и ещё {} в автозагрузке сильнее всего замедляют запуск", "{} and {} more startup apps add the most delay", ja: "{} とほか {} 件のスタートアップ アプリが起動を最も遅くしています", pt: "{} e mais {} apps de inicialização atrasam mais", zh: "{} 等 {} 个启动应用对启动的拖慢最多", first, left.len() - 1),
        }
    };
    let app_first = apps.rows.iter().find(|r| r.bad);
    let title = match (n, app_first) {
        (0, _) => t!("Ничто не тормозит этот ПК", "Nothing is slowing this PC down", ja: "この PC を遅くしているものはありません", pt: "Nada está deixando este PC lento", zh: "没有什么在拖慢这台电脑").to_string(),
        (1, Some(r)) if r.memory => tf!("«{}» съедает память", "{} is eating memory", ja: "{} がメモリを大量に使っています", pt: "{} está consumindo memória", zh: "{} 占用了大量内存", r.name),
        (1, Some(r)) => tf!("«{}» съедает ЦП", "{} is eating CPU time", ja: "{} が CPU を大量に使っています", pt: "{} está consumindo CPU", zh: "{} 占用了大量 CPU", r.name),
        (1, None) if drivers.bad => t!("Драйвер вызывает треск звука и подтормаживание видео", "A driver makes sound and video stutter", ja: "ドライバーが音と映像のカクつきを起こしています", pt: "Um driver faz o som e o vídeo engasgarem", zh: "有驱动程序导致声音和视频卡顿").to_string(),
        (1, None) => t!("Windows долго запускается", "Windows takes long to start", ja: "Windows の起動に時間がかかっています", pt: "O Windows demora para iniciar", zh: "Windows 启动很慢").to_string(),
        (k, _) => tf!("Тормозят {} вещи", "{} things slow this PC down", ja: "{} 件が PC を遅くしています", pt: "{} coisas deixam este PC lento", zh: "有 {} 项在拖慢这台电脑", k),
    };
    let sub = if n == 0 {
        t!("Приложения, драйверы и запуск выглядят нормально.", "Apps, drivers and startup all look normal.", ja: "アプリ、ドライバー、起動はいずれも正常です。", pt: "Apps, drivers e inicialização parecem normais.", zh: "应用、驱动程序和启动看起来都正常。").to_string()
    } else {
        let mut parts: Vec<String> = Vec::new();
        if let Some(r) = app_first {
            parts.push(if r.times > 0 && r.memory {
                tf!("«{}» занимает в {}× больше обычной памяти", "{} uses {}× its usual memory", ja: "{} は通常の {} 倍のメモリを使用", pt: "{} usa {}× a memória habitual", zh: "{} 使用了平时 {} 倍的内存", r.name, r.times)
            } else if r.times > 0 {
                tf!("«{}» занимает в {}× больше обычного ЦП", "{} uses {}× its usual CPU time", ja: "{} は通常の {} 倍の CPU を使用", pt: "{} usa {}× a CPU habitual", zh: "{} 使用了平时 {} 倍的 CPU", r.name, r.times)
            } else if r.memory {
                tf!("«{}» держит больше всего памяти", "{} holds the most memory", ja: "{} が最も多くのメモリを使用", pt: "{} usa mais memória", zh: "{} 占用内存最多", r.name)
            } else {
                tf!("«{}» сильно загружает ЦП", "{} keeps the CPU busy", ja: "{} が CPU を大きく使用", pt: "{} mantém a CPU ocupada", zh: "{} 占用大量 CPU", r.name)
            });
        }
        if drivers.bad {
            parts.push(t!("драйверы слишком долго держат ЦП", "drivers hold the CPU too long", ja: "ドライバーが CPU を長く占有", pt: "drivers seguram a CPU por muito tempo", zh: "驱动程序占用 CPU 过久").to_string());
        }
        if boot.bad {
            parts.push(boot_part());
        }
        let s = parts.join(" · ");
        let mut ch = s.chars();
        ch.next().map_or(String::new(), |f| f.to_uppercase().chain(ch).collect())
    };
    Slow { title, sub, n, boot, apps, drivers }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{data::Usual, startup::BootLog, store};

    fn data() -> Data {
        Data::blank(store::Settings::default())
    }

    fn log(boots: &[(&str, u32, u32)], slow: &[(&str, &str, u32)]) -> BootLog {
        BootLog { boots: boots.iter().map(|b| (b.0.to_string(), b.1, b.2)).collect(), slow: slow.iter().map(|s| (s.0.to_string(), s.1.to_string(), s.2)).collect() }
    }

    #[test]
    fn weekdays_and_dates() {
        // 2026-11-20 is a Friday, 2026-11-27 too; 2000-01-01 was a Saturday
        assert_eq!((weekday(2026, 11, 20), weekday(2026, 11, 27), weekday(2000, 1, 1)), (5, 5, 6));
        assert_eq!(parts("2026-11-20 13:39"), Some((2026, 11, 20, 13, 39)));
        assert_eq!(parts("garbage"), None);
        assert_eq!(when_text("garbage"), "garbage");
        // the tests run in Russian
        assert_eq!(when_text("2026-11-20 13:39"), "пт 20 ноя, 13:39");
        assert_eq!(bar_label("2026-11-09 08:00"), "пн 9");
    }

    #[test]
    fn a_quiet_pc_has_nothing_to_say() {
        let s = build(&data());
        assert_eq!(s.n, 0);
        assert!(!s.boot.have && s.boot.chip.tone == Tone::Quiet, "no log: no data, not a verdict");
        assert!(!s.boot.bad && !s.apps.bad && !s.drivers.bad);
        assert!(!s.title.is_empty() && !s.sub.is_empty());
    }

    #[test]
    fn a_slow_startup_names_what_can_be_turned_off() {
        let mut d = data();
        d.boot = log(
            &[("2026-11-20 13:39", 88_000, 44_000), ("2026-11-17 09:00", 59_000, 30_000), ("2026-11-14 09:00", 71_000, 33_000), ("2026-11-11 09:00", 126_000, 50_000), ("2026-11-09 09:00", 139_000, 60_000), ("2026-11-08 09:00", 10_000, 5_000)],
            &[("msmpeng.exe", "Microsoft Defender Antivirus", 273_000), ("claude.exe", "Claude", 91_900), ("tslgame.exe", "tslgame.exe", 39_800)],
        );
        d.startup = vec![Item { name: "Claude".into(), command: r#""C:\Apps\Claude\claude.exe""#.into(), source: 0, enabled: true }];
        let b = build(&d).boot;
        assert!(b.have && b.bad);
        assert_eq!((b.total.as_str(), b.fast_ms, b.delta.as_deref()), ("88 с", 59_000, Some("+29 с")), "the fastest of the five shown, not of the whole log");
        assert_eq!(b.bars.len(), 5, "the last five, the oldest first");
        assert_eq!((b.bars[0].ms, b.bars[4].ms), (139_000, 88_000));
        assert_eq!(b.desk, "44 с");
        assert_eq!(b.after, "44 с");
        let by = |n: &str| b.delays.iter().find(|x| x.name == n).expect("the row");
        assert!(by("Claude").can && by("Claude").on && !by("Claude").key.is_empty());
        assert!(!by("Microsoft Defender Antivirus").can && by("Microsoft Defender Antivirus").keep.contains("Защита"), "Defender is kept on");
        assert!(!by("tslgame.exe").can, "no entry, nothing to turn off");
        assert_eq!(by("Claude").text, "+92 с");
        let s = build(&d);
        assert_eq!(s.n, 1);
        assert!(s.sub.contains("Claude"), "{}", s.sub);
    }

    #[test]
    fn once_the_apps_are_turned_off_the_startup_is_no_longer_called_slow() {
        let mut d = data();
        d.boot = log(&[("2026-11-20 13:39", 88_000, 44_000), ("2026-11-17 09:00", 59_000, 30_000)], &[("claude.exe", "Claude", 91_900)]);
        d.startup = vec![Item { name: "Claude".into(), command: r#""C:\Apps\Claude\claude.exe""#.into(), source: 0, enabled: false }];
        let s = build(&d);
        assert!(!s.boot.bad && s.boot.delta.is_some(), "still the slowest, but nothing is left to blame");
        assert!(!s.boot.delays[0].on && s.boot.delays[0].sub.contains("Не будет"));
        assert_eq!(s.n, 0);
    }

    #[test]
    fn a_normal_startup_is_fine_even_with_apps_in_the_log() {
        let mut d = data();
        d.boot = log(&[("2026-11-20 13:39", 60_000, 30_000), ("2026-11-17 09:00", 59_000, 30_000)], &[("claude.exe", "Claude", 3_000)]);
        d.startup = vec![Item { name: "Claude".into(), command: r#""C:\Apps\Claude\claude.exe""#.into(), source: 0, enabled: true }];
        let b = build(&d).boot;
        assert!(!b.bad && b.chip.tone == Tone::Fine && b.delta.is_none());
        assert_eq!(b.delays[0].text, "+3,0 с");
    }

    #[test]
    fn the_drivers_are_slow_from_two_per_cent() {
        let mut d = data();
        (d.dpc_pct, d.intr_pct) = (0.7, 0.1);
        assert!(!build(&d).drivers.bad);
        (d.dpc_pct, d.intr_pct) = (3.8, 0.4);
        let s = build(&d);
        assert!(s.drivers.bad && s.drivers.meters[0].hot && !s.drivers.meters[1].hot);
        assert_eq!(s.drivers.meters[0].val, "3,8%");
        assert!((s.drivers.meters[0].frac - 0.76).abs() < 1e-6);
        assert_eq!(s.n, 1);
        d.dpc_pct = 9.0;
        assert_eq!(build(&d).drivers.meters[0].frac, 1.0, "the bar stops at its end");
    }

    fn proc(pid: u32, name: &str, mem_mb: u64, avg: f32) -> crate::data::Proc {
        crate::data::Proc { pid, ppid: 4, name: name.into(), exe: None, cpu: avg, avg, mem: mem_mb * 1_048_576, disk: 0.0, net: 0.0, gpu: 0.0, reasons: vec![], title: None, user: "u".into() }
    }

    #[test]
    fn an_app_far_over_its_usual_is_flagged_and_what_is_not_known_is_not_compared() {
        let mut d = data();
        d.procs = vec![proc(10, "Discord.exe", 700, 0.0), proc(11, "Discord.exe", 938, 0.0), proc(20, "edge.exe", 400, 0.0), proc(30, "claude.exe", 1200, 0.0), proc(40, "rustc.exe", 100, 12.0), proc(4, "System", 9000, 0.0)];
        let s = build(&d);
        assert!(!s.apps.compare && !s.apps.bad, "nothing is known of the usual and memory is not short");
        assert!(s.apps.rows.iter().all(|r| r.usual.is_none() && r.usual_frac.is_none()));
        assert!(s.apps.rows.iter().all(|r| r.name != "System"), "the system's own are not an app");
        assert_eq!(s.apps.rows.len(), 4, "the three biggest in memory and the busiest in the processor");
        d.usual.insert("discord.exe".into(), Usual { mem: 380 * 1_048_576, cpu: 0.0 });
        d.usual.insert("claude.exe".into(), Usual { mem: 1120 * 1_048_576, cpu: 0.0 });
        d.usual.insert("rustc.exe".into(), Usual { mem: 0, cpu: 10.0 });
        let s = build(&d);
        assert!(s.apps.compare && s.apps.bad);
        let first = &s.apps.rows[0];
        assert!(first.name == "Discord" && first.bad && first.times == 4, "{first:?}");
        assert_eq!(first.now, "1,6 GB".replace(',', "."), "the sum of its processes");
        assert!(first.usual.as_deref().is_some_and(|u| u.contains("380")));
        assert!(s.apps.rows.iter().filter(|r| r.bad).count() == 1, "Claude 1.07 times and rustc 1.2 times are as usual");
        assert_eq!(s.n, 1);
        assert!(s.title.contains("Discord"), "{}", s.title);
        assert!(s.sub.contains("4"), "{}", s.sub);
    }

    #[test]
    fn short_memory_blames_the_biggest_app() {
        let mut d = data();
        d.procs = vec![proc(1, "a.exe", 3000, 0.0), proc(2, "b.exe", 900, 0.0)];
        d.mem_pct = 91.0;
        let s = build(&d);
        assert!(s.apps.bad && s.apps.rows[0].name == "a" && s.apps.rows[0].why.contains("91"), "{:?}", s.apps.rows[0]);
        assert_eq!(s.n, 1);
    }

    #[test]
    fn the_number_of_slow_things_is_the_badge() {
        let mut d = data();
        d.procs = vec![proc(1, "a.exe", 3000, 0.0)];
        d.mem_pct = 91.0;
        d.dpc_pct = 4.0;
        d.boot = log(&[("2026-11-20 13:39", 88_000, 44_000), ("2026-11-17 09:00", 59_000, 30_000)], &[("claude.exe", "Claude", 91_900)]);
        d.startup = vec![Item { name: "Claude".into(), command: r#""C:\Apps\Claude\claude.exe""#.into(), source: 0, enabled: true }];
        let s = build(&d);
        assert_eq!(s.n, 3);
        assert!(s.title.contains('3'), "{}", s.title);
    }
}
