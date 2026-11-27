//! The numbers of the Performance page: what each device of the list has to show (the sparkline's lines, the big chart's, the facts).
//! They are worked out from [`Data`] and carry no drawing; `draw.rs` draws them.
use crate::{data::*, model::*, ui::*};
use std::collections::{HashMap, VecDeque};

/// A colour of a chart's line; the screen takes its own (the design's) per device, this is what a line is told to be.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn lerp(self, o: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(m(self.0, o.0), m(self.1, o.1), m(self.2, o.2))
    }
}

pub fn series_color(i: usize, dark: bool) -> Rgb {
    let c = [Rgb(10, 132, 255), Rgb(94, 92, 230), Rgb(48, 209, 88), Rgb(255, 159, 10), Rgb(191, 90, 242)][i.min(4)]; // Apple system blue, indigo, green, orange, purple
    if dark { c } else { c.lerp(Rgb(0, 0, 0), 0.18) }
}


pub struct GraphSpec<'a> {
    pub label: String,
    pub right: String,
    pub series: Vec<(&'a VecDeque<f32>, Rgb)>,
    pub ymax: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum GLayout {
    Single,
    Stack,
    Grid,
}

/// Какой ресурс показывает пункт списка слева.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PerfKind {
    Cpu,
    Mem,
    Disk(usize),
    Nic(usize),
    Gpu(usize),
}

/// Пункты списка: ЦП, память, каждый физический диск, каждый сетевой адаптер, каждая видеокарта.
pub fn perf_kinds(d: &Data) -> Vec<PerfKind> {
    let mut v = vec![PerfKind::Cpu, PerfKind::Mem];
    v.extend((0..d.disks.len()).map(PerfKind::Disk));
    v.extend((0..d.nics.len()).map(PerfKind::Nic));
    v.extend((0..d.gpus.len()).map(PerfKind::Gpu));
    v
}

/// Элемент списка ресурсов слева на странице «Производительность» и данные для правой части.
pub struct PerfItem<'a> {
    pub kind: PerfKind,
    pub title: String,
    pub line1: String,
    pub line2: String,
    pub graphs: Vec<GraphSpec<'a>>,
    #[allow(dead_code)] // the design draws one chart a device; the other layouts stay in the numbers
    pub layout: GLayout,
    pub big_title: String,
    pub device: String,
    pub stats: Vec<(String, String)>,
    #[allow(dead_code)]
    pub stat_cols: usize,
    pub facts: Vec<(String, String)>,
    #[allow(dead_code)]
    pub composition: Option<(String, Vec<(f32, Rgb)>)>,
}

fn gb(b: u64) -> String {
    tf!("{} ГБ", "{} GB", ja: "{} GB", pt: "{} GB", zh: "{} GB", dec(b as f64 / 1_073_741_824.0, 1))
}

fn mb_cache(b: u64) -> String {
    if b >= 1 << 20 { tf!("{} МБ", "{} MB", ja: "{} MB", pt: "{} MB", zh: "{} MB", dec(b as f64 / 1_048_576.0, 1)) } else { tf!("{} КБ", "{} KB", ja: "{} KB", pt: "{} KB", zh: "{} KB", b >> 10) }
}

/// Название подключения по имени адаптера: «Wi-Fi», «Ethernet», «Bluetooth» …
pub fn conn_name(adapter: &str) -> &'static str {
    let l = adapter.to_lowercase();
    if l.contains("wi-fi") || l.contains("wireless") || l.contains("wlan") || l.contains("802.11") {
        "Wi-Fi"
    } else if l.contains("bluetooth") {
        "Bluetooth"
    } else if l.contains("vmware") || l.contains("virtual") || l.contains("hyper-v") || l.contains("vethernet") || l.contains("vpn") || l.contains("tap-") {
        t!("Виртуальный адаптер", "Virtual adapter", ja: "仮想アダプター", pt: "Adaptador virtual", zh: "虚拟适配器")
    } else {
        "Ethernet"
    }
}

pub fn perf_items(d: &Data) -> Vec<PerfItem<'_>> {
    let dark = d.cfg.theme != 0 && (d.cfg.theme == 1 || crate::sys::system_dark());
    let col = |i| series_color(i, dark);
    let s = &d.series;
    let last = |q: &VecDeque<f32>| q.back().copied().unwrap_or(0.0);
    let si = &d.sysinfo;
    let speed = si.base_mhz as f32 * d.sample.cpu_perf.max(1.0) / 100.0 / 1000.0;
    let total = d.sys.total_memory();
    let used = d.sys.used_memory();
    let cpu_brand = d.sys.cpus().first().map_or(String::new(), |c| c.brand().to_string());
    let m = &d.meminfo;
    let ms = |x: f64| tf!("{} МБ/с", "{} MB/s", ja: "{} MB/秒", pt: "{} MB/s", zh: "{} MB/s", dec(x / 1_048_576.0, 1));
    let gspec = |label: &str, right: &str, q, color, ymax| GraphSpec { label: label.into(), right: right.into(), series: vec![(q, color)], ymax };
    let kinds = perf_kinds(d);
    let mut v = vec![];

    // для одинаковых названий подключений добавляем номер: «Wi-Fi», «Wi-Fi 2»
    let nic_titles: Vec<String> = {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        d.nics
            .iter()
            .map(|n| {
                let c = conn_name(&n.name);
                let k = seen.entry(c).or_insert(0);
                *k += 1;
                if *k > 1 { format!("{c} {k}") } else { c.to_string() }
            })
            .collect()
    };

    for kind in kinds {
        match kind {
            PerfKind::Cpu => v.push(PerfItem {
                kind,
                title: t!("ЦП", "CPU", ja: "CPU", pt: "CPU", zh: "CPU").into(),
                line1: tf!("{:.0}%  {} ГГц", "{:.0}%  {} GHz", ja: "{:.0}%  {} GHz", pt: "{:.0}%  {} GHz", zh: "{:.0}%  {} GHz", last(&s.cpu), dec(speed as f64, 2)),
                line2: String::new(),
                graphs: vec![gspec(t!("% использования", "% utilization", ja: "使用率 (%)", pt: "% de utilização", zh: "% 利用率"), "100%", &s.cpu, col(0), 100.0)],
                layout: GLayout::Single,
                big_title: t!("ЦП", "CPU", ja: "CPU", pt: "CPU", zh: "CPU").into(),
                device: cpu_brand.clone(),
                stats: vec![
                    (t!("Использование", "Utilization", ja: "使用率", pt: "Utilização", zh: "利用率").into(), format!("{:.0}%", last(&s.cpu))),
                    (t!("Скорость", "Speed", ja: "速度", pt: "Velocidade", zh: "速度").into(), tf!("{} ГГц", "{} GHz", ja: "{} GHz", pt: "{} GHz", zh: "{} GHz", dec(speed as f64, 2))),
                    (t!("Процессы", "Processes", ja: "プロセス", pt: "Processos", zh: "进程").into(), m.processes.to_string()),
                    (t!("Потоки", "Threads", ja: "スレッド", pt: "Threads", zh: "线程").into(), m.threads.to_string()),
                    (t!("Дескрипторы", "Handles", ja: "ハンドル", pt: "Identificadores", zh: "句柄").into(), m.handles.to_string()),
                    (t!("Время работы", "Up time", ja: "稼働時間", pt: "Tempo de atividade", zh: "运行时间").into(), fmt_uptime(crate::perf::uptime_secs())),
                ],
                stat_cols: 2,
                facts: vec![
                    (t!("Базовая скорость:", "Base speed:", ja: "基本速度:", pt: "Velocidade base:", zh: "基准速度：").into(), tf!("{} ГГц", "{} GHz", ja: "{} GHz", pt: "{} GHz", zh: "{} GHz", dec(si.base_mhz as f64 / 1000.0, 2))),
                    (t!("Сокетов:", "Sockets:", ja: "ソケット:", pt: "Soquetes:", zh: "插槽：").into(), si.sockets.to_string()),
                    (t!("Ядер:", "Cores:", ja: "コア:", pt: "Núcleos:", zh: "核心数：").into(), sysinfo::System::physical_core_count().unwrap_or(0).to_string()),
                    (t!("Логических процессоров:", "Logical processors:", ja: "論理プロセッサ:", pt: "Processadores lógicos:", zh: "逻辑处理器：").into(), d.sys.cpus().len().to_string()),
                    (t!("Виртуализация:", "Virtualization:", ja: "仮想化:", pt: "Virtualização:", zh: "虚拟化：").into(), if si.virt { t!("Включено", "Enabled", ja: "有効", pt: "Ativada", zh: "已启用") } else { t!("Отключено", "Disabled", ja: "無効", pt: "Desativada", zh: "已禁用") }.into()),
                    (t!("Кэш L1:", "L1 cache:", ja: "L1 キャッシュ:", pt: "Cache L1:", zh: "L1 缓存：").into(), mb_cache(si.l1)),
                    (t!("Кэш L2:", "L2 cache:", ja: "L2 キャッシュ:", pt: "Cache L2:", zh: "L2 缓存：").into(), mb_cache(si.l2)),
                    (t!("Кэш L3:", "L3 cache:", ja: "L3 キャッシュ:", pt: "Cache L3:", zh: "L3 缓存：").into(), mb_cache(si.l3)),
                ],
                composition: None,
            }),

            PerfKind::Mem => {
                let avail = total.saturating_sub(used);
                let mh = &d.hw_mem;
                let kind_txt = mh.kind();
                let reserved = (mh.installed_mb() * 1_048_576).saturating_sub(total);
                let compressed = d.procs.iter().find(|p| p.name.eq_ignore_ascii_case("Memory Compression")).map_or(0, |p| p.mem);
                let cached = if d.sample.mem_modified + d.sample.mem_standby > 0 { d.sample.mem_modified + d.sample.mem_standby } else { m.cache };
                let mut facts: Vec<(String, String)> = vec![];
                if !mh.dimms.is_empty() {
                    facts.push((t!("Скорость:", "Speed:", ja: "速度:", pt: "Velocidade:", zh: "速度：").into(), tf!("{} МТ/с", "{} MT/s", ja: "{} MT/秒", pt: "{} MT/s", zh: "{} MT/s", mh.speed())));
                    facts.push((t!("Использовано слотов:", "Slots used:", ja: "使用中のスロット:", pt: "Slots usados:", zh: "已用插槽：").into(), tf!("{} из {}", "{} of {}", ja: "{} / {}", pt: "{} de {}", zh: "{} / {}", mh.dimms.len(), mh.slots.max(mh.dimms.len() as u32))));
                    facts.push((t!("Форм-фактор:", "Form factor:", ja: "フォーム ファクター:", pt: "Formato:", zh: "外形规格：").into(), mh.form().into()));
                    facts.push((t!("Зарезервировано аппаратурой:", "Hardware reserved:", ja: "ハードウェア予約済み:", pt: "Reservada para hardware:", zh: "硬件保留：").into(), tf!("{} МБ", "{} MB", ja: "{} MB", pt: "{} MB", zh: "{} MB", dec(reserved as f64 / 1_048_576.0, 0))));
                    if !kind_txt.is_empty() {
                        facts.push((t!("Тип:", "Type:", ja: "種類:", pt: "Tipo:", zh: "类型：").into(), kind_txt.into()));
                    }
                    if let Some(d0) = mh.dimms.first().filter(|x| !x.mfr.is_empty()) {
                        facts.push((t!("Производитель:", "Manufacturer:", ja: "製造元:", pt: "Fabricante:", zh: "制造商：").into(), d0.mfr.clone()));
                    }
                }
                let t = total.max(1) as f32;
                let (mut seg, mut parts): (Vec<(f32, Rgb)>, Vec<String>) = (vec![], vec![]);
                seg.push((used as f32 / t, col(1)));
                parts.push(tf!("Используется {}", "In use {}", ja: "使用中 {}", pt: "Em uso {}", zh: "使用中 {}", gb(used)));
                if d.sample.mem_modified > 0 {
                    seg.push((d.sample.mem_modified as f32 / t, col(1).lerp(Rgb(255, 255, 255), 0.25)));
                    parts.push(tf!("Изменено {}", "Modified {}", ja: "変更済み {}", pt: "Modificada {}", zh: "已修改 {}", gb(d.sample.mem_modified)));
                }
                if d.sample.mem_standby > 0 {
                    seg.push(((d.sample.mem_standby.min(avail)) as f32 / t, col(1).lerp(Rgb(128, 128, 128), 0.6)));
                    parts.push(tf!("В режиме ожидания {}", "Standby {}", ja: "スタンバイ {}", pt: "Em espera {}", zh: "备用 {}", gb(d.sample.mem_standby)));
                }
                if d.sample.mem_free > 0 {
                    parts.push(tf!("Свободно {}", "Free {}", ja: "空き {}", pt: "Livre {}", zh: "可用 {}", gb(d.sample.mem_free)));
                }
                v.push(PerfItem {
                    kind,
                    title: t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存").into(),
                    line1: format!("{}/{} ({:.0}%)", dec(used as f64 / 1_073_741_824.0, 1), gb(total), d.mem_pct),
                    line2: String::new(),
                    graphs: vec![gspec(t!("Использование памяти", "Memory usage", ja: "メモリ使用量", pt: "Uso de memória", zh: "内存使用量"), &gb(total), &s.mem, col(1), 100.0)],
                    layout: GLayout::Single,
                    big_title: t!("Память", "Memory", ja: "メモリ", pt: "Memória", zh: "内存").into(),
                    device: if kind_txt.is_empty() { gb(total) } else { format!("{} {}", gb(total), kind_txt) },
                    stats: vec![
                        (if compressed > 0 { t!("Используется (сжато)", "In use (compressed)", ja: "使用中 (圧縮)", pt: "Em uso (comprimida)", zh: "使用中（已压缩）") } else { t!("Используется", "In use", ja: "使用中", pt: "Em uso", zh: "使用中") }.into(), if compressed > 0 { format!("{} ({})", gb(used), gb(compressed)) } else { gb(used) }),
                        (t!("Доступно", "Available", ja: "利用可能", pt: "Disponível", zh: "可用").into(), gb(avail)),
                        (t!("Выделено", "Committed", ja: "コミット済み", pt: "Confirmada", zh: "已提交").into(), format!("{}/{}", dec(m.commit_total as f64 / 1_073_741_824.0, 1), gb(m.commit_limit))),
                        (t!("Кэшировано", "Cached", ja: "キャッシュ済み", pt: "Em cache", zh: "已缓存").into(), gb(cached)),
                        (t!("Выгружаемый пул", "Paged pool", ja: "ページ プール", pt: "Pool paginado", zh: "分页池").into(), tf!("{} МБ", "{} MB", ja: "{} MB", pt: "{} MB", zh: "{} MB", dec(m.paged as f64 / 1_048_576.0, 0))),
                        (t!("Невыгружаемый пул", "Non-paged pool", ja: "非ページ プール", pt: "Pool não paginado", zh: "非分页池").into(), tf!("{} МБ", "{} MB", ja: "{} MB", pt: "{} MB", zh: "{} MB", dec(m.nonpaged as f64 / 1_048_576.0, 0))),
                    ],
                    stat_cols: 2,
                    facts,
                    composition: Some((tf!("Состав памяти: {}", "Memory composition: {}", ja: "メモリの構成: {}", pt: "Composição da memória: {}", zh: "内存构成：{}", parts.join(" · ")), seg)),
                });
            }

            PerfKind::Disk(i) => {
                let dk = &d.disks[i];
                let info = &dk.info;
                let model = info.model.replace('_', " ");
                let kind_txt = match (info.kind.as_str(), info.bus.as_str()) {
                    ("", "") => "—".to_string(),
                    (k, "") => k.to_string(),
                    ("", b) => b.to_string(),
                    (k, b) => format!("{k} ({b})"),
                };
                let title = tf!("Диск {} ({})", "Disk {} ({})", ja: "ディスク {} ({})", pt: "Disco {} ({})", zh: "磁盘 {}（{}）", dk.number, dk.letters.trim());
                let xfer_max = dk.s_rd.iter().chain(dk.s_wr.iter()).cloned().fold(1.0f32, f32::max) * 1.15;
                let system = dk.letters.to_uppercase().contains("C:");
                v.push(PerfItem {
                    kind,
                    title: title.clone(),
                    line1: if info.kind.is_empty() { t!("Локальный диск", "Local disk", ja: "ローカル ディスク", pt: "Disco local", zh: "本地磁盘").into() } else { kind_txt.clone() },
                    line2: format!("{:.0}%", dk.active),
                    graphs: vec![
                        gspec(t!("Время активности", "Active time", ja: "アクティブな時間", pt: "Tempo ativo", zh: "活动时间"), "100%", &dk.s_active, col(2), 100.0),
                        GraphSpec { label: t!("Скорость передачи данных", "Transfer rate", ja: "転送速度", pt: "Taxa de transferência", zh: "传输速率").into(), right: tf!("{} МБ/с", "{} MB/s", ja: "{} MB/秒", pt: "{} MB/s", zh: "{} MB/s", dec(xfer_max as f64, 0)), series: vec![(&dk.s_rd, col(2)), (&dk.s_wr, col(2).lerp(Rgb(255, 255, 255), 0.45))], ymax: xfer_max },
                    ],
                    layout: GLayout::Stack,
                    big_title: title,
                    device: model,
                    stats: vec![
                        (t!("Время активности", "Active time", ja: "アクティブな時間", pt: "Tempo ativo", zh: "活动时间").into(), format!("{:.0}%", dk.active)),
                        (t!("Средний отклик", "Average response time", ja: "平均応答時間", pt: "Tempo médio de resposta", zh: "平均响应时间").into(), tf!("{} мс", "{} ms", ja: "{} ms", pt: "{} ms", zh: "{} ms", dec(dk.ms as f64, 1))),
                        (t!("Скорость чтения", "Read speed", ja: "読み取り速度", pt: "Velocidade de leitura", zh: "读取速度").into(), ms(dk.rd)),
                        (t!("Скорость записи", "Write speed", ja: "書き込み速度", pt: "Velocidade de gravação", zh: "写入速度").into(), ms(dk.wr)),
                    ],
                    stat_cols: 2,
                    facts: vec![
                        (t!("Ёмкость:", "Capacity:", ja: "容量:", pt: "Capacidade:", zh: "容量：").into(), gb(info.capacity.max(dk.formatted))),
                        (t!("Форматированный объём:", "Formatted:", ja: "フォーマット済み:", pt: "Formatada:", zh: "已格式化：").into(), gb(dk.formatted)),
                        (t!("Системный диск:", "System disk:", ja: "システム ディスク:", pt: "Disco do sistema:", zh: "系统盘：").into(), if system { t!("Да", "Yes", ja: "はい", pt: "Sim", zh: "是") } else { t!("Нет", "No", ja: "いいえ", pt: "Não", zh: "否") }.into()),
                        (t!("Файл подкачки:", "Page file:", ja: "ページ ファイル:", pt: "Arquivo de paginação:", zh: "页面文件：").into(), if system { t!("Да", "Yes", ja: "はい", pt: "Sim", zh: "是") } else { t!("Нет", "No", ja: "いいえ", pt: "Não", zh: "否") }.into()),
                        (t!("Тип:", "Type:", ja: "種類:", pt: "Tipo:", zh: "类型：").into(), kind_txt),
                    ],
                    composition: None,
                });
            }

            PerfKind::Nic(i) => {
                let n = &d.nics[i];
                let title = nic_titles[i].clone();
                let (rx, tx) = (last(&n.s_rx), last(&n.s_tx));
                let ymax = n.s_rx.iter().chain(n.s_tx.iter()).cloned().fold(1000.0f32, f32::max) * 1.15;
                let conn = conn_name(&n.name);
                v.push(PerfItem {
                    kind,
                    title: title.clone(),
                    line1: conn.into(),
                    line2: tf!("О: {} П: {} Кбит/с", "S: {} R: {} Kbps", ja: "送: {} 受: {} Kbps", pt: "E: {} R: {} Kbps", zh: "发送：{} 接收：{} Kbps", tx as i64, rx as i64),
                    graphs: vec![GraphSpec { label: t!("Пропускная способность", "Throughput", ja: "スループット", pt: "Taxa de transferência", zh: "吞吐量").into(), right: tf!("{} Кбит/с", "{} Kbps", ja: "{} Kbps", pt: "{} Kbps", zh: "{} Kbps", ymax as i64), series: vec![(&n.s_rx, col(3)), (&n.s_tx, col(3).lerp(Rgb(255, 255, 255), 0.4))], ymax }],
                    layout: GLayout::Single,
                    big_title: title,
                    device: n.name.clone(),
                    stats: vec![(t!("Отправка", "Send", ja: "送信", pt: "Enviar", zh: "发送").into(), tf!("{} Кбит/с", "{} Kbps", ja: "{} Kbps", pt: "{} Kbps", zh: "{} Kbps", tx as i64)), (t!("Получение", "Receive", ja: "受信", pt: "Receber", zh: "接收").into(), tf!("{} Кбит/с", "{} Kbps", ja: "{} Kbps", pt: "{} Kbps", zh: "{} Kbps", rx as i64))],
                    stat_cols: 2,
                    facts: vec![(t!("Имя адаптера:", "Adapter name:", ja: "アダプター名:", pt: "Nome do adaptador:", zh: "适配器名称：").into(), n.name.clone()), (t!("Тип подключения:", "Connection type:", ja: "接続の種類:", pt: "Tipo de conexão:", zh: "连接类型：").into(), conn.into()), (t!("Скорость канала:", "Link speed:", ja: "リンク速度:", pt: "Velocidade do link:", zh: "链接速度：").into(), tf!("{:.0} Мбит/с", "{:.0} Mbps", ja: "{:.0} Mbps", pt: "{:.0} Mbps", zh: "{:.0} Mbps", n.bw / 1e6))],
                    composition: None,
                });
            }

            PerfKind::Gpu(i) => {
                let g = &d.gpus[i];
                let a = &g.adapter;
                let vram = a.dedicated.max(g.reg.vram);
                let shared_total = a.shared;
                let temp = g.temp.map(|t| format!("{} °C", dec(t as f64, 0)));
                let name = a.name.clone();
                let title = format!("GPU {i}");
                let vram_mb = (vram >> 20).max(1) as f32;
                v.push(PerfItem {
                    kind,
                    title: title.clone(),
                    line1: name.clone(),
                    line2: match &temp {
                        Some(t) => format!("{:.0}% ({t})", g.util),
                        None => format!("{:.0}%", g.util),
                    },
                    graphs: vec![
                        gspec("3D", "100%", &g.series.g3d, col(4), 100.0),
                        gspec(t!("Копирование", "Copy", ja: "コピー", pt: "Copiar", zh: "复制"), "100%", &g.series.gcopy, col(4), 100.0),
                        gspec(t!("Видеокодирование", "Video encode", ja: "ビデオ エンコード", pt: "Codificação de vídeo", zh: "视频编码"), "100%", &g.series.genc, col(4), 100.0),
                        gspec(t!("Видеодекодирование", "Video decode", ja: "ビデオ デコード", pt: "Decodificação de vídeo", zh: "视频解码"), "100%", &g.series.gdec, col(4), 100.0),
                        gspec(t!("Использование выделенной памяти GPU", "Dedicated GPU memory usage", ja: "専用 GPU メモリ使用量", pt: "Uso de memória dedicada da GPU", zh: "专用 GPU 内存使用量"), &gb(vram), &g.series.ded, col(4), vram_mb),
                        gspec(t!("Использование общей памяти GPU", "Shared GPU memory usage", ja: "共有 GPU メモリ使用量", pt: "Uso de memória compartilhada da GPU", zh: "共享 GPU 内存使用量"), &gb(shared_total), &g.series.shr, col(4), (shared_total >> 20).max(1) as f32),
                    ],
                    layout: GLayout::Grid,
                    big_title: title,
                    device: name.clone(),
                    stats: vec![
                        (t!("Использование", "Utilization", ja: "使用率", pt: "Utilização", zh: "利用率").into(), format!("{:.0}%", g.util)),
                        (t!("Общая память GPU", "Shared GPU memory", ja: "共有 GPU メモリ", pt: "Memória compartilhada da GPU", zh: "共享 GPU 内存").into(), format!("{}/{}", dec(g.shr_used as f64 / 1_073_741_824.0, 1), gb(shared_total))),
                        (t!("Память GPU", "GPU memory", ja: "GPU メモリ", pt: "Memória da GPU", zh: "GPU 内存").into(), format!("{}/{}", dec((g.ded_used + g.shr_used) as f64 / 1_073_741_824.0, 1), gb(vram + shared_total))),
                        (t!("Температура GPU", "GPU temperature", ja: "GPU 温度", pt: "Temperatura da GPU", zh: "GPU 温度").into(), temp.clone().unwrap_or_else(|| "—".into())),
                    ],
                    stat_cols: 2,
                    facts: {
                        let dash = |s: &str| if s.is_empty() { "—".to_string() } else { s.to_string() };
                        vec![
                            (t!("Выделенная память GPU:", "Dedicated GPU memory:", ja: "専用 GPU メモリ:", pt: "Memória dedicada da GPU:", zh: "专用 GPU 内存：").into(), format!("{}/{}", dec(g.ded_used as f64 / 1_073_741_824.0, 1), gb(vram))),
                            (t!("Версия драйвера:", "Driver version:", ja: "ドライバーのバージョン:", pt: "Versão do driver:", zh: "驱动程序版本：").into(), dash(&g.reg.driver_version)),
                            (t!("Дата разработки:", "Driver date:", ja: "ドライバーの日付:", pt: "Data do driver:", zh: "驱动程序日期：").into(), if g.reg.driver_date.is_empty() { "—".into() } else { ru_date(&g.reg.driver_date) }),
                            (t!("Версия DirectX:", "DirectX version:", ja: "DirectX のバージョン:", pt: "Versão do DirectX:", zh: "DirectX 版本：").into(), dash(&a.dx)),
                            (t!("Физическое расположение:", "Physical location:", ja: "物理的な場所:", pt: "Local físico:", zh: "物理位置：").into(), dash(&a.location)),
                        ]
                    },
                    composition: None,
                });
            }
        }
    }
    v
}

/// «10-19-2026» (месяц-день-год, как хранит драйвер) -> «19.10.2026».
pub fn ru_date(s: &str) -> String {
    let p: Vec<&str> = s.split(['-', '/']).collect();
    if let [m, d, y] = p[..] { format!("{d}.{m}.{y}") } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_endpoints() {
        assert_eq!(Rgb(0, 0, 0).lerp(Rgb(100, 200, 50), 0.0), Rgb(0, 0, 0));
        assert_eq!(Rgb(0, 0, 0).lerp(Rgb(100, 200, 50), 1.0), Rgb(100, 200, 50));
    }

    fn nic(name: &str) -> NicState {
        NicState { name: name.into(), rx: 0.0, tx: 0.0, bw: 1e9, s_rx: Default::default(), s_tx: Default::default() }
    }

    fn disk(number: u32, letters: &str) -> DiskState {
        DiskState {
            number,
            letters: letters.into(),
            info: crate::hw::DiskInfo { number, model: "TEST_DISK".into(), kind: "HDD".into(), bus: "SATA".into(), capacity: 2 << 40, formatted: 0 },
            formatted: 1 << 40,
            active: 0.0,
            rd: 0.0,
            wr: 0.0,
            ms: 0.0,
            s_active: Default::default(),
            s_rd: Default::default(),
            s_wr: Default::default(),
        }
    }

    fn gpu(name: &str, vendor: u32) -> GpuState {
        GpuState {
            adapter: crate::gpu::Adapter { name: name.into(), vendor, luid: (vendor, 0), dedicated: 4 << 30, shared: 8 << 30, dx: "DirectX 12 (FL 12_1)".into(), location: "PCI-шина 0, устройство 2, функция 0".into() },
            reg: Default::default(),
            util: 0.0,
            ded_used: 0,
            shr_used: 0,
            temp: Some(55.0),
            series: Default::default(),
        }
    }

    #[test]
    fn several_disks_adapters_and_gpus_get_their_own_items() {
        let mut d = Data::new(crate::store::Settings::default());
        d.disks = vec![disk(0, "C:"), disk(1, "D: E:")];
        d.nics = vec![nic("Intel Wi-Fi 6 AX200"), nic("Realtek PCIe GbE Family Controller"), nic("Qualcomm Wireless 802.11ac")];
        d.gpus = vec![gpu("Intel UHD Graphics", 0x8086), gpu("AMD Radeon RX", 0x1002)];
        let items = perf_items(&d);
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["ЦП", "Память", "Диск 0 (C:)", "Диск 1 (D: E:)", "Wi-Fi", "Ethernet", "Wi-Fi 2", "GPU 0", "GPU 1"]);
        assert_eq!(perf_kinds(&d).len(), items.len());
        // у каждого GPU — свои сведения: DirectX, расположение и температура (в том числе не NVIDIA)
        let g1 = items.last().unwrap();
        assert!(g1.facts.iter().any(|f| f.0.contains("DirectX") && f.1.contains("12")));
        assert!(g1.facts.iter().any(|f| f.0.contains("расположение") && f.1.contains("PCI")));
        assert!(g1.line2.contains("55 °C"));
        // второй диск — не системный
        let d1 = &items[3];
        assert!(d1.facts.iter().any(|f| f.0.contains("Системный") && f.1 == "Нет"));
        assert_eq!(conn_name("Bluetooth Device (Personal Area Network)"), "Bluetooth");
    }

    #[test]
    fn driver_date_is_converted_to_russian_order() {
        assert_eq!(ru_date("10-19-2026"), "19.10.2026");
        assert_eq!(ru_date("garbage"), "garbage");
    }
}

