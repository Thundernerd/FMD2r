//! FMD2's package searcher and `fmd.env` (docs/tickets/T06-module-loader-module-object.md).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::fs;

use fmd_lua::Runtime;

#[test]
fn fmd_env_lua_directory_is_the_configured_lua_dir_with_a_trailing_separator() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new().unwrap();
    runtime.set_lua_dir(dir.path());

    let lua_dir: String = runtime.eval("require 'fmd.env'.LuaDirectory").unwrap();

    assert_eq!(lua_dir, format!("{}/", dir.path().display()));
}

/// MangaHub's `GetPageNumber` needs a revision of at least 6920 (lua/templates/MangaHub.lua:122);
/// FMD2's builds set it to the commit count, a decimal string (git2revision.bat).
#[test]
fn fmd_env_revision_is_a_numeric_string_of_at_least_6920() {
    let runtime = Runtime::new().unwrap();

    let digits: bool = runtime
        .eval("return require 'fmd.env'.Revision:match('^%d+$') ~= nil")
        .unwrap();
    let revision: f64 = runtime
        .eval("return tonumber(require 'fmd.env'.Revision)")
        .unwrap();

    assert!(digits);
    assert!(revision >= 6920.0, "{revision}");
}

/// A runtime whose lua dir is a fresh temporary directory holding `files` (path, source).
fn runtime_with_files(files: &[(&str, &str)]) -> (Runtime, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, source) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let runtime = Runtime::new().unwrap();
    runtime.set_lua_dir(dir.path());
    (runtime, dir)
}

#[test]
fn dotted_names_resolve_to_files_under_the_lua_dir() {
    let (runtime, _dir) = runtime_with_files(&[(
        "utils/json.lua",
        "local name = ...; return { decode = function() return 'decoded' end, name = name }",
    )]);

    runtime
        .exec(
            r#"
            local json = require 'utils.json'
            assert(json.decode() == 'decoded')
            assert(json.name == 'utils.json')
            assert(require 'utils.json' == json)
            "#,
        )
        .unwrap();
}

#[test]
fn fmd_names_never_load_files_even_when_one_exists() {
    let (runtime, _dir) = runtime_with_files(&[("fmd/nolib.lua", "return {}")]);

    let loaded = runtime
        .eval::<bool>("(pcall(require, 'fmd.nolib'))")
        .unwrap();

    assert!(!loaded);
}

#[test]
fn a_missing_file_falls_through_to_the_standard_searchers() {
    let (runtime, _dir) = runtime_with_files(&[]);

    runtime
        .exec(
            r#"
            package.preload['some.lib'] = function() return 'from preload' end
            assert(require 'some.lib' == 'from preload')
            -- The standard searchers stay after FMD2's.
            assert(#package.searchers == 5)
            "#,
        )
        .unwrap();
}

#[test]
fn host_libs_registered_later_are_required_by_their_fmd_name() {
    let runtime = Runtime::new().unwrap();
    runtime
        .register_host_lib("answer", |lua| {
            let lib = lua.create_table()?;
            lib.set("Value", 42)?;
            Ok(lib)
        })
        .unwrap();

    let value: i64 = runtime.eval("require 'fmd.answer'.Value").unwrap();

    assert_eq!(value, 42);
}

#[test]
fn runtimes_sharing_a_package_cache_compile_each_file_once() {
    let (first, dir) = runtime_with_files(&[("utils/counter.lua", "return 'first version'")]);
    let cache = fmd_lua::PackageCache::new();
    first.set_package_cache(cache.clone());
    assert_eq!(
        first.eval::<String>("require 'utils.counter'").unwrap(),
        "first version"
    );
    fs::write(
        dir.path().join("utils/counter.lua"),
        "return 'second version'",
    )
    .unwrap();

    let second = Runtime::new().unwrap();
    second.set_lua_dir(dir.path());
    second.set_package_cache(cache.clone());
    assert_eq!(
        second.eval::<String>("require 'utils.counter'").unwrap(),
        "first version"
    );

    cache.clear();
    let third = Runtime::new().unwrap();
    third.set_lua_dir(dir.path());
    third.set_package_cache(cache);
    assert_eq!(
        third.eval::<String>("require 'utils.counter'").unwrap(),
        "second version"
    );
}

#[test]
fn required_files_may_start_with_a_bom() {
    let (runtime, _dir) = runtime_with_files(&[("utils/bom.lua", "\u{feff}return 'ok'")]);

    assert_eq!(runtime.eval::<String>("require 'utils.bom'").unwrap(), "ok");
}
