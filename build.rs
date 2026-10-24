//! Build script: embeds the Windows application manifest
//! (`res/logirev.manifest`) into the executables when building with the
//! Microsoft toolchain. Other targets need nothing.

use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=res/logirev.manifest");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && target_env == "msvc" {
        let dir = env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
        let manifest = Path::new(&dir).join("res").join("logirev.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    }
}
