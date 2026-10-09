//! The `fpc` backend: FMD2's own engine (internettools) through the C ABI of `libfmdxpath.so`
//! (crates/xpath-fpc/fmdxpath.h).

use std::any::Any;
use std::ffi::{c_char, c_int};
use std::ptr::{self, NonNull};

use crate::{Document, Error, Kind, Result, XPathEngine, XPathValue};

mod ffi {
    use std::ffi::{c_char, c_int};

    #[repr(C)]
    pub struct FxDoc {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct FxValue {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct FxString {
        pub ptr: *const c_char,
        pub len: usize,
    }

    #[link(name = "fmdxpath")]
    unsafe extern "C" {
        pub fn fx_doc_parse(html: *const c_char, len: usize) -> *mut FxDoc;
        pub fn fx_doc_free(doc: *mut FxDoc);
        pub fn fx_eval(
            doc: *mut FxDoc,
            expr: *const c_char,
            len: usize,
            context: *mut FxValue,
            is_css: c_int,
        ) -> *mut FxValue;
        pub fn fx_value_free(value: *mut FxValue);
        pub fn fx_value_count(value: *mut FxValue) -> i64;
        pub fn fx_value_get(value: *mut FxValue, index: i64) -> *mut FxValue;
        pub fn fx_value_kind(value: *mut FxValue) -> c_int;
        pub fn fx_value_to_string(value: *mut FxValue) -> FxString;
        pub fn fx_value_inner_html(value: *mut FxValue) -> FxString;
        pub fn fx_value_outer_html(value: *mut FxValue) -> FxString;
        pub fn fx_value_inner_text(value: *mut FxValue) -> FxString;
        pub fn fx_value_get_attribute(
            value: *mut FxValue,
            name: *const c_char,
            len: usize,
        ) -> FxString;
        pub fn fx_value_get_property(
            value: *mut FxValue,
            name: *const c_char,
            len: usize,
        ) -> *mut FxValue;
        pub fn fx_string_free(s: FxString);
        pub fn fx_last_error() -> FxString;
        pub fn fx_thread_exit();
    }
}

/// `FX_KIND_UNDEFINED` (crates/xpath-fpc/fmdxpath.h).
const FX_KIND_UNDEFINED: c_int = 0;

/// The `fpc` backend. Any thread may use it; a document and its values stay on one thread
/// (they are neither `Send` nor `Sync`).
#[derive(Debug, Default, Clone, Copy)]
pub struct FpcEngine;

impl XPathEngine for FpcEngine {
    fn parse(&self, html: &[u8]) -> Result<Box<dyn Document>> {
        thread_guard();
        // SAFETY: the pointer and length describe `html`, which outlives the call.
        let doc = unsafe { ffi::fx_doc_parse(html.as_ptr().cast::<c_char>(), html.len()) };
        match NonNull::new(doc) {
            Some(doc) => Ok(Box::new(FpcDocument(doc))),
            None => Err(Error::Parse(last_error())),
        }
    }
}

/// An owned `fx_doc`.
struct FpcDocument(NonNull<ffi::FxDoc>);

impl Document for FpcDocument {
    fn eval(&self, expr: &str, context: Option<&dyn XPathValue>, css: bool) -> Box<dyn XPathValue> {
        let context = match context {
            None => ptr::null_mut(),
            Some(value) => match value.as_any().downcast_ref::<FpcValue>() {
                Some(value) => value.0.as_ptr(),
                // A value of another backend.
                None => return Box::new(EmptyValue),
            },
        };
        // SAFETY: the document and context handles are live; the expression buffer outlives
        // the call.
        let value = unsafe {
            ffi::fx_eval(
                self.0.as_ptr(),
                expr.as_ptr().cast::<c_char>(),
                expr.len(),
                context,
                c_int::from(css),
            )
        };
        wrap(value)
    }
}

impl Drop for FpcDocument {
    fn drop(&mut self) {
        // SAFETY: the handle is live and freed exactly once; values keep their own reference.
        unsafe { ffi::fx_doc_free(self.0.as_ptr()) }
    }
}

/// An owned `fx_value`.
struct FpcValue(NonNull<ffi::FxValue>);

impl FpcValue {
    fn raw(&self) -> *mut ffi::FxValue {
        self.0.as_ptr()
    }
}

impl XPathValue for FpcValue {
    fn count(&self) -> i64 {
        // SAFETY: the handle is live.
        unsafe { ffi::fx_value_count(self.raw()) }
    }

    fn get(&self, index: i64) -> Box<dyn XPathValue> {
        // SAFETY: the handle is live.
        wrap(unsafe { ffi::fx_value_get(self.raw(), index) })
    }

    fn is_undefined(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { ffi::fx_value_kind(self.raw()) == FX_KIND_UNDEFINED }
    }

    fn kind(&self) -> Kind {
        // SAFETY: the handle is live.
        kind(unsafe { ffi::fx_value_kind(self.raw()) })
    }

    fn string(&self) -> String {
        // SAFETY: the handle is live.
        take(unsafe { ffi::fx_value_to_string(self.raw()) })
    }

    fn inner_html(&self) -> String {
        // SAFETY: the handle is live.
        take(unsafe { ffi::fx_value_inner_html(self.raw()) })
    }

    fn outer_html(&self) -> String {
        // SAFETY: the handle is live.
        take(unsafe { ffi::fx_value_outer_html(self.raw()) })
    }

    fn inner_text(&self) -> String {
        // SAFETY: the handle is live.
        take(unsafe { ffi::fx_value_inner_text(self.raw()) })
    }

    fn attribute(&self, name: &str) -> String {
        // SAFETY: the handle is live; the name buffer outlives the call.
        take(unsafe {
            ffi::fx_value_get_attribute(self.raw(), name.as_ptr().cast::<c_char>(), name.len())
        })
    }

    fn property(&self, name: &str) -> Box<dyn XPathValue> {
        // SAFETY: the handle is live; the name buffer outlives the call.
        wrap(unsafe {
            ffi::fx_value_get_property(self.raw(), name.as_ptr().cast::<c_char>(), name.len())
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Drop for FpcValue {
    fn drop(&mut self) {
        // SAFETY: the handle is live and freed exactly once.
        unsafe { ffi::fx_value_free(self.raw()) }
    }
}

/// NULL only happens on out-of-memory (crates/xpath-fpc/fmdxpath.h); it becomes an empty value.
fn wrap(value: *mut ffi::FxValue) -> Box<dyn XPathValue> {
    match NonNull::new(value) {
        Some(value) => Box::new(FpcValue(value)),
        None => Box::new(EmptyValue),
    }
}

/// The empty sequence, for failures that never reach the library.
struct EmptyValue;

impl XPathValue for EmptyValue {
    fn count(&self) -> i64 {
        0
    }
    fn get(&self, _: i64) -> Box<dyn XPathValue> {
        Box::new(EmptyValue)
    }
    fn is_undefined(&self) -> bool {
        true
    }
    fn kind(&self) -> Kind {
        Kind::Undefined
    }
    fn string(&self) -> String {
        String::new()
    }
    fn inner_html(&self) -> String {
        String::new()
    }
    fn outer_html(&self) -> String {
        String::new()
    }
    fn inner_text(&self) -> String {
        String::new()
    }
    fn attribute(&self, _: &str) -> String {
        String::new()
    }
    fn property(&self, _: &str) -> Box<dyn XPathValue> {
        Box::new(EmptyValue)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The [`Kind`] of an `fx_kind` (crates/xpath-fpc/fmdxpath.h); the library returns no other.
fn kind(kind: c_int) -> Kind {
    match kind {
        1 => Kind::Boolean,
        2 => Kind::Int64,
        3 => Kind::Null,
        4 => Kind::Node,
        5 => Kind::Sequence,
        6 => Kind::Array,
        7 => Kind::Double,
        8 => Kind::String,
        9 => Kind::Decimal,
        10 => Kind::Binary,
        11 => Kind::QName,
        12 => Kind::DateTime,
        13 => Kind::Object,
        14 => Kind::Function,
        _ => Kind::Undefined,
    }
}

/// Copies an `fx_string` into a Rust string and frees it.
fn take(s: ffi::FxString) -> String {
    let text = if s.ptr.is_null() {
        String::new()
    } else {
        // SAFETY: a non-NULL `fx_string` points at `len` readable bytes until freed.
        let bytes = unsafe { std::slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    // SAFETY: the string came from the library and is freed exactly once.
    unsafe { ffi::fx_string_free(s) };
    text
}

/// The calling thread's last library error.
fn last_error() -> String {
    // SAFETY: always callable.
    take(unsafe { ffi::fx_last_error() })
}

/// Calls `fx_thread_exit` when a thread that used the library exits, so its per-thread state
/// doesn't leak (crates/xpath-fpc/fmdxpath.h, "Threads").
struct ThreadGuard;

impl Drop for ThreadGuard {
    fn drop(&mut self) {
        // SAFETY: always callable; handles stay valid.
        unsafe { ffi::fx_thread_exit() }
    }
}

thread_local! {
    static THREAD_GUARD: ThreadGuard = const { ThreadGuard };
}

/// Registers the calling thread's [`ThreadGuard`]. Parsing is the first call any thread makes:
/// documents and their values never leave the thread that parsed them.
fn thread_guard() {
    THREAD_GUARD.with(|_| {});
}
