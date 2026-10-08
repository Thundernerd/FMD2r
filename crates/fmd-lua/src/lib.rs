//! Lua runtime and the full FMD2 Host API that website modules see (the core of FMD2r).

mod class;
pub mod crypto;
mod duktape;
mod file;
mod globals;
mod libs;
mod memory_stream;
mod strings;
pub mod xquery;

use std::path::PathBuf;

pub use class::LuaClass;
pub use duktape::JsLimits;
pub use fmd_http::TerminateToken;
pub use globals::Globals;
pub use libs::subprocess;
pub use memory_stream::{LuaMemoryStream, MemoryStream};
pub use mlua;
pub use strings::{ListIndexError, LuaStrings, StringList};

/// Errors raised by the Lua runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Lua chunk failed to compile or raised an error, or a value failed to convert.
    #[error(transparent)]
    Lua(#[from] mlua::Error),
}

impl From<Error> for mlua::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Lua(error) => error,
        }
    }
}

/// Result type of the `fmd-lua` crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The `lua/` directory of a runtime, stored as app data on its Lua state.
#[derive(Clone, Default)]
struct LuaDir(PathBuf);

/// One Lua state with the FMD2 Host API installed.
pub struct Runtime {
    lua: mlua::Lua,
}

impl Runtime {
    /// Creates a Lua 5.4 state with every standard library opened, like `luaL_openlibs` in
    /// FMD2's base state (baseunits/lua/LuaBase.pas:123), with the `fmd.*` Host API libraries
    /// implemented so far (`fmd.strings`, [`crypto`], `fmd.duktape`, gzip, fileutil, logger, subprocess, imagepuzzle,
    /// mangafoxwatermark, the pcre2 stub, and the `pb` C module) in `package.preload`. FMD2's package
    /// searcher (:124) comes with T06.
    pub fn new() -> Result<Runtime> {
        // SAFETY: FMD2 opens every standard library, including `debug` (used by e.g.
        // lua/modules/MangaPlus.lua), which mlua only loads in unsafe mode. Later tickets also
        // need C modules (`pb`), which the safe mode forbids.
        let lua =
            unsafe { mlua::Lua::unsafe_new_with(mlua::StdLib::ALL, mlua::LuaOptions::default()) };
        strings::register(&lua)?;
        crypto::register(&lua)?;
        lua.set_app_data(LuaDir(PathBuf::from("lua")));
        lua.set_app_data(duktape::JsSettings::default());
        duktape::register(&lua)?;
        libs::register(&lua)?;
        // `CreateTXQuery` (baseunits/lua/LuaXQuery.pas:196-199) needs an XPath backend: without
        // the `xpath-fpc` feature there is none yet (the native one is T34), so the global is
        // missing.
        #[cfg(feature = "xpath-fpc")]
        xquery::register(&lua, std::rc::Rc::new(fmd_xpath::fpc::FpcEngine))?;
        Ok(Runtime { lua })
    }

    /// Sets the directory holding FMD2's `lua/` tree (`modules/`, `utils/`, ...). Defaults to
    /// `lua` in the working directory, like `DukLibDir` (baseunits/Duktape.pas:13).
    pub fn set_lua_dir(&self, dir: impl Into<PathBuf>) {
        self.lua.set_app_data(LuaDir(dir.into()));
    }

    /// Sets the time and memory bounds of every `fmd.duktape.ExecJS` call.
    pub fn set_js_limits(&self, limits: JsLimits) {
        self.js_settings(|settings| settings.limits = limits);
    }

    /// Ties the runtime to its worker's token: terminating it interrupts a running
    /// `fmd.duktape.ExecJS` script, which then fails like a script error.
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
