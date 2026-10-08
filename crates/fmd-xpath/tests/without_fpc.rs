//! Without the `fpc` feature there is no backend to test; say so instead of passing silently.

#[cfg(not(feature = "fpc"))]
#[test]
#[ignore = "the fpc backend is off: install fpc and run `cargo test -p fmd-xpath --features fpc`"]
fn fpc_backend_tests_need_the_fpc_feature() {}
