//! Optional console window ("命令提示符") for the running instance.
//!
//! Release builds use the `windows` subsystem, so they start without a console
//! and the log only exists as a file. With `console` enabled in the
//! configuration the process allocates one of its own at startup, which makes
//! the very same lines visible while the tool runs - useful when a key or a
//! pressure threshold does not behave as expected.
//!
//! Two details drive the implementation:
//!
//! * the console has to exist *before* the first `println!`, because the Rust
//!   standard library caches the standard handles the first time they are used.
//!   Everything written through this module therefore goes to a handle of its
//!   own (`CONOUT$`), which also makes switching the console off at runtime
//!   possible.
//! * a debug build started from a terminal already has a console, and `println!`
//!   is already writing to it. In that case nothing is allocated and this module
//!   stays silent, so lines are never printed twice.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::System::Console::{
    AllocConsole, FreeConsole, GetConsoleWindow, SetConsoleOutputCP, SetConsoleTitleW,
};

/// Title of the window, so it is obvious which process it belongs to.
pub const TITLE: &str = "MP14Tools 日志";

/// UTF-8, matching what [`write`] sends to the console.
const CODEPAGE_UTF8: u32 = 65001;

/// True when *we* own the console, i.e. when this module is the only writer.
/// An inherited console (debug build in a terminal) leaves this `false`.
static OWNED: AtomicBool = AtomicBool::new(false);

static SINK: OnceLock<Mutex<Option<File>>> = OnceLock::new();

fn sink() -> &'static Mutex<Option<File>> {
    SINK.get_or_init(|| Mutex::new(None))
}

/// Bring the console in line with `enabled`. Idempotent, so it can be called
/// from the startup path, from the configuration watcher and from the settings
/// window without any bookkeeping.
pub fn sync(enabled: bool) {
    if enabled {
        attach();
    } else {
        detach();
    }
}

/// Create and take over a console window. Returns false when the system refuses.
pub fn attach() -> bool {
    if OWNED.load(Ordering::SeqCst) {
        return true;
    }

    unsafe {
        // A console may already be attached (debug build, or a second call after
        // a failed detach). Only allocate one when there really is none.
        if GetConsoleWindow().0.is_null() {
            if AllocConsole().is_err() {
                return false;
            }
            OWNED.store(true, Ordering::SeqCst);
        }

        // Without this the Chinese log lines would arrive as mojibake.
        let _ = SetConsoleOutputCP(CODEPAGE_UTF8);
        let title = crate::win::wide(TITLE);
        let _ = SetConsoleTitleW(PCWSTR(title.as_ptr()));
    }

    match OpenOptions::new().write(true).open("CONOUT$") {
        Ok(file) => {
            if let Ok(mut slot) = sink().lock() {
                *slot = Some(file);
            }
            true
        }
        Err(error) => {
            // Without a writable handle the window would stay empty, so hand it
            // back instead of leaving a blank box on screen.
            OWNED.store(false, Ordering::SeqCst);
            unsafe {
                let _ = FreeConsole();
            }
            // Logged only here: a console that works is not worth a line.
            crate::log::line(&format!("console: could not write to it ({error})"));
            false
        }
    }
}

/// Close our handle and release the window. A console that the process did not
/// create (the terminal of a debug build) is deliberately left alone.
pub fn detach() {
    if !OWNED.swap(false, Ordering::SeqCst) {
        return;
    }

    if let Ok(mut slot) = sink().lock() {
        if let Some(mut file) = slot.take() {
            let _ = file.flush();
        }
    }

    unsafe {
        let _ = FreeConsole();
    }
}

/// Write one line to the console. Does nothing unless this module owns it.
///
/// `\r\n` is written explicitly: a console device opened as a file performs no
/// newline translation.
pub fn write(text: &str) {
    if !OWNED.load(Ordering::SeqCst) {
        return;
    }

    if let Ok(mut slot) = sink().lock() {
        if let Some(file) = slot.as_mut() {
            let _ = file.write_all(text.as_bytes());
            let _ = file.write_all(b"\r\n");
            let _ = file.flush();
        }
    }
}
