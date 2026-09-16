//! Displays, the power source, and HDR.
//!
//! Three unrelated Win32 areas, kept in one module because the feature built on
//! top of them - "drop to 60 Hz on battery, and offer to turn HDR off" - always
//! needs all three at once:
//!
//! * the refresh rate lives in the GDI `DEVMODE` world (`EnumDisplaySettingsW` /
//!   `ChangeDisplaySettingsExW`), addressed by adapter name (`\\.\DISPLAY1`);
//! * HDR lives in the DisplayConfig world (`QueryDisplayConfig` +
//!   `DisplayConfigGet/SetDeviceInfo`), addressed by a target id;
//! * the power source comes from `GetSystemPowerStatus`.
//!
//! [`list`] resolves both address spaces into a single struct, so callers never
//! have to know that these are two different APIs and two different naming
//! schemes.
//!
//! Everything here is read-only until one of the `set_*` functions is called.

use std::mem::{size_of, zeroed};
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes,
    QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{LUID, ERROR_SUCCESS};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplaySettingsW, DEVMODEW, DISP_CHANGE_SUCCESSFUL,
    DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_CURRENT_SETTINGS,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

/// One active display, in both address spaces at once.
#[derive(Clone, Debug)]
pub struct Display {
    /// GDI adapter name, e.g. `\\.\DISPLAY1` - the key for the mode APIs.
    pub adapter: String,
    /// What the monitor calls itself, for the settings window and the log.
    pub label: String,
    /// True for the built-in panel, false for anything that was plugged in.
    pub internal: bool,
    pub hdr_supported: bool,
    pub hdr_enabled: bool,
    pub width: u32,
    pub height: u32,
    pub refresh: u32,
    /// Target the HDR APIs address. Not part of the public surface: callers get
    /// through [`set_hdr`], which is the only thing that needs it.
    target: Target,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Target {
    adapter: LUID,
    id: u32,
}

impl Display {
    /// Short description used in notifications, e.g. `内屏 (3120×2080)`.
    pub fn describe(&self) -> String {
        format!(
            "{} ({}×{}, {} Hz)",
            self.label, self.width, self.height, self.refresh
        )
    }
}

/// Every active display, internal first so notifications read naturally.
pub fn list() -> Vec<Display> {
    let mut displays: Vec<Display> = query_paths()
        .into_iter()
        .filter_map(describe_path)
        .collect();

    // Stable and useful ordering: the built-in panel is the one the user thinks
    // of as "the screen".
    displays.sort_by_key(|display| (!display.internal, display.adapter.clone()));
    displays
}

/// Current refresh rate of an adapter, in Hz.
pub fn current_rate(adapter: &str) -> Option<u32> {
    let (_, _, hz) = current_mode(adapter)?;
    Some(hz)
}

/// Refresh rates the adapter offers at `width`×`height`, ascending.
pub fn frequencies(adapter: &str, width: u32, height: u32) -> Vec<u32> {
    let name = crate::win::wide(adapter);
    let mut rates = Vec::new();
    let mut index = 0;

    loop {
        let mut mode: DEVMODEW = unsafe { zeroed() };
        mode.dmSize = size_of::<DEVMODEW>() as u16;

        let more = unsafe {
            EnumDisplaySettingsW(
                PCWSTR(name.as_ptr()),
                windows::Win32::Graphics::Gdi::ENUM_DISPLAY_SETTINGS_MODE(index),
                &mut mode,
            )
        };
        if !more.as_bool() {
            break;
        }
        index += 1;

        if mode.dmPelsWidth == width && mode.dmPelsHeight == height && mode.dmDisplayFrequency > 1 {
            rates.push(mode.dmDisplayFrequency);
        }
    }

    rates.sort_unstable();
    rates.dedup();
    rates
}

/// Highest rate the adapter offers at `width`×`height` that is at most `limit`.
///
/// Falls back to the lowest rate above the limit when the panel cannot reach it
/// at all, mirroring what the reference tool does - a wrong-but-close rate beats
/// leaving the display alone in a state the user explicitly asked to change.
pub fn rate_at_most(adapter: &str, width: u32, height: u32, limit: u32) -> Option<u32> {
    let rates = frequencies(adapter, width, height);
    rates
        .iter()
        .rev()
        .find(|rate| **rate <= limit)
        .copied()
        .or_else(|| rates.first().copied())
}

/// Highest rate the adapter offers at `width`×`height`.
#[allow(dead_code)]
pub fn highest_rate(adapter: &str, width: u32, height: u32) -> Option<u32> {
    frequencies(adapter, width, height).into_iter().max()
}

/// Switch an adapter to `hz` at its current resolution.
pub fn set_rate(adapter: &str, hz: u32) -> Result<(), String> {
    let (width, height, _) = current_mode(adapter)
        .ok_or_else(|| format!("{adapter}: could not read the current mode"))?;
    set_mode(adapter, width, height, hz)
}

/// Switch an adapter to `width`×`height` at `hz`.
///
/// The change is written to the registry as well, so it survives a reboot - the
/// same thing every display settings dialog does.
pub fn set_mode(adapter: &str, width: u32, height: u32, hz: u32) -> Result<(), String> {
    let name = crate::win::wide(adapter);

    let mut mode: DEVMODEW = unsafe { zeroed() };
    mode.dmSize = size_of::<DEVMODEW>() as u16;
    if !unsafe {
        EnumDisplaySettingsW(
            PCWSTR(name.as_ptr()),
            ENUM_CURRENT_SETTINGS,
            &mut mode,
        )
    }
    .as_bool()
    {
        return Err(format!("{adapter}: could not read the current mode"));
    }

    mode.dmPelsWidth = width;
    mode.dmPelsHeight = height;
    mode.dmDisplayFrequency = hz;
    mode.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;

    let result = unsafe {
        ChangeDisplaySettingsExW(
            PCWSTR(name.as_ptr()),
            Some(&mode as *const DEVMODEW),
            None,
            windows::Win32::Graphics::Gdi::CDS_UPDATEREGISTRY,
            None,
        )
    };

    if result == DISP_CHANGE_SUCCESSFUL {
        Ok(())
    } else {
        // The negative values are the documented failure codes; reporting the
        // number is what makes the log useful.
        Err(format!(
            "{adapter}: display change to {width}×{height} {hz} Hz rejected ({})",
            result.0
        ))
    }
}

/// Turn HDR (advanced colour) of one display on or off.
pub fn set_hdr(display: &Display, enabled: bool) -> Result<(), String> {
    let mut state: DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE = unsafe { zeroed() };
    state.header.r#type = DISPLAYCONFIG_DEVICE_INFO_SET_ADVANCED_COLOR_STATE;
    state.header.size = size_of::<DISPLAYCONFIG_SET_ADVANCED_COLOR_STATE>() as u32;
    state.header.adapterId = display.target.adapter;
    state.header.id = display.target.id;
    // Bit 0 is `enableAdvancedColor`.
    state.Anonymous.value = u32::from(enabled);

    let status = unsafe { DisplayConfigSetDeviceInfo(&state.header) };
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "{}: could not {} HDR ({status})",
            display.adapter,
            if enabled { "enable" } else { "disable" }
        ))
    }
}

/// `Some(true)` on AC, `Some(false)` on battery, `None` when the machine cannot
/// tell (a desktop, or a VM).
pub fn on_battery() -> Option<bool> {
    let mut status: SYSTEM_POWER_STATUS = unsafe { zeroed() };
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;

    match status.ACLineStatus {
        0 => Some(true),
        1 => Some(false),
        // 255 is documented as "unknown".
        _ => None,
    }
}

fn current_mode(adapter: &str) -> Option<(u32, u32, u32)> {
    mode_at(adapter, ENUM_CURRENT_SETTINGS)
}

fn mode_at(
    adapter: &str,
    which: windows::Win32::Graphics::Gdi::ENUM_DISPLAY_SETTINGS_MODE,
) -> Option<(u32, u32, u32)> {
    let name = crate::win::wide(adapter);
    let mut mode: DEVMODEW = unsafe { zeroed() };
    mode.dmSize = size_of::<DEVMODEW>() as u16;

    if !unsafe { EnumDisplaySettingsW(PCWSTR(name.as_ptr()), which, &mut mode) }.as_bool() {
        return None;
    }

    Some((mode.dmPelsWidth, mode.dmPelsHeight, mode.dmDisplayFrequency))
}

/// One entry of `QueryDisplayConfig`, with both names already resolved.
struct Path {
    gdi_name: String,
    label: String,
    output: i32,
    target: Target,
}

/// Active display paths, each with its GDI name and monitor name resolved.
fn query_paths() -> Vec<Path> {
    let mut path_count = 0u32;
    let mut mode_count = 0u32;

    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count) }
        != ERROR_SUCCESS
    {
        return Vec::new();
    }
    if path_count == 0 {
        return Vec::new();
    }

    let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![unsafe { zeroed() }; path_count as usize];
    let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![unsafe { zeroed() }; mode_count as usize];

    // The display topology can change between the size query and the query
    // itself; the API then asks to be called again with fresh sizes.
    if unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    } != ERROR_SUCCESS
    {
        return Vec::new();
    }
    paths.truncate(path_count as usize);

    paths
        .iter()
        .filter_map(|path| {
            let gdi_name = source_name(path)?;
            Some(Path {
                gdi_name,
                label: target_name(path).unwrap_or_else(|| "Display".to_string()),
                output: path.targetInfo.outputTechnology.0,
                target: Target {
                    adapter: path.targetInfo.adapterId,
                    id: path.targetInfo.id,
                },
            })
        })
        .collect()
}

/// Turn a resolved path into a [`Display`] by adding its live mode and HDR state.
fn describe_path(path: Path) -> Option<Display> {
    let (width, height, refresh) = current_mode(&path.gdi_name)?;
    let (hdr_supported, hdr_enabled) = hdr_info(path.target).unwrap_or((false, false));

    Some(Display {
        adapter: path.gdi_name,
        label: path.label,
        internal: path.output == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL.0,
        hdr_supported,
        hdr_enabled,
        width,
        height,
        refresh,
        target: path.target,
    })
}

/// GDI name of the source behind a path, e.g. `\\.\DISPLAY1`.
fn source_name(path: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut name: DISPLAYCONFIG_SOURCE_DEVICE_NAME = unsafe { zeroed() };
    name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
    name.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
    name.header.adapterId = path.sourceInfo.adapterId;
    name.header.id = path.sourceInfo.id;

    if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } != 0 {
        return None;
    }

    let name = crate::win::from_wide(&name.viewGdiDeviceName);
    (!name.is_empty()).then_some(name)
}

/// Monitor name reported by the display itself.
fn target_name(path: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut name: DISPLAYCONFIG_TARGET_DEVICE_NAME = unsafe { zeroed() };
    name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
    name.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
    name.header.adapterId = path.targetInfo.adapterId;
    name.header.id = path.targetInfo.id;

    if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } != 0 {
        return None;
    }

    let name = crate::win::from_wide(&name.monitorFriendlyDeviceName);
    (!name.is_empty()).then_some(name)
}

/// `(supported, enabled)` for HDR on a target.
fn hdr_info(target: Target) -> Option<(bool, bool)> {
    /// Bit 0: the display supports advanced colour; bit 1: it is on.
    const SUPPORTED: u32 = 0x1;
    const ENABLED: u32 = 0x2;

    let mut info: DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO = unsafe { zeroed() };
    info.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
    info.header.size = size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
    info.header.adapterId = target.adapter;
    info.header.id = target.id;

    if unsafe { DisplayConfigGetDeviceInfo(&mut info.header) } != 0 {
        // An older driver that does not know the query at all: report "no HDR"
        // rather than failing the whole enumeration.
        return None;
    }

    let flags = unsafe { info.Anonymous.value };
    Some((flags & SUPPORTED != 0, flags & ENABLED != 0))
}

/// Cached copy of the display list, refreshed on demand.
///
/// The list is asked for from three places (settings window, power events, the
/// conflict check), and each enumeration costs a few hundred microseconds -
/// cheap, but not cheap enough to repeat inside a repaint.
static CACHE: OnceLock<std::sync::Mutex<Option<(std::time::Instant, Vec<Display>)>>> = OnceLock::new();

/// How long a cached enumeration stays valid.
const CACHE_TTL: std::time::Duration = std::time::Duration::from_millis(750);

/// [`list`], but reuses an enumeration younger than [`CACHE_TTL`].
pub fn list_cached() -> Vec<Display> {
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(cache) = cache.lock() {
        if let Some((stamp, displays)) = cache.as_ref() {
            if stamp.elapsed() < CACHE_TTL {
                return displays.clone();
            }
        }
    }

    let displays = list();
    if let Ok(mut cache) = cache.lock() {
        *cache = Some((std::time::Instant::now(), displays.clone()));
    }
    displays
}

/// Drop the cache, so the next [`list_cached`] sees the current topology.
pub fn invalidate() {
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut cache) = cache.lock() {
        *cache = None;
    }
}
