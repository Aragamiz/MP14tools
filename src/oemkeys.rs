//! OEM / vendor hotkey capture through WMI event subscriptions.
//!
//! Vendor hotkeys on this class of laptop are not ordinary keystrokes: the
//! embedded controller publishes a HID report as a WMI *event* under
//! `ROOT\WMI` (`HID_EVENT20` ... `HID_EVENT23`). We subscribe with
//! `ExecNotificationQuery` and pull events with the synchronous `Next`, which
//! blocks without consuming CPU until something actually happens.
//!
//! One thread per WMI class; every thread spends its life blocked in `Next`.

use std::sync::Arc;

use windows::core::{BSTR, GUID};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayUnaccessData,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{VariantClear, VARIANT};
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

use crate::config::{normalize_hex, OemKey};
use crate::state::{dispatch, mark_wmi_ready, Shared};

/// `CLSID_WbemLocator` - spelled out so the crate does not depend on the
/// generated constant being present under a particular feature set.
const CLSID_WBEM_LOCATOR: GUID = GUID::from_u128(0x4590_f811_1d3a_11d0_891f_00aa_004b_2e24);

const WMI_NAMESPACE: &str = "ROOT\\WMI";

/// Vendor event classes. Machines differ in which one they use, so all four are
/// watched and the report prefix decides which key actually matched.
const WMI_CLASSES: &[&str] = &["HID_EVENT20", "HID_EVENT21", "HID_EVENT22", "HID_EVENT23"];

const STACK_SIZE: usize = 512 * 1024;
const VT_UI1: u16 = 0x11;
const VT_ARRAY: u16 = 0x2000;
const VT_BOOL: u16 = 0x000B;

/// Start one listener thread per vendor event class.
pub fn spawn(shared: Arc<Shared>) {
    for class in WMI_CLASSES {
        let thread_name = format!("wmi-{class}");
        let thread_shared = shared.clone();
        let thread_class = (*class).to_string();

        let spawned = std::thread::Builder::new()
            .name(thread_name)
            .stack_size(STACK_SIZE)
            .spawn(move || match run(&thread_shared, &thread_class) {
                Ok(()) => crate::log::line(&format!("wmi {thread_class}: subscription ended")),
                Err(error) => crate::log::line(&format!("wmi {thread_class}: unavailable ({error})")),
            });

        if let Err(error) = spawned {
            crate::log::line(&format!("wmi {class}: thread spawn failed: {error}"));
        }
    }
}

fn run(shared: &Arc<Shared>, class: &str) -> windows::core::Result<()> {
    unsafe {
        // The thread owns its COM apartment for its whole life.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let locator: IWbemLocator =
            CoCreateInstance(&CLSID_WBEM_LOCATOR, None, CLSCTX_INPROC_SERVER)?;

        let services: IWbemServices = locator.ConnectServer(
            &BSTR::from(WMI_NAMESPACE),
            &BSTR::new(),
            &BSTR::new(),
            &BSTR::new(),
            0,
            &BSTR::new(),
            None,
        )?;

        // Without this the notification query comes back with access denied.
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            None,
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )?;

        let language = BSTR::from("WQL");
        let query = BSTR::from(format!("SELECT * FROM {class}"));
        let flags = WBEM_FLAG_RETURN_IMMEDIATELY | WBEM_FLAG_FORWARD_ONLY;

        let enumerator: IEnumWbemClassObject =
            services.ExecNotificationQuery(&language, &query, flags, None)?;

        mark_wmi_ready(shared);
        crate::log::line(&format!("wmi {class}: subscribed"));

        loop {
            let mut objects: [Option<IWbemClassObject>; 1] = [None];
            let mut returned = 0u32;

            // Blocks until the next event - no polling, no idle CPU.
            let result = enumerator.Next(WBEM_INFINITE, &mut objects, &mut returned);
            if result.is_err() || returned == 0 {
                break;
            }

            if let Some(object) = objects[0].take() {
                handle_object(shared, &object);
            }
        }
    }

    Ok(())
}

/// Extract the report and the `Active` flag, then hand off to the matcher.
unsafe fn handle_object(shared: &Arc<Shared>, object: &IWbemClassObject) {
    let Some(report) = read_property_bytes(object, "EventDetail") else {
        return;
    };
    let active = read_property_bool(object, "Active");

    let report_hex = to_hex(&report);
    let matched = {
        let Ok(config) = shared.config.read() else {
            return;
        };
        match_key(&config.oem_keys, &report_hex, active).cloned()
    };

    if let Some(key) = matched {
        dispatch(shared, &key.action, &key.name);
    }
}

/// Longest matching report prefix wins, so a specific entry can override a
/// broader one.
fn match_key<'a>(keys: &'a [OemKey], report_hex: &str, active: Option<bool>) -> Option<&'a OemKey> {
    let report = normalize_hex(report_hex);
    if report.is_empty() {
        return None;
    }

    let mut best: Option<(&OemKey, usize)> = None;
    for key in keys.iter().filter(|key| key.enabled) {
        // A key that only fires on press must not also fire on release.
        if key.press_only && active == Some(false) {
            continue;
        }

        let prefix = key.normalized_prefix();
        if prefix.is_empty() || !report.starts_with(&prefix) {
            continue;
        }

        if best.map(|(_, length)| prefix.len() > length).unwrap_or(true) {
            best = Some((key, prefix.len()));
        }
    }

    best.map(|(key, _)| key)
}

fn to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join("-")
}

unsafe fn read_property(object: &IWbemClassObject, name: &str) -> Option<VARIANT> {
    let mut value = VARIANT::default();
    let property = BSTR::from(name);
    object.Get(&property, 0, &mut value, None, None).ok()?;
    Some(value)
}

unsafe fn read_property_bytes(object: &IWbemClassObject, name: &str) -> Option<Vec<u8>> {
    let mut value = read_property(object, name)?;

    if value.Anonymous.Anonymous.vt.0 != (VT_ARRAY | VT_UI1) {
        let _ = VariantClear(&mut value);
        return None;
    }

    let array = value.Anonymous.Anonymous.Anonymous.parray;
    let bytes = read_safe_array(array);
    let _ = VariantClear(&mut value);
    bytes
}

unsafe fn read_property_bool(object: &IWbemClassObject, name: &str) -> Option<bool> {
    let mut value = read_property(object, name)?;

    let result = if value.Anonymous.Anonymous.vt.0 == VT_BOOL {
        Some(value.Anonymous.Anonymous.Anonymous.boolVal.0 != 0)
    } else {
        None
    };

    let _ = VariantClear(&mut value);
    result
}

unsafe fn read_safe_array(array: *mut windows::Win32::System::Com::SAFEARRAY) -> Option<Vec<u8>> {
    if array.is_null() {
        return None;
    }

    let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
    SafeArrayAccessData(array, &mut data).ok()?;

    let lower = SafeArrayGetLBound(array, 1).ok();
    let upper = SafeArrayGetUBound(array, 1).ok();

    let bytes = match (lower, upper) {
        (Some(lower), Some(upper)) if upper >= lower && !data.is_null() => {
            let length = (upper - lower + 1) as usize;
            Some(std::slice::from_raw_parts(data as *const u8, length).to_vec())
        }
        _ => None,
    };

    let _ = SafeArrayUnaccessData(array);
    bytes
}
