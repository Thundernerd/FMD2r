//! Builds `libfmdxpath.so` with `build.sh` into `OUT_DIR` and points the linker at it, or, when
//! `FMDXPATH_LIB_DIR` is set, uses the `libfmdxpath.so` already in that directory.

use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    println!("cargo:rerun-if-changed=build.sh");
    println!("cargo:rerun-if-changed=pascal");
    println!("cargo:rerun-if-changed=fmdxpath.h");
    println!("cargo:rerun-if-env-changed=FMDXPATH_LIB_DIR");

    if let Some(dir) = env::var_os("FMDXPATH_LIB_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.join("libfmdxpath.so").is_file() {
            return Err(
                format!("FMDXPATH_LIB_DIR has no libfmdxpath.so: {}", dir.display()).into(),
            );
        }
        println!(
            "cargo:rerun-if-changed={}",
            dir.join("libfmdxpath.so").display()
        );
        link(&dir);
        return Ok(());
    }

    let status = Command::new("sh")
        .arg(manifest_dir.join("build.sh"))
        .arg(&out_dir)
        .status()?;
    if !status.success() {
        return Err(format!("build.sh failed with {status}").into());
    }

    link(&out_dir);
    Ok(())
}

/// Points the linker, and this package's tests, at `dir`'s `libfmdxpath.so`.
fn link(dir: &Path) {
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-arg-tests=-Wl,-rpath,{}", dir.display());
    println!("cargo:lib_dir={}", dir.display());
    println!("cargo:rustc-env=FMDXPATH_LIB_DIR={}", dir.display());
}
