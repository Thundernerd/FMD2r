//! The shared `fmd-xpath` suite over the `fpc` backend
//! (docs/tickets/T08-fmd-xpath-trait-ffi-lua-bindings.md, "Seams under test").
//!
//! Expected values come from FMD2's engine set-up (baseunits/XQueryEngineHTML.pas:384-400).

#![cfg(feature = "fpc")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod shared;

shared::suite!(fmd_xpath::fpc::FpcEngine);
