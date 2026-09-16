//! Touchpad deep-press detection over raw HID input.
//!
//! A message-only-style hidden window registers for the precision touchpad HID
//! collection (`UsagePage 0x0D`, `Usage 0x05`) with `RIDEV_INPUTSINK`, so reports
//! arrive even while another window has focus. Each report is decoded into
//! contacts and reduced to a single pressure value, then run through a small
//! state machine that decides when a "deep press" happened.
//!
//! Detection is read-only: no private HID feature reports are written back, so
//! the touchpad's own firmware behaviour is untouched.

use std::mem::size_of;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS,
    RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEHID,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::state::{dispatch, Shared};
use crate::win;

const WM_INPUT: u32 = 0x00FF;
/// `RIDEV_DEVNOTIFY` - lets us drop cached state when devices change.
const RIDEV_DEVNOTIFY_FLAG: u32 = 0x0000_2000;
/// A press must exceed the deep threshold for this many consecutive reports.
const DEEP_FRAMES_REQUIRED: i32 = 2;
/// Minimum rise above the pressure the press started at.
const DEEP_RISE_REQUIRED: i32 = 20;
/// A new touch session starts when this much time passed since the last report.
const SESSION_GAP: Duration = Duration::from_millis(350);

/// HID report from the touchpad: `report[0]` is the report id.
const SLOT_SIZE: usize = 7;
const FIRST_SLOT_OFFSET: usize = 1;
const MIN_TRAILER_BYTES: usize = 3;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static PRESS: OnceLock<std::sync::Mutex<PressState>> = OnceLock::new();
/// Whether the last report carried any pressure, used to wake the UI once.
static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One finger reported by the touchpad.
struct Contact {
    tip: bool,
    confidence: bool,
    pressure: u16,
}

/// A single decoded HID report.
struct Report {
    button: bool,
    contacts: Vec<Contact>,
}

impl Report {
    fn has_interaction(&self) -> bool {
        self.button
            || self.contacts.iter().any(|contact| contact.tip || contact.confidence)
    }

    fn pressure(&self) -> i32 {
        self.contacts
            .iter()
            .filter(|contact| contact.tip || contact.confidence)
            .map(|contact| contact.pressure as i32)
            .max()
            .unwrap_or(0)
    }
}

/// Everything needed to recognise one deep press.
#[derive(Default)]
struct PressState {
    active: bool,
    deep_pressed: bool,
    press_start_pressure: i32,
    last_pressure: i32,
    deep_frames: i32,
    last_report: Option<Instant>,
}

impl PressState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn reset_with(&mut self, pressure: i32) {
        *self = Self {
            last_pressure: pressure,
            last_report: self.last_report,
            ..Default::default()
        };
    }
}

/// Spawn the raw-input listener on its own thread.
pub fn spawn(shared: Arc<Shared>) {
    let _ = SHARED.set(shared);
    let _ = PRESS.set(std::sync::Mutex::new(PressState::default()));

    std::thread::Builder::new()
        .name("touchpad-input".to_string())
        .spawn(|| unsafe { run() })
        .expect("failed to spawn the touchpad thread");
}

unsafe fn run() {
    let class_name = win::wide("MP14Tools.TouchpadInput");
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: win::module_instance(),
        lpszClassName: win::pcw(&class_name),
        ..Default::default()
    };

    if RegisterClassW(&class) == 0 {
        crate::log::line("touchpad: RegisterClassW failed");
        return;
    }

    let window = match CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        win::pcw(&class_name),
        win::pcw(&class_name),
        WS_POPUP,
        0,
        0,
        0,
        0,
        None,
        None,
        Some(win::module_instance()),
        None,
    ) {
        Ok(window) => window,
        Err(error) => {
            crate::log::line(&format!("touchpad: CreateWindowExW failed: {error}"));
            return;
        }
    };

    register_raw_input(window);

    let mut message = Default::default();
    while GetMessageW(&mut message, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&message);
        let _ = DispatchMessageW(&message);
    }
}

unsafe fn register_raw_input(window: HWND) {
    let device = RAWINPUTDEVICE {
        usUsagePage: 0x0D, // Digitizer
        usUsage: 0x05,     // Touch Pad
        dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK.0 | RIDEV_DEVNOTIFY_FLAG),
        hwndTarget: window,
    };

    match RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) {
        Ok(()) => {
            if let Some(shared) = SHARED.get() {
                shared.touchpad_registered.store(true, Ordering::SeqCst);
            }
            crate::log::line("touchpad: raw input registered (0x0D/0x05)");
        }
        Err(error) => {
            crate::log::line(&format!("touchpad: RegisterRawInputDevices failed: {error}"));
        }
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        process_raw_input(HRAWINPUT(lparam.0 as *mut core::ffi::c_void));
    }

    DefWindowProcW(window, message, wparam, lparam)
}

/// Pull the HID report out of a `WM_INPUT` payload.
///
/// Fields are read with explicit little-endian offsets rather than by casting
/// the byte buffer to `RAWINPUT`, which keeps the code free of alignment
/// assumptions.
unsafe fn process_raw_input(handle: HRAWINPUT) {
    let mut size = 0u32;
    let header_bytes = size_of::<windows::Win32::UI::Input::RAWINPUTHEADER>() as u32;

    if GetRawInputData(handle, RID_INPUT, None, &mut size, header_bytes) == u32::MAX || size == 0 {
        return;
    }

    let mut buffer = vec![0u8; size as usize];
    if GetRawInputData(
        handle,
        RID_INPUT,
        Some(buffer.as_mut_ptr().cast()),
        &mut size,
        header_bytes,
    ) == u32::MAX
    {
        return;
    }

    let header_bytes = header_bytes as usize;
    if buffer.len() < header_bytes + 8 {
        return;
    }

    let device_type = read_u32(&buffer, 0);
    if device_type != RIM_TYPEHID.0 {
        return;
    }

    let hid_size = read_u32(&buffer, header_bytes) as usize;
    let hid_count = read_u32(&buffer, header_bytes + 4) as usize;
    if hid_size == 0 {
        return;
    }

    let data = &buffer[header_bytes + 8..];
    for index in 0..hid_count {
        let start = index * hid_size;
        if start + hid_size > data.len() {
            break;
        }
        let report = data[start..start + hid_size].to_vec();
        process_report(&report);
    }
}

fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
    ])
}

/// Decode one HID report: one byte of flags, then 7-byte contact slots
/// (flags, X, Y, pressure), followed by scan time, contact count and buttons.
fn decode(report: &[u8]) -> Option<Report> {
    if report.len() < FIRST_SLOT_OFFSET + SLOT_SIZE + MIN_TRAILER_BYTES || report[0] != 0x04 {
        return None;
    }

    let slot_count = (report.len() - FIRST_SLOT_OFFSET - MIN_TRAILER_BYTES) / SLOT_SIZE;
    if slot_count == 0 {
        return None;
    }

    let mut contacts = Vec::with_capacity(slot_count);
    for slot in 0..slot_count {
        let offset = FIRST_SLOT_OFFSET + slot * SLOT_SIZE;
        if offset + 6 >= report.len() {
            break;
        }

        let flags = report[offset];
        contacts.push(Contact {
            tip: flags & 0x01 != 0,
            confidence: flags & 0x02 != 0,
            pressure: u16::from_le_bytes([report[offset + 5], report[offset + 6]]),
        });
    }

    if contacts.is_empty() {
        return None;
    }

    let trailer = FIRST_SLOT_OFFSET + contacts.len() * SLOT_SIZE;
    let button = trailer + 3 < report.len() && (report[trailer + 3] & 0x01) != 0;

    Some(Report { button, contacts })
}

fn process_report(report: &[u8]) {
    let Some(shared) = SHARED.get() else {
        return;
    };
    let Some(state) = PRESS.get() else {
        return;
    };

    let parsed = decode(report);
    let pressure = parsed.as_ref().map(Report::pressure).unwrap_or(0);
    shared.set_pressure(pressure);

    // Wake the settings window only on the idle -> touching transition. The UI
    // then keeps its own short cadence while pressure is present, so a window
    // sitting on the touchpad tab costs nothing when nobody is touching it.
    if !ACTIVE.swap(pressure > 0, Ordering::Relaxed) && pressure > 0 {
        shared.wake_ui();
    }

    if !shared.touchpad_enabled() {
        return;
    }

    let Some(parsed) = parsed else {
        return;
    };
    if !parsed.has_interaction() {
        if let Ok(mut state) = state.lock() {
            state.reset();
        }
        return;
    }

    let (light, deep, action, label) = match shared.config.read() {
        Ok(config) => (
            config.touchpad.light_press_threshold as i32,
            config.touchpad.deep_press_threshold as i32,
            config.touchpad.action.clone(),
            "触摸板重按".to_string(),
        ),
        Err(_) => return,
    };

    let rearm = (light - 20).max(20);
    let triggered = {
        let Ok(mut state) = state.lock() else {
            return;
        };
        state.observe(pressure, light, deep, rearm, Instant::now())
    };

    if triggered {
        dispatch(shared, &action, &label);
    }
}

impl PressState {
    /// Advance the state machine by one report. Returns true on the exact frame
    /// the deep press is recognised.
    ///
    /// All three bounds come straight out of the configuration, so the settings
    /// window is in full control of the behaviour: `light` is the pressure a
    /// touch has to reach before it counts as an intentional press, `deep` is the
    /// pressure that fires, and `rearm` is what the pressure has to fall back to
    /// before a new press can start.
    fn observe(&mut self, pressure: i32, light: i32, deep: i32, rearm: i32, now: Instant) -> bool {
        // A long silence means a new touch session: never let a stale "already
        // fired" flag swallow the next press.
        let stale = self
            .last_report
            .map(|previous| now.duration_since(previous) > SESSION_GAP)
            .unwrap_or(false);
        if self.active && stale {
            self.reset_with(pressure);
        }
        self.last_report = Some(now);

        // Everything below the light threshold is a resting finger, not a press.
        if !self.active {
            if pressure >= light {
                self.active = true;
                self.deep_pressed = false;
                self.press_start_pressure = pressure;
                self.deep_frames = 0;
            }
            self.last_pressure = pressure;
            return false;
        }

        if pressure <= rearm {
            self.reset_with(pressure);
            return false;
        }

        let above_threshold =
            pressure >= deep && pressure - self.press_start_pressure >= DEEP_RISE_REQUIRED;
        self.deep_frames = if above_threshold { self.deep_frames + 1 } else { 0 };

        let mut triggered = false;
        if !self.deep_pressed && self.deep_frames >= DEEP_FRAMES_REQUIRED {
            self.deep_pressed = true;
            triggered = true;
        }

        // Release condition, mirroring the reference implementation so a slow
        // release cannot fire twice.
        let release_floor = rearm.max(deep - 40);
        if self.deep_pressed && (pressure <= release_floor || pressure - self.last_pressure <= -25) {
            self.deep_pressed = false;
            self.deep_frames = 0;
        }

        self.last_pressure = pressure;
        triggered
    }
}
