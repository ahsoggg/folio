#![cfg_attr(not(test), windows_subsystem = "windows")]
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::large_enum_variant
)]

mod diag;
mod gui;
mod i18n;
mod installer;
mod library;
mod pdfdll;
mod settings;
mod shell;
mod single;
mod speech;
mod timeutil;
mod update;
mod winint;

use installer::Env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const HELP: &str = "\
Folio — document reader

  Folio.exe [file ...]              open documents (PDF, DOCX, ODT, RTF, EPUB, FB2, MD, HTML, TXT, XLSX, CSV, PPTX, CBZ, images, code)
  Folio.exe --screenshots DIR       regenerate the README screenshots into DIR (made-up documents, off-screen window)
  Folio.exe --install [--silent]    install for the current user (also when the file is named *Setup*.exe)
  Folio.exe --uninstall [--silent]
  Folio.exe --register / --unregister   add or remove Folio in \"Open with\" and Default apps
  Folio.exe --update                check GitHub for a newer version and replace this file
  Folio.exe --version
";

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn main() -> ExitCode {
    diag::install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_name = exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let settings = settings::Settings::load();
    i18n::set_lang(i18n::Lang::from_code(&settings.language));

    if flag(&args, "--version") {
        winint::console_print(&format!("Folio {}\n", env!("CARGO_PKG_VERSION")));
        return ExitCode::SUCCESS;
    }
    if flag(&args, "--help") || flag(&args, "-h") || flag(&args, "/?") {
        winint::console_print(HELP);
        return ExitCode::SUCCESS;
    }
    if flag(&args, "--update") {
        return ExitCode::from(update::run_headless() as u8);
    }
    if flag(&args, "--uninstall") {
        return run_uninstall(&exe, flag(&args, "--silent"));
    }
    if flag(&args, "--register") || flag(&args, "--unregister") {
        let env = Env::real();
        if flag(&args, "--unregister") {
            shell::unregister(&env);
        } else if let Err(e) = shell::register(&env, &exe, true, flag(&args, "--context-menu")) {
            winint::console_print(&format!("registration failed: {e}\n"));
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }
    if installer::started_as_installer(&exe_name, &args) {
        return run_install(&exe, flag(&args, "--silent"));
    }

    if installer::is_installed_copy(&Env::real(), &exe) {
        update::cleanup_old(&exe);
    }

    let shots = args
        .iter()
        .position(|a| a == "--screenshots")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    if shots.is_some() {
        settings::set_read_only();
    }
    let restarting = flag(&args, "--restart");
    let files: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--") && Some(PathBuf::from(a)) != shots)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();

    // one window per user: hand the files to the running one
    let mut guard = None;
    if shots.is_none() {
        let mut role = single::acquire();
        if restarting {
            // the previous window is still closing after an update
            for _ in 0..40 {
                if matches!(role, single::Role::First(_)) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
                role = single::acquire();
            }
        }
        match role {
            single::Role::First(g) => guard = Some(g),
            single::Role::Another => {
                if settings.single_window && single::forward(&files, Duration::from_millis(2500)) {
                    return ExitCode::SUCCESS;
                }
            }
        }
    }

    diag::info(&format!("start {}", env!("CARGO_PKG_VERSION")));
    let opts = gui::StartOptions {
        open: files,
        shots,
        guard,
    };
    match gui::run(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            diag::error(&format!("window error: {e}"));
            winint::message_box(
                "Folio",
                &i18n::trf(
                    "Не удалось открыть окно: {0}",
                    "The window could not be opened: {0}",
                    &[&e],
                ),
                false,
            );
            ExitCode::FAILURE
        }
    }
}

fn run_install(exe: &std::path::Path, silent: bool) -> ExitCode {
    let env = Env::real();
    if !silent
        && !winint::message_box(
            "Folio",
            &format!(
                "{}\n{}",
                i18n::tr(
                    "Установить Folio для текущего пользователя?",
                    "Install Folio for the current user?"
                ),
                env.install_dir.display()
            ),
            true,
        )
    {
        return ExitCode::SUCCESS;
    }
    match installer::install(&env, exe) {
        Ok(installed) => {
            diag::info("installed");
            // a silent install (scripts, CI) must not start the program
            if !silent
                && winint::message_box(
                    "Folio",
                    &format!(
                        "{}\n\n{}",
                        i18n::trf("Установлено: {0}", "Installed: {0}", &[&installed.display()]),
                        i18n::tr(
                            "Запустить Folio сейчас? Он уже есть в меню «Открыть с помощью» и в «Приложениях по умолчанию».",
                            "Start Folio now? It is already in “Open with” and in Default apps."
                        )
                    ),
                    true,
                )
            {
                let _ = std::process::Command::new(&installed).spawn();
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            diag::error(&format!("install failed: {e}"));
            winint::message_box("Folio", &e, false);
            ExitCode::FAILURE
        }
    }
}

fn run_uninstall(exe: &std::path::Path, silent: bool) -> ExitCode {
    let env = Env::real();
    if !silent
        && !winint::message_box(
            "Folio",
            &format!(
                "{} {}?",
                i18n::tr("Удалить Folio из", "Remove Folio from"),
                env.install_dir.display()
            ),
            true,
        )
    {
        return ExitCode::SUCCESS;
    }
    match installer::uninstall(&env, Some(exe)) {
        Ok(()) => {
            diag::info("uninstalled");
            if !silent {
                winint::message_box("Folio", i18n::tr("Folio удалён.", "Folio has been removed."), false);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            diag::error(&format!("uninstall failed: {e}"));
            winint::message_box("Folio", &e, false);
            ExitCode::FAILURE
        }
    }
}
