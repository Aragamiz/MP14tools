//! MP14Tools - a small, single-process remapper for the touchpad's deep press
//! and for vendor (OEM) hotkeys.
//!
//! Layout: one UI thread (eframe), one shell thread (tray icon + OSD), one raw
//! input thread (touchpad) and one mostly-blocked thread per WMI event class.
//! No services, no scheduled tasks, no idle polling beyond a 1.5 s check of the
//! configuration file's contents.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod catalog;
mod config;
mod console;
mod display;
mod haptics;
mod icon;
mod input;
mod log;
mod notice;
mod oemkeys;
mod osd;
mod power;
mod state;
mod touchpad;
mod tray;
mod ui;
mod win;

use std::sync::Arc;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// How often the configuration file's contents are compared.
const CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(1500);

fn main() {
    // Claim the single-instance mutex before anything observable: a second copy
    // exits right away, and an optional console window would only flash.
    if already_running() {
        log::line("another instance is already running; exiting");
        return;
    }

    // Both of these have to be in place before the first log line: the console
    // must exist while the standard handles are still unused, and the log file
    // is opened by the first line written.
    let preview = config::startup_preview();
    console::sync(preview.console());
    log::set_directory(&preview.log_directory());

    log::line(&format!("mp14tools {} starting", env!("CARGO_PKG_VERSION")));

    unsafe {
        // Physical pixels everywhere; the OSD then positions itself correctly on
        // scaled displays.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // The product was renamed, so the very first start has to pick up the file
    // the previous name left behind. Clearing the old autostart entry keeps the
    // startup list from keeping a path that no longer exists.
    config::adopt_legacy_config();
    autostart::remove_legacy_entry();

    let config = config::load();
    // Materialise the file on first run so it can be hand-edited, and so the
    // settings window always has something on disk to compare against.
    if !config::config_path().exists() {
        match config::save(&config) {
            Ok(()) => log::line(&format!(
                "wrote the default configuration to {}",
                config::config_path().display()
            )),
            Err(error) => log::line(&format!("could not write the default configuration: {error}")),
        }
    }

    let shared = state::Shared::new(config);
    if let Ok(text) = std::fs::read_to_string(config::config_path()) {
        if let Ok(mut applied) = shared.config_text.lock() {
            *applied = text;
        }
    }

    touchpad::spawn(shared.clone());
    oemkeys::spawn(shared.clone());
    tray::spawn(shared.clone());
    haptics::spawn(shared.clone());
    power::spawn(shared.clone());
    start_config_watcher(shared.clone());

    log::line("opening settings window");
    if let Err(error) = ui::run(shared.clone()) {
        log::line(&format!("settings window ended with an error: {error}"));
    }

    log::line("mp14tools exiting");
}

/// Apply the log-related configuration: the console window and the directory the
/// log file goes to.
///
/// Idempotent, and the single place both are applied from - startup, the watcher
/// and the settings window all end up here, so a checkbox and a hand-edited file
/// cannot drift apart.
fn apply_log_settings(shared: &Arc<state::Shared>) {
    let (console, directory) = match shared.config.read() {
        Ok(config) => (config.log.console, config.log.resolved_directory()),
        Err(_) => return,
    };

    console::sync(console);
    log::set_directory(&directory);
}

/// A second copy would double every remapped keystroke, so refuse to start.
fn already_running() -> bool {
    let name = win::wide("MP14Tools.SingleInstance");
    match unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) } {
        Ok(handle) => {
            // Reading the last error immediately after the call is the
            // documented way to detect the pre-existing case.
            let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            // The handle is intentionally kept alive for the process lifetime.
            let _ = handle;
            existing
        }
        Err(error) => {
            log::line(&format!("single-instance mutex failed: {error}"));
            false
        }
    }
}

/// Watch the configuration file for hand edits.
///
/// Compares file contents rather than timestamps, and skips anything the
/// settings window itself wrote, so an in-progress edit is never clobbered.
fn start_config_watcher(shared: Arc<state::Shared>) {
    let spawned = std::thread::Builder::new()
        .name("config-watch".to_string())
        .spawn(move || loop {
            std::thread::sleep(CONFIG_POLL_INTERVAL);

            let Ok(text) = std::fs::read_to_string(config::config_path()) else {
                continue;
            };
            if text == shared.applied_text() {
                continue;
            }

            match serde_json::from_str::<config::Config>(&text) {
                Ok(mut parsed) => {
                    parsed.normalize();
                    log::line("configuration reloaded from disk");
                    shared.apply_external_config(text, parsed);
                    shared.set_config_notice(
                        "配置文件已被外部修改（可能是其他程序或手动编辑），已按文件重新加载，\\
                         本工具不会覆盖这些改动"
                            .to_string(),
                    );
                    tray::apply_visibility(&shared);
                    // The two options that act outside this process follow the
                    // file as well, so a hand edit is enough to change them.
                    apply_log_settings(&shared);
                    haptics::request_from(&shared);
                }
                Err(error) => {
                    log::line(&format!("configuration reload skipped: {error}"));
                    // Remember the broken text so the same error is logged once.
                    if let Ok(mut applied) = shared.config_text.lock() {
                        *applied = text;
                    }
                }
            }
        });

    if let Err(error) = spawned {
        log::line(&format!("could not start the config watcher: {error}"));
    }
}
