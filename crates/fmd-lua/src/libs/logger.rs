//! `fmd.logger` (baseunits/lua/LuaLogger.pas:15-46): FMD2's MultiLog `Logger` becomes tracing
//! events with target `fmd.logger`.

use mlua::{Lua, Table, Value};

use super::{lib_table, to_string_arg};

/// The running website module's name, read from the `MODULE` global when one is set, so a log
/// line says which module wrote it.
fn module_name(lua: &Lua) -> String {
    let name = || -> mlua::Result<Option<String>> {
        match lua.globals().get::<Value>("MODULE")? {
            Value::Nil => Ok(None),
            module => {
                let name: Value = match module {
                    Value::Table(t) => t.get("Name")?,
                    Value::UserData(u) => mlua::ObjectLike::get(&u, "Name")?,
                    _ => Value::Nil,
                };
                Ok(lua.coerce_string(name)?.map(|s| s.to_string_lossy()))
            }
        }
    };
    // A MODULE without a readable name must not make logging fail.
    name().ok().flatten().unwrap_or_default()
}

/// A logging function that converts its argument like `luaToString` and passes it to `log`.
fn sender(lua: &Lua, log: fn(&str, &str)) -> mlua::Result<mlua::Function> {
    lua.create_function(move |lua, message: Value| {
        let message = to_string_arg(lua, message)?;
        log(&module_name(lua), &String::from_utf8_lossy(&message));
        Ok(())
    })
}

/// Opens the library (baseunits/lua/LuaLogger.pas:33-44).
pub(super) fn open(lua: &Lua) -> mlua::Result<Table> {
    lib_table(
        lua,
        vec![
            (
                // baseunits/lua/LuaLogger.pas:15-19
                "Send",
                sender(
                    lua,
                    |module, message| tracing::info!(target: "fmd.logger", module, "{message}"),
                )?,
            ),
            (
                // baseunits/lua/LuaLogger.pas:21-25
                "SendWarning",
                sender(
                    lua,
                    |module, message| tracing::warn!(target: "fmd.logger", module, "{message}"),
                )?,
            ),
            (
                // baseunits/lua/LuaLogger.pas:27-31
                "SendError",
                sender(
                    lua,
                    |module, message| tracing::error!(target: "fmd.logger", module, "{message}"),
                )?,
            ),
        ],
    )
}
