//! Compiles the vendored lua-protobuf C module (`pb`) against the Lua 5.4 headers that
//! mlua-sys builds, so `require 'pb'` works as with the `pb.dll` FMD2 ships
//! (dist/x86_64-win64/pb.dll). The pinned version is in vendor/lua-protobuf/VERSION.
//!
//! Also resolves `fmd.env.Revision` as `FMD_ENV_REVISION`: `FMD2R_REVISION` when set at build
//! time, else [`REFERENCE_REVISION`].

/// The FMD2 revision whose Host API FMD2r reproduces: `git rev-list --count --first-parent HEAD`
/// of the reference checkout (ad3a5b63), as FMD2's builds compute `REVISION_NUMBER`
/// (git2revision.bat).
const REFERENCE_REVISION: &str = "6930";

fn main() {
    revision();
    let src = "vendor/lua-protobuf";
    println!("cargo:rerun-if-changed={src}");
    // mlua-sys `links = "lua"` and prints the vendored include directory as `include`.
    let Ok(lua_include) = std::env::var("DEP_LUA_INCLUDE") else {
        println!("cargo::error=DEP_LUA_INCLUDE is unset: mlua-sys must build vendored Lua");
        return;
    };
    cc::Build::new()
        .file(format!("{src}/pb.c"))
        .include(src)
        .include(lua_include)
        .warnings(false)
        .compile("luapb");
}

/// Emits `FMD_ENV_REVISION`, failing the build when the override is not a decimal number, since
/// modules compare it with numbers (lua/templates/MangaHub.lua:122).
fn revision() {
    println!("cargo:rerun-if-env-changed=FMD2R_REVISION");
    let revision = std::env::var("FMD2R_REVISION")
        .ok()
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| REFERENCE_REVISION.to_owned());
    if !revision.bytes().all(|b| b.is_ascii_digit()) {
        println!("cargo::error=FMD2R_REVISION must be a decimal number, got {revision:?}");
        return;
    }
    println!("cargo:rustc-env=FMD_ENV_REVISION={revision}");
}
