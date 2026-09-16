//! Small Win32 helpers shared by the other modules.

use windows::core::PCWSTR;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

/// Wide (UTF-16, NUL-terminated) representation of `text`.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `PCWSTR` pointing at a caller-owned buffer produced by [`wide`].
///
/// The buffer must outlive the returned pointer - keep it in a local binding.
pub fn pcw(buffer: &[u16]) -> PCWSTR {
    PCWSTR(buffer.as_ptr())
}

/// Handle of the running executable, as required by window-class registration.
pub fn module_instance() -> HINSTANCE {
    unsafe {
        match GetModuleHandleW(None) {
            Ok(module) => HINSTANCE(module.0),
            Err(_) => HINSTANCE(std::ptr::null_mut()),
        }
    }
}

/// Copy a NUL-terminated UTF-16 buffer into a `String`, dropping the terminator.
pub fn from_wide(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}
