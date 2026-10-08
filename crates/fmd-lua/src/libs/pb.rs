//! The `pb` C module (lua-protobuf, vendor/lua-protobuf), which FMD2 ships as `pb.dll`
//! (dist/x86_64-win64/pb.dll) for `lua/utils/protoc.lua:993` and `lua/modules/MangaPlus.lua`.

use mlua::{Lua, Table, ffi};

unsafe extern "C-unwind" {
    fn luaopen_pb(state: *mut ffi::lua_State) -> std::os::raw::c_int;
    fn luaopen_pb_io(state: *mut ffi::lua_State) -> std::os::raw::c_int;
    fn luaopen_pb_conv(state: *mut ffi::lua_State) -> std::os::raw::c_int;
    fn luaopen_pb_buffer(state: *mut ffi::lua_State) -> std::os::raw::c_int;
    fn luaopen_pb_slice(state: *mut ffi::lua_State) -> std::os::raw::c_int;
    fn luaopen_pb_unsafe(state: *mut ffi::lua_State) -> std::os::raw::c_int;
}

/// Registers `pb` and its submodules in `package.preload`. Lua finds them in `pb.dll` through
/// its C searchers on FMD2; here the module is linked in.
pub(super) fn register(lua: &Lua, preload: &Table) -> mlua::Result<()> {
    let modules: [(&str, ffi::lua_CFunction); 6] = [
        ("pb", luaopen_pb),
        ("pb.io", luaopen_pb_io),
        ("pb.conv", luaopen_pb_conv),
        ("pb.buffer", luaopen_pb_buffer),
        ("pb.slice", luaopen_pb_slice),
        ("pb.unsafe", luaopen_pb_unsafe),
    ];
    for (name, open) in modules {
        // SAFETY: each is a lua_CFunction compiled from pb.c against the same Lua headers
        // and linked with the same Lua library as this state.
        let open = unsafe { lua.create_c_function(open)? };
        preload.set(name, open)?;
    }
    Ok(())
}
