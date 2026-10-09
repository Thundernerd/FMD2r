//! Compiles the vendored Duktape 2.3.0 with its 1.x-style module loader and the `ExecJS` shim.
//! The pinned version is in vendor/duktape/VERSION.
//!
//! duktape.c is compiled through src/duktape_fmd2.c, which gives it the date handling of FMD2's
//! Windows build.

fn main() {
    let vendor = "vendor/duktape";
    println!("cargo:rerun-if-changed={vendor}");
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=src/duktape_fmd2.c");
    cc::Build::new()
        .file("src/duktape_fmd2.c")
        .file(format!("{vendor}/duk_module_duktape.c"))
        .file("src/shim.c")
        .include(vendor)
        .warnings(false)
        .compile("duktape_ref");
}
