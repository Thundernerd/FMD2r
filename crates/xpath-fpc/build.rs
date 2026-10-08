//! Builds `libfmdxpath.so` with `build.sh` into `OUT_DIR` and points the linker at it.

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    println!("cargo:rerun-if-changed=build.sh");
    println!("cargo:rerun-if-changed=pascal");
    println!("cargo:rerun-if-changed=fmdxpath.h");

    let status = Command::new("sh")
        .arg(manifest_dir.join("build.sh"))
        .arg(&out_dir)
        .status()?;
    if !status.success() {
        return Err(format!("build.sh failed with {status}").into());
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!(
        "cargo:rustc-link-arg-tests=-Wl,-rpath,{}",
        out_dir.display()
    );
    println!("cargo:lib_dir={}", out_dir.display());
    Ok(())
}
