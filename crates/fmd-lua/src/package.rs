//! FMD2's package searcher (baseunits/lua/LuaPackage.pas): `require 'fmd.<lib>'` opens a host
//! library, any other name loads `<lua dir>/<name with . → />.lua` from a cache of compiled
//! chunks shared between Lua states.

use std::collections::HashMap;
use std::path::{MAIN_SEPARATOR, PathBuf};
use std::sync::{Arc, Mutex};

use mlua::prelude::LuaChunkMode as ChunkMode;
use mlua::{Function, Lua, Table, Value};

use crate::class::to_bytes;
use crate::{LuaDir, app_data_or_default};

/// The prefix of host library names (`LIBPREFIX`, baseunits/lua/LuaPackage.pas:23).
const LIB_PREFIX: &str = "fmd.";

/// Registry key of the table mapping `fmd.<lib>` names to their openers (FMD2's
/// `HostPackage` cache, baseunits/lua/LuaPackage.pas:42, :148).
const HOST_LIBS_KEY: &str = "fmd.package.hostlibs";

/// The compiled Lua files `require` has loaded, by path, shared by every Lua state that holds a
/// clone (FMD2's global `Package` cache, baseunits/lua/LuaPackage.pas:43, :147). Each file is
/// read and compiled once, then every state loads the same bytecode.
#[derive(Clone, Default)]
pub struct PackageCache(Arc<Mutex<HashMap<PathBuf, Arc<[u8]>>>>);

impl PackageCache {
    /// An empty cache.
    pub fn new() -> Self {
        PackageCache::default()
    }

    /// Drops every cached chunk, so the next `require` reads the files again
    /// (`ClearCache`, baseunits/lua/LuaPackage.pas:141-144).
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Arc<[u8]>>> {
        // A poisoned cache still holds valid compiled chunks.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The bytecode of `path`, compiled in `lua` and cached on first use; `None` when the file
    /// does not exist (`LoadLuaFile`, baseunits/lua/LuaPackage.pas:117-130).
    fn chunk(&self, lua: &Lua, path: &PathBuf) -> mlua::Result<Option<Arc<[u8]>>> {
        if let Some(chunk) = self.lock().get(path) {
            return Ok(Some(chunk.clone()));
        }
        if !path.is_file() {
            return Ok(None);
        }
        // `LuaDumpFileToStream` keeps debug info so errors name file and line
        // (baseunits/lua/LuaBase.pas:187-210).
        let chunk: Arc<[u8]> = crate::file::load_lua_file(lua, path)?.dump(false).into();
        self.lock().insert(path.clone(), chunk.clone());
        Ok(Some(chunk))
    }
}

/// Installs FMD2's searcher as `package.searchers[1]`, shifting the standard ones down
/// (`RegisterLoader`, baseunits/lua/LuaPackage.pas:93-115).
pub(crate) fn register(lua: &Lua) -> mlua::Result<()> {
    lua.set_named_registry_value(HOST_LIBS_KEY, lua.create_table()?)?;
    lua.set_app_data(PackageCache::new());
    let searchers: Table = lua.globals().get::<Table>("package")?.get("searchers")?;
    let count = searchers.raw_len();
    for i in (1..=count).rev() {
        searchers.raw_set(i + 1, searchers.raw_get::<Value>(i)?)?;
    }
    searchers.raw_set(1, lua.create_function(find_package)?)?;
    add_lib(lua, "env", open_env)
}

/// Makes `require 'fmd.<name>'` return the table `open` builds (`AddLib`,
/// baseunits/lua/LuaPackage.pas:132-139).
pub(crate) fn add_lib<F>(lua: &Lua, name: &str, open: F) -> mlua::Result<()>
where
    F: Fn(&Lua) -> mlua::Result<Table> + 'static,
{
    let libs: Table = lua.named_registry_value(HOST_LIBS_KEY)?;
    libs.raw_set(
        format!("{LIB_PREFIX}{name}"),
        lua.create_function(move |lua, _: mlua::MultiValue| open(lua))?,
    )
}

/// `_findpackage` (baseunits/lua/LuaPackage.pas:62-91): returns the loader for `name`, or
/// nothing so the standard searchers try next.
///
/// Unlike FMD2, an `fmd.` name with no host library ends here instead of also being looked up
/// as `<lua dir>/fmd/<lib>.lua`, so host library names never reach the filesystem.
fn find_package(lua: &Lua, name: Value) -> mlua::Result<Option<Function>> {
    let name = String::from_utf8_lossy(&to_bytes(lua, name)?).into_owned();
    if name.starts_with(LIB_PREFIX) {
        let libs: Table = lua.named_registry_value(HOST_LIBS_KEY)?;
        return libs.raw_get(name);
    }
    let LuaDir(dir) = app_data_or_default(lua);
    let path = dir.join(format!("{}.lua", name.replace('.', "/")));
    let cache: PackageCache = app_data_or_default(lua);
    let loaded = cache.chunk(lua, &path).and_then(|chunk| {
        chunk
            .map(|chunk| {
                lua.load(&*chunk)
                    .set_name(format!("@{}", path.display()))
                    .set_mode(ChunkMode::Binary)
                    .into_function()
            })
            .transpose()
    });
    match loaded {
        Ok(loader) => Ok(loader),
        // FMD2 logs a file that fails to load and lets the next searcher try (:85-89).
        Err(e) => {
            tracing::error!(target: "fmd.lua", "require '{name}': {e}");
            Ok(None)
        }
    }
}

/// `fmd.env` (`luaopen_fmd`, baseunits/lua/LuaFMD.pas:15-27), with FMD2r's values.
fn open_env(lua: &Lua) -> mlua::Result<Table> {
    let exe = std::env::current_exe().unwrap_or_default();
    let directory = exe
        .parent()
        .map(|d| with_separator(d.display().to_string()))
        .unwrap_or_default();
    let exe_name = exe
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let LuaDir(lua_dir) = app_data_or_default(lua);
    let env = lua.create_table()?;
    // `FMD_DIRECTORY` and `FMD_EXENAME` (baseunits/FMDOptions.pas:261-262): the program's
    // directory with a trailing separator and its file name without extension.
    env.raw_set("Directory", directory)?;
    env.raw_set("ExeName", exe_name)?;
    env.raw_set("Version", env!("CARGO_PKG_VERSION"))?;
    env.raw_set(
        "Revision",
        option_env!("FMD2R_REVISION").unwrap_or_default(),
    )?;
    // `LUA_REPO_FOLDER` (baseunits/FMDOptions.pas:297) ends with a separator.
    env.raw_set(
        "LuaDirectory",
        with_separator(lua_dir.display().to_string()),
    )?;
    // `SimpleTranslator.LastSelected`: FMD2r has no UI translations, so modules see English.
    env.raw_set("SelectedLanguage", "en")?;
    Ok(env)
}

/// `path` ending with the directory separator.
fn with_separator(mut path: String) -> String {
    if !path.ends_with(MAIN_SEPARATOR) {
        path.push(MAIN_SEPARATOR);
    }
    path
}
