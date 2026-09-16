//! State shared between the UI thread, the touchpad thread and the WMI threads.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::config::{Action, Config};
use crate::input;

/// Posted to the tray window after an entry is pushed to `osd_queue`.
pub const WM_MP14_OSD: u32 = WM_APP + 10;
/// Posted to the tray window when a notice is waiting in [`crate::notice`].
pub const WM_MP14_NOTICE: u32 = WM_APP + 11;

/// Everything the background threads need to talk to each other and to the UI.
pub struct Shared {
    /// Current configuration. Replaced wholesale by the config watcher.
    pub config: RwLock<Config>,
    /// Latest touchpad pressure plus when it was observed, for the live readout.
    pub touchpad_pressure: Mutex<(i32, Option<Instant>)>,
    /// True once `RegisterRawInputDevices` succeeded.
    pub touchpad_registered: AtomicBool,
    /// True once at least one WMI class subscription is live.
    pub wmi_ready: AtomicBool,
    /// True once a haptic strength write reached the touchpad firmware.
    pub haptics_ready: AtomicBool,
    /// Outcome of the most recent haptic strength write.
    pub haptics_status: Mutex<String>,
    /// What the display policy did last, for the settings window.
    pub display_status: Mutex<String>,
    /// True while the touchpad holds haptic strengths that something else wrote.
    pub haptics_conflict: AtomicBool,
    /// Set when the configuration file was changed from outside this window.
    pub config_notice: Mutex<String>,
    /// Short description of the most recent successful trigger.
    pub last_trigger: Mutex<String>,
    /// Pending OSD texts, drained by the tray window.
    pub osd_queue: Mutex<VecDeque<String>>,
    /// Handle of the tray window, used to wake it up. Stored as an integer so
    /// `Shared` stays `Send + Sync` (raw pointers are neither).
    pub shell_window: AtomicIsize,
    /// Set by the tray menu; the settings window polls it and comes to front.
    pub show_requested: AtomicBool,
    /// Set by the tray menu; the settings window polls it and exits.
    pub exit_requested: AtomicBool,
    /// Set by the tray menu; the settings window applies the autostart change,
    /// keeping the UI thread the only writer of the configuration file.
    pub autostart_toggle_requested: AtomicBool,
    /// Set by the tray menu; the settings window flips `console` and applies it.
    pub console_toggle_requested: AtomicBool,
    /// Runtime mute - flips from the tray without touching the config file.
    pub paused: AtomicBool,
    /// Text of the configuration file as last applied or written. Lets the
    /// watcher distinguish an external edit from our own save.
    pub config_text: Mutex<String>,
    /// Bumped only when a change came from *outside* the settings window.
    pub config_generation: AtomicU64,
    /// Wakes the UI thread out of its event loop.
    ///
    /// `egui` only runs a pass when something asks it to, so a tray click must
    /// ring this bell instead of the UI polling for it - polling would keep the
    /// process busy forever just to react to an occasional click.
    wake_ui: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl Shared {
    pub fn new(config: Config) -> Arc<Self> {
        Arc::new(Self {
            config: RwLock::new(config),
            touchpad_pressure: Mutex::new((0, None)),
            touchpad_registered: AtomicBool::new(false),
            wmi_ready: AtomicBool::new(false),
            haptics_ready: AtomicBool::new(false),
            haptics_status: Mutex::new(String::new()),
            display_status: Mutex::new(String::new()),
            haptics_conflict: AtomicBool::new(false),
            config_notice: Mutex::new(String::new()),
            last_trigger: Mutex::new(String::new()),
            osd_queue: Mutex::new(VecDeque::new()),
            shell_window: AtomicIsize::new(0),
            show_requested: AtomicBool::new(false),
            exit_requested: AtomicBool::new(false),
            autostart_toggle_requested: AtomicBool::new(false),
            console_toggle_requested: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            config_text: Mutex::new(String::new()),
            config_generation: AtomicU64::new(0),
            wake_ui: Mutex::new(None),
        })
    }

    /// Remember the tray window so background threads can post to it.
    pub fn set_shell_window(&self, window: HWND) {
        self.shell_window.store(window.0 as isize, Ordering::SeqCst);
    }

    pub fn touchpad_enabled(&self) -> bool {
        self.config
            .read()
            .map(|config| config.touchpad.enabled)
            .unwrap_or(false)
    }

    /// Record the observed pressure so the settings window can show it.
    pub fn set_pressure(&self, pressure: i32) {
        if let Ok(mut slot) = self.touchpad_pressure.lock() {
            *slot = (pressure, Some(Instant::now()));
        }
    }

    /// Fresh pressure reading, or `None` when the touchpad has been quiet.
    pub fn pressure(&self) -> Option<i32> {
        let slot = self.touchpad_pressure.lock().ok()?;
        let (pressure, stamp) = *slot;
        match stamp {
            Some(stamp) if stamp.elapsed().as_millis() < 400 => Some(pressure),
            _ => None,
        }
    }

    pub fn set_last_trigger(&self, text: String) {
        if let Ok(mut slot) = self.last_trigger.lock() {
            *slot = text;
        }
    }

    pub fn last_trigger(&self) -> String {
        self.last_trigger
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    /// Queue an OSD banner and wake the tray window so it can display it.
    pub fn queue_osd(&self, text: String) {
        if let Ok(mut queue) = self.osd_queue.lock() {
            // Only the newest banner matters for a short-lived toast.
            queue.clear();
            queue.push_back(text);
        }

        let raw = self.shell_window.load(Ordering::SeqCst);
        if raw != 0 {
            unsafe {
                let window = HWND(raw as *mut core::ffi::c_void);
                let _ = PostMessageW(Some(window), WM_MP14_OSD, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// True while the user has temporarily suspended remapping from the tray.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Record the outcome of a haptic strength write for the settings window.
    pub fn set_haptics_status(&self, ok: bool, text: String) {
        self.haptics_ready.store(ok, Ordering::SeqCst);
        if let Ok(mut slot) = self.haptics_status.lock() {
            *slot = text;
        }
    }

    /// Outcome of the most recent haptic strength write, empty before the first.
    pub fn haptics_status(&self) -> String {
        self.haptics_status
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    /// Record what the display policy did, for the settings window.
    pub fn set_display_status(&self, text: String) {
        if let Ok(mut slot) = self.display_status.lock() {
            *slot = text;
        }
    }

    /// Last thing the display policy did, empty before the first event.
    pub fn display_status(&self) -> String {
        self.display_status
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    /// True while the touchpad holds values that another program wrote.
    pub fn haptics_conflict(&self) -> bool {
        self.haptics_conflict.load(Ordering::SeqCst)
    }

    /// True once a write to the touchpad actually succeeded.
    pub fn haptics_ready(&self) -> bool {
        self.haptics_ready.load(Ordering::SeqCst)
    }

    pub fn set_haptics_conflict(&self, conflict: bool) {
        self.haptics_conflict.store(conflict, Ordering::SeqCst);
    }

    /// Remember that the configuration changed from outside this window, so the
    /// settings window can say so instead of silently showing new values.
    pub fn set_config_notice(&self, text: String) {
        if let Ok(mut slot) = self.config_notice.lock() {
            *slot = text;
        }
    }

    pub fn config_notice(&self) -> String {
        self.config_notice
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    /// Apply a configuration that came from somewhere other than the settings
    /// window (hand-edited file, external tool).
    pub fn apply_external_config(&self, text: String, config: Config) {
        if let Ok(mut slot) = self.config.write() {
            *slot = config;
        }
        if let Ok(mut applied) = self.config_text.lock() {
            *applied = text;
        }
        self.config_generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Text of the configuration snapshot currently in effect.
    pub fn applied_text(&self) -> String {
        self.config_text
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    /// Register how to nudge the UI thread.
    pub fn set_wake_ui(&self, wake: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut slot) = self.wake_ui.lock() {
            *slot = Some(wake);
        }
    }

    /// Nudge the UI thread, if it has registered a way to be nudged.
    pub fn wake_ui(&self) {
        if let Ok(slot) = self.wake_ui.lock() {
            if let Some(wake) = slot.as_ref() {
                wake();
            }
        }
    }
}

/// Run `action` and surface the outcome to the log, the UI and the OSD.
///
/// Called from the touchpad thread and the WMI threads, so it must not touch the
/// UI thread directly.
pub fn dispatch(shared: &Arc<Shared>, action: &Action, label: &str) {
    if shared.is_paused() {
        return;
    }

    if action.is_unset() {
        return;
    }

    match input::send(action) {
        Ok(sent) => {
            let summary = format!("{label} → {sent}");
            crate::log::line(&format!("trigger: {summary}"));
            shared.set_last_trigger(summary.clone());
            if shared
                .config
                .read()
                .map(|config| config.osd.enabled)
                .unwrap_or(false)
            {
                shared.queue_osd(summary);
            }
        }
        Err(error) => {
            crate::log::line(&format!("trigger failed for {label}: {error}"));
            shared.set_last_trigger(format!("{label} → 失败：{error}"));
        }
    }
}

/// Diagnostic counter helper used by the UI to show thread health.
pub fn mark_wmi_ready(shared: &Arc<Shared>) {
    if !shared.wmi_ready.swap(true, Ordering::SeqCst) {
        crate::log::line("WMI event subscription active");
    }
}
