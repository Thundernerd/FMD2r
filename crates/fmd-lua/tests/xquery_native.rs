//! The XQuery Lua suite over the `native` XPath backend (T34, issue #165).

#![cfg(feature = "xpath-native")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

mod xquery;

xquery::suite!(fmd_lua::XPathBackend::Native);
