//! The modules as the window lists them: the mockup's table (`D` in Installer.html) with its texts in five languages, and which
//! entry of the installer's `PLUGINS` each row is. Pure data.

/// What the switch of a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A plugin of the installer, by its name in `PLUGINS`.
    Plugin(&'static str),
    /// Installed together with another row's plugin (rs-shell comes with the file manager, the hotkeys with the bar): it shows
    /// "Included" in place of a switch.
    With(&'static str),
    /// Nothing to install yet: the row says "Soon".
    Soon,
}

/// The picture of a row's tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    /// `linear-gradient(140deg, a, b)` with a white glyph.
    Gradient(u32, u32),
    /// A plate of the ink with the file manager's folder.
    Folder,
    /// A plate of the ink with the task manager's pulse.
    Pulse,
}

/// The preview picture of a row: a PNG embedded in the exe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shot {
    Bar,
    Desk,
    Keys,
    Explorer,
    Search,
    Tm,
    Term,
    Set,
    Photo,
    Tree,
    Debloat,
    Purge,
}

impl Shot {
    /// The PNG, ~520 × 340, for the dark or the light mode (most pictures are the same in both).
    #[must_use]
    pub fn png(self, light: bool) -> &'static [u8] {
        match self {
            Shot::Bar => include_bytes!("../../assets/installer/bar.png"),
            Shot::Desk => include_bytes!("../../assets/installer/desk.png"),
            Shot::Keys if light => include_bytes!("../../assets/installer/keys-light.png"),
            Shot::Keys => include_bytes!("../../assets/installer/keys.png"),
            Shot::Explorer => include_bytes!("../../assets/installer/explorer.png"),
            Shot::Search => include_bytes!("../../assets/installer/search.png"),
            Shot::Tm => include_bytes!("../../assets/installer/tm.png"),
            Shot::Term => include_bytes!("../../assets/installer/term.png"),
            Shot::Set => include_bytes!("../../assets/installer/set.png"),
            Shot::Photo => include_bytes!("../../assets/installer/photo.png"),
            Shot::Tree => include_bytes!("../../assets/installer/tree.png"),
            Shot::Debloat => include_bytes!("../../assets/installer/debloat.png"),
            Shot::Purge => include_bytes!("../../assets/installer/purge.png"),
        }
    }

    /// CSS `object-position` y of the picture in its card (0 top, 1 bottom).
    #[must_use]
    pub fn pos_y(self) -> f32 {
        match self {
            Shot::Bar => 1.0,
            Shot::Search => 0.4,
            _ => 0.5,
        }
    }
}

/// A row of the list.
pub struct Row {
    /// The mockup's id (`topbar`, `shell`, `reshell`, ...), used by the demo states.
    pub id: &'static str,
    /// The program's name, in the list and under the preview.
    pub name: &'static str,
    /// 0 shell, 1 apps, 2 system.
    pub group: u8,
    /// `Win E`: shown as a key cap, and as `Win + E` under the preview.
    pub key: &'static str,
    pub title: &'static str,
    /// The line under the name, and the paragraph under the preview, in the order of `Lang`: en, ru, ja, pt, zh.
    pub short: [&'static str; 5],
    pub long: [&'static str; 5],
    /// A 24 × 24 stroked icon.
    pub icon: &'static str,
    pub tile: Tile,
    pub shot: Shot,
    pub size_mb: f32,
    pub ram: &'static str,
    pub replaces: Option<[&'static str; 5]>,
    pub new: bool,
    pub source: Source,
}

const GEAR: &str = "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z";

/// The file manager's folder and the task manager's pulse, for the finished page's chips and the rows' plates (the mockup shows the
/// programs' own icons there).
pub const FOLDER_BACK: &str = "M2.5 6.5a1.5 1.5 0 0 1 1.5-1.5h5l2 2h9a1.5 1.5 0 0 1 1.5 1.5V18a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 18z";
pub const FOLDER_FRONT: &str = "M2.5 9.5A1.5 1.5 0 0 1 4 8h16a1.5 1.5 0 0 1 1.5 1.5V18a1.5 1.5 0 0 1-1.5 1.5H4A1.5 1.5 0 0 1 2.5 18z";
pub const PULSE_BOX: &str = "M6 3.5h12A3.5 3.5 0 0 1 21.5 7v10a3.5 3.5 0 0 1-3.5 3.5H6A3.5 3.5 0 0 1 2.5 17V7A3.5 3.5 0 0 1 6 3.5z";
pub const PULSE_LINE: &str = "M5 13h3.2l2-5 3.4 9 2-4H19";

pub static ROWS: [Row; 12] = [
    Row {
        id: "topbar",
        name: "topbar",
        group: 0,
        key: "",
        title: "Bar",
        short: ["Bottom bar: windows, clock, volume, menus", "Нижняя панель: окна, часы, громкость, меню", "下部バー: ウィンドウ、時計、音量、メニュー", "Barra inferior: janelas, relógio, volume, menus", "底部栏：窗口、时钟、音量、菜单"],
        long: [
            "The bar at the bottom of the screen: pinned and running apps, quick settings, calendar, language and tray. Themes, position and size are set in Bar settings.",
            "Панель внизу экрана: закреплённые и открытые приложения, быстрые настройки, календарь, язык и трей. Тема, положение и размер — в настройках панели.",
            "画面下部のバー: ピン留めと実行中のアプリ、クイック設定、カレンダー、言語、トレイ。テーマ、位置、サイズはバーの設定で変更できます。",
            "A barra na parte inferior da tela: apps fixados e em execução, configurações rápidas, calendário, idioma e bandeja. Tema, posição e tamanho são definidos nas configurações da barra.",
            "屏幕底部的栏：固定和正在运行的应用、快速设置、日历、语言和托盘。主题、位置和大小可在栏设置中调整。",
        ],
        icon: "M3 5h18v14H3zM3 15h18",
        tile: Tile::Gradient(0x3b82f6, 0x1e40af),
        shot: Shot::Bar,
        size_mb: 2.4,
        ram: "6 MB",
        replaces: Some(["Taskbar", "Панель задач", "タスクバー", "Barra de tarefas", "任务栏"]),
        new: false,
        source: Source::Plugin("topbar"),
    },
    Row {
        id: "shell",
        name: "rs-shell",
        group: 0,
        key: "",
        title: "Shell",
        short: ["Runs WinRust as your shell, keeps the bar alive", "Запускает WinRust как оболочку и следит за панелью", "WinRust をシェルとして実行し、バーを維持します", "Executa a WinRust como seu shell e mantém a barra ativa", "将 WinRust 作为外壳运行，并保持栏存活"],
        long: [
            "Starts the wallpaper, startup apps and the bar at sign-in and restarts the bar if it ever crashes. If it keeps crashing, Explorer comes back as a fallback.",
            "При входе запускает обои, автозагрузку и панель и перезапускает панель, если она упала. Если падает снова и снова — возвращается Проводник.",
            "サインイン時に壁紙、スタートアップ アプリ、バーを起動し、バーが落ちた場合は再起動します。落ち続ける場合は、フォールバックとしてエクスプローラーに戻ります。",
            "Inicia o plano de fundo, os apps de inicialização e a barra no login e reinicia a barra se ela travar. Se continuar travando, o Explorer volta como alternativa.",
            "登录时启动壁纸、启动项和栏；栏崩溃时会自动重启。如果持续崩溃，则回退到资源管理器。",
        ],
        icon: "M12 3l8 4v10l-8 4-8-4V7zM12 12l8-4M12 12v9M12 12L4 8",
        tile: Tile::Gradient(0x64748b, 0x334155),
        shot: Shot::Desk,
        size_mb: 1.2,
        ram: "1.3 MB",
        replaces: Some(["explorer.exe", "explorer.exe", "explorer.exe", "explorer.exe", "explorer.exe"]),
        new: false,
        source: Source::With("rust-explorer"),
    },
    Row {
        id: "reshell",
        name: "reshell",
        group: 0,
        key: "",
        title: "Hotkeys",
        short: ["Desktop hotkeys for desktops, Settings and files", "Горячие клавиши рабочего стола", "デスクトップ、設定、ファイル用のホットキー", "Atalhos para áreas de trabalho, Configurações e arquivos", "用于桌面、设置和文件的快捷键"],
        long: [
            "Keyboard shortcuts that work everywhere: switch virtual desktops, open Settings and the file manager, show the desktop, clipboard history.",
            "Сочетания, которые работают везде: переключение рабочих столов, Параметры, файловый менеджер, показ рабочего стола, история буфера.",
            "どこでも使えるキーボード ショートカット: 仮想デスクトップの切り替え、設定とファイル マネージャーの起動、デスクトップの表示、クリップボード履歴。",
            "Atalhos de teclado que funcionam em qualquer lugar: alternar áreas de trabalho virtuais, abrir Configurações e o gerenciador de arquivos, mostrar a área de trabalho, histórico da área de transferência.",
            "随处可用的键盘快捷键：切换虚拟桌面、打开设置和文件管理器、显示桌面、剪贴板历史。",
        ],
        icon: "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8",
        tile: Tile::Gradient(0x8b5cf6, 0x5b21b6),
        shot: Shot::Keys,
        size_mb: 0.9,
        ram: "0.8 MB",
        replaces: None,
        new: false,
        source: Source::With("topbar"),
    },
    Row {
        id: "explorer",
        name: "rust-explorer",
        group: 0,
        key: "Win E",
        title: "File manager",
        short: ["Tabbed file manager that can replace Explorer", "Файловый менеджер с вкладками", "エクスプローラーを置き換えられるタブ付きファイル マネージャー", "Gerenciador de arquivos com abas que pode substituir o Explorer", "带标签页、可取代资源管理器的文件管理器"],
        long: [
            "Tabs, a preview pane, search across folders, drag and drop and real progress for copy jobs. Opens in milliseconds.",
            "Вкладки, панель просмотра, поиск по папкам, перетаскивание и понятный прогресс копирования. Открывается за миллисекунды.",
            "タブ、プレビュー ウィンドウ、フォルダーをまたぐ検索、ドラッグ アンド ドロップ、コピー ジョブの実際の進行状況。ミリ秒で開きます。",
            "Abas, painel de visualização, pesquisa entre pastas, arrastar e soltar e progresso real das cópias. Abre em milissegundos.",
            "标签页、预览窗格、跨文件夹搜索、拖放，以及真实的复制进度。毫秒级打开。",
        ],
        icon: "M3.5 7.5V18h17V9h-9l-2-2h-6z",
        tile: Tile::Folder,
        shot: Shot::Explorer,
        size_mb: 3.1,
        ram: "9 MB",
        replaces: Some(["File Explorer", "Проводник", "エクスプローラー", "Explorador de Arquivos", "文件资源管理器"]),
        new: false,
        source: Source::Plugin("rust-explorer"),
    },
    Row {
        id: "search",
        name: "rust-search",
        group: 0,
        key: "Alt Space",
        title: "Search",
        short: ["File search across all NTFS drives", "Поиск файлов на всех NTFS-дисках", "すべての NTFS ドライブでのファイル検索", "Pesquisa de arquivos em todas as unidades NTFS", "覆盖所有 NTFS 驱动器的文件搜索"],
        long: [
            "Finds files on every NTFS drive as you type, launches apps and runs commands. Reads the drive index directly, so there is nothing to wait for.",
            "Ищет файлы на всех NTFS-дисках прямо при вводе, запускает приложения и команды. Читает индекс диска напрямую, ждать нечего.",
            "入力に合わせてすべての NTFS ドライブからファイルを検索し、アプリやコマンドを起動します。ドライブのインデックスを直接読み取るため、待たされません。",
            "Encontra arquivos em todas as unidades NTFS enquanto você digita, abre apps e executa comandos. Lê o índice da unidade diretamente, então não há espera.",
            "输入时即可在所有 NTFS 驱动器上查找文件，还能启动应用和运行命令。直接读取磁盘索引，无需等待。",
        ],
        icon: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4",
        tile: Tile::Gradient(0x0ea5e9, 0x0369a1),
        shot: Shot::Search,
        size_mb: 1.8,
        ram: "12 MB",
        replaces: Some(["Windows Search", "Поиск Windows", "Windows 検索", "Pesquisa do Windows", "Windows 搜索"]),
        new: false,
        source: Source::Plugin("rust-search"),
    },
    Row {
        id: "taskmgr",
        name: "rtaskmgr",
        group: 1,
        key: "Ctrl Shift Esc",
        title: "Task Manager",
        short: ["Processes, performance and the rs-du disk cleaner", "Процессы, производительность и очистка rs-du", "プロセス、パフォーマンス、ディスク クリーナー rs-du", "Processos, desempenho e o limpador de disco rs-du", "进程、性能和磁盘清理工具 rs-du"],
        long: [
            "Processes grouped by app with suspicious ones flagged, live performance graphs, startup apps, services and speed profiles.",
            "Процессы по приложениям с пометкой подозрительных, живые графики, автозагрузка, службы и профили скорости.",
            "アプリごとにまとめたプロセス (不審なものは警告表示)、リアルタイムのパフォーマンス グラフ、スタートアップ アプリ、サービス、速度プロファイル。",
            "Processos agrupados por app com os suspeitos sinalizados, gráficos de desempenho em tempo real, apps de inicialização, serviços e perfis de velocidade.",
            "按应用分组的进程（可疑进程会被标记）、实时性能图表、启动项、服务和速度配置。",
        ],
        icon: "M3 12h4l2.5-6 4 12 2.5-6H21",
        tile: Tile::Pulse,
        shot: Shot::Tm,
        size_mb: 2.6,
        ram: "5 MB",
        replaces: Some(["Task Manager", "Диспетчер задач", "タスク マネージャー", "Gerenciador de Tarefas", "任务管理器"]),
        new: false,
        source: Source::Plugin("rtaskmgr-win32"),
    },
    Row {
        id: "term",
        name: "rs-term",
        group: 1,
        key: "Ctrl Alt T",
        title: "Terminal",
        short: ["Tabs, split panes, PowerShell, cmd and WSL", "Вкладки, панели, PowerShell, cmd и WSL", "タブ、分割ペイン、PowerShell、cmd、WSL", "Abas, painéis divididos, PowerShell, cmd e WSL", "标签页、分屏、PowerShell、cmd 和 WSL"],
        long: [
            "A GPU-light terminal with tabs and split panes, profiles for PowerShell, cmd and WSL, smart completions and a command palette.",
            "Лёгкий терминал с вкладками и разделением, профилями PowerShell, cmd и WSL, умными подсказками и палитрой команд.",
            "タブと分割ペイン、PowerShell・cmd・WSL のプロファイル、スマートな補完、コマンド パレットを備えた軽量ターミナル。",
            "Um terminal leve com abas e painéis divididos, perfis para PowerShell, cmd e WSL, sugestões inteligentes e uma paleta de comandos.",
            "轻量的终端，支持标签页和分屏，提供 PowerShell、cmd 和 WSL 配置文件、智能补全以及命令面板。",
        ],
        icon: "M4 5h16v14H4zM7.5 10l3 2.5-3 2.5M13 15h4",
        tile: Tile::Gradient(0x334155, 0x0f172a),
        shot: Shot::Term,
        size_mb: 3.4,
        ram: "14 MB",
        replaces: Some(["Windows Terminal", "Windows Terminal", "Windows Terminal", "Windows Terminal", "Windows Terminal"]),
        new: true,
        source: Source::Soon,
    },
    Row {
        id: "settings",
        name: "rust-settings",
        group: 1,
        key: "Win I",
        title: "Settings",
        short: ["Display, sound, network, Bluetooth and more", "Экран, звук, сеть, Bluetooth и другое", "ディスプレイ、サウンド、ネットワーク、Bluetooth など", "Tela, som, rede, Bluetooth e mais", "显示、声音、网络、蓝牙等"],
        long: [
            "The settings people change every day in one fast window: displays, sound devices, Wi-Fi and Ethernet, Bluetooth, power, apps and updates.",
            "Ежедневные настройки в одном быстром окне: экраны, звук, Wi‑Fi и Ethernet, Bluetooth, питание, приложения и обновления.",
            "毎日使う設定を 1 つの高速なウィンドウに: ディスプレイ、サウンド デバイス、Wi-Fi とイーサネット、Bluetooth、電源、アプリ、更新。",
            "As configurações que as pessoas mudam todo dia em uma janela rápida: telas, dispositivos de som, Wi-Fi e Ethernet, Bluetooth, energia, apps e atualizações.",
            "把日常常用的设置集中在一个快速的窗口里：显示器、声音设备、Wi-Fi 和以太网、蓝牙、电源、应用和更新。",
        ],
        icon: GEAR,
        tile: Tile::Gradient(0x6b7280, 0x374151),
        shot: Shot::Set,
        size_mb: 2.9,
        ram: "7 MB",
        replaces: Some(["Settings", "Параметры", "設定", "Configurações", "设置"]),
        new: true,
        source: Source::Plugin("rust-settings"),
    },
    Row {
        id: "photo",
        name: "photoview",
        group: 1,
        key: "",
        title: "Photo viewer",
        short: ["Fast photo viewer, added to Open with", "Быстрый просмотр фото, пункт «Открыть с помощью»", "高速なフォト ビューアー。「プログラムから開く」に追加されます", "Visualizador de fotos rápido, adicionado a Abrir com", "快速的图片查看器，会加入“打开方式”"],
        long: [
            "Opens pictures instantly, flips through the folder with the arrow keys, zooms to the cursor and shows details and colors.",
            "Открывает фото мгновенно, листает папку стрелками, увеличивает к курсору и показывает сведения и цвета.",
            "写真を一瞬で開き、矢印キーでフォルダー内を切り替え、カーソル位置にズームし、詳細情報と色を表示します。",
            "Abre fotos na hora, percorre a pasta com as setas, dá zoom no cursor e mostra detalhes e cores.",
            "瞬间打开图片，用方向键浏览文件夹，以光标为中心缩放，并显示详细信息和颜色。",
        ],
        icon: "M4 18l5-6 4 4 3-3 4 5M15.5 8.5h.01",
        tile: Tile::Gradient(0xf59e0b, 0xdb2777),
        shot: Shot::Photo,
        size_mb: 1.1,
        ram: "4 MB",
        replaces: Some(["Photos", "Фотографии", "フォト", "Fotos", "照片"]),
        new: false,
        source: Source::Plugin("photoview"),
    },
    Row {
        id: "wiztree",
        name: "wiztree",
        group: 1,
        key: "",
        title: "Disk map",
        short: ["See what takes up space on a drive", "Карта занятого места на диске", "ドライブの使用領域をマップで表示", "Veja o que ocupa espaço em uma unidade", "查看磁盘空间被什么占用"],
        long: [
            "Scans a drive in seconds and draws it as a map, so the biggest folders and files stand out.",
            "Сканирует диск за секунды и рисует его картой — сразу видно самые большие папки и файлы.",
            "ドライブを数秒でスキャンしてマップとして描画し、最も大きいフォルダーとファイルがひと目で分かります。",
            "Examina uma unidade em segundos e a desenha como um mapa, destacando as maiores pastas e arquivos.",
            "几秒内扫描磁盘并绘制成地图，最大的文件夹和文件一目了然。",
        ],
        icon: "M4 4h9v10H4zM15 4h5v6h-5zM15 12h5v8h-5zM4 16h9v4H4z",
        tile: Tile::Gradient(0x10b981, 0x047857),
        shot: Shot::Tree,
        size_mb: 1.6,
        ram: "—",
        replaces: None,
        new: false,
        source: Source::Plugin("wiztree"),
    },
    Row {
        id: "debloat",
        name: "debloat",
        group: 2,
        key: "",
        title: "Debloat",
        short: ["Turns off telemetry, Copilot, Recall and Start ads", "Отключает телеметрию, Copilot, Recall и рекламу", "テレメトリ、Copilot、Recall、スタートの広告をオフにします", "Desativa telemetria, Copilot, Recall e anúncios no Iniciar", "关闭遥测、Copilot、Recall 和开始菜单广告"],
        long: [
            "Switches off data collection, Copilot, Recall, suggested apps and ads in Start. Every change is listed and can be undone.",
            "Выключает сбор данных, Copilot, Recall, предлагаемые приложения и рекламу в «Пуске». Все изменения видны и обратимы.",
            "データ収集、Copilot、Recall、おすすめアプリ、スタートの広告をオフにします。すべての変更が一覧表示され、元に戻せます。",
            "Desativa a coleta de dados, o Copilot, o Recall, os apps sugeridos e os anúncios no Iniciar. Toda alteração é listada e pode ser desfeita.",
            "关闭数据收集、Copilot、Recall、推荐应用和开始菜单中的广告。每项更改都会列出，并且可以撤销。",
        ],
        icon: "M12 3l7 3v5c0 5-3 8-7 10-4-2-7-5-7-10V6z",
        tile: Tile::Gradient(0xef4444, 0x991b1b),
        shot: Shot::Debloat,
        size_mb: 0.4,
        ram: "—",
        replaces: None,
        new: false,
        source: Source::Plugin("debloat"),
    },
    // The mockup's text calls this "clears temp files and caches"; what the module does is removing the preinstalled apps for good, and
    // the row says so (the texts below are the installer's own, from its warning).
    Row {
        id: "purge",
        name: "purge",
        group: 2,
        key: "",
        title: "Purge",
        short: [
            "Permanently removes bundled apps and telemetry tasks",
            "Безвозвратно удаляет предустановленные приложения",
            "プリインストールのアプリを完全に削除します",
            "Remove de vez os apps pré-instalados",
            "永久移除预装应用",
        ],
        long: [
            "PERMANENTLY removes the bundled junk like Tiny11 does: Clipchamp, News, Weather, Xbox overlays, Teams, Cortana, Widgets, Skype, Solitaire and more, plus telemetry tasks. There is no undo.",
            "БЕЗВОЗВРАТНО удаляет предустановленный мусор, как Tiny11: Clipchamp, Новости, Погода, Xbox-оверлеи, Teams, Cortana, виджеты, Skype, Solitaire и др., а также задачи телеметрии. Отката нет.",
            "Tiny11 のように、プリインストールの不要なアプリを完全に削除します: Clipchamp、ニュース、天気、Xbox オーバーレイ、Teams、Cortana、ウィジェット、Skype、ソリティアなどと、テレメトリ タスク。元に戻すことはできません。",
            "Remove DE VEZ o lixo pré-instalado, como o Tiny11 faz: Clipchamp, Notícias, Clima, sobreposições do Xbox, Teams, Cortana, Widgets, Skype, Paciência e mais, além de tarefas de telemetria. Não há como desfazer.",
            "像 Tiny11 那样永久移除捆绑的垃圾：Clipchamp、新闻、天气、Xbox 覆盖层、Teams、Cortana、小组件、Skype、纸牌等等，还有遥测任务。无法撤销。",
        ],
        icon: "M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13M10 11v5M14 11v5",
        tile: Tile::Gradient(0xf97316, 0xc2410c),
        shot: Shot::Purge,
        size_mb: 0.7,
        ram: "—",
        replaces: None,
        new: false,
        source: Source::Plugin("purge"),
    },
];

#[must_use]
pub fn index_of(id: &str) -> Option<usize> {
    ROWS.iter().position(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_the_groups_are_in_order() {
        for (i, r) in ROWS.iter().enumerate() {
            assert_eq!(index_of(r.id), Some(i), "{} appears twice", r.id);
        }
        assert!(ROWS.windows(2).all(|w| w[0].group <= w[1].group), "the rows are listed group by group");
    }

    #[test]
    fn every_row_has_a_text_in_every_language() {
        for r in &ROWS {
            for (k, s) in r.short.iter().chain(r.long.iter()).enumerate() {
                assert!(!s.trim().is_empty(), "{}: text {k} is empty", r.id);
            }
        }
    }

    #[test]
    fn every_picture_decodes_as_a_png() {
        for r in &ROWS {
            for light in [false, true] {
                let b = r.shot.png(light);
                assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "{} is not a PNG", r.id);
            }
        }
    }
}
