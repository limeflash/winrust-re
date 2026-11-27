//! A [`Backend`] that changes nothing: what `--render`, `--bench` and the tests install into. It keeps a list of "installed" flags in
//! memory, and never touches the registry, `%ProgramData%`, Program Files or anything else of the machine.
use std::sync::Mutex;
use std::time::Duration;

use winrust_common::Lang;

use crate::ui::{Backend, ModInfo};

/// The plugins of `PLUGINS` in main.rs, in its order: name, part of "install everything", can be installed.
const NAMES: [(&str, bool, bool); 10] = [
    ("rust-search", true, true),
    ("rtaskmgr-win32", true, true),
    ("topbar", true, true),
    ("rust-explorer", true, true),
    ("wiztree", true, true),
    ("photoview", true, true),
    ("rust-settings", true, true),
    ("debloat", false, true),
    ("purge", false, true),
    ("chrome-rs", true, false),
];

pub struct Fake {
    installed: Mutex<Vec<bool>>,
    onedrive: Mutex<bool>,
    /// How long each step takes (a render and a test take none).
    pub delay: Duration,
    /// A plugin whose install fails, to see the failure.
    pub fail: Option<&'static str>,
    /// What was asked, in order.
    pub log: Mutex<Vec<String>>,
}

impl Fake {
    #[must_use]
    pub fn new() -> Self {
        Self { installed: Mutex::new(vec![false; NAMES.len()]), onedrive: Mutex::new(true), delay: Duration::ZERO, fail: None, log: Mutex::new(Vec::new()) }
    }

    /// Some plugins are there already.
    #[must_use]
    pub fn with(self, names: &[&str]) -> Self {
        for (i, n) in NAMES.iter().enumerate() {
            self.installed.lock().unwrap()[i] = names.contains(&n.0);
        }
        self
    }

    pub fn asked(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Default for Fake {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for Fake {
    fn mods(&self) -> Vec<ModInfo> {
        let inst = self.installed.lock().unwrap();
        NAMES.iter().enumerate().map(|(i, &(name, bulk, can))| ModInfo { name, installed: inst[i], bulk, can }).collect()
    }

    fn install(&self, m: usize) -> Result<String, String> {
        self.log.lock().unwrap().push(format!("install {}", NAMES[m].0));
        std::thread::sleep(self.delay);
        if self.fail == Some(NAMES[m].0) {
            return Err("the service could not be stopped (error 1051)".to_string());
        }
        self.installed.lock().unwrap()[m] = true;
        Ok(format!("{} installed.", NAMES[m].0))
    }

    fn uninstall(&self, m: usize) -> Result<String, String> {
        self.log.lock().unwrap().push(format!("uninstall {}", NAMES[m].0));
        std::thread::sleep(self.delay);
        self.installed.lock().unwrap()[m] = false;
        Ok(format!("{} removed.", NAMES[m].0))
    }

    fn rollback(&self) -> Result<String, String> {
        self.log.lock().unwrap().push("rollback".to_string());
        std::thread::sleep(self.delay);
        self.installed.lock().unwrap().iter_mut().for_each(|f| *f = false);
        Ok("All plugins removed.".to_string())
    }

    fn warning(&self, m: usize, lang: Lang) -> String {
        // the installer's own warnings of the two that ask for one, as `PLUGINS` has them (ru, en, ja, pt, zh)
        let five: [&str; 5] = match NAMES[m].0 {
            "rust-explorer" => [
                "ОБЫЧНЫЙ Explorer будет ОТКЛЮЧЁН: после установки панель задач, рабочий стол и окна папок дают topbar и rust-explorer. Сначала установите topbar, без него не будет панели. Если что-то пойдёт не так: Ctrl+Alt+Del, Диспетчер задач, Запустить новую задачу, explorer.exe; затем удалите плагин.",
                "The REGULAR Explorer will be DISABLED: after install, topbar and rust-explorer provide the taskbar, the desktop and folder windows. Install topbar first, without it there will be no bar. If something goes wrong: Ctrl+Alt+Del, Task Manager, Run new task, explorer.exe; then uninstall the plugin.",
                "通常のエクスプローラーが無効になります: インストール後は topbar と rust-explorer がタスクバー、デスクトップ、フォルダー ウィンドウを提供します。先に topbar をインストールしてください。問題が起きたら: Ctrl+Alt+Del、タスク マネージャー、新しいタスクの実行、explorer.exe の順に操作し、その後プラグインをアンインストールしてください。",
                "O Explorer COMUM será DESATIVADO: depois da instalação, a topbar e o rust-explorer fornecem a barra de tarefas, a área de trabalho e as janelas de pastas. Instale a topbar primeiro; sem ela não haverá barra. Se algo der errado: Ctrl+Alt+Del, Gerenciador de Tarefas, Executar nova tarefa, explorer.exe; depois desinstale o plugin.",
                "常规资源管理器将被停用：安装后，任务栏、桌面和文件夹窗口都由 topbar 和 rust-explorer 提供。请先安装 topbar，没有它就没有任务栏。如果出了问题：按 Ctrl+Alt+Del，打开任务管理器，运行新任务，输入 explorer.exe；然后卸载这个插件。",
            ],
            "purge" => [
                "НЕОБРАТИМО: приложения будут удалены для всех пользователей и из образа, задачи телеметрии удалены. Вернуть можно только из Store или winget.",
                "IRREVERSIBLE: apps are removed for all users and from the image, telemetry tasks are deleted. They come back only from the Store or winget.",
                "元に戻せません: アプリはすべてのユーザーから、およびイメージから削除され、テレメトリ タスクも削除されます。復活するのは Store または winget からのみです。",
                "IRREVERSÍVEL: os apps são removidos para todos os usuários e da imagem e as tarefas de telemetria são excluídas. Eles só voltam pela Store ou pelo winget.",
                "不可逆：应用会对所有用户并从镜像中移除，遥测任务会被删除。只能从应用商店或 winget 装回来。",
            ],
            _ => [""; 5],
        };
        // the order of `Lang`: en, ru, ja, pt, zh
        [five[1], five[0], five[2], five[3], five[4]][lang as usize].to_string()
    }

    fn onedrive_hidden(&self) -> bool {
        *self.onedrive.lock().unwrap()
    }

    fn set_onedrive_hidden(&self, hide: bool) -> Result<(), String> {
        *self.onedrive.lock().unwrap() = hide;
        Ok(())
    }

    fn set_lang(&self, _: Lang) -> bool {
        true
    }

    fn version(&self) -> String {
        "v0.5.1".to_string()
    }

    fn newer(&self) -> Option<String> {
        None
    }

    fn open_releases(&self) {
        self.log.lock().unwrap().push("open releases".to_string());
    }

    fn sign_out(&self) {
        self.log.lock().unwrap().push("sign out".to_string());
    }

    fn elevated(&self) -> bool {
        true
    }
}
