//! One worker thread and the Lua state it keeps per module (`TLuaWebsiteModuleHandler`,
//! baseunits/lua/LuaWebsiteModuleHandler.pas:33-64, over `TLuaHandler`,
//! baseunits/lua/LuaHandler.pas:42-151).

use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::Arc;

use fmd_http::{HttpSession, TerminateToken};
use fmd_xpath::{LoggingEngine, XPathEngine};
use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, MultiValue, Value};

use super::callbacks::{self, Callback};
use super::{Bytecode, CallbackError, Envelope, Job, JobError, JobResult, Queue, Shared};
use crate::module::lock;
use crate::{
    Globals, HttpModule, LuaHttp, Module, ModuleHttpOverrides, ModuleHttpSettings, Runtime,
    SettingsStoreError, XPathBackend, create_http,
};

/// Calls a function under `xpcall`, returning the traceback taken where it failed (or `''`),
/// then `xpcall`'s results. It holds the standard functions it uses, so a module that
/// replaces them cannot break it.
const INVOKE: &str = r#"
local xpcall, traceback, pack, unpack = xpcall, debug.traceback, table.pack, table.unpack
return function(f)
  local trace = ''
  local r = pack(xpcall(f, function(e) trace = traceback(nil, 2); return e end))
  return trace, unpack(r, 1, r.n)
end
"#;

/// Callbacks a state runs between two full collections: `CallFunction` collects once its
/// counter passes 15 (baseunits/lua/LuaHandler.pas:134-144).
const CALLS_PER_GC: u32 = 16;

/// Runs worker `index`: takes its jobs off `queue` until the pool shuts down.
pub(super) fn run(shared: &Shared, queue: &Queue, index: usize) {
    let mut loaded = None;
    while let Some(Envelope { job, reply }) = queue.pop(index) {
        let module = job.module.def().id;
        let callback = job.call.callback();
        let result =
            std::panic::catch_unwind(AssertUnwindSafe(|| process(shared, &mut loaded, job)));
        let result = result.unwrap_or_else(|panic| {
            // The state may be half-way through anything; the next job builds a new one.
            loaded = None;
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            Err(JobError::Callback(CallbackError {
                module,
                callback,
                function: String::new(),
                message: format!("panic: {message}"),
                traceback: String::new(),
            }))
        });
        // The caller may have stopped waiting.
        let _ = reply.send(result);
    }
}

/// The Lua state of a worker and the module loaded in it.
struct Loaded {
    module: Arc<Module>,
    /// When the state was built, to compare with invalidations.
    stamp: u64,
    invoke: Function,
    /// Callbacks run since the last full collection (`FCallFunctionCount`).
    calls: u32,
    /// The values FMD2 would find at the top and bottom of the state's stack, which it never
    /// clears: a callback that returns nothing reads whatever is on top
    /// (e.g. `lua_toboolean(L.Handle, -1)`, baseunits/lua/LuaWebsiteModules.pas:165).
    /// Not modelled: FMD2's anti-bot bypass runs in the same state and clears its stack
    /// afterwards (`L.ClearStack`, baseunits/lua/LuaWebsiteBypass.pas:139), so there a
    /// callback that went through the bypass and returns nothing reads `nil`. The bypass
    /// arrives with T30.
    top: Value,
    bottom: Option<Value>,
    runtime: Runtime,
}

fn process(shared: &Shared, loaded: &mut Option<Loaded>, job: Job) -> Result<JobResult, JobError> {
    let Job {
        module,
        call,
        http,
        terminate,
        affinity: _,
    } = job;
    let callback = call.callback();
    let def = module.def();
    if callback.function(&def).is_none() {
        return Err(JobError::NoCallback {
            module: def.id,
            callback,
        });
    }
    let state = load(shared, loaded, &module, callback)?;
    state.runtime.set_terminate_token(terminate.clone());
    let mut ctx = Ctx {
        state,
        shared,
        module: &module,
        http,
        lua_http: None,
        terminate,
    };
    let answer = callbacks::run(&mut ctx, call)?;
    let http = ctx.take_http();
    Ok(JobResult { answer, http })
}

/// `LoadModule` (baseunits/lua/LuaWebsiteModuleHandler.pas:33-54): the worker's state when it
/// already runs `module`, and was built after the module was last invalidated; otherwise a
/// new state, after the old one is closed.
fn load<'a>(
    shared: &Shared,
    loaded: &'a mut Option<Loaded>,
    module: &Arc<Module>,
    callback: Callback,
) -> Result<&'a mut Loaded, JobError> {
    let id = module.def().id;
    let current = loaded
        .as_ref()
        .is_some_and(|l| Arc::ptr_eq(&l.module, module) && !shared.is_stale(&id, l.stamp));
    if !current {
        *loaded = None;
        *loaded = Some(
            build(shared, module).map_err(|(message, traceback)| CallbackError {
                module: id,
                callback,
                function: String::new(),
                message,
                traceback,
            })?,
        );
    }
    loaded.as_mut().ok_or(JobError::Closed)
}

/// A new state running `module`: `LuaNewBaseState` (baseunits/lua/LuaBase.pas:119-130), then
/// the module's bytecode runs (`LuaDoMe`, baseunits/lua/LuaWebsiteModules.pas:840-843) and
/// `MODULE` is set (`LuaPushMe`, :820-823).
fn build(shared: &Shared, module: &Arc<Module>) -> Result<Loaded, (String, String)> {
    let plain = |message: String| (message, String::new());
    let stamp = shared.stamp();
    let def = module.def();
    let runtime = Runtime::new().map_err(|e| plain(format!("new Lua state: {e}")))?;
    runtime.set_lua_dir(&shared.lua_dir);
    // FMD2 runs in the parent of `lua/`, which upstream's relative paths assume, e.g.
    // `lua\websitebypass\websitebypass_config.json` (lua/websitebypass/cloudflare.lua:272).
    // A bare `lua` has an empty parent: the current directory, the default.
    if let Some(dir) = shared
        .lua_dir
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
    {
        runtime.set_working_dir(dir);
    }
    runtime.set_package_cache(shared.package.clone());
    runtime
        .install_globals(Globals {
            module: Some(def.id.clone()),
            terminate: None,
        })
        .map_err(|e| plain(format!("new Lua state: {e}")))?;
    // The XPath backend of `CreateTXQuery`: the configured one, else the runtime's default;
    // wrapped to record into the differential corpus when one is set.
    if shared.xpath_backend.is_some() || shared.xpath_corpus.is_some() {
        let engine = match shared.xpath_backend {
            Some(backend) => backend
                .engine()
                .ok_or(crate::Error::MissingXPathBackend(backend)),
            None => crate::default_xpath_engine()
                .ok_or(crate::Error::MissingXPathBackend(XPathBackend::default())),
        }
        .map_err(|e| plain(format!("new Lua state: {e}")))?;
        let engine: Rc<dyn XPathEngine> = match &shared.xpath_corpus {
            Some(corpus) => Rc::new(LoggingEngine::new(engine, corpus.hook())),
            None => engine,
        };
        runtime
            .set_xpath_engine(engine)
            .map_err(|e| plain(format!("new Lua state: {e}")))?;
    }
    let lua = runtime.lua();
    let invoke = lua
        .load(INVOKE)
        .set_name("=invoke")
        .call::<Function>(())
        .map_err(|e| plain(format!("new Lua state: {e}")))?;
    let chunk = bytecode(shared, lua, &def.id, &def.file, stamp).map_err(plain)?;
    // `LuaExecute` (baseunits/lua/LuaBase.pas:227-238) runs the chunk without results.
    let results: MultiValue = invoke
        .call(chunk)
        .map_err(|e| plain(format!("lua_pcall(): {e}")))?;
    let mut results = results.into_iter();
    let trace = results.next().unwrap_or(Value::Nil);
    if let Some(Value::Boolean(false)) = results.next() {
        let error = results.next().unwrap_or(Value::Nil);
        return Err((
            format!("lua_pcall(): {}", to_text(lua, &error)),
            to_text(lua, &trace),
        ));
    }
    runtime
        .set_module(module)
        .map_err(|e| plain(format!("MODULE: {e}")))?;
    Ok(Loaded {
        module: module.clone(),
        stamp,
        invoke,
        calls: 0,
        top: Value::Nil,
        bottom: None,
        runtime,
    })
}

/// The compiled chunk of the module file, from the pool's bytecode cache when it was compiled
/// after the module was last invalidated, else read and compiled now.
fn bytecode(
    shared: &Shared,
    lua: &Lua,
    id: &str,
    file: &std::path::Path,
    stamp: u64,
) -> Result<Function, String> {
    let mut cache = lock(&shared.bytecode);
    let cached = cache
        .get(file)
        .filter(|cached| !shared.is_stale(id, cached.stamp))
        .map(|cached| cached.code.clone());
    let code = match cached {
        Some(code) => code,
        None => {
            let chunk = crate::file::load_lua_file(lua, file)
                .map_err(|e| format!("luaL_loadfile(): {e}"))?;
            let code: Arc<[u8]> = chunk.dump(false).into();
            let cached = Bytecode {
                stamp,
                code: code.clone(),
            };
            cache.insert(file.to_path_buf(), cached);
            code
        }
    };
    lua.load(&code[..])
        .set_name(format!("@{}", file.display()))
        .set_mode(ChunkMode::Binary)
        .into_function()
        .map_err(|e| format!("luaL_loadbuffer(): {e}"))
}

/// What a callback runs with: the worker's state, the job's module and its HTTP session.
pub(super) struct Ctx<'a> {
    state: &'a mut Loaded,
    shared: &'a Shared,
    module: &'a Arc<Module>,
    /// The job's session, until `set_http` hands it to Lua.
    http: Option<HttpSession>,
    lua_http: Option<LuaHttp>,
    terminate: TerminateToken,
}

/// No HTTP overrides, and nothing stored.
struct NoOverrides;

impl ModuleHttpSettings for NoOverrides {
    fn http_overrides(&self) -> Option<ModuleHttpOverrides> {
        None
    }

    fn clear_cookies(&self) -> Result<(), SettingsStoreError> {
        Ok(())
    }

    fn store_bypass(&self, _: &str, _: &str) -> Result<(), SettingsStoreError> {
        Ok(())
    }
}

impl Ctx<'_> {
    pub(super) fn lua(&self) -> &Lua {
        self.state.runtime.lua()
    }

    pub(super) fn module(&self) -> &Arc<Module> {
        self.module
    }

    /// Runs `f`, which sets the callback's globals; a failure is the callback's.
    pub(super) fn setup(
        &mut self,
        callback: Callback,
        f: impl FnOnce(&mut Self) -> mlua::Result<()>,
    ) -> Result<(), JobError> {
        f(self).map_err(|e| self.error(callback, e.to_string(), String::new()))
    }

    /// Sets the global `HTTP` (`L.LoadObject('HTTP', ...)`) over the job's session, or a new
    /// one for the module (`CreateHTTP` and `PrepareHTTP`, baseunits/WebsiteModules.pas:382-387, :353-380), tied to the
    /// job's termination. Its requests run the module's anti-bot hook
    /// (`WebsiteBypassHTTPRequest`, baseunits/WebsiteModules.pas:272-276).
    pub(super) fn set_http(&mut self) -> mlua::Result<()> {
        let settings: Arc<dyn ModuleHttpSettings> = match &self.shared.http_settings {
            Some(source) => source(self.module),
            None => Arc::new(NoOverrides),
        };
        let mut session = match self.http.take() {
            Some(session) => session,
            None => {
                let module = HttpModule {
                    http: self.module.http().clone(),
                    settings: settings.clone(),
                };
                create_http(&self.shared.http, Some(&module))
            }
        };
        session.set_terminate_token(self.terminate.clone());
        let http = LuaHttp::with_website_bypass(session, self.module.clone(), settings);
        self.lua().globals().set("HTTP", http.build(self.lua())?)?;
        self.lua_http = Some(http);
        Ok(())
    }

    /// The session behind `HTTP`, or the job's own when the callback set no `HTTP`.
    fn take_http(&mut self) -> Option<HttpSession> {
        match self.lua_http.take() {
            Some(http) => http.into_session(self.shared.http.session()).ok(),
            None => self.http.take(),
        }
    }

    /// `CallFunction` (baseunits/lua/LuaHandler.pas:134-144) over `LuaCallFunction`
    /// (baseunits/lua/LuaBase.pas:132-144): after 16 calls, two full collections; then the
    /// global the callback names is called without arguments. Returns the value then on top
    /// of FMD2's stack: the callback's last result, or what was on top before when it returned
    /// none. A call that fails does not count towards the collection.
    pub(super) fn call(&mut self, callback: Callback) -> Result<Value, JobError> {
        let name = callback
            .function(&self.module.def())
            .unwrap_or_default()
            .to_owned();
        let lua = self.state.runtime.lua().clone();
        if self.state.calls >= CALLS_PER_GC {
            let collected = lua.gc_collect().and_then(|()| lua.gc_collect());
            collected.map_err(|e| self.error(callback, e.to_string(), String::new()))?;
            self.state.calls = 0;
        }
        let function: Value = lua
            .globals()
            .get(name.as_str())
            .map_err(|e| self.error(callback, e.to_string(), String::new()))?;
        if function.is_nil() {
            // `lua_settop(L, 1)`: only the bottom of the stack is left, or the `nil` just
            // pushed onto an empty one.
            let bottom = self.state.bottom.clone().unwrap_or(Value::Nil);
            self.state.bottom = Some(bottom.clone());
            self.state.top = bottom;
            let message = format!("No function name \"{name}\"");
            return Err(self.error_in(callback, &name, message, String::new()));
        }
        let results: MultiValue = self
            .state
            .invoke
            .call(function)
            .map_err(|e| self.error_in(callback, &name, e.to_string(), String::new()))?;
        let mut results = results.into_iter();
        let trace = results.next().unwrap_or(Value::Nil);
        let ok = results.next().unwrap_or(Value::Nil);
        let results: Vec<Value> = results.collect();
        if let Some(first) = results.first() {
            self.state.bottom.get_or_insert_with(|| first.clone());
        }
        if let Some(last) = results.last() {
            self.state.top = last.clone();
        }
        if ok != Value::Boolean(true) {
            let error = results.last().cloned().unwrap_or(Value::Nil);
            let message = to_text(&lua, &error);
            return Err(self.error_in(callback, &name, message, to_text(&lua, &trace)));
        }
        self.state.calls += 1;
        Ok(self.state.top.clone())
    }

    /// Pushes the value `f` reads onto FMD2's stack, as reading a global with `lua_getglobal`
    /// does, and returns it.
    pub(super) fn push_global(
        &mut self,
        callback: Callback,
        f: impl FnOnce(&Lua) -> mlua::Result<Value>,
    ) -> Result<Value, JobError> {
        let value =
            f(self.lua()).map_err(|e| self.error(callback, e.to_string(), String::new()))?;
        self.state.bottom.get_or_insert_with(|| value.clone());
        self.state.top = value.clone();
        Ok(value)
    }

    pub(super) fn error(&self, callback: Callback, message: String, traceback: String) -> JobError {
        let name = callback
            .function(&self.module.def())
            .unwrap_or_default()
            .to_owned();
        self.error_in(callback, &name, message, traceback)
    }

    fn error_in(
        &self,
        callback: Callback,
        function: &str,
        message: String,
        traceback: String,
    ) -> JobError {
        JobError::Callback(CallbackError {
            module: self.module.def().id,
            callback,
            function: function.to_owned(),
            message,
            traceback,
        })
    }
}

/// An error value or traceback as text: `luaToString` (baseunits/lua/LuaUtils.pas:206) for
/// strings and numbers, the message of a Rust error, and the type name of anything else.
fn to_text(lua: &Lua, value: &Value) -> String {
    match value {
        Value::Error(e) => e.to_string(),
        Value::String(_) | Value::Integer(_) | Value::Number(_) => {
            match lua.coerce_string(value.clone()) {
                Ok(Some(s)) => s.to_string_lossy(),
                _ => String::new(),
            }
        }
        other => format!("({})", other.type_name()),
    }
}
