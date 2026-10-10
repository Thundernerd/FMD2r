//! The anti-bot hook a module's `HTTP` object runs after a request: `WebsiteBypassRequest`
//! (baseunits/lua/LuaWebsiteBypass.pas:142-212) with upstream's `websitebypass/checkantibot.lua`
//! and `websitebypass/websitebypass.lua`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use fmd_http::{HttpError, HttpSession};
use mlua::{AnyUserData, Function, LightUserData, Lua, Table, Value};

use super::{HttpObject, LuaHttp, ModuleHttpSettings};
use crate::class::borrow;
use crate::{LuaDir, Module, PackageCache, app_data_or_default};

/// The module side of the hook: whose bypass guard, `Storage` and settings it uses.
///
/// FMD2 locates the module by the request's host for the settings and `Storage`
/// (baseunits/lua/LuaWebsiteBypass.pas:175-177) and uses the guard of the module that prepared
/// the session; here all three belong to the session's module.
pub(super) struct WebsiteBypass {
    pub(super) module: Arc<Module>,
    pub(super) settings: Arc<dyn ModuleHttpSettings>,
}

/// File name of the challenge check (baseunits/lua/LuaWebsiteBypass.pas:37).
const CHECKANTIBOT_FILE: &str = "checkantibot.lua";
/// File name of the bypass (baseunits/lua/LuaWebsiteBypass.pas:38).
const WEBSITEBYPASS_FILE: &str = "websitebypass.lua";

/// The scripts of one `websitebypass/` folder, loaded once like FMD2's `doInitialization`
/// (baseunits/lua/LuaWebsiteBypass.pas:45-75).
struct Scripts {
    /// The state `____CheckAntiBot` lives in: a plain Lua state of its own, like
    /// `checkantibot_state` (:53).
    check_lua: Lua,
    check: Function,
    /// The `HTTP` objects built in `check_lua`, by the address of their state. Weak values, so
    /// they live until the next collection; rebuilding per check would cost more than the check.
    objects: Table,
    /// Checks since the last full collection (`checkantibot_count`, :44).
    checks: Cell<u32>,
    websitebypass: PathBuf,
}

/// The scripts of one folder as loaded at one generation of the package cache.
struct Loaded {
    generation: u64,
    scripts: Option<Rc<Scripts>>,
}

thread_local! {
    /// The scripts of every `websitebypass/` folder used on this thread (`None` without them).
    /// FMD2 keeps one locked check state per process (:41-43, :95); `HTTP` objects are
    /// thread-bound here, so each thread keeps its own, reloaded when the package cache clears.
    static SCRIPTS: RefCell<HashMap<PathBuf, Loaded>> = RefCell::default();
}

/// The scripts in `dir`, loaded on first use and again after `cache` was cleared.
fn scripts(dir: &Path, cache: &PackageCache) -> Option<Rc<Scripts>> {
    let generation = cache.generation();
    SCRIPTS.with(|loaded| {
        let mut loaded = loaded.borrow_mut();
        match loaded.get(dir) {
            Some(l) if l.generation == generation => l.scripts.clone(),
            _ => {
                let scripts = load_scripts(dir).map(Rc::new);
                let entry = Loaded {
                    generation,
                    scripts: scripts.clone(),
                };
                loaded.insert(dir.to_path_buf(), entry);
                scripts
            }
        }
    })
}

/// Loads `checkantibot.lua` and keeps its `____CheckAntiBot`; `None` unless both scripts exist
/// and the check loads (baseunits/lua/LuaWebsiteBypass.pas:50-73, :151).
fn load_scripts(dir: &Path) -> Option<Scripts> {
    let websitebypass = dir.join(WEBSITEBYPASS_FILE);
    if !websitebypass.is_file() {
        return None;
    }
    let file = dir.join(CHECKANTIBOT_FILE);
    if !file.is_file() {
        return None;
    }
    let check_lua = Lua::new();
    let loaded = run_file(&check_lua, &file)
        .and_then(|()| check_lua.globals().get::<Value>("____CheckAntiBot"))
        .and_then(|check| Ok((check, weak_values(&check_lua)?)));
    match loaded {
        Ok((Value::Function(check), objects)) => Some(Scripts {
            check_lua,
            check,
            objects,
            checks: Cell::new(0),
            websitebypass,
        }),
        Ok(_) => None,
        Err(e) => {
            tracing::error!(target: "fmd.lua", "{}: {e}", file.display());
            None
        }
    }
}

impl Scripts {
    /// `CheckAntiBotActive` (baseunits/lua/LuaWebsiteBypass.pas:89-117): `____CheckAntiBot(HTTP)`
    /// over the request just made; an error is logged and counts as no challenge.
    fn check_anti_bot(&self, http: &Rc<RefCell<HttpObject>>) -> bool {
        // A full collection every 32 checks (:101-105), which also lets go of the cached
        // objects of finished sessions.
        if self.checks.get() > 31 {
            if let Err(e) = self.check_lua.gc_collect() {
                tracing::error!(target: "fmd.lua", "CheckAntiBot: {e}");
            }
            self.checks.set(0);
        }
        self.checks.set(self.checks.get() + 1);
        let answer = self
            .http_object(http)
            .and_then(|object| self.check.call::<Value>(object));
        match answer {
            Ok(answer) => truthy(&answer),
            Err(e) => {
                tracing::error!(target: "fmd.lua", "CheckAntiBot: {e}");
                false
            }
        }
    }

    /// The `HTTP` object over `http` in the check state, built once and then cached.
    fn http_object(&self, http: &Rc<RefCell<HttpObject>>) -> mlua::Result<AnyUserData> {
        // While cached, the object holds `http`, so its address cannot be reused.
        let key = LightUserData(Rc::as_ptr(http) as *mut std::ffi::c_void);
        if let Some(object) = self.objects.raw_get::<Option<AnyUserData>>(key)? {
            return Ok(object);
        }
        let object = LuaHttp {
            object: http.clone(),
        }
        .build(&self.check_lua)?;
        self.objects.raw_set(key, &object)?;
        Ok(object)
    }
}

/// A table whose values the garbage collector may take (`__mode = 'v'`).
fn weak_values(lua: &Lua) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.raw_set("__mode", "v")?;
    table.set_metatable(Some(metatable))?;
    Ok(table)
}

/// Runs the Lua file `file` in `lua` (`LoadChunkExecute`, baseunits/lua/LuaHandler.pas:94).
fn run_file(lua: &Lua, file: &Path) -> mlua::Result<()> {
    let source = std::fs::read(file).map_err(mlua::Error::external)?;
    lua.load(source)
        .set_name(format!("@{}", file.display()))
        .exec()
}

/// `lua_toboolean`: everything but `nil` and `false` is true.
fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

/// Runs a request through `send` and, for a module's `HTTP` object, the anti-bot hook after it
/// (`WebsiteBypassRequest`, baseunits/lua/LuaWebsiteBypass.pas:142-212). Returns the request's
/// result: that of the retried request after a bypass, or the bypass's own answer when it
/// failed or asked for no reload.
pub(super) fn request(
    lua: &Lua,
    http: &Rc<RefCell<HttpObject>>,
    object: &AnyUserData,
    method: &str,
    url: &str,
    send: impl FnOnce(&mut HttpSession) -> Result<bool, HttpError>,
) -> mlua::Result<bool> {
    let bypass = borrow(http)?.bypass.clone();
    let scripts = bypass.as_ref().and_then(|_| {
        let LuaDir(lua_dir) = app_data_or_default(lua);
        let cache: PackageCache = app_data_or_default(lua);
        scripts(&lua_dir.join("websitebypass"), &cache)
    });
    let (Some(bypass), Some(scripts)) = (bypass, scripts) else {
        return borrow(http)?.request(send);
    };
    // A challenge answers 403, 429 or 503; the check needs that answer rather than retries
    // (:156-158).
    let mut result = {
        let mut object = borrow(http)?;
        object.session.set_allow_server_error_response(true);
        object.request(send)?
    };
    if !scripts.check_anti_bot(http) {
        return Ok(result);
    }
    let guard = bypass.module.website_bypass_guard();
    if guard.try_enter() {
        let answer = run_bypass(lua, http, object, &bypass, &scripts, method, url);
        guard.leave();
        result = answer?;
    } else {
        // Another thread is bypassing. FMD2 re-sends right away (:199-202); waiting first lets
        // the re-sent request carry what that bypass obtained.
        guard.enter();
        guard.leave();
        let mut object = borrow(http)?;
        if !object.session.terminated() {
            result = object.request(|s| s.request(method, url))?;
        }
    }
    Ok(result)
}

/// The guarded part of the hook (baseunits/lua/LuaWebsiteBypass.pas:163-197): runs the bypass
/// with `HTTP` set to this object; on success stores the cookies and user agent in the
/// module's settings and, when `MODULE.Storage['reload']` holds `true`, re-sends the request.
fn run_bypass(
    lua: &Lua,
    http: &Rc<RefCell<HttpObject>>,
    object: &AnyUserData,
    bypass: &WebsiteBypass,
    scripts: &Scripts,
    method: &str,
    url: &str,
) -> mlua::Result<bool> {
    // `L.LoadObject('HTTP', AHTTP, ...)` (:172).
    lua.globals().set("HTTP", object)?;
    if let Err(e) = bypass.settings.clear_cookies() {
        tracing::error!(target: "fmd.lua", "WebsiteBypass: {e}");
    }
    if !website_bypass_answer(lua, &scripts.websitebypass, method, url) {
        return Ok(false);
    }
    let (cookies, user_agent) = {
        let object = borrow(http)?;
        // `StringReplace(AHTTP.Cookies.Text, #13#10, ';', ...)` (:183): every line ends in `;`.
        let cookies: String = borrow(object.cookies.list())?
            .items()
            .iter()
            .map(|item| format!("{};", String::from_utf8_lossy(item)))
            .collect();
        (cookies, object.session.user_agent().to_owned())
    };
    if let Err(e) = bypass.settings.store_bypass(&cookies, &user_agent) {
        tracing::error!(target: "fmd.lua", "WebsiteBypass: {e}");
    }
    // `Storage['reload'].Contains('true')` (:185).
    let reload = bypass.module.storage_value("reload");
    if reload.windows(4).any(|w| w == b"true") {
        return borrow(http)?.request(|s| {
            s.reset();
            s.request(method, url)
        });
    }
    Ok(true)
}

/// `WebsiteBypassGetAnswer` (baseunits/lua/LuaWebsiteBypass.pas:118-140): runs
/// `websitebypass.lua` in `lua`, then `____WebsiteBypass(method, url)`; an error is logged and
/// counts as failure.
fn website_bypass_answer(lua: &Lua, file: &Path, method: &str, url: &str) -> bool {
    let answer = (|| {
        run_file(lua, file)?;
        match lua.globals().get::<Value>("____WebsiteBypass")? {
            Value::Function(bypass) => Ok(truthy(&bypass.call::<Value>((method, url))?)),
            _ => Ok(false),
        }
    })();
    answer.unwrap_or_else(|e: mlua::Error| {
        tracing::error!(target: "fmd.lua", "WebsiteBypass: {e}");
        false
    })
}
