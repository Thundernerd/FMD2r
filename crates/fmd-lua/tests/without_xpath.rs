//! Without the `xpath-fpc` feature there is no XPath backend; say so instead of passing silently.

#[cfg(not(feature = "xpath-fpc"))]
#[test]
#[ignore = "the fpc XPath backend is off: install fpc and run `cargo test -p fmd-lua --features xpath-fpc`"]
fn xquery_tests_need_the_xpath_fpc_feature() {}
