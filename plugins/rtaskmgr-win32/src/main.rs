#![windows_subsystem = "windows"]
//! rtaskmgr-win32   The Task Manager of WinRust:RE: a window of `winrust-ui` in the Obsidian design.
//!
//! rtaskmgr-win32 [--tray]                  the window (or, with --tray, only the tray icon until it is asked for)
//! rtaskmgr-win32 --render <png> state=<name> [light] [scale=1.5] [max] [lang=ru]   a state of the design as a picture: no window, made-up processes
//! rtaskmgr-win32 --render-all <dir>        every state, dark and light, scale 1 and 1.5
//! rtaskmgr-win32 --bench [open|hover|scroll|tick|rest|memory|all]   what it costs (windows off the screen, made-up processes)
//! rtaskmgr-win32 --selftest tm | window | tray | memory | stage <png>   the window on made-up processes, off the screen
//!
//! The renders, the benchmark and the self-tests run on a made-up machine: they never end, suspend or change a process, a service or a
//! startup entry of this computer, and never make a tray icon.
#[macro_use]
extern crate winrust_common;
mod app;
mod backend;
mod boost;
mod check;
mod data;
mod demo;
mod draw;
mod gpu;
mod hw;
mod look;
mod model;
mod net;
mod perf;
mod perfview;
mod res;
mod startup;
mod store;
mod suspect;
mod sys;
mod tray;
mod ui;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Duration};

use app::Tm;
use backend::Real;
use data::Data;
use model::{NAV, Page};
use winrust_ui::{
    core::place_path,
    window::{Options, quit, run},
};

/// Самопроверка ETW: сессия + трафик через localhost + чтение счётчиков своего процесса. Результат — в файл.
fn net_selftest(out: &str) {
    use std::io::{Read, Write};
    let report = (|| -> Result<String, String> {
        let ses = net::start().ok_or(t!("не удалось запустить ETW-сессию (нужны права администратора)", "could not start the ETW session (administrator rights needed)", ja: "ETW セッションを開始できませんでした (管理者権限が必要です)", pt: "não foi possível iniciar a sessão ETW (são necessários direitos de administrador)", zh: "无法启动 ETW 会话（需要管理员权限）"))?;
        std::thread::sleep(Duration::from_millis(500));
        let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let addr = l.local_addr().map_err(|e| e.to_string())?;
        let srv = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = vec![0u8; 100_000];
            let mut got = 0;
            while got < 200_000 {
                match c.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
        });
        let mut c = std::net::TcpStream::connect(addr).map_err(|e| e.to_string())?;
        c.write_all(&vec![7u8; 200_000]).map_err(|e| e.to_string())?;
        drop(c);
        let _ = srv.join();
        std::thread::sleep(Duration::from_secs(2));
        let me = std::process::id();
        let all = net::drain();
        net::stop(ses);
        let (rx, tx) = all.get(&me).copied().unwrap_or((0, 0));
        Ok(tf!("pid={me} принято={rx} отправлено={tx} процессов_в_данных={}", "pid={me} received={rx} sent={tx} processes_in_data={}", ja: "pid={me} 受信={rx} 送信={tx} データ内のプロセス={}", pt: "pid={me} recebido={rx} enviado={tx} processos_nos_dados={}", zh: "pid={me} received={rx} sent={tx} processes_in_data={}", all.len()))
    })();
    let _ = std::fs::write(out, report.unwrap_or_else(|e| tf!("ОШИБКА: {e}", "ERROR: {e}", ja: "エラー: {e}", pt: "ERRO: {e}", zh: "错误：{e}")));
}

/// `lang=ru` (or en, ja, pt, zh) among the arguments of a render: its language, for this process only (the renders are English otherwise,
/// whatever the machine says: the design is).
fn render_language(args: &[String]) {
    let asked = args.iter().any(|a| matches!(a.as_str(), "--render" | "--render-all" | "--bench" | "--selftest"));
    if !asked {
        return;
    }
    let lang = args.iter().find_map(|a| a.strip_prefix("lang=")).map_or(winrust_common::Lang::En, winrust_common::Lang::parse);
    winrust_common::use_lang(lang);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // The renders, the benchmark and the self-tests come first of all: they run on made-up data, take no rights, and never read or
    // change this computer.
    render_language(&args);
    if args.iter().any(|a| a == "--render") {
        // the design's stage: 1360 x 860
        for (k, v) in [("w", "1360"), ("h", "860")] {
            if !args.iter().any(|a| a.starts_with(&format!("{k}="))) {
                args.push(format!("{k}={v}"));
            }
        }
    }
    if let Some(code) = check::cli(&args).or_else(|| winrust_ui::render::cli(&args, demo::demo, &demo::STATES)) {
        quit(code as u32);
    }
    if let Some(i) = std::env::args().position(|a| a == "--net-selftest")
        && let Some(out) = std::env::args().nth(i + 1)
    {
        return net_selftest(&out);
    }
    // the installer, at removal: put back everything the Speed page switched on (exit code 1 = something did not come back)
    if args.first().is_some_and(|a| a == "--undo-speed") {
        let errs = boost::undo_all();
        for e in &errs {
            eprintln!("{e}");
        }
        std::process::exit(i32::from(!errs.is_empty()));
    }
    // a second call with administrator rights from start_real_taskmgr: only open the system's task manager
    if args.first().is_some_and(|a| a == "--real-taskmgr") {
        sys::start_real_taskmgr(&args[1..].join(" "), false);
        return;
    }
    // Without administrator rights the manager cannot end protected processes (Malwarebytes and the like): start again through the
    // scheduler's task, with rights and no UAC. "--tray" goes in a file: the task has no arguments.
    let tray_flag = store::dir().join("tray.flag");
    if !sys::is_admin() {
        if args.iter().any(|a| a == "--tray") {
            let _ = std::fs::create_dir_all(store::dir());
            let _ = std::fs::write(&tray_flag, "");
        }
        if sys::run_elevated_task() {
            return;
        }
        let _ = std::fs::remove_file(&tray_flag);
    }
    let ifeo = sys::taskmgr_args(&args); // Some = started in place of taskmgr.exe
    // We stand in for the system's task manager: if we fall, Ctrl+Shift+Esc must not be left without an answer.
    // (panic = "abort" calls this hook first anyway.)
    let fallback_args = ifeo.clone().unwrap_or_default();
    std::panic::set_hook(Box::new(move |info| {
        let dir = store::dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("crash.log"), format!("{info}
{}", std::backtrace::Backtrace::force_capture()));
        sys::start_real_taskmgr(&fallback_args, true);
    }));
    let hidden = args.iter().any(|a| a == "--tray") || (sys::is_admin() && std::fs::remove_file(&tray_flag).is_ok()); // started at sign-in: straight to the tray

    store::trim_history(10_080);
    let cfg = store::load();
    // `taskmgr /0 /startup` (how Settings opens the startup page) goes straight to that page
    let startup_asked = ifeo.as_deref().is_some_and(|a| a.to_lowercase().contains("/startup"));
    let page = if startup_asked { Page::Startup } else { NAV.get(cfg.view).map_or(Page::Processes, |n| n.0) };
    let page = if page == Page::Insights { Page::Processes } else { page };
    let app = Tm::new(Arc::new(Real::new()), Data::new(cfg), None, hidden, page);
    let code = run(Box::new(app), Options { class: "WinRustTaskMgr", size: (1280.0, 780.0), icon: 1, place_file: place_path("rtaskmgr") });
    quit(code as u32);
}
