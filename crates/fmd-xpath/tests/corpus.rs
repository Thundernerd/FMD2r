//! The parity guard: the `native` backend gives the `fpc` backend's results on every entry of the
//! committed differential corpus (fixtures/xpath-corpus, recorded from the smoke list's replay).

#![cfg(feature = "diff")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use fmd_xpath::corpus::Corpus;
use fmd_xpath::fpc::FpcEngine;
use fmd_xpath::native::NativeEngine;

#[test]
fn the_native_backend_matches_fpc_on_the_corpus() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/xpath-corpus");
    let corpus = Corpus::load(&dir).unwrap();
    assert!(
        !corpus.entries().is_empty(),
        "no corpus in {}",
        dir.display()
    );

    let report = fmd_xpath::diff(&corpus, &FpcEngine, &NativeEngine);
    assert!(report.mismatches.is_empty(), "{}", report.render());
}
