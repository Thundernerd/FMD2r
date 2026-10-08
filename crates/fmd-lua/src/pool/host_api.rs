//! Which Host API names a callback's Lua state lacks, for the unknown-Host-API report.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use fmd_http::HttpClient;

use super::callbacks;
use crate::{Globals, LuaHttp, Module, Runtime};

/// Looks a name up in a state set up like a callback's: `fmd.<lib>` must `require`,
/// `OBJECT.Member` must be a non-nil member of an injected object, and anything else a
/// non-nil global.
const LOOKUP: &str = r#"
local name = ...
if name:sub(1, 4) == 'fmd.' then return (pcall(require, name)) end
local object, member = name:match('^([^.]+)%.(.+)$')
if object then
  local ok, value = pcall(function() return _G[object][member] end)
  return ok and value ~= nil
end
return _G[name] ~= nil
"#;

/// The names among `names` (as [`scan_host_api_names`](crate::scan_host_api_names) lists them) that the state
/// a callback runs in does not provide: a state with every global and object any `Do*`
/// function sets (baseunits/lua/LuaWebsiteModules.pas:154-465), `MODULE` over a module
/// with account support, and the loader's `NewWebsiteModule`.
pub fn missing_host_api<'a>(
    names: impl IntoIterator<Item = &'a str>,
) -> crate::Result<BTreeSet<String>> {
    let runtime = Runtime::new()?;
    runtime.install_globals(Globals::default())?;
    let lua = runtime.lua();
    let module = Arc::new(Module::new(PathBuf::new()));
    // `Account` is a member only of a `MODULE` built while the module supports accounts.
    runtime.set_module(&module)?;
    runtime.exec("MODULE.AccountSupport = true")?;
    runtime.set_module(&module)?;
    callbacks::install_every_global(lua)?;
    // Only the state that runs `Init` has it (baseunits/lua/LuaWebsiteModules.pas:490); a
    // stand-in marks it as provided.
    lua.globals()
        .set("NewWebsiteModule", lua.create_function(|_, ()| Ok(()))?)?;
    let http = HttpClient::new().map_err(mlua::Error::external)?;
    lua.globals()
        .set("HTTP", LuaHttp::new(http.session()).build(lua)?)?;
    let lookup = lua.load(LOOKUP).set_name("=lookup").into_function()?;
    let mut missing = BTreeSet::new();
    for name in names {
        if !lookup.call::<bool>(name)? {
            missing.insert(name.to_owned());
        }
    }
    Ok(missing)
}
