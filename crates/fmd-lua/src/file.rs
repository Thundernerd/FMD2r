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
