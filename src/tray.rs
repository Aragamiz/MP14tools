//! Tray icon, its context menu, and the hidden window that anchors both the
//! icon and the OSD.
//!
//! Everything here lives on one dedicated thread with its own message loop, so
//! the settings window never has to deal with shell messages. The menu only ever
//! flips atomics or calls read-only helpers - the UI thread stays the single
//! writer of the configuration file.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};

use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, PostMessageW, RegisterClassW,
    SetForegroundWindow, TrackPopupMenu, TranslateMessage, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, PBT_APMPOWERSTATUSCHANGE, PBT_APMRESUMEAUTOMATIC,
    PBT_APMRESUMESUSPEND, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_POWERBROADCAST, WM_RBUTTONUP, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use crate::state::{Shared, WM_MP14_NOTICE, WM_MP14_OSD};

/// Message the shell sends for clicks on our icon.
const TRAY_CALLBACK: u32 = WM_APP + 1;

const COMMAND_SHOW: usize = 1;
const COMMAND_PAUSE: usize = 2;
const COMMAND_AUTOSTART: usize = 3;
const COMMAND_CONSOLE: usize = 4;
const COMMAND_EXIT: usize = 5;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
/// Whether the shell currently knows about our icon.
static ICON_VISIBLE: AtomicBool = AtomicBool::new(false);

/// Spawn the shell thread (tray icon + OSD host).
pub fn spawn(shared: Arc<Shared>) {
    let _ = SHARED.set(shared);

    std::thread::Builder::new()
        .name("shell".to_string())
        .spawn(|| unsafe { run() })
        .expect("failed to spawn the tray thread");
}

unsafe fn run() {
    let class_name = crate::win::wide("MP14Tools.Shell");
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: crate::win::module_instance(),
        lpszClassName: crate::win::pcw(&class_name),
        ..Default::default()
    };

    if RegisterClassW(&class) == 0 {
        crate::log::line("tray: RegisterClassW failed");
        return;
    }

    // Never shown; it exists purely to receive shell messages.
    let window = match CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        crate::win::pcw(&class_name),
        crate::win::pcw(&class_name),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        None,
        None,
        Some(crate::win::module_instance()),
        None,
    ) {
        Ok(window) => window,
        Err(error) => {
            crate::log::line(&format!("tray: CreateWindowExW failed: {error}"));
            return;
        }
    };

    if let Some(shared) = SHARED.get() {
        shared.set_shell_window(window);
        if shared
            .config
            .read()
            .map(|config| config.show_tray_icon)
            .unwrap_or(true)
        {
            add_icon(window);
        }
    }

    crate::log::line("tray: shell window ready");

    let mut message = Default::default();
    while GetMessageW(&mut message, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&message);
        let _ = DispatchMessageW(&message);
    }

    remove_icon(window);
    crate::osd::dispose();
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        TRAY_CALLBACK => {
            match lparam.0 as u32 {
                WM_LBUTTONUP => request_show(window),
                WM_RBUTTONUP => show_menu(window),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            on_command(wparam.0 & 0xFFFF);
            LRESULT(0)
        }
        WM_MP14_OSD => {
            show_pending_osd();
            LRESULT(0)
        }
        WM_MP14_NOTICE => {
            crate::notice::serve();
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            on_power_broadcast(wparam.0 as u32);
            // FALSE on purpose: answering TRUE for PBT_APMRESUMEAUTOMATIC would
            // tell Windows that no other application needs the notification.
            LRESULT(0)
        }
        WM_DESTROY => {
            crate::osd::dispose();
            crate::notice::dispose();
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

fn request_show(_window: HWND) {
    if let Some(shared) = SHARED.get() {
        shared.show_requested.store(true, Ordering::SeqCst);
        shared.wake_ui();
    }
}

/// Wake the shell thread so it can serve a queued notice.
///
/// Called from whichever thread has a question to ask; the shell thread owns the
/// window and its GDI objects, so it has to be the one that shows it.
pub fn wake_shell() {
    let Some(shared) = SHARED.get() else {
        return;
    };

    let raw = shared.shell_window.load(Ordering::SeqCst);
    if raw == 0 {
        return;
    }

    unsafe {
        let window = HWND(raw as *mut core::ffi::c_void);
        let _ = PostMessageW(
            Some(window),
            crate::state::WM_MP14_NOTICE,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

/// React to a power broadcast.
///
/// Both edges of the interesting transitions are events here, so the display
/// policy never has to poll: the power source change and the resume each arrive
/// on their own.
fn on_power_broadcast(code: u32) {
    match code {
        PBT_APMPOWERSTATUSCHANGE => crate::power::request(crate::power::Event::PowerSource),
        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
            crate::display::invalidate();
            crate::power::request(crate::power::Event::Resume);
        }
        _ => {}
    }
}

fn on_command(command: usize) {
    let Some(shared) = SHARED.get() else {
        return;
    };

    match command {
        COMMAND_SHOW => {
            shared.show_requested.store(true, Ordering::SeqCst);
            shared.wake_ui();
        }
        COMMAND_PAUSE => {
            let paused = shared.paused.load(Ordering::SeqCst);
            shared.paused.store(!paused, Ordering::SeqCst);
            crate::log::line(if paused {
                "tray: remapping resumed"
            } else {
                "tray: remapping paused"
            });
        }
        COMMAND_AUTOSTART => {
            shared.autostart_toggle_requested.store(true, Ordering::SeqCst);
            shared.wake_ui();
        }
        COMMAND_CONSOLE => {
            // The settings window owns the configuration file, so it performs the
            // flip; all this has to do is ask for it.
            shared.console_toggle_requested.store(true, Ordering::SeqCst);
            shared.wake_ui();
        }
        COMMAND_EXIT => {
            shared.exit_requested.store(true, Ordering::SeqCst);
            shared.wake_ui();
        }
        _ => {}
    }
}

fn show_pending_osd() {
    let Some(shared) = SHARED.get() else {
        return;
    };
    let text = shared
        .osd_queue
        .lock()
        .ok()
        .and_then(|mut queue| queue.pop_front());
    let Some(text) = text else {
        return;
    };

    let duration = shared
        .config
        .read()
        .map(|config| config.osd.duration_ms)
        .unwrap_or(crate::config::OSD_HOLD_DEFAULT_MS);
    crate::osd::show(&text, duration);
}

unsafe fn show_menu(window: HWND) {
    let Some(shared) = SHARED.get() else {
        return;
    };

    let Ok(menu) = CreatePopupMenu() else {
        return;
    };

    let paused = shared.paused.load(Ordering::SeqCst);
    let autostart = crate::autostart::is_enabled();
    let console = shared
        .config
        .read()
        .map(|config| config.log.console)
        .unwrap_or(false);

    let show_label = crate::win::wide("打开设置");
    let pause_label = crate::win::wide(if paused { "恢复映射" } else { "暂停映射" });
    let autostart_label = crate::win::wide(if autostart {
        "取消开机自启"
    } else {
        "开机自启"
    });
    let console_label = crate::win::wide("命令提示符");
    let exit_label = crate::win::wide("退出");

    let _ = AppendMenuW(menu, MF_STRING, COMMAND_SHOW, PCWSTR(show_label.as_ptr()));
    let pause_flags = if paused {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(menu, pause_flags, COMMAND_PAUSE, PCWSTR(pause_label.as_ptr()));
    let console_flags = if console {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    let _ = AppendMenuW(
        menu,
        console_flags,
        COMMAND_CONSOLE,
        PCWSTR(console_label.as_ptr()),
    );
    let _ = AppendMenuW(menu, MF_STRING, COMMAND_AUTOSTART, PCWSTR(autostart_label.as_ptr()));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, COMMAND_EXIT, PCWSTR(exit_label.as_ptr()));

    let mut cursor = POINT::default();
    let _ = GetCursorPos(&mut cursor);

    // Required so the menu closes when the user clicks elsewhere.
    let _ = SetForegroundWindow(window);

    let selected = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
        cursor.x,
        cursor.y,
        None,
        window,
        None,
    );

    let _ = DestroyMenu(menu);
    let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));

    if selected.as_bool() {
        on_command(selected.0 as usize);
    }
}

unsafe fn add_icon(window: HWND) {
    let mut data: NOTIFYICONDATAW = std::mem::zeroed();
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = 1;
    data.uCallbackMessage = TRAY_CALLBACK;

    let icon = crate::icon::hicon();
    data.uFlags = if icon.is_invalid() {
        NIF_MESSAGE | NIF_TIP
    } else {
        NIF_MESSAGE | NIF_ICON | NIF_TIP
    };
    data.hIcon = icon;

    let tip = crate::win::wide("MP14Tools");
    for (index, unit) in tip.iter().take(data.szTip.len() - 1).enumerate() {
        data.szTip[index] = *unit;
    }

    if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
        ICON_VISIBLE.store(true, Ordering::SeqCst);
        crate::log::line("tray: icon added");
    } else {
        crate::log::line("tray: NIM_ADD failed");
    }
}

unsafe fn remove_icon(window: HWND) {
    let mut data: NOTIFYICONDATAW = std::mem::zeroed();
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = 1;
    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    ICON_VISIBLE.store(false, Ordering::SeqCst);
}

/// Toggle the icon at runtime (used when the setting changes).
pub fn apply_visibility(shared: &Arc<Shared>) {
    let raw = shared.shell_window.load(Ordering::SeqCst);
    if raw == 0 {
        return;
    }
    let window = HWND(raw as *mut core::ffi::c_void);
    let visible = shared
        .config
        .read()
        .map(|config| config.show_tray_icon)
        .unwrap_or(true);

    // Called on every save and every reload; only act on an actual change so the
    // shell is not asked to re-create the icon over and over.
    if ICON_VISIBLE.swap(visible, Ordering::SeqCst) == visible {
        return;
    }

    unsafe {
        if visible {
            add_icon(window);
        } else {
            remove_icon(window);
        }
    }
}

