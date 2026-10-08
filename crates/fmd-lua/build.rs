//! Compiles the vendored lua-protobuf C module (`pb`) against the Lua 5.4 headers that
//! mlua-sys builds, so `require 'pb'` works as with the `pb.dll` FMD2 ships
//! (dist/x86_64-win64/pb.dll). The pinned version is in vendor/lua-protobuf/VERSION.

fn main() {
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
