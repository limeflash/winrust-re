//! How the Task Manager looks: TaskManager.html drawn with `winrust-ui` (the sidebar, the head, the ten pages, the toast). It
//! only reads the window's state ([`Tm`]) and names its items; what a click does is `app.rs`.
use std::sync::LazyLock;

use crate::{
    app::*,
    boost,
    data::*,
    look::{self, *},
    model::*,
    perfview::*,
};
use winrust_ui::{
    anim::{EASE, LINEAR, SPRING},
    frame::{Frame, Hot, Id, WIN_CLOSE, WIN_MAX, WIN_MIN},
    gfx::{Align, Canvas, Font, Rect, Rgba},
    widgets::{
        field::{FieldStyle, field_with},
        list::{ListLook, columns as header, list_with},
        scroll::scroll,
    },
};

pub mod slowpage;

pub const HEAD_H: f32 = 64.0;
pub const SIDE_W: f32 = 228.0;
const F13: Font = Font::new(13.0, 400);
const F13M: Font = Font::new(13.0, 500);
const F12: Font = Font::new(12.0, 400);
const F12M: Font = Font::new(12.0, 500);

fn ink(a: f32) -> Rgba {
    Rgba::white(a)
}

/// `.cell`-sized numbers: 13 px, digits of one width.
fn num() -> Font {
    F13.tabular()
}

// ---------------------------------------------------------------------------------------------------------------------
// what the pages are made of

/// An app of a user: its processes together.
#[derive(Clone, Debug)]
pub struct AppM {
    pub name: String,
    /// The file whose icon it has (a made-up one starts with `demo:`).
    pub icon: Option<String>,
    pub cpu: f32,
    pub mem: u64,
    pub disk: f64,
    pub pids: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UDot {
    Active,
    Locked,
    Other,
}

#[derive(Clone, Debug)]
pub struct UserM {
    pub key: String,
    pub name: String,
    pub sub: String,
    pub dot: UDot,
    pub procs: usize,
    pub cpu: f32,
    pub mem: u64,
    pub disk: f64,
    pub apps: Vec<AppM>,
}

/// The signed-in users of what [`Data`] knows: every account that owns processes, with its apps (the processes of one name
/// together).
pub fn users_of(d: &Data) -> Vec<UserM> {
    let me = std::env::var("USERNAME").unwrap_or_default().to_lowercase();
    let mut by: Vec<(String, Vec<&Proc>)> = Vec::new();
    for p in &d.procs {
        match by.iter_mut().find(|(u, _)| *u == p.user) {
            Some(e) => e.1.push(p),
            None => by.push((p.user.clone(), vec![p])),
        }
    }
    let mut out: Vec<UserM> = by
        .into_iter()
        .map(|(user, ps)| {
            let mut apps: Vec<AppM> = Vec::new();
            for p in &ps {
                let key = p.name.to_lowercase();
                match apps.iter_mut().find(|a| a.name.to_lowercase() == key) {
                    Some(a) => {
                        a.cpu += p.cpu;
                        a.mem += p.mem;
                        a.disk += p.disk;
                        a.pids.push(p.pid);
                    }
                    None => apps.push(AppM { name: p.name.trim_end_matches(".exe").to_string(), icon: p.exe.clone(), cpu: p.cpu, mem: p.mem, disk: p.disk, pids: vec![p.pid] }),
                }
            }
            let mine = user.to_lowercase() == me;
            UserM {
                key: user.clone(),
                sub: if mine { t!("Активен · локальная учётная запись", "Active · local account", ja: "アクティブ · ローカル アカウント", pt: "Ativo · conta local", zh: "活动 · 本地帐户").into() } else { tf!("Процессов: {}", "{} processes", ja: "プロセス: {}", pt: "{} processos", zh: "{} 个进程", ps.len()) },
                dot: if mine { UDot::Active } else { UDot::Other },
                procs: ps.len(),
                cpu: ps.iter().map(|p| p.cpu).sum(),
                mem: ps.iter().map(|p| p.mem).sum(),
                disk: ps.iter().map(|p| p.disk).sum(),
                name: user,
                apps,
            }
        })
        .collect();
    out.sort_by(|a, b| (b.dot == UDot::Active).cmp(&(a.dot == UDot::Active)).then(b.mem.cmp(&a.mem)));
    out
}

/// What kind of care a tweak of the Speed page needs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TwKind {
    Plain,
    Admin,
    Reboot,
}

pub struct Tweak {
    pub id: boost::Id,
    pub icon: &'static str,
    pub title: &'static str,
    pub sub: &'static str,
    pub desc: &'static str,
    pub kind: TwKind,
}

pub static TWEAKS: LazyLock<[Tweak; 5]> = LazyLock::new(|| {
    [
        Tweak {
            id: boost::Id::Shell,
            icon: I_TW_SHELL,
            title: t!("Блокировать лишние процессы оболочки", "Block unneeded shell processes", ja: "不要なシェル プロセスをブロック", pt: "Bloquear processos de shell desnecessários", zh: "阻止不需要的外壳进程"),
            sub: t!("Меню «Пуск», виджеты и Phone Link остаются выключенными", "Start menu, widgets and Phone Link stay off", ja: "スタート メニュー、ウィジェット、Phone Link はオフのまま", pt: "Menu Iniciar, widgets e Phone Link ficam desligados", zh: "开始菜单、小组件和 Phone Link 保持关闭"),
            desc: t!("Меню «Пуск» Windows, виджеты и Phone Link не запускаются; их заменяет панель. Уведомления и ввод текста не затрагиваются.", "The Windows Start menu, widgets and Phone Link do not start; the bar replaces them. Notifications and text input are not touched.", ja: "Windows のスタート メニュー、ウィジェット、Phone Link は起動しません。バーが代わりになります。通知とテキスト入力には影響しません。", pt: "O menu Iniciar do Windows, os widgets e o Phone Link não iniciam; a barra os substitui. Notificações e entrada de texto não são afetadas.", zh: "Windows 开始菜单、小组件和 Phone Link 不会启动，由工具栏代替。通知和文本输入不受影响。"),
            kind: TwKind::Plain,
        },
        Tweak {
            id: boost::Id::Updaters,
            icon: I_TW_UPD,
            title: t!("Один обновлятор через winget", "One updater through winget", ja: "winget による単一のアップデーター", pt: "Um único atualizador via winget", zh: "通过 winget 统一更新"),
            sub: t!("Глушит обновляторы программ; winget обновляет всё раз в неделю", "Silences app updaters; winget updates all apps weekly", ja: "アプリのアップデーターを停止し、winget が週 1 回すべて更新します", pt: "Silencia atualizadores de apps; o winget atualiza tudo toda semana", zh: "关闭各应用的更新程序，winget 每周更新全部应用"),
            desc: t!("Отключает службы и задачи обновления Adobe, Mozilla, Dropbox, CCleaner, Opera, Apple, Zoom и OneDrive; winget обновляет все программы раз в неделю. Браузеры продолжают обновляться сами.", "Disables updater services and tasks of Adobe, Mozilla, Dropbox, CCleaner, Opera, Apple, Zoom and OneDrive; winget updates every app once a week. Browsers keep updating themselves.", ja: "Adobe、Mozilla、Dropbox、CCleaner、Opera、Apple、Zoom、OneDrive の更新サービスとタスクを無効にし、winget がすべてのアプリを週 1 回更新します。ブラウザーは自分で更新を続けます。", pt: "Desativa serviços e tarefas de atualização de Adobe, Mozilla, Dropbox, CCleaner, Opera, Apple, Zoom e OneDrive; o winget atualiza todos os apps uma vez por semana. Os navegadores continuam se atualizando sozinhos.", zh: "停用 Adobe、Mozilla、Dropbox、CCleaner、Opera、Apple、Zoom 和 OneDrive 的更新服务与任务，由 winget 每周更新所有应用。浏览器仍会自行更新。"),
            kind: TwKind::Plain,
        },
        Tweak {
            id: boost::Id::Defender,
            icon: I_TW_DEF,
            title: t!("Исключения Defender для сборок и игр", "Defender exclusions for builds and games", ja: "ビルドとゲーム用の Defender 除外", pt: "Exclusões do Defender para builds e jogos", zh: "为编译和游戏设置 Defender 排除项"),
            sub: t!("Не проверяет cargo, npm, pip и библиотеки игр", "Skips scanning cargo, npm, pip and game libraries", ja: "cargo、npm、pip、ゲーム ライブラリをスキャンしません", pt: "Não verifica cargo, npm, pip nem bibliotecas de jogos", zh: "不扫描 cargo、npm、pip 和游戏库"),
            desc: t!("Defender перестаёт проверять кэши cargo, rustup, npm, pip, NuGet, Gradle и библиотеки Steam/Epic, а также cargo.exe и rustc.exe. Сборки и загрузка игр ускоряются, но эти папки не проверяются.", "Defender stops scanning the caches of cargo, rustup, npm, pip, NuGet, Gradle and the Steam/Epic libraries, and cargo.exe and rustc.exe. Builds and game loading get faster, but those folders are not scanned.", ja: "Defender は cargo、rustup、npm、pip、NuGet、Gradle のキャッシュ、Steam/Epic のライブラリ、cargo.exe と rustc.exe をスキャンしなくなります。ビルドとゲームの読み込みは速くなりますが、これらのフォルダーはスキャンされません。", pt: "O Defender deixa de verificar os caches de cargo, rustup, npm, pip, NuGet, Gradle e as bibliotecas Steam/Epic, além de cargo.exe e rustc.exe. Builds e carregamento de jogos ficam mais rápidos, mas essas pastas não são verificadas.", zh: "Defender 不再扫描 cargo、rustup、npm、pip、NuGet、Gradle 的缓存、Steam/Epic 游戏库以及 cargo.exe 和 rustc.exe。编译和游戏加载会更快，但这些文件夹不会被扫描。"),
            kind: TwKind::Admin,
        },
        Tweak {
            id: boost::Id::Ntfs,
            icon: I_TW_NTFS,
            title: t!("Быстрее NTFS", "Faster NTFS", ja: "高速な NTFS", pt: "NTFS mais rápido", zh: "更快的 NTFS"),
            sub: t!("Без записи времени доступа и коротких имён 8.3", "No last-access writes and no 8.3 short names", ja: "最終アクセス時刻の書き込みと 8.3 短い名前を停止", pt: "Sem gravação de último acesso e sem nomes curtos 8.3", zh: "不写入最后访问时间，不生成 8.3 短文件名"),
            desc: t!("NTFS перестаёт записывать время последнего доступа при каждом чтении и создавать короткие имена 8.3. Вступает в силу после перезагрузки.", "NTFS stops writing the last-access time on every read and creating 8.3 short names. Takes effect after a reboot.", ja: "NTFS は読み取りのたびに最終アクセス時刻を書き込むことと、8.3 短い名前の作成をやめます。再起動後に有効になります。", pt: "O NTFS deixa de gravar a hora do último acesso a cada leitura e de criar nomes curtos 8.3. Entra em vigor após reiniciar.", zh: "NTFS 不再在每次读取时写入最后访问时间，也不再创建 8.3 短文件名。重启后生效。"),
            kind: TwKind::Reboot,
        },
        Tweak {
            id: boost::Id::Dns,
            icon: I_TW_ADS,
            title: t!("Блокировка рекламы", "Block ads", ja: "広告をブロック", pt: "Bloquear anúncios", zh: "屏蔽广告"),
            sub: t!("Около 170 рекламных и трекерных доменов через файл hosts", "About 170 ad and tracker domains, via the hosts file", ja: "hosts ファイル経由で約 170 の広告・トラッカー ドメイン", pt: "Cerca de 170 domínios de anúncios e rastreadores, via arquivo hosts", zh: "通过 hosts 文件屏蔽约 170 个广告和跟踪域名"),
            desc: t!("Около 170 рекламных и трекерных доменов никуда не ведут через файл hosts. Домены Microsoft не трогаются.", "About 170 ad and tracker domains lead nowhere through the hosts file. Microsoft domains are left alone.", ja: "約 170 の広告・トラッカー ドメインは hosts ファイルによりどこにも接続されません。Microsoft のドメインはそのままです。", pt: "Cerca de 170 domínios de anúncios e rastreadores não levam a lugar nenhum pelo arquivo hosts. Os domínios da Microsoft não são alterados.", zh: "约 170 个广告和跟踪域名通过 hosts 文件无法访问。Microsoft 域名不受影响。"),
            kind: TwKind::Plain,
        },
    ]
});

/// What a toast, a tip or a menu says of a process: the reasons on one line (the mockup's `title`), without the first line's
/// "Why flagged:" and the rating words.
pub fn tip_line(tip: &str) -> String {
    tip.lines().skip(1).map(|l| l.split_once(" — ").map_or(l, |(_, r)| r)).collect::<Vec<_>>().join(" · ")
}

/// What the memory card shows: bytes in use, in the standby cache, free, and in all.
#[derive(Clone, Copy, Debug)]
pub struct MemView {
    pub total: u64,
    pub used: u64,
    pub standby: u64,
    pub free: u64,
}

impl Tm {
    pub fn mem_view(&self) -> MemView {
        if let Some(dm) = &self.demo {
            return dm.mem;
        }
        let d = &self.d;
        let (total, used) = (d.sys.total_memory(), d.sys.used_memory());
        let standby = (d.sample.mem_standby + d.sample.mem_modified).min(total.saturating_sub(used));
        MemView { total, used, standby, free: total.saturating_sub(used + standby) }
    }

    pub fn proc_count(&self) -> usize {
        self.demo.as_ref().map_or_else(|| self.d.meminfo.processes as usize, |dm| dm.processes)
    }

    fn gb(b: u64) -> String {
        format!("{:.1} GB", b as f64 / 1_073_741_824.0)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// small shapes

/// A pill (`height:22px; padding:0 8px; border-radius:6px; 12px/500`) with an optional icon; its width, drawn at `x`, centred on `cy`.
#[allow(clippy::too_many_arguments)] // the design's pill: where, what, the two colours, the icon, the dot
fn pill(c: &mut Canvas<'_>, x: f32, cy: f32, text: &str, bg: Rgba, fg: Rgba, icon: Option<(&'static str, f32)>, dot: bool) -> f32 {
    let tw = c.measure(text, F12M).ceil();
    let lead = if icon.is_some() { icon.map_or(0.0, |i| i.1) + 5.0 } else if dot { 6.0 + 6.0 } else { 0.0 };
    let w = 16.0 + lead + tw;
    let r = Rect::new(x, cy - 11.0, w, 22.0);
    c.fill(r, 6.0, bg);
    let mut tx = x + 8.0;
    if let Some((d, s)) = icon {
        c.icon(d, (tx + s / 2.0, cy), s, 2.4, fg, 0.0);
        tx += s + 5.0;
    } else if dot {
        c.circle(tx + 3.0, cy, 3.0, fg);
        tx += 12.0;
    }
    c.text(text, Rect::new(tx, r.y, tw + 2.0, r.h), F12M, fg, Align::Start);
    w
}

fn pill_w(c: &mut Canvas<'_>, text: &str, icon: Option<f32>, dot: bool) -> f32 {
    16.0 + icon.map_or(if dot { 12.0 } else { 0.0 }, |s| s + 5.0) + c.measure(text, F12M).ceil()
}

/// The switch of the design: 36 x 20, a knob of 16. `id` is its item.
fn switch(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, x: f32, cy: f32, on: bool) -> Hot {
    let r = Rect::new(x, cy - 10.0, 36.0, 20.0);
    let hot = f.focusable(id, r);
    f.hand();
    let t = f.tween(id, 2, f32::from(on), 180, SPRING);
    c.fill(r, 10.0, ink(0.16).mix(accent(), t.clamp(0.0, 1.0)));
    c.circle(r.x + 10.0 + 16.0 * t, r.cy(), 8.0, Rgba::pure_white(1.0));
    if hot.focus {
        c.ring(r.inset(-3.0), 13.0, 2.0, focus_ring().fade(0.9));
    }
    hot
}

/// A segmented control of the design (`role=radiogroup`): the trough `well`, the chosen segment raised in `ink .1`; item `id + i`.
fn seg(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, right: f32, cy: f32, labels: &[&str], sel: usize) {
    let widths: Vec<f32> = labels.iter().map(|l| c.measure(l, F12M).ceil() + 24.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let r = Rect::new(right - total, cy - 16.0, total, 32.0);
    c.fill(r, 9.0, WELL);
    let mut x = r.x + 2.0;
    for (i, l) in labels.iter().enumerate() {
        let b = Rect::new(x, r.y + 2.0, widths[i], 28.0);
        let hot = f.focusable(id + i as Id, b);
        f.hand();
        let on = f.tween(id + i as Id, 2, f32::from(i == sel), 180, EASE);
        c.fill(b, 7.0, ink(0.10 * on + 0.04 * hot.hover * (1.0 - on)));
        c.text(l, b, F12M, T3.mix(T1, on.max(0.6 * hot.hover)), Align::Center);
        if hot.focus {
            c.ring(b.inset(-2.0), 9.0, 2.0, focus_ring().fade(0.9));
        }
        x += widths[i];
    }
}

/// A button of the design's `.btn.ghost`: `bg` and `fg` at rest, the plate takes more ink under the pointer.
#[allow(clippy::too_many_arguments)] // the design's `.btn.ghost`: where, what, its colours at rest and under the pointer
fn ghost(c: &mut Canvas<'_>, f: &mut Frame<'_>, id: Id, r: Rect, label: &str, bg: Rgba, fg: Rgba, hover_bg: Rgba) -> Hot {
    let hot = f.focusable(id, r);
    f.hand();
    c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
        c.fill(r, 9.0, bg.mix(hover_bg, hot.hover));
        c.text(label, r, F12M, fg, Align::Center);
    });
    if hot.focus {
        c.ring(r.inset(-3.0), 12.0, 2.0, focus_ring().fade(0.9));
    }
    hot
}

/// The tile of an app that has none of its own (or a made-up one): a gradient and a glyph.
fn tile(c: &mut Canvas<'_>, r: Rect, t: Tile) {
    c.gradient_angle(r, (r.w / 4.0).round().max(4.0), 160.0, &[(0.0, rgb(t.from)), (1.0, rgb(t.to))]);
    c.icon(t.glyph, (r.cx(), r.cy()), r.w * 0.6, 2.2, Rgba::pure_white(1.0), 0.0);
}

impl Tm {
    /// The icon of the file at `path` in `r`: a made-up app's tile, the file's own icon, or the system's "application".
    fn app_icon(&self, c: &mut Canvas<'_>, path: Option<&str>, r: Rect) {
        use windows::Win32::UI::WindowsAndMessaging::HICON;
        if let Some(p) = path
            && let Some(t) = crate::demo::tile_of(p)
        {
            return tile(c, r, t);
        }
        let icon = path.and_then(|p| self.d.res.get(p)).map_or(0, |x| x.icon);
        let icon = if icon != 0 { icon } else { self.d.res.generic_icon };
        if icon != 0 {
            c.hicon(HICON(icon as *mut _), r, 1.0);
        } else {
            tile(c, r, T_APP);
        }
    }

    /// Whether the file at `path` has an icon to show (else the page draws a letter tile).
    pub(super) fn has_icon(&self, path: &str) -> bool {
        crate::demo::tile_of(path).is_some() || self.d.res.get(path).is_some_and(|r| r.icon != 0)
    }

    /// The icon for a process named `name` (what the history says is the top consumer).
    fn icon_named(&self, name: &str) -> Option<String> {
        if self.is_demo() {
            return crate::demo::path_named(name);
        }
        let key = name.to_lowercase();
        self.d.procs.iter().find(|p| p.name.to_lowercase().trim_end_matches(".exe") == key.trim_end_matches(".exe")).and_then(|p| p.exe.clone())
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the frame

    pub fn paint(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        let (w, h) = f.size;
        self.size = (w, h);
        self.apply_theme();
        c.fill(Rect::new(0.0, 0.0, w, h), 0.0, WIN);
        self.sidebar(c, f, h);
        let mw = w - SIDE_W;
        self.head(c, f, SIDE_W, mw);
        let body = Rect::new(SIDE_W, HEAD_H, mw, h - HEAD_H);
        match self.page {
            Page::Processes => self.page_processes(c, f, body),
            Page::Performance => self.page_perf(c, f, body),
            Page::Users => self.page_users(c, f, body),
            Page::Boost => self.page_speed(c, f, body),
            Page::Settings => self.page_settings(c, f, body),
            Page::Insights => self.page_insights(c, f, body),
            _ => self.page_table(c, f, body),
        }
        if self.demo.as_ref().is_some_and(|d| d.hook != crate::demo::Hook::None) {
            f.item(ID_DEMO, Rect::new(0.0, 0.0, 1.0, 1.0));
        }
        self.draw_toast(c, f, w, h);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the sidebar

    fn nav_count(&self, p: Page) -> String {
        match p {
            Page::Processes => self.proc_count().to_string(),
            Page::Startup => self.d.startup.iter().filter(|s| s.enabled).count().to_string(),
            Page::Services => self.demo.as_ref().map_or(self.d.svcs.len(), |dm| dm.svc_total).to_string(),
            Page::Rules => self.d.rules.len().to_string(),
            Page::Insights if self.slow.n > 0 => self.slow.n.to_string(),
            _ => String::new(),
        }
    }

    fn sidebar(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, h: f32) {
        c.fill(Rect::new(0.0, 0.0, SIDE_W, h), 0.0, SIDE);
        c.fill(Rect::new(SIDE_W - 1.0, 0.0, 1.0, h), 0.0, ink(0.05));
        // the title: 22 px mark and "Task Manager"
        let mark = Rect::new(20.0, 23.0, 22.0, 22.0);
        tile(c, mark, T_WIN);
        c.icon(NAV_PERF, (mark.cx(), mark.cy()), 14.0, 2.4, Rgba::pure_white(1.0), 0.0);
        c.text(t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器"), Rect::new(52.0, 16.0, 150.0, 36.0), Font::new(14.0, 600), T1, Align::Start);
        let icons = [NAV_PROC, NAV_PERF, NAV_HIST, NAV_START, NAV_USERS, NAV_DET, NAV_SVC, NAV_BOOST, NAV_SLOW, NAV_RULES, NAV_SET];
        for (i, &p) in NAV_PAGES.iter().enumerate() {
            let y = if i < 7 { 66.0 + i as f32 * 38.0 } else { 355.0 + (i - 7) as f32 * 38.0 };
            if i == 7 {
                c.fill(Rect::new(24.0, 342.0, SIDE_W - 49.0, 1.0), 0.0, ink(0.06));
            }
            let r = Rect::new(12.0, y, SIDE_W - 25.0, 36.0);
            let cur = p == self.page;
            let hot = f.focusable(ID_NAV + i as Id, r);
            f.hand();
            let on = f.tween(ID_NAV + i as Id, 2, f32::from(cur), 160, EASE);
            c.fill(r, 10.0, ink(0.05 * hot.hover * (1.0 - on)));
            c.fill(r, 10.0, accent().alpha(0.16 * on));
            let col = T2.mix(T1, on.max(0.0));
            c.icon(icons[i], (r.x + 12.0 + 8.5, r.cy()), 17.0, 1.8, col, 0.0);
            c.text(nav_label(p), Rect::new(r.x + 41.0, r.y, 130.0, r.h), F13M, col, Align::Start);
            let n = self.nav_count(p);
            if !n.is_empty() {
                c.text(&n, Rect::new(r.x, r.y, r.w - 12.0, r.h), F12.tabular(), T3, Align::End);
            }
            if hot.focus {
                c.ring(r.inset(-2.0), 12.0, 2.0, focus_ring().fade(0.9));
            }
        }
        // the card at the bottom: two meters and the counts
        let card = Rect::new(12.0, h - 16.0 - 139.0, SIDE_W - 25.0, 139.0);
        c.fill(card, 12.0, ink(0.04));
        let (cpu, mem) = (self.d.cpu_total, self.d.mem_pct);
        for (k, (label, v)) in [(t!("ЦП", "CPU", ja: "CPU", pt: "CPU", zh: "CPU"), cpu), (t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存"), mem)].into_iter().enumerate() {
            let y = card.y + 12.0 + k as f32 * 36.0;
            c.text(label, Rect::new(card.x + 12.0, y, 100.0, 16.0), F12, T2, Align::Start);
            c.text(&format!("{v:.0}%"), Rect::new(card.x + 12.0, y, card.w - 24.0, 16.0), F12.tabular(), T1, Align::End);
            let bar = Rect::new(card.x + 12.0, y + 22.0, card.w - 24.0, 4.0);
            c.fill(bar, 2.0, ink(0.08));
            c.fill(Rect::new(bar.x, bar.y, bar.w * (v / 100.0).clamp(0.0, 1.0), 4.0), 2.0, accent());
        }
        let sy = card.y + 84.0;
        c.fill(Rect::new(card.x + 12.0, sy, card.w - 24.0, 1.0), 0.0, ink(0.06));
        let threads = self.demo.as_ref().map_or(self.d.meminfo.threads, |dm| dm.threads);
        for (k, (label, v)) in [(t!("Процессы", "Processes", ja: "プロセス", pt: "Processos", zh: "进程"), self.proc_count().to_string()), (t!("Потоки", "Threads", ja: "スレッド", pt: "Threads", zh: "线程"), crate::ui::group_thousands(u64::from(threads)).replace(',', " "))].into_iter().enumerate() {
            let y = sy + 9.0 + k as f32 * 19.0;
            c.text(label, Rect::new(card.x + 12.0, y, 100.0, 15.0), F12, T2, Align::Start);
            c.text(&v, Rect::new(card.x + 12.0, y, card.w - 24.0, 15.0), F12.tabular(), T1, Align::End);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the head: the title, the search, the buttons of the page, the window's buttons

    fn head(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, x0: f32, mw: f32) {
        let title = nav_label(self.page);
        let tfont = Font::new(20.0, 600).tracking(-0.01);
        let tw = c.measure(title, tfont).ceil();
        c.text(title, Rect::new(x0 + 24.0, 0.0, tw + 4.0, HEAD_H), tfont, T1, Align::Start);
        // the window's buttons: 26 round, 8 apart, 12 from the edge
        let right = x0 + mw - 12.0;
        let bx = right - 94.0;
        for (i, (id, d)) in [(WIN_MIN, "M6 12h12"), (WIN_MAX, if f.maximized() { "M8.5 8.5h9v9h-9zM6 15V6h9" } else { "M6.5 6.5h11v11h-11z" }), (WIN_CLOSE, "M7 7l10 10M17 7L7 17")].into_iter().enumerate() {
            let b = Rect::new(bx + i as f32 * 34.0, HEAD_H / 2.0 - 13.0, 26.0, 26.0);
            let hot = f.item(id, b);
            f.hand();
            let glide = f.tween(id, 2, f32::from(f.hover == Some(id)), 280, SPRING);
            let (bg, col) = if id == WIN_CLOSE { (ink(0.07).mix(look::red_soft(0.22), hot.hover), T2.mix(look::RED, hot.hover)) } else { (ink(0.07 + 0.09 * hot.hover), T2.mix(T1, hot.hover)) };
            c.transformed(b.cx(), b.cy(), 1.0 - 0.14 * hot.press, 0.0, 0.0, |c| {
                c.fill(b, 13.0, bg);
                let (dy, k, turn) = match id {
                    WIN_MIN => (2.0 * glide, 1.0, 0.0),
                    WIN_MAX => (0.0, 1.0 + 0.2 * glide, 0.0),
                    _ => (0.0, 1.0, 90.0 * glide),
                };
                c.icon(d, (b.cx(), b.cy() + dy), 12.0 * k, 2.4, col, turn);
            });
            if hot.focus {
                c.ring(b.inset(-3.0), 16.0, 2.0, focus_ring().fade(0.9));
            }
        }
        // the buttons of the page, right to left
        let acts = self.head_actions();
        let compact = mw < 880.0;
        let mut rx = bx - 20.0;
        let mut rects = vec![Rect::default(); acts.len()];
        for (i, (a, label, icon, enabled)) in acts.iter().enumerate().rev() {
            let labeled = !compact && !label.is_empty();
            let w = if labeled { 12.0 + 15.0 + 8.0 + c.measure(label, F13M).ceil() + 12.0 } else { 34.0 };
            let r = Rect::new(rx - w, 15.0, w, 34.0);
            rx -= w + 8.0;
            rects[i] = r;
            let id = ID_ACT + i as Id;
            let danger = *a == HeadAct::Do(Act::EndTask) || *a == HeadAct::Do(Act::SvcStop) || *a == HeadAct::Do(Act::RuleDelete);
            let pressed = *a == HeadAct::Do(Act::Efficiency) && self.selected_row().is_some_and(|r| !r.pids.is_empty() && r.pids.iter().all(|p| self.d.efficiency.contains(p)));
            let hot = f.focusable(id, r);
            f.hand();
            let (bg, hbg, fg) = if pressed {
                (accent().alpha(0.18), accent().alpha(0.26), acc_text())
            } else if danger && *enabled {
                (look::red_soft(0.12), look::red_soft(0.16), look::RED)
            } else {
                (ink(0.06), ink(0.1), T1)
            };
            let op = if *enabled { 1.0 } else { 0.4 };
            c.transformed(r.cx(), r.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                c.faded(op, |c| {
                    c.fill(r, 10.0, bg.mix(hbg, hot.hover));
                    if labeled {
                        c.icon(icon, (r.x + 12.0 + 7.5, r.cy()), 15.0, 2.0, fg, 0.0);
                        c.text(label, Rect::new(r.x + 12.0 + 15.0 + 8.0, r.y, r.w, r.h), F13M, fg, Align::Start);
                    } else {
                        c.icon(icon, (r.cx(), r.cy()), 18.0, 3.0, fg, 0.0);
                    }
                });
            });
            if !label.is_empty() {
                f.tip(*label);
            }
            if hot.focus {
                c.ring(r.inset(-3.0), 13.0, 2.0, focus_ring().fade(0.9));
            }
        }
        self.head_rects = rects;
        // the search box
        if self.page_has_search() {
            let sx = x0 + 24.0 + tw + 16.0;
            let sw = 280.0f32.min((rx - sx - 8.0).max(160.0));
            let r = Rect::new(sx, 15.0, sw, 34.0);
            let ph = match self.page {
                Page::Services => t!("Поиск служб", "Search services", ja: "サービスを検索", pt: "Pesquisar serviços", zh: "搜索服务"),
                Page::Startup => t!("Поиск в автозагрузке", "Search startup apps", ja: "スタートアップ アプリを検索", pt: "Pesquisar apps de inicialização", zh: "搜索启动应用"),
                Page::History => t!("Поиск по приложению или времени", "Search by app or time", ja: "アプリまたは時刻で検索", pt: "Pesquisar por app ou hora", zh: "按应用或时间搜索"),
                Page::Rules => t!("Поиск правил", "Search rules", ja: "ルールを検索", pt: "Pesquisar regras", zh: "搜索规则"),
                Page::Details => t!("Поиск по имени, PID или командной строке", "Search by name, PID or command line", ja: "名前、PID、コマンド ラインで検索", pt: "Pesquisar por nome, PID ou linha de comando", zh: "按名称、PID 或命令行搜索"),
                _ => t!("Введите имя, издателя или PID для поиска", "Search by name, publisher or PID", ja: "名前、発行元、または PID で検索", pt: "Pesquisar por nome, editor ou PID", zh: "按名称、发布者或 PID 搜索"),
            };
            let st = FieldStyle { icon: Some(I_SEARCH), clear: false, pad: 12.0, ..FieldStyle::INPUT };
            field_with(c, f, ID_SEARCH, r, &mut self.search, ph, &st);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Processes

    /// The pill of a process the checks flag: its colours, icon and word.
    fn sus_look(sev: u8) -> (Rgba, Rgba, &'static str) {
        match sev {
            3.. => (look::red_soft(0.16), look::RED, I_ALERT_HIGH),
            2 => (look::amber_soft(0.13), look::AMBER, I_ALERT_MED),
            _ => (ink(0.07), T2, I_ALERT_MED),
        }
    }

    fn page_processes(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, body: Rect) {
        let mut y = body.y;
        // the alert of a process rated High
        if let Some(pid) = self.alert_key() {
            let name = self.d.procs.iter().find(|p| p.pid == pid).map(|p| (p.name.clone(), p.reasons.iter().map(|r| r.text.clone()).collect::<Vec<_>>().join(" · "))).unwrap_or_default();
            let r = Rect::new(body.x + 20.0, y, body.w - 40.0, 48.0);
            c.fill(r, 12.0, look::red_soft(0.09));
            c.ring(r, 12.0, 1.0, look::red_soft(0.22));
            c.icon(I_ALERT_HIGH, (r.x + 14.0 + 9.0, r.cy()), 18.0, 2.0, look::RED, 0.0);
            // 8 px of room on the right, then dismiss (32), End task, Show
            let (end_l, show_l) = (t!("Снять задачу", "End task", ja: "タスクの終了", pt: "Finalizar tarefa", zh: "结束任务"), t!("Показать", "Show", ja: "表示", pt: "Mostrar", zh: "显示"));
            let (ew, sw) = (c.measure(end_l, F13M).ceil() + 24.0, c.measure(show_l, F13M).ceil() + 24.0);
            let dis = Rect::new(r.x + r.w - 8.0 - 32.0, r.cy() - 16.0, 32.0, 32.0);
            let end = Rect::new(dis.x - 12.0 - ew, r.cy() - 16.0, ew, 32.0);
            let show = Rect::new(end.x - 12.0 - sw, r.cy() - 16.0, sw, 32.0);
            let ft = Font::new(13.0, 500);
            let _ = ft;
            ghost(c, f, ID_ALERT_SHOW, show, show_l, ink(0.07), T1, ink(0.11));
            ghost(c, f, ID_ALERT_END, end, end_l, look::red_soft(0.16), look::RED, look::red_soft(0.24));
            let hot = f.item(ID_ALERT_HIDE, dis);
            f.hand();
            f.tip(t!("Скрыть", "Dismiss", ja: "閉じる", pt: "Dispensar", zh: "关闭"));
            c.fill(dis, 9.0, ink(0.08 * hot.hover));
            c.icon(I_DISMISS, (dis.cx(), dis.cy()), 12.0, 2.4, T2.mix(T1, hot.hover), 0.0);
            let room = (show.x - 12.0) - (r.x + 14.0 + 18.0 + 12.0);
            let nm = name.0.clone();
            let nw = c.measure(&nm, Font::new(13.0, 600)).ceil();
            let tx = r.x + 14.0 + 18.0 + 12.0;
            c.text(&nm, Rect::new(tx, r.y, nw.min(room), r.h), Font::new(13.0, 600), T1, Align::Start);
            let why = format!(" · {}", lower_first(&name.1));
            c.text_fit(&why, Rect::new(tx + nw, r.y, (room - nw).max(0.0), r.h), F13, T2, Align::Start);
            y += 48.0 + 8.0;
        }
        let area = Rect::new(body.x, y, body.w, body.y + body.h - y);
        // the grid of the table: name takes what the rest leave
        let fixed = [104.0, 88.0, 104.0, 96.0, 88.0, 64.0, 124.0];
        let gx = area.x + 16.0;
        let gw = area.w - 32.0;
        let flex = (gw - fixed.iter().sum::<f32>()).max(120.0);
        let mut xs = vec![gx];
        xs.push(gx + flex);
        for w in fixed {
            let last = *xs.last().unwrap_or(&gx);
            xs.push(last + w);
        }
        // the header: the totals above the names of the measures
        let head = Rect::new(area.x, area.y, area.w, 60.0);
        c.fill(Rect::new(head.x + 16.0, head.y + head.h - 1.0, head.w - 32.0, 1.0), 0.0, ink(0.06));
        let (sort_col, sort_desc) = self.sort_of(Page::Processes);
        let net_known = self.d.etw.is_some();
        let sus_n = self.d.procs.iter().filter(|p| !p.reasons.is_empty()).count();
        let totals = [
            (2usize, t!("ЦП", "CPU", ja: "CPU", pt: "CPU", zh: "CPU"), format!("{:.0}%", self.d.cpu_total)),
            (3, t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存"), format!("{:.0}%", self.d.mem_pct)),
            (4, t!("Диск", "Disk", ja: "ディスク", pt: "Disco", zh: "磁盘"), format!("{:.0}%", self.d.sample.disk_active)),
            (5, t!("Сеть", "Network", ja: "ネットワーク", pt: "Rede", zh: "网络"), if net_known { format!("{:.0}%", self.d.sample.net_util) } else { "—".into() }),
            (6, "GPU", format!("{:.0}%", self.d.sample.gpu_total)),
            (7, t!("Подозрительное", "Suspicious", ja: "不審", pt: "Suspeito", zh: "可疑"), sus_n.to_string()),
        ];
        // "Name" and "Status": plain labels at the foot
        let nm_r = Rect::new(xs[0], head.y, xs[1] - xs[0], head.h);
        let hot = f.focusable(ID_PHEAD, nm_r);
        f.hand();
        let on = sort_col == 0;
        c.clipped(nm_r, |c| c.fill(Rect::new(nm_r.x, nm_r.y, nm_r.w, nm_r.h + 8.0), 8.0, ink(0.04 * hot.hover)));
        let col = T3.mix(acc_text(), f32::from(on));
        let nlabel = nav_name_col();
        c.text(nlabel, Rect::new(nm_r.x + 12.0, head.y + 35.0, 120.0, 15.0), F12M, col, Align::Start);
        if on {
            let lw = c.measure(nlabel, F12M).ceil();
            arrow(c, nm_r.x + 12.0 + lw + 6.0 + 5.0, head.y + 42.5, col, sort_desc);
        }
        c.text(t!("Состояние", "Status", ja: "状態", pt: "Status", zh: "状态"), Rect::new(xs[1] + 12.0, head.y + 35.0, 90.0, 15.0), F12M, T3, Align::Start);
        for (k, (colno, label, total)) in totals.iter().enumerate() {
            let r = Rect::new(xs[k + 2], head.y, xs[k + 3] - xs[k + 2], head.h);
            let is_net = columns(Page::Processes).get(*colno).and_then(|c| c.metric) == Some(Metric::Net);
            let sortable = !is_net;
            let hot = if sortable { f.focusable(ID_PHEAD + *colno as Id, r) } else { f.item(ID_PHEAD + *colno as Id, r) };
            if sortable {
                f.hand();
            }
            if is_net && !net_known {
                f.tip(t!("Сеть по процессам требует прав администратора (Параметры)", "Per-process network needs administrator rights (Settings)", ja: "プロセスごとのネットワークには管理者権限が必要です (設定)", pt: "A rede por processo exige direitos de administrador (Configurações)", zh: "按进程统计网络需要管理员权限（设置）"));
            }
            let on = sort_col == *colno;
            let t = f.tween(ID_PHEAD + *colno as Id, 3, f32::from(on), 160, EASE);
            c.clipped(r, |c| c.fill(Rect::new(r.x, r.y, r.w, r.h + 8.0), 8.0, accent().alpha(0.08 * t).mix(ink(0.04), 0.0)));
            if sortable && hot.hover > 0.0 && !on {
                c.clipped(r, |c| c.fill(Rect::new(r.x, r.y, r.w, r.h + 8.0), 8.0, ink(0.04 * hot.hover)));
            }
            let col = T3.mix(acc_text(), t);
            let lw = c.measure(label, F12M).ceil();
            let right = r.x + r.w - 12.0;
            c.text(total, Rect::new(r.x, head.y + 13.0, r.w - 12.0, 20.0), Font::new(18.0, 600).tracking(-0.01).tabular(), T1, Align::End);
            let extra = if on { 4.0 + 10.0 } else { 0.0 };
            c.text(label, Rect::new(right - lw - extra, head.y + 35.0, lw + 2.0, 15.0), F12M, col, Align::Start);
            if on {
                arrow(c, right - 5.0, head.y + 42.5, col, sort_desc);
            }
        }
        // the rows
        let list = Rect::new(area.x, area.y + head.h + 1.0, area.w, area.h - head.h - 1.0);
        let n = self.rows.len();
        let mut offs = Vec::with_capacity(n + 1);
        let mut yy = 6.0f32;
        for r in &self.rows {
            offs.push(yy);
            yy += if r.depth > 0 { KID_H } else { ROW_H } + 2.0;
        }
        let content = yy + 4.0;
        if let Some(i) = self.reveal.take()
            && let Some(&top) = offs.get(i)
        {
            let h = if self.rows[i].depth > 0 { KID_H } else { ROW_H };
            self.proc_sc.set_extent(content, list.h);
            self.proc_sc.ensure_visible(top - 6.0, h + 12.0, f.now);
        }
        let mut sc = std::mem::take(&mut self.proc_sc);
        let sel = self.sel.clone();
        let (xs2, offs2) = (xs.clone(), offs);
        scroll(c, f, ID_PROC, list, &mut sc, content, |c, f, v| {
            let first = offs2.partition_point(|&o| o + ROW_H < v.y).min(n);
            for i in first..n {
                let row = &self.rows[i];
                let hgt = if row.depth > 0 { KID_H } else { ROW_H };
                let top = v.at(offs2[i]);
                if top > list.y + list.h {
                    break;
                }
                let rr = Rect::new(list.x + 10.0, top, list.w - 20.0, hgt);
                self.draw_proc_row(c, f, i, row, rr, &xs2, sel.as_deref() == Some(row.key.as_str()));
            }
            if n == 0 {
                let msg = if self.search.text.trim().is_empty() { String::new() } else { tf!("Нет процессов по запросу «{}»", "No processes match “{}”", ja: "「{}」に一致するプロセスはありません", pt: "Nenhum processo corresponde a “{}”", zh: "没有与“{}”匹配的进程", self.search.text) };
                c.text(&msg, Rect::new(list.x, list.y + 6.0, list.w, 120.0), F13, T3, Align::Center);
            }
        });
        self.proc_sc = sc;
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_proc_row(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>, i: usize, row: &Row, rr: Rect, xs: &[f32], sel: bool) {
        let kid = row.depth > 0;
        let id = ID_PROW + i as Id;
        let hot = f.item(id, rr);
        f.hand();
        let t = f.tween(id, 6, f32::from(sel), 140, EASE);
        c.fill(rr, 10.0, ink(0.04 * hot.hover * (1.0 - t)));
        if t > 0.0 {
            c.fill(rr, 10.0, accent().alpha(0.12 * t));
            c.ring(rr, 10.0, 1.0, accent().alpha(0.45 * t));
        }
        let cy = rr.cy();
        // the name: [chevron] [icon] name
        let cell_x = xs[0];
        let ind = if kid { 34.0 } else { 6.0 };
        let chev = Rect::new(cell_x + ind - 4.0, cy - 11.0, 22.0, 22.0);
        if row.expandable && !kid {
            let ch = f.item(ID_CHEV + i as Id, chev);
            f.hand();
            c.fill(chev, 6.0, ink(0.1 * ch.hover));
            let turn = f.tween(ID_CHEV + i as Id, 2, f32::from(row.expanded), 250, SPRING);
            c.icon(I_CHEV, (chev.cx(), chev.cy()), 12.0, 2.6, T2, 90.0 * turn);
        }
        let isz = if kid { 16.0 } else { 20.0 };
        let ix = chev.x + 26.0;
        self.app_icon(c, row.icon_path.as_deref(), Rect::new(ix, cy - isz / 2.0, isz, isz));
        let nx = ix + isz + 10.0;
        let nw = (xs[1] - nx - 8.0).max(0.0);
        c.text_fit(&row.cells[0], Rect::new(nx, rr.y, nw, rr.h), if kid { F13 } else { F13M }, if kid { T2 } else { T1 }, Align::Start);
        // the status
        let mut px = xs[1] + 12.0;
        if row.pids.iter().all(|p| self.d.suspended.contains(p)) && !row.pids.is_empty() {
            px += pill(c, px, cy, t!("Приостановлен", "Suspended", ja: "中断中", pt: "Suspenso", zh: "已挂起"), ink(0.07), T2, Some((I_PAUSE, 10.0)), false) + 4.0;
        }
        if !kid && !row.pids.is_empty() && row.pids.iter().all(|p| self.d.efficiency.contains(p)) {
            pill(c, px, cy, t!("Эффективность", "Efficiency", ja: "効率", pt: "Eficiência", zh: "能效"), look::green_soft(), look::GREEN, Some((I_LEAF, 11.0)), false);
        }
        // the measures: cells of 32 with the heat of the value behind them
        let caps = [(15.0, 36.0), (0.0, 30.0), (0.0, 20.0), (0.0, 0.0), (0.0, 24.0)];
        let _ = caps;
        for (k, col) in (2..=6).enumerate() {
            let cell = Rect::new(xs[k + 2] + 2.0, cy - 16.0, xs[k + 3] - xs[k + 2] - 4.0, 32.0);
            let (text, dim) = (&row.cells[col], row.cells[col] == "—");
            let tint = match (col, row.heat.get(col).copied().flatten()) {
                (2, Some((v, _))) => heat(v, 15.0, 36.0),
                (3, Some((v, s))) => heat(v, s, 30.0),
                (4, Some((v, s))) => heat(v, s, 20.0),
                (6, Some((v, _))) => heat(v, 10.0, 24.0),
                _ => None,
            };
            if let Some(tc) = tint {
                c.fill(cell, 7.0, tc);
            }
            let zero = row.heat.get(col).copied().flatten().is_some_and(|(v, _)| v <= 0.0);
            let col_c = if dim { T4 } else if zero && col != 3 { T3 } else { T1 };
            c.text(text, Rect::new(cell.x, cell.y, cell.w - 10.0, cell.h), num(), col_c, Align::End);
        }
        // the suspicious column
        let sr = Rect::new(xs[7], rr.y, xs[8] - xs[7], rr.h);
        if row.sev > 0 && !kid {
            let (bg, fg, icon) = Self::sus_look(row.sev);
            let label = suspect_label(row.sev);
            let w = pill_w(c, label, Some(11.0), false);
            let hit = Rect::new(sr.x + sr.w - 8.0 - w, cy - 11.0, w, 22.0);
            f.item(ID_PROW + 20_000 + i as Id, hit);
            f.tip(tip_line(&row.tip));
            pill(c, hit.x, cy, label, bg, fg, Some((icon, 11.0)), false);
        } else {
            c.text("—", Rect::new(sr.x, sr.y, sr.w - 8.0, sr.h), F13, T4, Align::End);
        }
        if hot.focus {
            c.ring(rr, 10.0, 1.5, accent().alpha(0.7));
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Performance

    fn perf_color(kind: PerfKind) -> Rgba {
        match kind {
            PerfKind::Cpu => accent(),
            PerfKind::Mem => MEM_C,
            PerfKind::Disk(_) => DISK_C,
            PerfKind::Nic(_) => NET_C,
            PerfKind::Gpu(_) => GPU_C,
        }
    }

    /// Points of a chart in `r`: the newest at the right edge, 60 s across; `ymax` is the top.
    fn chart_points(q: &std::collections::VecDeque<f32>, r: Rect, ymax: f32, pad: f32) -> Vec<(f32, f32)> {
        let n = q.len();
        let step = r.w / (CAP as f32 - 1.0);
        q.iter()
            .enumerate()
            .map(|(i, &v)| {
                let y = r.y + r.h - (v / ymax.max(0.001) * r.h).clamp(pad, r.h - pad);
                (r.x + r.w - (n - 1 - i) as f32 * step, y)
            })
            .collect()
    }

    fn page_perf(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, body: Rect) {
        let items = self.perf_list();
        if items.is_empty() {
            return;
        }
        let cur = items.iter().position(|i| i.kind == self.perf_sel).unwrap_or(0);
        let inner = Rect::new(body.x + 20.0, body.y + 4.0, body.w - 40.0, body.h - 4.0 - 20.0);
        // the list: 240 wide, a tab of 68 for each
        let n = items.len() as f32;
        let tab_h = 68.0f32.min((inner.h - 6.0 * (n - 1.0)) / n);
        for (k, it) in items.iter().enumerate() {
            let col = Self::perf_color(it.kind);
            let r = Rect::new(inner.x, inner.y + k as f32 * (tab_h + 6.0), 240.0, tab_h);
            let on = k == cur;
            let hot = f.focusable(ID_PERF + k as Id, r);
            f.hand();
            let t = f.tween(ID_PERF + k as Id, 2, f32::from(on), 160, EASE);
            c.fill(r, 12.0, ink(0.03 + 0.03 * hot.hover * (1.0 - t)).mix(col.alpha(0.12), t));
            if t > 0.0 {
                c.ring(r, 12.0, 1.0, col.alpha(0.5 * t));
            }
            let sp = Rect::new(r.x + 12.0, r.y + (r.h - 36.0) / 2.0, 64.0, 36.0);
            c.fill(sp, 6.0, ink(0.04));
            if let Some(g) = it.graphs.first()
                && let Some((q, _)) = g.series.first()
            {
                let pts = Self::chart_points(q, sp, g.ymax, 2.0);
                c.clipped(sp, |c| c.polyline(&pts, 1.6, col));
            }
            let tx = sp.x + sp.w + 12.0;
            c.text(&it.title, Rect::new(tx, r.cy() - 17.0, r.x + r.w - tx - 8.0, 18.0), F13M, T1, Align::Start);
            let sub = if it.line2.is_empty() { it.line1.replace("  ", " · ") } else { format!("{} · {}", it.line1.replace("  ", " · "), it.line2) };
            c.text_fit(&sub, Rect::new(tx, r.cy() + 1.0, r.x + r.w - tx - 8.0, 17.0), F12.tabular(), T3, Align::Start);
        }
        // the card
        let card = Rect::new(inner.x + 252.0, inner.y, inner.w - 252.0, inner.h);
        let it = &items[cur];
        let col = Self::perf_color(it.kind);
        c.fill(card, 16.0, CARD);
        c.ring(card, 16.0, 1.0, ink(0.05));
        let (px, py) = (card.x + 22.0, card.y + 20.0);
        c.text(&it.big_title, Rect::new(px, py + 5.0, card.w - 200.0, 24.0), Font::new(20.0, 600).tracking(-0.01), T1, Align::Start);
        c.text_fit(&it.device, Rect::new(px, py + 33.0, card.w - 240.0, 16.0), F13, T3, Align::Start);
        let big = match it.graphs.first().and_then(|g| g.series.first()).and_then(|s| s.0.back().copied()) {
            Some(v) if matches!(it.kind, PerfKind::Nic(_)) => format!("{:.1}", v / 1000.0),
            Some(v) => format!("{v:.0}%"),
            None => String::new(),
        };
        c.text(&big, Rect::new(px, py, card.w - 44.0, 49.0), Font::new(40.0, 600).tracking(-0.03).tabular(), col, Align::End);
        // the tiles under the chart: 4 to a line, 62 high
        let tiles: Vec<(String, String)> = it.stats.iter().chain(it.facts.iter()).take(12).map(|(k, v)| (k.trim_end_matches(':').to_string(), v.clone())).collect();
        let lines = tiles.len().div_ceil(4).max(1) as f32;
        let tiles_h = lines * 62.0 + (lines - 1.0) * 8.0;
        let chart_y = py + 49.0 + 16.0;
        let chart_h = (card.y + card.h - 20.0 - tiles_h - 16.0 - chart_y).max(120.0);
        let chart = Rect::new(px, chart_y, card.w - 44.0, chart_h);
        c.fill(chart, 12.0, WELL);
        f.item(ID_CHART, chart);
        if let Some(g) = it.graphs.first() {
            f.tip(g.label.clone());
        }
        c.clipped(chart, |c| {
            for k in 0..4 {
                c.fill(Rect::new(chart.x, chart.y + chart.h - 1.0 - k as f32 * chart.h / 4.0, chart.w, 1.0), 0.0, ink(0.04));
            }
            for k in 0..10 {
                c.fill(Rect::new(chart.x + k as f32 * chart.w / 10.0, chart.y, 1.0, chart.h), 0.0, ink(0.04));
            }
            if let Some(g) = it.graphs.first() {
                for (si, (q, _)) in g.series.iter().enumerate().take(2) {
                    let pts = Self::chart_points(q, chart, g.ymax, 2.0);
                    if pts.len() >= 2 {
                        if si == 0 {
                            c.area(&pts, chart.y + chart.h, col.alpha(0.18), col.alpha(0.18));
                        }
                        c.polyline(&pts, 2.0, if si == 0 { col } else { col.alpha(0.55) });
                    }
                }
            }
        });
        c.text(&it.graphs.first().map_or(String::new(), |g| if g.right.is_empty() { "100%".into() } else { g.right.clone() }), Rect::new(chart.x + 10.0, chart.y + 8.0, 140.0, 16.0), F12.tabular(), T3, Align::Start);
        c.text(t!("60 с назад", "60 s ago", ja: "60 秒前", pt: "há 60 s", zh: "60 秒前"), Rect::new(chart.x + 10.0, chart.y + chart.h - 24.0, 140.0, 16.0), F12.tabular(), T3, Align::Start);
        c.text(t!("сейчас", "now", ja: "現在", pt: "agora", zh: "现在"), Rect::new(chart.x, chart.y + chart.h - 24.0, chart.w - 10.0, 16.0), F12.tabular(), T3, Align::End);
        let tw = (chart.w - 3.0 * 8.0) / 4.0;
        for (k, (label, v)) in tiles.iter().enumerate() {
            let (col_i, row_i) = ((k % 4) as f32, (k / 4) as f32);
            let r = Rect::new(chart.x + col_i * (tw + 8.0), chart.y + chart.h + 16.0 + row_i * 70.0, tw, 62.0);
            c.fill(r, 12.0, ink(0.04));
            c.text_fit(label, Rect::new(r.x + 14.0, r.y + 12.0, r.w - 28.0, 15.0), F12, T3, Align::Start);
            c.text_fit(v, Rect::new(r.x + 14.0, r.y + 31.0, r.w - 28.0, 19.0), Font::new(15.0, 600).tabular(), T1, Align::Start);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the generic tables

    fn page_table(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, body: Rect) {
        let note = match self.page {
            Page::History => t!("Сегодня · одна строка в минуту, в том числе когда диспетчер в трее", "Today · one row a minute, also while Task Manager sits in the tray", ja: "今日 · 1 分に 1 行。タスク マネージャーがトレイにある間も記録されます", pt: "Hoje · uma linha por minuto, também com o Gerenciador na bandeja", zh: "今天 · 每分钟一行，任务管理器在托盘中时也会记录"),
            Page::Startup => t!("Пожалуйста, отключите то, что не нужно сразу после входа", "Turn off what you don’t need right after sign-in", ja: "サインイン直後に不要なものはオフにしましょう", pt: "Desative o que você não precisa logo após entrar", zh: "关闭登录后不需要的项目"),
            Page::Rules => t!("Правила работают, пока диспетчер в трее. Добавьте правило из контекстного меню процесса.", "Rules keep working while Task Manager is in the tray. Add one from a process’s right-click menu.", ja: "ルールはタスク マネージャーがトレイにある間も動作します。プロセスの右クリック メニューから追加できます。", pt: "As regras continuam funcionando com o Gerenciador na bandeja. Adicione uma pelo menu de contexto de um processo.", zh: "任务管理器在托盘中时规则仍会生效。请从进程的右键菜单添加。"),
            _ => "",
        };
        let mut y = body.y;
        if !note.is_empty() {
            c.text_fit(note, Rect::new(body.x + 24.0, y, body.w - 48.0, 18.0), F13, T3, Align::Start);
            y += 18.0 + 8.0;
        }
        let area = Rect::new(body.x, y, body.w, body.y + body.h - y);
        let page = self.page;
        self.table_cols(page);
        let head = Rect::new(area.x, area.y, area.w, 36.0);
        header(c, f, ID_COLS, head, &self.cols, 22.0);
        let list = Rect::new(area.x, area.y + 37.0, area.w, area.h - 37.0);
        let lay = self.cols.layout(area.w - 44.0);
        let n = self.rows.len();
        let look = ListLook { gap: 2.0, radius: 10.0, inset: 10.0, top: 6.0, bottom: 6.0 };
        let mut st = std::mem::take(&mut self.list);
        let sel = self.sel.clone();
        list_with(c, f, ID_TABLE, list, &mut st, n, 40.0, &look, |c, f, i, rr, _rs| {
            let row = &self.rows[i];
            let selected = sel.as_deref() == Some(row.key.as_str());
            let t = f.tween(ID_TABLE + 2 + i as Id, 7, f32::from(selected), 140, EASE);
            if t > 0.0 {
                c.fill(rr, 10.0, accent().alpha(0.12 * t));
                c.ring(rr, 10.0, 1.0, accent().alpha(0.45 * t));
            }
            self.draw_table_row(c, f, i, row, rr, &lay);
        });
        self.list = st;
        if n == 0 {
            let msg = match page {
                Page::History if self.d.hist.is_empty() && self.search.text.is_empty() => t!("История использования пуста", "Usage history is empty", ja: "使用履歴は空です", pt: "O histórico de uso está vazio", zh: "使用历史记录为空").to_string(),
                Page::Rules if self.d.rules.is_empty() && self.search.text.is_empty() => t!("Правил пока нет. Добавьте правило из контекстного меню процесса.", "No rules yet. Add one from a process’s right-click menu.", ja: "ルールはまだありません。プロセスの右クリック メニューから追加できます。", pt: "Nenhuma regra ainda. Adicione uma pelo menu de contexto de um processo.", zh: "还没有规则。请从进程的右键菜单添加。").to_string(),
                _ => tf!("Ничего не найдено по запросу «{}»", "Nothing matches “{}”", ja: "「{}」に一致するものはありません", pt: "Nada corresponde a “{}”", zh: "没有与“{}”匹配的内容", self.search.text),
            };
            c.text(&msg, Rect::new(list.x, list.y + 6.0, list.w, 120.0), F13, T3, Align::Center);
        }
    }

    fn draw_table_row(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>, i: usize, row: &Row, rr: Rect, lay: &[(f32, f32)]) {
        let spec = table_spec(self.page);
        let cy = rr.cy();
        for (k, ((x, w), &(mi, _, _))) in lay.iter().zip(spec.iter()).enumerate() {
            let cell = Rect::new(rr.x + 12.0 + x, rr.y, *w, rr.h);
            let inner = Rect::new(cell.x + 8.0, cell.y, (cell.w - 16.0).max(0.0), cell.h);
            if mi == SWITCH_COL {
                let on = self.d.startup.iter().find(|s| s.key() == row.key).is_some_and(|s| s.enabled);
                switch(c, f, ID_SWITCH + i as Id, cell.x + cell.w - 8.0 - 36.0, cy, on);
                continue;
            }
            let text = row.cells.get(mi).map_or("", String::as_str);
            let right = self.cols.cols.get(k).is_some_and(|c| c.align_right);
            match (self.page, mi) {
                (Page::Details, 0) | (Page::Startup, 0) => {
                    let ic = self.app_icon_rect(inner, cy);
                    self.app_icon(c, row.icon_path.as_deref(), ic);
                    c.text_fit(text, Rect::new(inner.x + 30.0, inner.y, (inner.w - 30.0).max(0.0), inner.h), F13M, T1, Align::Start);
                }
                (Page::History, 5) => {
                    let ic = self.app_icon_rect(inner, cy);
                    let path = self.icon_named(text);
                    self.app_icon(c, path.as_deref(), ic);
                    c.text_fit(text, Rect::new(inner.x + 30.0, inner.y, (inner.w - 30.0).max(0.0), inner.h), F13M, T1, Align::Start);
                }
                (Page::History, 1) | (Page::History, 2) => {
                    // a bar and the figure: 4 px, the share filled
                    let v: f32 = text.trim().parse().unwrap_or(0.0);
                    let bar_c = if mi == 1 { if v > 40.0 { look::NET_C } else { accent() } } else { look::MEM_C };
                    let tw = 56.0;
                    let bar = Rect::new(inner.x, cy - 2.0, (inner.w - tw - 8.0).max(0.0), 4.0);
                    c.fill(bar, 2.0, ink(0.08));
                    c.fill(Rect::new(bar.x, bar.y, bar.w * (v / 100.0).clamp(0.0, 1.0), 4.0), 2.0, bar_c);
                    c.text(&format!("{text}%"), Rect::new(inner.x + inner.w - tw, rr.y, tw, rr.h), num(), T1, Align::End);
                }
                (Page::History, 0) => {
                    c.text_fit(&hist_time(text), Rect::new(inner.x, inner.y, inner.w, inner.h), F12.tabular(), T1, Align::Start);
                }
                (Page::History, 3) | (Page::History, 4) => {
                    c.text_fit(text, inner, num(), T1, Align::End);
                }
                (Page::Startup, 4) | (Page::Details, 6) => {
                    c.text_fit(text, inner, F12.tabular(), T3, Align::Start);
                }
                (Page::Startup, 3) => {
                    // the cost of the entry: amber from 1.5 s on; "Off" for a disabled one
                    let on = self.d.startup.iter().find(|s| s.key() == row.key).is_some_and(|s| s.enabled);
                    let secs: f32 = text.trim_start_matches('+').split_whitespace().next().unwrap_or("").replace(',', ".").parse().unwrap_or(0.0);
                    let (txt, colr) = if !on { (t!("Выкл.", "Off", ja: "オフ", pt: "Desl.", zh: "关"), T3) } else if secs >= 1.5 { (text, look::AMBER) } else { (text, if secs < 1.0 { T3 } else { T1 }) };
                    c.text_fit(txt, inner, if colr == look::AMBER { F13M.tabular() } else { num() }, colr, Align::End);
                }
                (Page::Details, 1) | (Page::Services, 1) => {
                    c.text_fit(text, inner, F12.tabular(), T2, Align::End);
                }
                (Page::Details, 2) | (Page::Services, 3) => {
                    let on = text == t!("Выполняется", "Running", ja: "実行中", pt: "Em execução", zh: "正在运行");
                    let (bg, fg) = if on { (look::green_soft(), look::GREEN) } else { (ink(0.06), T3) };
                    if !text.is_empty() {
                        pill(c, inner.x, cy, text, bg, fg, None, true);
                    }
                }
                (Page::Rules, 2) => {
                    let lower = text == t!("Понизить приоритет", "Lower priority", ja: "優先度を下げる", pt: "Reduzir a prioridade", zh: "降低优先级");
                    pill(c, inner.x, cy, text, if lower { look::green_soft() } else { ink(0.06) }, if lower { look::GREEN } else { T3 }, None, true);
                }
                (Page::Rules, 0) | (Page::Services, 0) => {
                    c.text_fit(text, inner, F12.tabular(), T1, Align::Start);
                }
                (Page::Rules, 3) => {
                    c.text_fit(text, inner, num(), if text == "0" { T3 } else { T1 }, Align::End);
                }
                (Page::Details, _) => {
                    let dim = text.starts_with("0 ") || text == "0%" || text == "0,0%";
                    c.text_fit(text, inner, num(), if dim { T3 } else { T1 }, Align::End);
                }
                (Page::Startup, 1) => c.text_fit(text, inner, F13, T2, Align::Start),
                _ => c.text_fit(text, inner, F13, if self.page == Page::Services && mi == 2 { T1 } else { T2 }, if right { Align::End } else { Align::Start }),
            }
        }
    }

    fn app_icon_rect(&self, inner: Rect, cy: f32) -> Rect {
        Rect::new(inner.x, cy - 10.0, 20.0, 20.0)
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Users

    fn page_users(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, body: Rect) {
        let users = self.users();
        let inner = Rect::new(body.x + 20.0, body.y + 4.0, body.w - 40.0, body.h - 4.0 - 20.0);
        // the four tiles
        let d = &self.d;
        let (cpu_all, mem_all) = (users.iter().map(|u| u.cpu).sum::<f32>(), users.iter().map(|u| u.mem).sum::<u64>());
        let total_mem = self.mem_view().total.max(1);
        let labels = [t!("ЦП", "CPU", ja: "CPU", pt: "CPU", zh: "CPU"), t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存"), t!("Диск", "Disk", ja: "ディスク", pt: "Disco", zh: "磁盘"), t!("Сеть", "Network", ja: "ネットワーク", pt: "Rede", zh: "网络")];
        let values: Vec<(String, String, f32)> = match &self.demo {
            Some(dm) if dm.tiles.len() == 4 => dm.tiles.clone(),
            _ => vec![
                (format!("{:.0}%", cpu_all.max(d.cpu_total)), t!("все пользователи", "all users", ja: "全ユーザー", pt: "todos os usuários", zh: "所有用户").to_string(), (cpu_all.max(d.cpu_total) / 100.0).clamp(0.0, 1.0)),
                (Self::gb(mem_all.max((d.mem_pct as f64 / 100.0 * total_mem as f64) as u64)), tf!("из {}", "of {}", ja: "/ {}", pt: "de {}", zh: "共 {}", Self::gb(total_mem).replace(".0 GB", " GB")), (mem_all as f32 / total_mem as f32).clamp(0.0, 1.0)),
                (format!("{:.1} MB/s", users.iter().map(|u| u.disk).sum::<f64>() / 1_048_576.0), "C:".to_string(), (d.sample.disk_active / 100.0).clamp(0.0, 1.0)),
                (format!("{:.1} Mbps", d.sample.nics.first().map_or(0.0, |n| (n.rx + n.tx) * 8.0 / 1e6)), "Wi-Fi".to_string(), (d.sample.net_util / 100.0).clamp(0.0, 1.0)),
            ],
        };
        let tw = (inner.w - 3.0 * 8.0) / 4.0;
        for (k, (v, sub, pct)) in values.iter().enumerate() {
            let label = labels[k];
            let r = Rect::new(inner.x + k as f32 * (tw + 8.0), inner.y, tw, 93.0);
            c.fill(r, 14.0, CARD);
            c.ring(r, 14.0, 1.0, ink(0.05));
            c.text(label, Rect::new(r.x + 16.0, r.y + 14.0, tw - 32.0, 16.0), F12M, T3, Align::Start);
            c.text(sub, Rect::new(r.x + 16.0, r.y + 14.0, tw - 32.0, 16.0), F12, T3, Align::End);
            c.text(v, Rect::new(r.x + 16.0, r.y + 40.0, tw - 32.0, 26.0), Font::new(22.0, 600).tracking(-0.01).tabular(), T1, Align::Start);
            let bar = Rect::new(r.x + 16.0, r.y + r.h - 18.0, tw - 32.0, 4.0);
            c.fill(bar, 2.0, ink(0.07));
            c.fill(Rect::new(bar.x, bar.y, bar.w * pct, 4.0), 2.0, accent());
        }
        // the card with the users
        let avail = inner.h - 93.0 - 12.0;
        let open = self.u_open.clone();
        let mut ys = 6.0f32;
        let mut layout = Vec::new();
        for u in &users {
            let is_open = open.contains(&u.key);
            let apps_h = if is_open { if u.apps.is_empty() { 2.0 + 2.0 + 44.0 + 8.0 } else { 2.0 + 2.0 + u.apps.len() as f32 * 40.0 + 6.0 + 30.0 + 8.0 } } else { 0.0 };
            layout.push((ys, 56.0 + apps_h));
            ys += 56.0 + apps_h + 2.0;
        }
        let content = ys + 4.0;
        let card = Rect::new(inner.x, inner.y + 93.0 + 12.0, inner.w, (52.0 + 34.0 + content).min(avail));
        c.fill(card, 16.0, CARD);
        c.ring(card, 16.0, 1.0, ink(0.05));
        c.text(t!("Вошедшие пользователи", "Signed-in users", ja: "サインイン中のユーザー", pt: "Usuários conectados", zh: "已登录用户"), Rect::new(card.x + 18.0, card.y, 220.0, 52.0), Font::new(13.0, 600), T1, Align::Start);
        let hw = c.measure(t!("Вошедшие пользователи", "Signed-in users", ja: "サインイン中のユーザー", pt: "Usuários conectados", zh: "已登录用户"), Font::new(13.0, 600)).ceil();
        c.text(&users.len().to_string(), Rect::new(card.x + 18.0 + hw + 12.0, card.y, 60.0, 52.0), F13, T3, Align::Start);
        let metrics = [t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存"), t!("ЦП", "CPU", ja: "CPU", pt: "CPU", zh: "CPU"), t!("Диск", "Disk", ja: "ディスク", pt: "Disco", zh: "磁盘")];
        let sel = match self.u_metric {
            UMetric::Mem => 0,
            UMetric::Cpu => 1,
            UMetric::Disk => 2,
        };
        seg(c, f, ID_UMET, card.x + card.w - 12.0, card.y + 26.0, &metrics, sel);
        let sw: f32 = metrics.iter().map(|l| c.measure(l, F12M).ceil() + 24.0).sum::<f32>() + 4.0;
        c.text(t!("Сортировать приложения", "Sort apps by", ja: "アプリの並べ替え", pt: "Ordenar apps por", zh: "应用排序"), Rect::new(card.x, card.y, card.w - 12.0 - sw - 12.0, 52.0), F12, T3, Align::End);
        // the column heads
        let hy = card.y + 52.0;
        c.fill(Rect::new(card.x, hy, card.w, 1.0), 0.0, ink(0.05));
        c.fill(Rect::new(card.x, hy + 33.0, card.w, 1.0), 0.0, ink(0.05));
        let colw = [96.0, 96.0, 104.0, 96.0, 32.0];
        let gx = card.x + 18.0;
        let gw = card.w - 36.0;
        let mut xs = vec![gx, gx + gw - colw.iter().sum::<f32>()];
        for w in colw {
            let last = *xs.last().unwrap_or(&gx);
            xs.push(last + w);
        }
        for (k, label) in [t!("Пользователь", "User", ja: "ユーザー", pt: "Usuário", zh: "用户"), t!("Процессов", "Processes", ja: "プロセス", pt: "Processos", zh: "进程"), t!("ЦП", "CPU", ja: "CPU", pt: "CPU", zh: "CPU"), t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存"), t!("Диск", "Disk", ja: "ディスク", pt: "Disco", zh: "磁盘")].into_iter().enumerate() {
            let r = Rect::new(xs[k], hy + 1.0, xs[k + 1] - xs[k], 32.0);
            c.text(label, if k == 0 { r } else { Rect::new(r.x, r.y, r.w, r.h) }, F12M, T3, if k == 0 { Align::Start } else { Align::End });
        }
        // the rows (and what they hold) scroll
        let lst = Rect::new(card.x, hy + 34.0, card.w, card.h - 52.0 - 34.0);
        let metric = self.u_metric;
        let mut sc = std::mem::take(&mut self.u_sc);
        let mut end_ix = 0;
        scroll(c, f, ID_SCROLL_USERS, lst, &mut sc, content, |c, f, v| {
            for (k, u) in users.iter().enumerate() {
                let (top, h) = layout[k];
                let is_open = open.contains(&u.key);
                let row = Rect::new(lst.x + 6.0, v.at(top), lst.w - 12.0, 56.0);
                if row.y > lst.y + lst.h || row.y + h < lst.y {
                    if is_open {
                        end_ix += u.apps.len();
                    }
                    continue;
                }
                let hot = f.focusable(ID_USER + k as Id, row);
                f.hand();
                let t = f.tween(ID_USER + k as Id, 2, f32::from(is_open), 200, EASE);
                c.fill(row, 12.0, ink(0.04 * t.max(hot.hover * 0.75)));
                let cy = row.cy();
                let av = Rect::new(row.x + 12.0, cy - 18.0, 36.0, 36.0);
                let first = k == 0;
                c.circle(av.cx(), av.cy(), 18.0, if first { accent().alpha(0.18) } else { ink(0.08) });
                let ini: String = u.name.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default();
                c.text(&ini, av, F13.tracking(0.0), if first { acc_text() } else { T2 }, Align::Center);
                let dot = match u.dot {
                    UDot::Active => look::DOT_OK,
                    UDot::Locked => look::DOT_WARN,
                    UDot::Other => T3.mix(T3, 0.0),
                };
                c.circle(av.x + 35.0, av.y + 35.0, 6.0, CARD);
                c.circle(av.x + 35.0, av.y + 35.0, 4.0, dot);
                c.text(&u.name, Rect::new(av.x + 48.0, cy - 19.0, 260.0, 18.0), Font::new(13.0, 600), T1, Align::Start);
                c.text_fit(&u.sub, Rect::new(av.x + 48.0, cy + 1.0, 360.0, 16.0), F12, T3, Align::Start);
                let nc = if u.apps.is_empty() { T3 } else { T1 };
                let cols = [(xs[1], xs[2], u.procs.to_string()), (xs[2], xs[3], format!("{:.1}%", u.cpu)), (xs[3], xs[4], fmt_mem_gb(u.mem)), (xs[4], xs[5], format!("{:.1} MB/s", u.disk / 1_048_576.0))];
                for (a, b, txt) in cols {
                    c.text(&txt, Rect::new(a, row.y, b - a, row.h), num(), nc, Align::End);
                }
                c.icon(I_CHEV_DOWN, (xs[5] + 32.0 - 7.0, cy), 14.0, 2.4, T3, 180.0 * t);
                if is_open && t > 0.0 {
                    let mut ay = row.y + 56.0 + 4.0;
                    let ax = row.x + 60.0;
                    c.clipped(Rect::new(row.x, row.y + 56.0, row.w, h - 56.0), |c| {
                        c.faded(t, |c| {
                            let key = |a: &AppM| match metric {
                                UMetric::Mem => a.mem as f64,
                                UMetric::Cpu => f64::from(a.cpu),
                                UMetric::Disk => a.disk,
                            };
                            let mut apps: Vec<&AppM> = u.apps.iter().collect();
                            apps.sort_by(|a, b| key(b).total_cmp(&key(a)));
                            let (mx, tot) = (apps.iter().map(|a| key(a)).fold(0.0001, f64::max), apps.iter().map(|a| key(a)).sum::<f64>());
                            for (ai, a) in apps.iter().enumerate() {
                                let ar = Rect::new(ax, ay, row.x + row.w - ax, 38.0);
                                let ah = f.item(ID_UEND + 100 + (end_ix + ai) as Id, ar);
                                c.fill(ar, 10.0, ink(0.04 * ah.hover));
                                let acy = ar.cy();
                                self.app_icon(c, a.icon.as_deref(), Rect::new(ar.x + 10.0, acy - 9.0, 18.0, 18.0));
                                c.text_fit(&a.name, Rect::new(ar.x + 38.0, ar.y, 180.0 - 28.0, ar.h), F13M, T1, Align::Start);
                                let bx = ar.x + 10.0 + 180.0 + 14.0;
                                let bw = (ar.w - 10.0 - 180.0 - 14.0 - 14.0 - 88.0 - 14.0 - 44.0 - 14.0 - 32.0).max(20.0);
                                let bar = Rect::new(bx, acy - 2.0, bw, 4.0);
                                c.fill(bar, 2.0, ink(0.06));
                                let share = (key(a) / mx).clamp(0.0, 1.0) as f32;
                                c.fill(Rect::new(bar.x, bar.y, if key(a) > 0.0 { (bar.w * share).max(bar.w * 0.015) } else { 0.0 }, 4.0), 2.0, accent().alpha(if ai == 0 { 1.0 } else { 0.5 }));
                                let vtxt = match metric {
                                    UMetric::Mem => crate::draw::fmt_mem_mb(a.mem),
                                    UMetric::Cpu => format!("{:.1}%", a.cpu),
                                    UMetric::Disk => format!("{:.1} MB/s", a.disk / 1_048_576.0),
                                };
                                c.text(&vtxt, Rect::new(bar.x + bar.w + 14.0, ar.y, 88.0, ar.h), num(), T1, Align::End);
                                let sh = if tot > 0.0 { format!("{:.0}%", key(a) / tot * 100.0) } else { "—".into() };
                                c.text(&sh, Rect::new(bar.x + bar.w + 14.0 + 88.0 + 14.0, ar.y, 44.0, ar.h), F12.tabular(), T3, Align::End);
                                let xb = Rect::new(bx + bw + 14.0 + 88.0 + 14.0 + 44.0 + 14.0, acy - 13.0, 26.0, 26.0);
                                let xh = f.item(ID_UEND + (end_ix + ai) as Id, xb);
                                f.hand();
                                f.tip(t!("Снять задачу", "End task", ja: "タスクの終了", pt: "Finalizar tarefa", zh: "结束任务"));
                                if ah.hover > 0.0 || xh.hover > 0.0 {
                                    c.fill(xb, 8.0, look::red_soft(0.14 * xh.hover));
                                    c.icon(I_DISMISS, (xb.cx(), xb.cy()), 12.0, 2.4, look::RED.fade(ah.hover.max(xh.hover)), 0.0);
                                }
                                ay += 40.0;
                            }
                            if u.apps.is_empty() {
                                let r = Rect::new(ax, ay, row.x + row.w - ax, 44.0);
                                c.text(t!("Сеанс заблокирован · приложений нет", "Session is locked · no apps running", ja: "セッションはロックされています · 実行中のアプリはありません", pt: "Sessão bloqueada · nenhum app em execução", zh: "会话已锁定 · 没有运行的应用"), Rect::new(r.x + 10.0, r.y, r.w - 120.0, r.h), F13, T3, Align::Start);
                                let bl = t!("Выйти", "Sign out", ja: "サインアウト", pt: "Sair", zh: "注销");
                                let bw = c.measure(bl, F12M).ceil() + 24.0;
                                ghost(c, f, ID_UBTN + (k * 2) as Id, Rect::new(r.x + r.w - bw, r.cy() - 15.0, bw, 30.0), bl, ink(0.07), T1, ink(0.11));
                            } else {
                                let by = ay + 6.0;
                                let (l1, l2) = (t!("Отправить сообщение", "Send message", ja: "メッセージを送信", pt: "Enviar mensagem", zh: "发送消息"), t!("Выйти", "Sign out", ja: "サインアウト", pt: "Sair", zh: "注销"));
                                let (w1, w2) = (c.measure(l1, F12M).ceil() + 24.0, c.measure(l2, F12M).ceil() + 24.0);
                                let rr = row.x + row.w;
                                ghost(c, f, ID_UBTN + (k * 2 + 1) as Id, Rect::new(rr - w2, by, w2, 30.0), l2, ink(0.06), T1, ink(0.1));
                                ghost(c, f, ID_UBTN + (k * 2) as Id, Rect::new(rr - w2 - 8.0 - w1, by, w1, 30.0), l1, ink(0.06), T1, ink(0.1));
                            }
                        });
                    });
                    end_ix += u.apps.len();
                } else if is_open {
                    end_ix += u.apps.len();
                }
            }
        });
        self.u_sc = sc;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Speed

    fn page_speed(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, body: Rect) {
        use boost::Id as B;
        let area = Rect::new(body.x, body.y, body.w, body.h);
        let mut sc = std::mem::take(&mut self.speed_sc);
        let mv = self.mem_view();
        let auto_on = self.boost_on(B::MemAuto);
        let (game, work) = (self.boost_on(B::Game), self.boost_on(B::Work));
        let prof = if game { 1 } else if work { 2 } else { 0 };
        let tw_open = self.tw_open.clone();
        let tw_state: Vec<bool> = TWEAKS.iter().map(|t| self.boost_on(t.id)).collect();
        let admin = self.d.is_admin;
        let mut extra = 0.0;
        for &i in &tw_open {
            extra += 16.0 + 18.6 * (TWEAKS[i].desc.chars().count() as f32 * 6.4 / 640.0).ceil().max(1.0);
        }
        let content = 24.0 + 88.0 + 20.0 + 153.0 + 20.0 + 24.0 + 304.0 + extra + 24.0;
        scroll(c, f, ID_SCROLL_SPEED, area, &mut sc, content, |c, f, v| {
            let x = area.x + 24.0;
            let w = area.w - 48.0;
            let mut y = v.at(0.0);
            // Profile
            c.text(t!("Профиль", "Profile", ja: "プロファイル", pt: "Perfil", zh: "配置"), Rect::new(x, y, 200.0, 16.0), Font::new(13.0, 600), T1, Align::Start);
            c.text(t!("Только один сразу · обратное переключение всё возвращает", "Only one at a time · switching back restores everything", ja: "同時に 1 つのみ · 戻すとすべて元に戻ります", pt: "Só um por vez · voltar restaura tudo", zh: "一次只能选一个 · 切换回来会恢复一切"), Rect::new(x, y, w, 16.0), F12, T3, Align::End);
            y += 16.0 + 8.0;
            let pw = (w - 16.0) / 3.0;
            let profs = [
                (t!("Обычный", "Normal", ja: "通常", pt: "Normal", zh: "普通"), I_POWER, t!("Ничего не приостановлено", "Nothing is paused", ja: "何も停止しません", pt: "Nada é pausado", zh: "不暂停任何东西")),
                (t!("Игра", "Game", ja: "ゲーム", pt: "Jogo", zh: "游戏"), I_GAME_P, t!("Приостанавливает обновления, телеметрию, облачную синхронизацию", "Pauses updates, telemetry, cloud sync", ja: "更新、テレメトリ、クラウド同期を一時停止", pt: "Pausa atualizações, telemetria e sincronização na nuvem", zh: "暂停更新、遥测和云同步")),
                (t!("Работа", "Work", ja: "仕事", pt: "Trabalho", zh: "工作"), I_WORK_P, t!("Закрывает лаунчеры игр и Game Bar", "Closes game launchers and Game Bar", ja: "ゲーム ランチャーと Game Bar を閉じます", pt: "Fecha os launchers de jogos e a Game Bar", zh: "关闭游戏启动器和 Game Bar")),
            ];
            for (k, (title, icon, sub)) in profs.iter().enumerate() {
                let r = Rect::new(x + k as f32 * (pw + 8.0), y, pw, 64.0);
                let on = k == prof;
                let hot = f.focusable(ID_PROF + k as Id, r);
                f.hand();
                let t = f.tween(ID_PROF + k as Id, 2, f32::from(on), 200, EASE);
                c.fill(r, 14.0, CARD.mix(accent().alpha(0.10), t));
                c.ring(r, 14.0, 1.0, ink(0.05 * (1.0 - t)));
                if t > 0.0 {
                    c.ring(r, 14.0, 1.5, accent().alpha(0.7 * t));
                }
                if hot.hover > 0.0 {
                    c.fill(r, 14.0, Rgba::pure_white(0.04 * hot.hover));
                }
                let tile_r = Rect::new(r.x + 14.0, r.cy() - 18.0, 36.0, 36.0);
                c.fill(tile_r, 10.0, ink(0.06).mix(accent().alpha(0.2), t));
                c.icon(icon, (tile_r.cx(), tile_r.cy()), 18.0, 1.9, T2.mix(acc_text(), t), 0.0);
                c.text(title, Rect::new(tile_r.x + 48.0, r.cy() - 18.0, r.w - 48.0 - 14.0 - 36.0 - 18.0, 18.0), Font::new(13.0, 600), T1, Align::Start);
                c.text_fit(sub, Rect::new(tile_r.x + 48.0, r.cy() + 1.0, r.w - 14.0 - 36.0 - 48.0 - 18.0 - 14.0, 17.0), F12, T3, Align::Start);
                let rc = (r.x + r.w - 14.0 - 9.0, r.cy());
                c.ring(Rect::new(rc.0 - 8.0, rc.1 - 8.0, 16.0, 16.0), 8.0, 1.5, ink(0.25).mix(accent(), t));
                if t > 0.0 {
                    c.circle(rc.0, rc.1, 4.5 * t, accent());
                }
            }
            y += 64.0 + 20.0;
            // Memory
            c.text(t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存"), Rect::new(x, y, 200.0, 16.0), Font::new(13.0, 600), T1, Align::Start);
            y += 16.0 + 8.0;
            let card = Rect::new(x, y, w, 129.0);
            c.fill(card, 14.0, CARD);
            c.ring(card, 14.0, 1.0, ink(0.05));
            let btn_l = t!("Очистить кэш", "Clear cache", ja: "キャッシュを消去", pt: "Limpar cache", zh: "清除缓存");
            let bw = c.measure(btn_l, F13M).ceil() + 28.0;
            let bar = Rect::new(card.x + 16.0, card.y + 16.0, card.w - 32.0 - bw - 16.0, 10.0);
            c.fill(bar, 5.0, ink(0.06));
            let tot = mv.total.max(1) as f32;
            let uw = bar.w * mv.used as f32 / tot;
            c.fill(Rect::new(bar.x, bar.y, uw, 10.0), 5.0, accent());
            let cw = bar.w * mv.standby as f32 / tot;
            if cw > 0.0 {
                c.fill(Rect::new(bar.x + uw + 2.0, bar.y, cw, 10.0), 5.0, accent().alpha(0.4));
            }
            let ly = bar.y + 10.0 + 10.0;
            let mut lx = bar.x;
            let in_use = t!("Используется", "In use", ja: "使用中", pt: "Em uso", zh: "使用中");
            let stand = t!("Кэш в режиме ожидания", "Standby cache", ja: "スタンバイ キャッシュ", pt: "Cache em espera", zh: "备用缓存");
            let free_l = t!("Свободно", "Free", ja: "空き", pt: "Livre", zh: "可用");
            for (k, (label, val, dot)) in [(in_use, Self::gb(mv.used), 0), (stand, Self::gb(mv.standby), 1), (free_l, Self::gb(mv.free), 2)].iter().enumerate() {
                let _ = k;
                match dot {
                    0 => c.circle(lx + 4.0, ly + 8.0, 4.0, accent()),
                    1 => c.circle(lx + 4.0, ly + 8.0, 4.0, accent().alpha(0.4)),
                    _ => c.ring(Rect::new(lx + 0.75, ly + 4.75, 6.5, 6.5), 3.25, 1.5, T3),
                }
                let lw = c.measure(label, F12).ceil();
                c.text(label, Rect::new(lx + 14.0, ly, lw + 2.0, 16.0), F12, T2, Align::Start);
                let vw = c.measure(val, Font::new(12.0, 600).tabular()).ceil();
                c.text(val, Rect::new(lx + 14.0 + lw + 6.0, ly, vw + 2.0, 16.0), Font::new(12.0, 600).tabular(), T1, Align::Start);
                lx += 14.0 + lw + 6.0 + vw + 18.0;
            }
            // clear button
            let b = Rect::new(card.x + card.w - 16.0 - bw, card.y + 16.0, bw, 34.0);
            let hot = f.focusable(ID_MEM_CLEAR, b);
            f.hand();
            c.transformed(b.cx(), b.cy(), 1.0 - 0.03 * hot.press, 0.0, 0.0, |c| {
                c.fill(b, 10.0, T1.mix(Rgba::pure_white(1.0), 0.0));
                if hot.hover > 0.0 {
                    c.fill(b, 10.0, Rgba::pure_white(0.08 * hot.hover));
                }
                c.text(btn_l, b, F13M, INV, Align::Center);
            });
            c.fill(Rect::new(card.x + 16.0, card.y + 65.0, card.w - 32.0, 1.0), 0.0, ink(0.06));
            let ry = card.y + 96.5;
            c.text(t!("Очищать автоматически, когда памяти мало", "Clear automatically when memory runs short", ja: "メモリが不足したら自動で消去", pt: "Limpar automaticamente quando a memória estiver baixa", zh: "内存不足时自动清除"), Rect::new(card.x + 16.0, ry - 16.5, card.w - 100.0, 16.0), F13M, T1, Align::Start);
            c.text_fit(t!("Когда свободно меньше 5% и кэш больше 1 ГБ · не чаще раза в минуту · работает, пока работает панель", "When less than 5% is free and the cache holds over 1 GB · at most once a minute · works while the bar runs", ja: "空きが 5% 未満でキャッシュが 1 GB を超えたとき · 1 分に 1 回まで · バーの動作中のみ", pt: "Quando há menos de 5% livre e o cache passa de 1 GB · no máximo uma vez por minuto · funciona enquanto a barra roda", zh: "可用内存低于 5% 且缓存超过 1 GB 时 · 每分钟最多一次 · 工具栏运行时有效"), Rect::new(card.x + 16.0, ry + 1.5, card.w - 100.0, 15.0), F12, T3, Align::Start);
            switch(c, f, ID_MEM_AUTO, card.x + card.w - 16.0 - 36.0, ry, auto_on);
            y += 129.0 + 20.0;
            // Tweaks
            c.text(t!("Твики", "Tweaks", ja: "チューニング", pt: "Ajustes", zh: "调整"), Rect::new(x, y, 200.0, 16.0), Font::new(13.0, 600), T1, Align::Start);
            c.text(t!("Нажмите на строку, чтобы узнать подробности", "Click a row for details", ja: "行をクリックすると詳細を表示します", pt: "Clique em uma linha para ver detalhes", zh: "点击一行查看详情"), Rect::new(x, y, w, 16.0), F12, T3, Align::End);
            y += 16.0 + 8.0;
            let mut total_h = 0.0;
            for (i, tw) in TWEAKS.iter().enumerate() {
                let open = tw_open.contains(&i);
                let lines = if open { (c.measure(tw.desc, F12) / 640.0).ceil().max(1.0) } else { 0.0 };
                total_h += if i == 0 { 60.0 } else { 61.0 } + if open { 16.0 + lines * 18.6 } else { 0.0 };
            }
            let box_r = Rect::new(x, y, w, total_h);
            c.fill(box_r, 14.0, CARD);
            c.ring(box_r, 14.0, 1.0, ink(0.05));
            let mut ty = y;
            for (i, tw) in TWEAKS.iter().enumerate() {
                let open = tw_open.contains(&i);
                let lines = if open { (c.measure(tw.desc, F12) / 640.0).ceil().max(1.0) } else { 0.0 };
                let h = if i == 0 { 60.0 } else { 61.0 } + if open { 16.0 + lines * 18.6 } else { 0.0 };
                let row = Rect::new(x, ty + if i == 0 { 0.0 } else { 1.0 }, w, 60.0);
                if i > 0 {
                    c.fill(Rect::new(x, ty, w, 1.0), 0.0, ink(0.05));
                }
                let hot = f.focusable(ID_TWEAK + 2 * i as Id, row);
                f.hand();
                c.fill(row, if i == 0 { 14.0 } else { 0.0 }, ink(0.03 * hot.hover));
                let on = tw_state[i];
                let t = f.tween(ID_TWEAK + 2 * i as Id, 3, f32::from(on), 200, EASE);
                let tr = Rect::new(x + 16.0, row.cy() - 16.0, 32.0, 32.0);
                c.fill(tr, 9.0, ink(0.06).mix(accent().alpha(0.2), t));
                c.icon(tw.icon, (tr.cx(), tr.cy()), 16.0, 1.9, T2.mix(acc_text(), t), 0.0);
                let tag = match tw.kind {
                    TwKind::Admin if !on && !admin => Some((t!("Нужен администратор", "Needs admin", ja: "管理者が必要", pt: "Requer administrador", zh: "需要管理员"), look::amber_soft(0.12), look::AMBER)),
                    TwKind::Admin if !on => Some((t!("Нужен администратор", "Needs admin", ja: "管理者が必要", pt: "Requer administrador", zh: "需要管理员"), look::amber_soft(0.12), look::AMBER)),
                    TwKind::Reboot if on => Some((t!("После перезагрузки", "After reboot", ja: "再起動後", pt: "Após reiniciar", zh: "重启后生效"), ink(0.07), T2)),
                    _ => None,
                };
                let sw_x = x + w - 16.0 - 36.0;
                let chev_x = sw_x - 12.0 - 14.0;
                let mut right = chev_x - 12.0;
                if let Some((label, bg, fg)) = tag {
                    let pw = c.measure(label, F12M).ceil() + 16.0;
                    pill(c, right - pw, row.cy(), label, bg, fg, None, false);
                    right -= pw + 12.0;
                }
                c.text(tw.title, Rect::new(tr.x + 44.0, row.cy() - 19.0, (right - tr.x - 44.0).max(0.0), 18.0), F13M, T1, Align::Start);
                c.text_fit(tw.sub, Rect::new(tr.x + 44.0, row.cy() + 1.0, (right - tr.x - 44.0).max(0.0), 17.0), F12, T3, Align::Start);
                let turn = f.tween(ID_TWEAK + 2 * i as Id, 4, f32::from(open), 250, SPRING);
                c.icon(I_CHEV_DOWN, (chev_x + 7.0, row.cy()), 14.0, 2.2, T3, 180.0 * turn);
                switch(c, f, ID_TWEAK + 2 * i as Id + 1, sw_x, row.cy(), on);
                if open {
                    c.text_wrapped(tw.desc, Rect::new(x + 60.0, row.y + 60.0, 640.0f32.min(w - 76.0), h - 60.0), F12, T2, 18.6, 0);
                }
                ty += h;
            }
        });
        self.speed_sc = sc;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Settings

    fn page_settings(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, body: Rect) {
        let area = Rect::new(body.x, body.y, body.w, body.h);
        let mut sc = std::mem::take(&mut self.set_sc);
        let d = &self.d;
        let admin = d.is_admin;
        // the design's own pick (its theme is the one on show): a demo that follows the system says which one it is showing
        let theme = if self.is_demo() && d.cfg.theme == 2 { u8::from(!winrust_common::style::light()) } else { d.cfg.theme };
        let cards: [(&'static str, &'static str, &'static str, u8); 5] = [
            (I_THEME, t!("Тема оформления", "Theme", ja: "テーマ", pt: "Tema", zh: "主题"), t!("Светлая, тёмная или как в системе", "Light, dark or follow the system", ja: "ライト、ダーク、またはシステムに従う", pt: "Claro, escuro ou seguir o sistema", zh: "浅色、深色或跟随系统"), 0),
            (I_TRAY, t!("Закрытие окна сворачивает в трей", "Closing the window minimizes to tray", ja: "ウィンドウを閉じるとトレイに最小化", pt: "Fechar a janela minimiza para a bandeja", zh: "关闭窗口时最小化到托盘"), t!("Продолжает собирать историю и применять правила; вернуть окно можно по Ctrl+Alt+T", "Keeps collecting history and applying rules; Ctrl+Alt+T brings the window back", ja: "履歴の収集とルールの適用を続けます。Ctrl+Alt+T でウィンドウを戻せます", pt: "Continua coletando o histórico e aplicando as regras; Ctrl+Alt+T traz a janela de volta", zh: "继续收集历史记录并应用规则；按 Ctrl+Alt+T 可调回窗口"), 1),
            (I_POWER, t!("Запускать при входе в Windows", "Start with Windows", ja: "Windows と同時に起動", pt: "Iniciar com o Windows", zh: "随 Windows 启动"), t!("Сразу в трей, без окна", "Straight to the tray, no window", ja: "ウィンドウを表示せず、直接トレイへ", pt: "Direto na bandeja, sem janela", zh: "直接进托盘，不显示窗口"), 2),
            (I_NET, t!("Сеть по процессам", "Per-process network", ja: "プロセスごとのネットワーク", pt: "Rede por processo", zh: "按进程统计网络"), t!("Колонка «Сеть» в процессах (ETW)", "‘Network’ column in Processes (ETW)", ja: "「プロセス」の「ネットワーク」列 (ETW)", pt: "Coluna ‘Rede’ em Processos (ETW)", zh: "进程中的「网络」列（ETW）"), 3),
            (I_THERMO, t!("Всегда читать температуру GPU", "Always read GPU temperature", ja: "GPU 温度を常に読み取る", pt: "Sempre ler a temperatura da GPU", zh: "始终读取 GPU 温度"), t!("Иначе она читается только на странице GPU. Постоянный опрос может будить дискретную видеокарту ноутбука", "Otherwise it is read only on the GPU page. Constant polling can wake a laptop’s discrete GPU", ja: "オフの場合、GPU ページでのみ読み取ります。常時ポーリングするとノート PC の外部 GPU が起動することがあります", pt: "Caso contrário, é lida só na página da GPU. A leitura constante pode acordar a GPU dedicada de um notebook", zh: "否则仅在 GPU 页面读取。持续轮询可能唤醒笔记本的独立显卡"), 4),
        ];
        let on_of = |k: u8| match k {
            1 => d.cfg.to_tray,
            2 => self.m.self_autostart(),
            3 => d.etw.is_some(),
            4 => d.cfg.gpu_temp_always,
            _ => false,
        };
        let version = if self.is_demo() { "0.5.9" } else { env!("CARGO_PKG_VERSION") };
        let content = 8.0 + 5.0 * 76.0 + 40.0;
        let theme_l = [t!("Светлая", "Light", ja: "ライト", pt: "Claro", zh: "浅色"), t!("Тёмная", "Dark", ja: "ダーク", pt: "Escuro", zh: "深色"), t!("Системная", "System", ja: "システム", pt: "Sistema", zh: "系统")];
        scroll(c, f, ID_SCROLL_SET, area, &mut sc, content, |c, f, v| {
            let w = (area.w - 48.0).min(760.0);
            let x = area.x + 24.0;
            let mut y = v.at(0.0);
            for (icon, title, sub, k) in cards {
                let (ctrl_w, badge) = match k {
                    0 => (c.measure(theme_l[0], F12M).ceil() + c.measure(theme_l[1], F12M).ceil() + c.measure(theme_l[2], F12M).ceil() + 72.0 + 4.0, ""),
                    3 if !admin => (c.measure(t!("Перезапустить от имени администратора", "Restart as administrator", ja: "管理者として再起動", pt: "Reiniciar como administrador", zh: "以管理员身份重启"), F12M).ceil() + 24.0, t!("Нужен администратор", "Administrator needed", ja: "管理者が必要", pt: "Requer administrador", zh: "需要管理员")),
                    _ => (36.0, ""),
                };
                let text_w = w - 32.0 - 34.0 - 14.0 - ctrl_w - 14.0;
                let lines = c.lines(sub, F12, text_w.max(80.0)) as f32;
                let h = (24.0 + 20.0 + 3.0 + lines * 17.4).max(68.0);
                let r = Rect::new(x, y, w, h);
                c.fill(r, 14.0, CARD);
                c.ring(r, 14.0, 1.0, ink(0.05));
                let ic = Rect::new(r.x + 16.0, r.cy() - 17.0, 34.0, 34.0);
                c.fill(ic, 10.0, ink(0.06));
                c.icon(icon, (ic.cx(), ic.cy()), 17.0, 1.8, T2, 0.0);
                let tx = ic.x + 34.0 + 14.0;
                let ty = r.cy() - (20.0 + 3.0 + lines * 17.4) / 2.0;
                let tw = c.measure(title, F13M).ceil();
                c.text(title, Rect::new(tx, ty, tw + 2.0, 20.0), F13M, T1, Align::Start);
                if !badge.is_empty() {
                    let bw = c.measure(badge, Font::new(11.0, 500)).ceil() + 14.0;
                    let br = Rect::new(tx + tw + 8.0, ty + 0.0, bw, 20.0);
                    c.fill(br, 6.0, look::amber_soft(0.12));
                    c.text(badge, br, Font::new(11.0, 500), look::AMBER, Align::Center);
                }
                c.text_wrapped(sub, Rect::new(tx, ty + 23.0, text_w.max(80.0), lines * 17.4), F12, T3, 17.4, 0);
                let right = r.x + r.w - 16.0;
                match k {
                    0 => seg(c, f, ID_SETSEG, right, r.cy(), &theme_l, theme.min(2) as usize),
                    3 if !admin => {
                        let b = Rect::new(right - ctrl_w, r.cy() - 15.0, ctrl_w, 30.0);
                        ghost(c, f, ID_SETBTN, b, t!("Перезапустить от имени администратора", "Restart as administrator", ja: "管理者として再起動", pt: "Reiniciar como administrador", zh: "以管理员身份重启"), ink(0.07), T1, ink(0.11));
                    }
                    _ => {
                        switch(c, f, ID_SETSW + k as Id, right - 36.0, r.cy(), on_of(k));
                    }
                }
                y += h + 8.0;
            }
            c.text(&format!("{} · WinRust v{version} · {}", t!("Диспетчер задач (Rust)", "Task Manager (Rust)", ja: "タスク マネージャー (Rust)", pt: "Gerenciador de Tarefas (Rust)", zh: "任务管理器（Rust）"), t!("Ctrl+Alt+T возвращает окно", "Ctrl+Alt+T brings the window back", ja: "Ctrl+Alt+T でウィンドウを戻します", pt: "Ctrl+Alt+T traz a janela de volta", zh: "Ctrl+Alt+T 可调回窗口")), Rect::new(x, y, w, 24.0), F12.tabular(), T3, Align::Start);
        });
        self.set_sc = sc;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // the toast

    fn draw_toast(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>, w: f32, h: f32) {
        let Some(t) = self.toast.as_mut() else { return };
        let born = *t.born.get_or_insert(f.now);
        let age = f.now.saturating_sub(born);
        if age >= TOAST_MS {
            self.toast = None;
            return;
        }
        // keeps frames coming for as long as it shows (the glide of an item that was retargeted at its birth)
        f.tween(-5000, 9, t.serial as f32, TOAST_MS as u32, LINEAR);
        let p = age as f32 / TOAST_MS as f32;
        let (op, dy) = if p < 0.12 {
            let k = EASE.ease(p / 0.12);
            (k, 10.0 * (1.0 - k))
        } else if p < 0.85 {
            (1.0, 0.0)
        } else {
            (1.0 - (p - 0.85) / 0.15, 0.0)
        };
        let tw = c.measure(&t.text, F13M).ceil();
        let r = Rect::new(((w - (16.0 + 8.0 + 10.0 + tw + 16.0)) / 2.0).round(), h - 20.0 - 40.0, 16.0 + 8.0 + 10.0 + tw + 16.0, 40.0);
        let (text, dot) = (t.text.clone(), t.dot);
        c.faded(op, |c| {
            c.transformed(0.0, 0.0, 1.0, 0.0, dy, |c| {
                c.outside(r, 20.0, |c| c.shadow(r, 20.0, 16.0, 40.0, Rgba::css(0, 0, 0, 0.5)));
                c.fill(r, 20.0, MENU);
                c.ring(r.inset(-1.0), 21.0, 1.0, Rgba::css(0, 0, 0, 0.5));
                c.ring(r, 20.0, 1.0, ink(0.1));
                c.circle(r.x + 16.0 + 4.0, r.cy(), 4.0, dot);
                c.text(&text, Rect::new(r.x + 16.0 + 8.0 + 10.0, r.y, tw + 2.0, r.h), F13M, T1, Align::Start);
            });
        });
    }
}

use winrust_ui::widgets::look::MENU;

fn lower_first(s: &str) -> String {
    let mut ch = s.chars();
    ch.next().map_or(String::new(), |f| f.to_lowercase().chain(ch).collect())
}

fn nav_name_col() -> &'static str {
    t!("Имя", "Name", ja: "名前", pt: "Nome", zh: "名称")
}

fn suspect_label(sev: u8) -> &'static str {
    crate::suspect::sev_word(sev)
}

/// The sort arrow of a header, `↓` for the descending order (the biggest first), `↑` for the other.
fn arrow(c: &mut Canvas<'_>, cx: f32, cy: f32, col: Rgba, desc: bool) {
    c.icon(I_ARROW_DOWN, (cx, cy), 10.0, 2.6, col, if desc { 0.0 } else { 180.0 });
}

/// The history's time, `2026-11-27 16:58` -> `16:58`, as the design shows it; a time that is only a clock stays.
fn hist_time(s: &str) -> String {
    s.split_whitespace().last().unwrap_or(s).to_string()
}

pub fn fmt_mem_mb(b: u64) -> String {
    let mb = b as f64 / 1_048_576.0;
    if mb >= 100.0 { format!("{} MB", crate::ui::group_thousands(mb.round() as u64).replace(',', " ")) } else { format!("{mb:.1} MB") }
}

/// A user's memory: gigabytes from one up, megabytes below.
pub fn fmt_mem_gb(b: u64) -> String {
    if b >= 1_073_741_824 { format!("{:.1} GB", b as f64 / 1_073_741_824.0) } else { format!("{} MB", (b as f64 / 1_048_576.0).round()) }
}
