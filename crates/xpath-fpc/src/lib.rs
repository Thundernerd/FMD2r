//! Build wrapper for `libfmdxpath.so`, FMD2's XPath engine (internettools) exported over a C ABI.
//!
//! The C ABI is declared in `fmdxpath.h`; `fmd-xpath` binds it.

/// Directory that holds `libfmdxpath.so`: the one built into `OUT_DIR`, or `FMDXPATH_LIB_DIR`
/// when that was set at build time.
pub const LIB_DIR: &str = env!("FMDXPATH_LIB_DIR");
