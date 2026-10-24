//! Build script, Windows executables only:
//!
//! * embeds the application manifest (`res/logirev.manifest`): modern
//!   controls and sharp rendering on high-resolution screens;
//! * draws the application icon (`src/icon.rs`) into a `.ico` file and
//!   embeds it, so Explorer, the taskbar and shortcuts show it.
//!
//! The icon resource needs the Windows resource compiler, so it is only
//! built on a Windows machine (locally or in CI); cross-checking from Linux
//! simply skips it.

use std::env;
use std::path::Path;

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=res/logirev.manifest");
    println!("cargo:rerun-if-changed=src/icon.rs");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" {
        return;
    }
    let dir = env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    if target_env == "msvc" {
        let manifest = Path::new(&dir).join("res").join("logirev.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
    embed_icon();
}

/// Writes `logirev.ico` and a one-line resource script into OUT_DIR and
/// compiles them into the executables.
#[cfg(windows)]
fn embed_icon() {
    let out = env::var("OUT_DIR").expect("set by cargo");
    let ico = Path::new(&out).join("logirev.ico");
    std::fs::write(&ico, icon::ico(&[16, 20, 24, 32, 40, 48, 64, 256])).expect("write icon");
    let rc = Path::new(&out).join("logirev.rc");
    // Resource id 1: Windows shows the first icon of an executable.
    let path = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).expect("write resource script");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .expect("compile the icon resource");
}

#[cfg(not(windows))]
fn embed_icon() {
    // Cross-compiling from Linux/macOS: no resource compiler, no file icon.
}
