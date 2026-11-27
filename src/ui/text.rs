//! The window's texts in the five languages of the product. One line per text, the languages side by side
//! (`en, ru, ja, pt, zh`); a missing translation does not compile. The window holds its own `Lang`, so a render can show any of them
//! without touching `lang.txt`.
use winrust_common::Lang;

macro_rules! strings {
    ($($f:ident : [$en:expr, $ru:expr, $ja:expr, $pt:expr, $zh:expr]),* $(,)?) => {
        /// Every fixed text of the window in one language.
        pub struct T { pub lang: Lang, $(pub $f: &'static str),* }
        const EN: T = T { lang: Lang::En, $($f: $en),* };
        const RU: T = T { lang: Lang::Ru, $($f: $ru),* };
        const JA: T = T { lang: Lang::Ja, $($f: $ja),* };
        const PT: T = T { lang: Lang::Pt, $($f: $pt),* };
        const ZH: T = T { lang: Lang::Zh, $($f: $zh),* };
    };
}

/// The texts of `l`.
#[must_use]
pub fn t(l: Lang) -> &'static T {
    match l {
        Lang::En => &EN,
        Lang::Ru => &RU,
        Lang::Ja => &JA,
        Lang::Pt => &PT,
        Lang::Zh => &ZH,
    }
}

strings! {
    window: ["WinRust Setup", "Установка WinRust", "WinRust セットアップ", "Instalação da WinRust", "WinRust 安装程序"],
    tagline: ["A Windows shell written in Rust", "Оболочка Windows на Rust", "Rust 製の Windows シェル", "Um shell do Windows escrito em Rust", "用 Rust 编写的 Windows 外壳"],
    close: ["Close", "Закрыть", "閉じる", "Fechar", "关闭"],
    update: ["Update", "Обновить", "更新", "Atualizar", "更新"],
    remove_all: ["Remove all", "Удалить всё", "すべて削除", "Remover tudo", "全部移除"],
    note: [
        "Turns off Windows Search and hides the taskbar. The exe is unsigned, so SmartScreen may warn you.",
        "Отключает поиск Windows и скрывает панель задач. Файл не подписан — SmartScreen может предупредить.",
        "Windows 検索をオフにし、タスクバーを非表示にします。exe は署名されていないため、SmartScreen が警告を表示する場合があります。",
        "Desativa a Pesquisa do Windows e oculta a barra de tarefas. O exe não é assinado, então o SmartScreen pode exibir um aviso.",
        "会关闭 Windows 搜索并隐藏任务栏。该 exe 未签名，SmartScreen 可能会发出警告。"
    ],
    not_admin: [
        "Not an administrator: run the installer as administrator",
        "Нет прав администратора: запустите от имени администратора",
        "管理者ではありません: 管理者として実行してください",
        "Sem direitos de administrador: execute como administrador",
        "没有管理员权限：请以管理员身份运行"
    ],
    choose: ["Choose modules", "Выберите модули", "モジュールを選択", "Escolha os módulos", "选择模块"],
    soon: ["Soon", "Скоро", "近日公開", "Em breve", "即将推出"],
    included: ["Included", "В составе", "同梱", "Incluído", "已包含"],
    installed_chip: ["Installed", "Установлен", "インストール済み", "Instalado", "已安装"],
    new_tag: ["NEW", "NEW", "NEW", "NEW", "NEW"],
    g_shell: ["Shell", "Оболочка", "シェル", "Shell", "外壳"],
    g_apps: ["Apps", "Приложения", "アプリ", "Apps", "应用"],
    g_sys: ["System", "Система", "システム", "Sistema", "系统"],
    all: ["All", "Все", "すべて", "Todos", "全选"],
    none: ["None", "Снять", "解除", "Nenhum", "清除"],
    size: ["Size", "Размер", "サイズ", "Tamanho", "大小"],
    ram: ["Memory at idle", "Память в простое", "待機時のメモリ", "Memória em repouso", "空闲内存"],
    key: ["Shortcut", "Клавиши", "ショートカット", "Atalho", "快捷键"],
    replaces: ["Replaces ", "Заменяет ", "置き換え対象: ", "Substitui ", "取代 "],
    mb: ["MB", "МБ", "MB", "MB", "MB"],
    od_title: ["Hide the OneDrive icon", "Скрыть значок OneDrive", "OneDrive アイコンを隠す", "Ocultar o ícone do OneDrive", "隐藏 OneDrive 图标"],
    od_sub: ["Sync keeps working", "Синхронизация продолжает работать", "同期は引き続き動作します", "A sincronização continua funcionando", "同步功能仍可正常使用"],
    busy_inst: ["Installing…", "Идёт установка…", "インストール中…", "Instalando…", "正在安装…"],
    busy_rem: ["Removing…", "Идёт удаление…", "削除中…", "Removendo…", "正在移除…"],
    busy_sub: [
        "Keep the PC on. This usually takes under a minute.",
        "Не выключайте компьютер. Обычно это занимает меньше минуты.",
        "PC の電源を切らないでください。通常 1 分以内に完了します。",
        "Mantenha o PC ligado. Normalmente leva menos de um minuto.",
        "请保持电脑开机。通常不到一分钟。"
    ],
    stopping: [
        "Stopping after this step…",
        "Остановка после этого шага…",
        "このステップの後に停止します…",
        "Parando após esta etapa…",
        "完成此步骤后停止…"
    ],
    rolling_back: [
        "Bringing the stock Windows parts back…",
        "Возвращаются стандартные части Windows…",
        "Windows 標準の部品を戻しています…",
        "Trazendo de volta as partes originais do Windows…",
        "正在恢复系统自带的 Windows 部件…"
    ],
    cancel: ["Cancel", "Отмена", "キャンセル", "Cancelar", "取消"],
    queued: ["Queued", "В очереди", "待機中", "Na fila", "已排队"],
    done_word: ["Done", "Готово", "完了", "Pronto", "完成"],
    failed_word: ["Failed", "Ошибка", "失敗", "Falhou", "失败"],
    later: [
        "You can sign out later — modules start with your next sign-in.",
        "Можно выйти позже — модули заработают при следующем входе.",
        "サインアウトは後でも構いません。モジュールは次回のサインイン時に起動します。",
        "Você pode sair depois — os módulos iniciam no próximo login.",
        "可以稍后再注销——模块会在下次登录时启动。"
    ],
    back: ["Back", "Назад", "戻る", "Voltar", "返回"],
    sign_out: ["Sign out", "Выйти из системы", "サインアウト", "Sair da sessão", "注销"],
    done_sub: [
        "Changes apply after you sign in again",
        "Изменения применятся после повторного входа в систему",
        "変更はサインインし直すと反映されます",
        "As alterações são aplicadas depois que você entrar novamente",
        "更改将在你重新登录后生效"
    ],
    done_bad: [
        "Finished, but not everything worked",
        "Готово, но не всё получилось",
        "完了しましたが、一部失敗しました",
        "Concluído, mas nem tudo funcionou",
        "已完成，但有部分失败"
    ],
    done_bad_sub: [
        "The error is shown below. The rest went through; go back to try again.",
        "Ошибка указана ниже. Остальное выполнено; вернитесь назад и повторите.",
        "エラーは下に表示されています。それ以外は完了しています。戻って再試行できます。",
        "O erro aparece abaixo. O resto foi concluído; volte para tentar de novo.",
        "错误显示在下方。其余部分已完成；可以返回重试。"
    ],
    done_rollback: ["WinRust has been removed", "WinRust удалён", "WinRust を削除しました", "A WinRust foi removida", "WinRust 已移除"],
    done_rollback_sub: [
        "Windows Search, the stock Task Manager, the taskbar and Explorer are back.",
        "Вернулись поиск Windows, стандартный диспетчер задач, панель задач и Проводник.",
        "Windows 検索、標準のタスク マネージャー、タスクバー、エクスプローラーが戻りました。",
        "A Pesquisa do Windows, o Gerenciador de Tarefas padrão, a barra de tarefas e o Explorer voltaram.",
        "Windows 搜索、系统任务管理器、任务栏和资源管理器都已恢复。"
    ],
    toast_busy: [
        "Keep the window open until it finishes.",
        "Не закрывайте окно, пока идёт работа.",
        "完了するまでウィンドウを閉じないでください。",
        "Mantenha a janela aberta até terminar.",
        "完成之前请不要关闭窗口。"
    ],
    toast_lang_ok: [
        "Language changed. Restart the WinRust programs (or sign out) to apply it everywhere.",
        "Язык изменён. Перезапустите программы WinRust (или выйдите из системы), чтобы он применился везде.",
        "言語を変更しました。すべてに反映するには WinRust の各プログラムを再起動 (またはサインアウト) してください。",
        "Idioma alterado. Reinicie os programas da WinRust (ou saia da sessão) para aplicá-lo em todos.",
        "语言已切换。请重启 WinRust 程序（或注销后重新登录），让所有程序都生效。"
    ],
    toast_lang_local: [
        "Only this window switched: %ProgramData%\\WinRust\\lang.txt could not be written.",
        "Язык переключён только в этом окне: %ProgramData%\\WinRust\\lang.txt защищён от записи.",
        "このウィンドウだけが切り替わりました: %ProgramData%\\WinRust\\lang.txt に書き込めません。",
        "Somente esta janela mudou: não foi possível gravar %ProgramData%\\WinRust\\lang.txt.",
        "只有这个窗口切换了：无法写入 %ProgramData%\\WinRust\\lang.txt。"
    ],
    cancel_btn: ["Cancel", "Отмена", "キャンセル", "Cancelar", "取消"],
    rm_title: ["Remove WinRust?", "Удалить WinRust?", "WinRust を削除しますか?", "Remover a WinRust?", "移除 WinRust？"],
    rm_text: [
        "All components will be removed. Windows Search, the stock Task Manager, the taskbar and regular Explorer come back.",
        "Все компоненты будут удалены. Вернутся поиск Windows, стандартный диспетчер задач, панель задач и обычный Проводник.",
        "すべてのコンポーネントが削除されます。Windows 検索、標準のタスク マネージャー、タスクバー、通常のエクスプローラーが元に戻ります。",
        "Todos os componentes serão removidos. A Pesquisa do Windows, o Gerenciador de Tarefas padrão, a barra de tarefas e o Explorer comum voltam.",
        "所有组件都将被移除。Windows 搜索、系统自带的任务管理器、任务栏和常规资源管理器都会恢复。"
    ],
    rm_ok: ["Remove all", "Удалить всё", "すべて削除", "Remover tudo", "全部移除"],
    up_title: ["Update WinRust?", "Обновить WinRust?", "WinRust を更新しますか?", "Atualizar a WinRust?", "更新 WinRust？"],
    up_text: [
        "The installed components will be reinstalled from this version of the installer. Settings are kept.",
        "Установленные компоненты будут переустановлены из этой версии установщика. Настройки сохранятся.",
        "インストール済みのコンポーネントを、このバージョンのインストーラーから再インストールします。設定は保持されます。",
        "Os componentes instalados serão reinstalados a partir desta versão do instalador. As configurações são mantidas.",
        "已安装的组件将用此版本的安装程序重新安装。设置会被保留。"
    ],
    up_ok: ["Update", "Обновить", "更新", "Atualizar", "更新"],
    ap_install: ["Install the selected?", "Установить выбранное?", "選択したものをインストールしますか?", "Instalar os selecionados?", "安装所选项目？"],
    ap_install_ok: ["Install", "Установить", "インストール", "Instalar", "安装"],
    ap_remove: ["Remove the selected?", "Удалить выбранное?", "選択したものを削除しますか?", "Remover os selecionados?", "移除所选项目？"],
    ap_remove_ok: ["Remove", "Удалить", "削除", "Remover", "移除"],
    ap_apply: ["Apply the changes?", "Применить изменения?", "変更を適用しますか?", "Aplicar as alterações?", "应用更改？"],
    ap_apply_ok: ["Apply", "Применить", "適用", "Aplicar", "应用"],
    ap_purge: ["Remove apps for good?", "Удалить приложения навсегда?", "アプリを完全に削除しますか?", "Remover os apps de vez?", "永久删除应用？"],
    ap_purge_ok: ["Remove for good", "Удалить навсегда", "完全に削除", "Remover de vez", "永久删除"],
    restore_point: [
        "A restore point is created before installing.",
        "Перед установкой создаётся точка восстановления.",
        "インストールの前に復元ポイントを作成します。",
        "Um ponto de restauração é criado antes de instalar.",
        "安装前会创建还原点。"
    ],
    so_title: ["Sign out now?", "Выйти из системы сейчас?", "今すぐサインアウトしますか?", "Sair da sessão agora?", "现在注销？"],
    so_text: [
        "Open programs will be closed. Save your work first.",
        "Открытые программы будут закрыты. Сначала сохраните свою работу.",
        "開いているプログラムは閉じられます。先に作業内容を保存してください。",
        "Os programas abertos serão fechados. Salve seu trabalho antes.",
        "已打开的程序将被关闭。请先保存你的工作。"
    ],
}

fn ru_modules(n: usize) -> &'static str {
    match (n % 10, n % 100) {
        (1, r) if r != 11 => "модуль",
        (2..=4, r) if !(12..=14).contains(&r) => "модуля",
        _ => "модулей",
    }
}

impl T {
    /// `8 selected · 18.5 MB`
    #[must_use]
    pub fn selected(&self, n: usize, mb: f32) -> String {
        match self.lang {
            Lang::En => format!("{n} selected · {mb:.1} MB"),
            Lang::Ru => format!("Выбрано {n} · {mb:.1} МБ"),
            Lang::Ja => format!("{n} 個を選択 · {mb:.1} MB"),
            Lang::Pt => format!("{n} selecionado{} · {mb:.1} MB", if n == 1 { "" } else { "s" }),
            Lang::Zh => format!("已选 {n} 项 · {mb:.1} MB"),
        }
    }

    /// The install button's label: `Install 8 modules`; `Choose modules` when there is nothing to do.
    #[must_use]
    pub fn install_n(&self, n: usize) -> String {
        if n == 0 {
            return self.choose.to_string();
        }
        match self.lang {
            Lang::En => format!("Install {n} module{}", if n == 1 { "" } else { "s" }),
            Lang::Ru => format!("Установить {n} {}", ru_modules(n)),
            Lang::Ja => format!("{n} 個のモジュールをインストール"),
            Lang::Pt => format!("Instalar {n} módulo{}", if n == 1 { "" } else { "s" }),
            Lang::Zh => format!("安装 {n} 个模块"),
        }
    }

    /// The label when the switches ask to remove `n` installed modules.
    #[must_use]
    pub fn remove_n(&self, n: usize) -> String {
        match self.lang {
            Lang::En => format!("Remove {n} module{}", if n == 1 { "" } else { "s" }),
            Lang::Ru => format!("Удалить {n} {}", ru_modules(n)),
            Lang::Ja => format!("{n} 個のモジュールを削除"),
            Lang::Pt => format!("Remover {n} módulo{}", if n == 1 { "" } else { "s" }),
            Lang::Zh => format!("移除 {n} 个模块"),
        }
    }

    /// Both at once: `Apply +2 −1`.
    #[must_use]
    pub fn apply_ar(&self, add: usize, rm: usize) -> String {
        let w = match self.lang {
            Lang::En => "Apply",
            Lang::Ru => "Применить",
            Lang::Ja => "適用",
            Lang::Pt => "Aplicar",
            Lang::Zh => "应用",
        };
        format!("{w} +{add} −{rm}")
    }

    /// `Installing rust-search` / `Removing rust-search`
    #[must_use]
    pub fn busy_on(&self, install: bool, name: &str) -> String {
        match (self.lang, install) {
            (Lang::En, true) => format!("Installing {name}"),
            (Lang::En, false) => format!("Removing {name}"),
            (Lang::Ru, true) => format!("Устанавливаю {name}"),
            (Lang::Ru, false) => format!("Удаляю {name}"),
            (Lang::Ja, true) => format!("{name} をインストール中"),
            (Lang::Ja, false) => format!("{name} を削除中"),
            (Lang::Pt, true) => format!("Instalando {name}"),
            (Lang::Pt, false) => format!("Removendo {name}"),
            (Lang::Zh, true) => format!("正在安装 {name}"),
            (Lang::Zh, false) => format!("正在移除 {name}"),
        }
    }

    /// The headline of the finished page: what was installed and removed.
    #[must_use]
    pub fn done_title(&self, added: usize, removed: usize) -> String {
        let (a, r) = (added, removed);
        match self.lang {
            Lang::En => match (a, r) {
                (a, 0) => format!("{a} module{} installed", if a == 1 { "" } else { "s" }),
                (0, r) => format!("{r} module{} removed", if r == 1 { "" } else { "s" }),
                (a, r) => format!("{a} installed, {r} removed"),
            },
            Lang::Ru => match (a, r) {
                (a, 0) => format!("Установлено модулей: {a}"),
                (0, r) => format!("Удалено модулей: {r}"),
                (a, r) => format!("Установлено: {a}, удалено: {r}"),
            },
            Lang::Ja => match (a, r) {
                (a, 0) => format!("{a} 個のモジュールをインストールしました"),
                (0, r) => format!("{r} 個のモジュールを削除しました"),
                (a, r) => format!("{a} 個をインストール、{r} 個を削除しました"),
            },
            Lang::Pt => match (a, r) {
                (a, 0) => format!("{a} módulo{} instalado{}", if a == 1 { "" } else { "s" }, if a == 1 { "" } else { "s" }),
                (0, r) => format!("{r} módulo{} removido{}", if r == 1 { "" } else { "s" }, if r == 1 { "" } else { "s" }),
                (a, r) => format!("{a} instalado{}, {r} removido{}", if a == 1 { "" } else { "s" }, if r == 1 { "" } else { "s" }),
            },
            Lang::Zh => match (a, r) {
                (a, 0) => format!("已安装 {a} 个模块"),
                (0, r) => format!("已移除 {r} 个模块"),
                (a, r) => format!("已安装 {a} 个，已移除 {r} 个"),
            },
        }
    }

    /// `New version v0.6.0 ›`
    #[must_use]
    pub fn newer(&self, v: &str) -> String {
        match self.lang {
            Lang::En => format!("New version {v} ›"),
            Lang::Ru => format!("Новая версия {v} ›"),
            Lang::Ja => format!("新しいバージョン {v} ›"),
            Lang::Pt => format!("Nova versão {v} ›"),
            Lang::Zh => format!("新版本 {v} ›"),
        }
    }

    /// `To be installed: a, b.`
    #[must_use]
    pub fn will_install(&self, names: &str) -> String {
        match self.lang {
            Lang::En => format!("To be installed: {names}."),
            Lang::Ru => format!("Будет установлено: {names}."),
            Lang::Ja => format!("インストールされるもの: {names}。"),
            Lang::Pt => format!("Serão instalados: {names}."),
            Lang::Zh => format!("将安装：{names}。"),
        }
    }

    /// `To be removed: a, b (the stock Windows parts come back).`
    #[must_use]
    pub fn will_remove(&self, names: &str) -> String {
        match self.lang {
            Lang::En => format!("To be removed: {names} (the stock Windows parts come back)."),
            Lang::Ru => format!("Будет удалено: {names} (вернутся стандартные части Windows)."),
            Lang::Ja => format!("削除されるもの: {names} (Windows 標準の部品が元に戻ります)。"),
            Lang::Pt => format!("Serão removidos: {names} (as partes originais do Windows voltam)."),
            Lang::Zh => format!("将移除：{names}（系统自带的 Windows 部件会恢复）。"),
        }
    }

    /// `Error: …`
    #[must_use]
    pub fn error(&self, e: &str) -> String {
        match self.lang {
            Lang::En => format!("Error: {e}"),
            Lang::Ru => format!("Ошибка: {e}"),
            Lang::Ja => format!("エラー: {e}"),
            Lang::Pt => format!("Erro: {e}"),
            Lang::Zh => format!("错误：{e}"),
        }
    }
}

/// A text of the module table in `l`: the five texts are in the order of [`Lang`].
#[must_use]
pub fn pick(l: Lang, five: &[&'static str; 5]) -> &'static str {
    five[l as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_five_languages_are_in_the_order_pick_uses() {
        assert_eq!(pick(Lang::Ru, &["en", "ru", "ja", "pt", "zh"]), "ru");
        assert_eq!(pick(Lang::Zh, &["en", "ru", "ja", "pt", "zh"]), "zh");
        assert_eq!(t(Lang::Ja).lang, Lang::Ja);
    }

    #[test]
    fn counts_agree_with_their_nouns() {
        assert_eq!(t(Lang::En).install_n(1), "Install 1 module");
        assert_eq!(t(Lang::En).install_n(8), "Install 8 modules");
        assert_eq!(t(Lang::Ru).install_n(2), "Установить 2 модуля");
        assert_eq!(t(Lang::Ru).install_n(5), "Установить 5 модулей");
        assert_eq!(t(Lang::Ru).install_n(21), "Установить 21 модуль");
        assert_eq!(t(Lang::Ru).install_n(11), "Установить 11 модулей");
        assert_eq!(t(Lang::En).install_n(0), "Choose modules");
        assert_eq!(t(Lang::En).done_title(8, 0), "8 modules installed");
        assert_eq!(t(Lang::En).done_title(0, 1), "1 module removed");
    }
}
