//! The shared `fmd-xpath` suite over the `native` backend.

#![cfg(feature = "native")]
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

mod shared;

shared::suite!(fmd_xpath::native::NativeEngine);
