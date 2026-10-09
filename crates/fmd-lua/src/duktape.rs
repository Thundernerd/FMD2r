//! `fmd.duktape` (baseunits/lua/LuaDuktape.pas): `ExecJS` on an embedded QuickJS standing in
//! for FMD2's Duktape (docs/plan.md, "JavaScript").

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fmd_http::TerminateToken;
use mlua::IntoLuaMulti;
use rquickjs::context::EvalOptions;
use rquickjs::{Context, Ctx, FromJs, Function, Value};

use crate::{LuaDir, app_data_or_default};

/// Bounds on one `ExecJS` call. FMD2's Duktape has none; a script that exceeds them fails like
/// any script error, so a runaway script cannot hang or crash a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsLimits {
    /// Wall-clock time the script may run.
    pub time: Duration,
    /// Bytes the JS heap may allocate.
    pub memory: usize,
}

impl Default for JsLimits {
    /// 30 seconds and 256 MiB: far beyond what module scripts (unpackers, crypto-js) need.
    fn default() -> Self {
        JsLimits {
            time: Duration::from_secs(30),
            memory: 256 * 1024 * 1024,
        }
    }
}

/// The `ExecJS` settings of a runtime, stored as app data on its Lua state.
#[derive(Clone, Default)]
pub(crate) struct JsSettings {
    pub(crate) limits: JsLimits,
    /// The worker's token: terminating it interrupts the running script.
    pub(crate) terminate: TerminateToken,
}

/// Why a script produced no result. The message is what Duktape's error coerces to.
struct JsError(String);

/// Registers `fmd.duktape`, like `LuaPackage.AddLib`
/// (baseunits/lua/LuaDuktape.pas:39).
pub(crate) fn register(lua: &mlua::Lua) -> mlua::Result<()> {
    crate::package::add_lib(lua, "duktape", open)
}

/// Opens the library table, like `luaopen_duktape` (baseunits/lua/LuaDuktape.pas:32-36).
fn open(lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
    let lib = lua.create_table()?;
    lib.set("ExecJS", lua.create_function(exec_js)?)?;
    Ok(lib)
}

/// `ExecJS(code)` (baseunits/lua/LuaDuktape.pas:14-24): the script's completion value as a
/// string, or no value at all when the script fails, after logging the error.
fn exec_js(lua: &mlua::Lua, code: mlua::Value) -> mlua::Result<mlua::MultiValue> {
    // luaToString (baseunits/lua/LuaUtils.pas:206-213): `lua_tolstring` read as a C string,
    // so non-strings other than numbers become '' and the code ends at the first NUL.
    let source = lua
        .coerce_string(code)?
        .map(|s| until_nul(&s.as_bytes()).to_vec())
        .unwrap_or_default();
    let lua_dir = app_data_or_default::<LuaDir>(lua).0;
    let settings = app_data_or_default::<JsSettings>(lua);
    match eval(&source, &lua_dir, settings) {
        Ok(result) => {
            // baseunits/Duktape.pas:98-99: a result of "undefined" is returned as ''. Then
            // `lua_pushstring` of the Pascal string ends it at the first NUL.
            let result = if result == b"undefined" {
                &[][..]
            } else {
                until_nul(&result)
            };
            lua.create_string(result)?.into_lua_multi(lua)
        }
        Err(JsError(message)) => {
            // baseunits/lua/LuaDuktape.pas:20-22 around the exception from
            // baseunits/Duktape.pas:95.
            tracing::error!("Duktape.ExecJS() Duktape error: {message}");
            Ok(mlua::MultiValue::new())
        }
    }
}

/// `ExecJS` (baseunits/Duktape.pas:77-104): evaluates `source` as global code in a fresh heap
/// with FMD2's globals, and coerces the completion value with `duk_safe_to_string`.
fn eval(source: &[u8], lua_dir: &Path, settings: JsSettings) -> Result<Vec<u8>, JsError> {
    let runtime = rquickjs::Runtime::new().map_err(setup_error)?;
    runtime.set_memory_limit(settings.limits.memory);
    let deadline = Instant::now() + settings.limits.time;
    let terminate = settings.terminate;
    // QuickJS polls this while running; `true` throws an uncatchable "interrupted" error.
    runtime.set_interrupt_handler(Some(Box::new(move || {
        terminate.is_terminated() || Instant::now() >= deadline
    })));
    let context = Context::full(&runtime).map_err(setup_error)?;
    context.with(|ctx| {
        install_globals(&ctx, lua_dir.to_path_buf()).map_err(setup_error)?;
        // `duk_peval` compiles non-strict global code; rquickjs defaults to strict.
        let mut options = EvalOptions::default();
        options.strict = false;
        let completion = ctx.eval_with_options::<Value, _>(source.to_vec(), options);
        match completion {
            Ok(value) => Ok(cesu8(safe_to_string(&ctx, value))),
            Err(_) => Err(JsError(utf8_lossy(safe_to_string(&ctx, ctx.catch())))),
        }
    })
}

/// Gives the heap Duktape 2.3's built-ins where QuickJS's differ (`builtins.js`), adds `print`
/// and `require` (baseunits/Duktape.pas:88-90, `prelude.js`), then removes the built-in members
/// Duktape lacks (`surface.js`).
fn install_globals<'js>(ctx: &Ctx<'js>, lua_dir: PathBuf) -> rquickjs::Result<()> {
    let builtins: Function = ctx.eval(include_str!("duktape/builtins.js"))?;
    let native: Function = builtins.call(())?;
    let prelude: Function = ctx.eval(include_str!("duktape/prelude.js"))?;
    let mod_search = Function::new(ctx.clone(), move |id: String| mod_search(&lua_dir, &id))?;
    // baseunits/Duktape.pas:28.
    let log = Function::new(ctx.clone(), |text: String| tracing::info!("{text}"))?;
    prelude.call::<_, ()>((mod_search, log, native))?;
    let surface: Function = ctx.eval(include_str!("duktape/surface.js"))?;
    surface.call::<_, ()>(())
}

/// `Duktape.modSearch` (baseunits/Duktape.pas:39-67): the source of module `id`, read by
/// `loadModuleFile` (baseunits/Duktape.pas:106-121) from `<lua dir>/<id>`, else
/// `<lua dir>/<id>.js`. With neither file it returns `undefined`, so `require` hands back the
/// module's empty `exports` instead of throwing.
///
/// Module sources are decoded as UTF-8, replacing invalid bytes. Files are read on every
/// `require` rather than cached for the process (`TFileCache`, baseunits/Duktape.pas:150), so
/// an updated `lua/` tree takes effect without a restart.
fn mod_search(lua_dir: &Path, id: &str) -> Option<String> {
    let mut path = lua_dir.join(id);
    if !path.is_file() {
        path.as_mut_os_string().push(".js");
        if !path.is_file() {
            return None;
        }
    }
    match std::fs::read(&path) {
        Ok(bytes) => Some(utf8_lossy(bytes)),
        Err(error) => {
            // baseunits/Duktape.pas:63-65.
            tracing::error!("modSearch Error: {}: {error}", path.display());
            None
        }
    }
}

/// `duk_safe_to_string` (baseunits/Duktape.Api.pas:1636; Duktape 2.3's `duk_safe_to_lstring`):
/// `ToString(value)`; when that throws, `ToString` of the error; when
/// that throws too, `"Error"`.
fn safe_to_string<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Vec<u8> {
    to_string(ctx, value)
        .or_else(|| to_string(ctx, ctx.catch()))
        .unwrap_or_else(|| b"Error".to_vec())
}

/// `ToString(value)` as raw bytes, or `None` when the coercion throws.
fn to_string<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Option<Vec<u8>> {
    let string = rquickjs::convert::Coerced::<rquickjs::String>::from_js(ctx, value).ok()?;
    let mut len = 0;
    // SAFETY: `JS_ToCStringLen` returns a NUL-terminated buffer of `len` bytes owned by the
    // context (or null on failure), which is copied out and freed before returning. The safe
    // `String::to_string` is not used because it rejects the lone surrogates QuickJS encodes as
    // 3-byte sequences, which Duktape passes through the same way.
    unsafe {
        let ptr =
            rquickjs::qjs::JS_ToCStringLen(ctx.as_raw().as_ptr(), &mut len, string.0.as_raw());
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), len).to_vec();
        rquickjs::qjs::JS_FreeCString(ctx.as_raw().as_ptr(), ptr);
        Some(bytes)
    }
}

/// `utf8` with every 4-byte sequence (a character outside the BMP, which QuickJS encodes from a
/// surrogate pair) re-encoded as its two surrogates, 3 bytes each. That is CESU-8, how Duktape
/// 2.3 stores and returns such characters: its strings hold UTF-16 code units in its extended
/// UTF-8, so `duk_safe_to_string` (baseunits/Duktape.pas:94) never joins a pair.
fn cesu8(utf8: Vec<u8>) -> Vec<u8> {
    if !utf8.iter().any(|&b| b >= 0xf0) {
        return utf8;
    }
    let mut out = Vec::with_capacity(utf8.len() + utf8.len() / 2);
    let mut i = 0;
    while i < utf8.len() {
        let b = utf8[i];
        let c = (i + 4 <= utf8.len()).then(|| {
            (u32::from(b & 0x07) << 18)
                | (u32::from(utf8[i + 1] & 0x3f) << 12)
                | (u32::from(utf8[i + 2] & 0x3f) << 6)
                | u32::from(utf8[i + 3] & 0x3f)
        });
        if let Some(c) = c.filter(|c| b >= 0xf0 && *c >= 0x10000) {
            let c = c - 0x10000;
            for unit in [0xd800 | (c >> 10), 0xdc00 | (c & 0x3ff)] {
                // A surrogate is U+D800..U+DFFF: 1110_1101 10xx_xxxx 10xx_xxxx.
                out.extend([
                    0xe0 | (unit >> 12) as u8,
                    0x80 | ((unit >> 6) & 0x3f) as u8,
                    0x80 | (unit & 0x3f) as u8,
                ]);
            }
            i += 4;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// The bytes before the first NUL.
fn until_nul(bytes: &[u8]) -> &[u8] {
    bytes.split(|&b| b == 0).next().unwrap_or_default()
}

/// `bytes` decoded as UTF-8, with invalid sequences replaced by U+FFFD.
fn utf8_lossy(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

/// QuickJS could not create the heap or install FMD2's globals, like `duk_create_heap_default`
/// returning nil (baseunits/Duktape.pas:86-87).
fn setup_error(error: rquickjs::Error) -> JsError {
    JsError(format!("Failed to set up a QuickJS heap: {error}"))
}
