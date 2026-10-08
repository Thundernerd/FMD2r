//! The global helper functions every module may call without a `require`
//! (baseunits/lua/LuaBase.pas:73-92).

use std::time::{Duration, Instant};

use fmd_http::TerminateToken;
use mlua::{Lua, LuaString, Table, Value, Variadic};

/// What the global helpers know about the state they are installed in.
#[derive(Clone, Default)]
pub struct Globals {
    /// The website module the state runs, recorded with every `print`.
    pub module: Option<String>,
    /// The worker's cancellation: terminating it cuts a running `sleep` short. When `None`,
    /// the runtime's token (`Runtime::set_terminate_token`) does.
    pub terminate: Option<TerminateToken>,
}

/// How often a `sleep` checks whether its worker was terminated.
const SLEEP_POLL: Duration = Duration::from_millis(10);

/// Installs the globals into `lua`.
pub(crate) fn install(lua: &Lua, globals: Globals) -> mlua::Result<()> {
    let g = lua.globals();
    let terminate = globals.terminate;
    g.set(
        "sleep",
        lua.create_function(move |lua, ms: Value| {
            // `lua_tointeger` (baseunits/lua/LuaBase.pas:70) yields 0 for anything that is not
            // an integer or a string holding one. FPC's `Sleep` takes a Cardinal, so a negative
            // value would wrap to weeks; it is 0 here instead.
            let ms = lua.coerce_integer(ms).ok().flatten().unwrap_or(0);
            // Without a token of its own, `sleep` stops with the runtime's job
            // (`Runtime::set_terminate_token`).
            let job = || {
                lua.app_data_ref::<crate::duktape::JsSettings>()
                    .map(|s| s.terminate.clone())
            };
            sleep(
                Duration::from_millis(u64::try_from(ms).unwrap_or(0)),
                terminate.clone().or_else(job).as_ref(),
            );
            Ok(())
        })?,
    )?;
    let module = globals.module;
    g.set(
        "print",
        lua.create_function(move |lua, args: Variadic<Value>| {
            for arg in args {
                print(lua, module.as_deref(), arg);
            }
            Ok(())
        })?,
    )?;
    g.set("Trim", lua.create_function(trim)?)?;
    // baseunits/lua/LuaBaseUnit.pas:26-30.
    string_fn(lua, &g, "MaybeFillHost", |[host, url]| {
        maybe_fill_host(host, url)
    })?;
    g.set(
        "MangaInfoStatusIfPos",
        lua.create_function(manga_info_status_if_pos)?,
    )?;
    // baseunits/lua/LuaSynaUtil.pas:17-21.
    string_fn(lua, &g, "GetBetween", |[pair_begin, pair_end, value]| {
        get_between(pair_begin, pair_end, value)
    })?;
    // baseunits/lua/LuaSynaUtil.pas:23-27.
    string_fn(lua, &g, "SeparateLeft", |[value, delimiter]| {
        separate_left(value, delimiter).to_vec()
    })?;
    // baseunits/lua/LuaSynaUtil.pas:29-33.
    string_fn(lua, &g, "SeparateRight", |[value, delimiter]| {
        separate_right(value, delimiter).to_vec()
    })?;
    Ok(())
}

/// Blocks the calling thread for `duration`, like `luabase_sleep` (baseunits/lua/LuaBase.pas:67-71),
/// but returns early once `terminate` is terminated.
fn sleep(duration: Duration, terminate: Option<&TerminateToken>) {
    let Some(terminate) = terminate else {
        std::thread::sleep(duration);
        return;
    };
    let deadline = Instant::now() + duration;
    while !terminate.is_terminated() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(SLEEP_POLL));
    }
}

/// Logs one `print` argument as its own line, like `luabase_print`'s `SendLog` per argument
/// (baseunits/lua/LuaBase.pas:53-65): booleans as `true`/`false`, anything else through
/// `luaToString`. The line carries the module and the worker thread's name when known.
fn print(lua: &Lua, module: Option<&str>, arg: Value) {
    let text = match arg {
        Value::Boolean(b) => b.to_string(),
        arg => String::from_utf8_lossy(&to_pascal_string(lua, arg)).into_owned(),
    };
    let thread = std::thread::current();
    let thread = thread.name().unwrap_or("");
    match module {
        Some(module) => tracing::info!(target: "fmd_lua::print", module, thread, "{text}"),
        None => tracing::info!(target: "fmd_lua::print", thread, "{text}"),
    }
}

/// Sets global `name` to a function of the first `N` arguments, each read with `luaToString`,
/// that returns one string, like FMD2's `lua_pushstring(L, F(luaToString(L, 1), ...))`
/// wrappers.
fn string_fn<const N: usize>(
    lua: &Lua,
    globals: &Table,
    name: &str,
    f: impl Fn([&[u8]; N]) -> Vec<u8> + 'static,
) -> mlua::Result<()> {
    let function = lua.create_function(move |lua, args: Variadic<Value>| {
        let args: [Vec<u8>; N] = std::array::from_fn(|i| {
            args.get(i)
                .map_or_else(Vec::new, |v| to_pascal_string(lua, v.clone()))
        });
        lua.create_string(f(args.each_ref().map(Vec::as_slice)))
    })?;
    globals.set(name, function)
}

/// FPC `Pos(needle, haystack)` as a 0-based index: `None` when absent or when `needle` is empty.
fn pos(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Synapse `SeparateLeft` (baseunits/synapse/synautil.pas:1156-1165): the part before the first
/// `delimiter`, or the whole value without one.
fn separate_left<'a>(value: &'a [u8], delimiter: &[u8]) -> &'a [u8] {
    match pos(delimiter, value) {
        Some(x) => &value[..x],
        None => value,
    }
}

/// Synapse `SeparateRight` (baseunits/synapse/synautil.pas:1169-1177): the part after the first
/// `delimiter`, or the whole value without one.
fn separate_right<'a>(value: &'a [u8], delimiter: &[u8]) -> &'a [u8] {
    match pos(delimiter, value) {
        Some(x) => &value[x + delimiter.len()..],
        None => value,
    }
}

/// `MaybeFillHost` (baseunits/uBaseUnit.pas:943-950): when `SplitURL` finds a path but no
/// host in `url`, that path prefixed with `host` minus its trailing slashes (`RemoveURLDelim`,
/// baseunits/uBaseUnit.pas:2003-2006); otherwise `url` unchanged.
fn maybe_fill_host(host: &[u8], url: &[u8]) -> Vec<u8> {
    let (url_host, path) = fmd_http::split_url_bytes(url);
    if url_host.is_empty() && !path.is_empty() {
        let host_end = host.iter().rposition(|&b| b != b'/').map_or(0, |i| i + 1);
        [&host[..host_end], &path].concat()
    } else {
        url.to_vec()
    }
}

/// `lua_trim` (baseunits/lua/LuaBaseUnit.pas:17-24): FPC `Trim` strips bytes `<= ' '` at both
/// ends; with a second argument (even `nil`, which reads as `''`), `TStringHelper.Trim` strips
/// the characters it holds instead.
fn trim(lua: &Lua, args: Variadic<Value>) -> mlua::Result<LuaString> {
    let mut args = args.into_iter().map(|v| to_pascal_string(lua, v));
    let s = args.next().unwrap_or_default();
    let trimmed = match args.next() {
        Some(chars) => trim_by(&s, |b| chars.contains(&b)),
        None => trim_by(&s, |b| b <= b' '),
    };
    lua.create_string(trimmed)
}

/// The status codes `MangaInfoStatusIfPos` checks, in order (baseunits/uBaseUnit.pas:230-233),
/// each with the default string it searches for (baseunits/uBaseUnit.pas:626-628).
const STATUSES: [(&str, &str); 4] = [
    ("1", "ongoing"),
    ("0", "complete"),
    ("2", "hiatus"),
    ("3", "cancel"),
];

/// `lua_mangainfostatusifpos` (baseunits/lua/LuaBaseUnit.pas:32-50) over `MangaInfoStatusIfPos`
/// (baseunits/uBaseUnit.pas:2793-2850): only 1 to 5 arguments return a value. That is `''` for
/// an empty search, otherwise the code of the first status with an alternative (`|`-separated)
/// found in the search, all compared lowercased, or `RS_InfoStatus_Unknown`
/// (mangadownloader/forms/frmMain.pas:1010) when none matches. Missing status arguments take
/// their defaults; an explicit `nil` reads as `''`, which never matches.
fn manga_info_status_if_pos(lua: &Lua, args: Variadic<Value>) -> mlua::Result<Variadic<LuaString>> {
    if !(1..=5).contains(&args.len()) {
        return Ok(Variadic::new());
    }
    let mut args = args.into_iter().map(|v| to_pascal_string(lua, v));
    let search = args.next().unwrap_or_default().to_ascii_lowercase();
    if search.is_empty() {
        return Ok(Variadic::from_iter([lua.create_string("")?]));
    }
    // `searchMany`: an empty alternative never matches, since FPC's `Pos('')` is 0.
    let matches = |status: &[u8]| {
        status
            .to_ascii_lowercase()
            .split(|&b| b == b'|')
            .any(|alternative| pos(alternative, &search).is_some())
    };
    let mut code = "Unknown";
    for (status_code, default) in STATUSES {
        let status = args.next().unwrap_or_else(|| default.into());
        if matches(&status) {
            code = status_code;
            break;
        }
    }
    Ok(Variadic::from_iter([lua.create_string(code)?]))
}

/// Synapse `GetBetween` (baseunits/synapse/synautil.pas:1671-1723): the text after the first
/// `pair_begin` up to the `pair_end` that balances it, counting nested pairs; the whole value
/// when either pair is missing.
fn get_between(pair_begin: &[u8], pair_end: &[u8], value: &[u8]) -> Vec<u8> {
    if value == [pair_begin, pair_end].concat() {
        return Vec::new();
    }
    if value.len() < pair_begin.len() + pair_end.len() {
        return value.to_vec();
    }
    let s = separate_right(value, pair_begin);
    if s == value || pos(pair_end, s).is_none() {
        return value.to_vec();
    }
    let mut result = Vec::new();
    let mut depth = 1;
    // `for n := 1 to Length(s) - lenEnd + 1`, 0-based here.
    for n in 0..=s.len() - pair_end.len() {
        if s[n..].starts_with(pair_end) {
            depth -= 1;
            if depth <= 0 {
                break;
            }
        }
        if s[n..].starts_with(pair_begin) {
            depth += 1;
        }
        result.push(s[n]);
    }
    result
}

/// `luaToString` (baseunits/lua/LuaUtils.pas:206-213): `lua_tolstring` (so numbers become
/// strings and anything else that is not a string becomes `''`), truncated at the first NUL by
/// the `PAnsiChar` to `String` conversion.
fn to_pascal_string(lua: &Lua, value: Value) -> Vec<u8> {
    match lua.coerce_string(value) {
        Ok(Some(s)) => {
            let bytes = s.as_bytes();
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            bytes[..end].to_vec()
        }
        _ => Vec::new(),
    }
}

/// Strips the bytes matching `strip` from both ends of `s`.
fn trim_by(s: &[u8], strip: impl Fn(u8) -> bool) -> &[u8] {
    let start = s.iter().position(|&b| !strip(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|&b| !strip(b)).map_or(start, |i| i + 1);
    &s[start..end]
}
