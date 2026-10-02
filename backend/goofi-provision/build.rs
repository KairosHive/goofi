//! The target this binary is built for, and the wheels and vendored sources a distribution build carries.

use std::path::PathBuf;

/// Names the directory holding the goofi wheels and `vendor.tar.xz` (the node build's crate
/// sources, `vendor/` at its root) a DISTRIBUTION build embeds; unset in development.
const DIST: &str = "GOOFI_DIST";

fn main() {
    println!("cargo:rustc-env=GOOFI_TARGET={}", std::env::var("TARGET").expect("cargo sets TARGET"));
    println!("cargo:rerun-if-env-changed={DIST}");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let mut wheels = String::new();
    let mut vendor = String::from("&[]");
    let dist = std::env::var_os(DIST).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = &dist {
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{DIST}={}: {e}", dir.display()))
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "whl"))
            .collect();
        found.sort();
        assert!(!found.is_empty(), "{DIST}={} holds no wheel", dir.display());
        for wheel in found {
            let name = wheel.file_name().unwrap().to_string_lossy().into_owned();
            wheels += &format!("    ({name:?}, include_bytes!({:?})),\n", wheel.display().to_string());
        }
        let archive = dir.join("vendor.tar.xz");
        assert!(archive.is_file(), "{DIST}={} holds no vendor.tar.xz", dir.display());
        vendor = format!("include_bytes!({:?})", archive.display().to_string());
    }
    std::fs::write(
        out.join("dist.rs"),
        format!(
            "/// Whether this is a distribution build: one that provisions its own runtime at start.\n\
             pub const DIST: bool = {};\n\
             /// The goofi wheels a distribution build installs, by file name.\n\
             pub static WHEELS: &[(&str, &[u8])] = &[\n{wheels}];\n\
             /// The node build's crate sources as `vendor.tar.xz`, empty in development.\n\
             pub static VENDOR: &[u8] = {vendor};\n",
            dist.is_some()
        ),
    )
    .expect("write dist.rs");
}
