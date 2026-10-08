//! Build wrapper for `libfmdxpath.so`, FMD2's XPath engine (internettools) exported over a C ABI.
//!
//! The C ABI is declared in `fmdxpath.h`; `fmd-xpath` binds it.

/// Directory that holds the built `libfmdxpath.so`.
pub const LIB_DIR: &str = env!("OUT_DIR");
