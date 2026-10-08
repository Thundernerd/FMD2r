//! Lua runtime and the full FMD2 Host API that website modules see (the core of FMD2r).

mod class;
mod duktape;

use std::path::PathBuf;

pub use class::LuaClass;
pub use duktape::JsLimits;
pub use fmd_http::TerminateToken;
pub use mlua;

/// Errors raised by the Lua runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Lua chunk failed to compile or raised an error, or a value failed to convert.
    #[error(transparent)]
    Lua(#[from] mlua::Error),
}

/// Result type of the `fmd-lua` crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The `lua/` directory of a runtime, stored as app data on its Lua state.
struct LuaDir(PathBuf);

/// Makes `require(name)` return the table `open` builds, like `LuaPackage.AddLib` registers
/// `fmd.<name>` libraries (baseunits/lua/LuaDuktape.pas:39).
fn register_lib(
    lua: &mlua::Lua,
    name: &str,
    open: fn(&mlua::Lua) -> mlua::Result<mlua::Table>,
) -> mlua::Result<()> {
    let preload: mlua::Table = lua
        .globals()
        .get::<mlua::Table>("package")?
        .get("preload")?;
    preload.set(name, lua.create_function(move |lua, ()| open(lua))?)
}

/// One Lua state with the FMD2 Host API installed.
pub struct Runtime {
    lua: mlua::Lua,
}

impl Runtime {
    /// Creates a Lua 5.4 state with every standard library opened, like `luaL_openlibs` in
    /// FMD2's base state (baseunits/lua/LuaBase.pas:123). The Host API libraries and package
    /// loader it registers next (:124-125) come with later tickets.
    pub fn new() -> Result<Runtime> {
        // SAFETY: FMD2 opens every standard library, including `debug` (used by e.g.
        // lua/modules/MangaPlus.lua), which mlua only loads in unsafe mode. Later tickets also
        // need C modules (`pb`), which the safe mode forbids.
        let lua =
            unsafe { mlua::Lua::unsafe_new_with(mlua::StdLib::ALL, mlua::LuaOptions::default()) };
        lua.set_app_data(LuaDir(PathBuf::from("lua")));
        lua.set_app_data(duktape::JsSettings::default());
        register_lib(&lua, "fmd.duktape", duktape::open)?;
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
