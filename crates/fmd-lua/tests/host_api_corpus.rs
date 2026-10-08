//! The unknown-Host-API report: every Host API name the upstream modules and templates reference
//! (T02's static scan) checked against what a callback's Lua state provides
//! (docs/tickets/T14-callback-runner-worker-pool.md, "Corpus check").

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use fmd_lua::scan_host_api_names;
use fmd_testkit::{check_each, corpus_root, module_files};

/// The module files, then the templates they build on.
fn corpus_files() -> Vec<PathBuf> {
    let mut files = module_files().unwrap();
    let mut templates: Vec<PathBuf> = fs::read_dir(corpus_root().join("templates"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "lua"))
        .collect();
    templates.sort();
    files.extend(templates);
    files
}

#[test]
fn every_host_api_name_the_corpus_references_is_implemented() {
    let files = corpus_files();
    let mut referenced = BTreeSet::new();
    let mut per_file = Vec::new();
    for file in &files {
        let names = scan_host_api_names(&String::from_utf8_lossy(&fs::read(file).unwrap()));
        referenced.extend(names.iter().cloned());
        per_file.push(names);
    }
    let missing = fmd_lua::missing_host_api(referenced.iter().map(String::as_str)).unwrap();

    let mut names = per_file.into_iter();
    let outcome = check_each(&files, |_| {
        let unknown: Vec<_> = names
            .next()
            .unwrap_or_default()
            .intersection(&missing)
            .cloned()
            .collect();
        match unknown.is_empty() {
            true => Ok(()),
            false => Err(format!("unknown Host API names: {}", unknown.join(", "))),
        }
    });
    let report = outcome.err().map(|r| r.to_string()).unwrap_or_default();
    println!(
        "host API corpus: {} names referenced by {} files, {} not implemented\n{report}",
        referenced.len(),
        files.len(),
        missing.len()
    );
    // FMD2's `MANGAINFO` has no `Artist` either (baseunits/lua/LuaMangaInfo.pas:18-37): the
    // assignment in modules/OrckuMangas.lua is an upstream typo that FMD2 silently ignores
    // too. Any other name listed is a Host API gap (or an upstream change to look at).
    assert_eq!(
        missing,
        BTreeSet::from(["MANGAINFO.Artist".to_owned()]),
        "{report}"
    );
}

#[test]
fn missing_host_api_lists_absent_members_libraries_and_globals() {
    let names = [
        "HTTP.GET",
        "HTTP.Nope",
        "TASK.PageLinks",
        "MODULE.Account",
        "LINKS.Count",
        "fmd.crypto",
        "fmd.nope",
        "Trim",
        "WORKID",
        "Nope",
    ];

    let missing = fmd_lua::missing_host_api(names).unwrap();

    let expected = ["HTTP.Nope", "fmd.nope", "Nope"].map(String::from);
    assert_eq!(missing, BTreeSet::from(expected));
}
