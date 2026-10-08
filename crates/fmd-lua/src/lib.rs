//! Lua runtime and the full FMD2 Host API that website modules see (the core of FMD2r).

mod class;
mod libs;

pub use class::LuaClass;
pub use libs::subprocess;
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

/// One Lua state with the FMD2 Host API installed.
pub struct Runtime {
    lua: mlua::Lua,
}

impl Runtime {
    /// Creates a Lua 5.4 state with every standard library opened, like `luaL_openlibs` in
    /// FMD2's base state (baseunits/lua/LuaBase.pas:123). The Host API libraries and package
    /// loader it registers next (:124-125) are installed as far as they exist so far.
    pub fn new() -> Result<Runtime> {
        // SAFETY: FMD2 opens every standard library, including `debug` (used by e.g.
        // lua/modules/MangaPlus.lua), which mlua only loads in unsafe mode. Later tickets also
        // need C modules (`pb`), which the safe mode forbids.
        let lua =
            unsafe { mlua::Lua::unsafe_new_with(mlua::StdLib::ALL, mlua::LuaOptions::default()) };
        libs::register(&lua)?;
        Ok(Runtime { lua })
    }

    /// The underlying Lua state, for registering Host API objects and globals.
    pub fn lua(&self) -> &mlua::Lua {
        &self.lua
    }

    /// Makes `fmd.subprocess` start its processes through `spawner` instead of the system.
    pub fn set_spawner(&self, spawner: impl subprocess::Spawner + 'static) {
        let mut config = subprocess::Config::of(&self.lua);
        config.spawner = std::rc::Rc::new(spawner);
        self.lua.set_app_data(config);
    }

    /// Sets the directory `fmd.subprocess` runs commands in, against which their relative paths
    /// resolve; FMD2 runs them in its own directory, the parent of `lua/`. Defaults to the
    /// process's current directory.
    pub fn set_working_dir(&self, dir: impl Into<std::path::PathBuf>) {
        let mut config = subprocess::Config::of(&self.lua);
        config.working_dir = Some(dir.into());
        self.lua.set_app_data(config);
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
