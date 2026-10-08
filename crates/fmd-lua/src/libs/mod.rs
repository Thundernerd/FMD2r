//! The `fmd.*` host libraries modules `require`, registered the way FMD2's `LuaPackage.AddLib`
//! makes them available under the `fmd.` prefix (baseunits/lua/LuaPackage.pas:62-79).

mod fileutil;
mod gzip;
mod imagepuzzle;
mod logger;
mod mangafoxwatermark;
mod pb;
mod pcre2;
pub mod subprocess;

use mlua::{AnyUserData, Function, Lua, LuaString, ObjectLike, Table, Value};

/// Opens one library, returning its table, like the `luaopen_*` functions FMD2 registers.
type Opener = fn(&Lua) -> mlua::Result<Table>;

/// Every host library, by the name modules `require` it under.
const LIBS: &[(&str, Opener)] = &[
    ("fmd.fileutil", fileutil::open),
    ("fmd.gzip", gzip::open),
    ("fmd.imagepuzzle", imagepuzzle::open),
    ("fmd.logger", logger::open),
    ("fmd.mangafoxwatermark", mangafoxwatermark::open),
    ("fmd.pcre2", pcre2::open),
    ("fmd.subprocess", subprocess::open),
];

/// Makes every host library loadable with `require`.
///
/// FMD2 resolves `fmd.<lib>` in its own searcher (baseunits/lua/LuaPackage.pas:71-79); until
/// the module loader installs that searcher, `package.preload` serves the same names without
/// touching the filesystem.
pub(crate) fn register(lua: &Lua) -> mlua::Result<()> {
    let preload: Table = lua.globals().get::<Table>("package")?.get("preload")?;
    for &(name, open) in LIBS {
        let loader: Function = lua.create_function(move |lua, ()| open(lua))?;
        preload.set(name, loader)?;
    }
    pb::register(lua, &preload)
}

/// Builds a library table from its functions, like `luaNewLibTable`
/// (baseunits/lua/LuaUtils.pas:64).
fn lib_table(lua: &Lua, functions: Vec<(&str, Function)>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (name, function) in functions {
        table.raw_set(name, function)?;
    }
    Ok(table)
}

/// Converts an argument like `luaToString` (baseunits/lua/LuaUtils.pas:206-213): strings and
/// numbers convert, anything else becomes empty, and the result ends at the first NUL because
/// FMD2 copies it as a C string.
fn to_string_arg(lua: &Lua, value: Value) -> mlua::Result<Vec<u8>> {
    let Some(s) = lua.coerce_string(value)? else {
        return Ok(Vec::new());
    };
    let bytes = s.as_bytes();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Ok(bytes[..end].to_vec())
}

/// Reads the whole content of a stream object (T04's MemoryStream) through its Lua surface
/// (`ToString`), like the `LoadFromStream` copies FMD2 makes of a `TStream`.
fn read_stream(stream: &AnyUserData) -> mlua::Result<Vec<u8>> {
    let content: LuaString = stream.get::<Function>("ToString")?.call(())?;
    Ok(content.as_bytes().to_vec())
}

/// Replaces the content of a stream object through its Lua surface (`Clear`, `WriteString`),
/// like setting `Size := 0` and writing from the start.
fn write_stream(lua: &Lua, stream: &AnyUserData, data: &[u8]) -> mlua::Result<()> {
    stream.get::<Function>("Clear")?.call::<()>(())?;
    if !data.is_empty() {
        stream
            .get::<Function>("WriteString")?
            .call::<()>(lua.create_string(data)?)?;
    }
    Ok(())
}
