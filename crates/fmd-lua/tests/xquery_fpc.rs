//! The XQuery Lua suite over the `fpc` XPath backend.

#![cfg(feature = "xpath-fpc")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::panic)]

mod xquery;

xquery::suite!(fmd_lua::XPathBackend::Fpc);
