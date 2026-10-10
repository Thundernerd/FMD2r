//! Every upstream module in the corpus fixture through `ModuleRegistry::load_dir`
//! (docs/tickets/T06-module-loader-module-object.md).

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_lua::ModuleRegistry;
use fmd_testkit::{check_each, corpus_root, module_files};

#[test]
fn every_upstream_module_passes_init() {
    let report = ModuleRegistry::load_dir(&corpus_root());
    let files = module_files().unwrap();
    assert_eq!(report.files, files.len());

    let outcome = check_each(&files, |path| {
        match report.failures.iter().find(|f| f.file == path) {
            Some(failure) => Err(failure.error.clone()),
            None => Ok(()),
        }
    });
    println!(
        "module corpus: {} of {} files pass Init, {} modules loaded",
        files.len() - report.failures.len(),
        files.len(),
        report.registry.modules().len()
    );
    // Every file passed when this was written; a failing `Init` is a Host API regression.
    outcome.unwrap();
}
