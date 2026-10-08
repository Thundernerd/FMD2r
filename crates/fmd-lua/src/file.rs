//! File access shared by the Host API objects that load and save files.

use std::path::{Path, PathBuf};

use mlua::{Lua, Value};

use crate::class::to_bytes;

/// A file name argument, converted like `luaToString` (baseunits/lua/LuaUtils.pas:206).
pub(crate) fn file_path(lua: &Lua, value: Value) -> mlua::Result<PathBuf> {
    let path = to_bytes(lua, value)?;
    Ok(PathBuf::from(String::from_utf8_lossy(&path).into_owned()))
}

/// Reads a whole file, failing with FPC's `EFOpenError` message (`SFOpenErrorEx`).
pub(crate) fn read_file(path: &Path) -> mlua::Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| {
        mlua::Error::runtime(format!("Unable to open file \"{}\": {e}", path.display()))
    })
}

/// Creates or replaces a file, failing with FPC's `EFCreateError` message (`SFCreateErrorEx`).
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> mlua::Result<()> {
    std::fs::write(path, bytes).map_err(|e| {
        mlua::Error::runtime(format!("Unable to create file \"{}\": {e}", path.display()))
    })
}

/// Compiles a Lua file like `luaL_loadfile`: read with [`read_lua_file`] and named `@<path>`.
pub(crate) fn load_lua_file(lua: &Lua, path: &Path) -> mlua::Result<mlua::Function> {
    let source = read_lua_file(path).map_err(mlua::Error::external)?;
    lua.load(source)
        .set_name(format!("@{}", path.display()))
        .into_function()
}

/// Reads a Lua file as `luaL_loadfile` hands it to the parser (lauxlib.c `skipcomment` in Lua
/// 5.4): a leading UTF-8 BOM is dropped, and a first line starting with `#` becomes an empty
/// line, so line numbers stay right.
fn read_lua_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    let source = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    if source.first() != Some(&b'#') {
        return Ok(source.to_vec());
    }
    Ok(match source.iter().position(|&b| b == b'\n') {
        Some(end) => source[end..].to_vec(),
        None => Vec::new(),
    })
}
