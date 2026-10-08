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

use mlua::{AnyUserData, Function, Lua, Table, Value};

use crate::LuaMemoryStream;

/// Opens one library, returning its table, like the `luaopen_*` functions FMD2 registers.
type Opener = fn(&Lua) -> mlua::Result<Table>;

/// Every host library, by its name after the `fmd.` prefix modules `require` it under.
const LIBS: &[(&str, Opener)] = &[
    ("fileutil", fileutil::open),
    ("gzip", gzip::open),
    ("imagepuzzle", imagepuzzle::open),
    ("logger", logger::open),
    ("mangafoxwatermark", mangafoxwatermark::open),
    ("pcre2", pcre2::open),
    ("subprocess", subprocess::open),
];

/// Makes every host library loadable with `require`, through FMD2's searcher
/// (baseunits/lua/LuaPackage.pas:71-79), and `pb` through `package.preload`.
pub(crate) fn register(lua: &Lua) -> mlua::Result<()> {
    for &(name, open) in LIBS {
        crate::package::add_lib(lua, name, open)?;
    }
    let preload: Table = lua.globals().get::<Table>("package")?.get("preload")?;
    pb::register(lua, &preload)
}

/// The quality FMD2's JPEG writers save with: Lazarus' `TJPEGImage` and FPC's `TFPWriterJPEG`
/// both default to 75 (lcl/include/jpegimage.inc:23 in Lazarus, fcl-image fpwritejpeg.pas:216
/// in FPC 3.2.2).
const JPEG_QUALITY: u8 = 75;

/// Builds a `LuaClass` object inside a Lua callback, where errors are Lua errors.
fn build_object<T: 'static>(lua: &Lua, class: crate::LuaClass<T>) -> mlua::Result<AnyUserData> {
    class.build(lua).map_err(mlua::Error::from)
}

/// Registers `create` as both `New` and `Create`, the constructor pair FMD2's object libraries
/// share (e.g. baseunits/lua/LuaImagePuzzle.pas:73-76).
fn constructors<F>(lua: &Lua, create: F) -> mlua::Result<Vec<(&'static str, Function)>>
where
    F: Fn(&Lua, mlua::Variadic<Value>) -> mlua::Result<Value> + Clone + 'static,
{
    Ok(vec![
        ("New", lua.create_function(create.clone())?),
        ("Create", lua.create_function(create)?),
    ])
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

/// The MemoryStream behind a Lua argument, like FMD2 casting `luaToUserData` to a `TStream`
/// (baseunits/lua/LuaUtils.pas:201); anything else is `None`.
fn memory_stream(value: &Value) -> Option<LuaMemoryStream> {
    match value {
        Value::UserData(object) => LuaMemoryStream::from_lua(object),
        _ => None,
    }
}
