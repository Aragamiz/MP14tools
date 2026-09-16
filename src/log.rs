//! Append-only log file, used instead of a console because release builds have
//! no console subsystem.
//!
//! Kept intentionally tiny: one `Mutex<Sink>` and no rotation. The file lives
//! next to the configuration by default and can be moved to any directory
//! through the settings window - the directory is part of the configuration, so
//! it survives a restart and can also be changed while the tool runs.
//!
//! Every line also goes to the console window when one is showing (see
//! [`crate::console`]), which is what makes the optional command prompt useful.

use std::fs::OpenOptions;
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Name of the file inside the configured directory.
const FILE_NAME: &str = "mp14tools.log";

/// The open file plus the path it was opened from, so the location can change
/// at runtime without losing the indirection.
struct Sink {
    file: std::fs::File,
    path: PathBuf,
}

/// Set before the first line is written; ignored afterwards, because from then
/// on the sink itself carries the location.
static DESIRED: Mutex<Option<PathBuf>> = Mutex::new(None);
static LOG: OnceLock<Mutex<Sink>> = OnceLock::new();

fn sink() -> &'static Mutex<Sink> {
    LOG.get_or_init(|| {
        // A directory chosen before the first line wins; that is how the startup
        // path keeps the file out of the default location entirely.
        let path = DESIRED
            .lock()
            .ok()
            .and_then(|desired| desired.clone())
            .unwrap_or_else(|| crate::config::data_dir().join(FILE_NAME));
        Mutex::new(open(&path))
    })
}

/// Open `path`, creating its directory, and fall back to `%TEMP%` when that is
/// impossible - a tool that silently stops logging is worse than one that logs
/// in an unexpected place.
fn open(path: &Path) -> Sink {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(file) = OpenOptions::new().create(true).append(true).open(path) {
        return Sink {
            file,
            path: path.to_path_buf(),
        };
    }

    let fallback = std::env::temp_dir().join(FILE_NAME);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&fallback)
        .expect("no writable log destination");
    Sink {
        file,
        path: fallback,
    }
}

/// Point the log at `directory`. Safe to call at any time: before the first line
/// it only records the choice, afterwards it reopens the file in place.
pub fn set_directory(directory: &Path) {
    let path = directory.join(FILE_NAME);

    if let Some(lock) = LOG.get() {
        if let Ok(mut sink) = lock.lock() {
            if sink.path != path {
                *sink = open(&path);
            }
            return;
        }
    }

    if let Ok(mut desired) = DESIRED.lock() {
        *desired = Some(path);
    }
}

/// Where the log is currently going.
pub fn path() -> PathBuf {
    let sink = sink();
    match sink.lock() {
        Ok(sink) => sink.path.clone(),
        Err(_) => crate::config::data_dir().join(FILE_NAME),
    }
}

/// Empty the current log file, keeping the handle open so logging continues.
pub fn clear() {
    let handle = sink();
    if let Ok(mut sink) = handle.lock() {
        let _ = sink.file.set_len(0);
        let _ = sink.file.seek(std::io::SeekFrom::Start(0));
        let _ = sink.file.flush();
    }
}

/// Append one timestamped line. Never panics, never blocks the caller for long.
pub fn line(message: &str) {
    println!("[mp14tools] {message}");
    crate::console::write(&format!("[mp14tools] {message}"));

    let handle = sink();
    if let Ok(mut sink) = handle.lock() {
        let _ = writeln!(sink.file, "{} {message}", timestamp());
        let _ = sink.file.flush();
    }
}

fn timestamp() -> String {
    // Seconds since the Unix epoch is enough for support purposes and avoids a
    // date/time dependency.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    format!("epoch={seconds}")
}
