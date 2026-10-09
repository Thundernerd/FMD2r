//! Scanning `lua/modules` and running every file's `Init()` (`TLuaWebsiteModulesLoader`,
//! baseunits/lua/LuaWebsiteModules.pas:467-656).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use mlua::Function;

use super::{MemorySettingsStore, Module, ModuleSettingsStore, build_module};
use crate::{Globals, PackageCache, Runtime};

/// The modules every file's `Init` declared, sorted by ID like FMD2's `Modules.Sort`
/// (baseunits/WebsiteModules.pas:465-468, called at
/// baseunits/lua/LuaWebsiteModules.pas:655).
#[derive(Default)]
pub struct ModuleRegistry {
    modules: Vec<Arc<Module>>,
}

/// The outcome of [`ModuleRegistry::load_dir`].
pub struct LoadReport {
    pub registry: ModuleRegistry,
    /// How many module files were found.
    pub files: usize,
    /// Every file that failed, with its error, sorted by path.
    pub failures: Vec<LoadFailure>,
}

/// A module file that failed to load, or the module directory when it could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadFailure {
    pub file: PathBuf,
    pub error: String,
}

/// What one file's `Init` produced.
struct FileResult {
    modules: Vec<Arc<Module>>,
    error: Option<String>,
}

impl ModuleRegistry {
    /// Loads every module in `<lua_dir>/modules`, keeping options and cookies in memory.
    pub fn load_dir(lua_dir: &Path) -> LoadReport {
        Self::load_dir_with(lua_dir, Arc::new(MemorySettingsStore::new()))
    }

    /// Loads every module in `<lua_dir>/modules`, with options and cookies in `settings`.
    ///
    /// Like `ScanAndLoadFiles` (baseunits/lua/LuaWebsiteModules.pas:636-656): one thread per
    /// CPU, a fresh Lua state per file, one shared `require` cache. A failing file is reported
    /// and the scan goes on.
    pub fn load_dir_with(lua_dir: &Path, settings: Arc<dyn ModuleSettingsStore>) -> LoadReport {
        let dir = lua_dir.join("modules");
        match module_files(&dir) {
            Ok(files) => Self::load_files_with(lua_dir, &files, settings),
            Err(e) => LoadReport {
                registry: ModuleRegistry::default(),
                files: 0,
                failures: vec![LoadFailure {
                    file: dir,
                    error: e.to_string(),
                }],
            },
        }
    }

    /// Loads `files` (in `<lua_dir>/modules`) like [`load_dir_with`], for re-scanning only the
    /// files an update changed.
    ///
    /// [`load_dir_with`]: ModuleRegistry::load_dir_with
    pub fn load_files_with(
        lua_dir: &Path,
        files: &[PathBuf],
        settings: Arc<dyn ModuleSettingsStore>,
    ) -> LoadReport {
        let results = load_files(lua_dir, files);
        Self::collect(files.to_vec(), results, &settings)
    }

    /// Loads one module file as the scan does (`LoadLuaWebsiteModule`,
    /// baseunits/lua/LuaWebsiteModules.pas:514-590), keeping options and cookies in memory.
    pub fn load_file(lua_dir: &Path, file: &Path) -> LoadReport {
        let settings: Arc<dyn ModuleSettingsStore> = Arc::new(MemorySettingsStore::new());
        let files = vec![file.to_path_buf()];
        let results = load_files(lua_dir, &files);
        Self::collect(files, results, &settings)
    }

    /// `results` are in `files` order.
    fn collect(
        files: Vec<PathBuf>,
        results: Vec<FileResult>,
        settings: &Arc<dyn ModuleSettingsStore>,
    ) -> LoadReport {
        let mut modules = Vec::new();
        let mut failures = Vec::new();
        for (file, result) in files.iter().zip(results) {
            let mut error = result.error;
            for module in result.modules {
                if let Err(e) = module.attach_settings(settings.clone()) {
                    error.get_or_insert(e);
                }
                modules.push(module);
            }
            if let Some(error) = error {
                failures.push(LoadFailure {
                    file: file.clone(),
                    error,
                });
            }
        }
        LoadReport {
            registry: ModuleRegistry::from_modules(modules),
            files: files.len(),
            failures,
        }
    }

    /// A registry of `modules`, sorted by ID.
    pub fn from_modules(mut modules: Vec<Arc<Module>>) -> ModuleRegistry {
        // `TModuleContainerCompare` compares IDs with `AnsiCompareStr`; IDs are ASCII.
        modules.sort_by_key(|m| m.def_read().id.clone());
        ModuleRegistry { modules }
    }

    /// Every module, sorted by ID.
    pub fn modules(&self) -> &[Arc<Module>] {
        &self.modules
    }

    pub fn get(&self, id: &str) -> Option<&Arc<Module>> {
        self.modules.iter().find(|m| m.def_read().id == id)
    }

    /// The module serving `host` (`LocateModuleByHost`, baseunits/WebsiteModules.pas:500-534):
    /// the last module, by ID, whose `RootURL` contains the lowercased host; else the bare host
    /// name (`SplitURL` without protocol and port); else, when that name holds a `w` (FMD2's
    /// `w+\d*` regex matches anywhere), the name without its first four characters.
    ///
    /// FMD2 first tries the module it located last; every call here starts afresh.
    pub fn locate_by_host(&self, host: &str) -> Option<&Arc<Module>> {
        // Pascal's `Pos` never finds an empty string.
        let pos_module = |s: &str| {
            self.modules
                .iter()
                .rev()
                .find(|m| !s.is_empty() && m.def_read().root_url.contains(s))
        };
        // Pascal's `LowerCase` only maps ASCII letters.
        let host = host.to_ascii_lowercase();
        if let Some(module) = pos_module(&host) {
            return Some(module);
        }
        let name = bare_host(&host);
        pos_module(&name).or_else(|| {
            if name.starts_with("www.") || name.contains('w') {
                pos_module(name.get(4..).unwrap_or_default())
            } else {
                None
            }
        })
    }
}

/// The host of `url` without protocol and port: `SplitURL(url, @host, nil, False, False)`
/// (baseunits/httpsendthread.pas:191-276).
fn bare_host(url: &str) -> String {
    let (host, _) = fmd_http::split_url_bytes(url.as_bytes());
    let host = String::from_utf8_lossy(&host).into_owned();
    // `split_url_bytes` adds `proto://` and appends `:port` when there is one.
    let host = host.split_once("://").map_or(host.as_str(), |(_, h)| h);
    match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            name.to_owned()
        }
        _ => host.to_owned(),
    }
}

/// The module files in `dir`, sorted (`FindAllFiles(..., '*.lua;*.luac', False)`,
/// baseunits/lua/LuaWebsiteModules.pas:644).
fn module_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let is_module = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lua") || e.eq_ignore_ascii_case("luac"));
        if is_module && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Runs every file's `Init` on one thread per CPU (`TLuaWebsiteModulesLoaderThread.Execute`,
/// baseunits/lua/LuaWebsiteModules.pas:592-606). Results come back in `files` order.
fn load_files(lua_dir: &Path, files: &[PathBuf]) -> Vec<FileResult> {
    let cache = PackageCache::new();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, FileResult)>> = Mutex::new(Vec::with_capacity(files.len()));
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(files.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(file) = files.get(i) else { break };
                    let result = load_file(lua_dir, file, &cache);
                    super::lock(&results).push((i, result));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results.sort_by_key(|(i, _)| *i);
    results.into_iter().map(|(_, result)| result).collect()
}

/// `LoadLuaWebsiteModule` (baseunits/lua/LuaWebsiteModules.pas:514-590): runs the file's
/// `Init`, then keeps the modules it created that have an ID and a name, with their `RootURL`
/// lowercased. As in FMD2, modules created before `Init` failed are kept too.
fn load_file(lua_dir: &Path, file: &Path, cache: &PackageCache) -> FileResult {
    let created = Rc::new(RefCell::new(Vec::new()));
    let error = run_init(lua_dir, file, cache, &created).err();
    let modules = created
        .take()
        .into_iter()
        .filter(|m: &Arc<Module>| {
            let mut def = m.def_write();
            // Pascal's `LowerCase` only maps ASCII letters.
            def.root_url.make_ascii_lowercase();
            !def.id.is_empty() && !def.name.is_empty()
        })
        .collect();
    FileResult { modules, error }
}

/// `DoInit` (baseunits/lua/LuaWebsiteModules.pas:473-500): a fresh state runs the file, then
/// gets the global `NewWebsiteModule`, then `Init()` runs. Errors carry FMD2's wording.
fn run_init(
    lua_dir: &Path,
    file: &Path,
    cache: &PackageCache,
    created: &Rc<RefCell<Vec<Arc<Module>>>>,
) -> Result<(), String> {
    let runtime = Runtime::new().map_err(|e| format!("new Lua state: {e}"))?;
    runtime.set_lua_dir(lua_dir);
    runtime.set_package_cache(cache.clone());
    runtime
        .install_globals(Globals::default())
        .map_err(|e| format!("new Lua state: {e}"))?;
    let lua = runtime.lua();
    let chunk = crate::file::load_lua_file(lua, file).map_err(|e| format!("luaL_loadfile: {e}"))?;
    chunk
        .call::<()>(())
        .map_err(|e| format!("lua_pcall: {e}"))?;
    let Ok(init) = lua.globals().get::<Function>("Init") else {
        return Err(r#"no function name "Init()""#.to_owned());
    };
    let new_module = {
        let created = created.clone();
        let file = file.to_path_buf();
        // `_newwebsitemodule` (baseunits/lua/LuaWebsiteModules.pas:467-471).
        lua.create_function(move |lua, ()| {
            let module = Arc::new(Module::new(file.clone()));
            created.borrow_mut().push(module.clone());
            build_module(lua, &module)
        })
    };
    let set = new_module.and_then(|f| lua.globals().set("NewWebsiteModule", f));
    set.map_err(|e| format!("NewWebsiteModule: {e}"))?;
    init.call::<()>(())
        .map_err(|e| format!(r#"LuaCallFunction("Init()"): {e}"#))
}
