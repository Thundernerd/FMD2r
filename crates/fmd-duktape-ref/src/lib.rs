//! Test-only reference for `fmd.duktape`: FMD2's `ExecJS` (baseunits/Duktape.pas:77-104) on the
//! Duktape 2.3.0 FMD2 bundles (`DUK_VERSION = 20300`, baseunits/Duktape.Api.pas:431).

use std::ffi::{CString, c_char, c_int};
use std::path::Path;

unsafe extern "C" {
    fn fmd_duk_exec(
        src: *const c_char,
        src_len: usize,
        lib_dir: *const c_char,
        out: *mut *mut c_char,
        out_len: *mut usize,
    ) -> c_int;
    fn fmd_duk_free(p: *mut c_char);
}

/// Why `ExecJS` raised instead of returning (baseunits/Duktape.pas:86-95).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DuktapeError {
    /// The script threw; the message is the error coerced by `duk_safe_to_string`.
    #[error("Duktape error: {}", String::from_utf8_lossy(.0))]
    Script(Vec<u8>),
    /// The heap or the result buffer could not be allocated.
    #[error("Failed to create a Duktape heap.")]
    Heap,
    #[error("the library directory contains a NUL byte")]
    LibDir,
}

/// `ExecJS(text)` (baseunits/Duktape.pas:77-104) with `lib_dir` as `DukLibDir`: the completion
/// value up to the first NUL (a Pascal string), `""` for `"undefined"`.
pub fn exec_js(text: &[u8], lib_dir: &Path) -> Result<Vec<u8>, DuktapeError> {
    let lib_dir =
        CString::new(lib_dir.as_os_str().as_encoded_bytes()).map_err(|_| DuktapeError::LibDir)?;
    let mut out: *mut c_char = std::ptr::null_mut();
    let mut out_len = 0usize;
    // SAFETY: `text` and `lib_dir` outlive the call; on return `out` is null or a malloc'd buffer
    // of `out_len` bytes, copied out and released with `fmd_duk_free`.
    let (rc, bytes) = unsafe {
        let rc = fmd_duk_exec(
            text.as_ptr().cast(),
            text.len(),
            lib_dir.as_ptr(),
            &mut out,
            &mut out_len,
        );
        let bytes = if out.is_null() {
            Vec::new()
        } else {
            let bytes = std::slice::from_raw_parts(out.cast::<u8>(), out_len).to_vec();
            fmd_duk_free(out);
            bytes
        };
        (rc, bytes)
    };
    // `duk_safe_to_string` returns a C string, assigned to a Pascal string.
    let s = bytes.split(|&b| b == 0).next().unwrap_or_default().to_vec();
    match rc {
        0 if s == b"undefined" => Ok(Vec::new()),
        0 => Ok(s),
        1 => Err(DuktapeError::Script(s)),
        _ => Err(DuktapeError::Heap),
    }
}
