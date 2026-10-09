//! Lua runtime and the full FMD2 Host API that website modules see (the core of FMD2r).

mod class;
pub mod crypto;
mod duktape;
mod file;
mod globals;
mod http;
mod libs;
mod memory_stream;
mod module;
mod package;
mod pool;
mod scan;
mod strings;
pub mod xquery;

use std::path::PathBuf;
use std::rc::Rc;

use fmd_xpath::XPathEngine;

pub use class::LuaClass;
pub use duktape::JsLimits;
pub use fmd_http::TerminateToken;
pub use fmd_xpath::Backend as XPathBackend;
pub use fmd_xpath::corpus::CorpusWriter as XPathCorpusWriter;
pub use globals::Globals;
pub use http::{
    HttpModule, LuaHttp, ModuleHttpOverrides, ModuleHttpSettings, ProxyOverride, create_http,
};
pub use libs::subprocess;
pub use memory_stream::{LuaMemoryStream, MemoryStream};
pub use mlua;
pub use module::{
    Account, AccountState, CriticalSection, LoadFailure, LoadReport, MemorySettingsStore, Module,
    ModuleDef, ModuleLimits, ModuleOption, ModuleRegistry, ModuleSettingsStore, OptionKind,
    OptionValue, SettingsStoreError,
};
pub use package::PackageCache;
pub use pool::{
    Affinity, Answer, Call, Callback, CallbackError, Caller, HttpSettingsSource, InfoReply,
    Invalidate, Job, JobError, JobResult, ListReply, MangaInfo, NamesAndLinks, PageCount, Pending,
    PoolConfig, Reply, Task, TaskReply, UpdateList, WorkerPool, missing_host_api,
};
pub use scan::scan_host_api_names;
pub use strings::{ListIndexError, LuaStrings, StringList};

/// Errors raised by the Lua runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Lua chunk failed to compile or raised an error, or a value failed to convert.
    #[error(transparent)]
    Lua(#[from] mlua::Error),
    /// The XPath backend asked for isn't part of this build (its cargo feature is off).
    #[error("the {0:?} XPath backend is not built in")]
    MissingXPathBackend(XPathBackend),
}

impl From<Error> for mlua::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Lua(error) => error,
            other => mlua::Error::external(other),
        }
    }
}

/// Result type of the `fmd-lua` crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The `lua/` directory of a runtime, stored as app data on its Lua state.
#[derive(Clone, Default)]
struct LuaDir(PathBuf);

/// A copy of the runtime's app data of type `T`, or its default when none is set.
fn app_data_or_default<T: Clone + Default + 'static>(lua: &mlua::Lua) -> T {
    lua.app_data_ref::<T>()
        .map(|data| data.clone())
        .unwrap_or_default()
}

/// One Lua state with the FMD2 Host API installed.
///
/// A state never leaves the thread that created it: like FMD2's per-thread handlers
/// (baseunits/lua/LuaWebsiteModuleHandler.pas:56-64), and because `lua_close` must run on that
/// thread for `__gc` to work (:77-78). The type is `!Send`:
///
/// ```compile_fail
/// fn send<T: Send>() {}
/// send::<fmd_lua::Runtime>();
/// ```
pub struct Runtime {
    lua: mlua::Lua,
}

/// The default XPath backend's engine: `native` (the `xpath.backend` setting's default), or `fpc`
/// when this build has no `native`; `None` with neither built in.
pub(crate) fn default_xpath_engine() -> Option<Rc<dyn XPathEngine>> {
    XPathBackend::default()
        .engine()
        .or_else(|| XPathBackend::Fpc.engine())
}

impl Runtime {
    /// Creates a Lua 5.4 state with every standard library opened, like `luaL_openlibs` in
    /// FMD2's base state (baseunits/lua/LuaBase.pas:123), with the `fmd.*` Host API libraries
    /// implemented so far (`fmd.env`, `fmd.strings`, [`crypto`], `fmd.duktape`, gzip, fileutil, logger,
    /// subprocess, imagepuzzle, mangafoxwatermark and the pcre2 stub) behind FMD2's package searcher
    /// (:125), and the `pb` C module in `package.preload`.
    pub fn new() -> Result<Runtime> {
        // SAFETY: FMD2 opens every standard library, including `debug` (used by e.g.
        // lua/modules/MangaPlus.lua), which mlua only loads in unsafe mode. Later tickets also
        // need C modules (`pb`), which the safe mode forbids.
        let lua =
            unsafe { mlua::Lua::unsafe_new_with(mlua::StdLib::ALL, mlua::LuaOptions::default()) };
        package::register(&lua)?;
        strings::register(&lua)?;
        crypto::register(&lua)?;
        lua.set_app_data(LuaDir(PathBuf::from("lua")));
        lua.set_app_data(duktape::JsSettings::default());
        duktape::register(&lua)?;
        libs::register(&lua)?;
        // `CreateTXQuery` (baseunits/lua/LuaXQuery.pas:196-199) over the default backend; with
        // none built in, the global is missing.
        if let Some(engine) = default_xpath_engine() {
            xquery::register(&lua, engine)?;
        }
        Ok(Runtime { lua })
    }

    /// Reinstalls `CreateTXQuery` over `backend` (the `xpath.backend` setting). TXQuery objects
    /// made before keep their backend.
    pub fn set_xpath_backend(&self, backend: XPathBackend) -> Result<()> {
        let engine = backend
            .engine()
            .ok_or(Error::MissingXPathBackend(backend))?;
        xquery::register(&self.lua, engine)
    }

    /// Reinstalls `CreateTXQuery` over `engine`, e.g. a backend wrapped in a
    /// [`LoggingEngine`](fmd_xpath::LoggingEngine). TXQuery objects made before keep theirs.
    pub fn set_xpath_engine(&self, engine: Rc<dyn XPathEngine>) -> Result<()> {
        xquery::register(&self.lua, engine)
    }

    /// Sets the directory holding FMD2's `lua/` tree (`modules/`, `utils/`, ...). Defaults to
    /// `lua` in the working directory, like `DukLibDir` (baseunits/Duktape.pas:13).
    pub fn set_lua_dir(&self, dir: impl Into<PathBuf>) {
        self.lua.set_app_data(LuaDir(dir.into()));
    }

    /// Makes `require 'fmd.<name>'` return the table `open` builds, like FMD2's `AddLib`
    /// (baseunits/lua/LuaPackage.pas:132-139). Registering a name again replaces it.
    pub fn register_host_lib<F>(&self, name: &str, open: F) -> Result<()>
    where
        F: Fn(&mlua::Lua) -> mlua::Result<mlua::Table> + 'static,
    {
        package::add_lib(&self.lua, name, open)?;
        Ok(())
    }

    /// Makes `require` share `cache` of compiled Lua files with every other runtime using it.
    /// Each runtime starts with a cache of its own.
    pub fn set_package_cache(&self, cache: PackageCache) {
        self.lua.set_app_data(cache);
    }

    /// Sets the time and memory bounds of every `fmd.duktape.ExecJS` call.
    pub fn set_js_limits(&self, limits: JsLimits) {
        self.js_settings(|settings| settings.limits = limits);
    }

    /// Ties the runtime to its worker's token: terminating it interrupts a running
    /// `fmd.duktape.ExecJS` script, which then fails like a script error, and cuts short a
    /// `sleep` installed without a token of its own.
    pub fn set_terminate_token(&self, token: TerminateToken) {
        self.js_settings(|settings| settings.terminate = token);
    }

    fn js_settings(&self, update: impl FnOnce(&mut duktape::JsSettings)) {
        if let Some(mut settings) = self.lua.app_data_mut::<duktape::JsSettings>() {
            update(&mut settings);
        }
    }

    /// The underlying Lua state, for registering Host API objects and globals.
    pub fn lua(&self) -> &mlua::Lua {
        &self.lua
    }

    /// Makes `fmd.subprocess` start its processes through `spawner` instead of the system.
    pub fn set_spawner(&self, spawner: impl subprocess::Spawner + 'static) {
        self.update_subprocess_config(|c| c.spawner = std::rc::Rc::new(spawner));
    }

    /// Sets the directory `fmd.subprocess` runs commands in, against which their relative paths
    /// resolve; FMD2 runs them in its own directory, the parent of `lua/`. Defaults to the
    /// process's current directory.
    pub fn set_working_dir(&self, dir: impl Into<std::path::PathBuf>) {
        self.update_subprocess_config(|c| c.working_dir = Some(dir.into()));
    }

    fn update_subprocess_config(&self, update: impl FnOnce(&mut subprocess::Config)) {
        let mut config = subprocess::Config::of(&self.lua);
        update(&mut config);
        self.lua.set_app_data(config);
    }

    /// Installs the global helper functions (`print`, `sleep`, `Trim`, `MaybeFillHost`,
    /// `MangaInfoStatusIfPos`, `GetBetween`, `SeparateLeft`, `SeparateRight`), like FMD2's
    /// `LuaBaseRegisterAll` (baseunits/lua/LuaBase.pas:86-92). Installing again replaces them.
    pub fn install_globals(&self, globals: Globals) -> Result<()> {
        globals::install(&self.lua, globals)?;
        Ok(())
    }

    /// Sets the global `MODULE` to an object over `module`, as FMD2 does before running a
    /// module's callbacks (`LuaPushMe`, baseunits/lua/LuaWebsiteModules.pas:820-823). Every
    /// runtime given the same module shares its properties, options, cookies, `Storage`,
    /// `Guardian` and `Account`.
    pub fn set_module(&self, module: &std::sync::Arc<Module>) -> Result<()> {
        let object = module::build_module(&self.lua, module)?;
        self.lua.globals().set("MODULE", object)?;
        Ok(())
    }

    /// Runs a chunk of Lua code.
    pub fn exec(&self, chunk: &str) -> Result<()> {
        self.lua.load(chunk).set_name("=exec").exec()?;
        Ok(())
    }

    /// Evaluates a Lua expression and converts its value to `T`.
    pub fn eval<T: mlua::FromLua>(&self, expr: &str) -> Result<T> {
        Ok(self.lua.load(expr).set_name("=eval").eval()?)
    }
}
