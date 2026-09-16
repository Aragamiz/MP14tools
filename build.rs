//! Build script: embeds the application icon and version information.
//!
//! A Windows executable carries its icon as a *resource*, so unlike the tray
//! icon and the window icon (which are drawn at runtime, see `src/icon.rs`) this
//! one has to be compiled into the binary by a resource compiler. The version
//! block of `assets/mp14tools.rc` is filled in from the package version, so it
//! cannot drift away from `Cargo.toml`.
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
    // A version bump has to reach the file properties, so the resource is
    // recompiled whenever the package version changes.
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

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

    // The version block is generated instead of written by hand: a resource
    // script cannot be handed a quoted string from the command line, and the
    // copy that used to live in the .rc is exactly what drifted out of step with
    // `Cargo.toml`.
    let version_rc = out_dir.join("mp14tools_version.rc");
    if let Err(error) = std::fs::write(&version_rc, version_block()) {
        println!("cargo:warning=could not write the version block ({error})");
        return;
    }

    let result = Command::new(&windres)
        .arg("--input-format=rc")
        // COFF, not the default: the GNU linker eats it as an ordinary object.
        .arg("--output-format=coff")
        .arg(format!("--include-dir={}", assets_dir().display()))
        .arg(format!("--include-dir={}", out_dir.display()))
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

/// The package version in the two shapes a version resource needs:
/// `0,2,8,0` for `FILEVERSION` and `"0.2.8.0\0"` for the string block.
fn version_defines() -> (String, String) {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());

    // Punctuation and any pre-release suffix are dropped: the resource takes
    // numbers only.
    let mut parts: Vec<String> = version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    while parts.len() < 4 {
        parts.push("0".to_string());
    }
    parts.truncate(4);

    (parts.join(","), format!("\"{}\\0\"", parts.join(".")))
}

/// The `VS_VERSION_INFO` block for the package version, as a resource script
/// fragment that `assets/mp14tools.rc` includes.
fn version_block() -> String {
    let (comma, dotted) = version_defines();

    // Raw string on purpose: the `\0` terminators belong to the resource text,
    // not to Rust.
    const TEMPLATE: &str = r#"VS_VERSION_INFO VERSIONINFO
FILEVERSION     @COMMA@
PRODUCTVERSION  @COMMA@
FILEFLAGSMASK   VS_FFI_FILEFLAGSMASK
FILEFLAGS       0x0L
FILEOS          VOS_NT_WINDOWS32
FILETYPE        VFT_APP
FILESUBTYPE     VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "FileDescription", "MP14Tools\0"
            VALUE "FileVersion",     @DOTTED@
            VALUE "InternalName",    "mp14tools\0"
            VALUE "OriginalFilename","mp14tools.exe\0"
            VALUE "ProductName",     "MP14Tools\0"
            VALUE "ProductVersion",  @DOTTED@
            VALUE "Comments",        "Touchpad deep-press and OEM key remapper\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#;

    TEMPLATE
        .replace("@COMMA@", &comma)
        .replace("@DOTTED@", &dotted)
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
