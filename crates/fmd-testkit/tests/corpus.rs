//! The committed upstream Lua snapshot in `fixtures/lua`.

use fmd_testkit::{check_each, corpus_root, module_files};

#[test]
fn fixture_is_present_with_the_full_module_set() {
    let root = corpus_root();
    for dir in ["modules", "templates", "utils", "websitebypass"] {
        assert!(
            root.join(dir).is_dir(),
            "missing {}",
            root.join(dir).display()
        );
    }
    let sha = std::fs::read_to_string(root.join("UPSTREAM_REF")).unwrap();
    assert_eq!(
        sha.trim().len(),
        40,
        "UPSTREAM_REF should hold a commit SHA: {sha:?}"
    );

    let files = module_files().unwrap();
    assert!(files.len() > 500, "only {} module files", files.len());
}

#[test]
fn module_files_are_every_lua_file_under_modules_sorted() {
    let files = module_files().unwrap();
    let modules = corpus_root().join("modules");

    assert!(files.iter().all(|f| f.parent() == Some(modules.as_path())));
    assert!(
        files
            .iter()
            .all(|f| f.extension().is_some_and(|e| e == "lua"))
    );
    assert!(files.is_sorted());
    // Non-Lua files next to the modules are not modules.
    assert!(modules.join("MangaPlus.proto").is_file());
    assert!(files.contains(&modules.join("MangaDex.lua")));
    let mut names: Vec<_> = files.iter().filter_map(|f| f.file_name()).collect();
    names.dedup();
    assert_eq!(names.len(), files.len());
}

#[test]
fn check_each_reports_every_failing_module_at_once() {
    let files = module_files().unwrap();
    let mut visited = 0;

    let report = check_each(&files, |path| {
        visited += 1;
        match path.file_name().and_then(|n| n.to_str()) {
            Some("18Kami.lua") => Err("Init failed"),
            Some("MangaDex.lua") => Err("unknown Host API name HTTP.Frobnicate"),
            _ => Ok(()),
        }
    })
    .unwrap_err()
    .to_string();

    assert_eq!(visited, files.len());
    assert!(
        report.starts_with(&format!("2 of {} modules failed:\n", files.len())),
        "{report}"
    );
    assert!(
        report.contains("modules/18Kami.lua: Init failed"),
        "{report}"
    );
    assert!(
        report.contains("modules/MangaDex.lua: unknown Host API name HTTP.Frobnicate"),
        "{report}"
    );
}

#[test]
fn check_each_passes_when_every_module_passes() {
    let files = module_files().unwrap();
    check_each(&files, |_| Ok::<_, String>(())).unwrap();
}
