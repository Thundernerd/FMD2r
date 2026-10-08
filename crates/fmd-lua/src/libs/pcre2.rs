//! `fmd.pcre2` (baseunits/lua/LuaPCRE2.pas:92-252) as a stub: no upstream module uses it, so
//! each function exists under its FMD2 name but raises a "not implemented" error.

use mlua::{Lua, Table, Value, Variadic};

use super::lib_table;

/// The library's functions (baseunits/lua/LuaPCRE2.pas:237-245).
const NAMES: [&str; 5] = ["exec", "find", "match", "gmatch", "gsub"];

/// Opens the library (baseunits/lua/LuaPCRE2.pas:246-250).
pub(super) fn open(lua: &Lua) -> mlua::Result<Table> {
    let functions = NAMES
        .into_iter()
        .map(|name| {
            let f = lua.create_function(move |_, _: Variadic<Value>| -> mlua::Result<()> {
                Err(mlua::Error::runtime(format!(
                    "fmd.pcre2.{name} is not implemented in FMD2r"
                )))
            })?;
            Ok((name, f))
        })
        .collect::<mlua::Result<Vec<_>>>()?;
    lib_table(lua, functions)
}
