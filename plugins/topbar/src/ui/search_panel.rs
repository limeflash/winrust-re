//! The search box (the magnifier on the bar), as in the Obsidian mockup: 640 wide, 150 down from the top of the screen over a
//! dimmed screen, a big field, and under it what the query finds in groups: apps, commands, files and Windows settings (what was
//! opened last while the field is empty). Arrows pick, Enter opens, Ctrl+Shift+Enter opens as administrator, Esc clears and then
//! closes. It comes in with `srBoxIn` over `srIn`'s scrim and goes with `srOut`.
//!
//! Apps come from the same engine as the Start menu (`crate::apps`), files from the rust-search service when it runs, commands
//! and settings are the machine's own tools and Settings pages.

use std::sync::Mutex;

use winrust_common::style::pal;

use super::anim::{EASE, POP};
use super::gfx::{Align, Canvas, Font, Gfx, Rect, Rgba};
use super::panel::{Along, Button, Content, Cx, Dir, Entry, Exit, Frame, Id, Look, Mods, Origin, Place, Pose, Scrim, Shadow};
use crate::apps::{self, App};

const W: f32 = 640.0;
const PAD: f32 = 8.0;
const TOP: f32 = 150.0;
const HEAD: f32 = 56.0;
const ROW: f32 = 48.0;
/// A group's caption: padding 8 and 4 around a 12 px line.
const CAPTION: f32 = 26.5;
/// The rule under the field with its margins (4 above, 8 below).
const RULE: f32 = 13.0;
/// The key hints at the bottom: 8 above, a 1 px rule and 40.
const FOOT: f32 = 49.0;
/// How many rows of each kind the mockup lists.
const PER_GROUP: usize = 3;

const INK: Rgba = Rgba::css(236, 238, 244, 1.0);
const MUTED: Rgba = Rgba::css(163, 169, 184, 1.0);
const DIM: Rgba = Rgba::css(138, 144, 162, 1.0);
const PLACEHOLDER: Rgba = Rgba::css(117, 117, 117, 1.0);

const TERMINAL: &str = "M4 5h16v14H4zM7.5 10l3 2.5-3 2.5M13 15h4";
const FILE: &str = "M7 3h7l4 4v14H7zM14 3v4h4M10 12h5M10 16h5";
const FOLDER: &str = "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z";

/// What a row shows on its tile: a line icon, or the shell's picture of a file or app.
#[derive(Clone)]
enum Glyph {
    Path(&'static str),
    Shell(String),
    App(Vec<u16>),
}

/// What a row does.
#[derive(Clone)]
enum Run {
    App(App),
    /// A command as typed or listed (`control`, `%appdata%`).
    Cmd(String),
    Open(String),
    Uri(&'static str),
    NightLight,
    Bluetooth,
}

#[derive(Clone)]
struct Row {
    name: String,
    sub: String,
    action: &'static str,
    glyph: Glyph,
    run: Run,
}

struct Group {
    title: &'static str,
    rows: Vec<Row>,
}

/// A file the service found.
#[derive(Clone)]
struct FileHit {
    path: String,
    dir: bool,
}

/// What the file search last answered: the query it answered and the files.
static FILES: Mutex<(String, Vec<FileHit>)> = Mutex::new((String::new(), Vec::new()));
static FILE_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Asks the rust-search service (when it runs) for files, off the UI thread and after a short pause, so typing does not queue
/// a search per letter; the panel repaints when the answer is in.
fn ask_files(q: &str) {
    use std::sync::atomic::Ordering::Relaxed;
    let q = q.trim().to_string();
    let gen_ = FILE_GEN.fetch_add(1, Relaxed) + 1;
    if q.chars().count() < 2 {
        *FILES.lock().unwrap() = (String::new(), Vec::new());
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        if FILE_GEN.load(Relaxed) != gen_ {
            return;
        }
        let hits: Vec<FileHit> = winrust_common::search::ask_all(&q)
            .into_iter()
            // the shortcuts of the Start menu are the apps above, not files
            .filter(|p| !(p.to_lowercase().ends_with(".lnk") && p.to_lowercase().contains(r"\start menu\")))
            .filter_map(|p| Some(FileHit { dir: std::fs::metadata(&p).ok()?.is_dir(), path: p }))
            .take(PER_GROUP)
            .collect();
        if FILE_GEN.load(Relaxed) == gen_ {
            *FILES.lock().unwrap() = (q.to_lowercase(), hits);
            super::panel::refresh();
        }
    });
}

pub struct SearchPanel {
    query: String,
    focus_all: bool,
    groups: Vec<Group>,
    /// Where the rows are, in the order of `flat()`.
    rects: Vec<Rect>,
    captions: Vec<(f32, &'static str)>,
    /// The row Enter opens.
    sel: usize,
    last_hover: Option<i32>,
    /// The height of the box now, and the most it can be.
    height: f32,
    max_rows: usize,
    run_row: Option<f32>,
    foot_y: f32,
    files_seen: String,
    /// A toggle was pressed: its row reads the new state a moment later.
    reread_at: u64,
    recent_files: Vec<apps::Recent>,
}

impl SearchPanel {
    /// `screen_h` is the screen's height in DIPs: the list never reaches below it.
    #[must_use]
    pub fn new(screen_h: f32) -> Self {
        crate::quick::refresh(); // the Bluetooth row reads it
        let mut p = Self::blank(screen_h);
        p.recent_files = apps::recent(PER_GROUP);
        p.rebuild();
        p
    }

    fn blank(screen_h: f32) -> Self {
        // the room a full list needs: four captions and as many rows as fit above the bottom edge
        let fixed = PAD + HEAD + RULE + 4.0 * CAPTION + PAD + FOOT + PAD;
        let max_rows = (((screen_h - TOP - 8.0 - fixed) / ROW).floor() as usize).clamp(3, 4 * PER_GROUP);
        Self {
            query: String::new(),
            focus_all: false,
            groups: Vec::new(),
            rects: Vec::new(),
            captions: Vec::new(),
            sel: 0,
            last_hover: None,
            height: 0.0,
            max_rows,
            run_row: None,
            foot_y: 0.0,
            files_seen: String::new(),
            reread_at: 0,
            recent_files: Vec::new(),
        }
    }

    /// The same panel with the files it would show made up (the renderer of previews).
    #[must_use]
    pub fn with_query(screen_h: f32, query: &str, files: Vec<(String, bool)>, recent: Vec<apps::Recent>) -> Self {
        let mut p = Self::blank(screen_h);
        p.query = query.to_string();
        p.recent_files = recent;
        *FILES.lock().unwrap() = (query.trim().to_lowercase(), files.into_iter().map(|(path, dir)| FileHit { path, dir }).collect());
        p.rebuild();
        p
    }

    fn flat(&self) -> impl Iterator<Item = &Row> {
        self.groups.iter().flat_map(|g| g.rows.iter())
    }

    fn row_count(&self) -> usize {
        self.groups.iter().map(|g| g.rows.len()).sum()
    }

    fn q(&self) -> String {
        self.query.trim().to_lowercase()
    }

    /// Finds what the query asks for and lays the rows out.
    fn rebuild(&mut self) {
        let q = self.q();
        let sub_app = t!("Приложение", "App", ja: "アプリ", pt: "App", zh: "应用");
        let (open, run) = (t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开"), t!("Запустить", "Run", ja: "実行", pt: "Executar", zh: "运行"));
        let mut groups: Vec<Group> = Vec::new();
        if q.is_empty() {
            let cmds = recent_commands();
            if !cmds.is_empty() {
                groups.push(Group { title: t!("Недавние команды", "Recent commands", ja: "最近のコマンド", pt: "Comandos recentes", zh: "最近的命令"), rows: cmds });
            } else {
                let rows = commands("").into_iter().take(PER_GROUP).collect();
                groups.push(Group { title: t!("Команды", "Commands", ja: "コマンド", pt: "Comandos", zh: "命令"), rows });
            }
            let rows = self
                .recent_files
                .iter()
                .map(|r| Row { name: r.name.clone(), sub: parent_of(&r.path), action: open, glyph: Glyph::Shell(r.path.clone()), run: Run::Open(r.path.clone()) })
                .collect();
            groups.push(Group { title: t!("Недавние файлы", "Recent files", ja: "最近使ったファイル", pt: "Arquivos recentes", zh: "最近的文件"), rows });
        } else {
            let found = apps::search(&q).into_iter().take(PER_GROUP);
            let rows = found.map(|a| Row { name: a.name.clone(), sub: sub_app.to_string(), action: open, glyph: Glyph::App(a.path.clone()), run: Run::App(a) }).collect();
            groups.push(Group { title: t!("Приложения", "Apps", ja: "アプリ", pt: "Aplicativos", zh: "应用"), rows });
            groups.push(Group { title: t!("Команды", "Commands", ja: "コマンド", pt: "Comandos", zh: "命令"), rows: commands(&q).into_iter().take(PER_GROUP).collect() });
            let files = {
                let f = FILES.lock().unwrap();
                self.files_seen = f.0.clone();
                if f.0 == q { f.1.clone() } else { Vec::new() }
            };
            let rows = files
                .into_iter()
                .map(|h| {
                    let name = std::path::Path::new(&h.path).file_name().map_or_else(|| h.path.clone(), |n| n.to_string_lossy().to_string());
                    let _ = h.dir;
                    Row { name, sub: parent_of(&h.path), action: open, glyph: Glyph::Shell(h.path.clone()), run: Run::Open(h.path) }
                })
                .collect();
            groups.push(Group { title: t!("Файлы", "Files", ja: "ファイル", pt: "Arquivos", zh: "文件"), rows });
            groups.push(Group { title: t!("Параметры", "Settings", ja: "設定", pt: "Configurações", zh: "设置"), rows: settings(&q) });
        }
        groups.retain(|g| !g.rows.is_empty());
        // what does not fit under the field is dropped from the end
        let mut left = self.max_rows;
        for g in &mut groups {
            g.rows.truncate(left);
            left -= g.rows.len();
        }
        groups.retain(|g| !g.rows.is_empty());
        let _ = run;
        self.groups = groups;
        self.sel = self.sel.min(self.row_count().saturating_sub(1));
        self.layout();
    }

    fn layout(&mut self) {
        self.rects.clear();
        self.captions.clear();
        let mut y = PAD + HEAD;
        self.run_row = None;
        if self.row_count() > 0 {
            y += RULE;
            for g in &self.groups {
                self.captions.push((y, g.title));
                y += CAPTION;
                for _ in &g.rows {
                    self.rects.push(Rect::new(PAD, y, W - 2.0 * PAD, ROW));
                    y += ROW;
                }
            }
        } else if !self.q().is_empty() {
            // nothing found: a row to run what was typed (margin 8 above, 4 below)
            self.run_row = Some(y + 8.0);
            y += 8.0 + ROW + 4.0;
        }
        self.foot_y = y + 8.0;
        self.height = self.foot_y + FOOT - 8.0 + PAD;
    }

    fn max_height(&self) -> f32 {
        PAD + HEAD + RULE + 4.0 * CAPTION + self.max_rows as f32 * ROW + PAD + FOOT + PAD
    }

    /// What Enter does with the selected row, or with what was typed when nothing was found.
    fn act(&mut self, idx: Option<usize>, admin: bool, cx: &mut Cx) {
        let row = idx.and_then(|i| self.flat().nth(i)).cloned();
        match row {
            Some(row) => match row.run {
                Run::NightLight => {
                    let on = crate::quick::night_light().unwrap_or(false);
                    if !crate::quick::set_night_light(!on) {
                        cx.close_then(|| open_str("ms-settings:nightlight"));
                    }
                    self.reread_at = reread_time();
                }
                Run::Bluetooth => match crate::quick::bluetooth() {
                    Some(on) => {
                        crate::quick::set(false, !on);
                        self.reread_at = reread_time();
                    }
                    None => cx.close_then(|| open_str("ms-settings:bluetooth")),
                },
                Run::App(a) => cx.close_then(move || if admin && apps::is_file(&a) { apps::launch_admin(&a) } else { apps::launch(&a) }),
                Run::Cmd(c) => cx.close_then(move || run_command(&c, admin)),
                Run::Open(p) => cx.close_then(move || open_str(&p)),
                Run::Uri(u) => cx.close_then(move || open_str(u)),
            },
            None => {
                let c = self.query.trim().to_string();
                if !c.is_empty() {
                    cx.close_then(move || run_command(&c, admin));
                }
            }
        }
    }

    fn draw_row(&self, c: &mut Canvas<'_>, f: &mut Frame<'_>, i: usize, row: &Row, r: Rect) {
        let sel = i == self.sel;
        let t = f.tween(i as i32, 3, f32::from(sel), 120, EASE);
        c.fill(r, 12.0, Rgba::white(0.09 * t));
        let tile = Rect::new(r.x + 12.0, r.cy() - 16.0, 32.0, 32.0);
        let image = match &row.glyph {
            Glyph::Path(_) => None,
            Glyph::App(p) => Some(apps::icon(p)),
            Glyph::Shell(p) => apps::image(p).map(|i| i.icon),
        };
        if let Some(ic) = image {
            c.hicon(ic, Rect::new(tile.cx() - 15.0, tile.cy() - 15.0, 30.0, 30.0), 1.0);
        } else {
            c.fill(tile, 9.0, Rgba::white(0.08));
            let g = match row.glyph {
                Glyph::Path(d) => d,
                _ => FILE,
            };
            c.icon(g, (tile.cx(), tile.cy()), 18.0, 1.8, INK, 0.0);
        }
        let tx = tile.x + 32.0 + 12.0;
        let tw = r.x + r.w - 12.0 - 70.0 - tx;
        c.text_fit(&row.name, Rect::new(tx, r.cy() - 16.0, tw, 15.7), Font::new(13.0, 500), INK, Align::Start);
        c.text_fit(&row.sub, Rect::new(tx, r.cy() + 0.5, tw, 14.5), Font::new(12.0, 400), DIM, Align::Start);
        if sel {
            c.text(row.action, Rect::new(r.x + r.w - 12.0 - 80.0, r.y, 80.0, r.h), Font::new(12.0, 400), MUTED, Align::End);
        }
    }

    fn draw_foot(&self, c: &mut Canvas<'_>) {
        let y = self.foot_y;
        c.fill(Rect::new(PAD, y, W - 2.0 * PAD, 1.0), 0.0, Rgba::white(0.06));
        let cy = y + 1.0 + 20.0;
        let (kf, tf) = (Font::new(11.0, 500), Font::new(12.0, 400));
        let mut x = PAD + 12.0;
        let key = |c: &mut Canvas<'_>, x: &mut f32, s: &str| {
            let w = c.measure(s, kf) + 12.0;
            kbd(c, Rect::new(*x, cy - 8.65, w, 17.3), s);
            *x += w;
        };
        let hint = |c: &mut Canvas<'_>, x: &mut f32, s: &str| {
            let w = c.measure(s, tf);
            c.text(s, Rect::new(*x, cy - 8.0, w + 2.0, 16.0), tf, DIM, Align::Start);
            *x += w + 16.0;
        };
        key(c, &mut x, "\u{2191}\u{2193}");
        x += 6.0;
        hint(c, &mut x, t!("выбрать", "select", ja: "選択", pt: "selecionar", zh: "选择"));
        key(c, &mut x, "Enter");
        x += 6.0;
        hint(c, &mut x, t!("открыть", "open", ja: "開く", pt: "abrir", zh: "打开"));
        for (n, k) in ["Ctrl", "Shift", "Enter"].into_iter().enumerate() {
            key(c, &mut x, k);
            x += if n < 2 { 3.0 } else { 6.0 };
        }
        hint(c, &mut x, t!("от имени администратора", "as administrator", ja: "管理者として", pt: "como administrador", zh: "以管理员身份"));
        // Esc at the right end
        let label = t!("закрыть", "close", ja: "閉じる", pt: "fechar", zh: "关闭");
        let (lw, kw) = (c.measure(label, tf), c.measure("Esc", kf) + 12.0);
        let rx = W - PAD - 12.0;
        c.text(label, Rect::new(rx - lw, cy - 8.0, lw + 2.0, 16.0), tf, DIM, Align::Start);
        kbd(c, Rect::new(rx - lw - 6.0 - kw, cy - 8.65, kw, 17.3), "Esc");
    }
}

/// When a switch that was just pressed should be read again.
fn reread_time() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { crate::now_ms() + 400 }
}

fn parent_of(path: &str) -> String {
    std::path::Path::new(path).parent().map(|p| p.to_string_lossy().to_string()).filter(|p| !p.is_empty()).unwrap_or_else(|| path.to_string())
}

fn kbd(c: &mut Canvas<'_>, r: Rect, key: &str) {
    c.fill(r, 4.0, Rgba::white(0.08));
    c.inset_edge(r, 4.0, -1.0, Rgba::white(0.1));
    c.text(key, r, Font::new(11.0, 500), MUTED, Align::Center);
}

fn open_str(what: &str) {
    // SAFETY: `open` hands the path to the shell.
    unsafe { crate::open(&crate::wide(what)) };
}

/// Runs a command the way the Run box does: environment variables expanded, a bare name looked up on PATH.
fn run_command(cmd: &str, admin: bool) {
    // SAFETY: shell calls with NUL-terminated strings.
    unsafe {
        let target = crate::resolve_cmd(cmd);
        if admin {
            crate::ShellExecuteW(crate::null_mut(), crate::wide("runas").as_ptr(), crate::wide(&target).as_ptr(), crate::null(), crate::null(), crate::SW_SHOWNORMAL);
        } else {
            crate::open(&crate::wide(&target));
        }
    }
}

/// The tools the mockup lists, with what they are in the user's language.
fn commands(q: &str) -> Vec<Row> {
    let list: [(&str, &str, &'static str); 7] = [
        ("control", t!("Панель управления", "Control Panel", ja: "コントロール パネル", pt: "Painel de Controle", zh: "控制面板"), "M4 7h10M18 7h2M4 17h2M10 17h10M14 5v4M6 15v4"),
        ("taskmgr", t!("Диспетчер задач", "Task Manager", ja: "タスク マネージャー", pt: "Gerenciador de Tarefas", zh: "任务管理器"), ""),
        ("regedit", t!("Редактор реестра", "Registry Editor", ja: "レジストリ エディター", pt: "Editor do Registro", zh: "注册表编辑器"), "M12 3l8 4.5v9L12 21l-8-4.5v-9zM12 12l8-4.5M12 12v9M12 12L4 7.5"),
        ("powershell", "Windows PowerShell", TERMINAL),
        ("cmd", t!("Командная строка", "Command Prompt", ja: "コマンド プロンプト", pt: "Prompt de Comando", zh: "命令提示符"), TERMINAL),
        ("%appdata%", t!("Папка AppData (Roaming)", "AppData folder (Roaming)", ja: "AppData フォルダー (Roaming)", pt: "Pasta AppData (Roaming)", zh: "AppData 文件夹 (Roaming)"), ""),
        ("msconfig", t!("Конфигурация системы, загрузка", "System Configuration, boot", ja: "システム構成、ブート", pt: "Configuração do Sistema, inicialização", zh: "系统配置、启动"), super::icons::GEAR),
    ];
    let run = t!("Запустить", "Run", ja: "実行", pt: "Executar", zh: "运行");
    list.into_iter()
        .filter(|(name, sub, _)| q.is_empty() || format!("{name} {sub}").to_lowercase().contains(q))
        .map(|(name, sub, d)| {
            // the tools with a real icon of their own show it
            let glyph = if d.is_empty() { // SAFETY: a path lookup.
            Glyph::Shell(unsafe { crate::resolve_cmd(name) }) } else { Glyph::Path(d) };
            Row { name: name.to_string(), sub: sub.to_string(), action: run, glyph, run: Run::Cmd(name.to_string()) }
        })
        .collect()
}

/// What the user typed in the Run box last (Windows keeps it under `RunMRU`), newest first.
fn recent_commands() -> Vec<Row> {
    let run = t!("Запустить", "Run", ja: "実行", pt: "Executar", zh: "运行");
    let cmd_sub = t!("Команда", "Command", ja: "コマンド", pt: "Comando", zh: "命令");
    apps::run_history(PER_GROUP)
        .into_iter()
        .map(|c| Row { glyph: Glyph::Path(TERMINAL), name: c.clone(), sub: cmd_sub.to_string(), action: run, run: Run::Cmd(c) })
        .collect()
}

/// The Windows Settings pages (and the two switches) that match the query.
fn settings(q: &str) -> Vec<Row> {
    let pages: [(&str, &str, &str, &str, &str, &'static str, &'static str); 14] = [
        ("Дисплей", "Display", "ディスプレイ", "Tela", "显示", "ms-settings:display", "M3 5h18v11H3zM8 20h8M12 16v4"),
        ("Звук", "Sound", "サウンド", "Som", "声音", "ms-settings:sound", "M4 9.5h3.5L12 6v12l-4.5-3.5H4zM15.5 9a4 4 0 0 1 0 6"),
        ("Ночной свет", "Night light", "夜間モード", "Luz noturna", "夜间模式", "ms-settings:nightlight", "M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5a8.5 8.5 0 1 0 10.7 10.7z"),
        ("Bluetooth", "Bluetooth", "Bluetooth", "Bluetooth", "蓝牙", "ms-settings:bluetooth", "M7 7l10 10-5 4V3l5 4L7 17"),
        ("Wi‑Fi", "Wi-Fi", "Wi‑Fi", "Wi‑Fi", "Wi‑Fi", "ms-settings:network-wifi", "M3 9.5a13 13 0 0 1 18 0M6 13a8.5 8.5 0 0 1 12 0M9 16.5a4 4 0 0 1 6 0M12 19.5h.01"),
        ("Персонализация", "Personalization", "個人用設定", "Personalização", "个性化", "ms-settings:personalization", "M12 21a9 9 0 1 1 9-9c0 2-1.5 3-3 3h-2a2 2 0 0 0-1.5 3.3A2 2 0 0 1 12 21zM7.5 11h.01M10 7.5h.01M14.5 7.5h.01"),
        ("Приложения", "Apps", "アプリ", "Aplicativos", "应用", "ms-settings:appsfeatures", "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z"),
        ("Язык и клавиатура", "Language and keyboard", "言語とキーボード", "Idioma e teclado", "语言和键盘", "ms-settings:regionlanguage", "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8"),
        ("Дата и время", "Date and time", "日付と時刻", "Data e hora", "日期和时间", "ms-settings:dateandtime", "M4 6h16v14H4zM4 10h16M8 3v4M16 3v4"),
        ("Питание и батарея", "Power and battery", "電源とバッテリー", "Energia e bateria", "电源和电池", "ms-settings:powersleep", "M12 3v9M6.3 6.8a8 8 0 1 0 11.4 0"),
        ("Центр обновления Windows", "Windows Update", "Windows Update", "Windows Update", "Windows 更新", "ms-settings:windowsupdate", "M4 12a8 8 0 1 0 2.5-5.8M4 4v5h5"),
        ("Конфиденциальность", "Privacy", "プライバシー", "Privacidade", "隐私", "ms-settings:privacy", "M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3"),
        ("Память", "Storage", "記憶域", "Armazenamento", "存储", "ms-settings:storagesense", "M4 6h16v5H4zM4 13h16v5H4zM7.5 8.5h.01M7.5 15.5h.01"),
        ("О системе", "About", "バージョン情報", "Sobre", "关于", "ms-settings:about", "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01"),
    ];
    let generic = t!("Параметры Windows", "Windows Settings", ja: "Windows の設定", pt: "Configurações do Windows", zh: "Windows 设置");
    let (open, toggle) = (t!("Открыть", "Open", ja: "開く", pt: "Abrir", zh: "打开"), t!("Переключить", "Toggle", ja: "切り替え", pt: "Alternar", zh: "切换"));
    let onoff = |s: Option<bool>| match s {
        Some(true) => t!("Вкл.", "On", ja: "オン", pt: "Ligado", zh: "开").to_string(),
        Some(false) => t!("Выкл.", "Off", ja: "オフ", pt: "Desligado", zh: "关").to_string(),
        None => generic.to_string(),
    };
    pages
        .into_iter()
        .filter(|p| [p.0, p.1, p.2, p.3, p.4].iter().any(|n| n.to_lowercase().contains(q)))
        .take(PER_GROUP)
        .map(|(ru, en, ja, pt, zh, uri, d)| {
            let name = t!(ru, en, ja: ja, pt: pt, zh: zh);
            let (sub, action, run) = match uri {
                "ms-settings:nightlight" => (onoff(crate::quick::night_light()), toggle, Run::NightLight),
                "ms-settings:bluetooth" => (onoff(crate::quick::bluetooth()), toggle, Run::Bluetooth),
                "ms-settings:sound" => (format!("{} {}%", t!("Громкость", "Volume", ja: "音量", pt: "Volume", zh: "音量"), crate::VOL.get().0.max(0)), open, Run::Uri(uri)),
                _ => (generic.to_string(), open, Run::Uri(uri)),
            };
            Row { name: name.to_string(), sub, action, glyph: Glyph::Path(d), run }
        })
        .collect()
}

impl Content for SearchPanel {
    fn id(&self) -> Id {
        Id::Search
    }

    fn look(&self) -> Look {
        Look::glass(22.0, 0.82, Shadow { dy: 40.0, blur: 100.0, color: Rgba::css(0, 0, 0, 0.6) }).dimming(Scrim { color: Rgba::css(5, 7, 12, 0.35), fade_in: 160, fade_out: 180 })
    }

    fn place(&self) -> Place {
        Place { along: Along::Top(TOP), gap: 12.0 }
    }

    /// `srBoxIn`: from 18 px up, at .95, transparent, in .3 s.
    fn entry(&self) -> Entry {
        Entry { from: Pose { opacity: 0.0, dx: 0.0, dy: -18.0, k: 0.95, clip: None }, dur: 300, ease: POP, origin: Origin::Frac(0.5, 0.5) }
    }

    /// `srOut`: it fades in .18 s.
    fn exit(&self) -> Exit {
        Exit { to: Pose { opacity: 0.0, ..Pose::REST }, dur: 180, ease: EASE, origin: Origin::Frac(0.5, 0.5) }
    }

    fn size(&mut self, _g: &mut Gfx) -> (f32, f32) {
        (W, self.max_height())
    }

    fn extent(&self) -> Option<f32> {
        Some(self.height)
    }

    fn typing(&self) -> bool {
        true
    }

    /// The caret blinks all the while the box is open: a frame every 106 ms (a fifth of the blink) shows it, and finds what the
    /// service answered meanwhile; a frame on every tick of the screen for that would repaint the box 360 times a second.
    fn pulse(&self) -> u32 {
        106
    }

    /// A switch of the list is waiting for its re-read.
    fn animating(&self, _now: u64) -> bool {
        self.reread_at != 0
    }

    fn draw(&mut self, c: &mut Canvas<'_>, f: &mut Frame<'_>) {
        // the files the service found since the last frame, a switch that settled
        let answered = FILES.lock().unwrap().0 != self.files_seen && !self.q().is_empty();
        let settled = self.reread_at != 0 && f.now >= self.reread_at;
        if settled {
            self.reread_at = 0;
        }
        if answered || settled {
            self.rebuild();
        }
        if f.hover != self.last_hover {
            self.last_hover = f.hover;
            if let Some(h) = f.hover.filter(|&h| (h as usize) < self.row_count()) {
                self.sel = h as usize;
            }
        }
        let accent = Rgba::colorref(pal().accent);

        // the field: the magnifier in the accent, the text at 20 px, the two keys that open WinRust's own search
        c.icon(super::icons::SEARCH, (PAD + 16.0 + 11.0, PAD + HEAD / 2.0), 22.0, 2.0, accent, 0.0);
        let kf = Font::new(11.0, 500);
        let mut x = W - PAD - 12.0;
        for key in ["Space", "Alt"] {
            let w = c.measure(key, kf) + 12.0;
            x -= w;
            kbd(c, Rect::new(x, PAD + HEAD / 2.0 - 8.65, w, 17.3), key);
            x -= 4.0;
        }
        let tx = PAD + 16.0 + 22.0 + 14.0;
        let field = Rect::new(tx, PAD + 6.0, x - 8.0 - tx, 44.0);
        let font = Font::new(20.0, 400);
        if self.query.is_empty() {
            c.text_fit(t!("Поиск приложений, файлов, параметров и команд", "Search apps, files, settings or run a command", ja: "アプリ、ファイル、設定を検索、またはコマンドを実行", pt: "Pesquisar apps, arquivos, configurações ou executar um comando", zh: "搜索应用、文件、设置或运行命令"), field, font, PLACEHOLDER, Align::Start);
        } else {
            if self.focus_all {
                let w = c.measure(&self.query, font).min(field.w);
                c.fill(Rect::new(tx, field.cy() - 12.0, w, 24.0), 2.0, accent.alpha(0.45));
            }
            c.text_fit(&self.query, field, font, INK, Align::Start);
        }
        if (f.now / 530).is_multiple_of(2) {
            let w = c.measure(&self.query, font).min(field.w);
            c.fill(Rect::new(tx + w, field.cy() - 12.0, 1.0, 24.0), 0.0, accent);
        }

        if self.row_count() > 0 {
            c.fill(Rect::new(PAD + 8.0, PAD + HEAD + 4.0, W - 2.0 * PAD - 16.0, 1.0), 0.0, Rgba::white(0.07));
            for &(y, title) in &self.captions {
                c.text(title, Rect::new(PAD + 12.0, y + 8.0, 300.0, 14.5), Font::new(12.0, 500), DIM, Align::Start);
            }
            let rows: Vec<Row> = self.flat().cloned().collect();
            for (i, row) in rows.iter().enumerate() {
                self.draw_row(c, f, i, row, self.rects[i]);
            }
        } else if let Some(y) = self.run_row {
            let r = Rect::new(PAD, y, W - 2.0 * PAD, ROW);
            c.fill(r, 12.0, Rgba::white(0.09));
            let tile = Rect::new(r.x + 12.0, r.cy() - 16.0, 32.0, 32.0);
            c.fill(tile, 9.0, Rgba::white(0.08));
            c.icon(TERMINAL, (tile.cx(), tile.cy()), 18.0, 1.8, INK, 0.0);
            let tx = tile.x + 44.0;
            let name = tf!("Запустить «{}»", "Run “{}”", ja: "「{}」を実行", pt: "Executar “{}”", zh: "运行“{}”", self.query.trim());
            c.text_fit(&name, Rect::new(tx, r.cy() - 16.0, r.w - 44.0 - 12.0 - 70.0 - 12.0, 15.7), Font::new(13.0, 500), INK, Align::Start);
            c.text(t!("Команда", "Command", ja: "コマンド", pt: "Comando", zh: "命令"), Rect::new(tx, r.cy() + 0.5, 200.0, 14.5), Font::new(12.0, 400), DIM, Align::Start);
            c.text("Enter", Rect::new(r.x + r.w - 12.0 - 80.0, r.y, 80.0, r.h), Font::new(12.0, 400), MUTED, Align::End);
        }
        self.draw_foot(c);
    }

    fn hit(&self, x: f32, y: f32) -> Option<i32> {
        self.rects.iter().position(|r| r.contains(x, y)).map(|i| i as i32)
    }

    fn click(&mut self, id: i32, _b: Button, cx: &mut Cx) {
        self.act(Some(id as usize), false, cx);
    }

    fn enter(&mut self, _hover: Option<i32>, mods: Mods, cx: &mut Cx) {
        let admin = mods.ctrl && mods.shift;
        let idx = (self.row_count() > 0).then_some(self.sel);
        self.act(idx, admin, cx);
    }

    fn nav(&self, _from: Option<i32>, dir: Dir) -> Option<i32> {
        let n = self.row_count();
        if n == 0 {
            return None;
        }
        let i = match dir {
            Dir::Up | Dir::Left => self.sel.saturating_sub(1),
            Dir::Down | Dir::Right => (self.sel + 1).min(n - 1),
            Dir::First => 0,
            Dir::Last => n - 1,
        };
        Some(i as i32)
    }

    fn text(&mut self, ch: char, _cx: &mut Cx) {
        let all = std::mem::replace(&mut self.focus_all, false);
        let mut q = std::mem::take(&mut self.query);
        match ch {
            '\u{1}' => self.focus_all = !q.is_empty(),
            '\u{7f}' => {
                if all {
                    q.clear();
                } else {
                    let end = q.trim_end().len();
                    q.truncate(q[..end].rfind(' ').map_or(0, |i| i + 1));
                }
            }
            '\u{8}' => {
                if all {
                    q.clear();
                } else {
                    q.pop();
                }
            }
            ch => {
                if all {
                    q.clear();
                }
                q.push(ch);
            }
        }
        self.query = q;
        self.sel = 0;
        apps::find_loose_exes(&self.query);
        ask_files(&self.query);
        self.rebuild();
    }

    fn escape(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.focus_all = false;
        self.query.clear();
        self.sel = 0;
        ask_files("");
        self.rebuild();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_field_lists_commands_and_recent_files_and_a_miss_offers_to_run_what_was_typed() {
        let mut p = SearchPanel::with_query(900.0, "", Vec::new(), Vec::new());
        assert!(p.row_count() >= 1, "the commands are listed");
        p.query = "zzzzqq nothing".into();
        *FILES.lock().unwrap() = (String::new(), Vec::new());
        p.rebuild();
        assert_eq!(p.row_count(), 0);
        assert!(p.run_row.is_some(), "a row to run the typed command");
    }

    #[test]
    fn the_list_never_reaches_below_the_screen() {
        let p = SearchPanel::with_query(900.0, "", Vec::new(), Vec::new());
        assert!(TOP + p.max_height() <= 900.0, "{}", p.max_height());
        assert_eq!(p.max_rows, 10);
    }

    #[test]
    fn settings_match_in_any_language() {
        let names = |q: &str| settings(q).into_iter().map(|r| r.name).collect::<Vec<_>>();
        assert_eq!(names("bluetooth").len(), 1);
        assert!(!names("night").is_empty() && !names("ночной").is_empty());
        assert!(names("qqqq").is_empty());
    }
}
