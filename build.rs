//! Build script: embeds the application icon and version information.
//!
//! A Windows executable carries its icon as a *resource*, so unlike the tray
//! icon and the window icon (which are drawn at runtime, see `src/icon.rs`) this
//! one has to be compiled into the binary by a resource compiler.
//!
//! The project builds with the GNU toolchain, where `windres` comes from the
//! toolchain the build already depends on (`build\toolchain\mingw64`). When it
//! cannot be found the build still succeeds - it just produces an executable with
//! the default icon - because a missing icon is not worth failing a build over.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/mp14tools.rc");
    println!("cargo:rerun-if-changed=assets/mp14tools.ico");

    let manifest = Path::new("assets/mp14tools.rc");
    if !manifest.exists() {
        return;
    }

    let Some(windres) = find_windres() else {
        println!("cargo:warning=windres not found; the executable keeps the default icon");
        return;
    };

    let out_dir = std::env::var_os("OUT_DIR").map(PathBuf::from);
    let Some(out_dir) = out_dir else {
        return;
    };
    let object = out_dir.join("mp14tools.res");

    let result = Command::new(&windres)
        .arg("--input-format=rc")
        // COFF, not the default: the GNU linker eats it as an ordinary object.
        .arg("--output-format=coff")
        .arg(format!("--include-dir={}", assets_dir().display()))
        .arg("-i")
        .arg(manifest)
        .arg("-o")
        .arg(&object)
        .output();

    match result {
        Ok(output) if output.status.success() => {
            println!("cargo:rustc-link-arg={}", object.display());
        }
        Ok(output) => {
            let message = String::from_utf8_lossy(&output.stderr);
            println!(
                "cargo:warning=windres failed; the executable keeps the default icon ({})",
                message.trim().lines().next().unwrap_or("no output")
            );
        }
        Err(error) => {
            println!("cargo:warning=could not run windres ({error}); the icon is not embedded");
        }
    }
}

fn assets_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("assets")
}

/// `windres.exe` from the project-local toolchain, else from `PATH`.
fn find_windres() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    // The project-local MinGW, which `tools\install-mingw.ps1` installs.
    let root = std::env::current_dir().ok()?;
    candidates.push(root.join("build/toolchain/mingw64/bin/windres.exe"));
    candidates.push(root.join("build/toolchain/mingw64/x86_64-w64-mingw32/bin/windres.exe"));

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            candidates.push(directory.join("windres.exe"));
        }
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}
