//! Compiles the vendored Duktape 2.3.0 with its 1.x-style module loader and the `ExecJS` shim.
//! The pinned version is in vendor/duktape/VERSION.

fn main() {
    let vendor = "vendor/duktape";
    println!("cargo:rerun-if-changed={vendor}");
    println!("cargo:rerun-if-changed=src/shim.c");
    cc::Build::new()
        .file(format!("{vendor}/duktape.c"))
        .file(format!("{vendor}/duk_module_duktape.c"))
        .file("src/shim.c")
        .include(vendor)
        .warnings(false)
        .compile("duktape_ref");
}
