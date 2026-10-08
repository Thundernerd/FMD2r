//! Lua runtime and the full FMD2 Host API that website modules see (the core of FMD2r).

mod class;
mod file;
mod globals;
mod memory_stream;
mod strings;
pub mod xquery;

pub use class::LuaClass;
pub use globals::Globals;
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

/// One Lua state with the FMD2 Host API installed.
pub struct Runtime {
    lua: mlua::Lua,
}

impl Runtime {
    /// Creates a Lua 5.4 state with every standard library opened, like `luaL_openlibs` in
    /// FMD2's base state (baseunits/lua/LuaBase.pas:123), with `fmd.strings` in
    /// `package.preload`. The other Host API libraries and the package loader it registers next
    /// (:124-125) come with later tickets.
    pub fn new() -> Result<Runtime> {
        // SAFETY: FMD2 opens every standard library, including `debug` (used by e.g.
        // lua/modules/MangaPlus.lua), which mlua only loads in unsafe mode. Later tickets also
        // need C modules (`pb`), which the safe mode forbids.
        let lua =
            unsafe { mlua::Lua::unsafe_new_with(mlua::StdLib::ALL, mlua::LuaOptions::default()) };
        strings::register(&lua)?;
        // `CreateTXQuery` (baseunits/lua/LuaXQuery.pas:196-199) needs an XPath backend: without
        // the `xpath-fpc` feature there is none yet (the native one is T34), so the global is
        // missing.
        #[cfg(feature = "xpath-fpc")]
        xquery::register(&lua, std::rc::Rc::new(fmd_xpath::fpc::FpcEngine))?;
        Ok(Runtime { lua })
    }

    /// The underlying Lua state, for registering Host API objects and globals.
    pub fn lua(&self) -> &mlua::Lua {
        &self.lua
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
