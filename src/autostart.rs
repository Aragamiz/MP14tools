//! Run-at-login, implemented as a value under the per-user `Run` key.
//!
//! Deliberately *not* a service or scheduled task: one HKCU value, removed again
//! when the user turns the switch off.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ,
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
/// Name shown in Task Manager's startup tab.
const VALUE_NAME: PCWSTR = w!("MP14Tools");
/// Name the value had before the product was renamed.
const LEGACY_VALUE_NAME: PCWSTR = w!("MeowBoxLite");

/// Current `Run` value, if any.
pub fn current() -> Option<String> {
    let key = open(KEY_QUERY_VALUE).ok()?;

    let mut kind = REG_SZ;
    let mut size = 0u32;

    unsafe {
        // First call only asks for the size.
        if RegQueryValueExW(key, VALUE_NAME, None, Some(&mut kind), None, Some(&mut size))
            != ERROR_SUCCESS
            || size == 0
        {
            let _ = RegCloseKey(key);
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let status = RegQueryValueExW(
            key,
            VALUE_NAME,
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        if status != ERROR_SUCCESS {
            return None;
        }

        buffer.truncate(size as usize);
        let units: Vec<u16> = buffer
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect();
        Some(crate::win::from_wide(&units))
    }
}

/// Is autostart pointing at *this* executable?
pub fn is_enabled() -> bool {
    let Some(value) = current() else {
        return false;
    };
    let Some(executable) = std::env::current_exe().ok() else {
        return false;
    };
    value
        .trim()
        .trim_matches('"')
        .eq_ignore_ascii_case(&executable.display().to_string())
}

/// Add or remove the autostart entry.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        set_value(&format!("\"{}\"", executable.display()))
    } else {
        remove_value()
    }
}

/// Delete the entry written by the pre-rename build, whose command line points
/// at an executable name that no longer exists.
pub fn remove_legacy_entry() {
    let Ok(key) = open(KEY_SET_VALUE) else {
        return;
    };
    let status = unsafe { RegDeleteValueW(key, LEGACY_VALUE_NAME) };
    unsafe {
        let _ = RegCloseKey(key);
    }

    if status == ERROR_SUCCESS {
        crate::log::line("removed the autostart entry of the previous product name");
    }
}

fn open(access: REG_SAM_FLAGS) -> Result<HKEY, String> {
    let mut key = HKEY::default();
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), access, &mut key) };
    check(status, "RegOpenKeyExW(Run)")?;
    Ok(key)
}

fn set_value(command_line: &str) -> Result<(), String> {
    let key = open(KEY_SET_VALUE)?;
    let mut data = crate::win::wide(command_line);
    // REG_SZ counts the terminating NUL in its byte length.
    let bytes =
        unsafe { std::slice::from_raw_parts(data.as_mut_ptr() as *const u8, data.len() * 2) };

    let status = unsafe { RegSetValueExW(key, VALUE_NAME, None, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(key);
    }

    check(status, "RegSetValueExW(MP14Tools)")
}

fn remove_value() -> Result<(), String> {
    let key = open(KEY_SET_VALUE)?;
    let status = unsafe { RegDeleteValueW(key, VALUE_NAME) };
    unsafe {
        let _ = RegCloseKey(key);
    }

    // Removing a value that is not there is the outcome the caller wanted.
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }

    check(status, "RegDeleteValueW(MP14Tools)")
}

fn check(status: WIN32_ERROR, what: &str) -> Result<(), String> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{what} failed with {status:?}"))
    }
}
