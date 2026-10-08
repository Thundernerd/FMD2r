//! Lua runtime and the full FMD2 Host API that website modules see (the core of FMD2r).

mod class;
pub mod crypto;

pub use class::LuaClass;
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
    /// FMD2's base state (baseunits/lua/LuaBase.pas:123), with the `fmd.*` Host API libraries
    /// implemented so far requirable (see [`crypto`]). FMD2's package searcher (:124)
    /// comes with T06.
    pub fn new() -> Result<Runtime> {
        // SAFETY: FMD2 opens every standard library, including `debug` (used by e.g.
        // lua/modules/MangaPlus.lua), which mlua only loads in unsafe mode. Later tickets also
        // need C modules (`pb`), which the safe mode forbids.
        let lua =
            unsafe { mlua::Lua::unsafe_new_with(mlua::StdLib::ALL, mlua::LuaOptions::default()) };
        crypto::register(&lua)?;
        Ok(Runtime { lua })
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
