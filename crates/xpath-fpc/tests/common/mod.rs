//! Raw FFI declarations for `fmdxpath.h`, with owning wrappers so tests free every handle.

#![allow(dead_code)] // each test crate uses a different subset

use std::ffi::{c_char, c_int};

#[repr(C)]
pub struct FxDoc {
    _private: [u8; 0],
}

#[repr(C)]
pub struct FxValue {
    _private: [u8; 0],
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
    pub fn fx_value_to_string(value: *mut FxValue) -> FxString;
    pub fn fx_string_free(s: FxString);
    pub fn fx_last_error() -> FxString;
    pub fn fx_thread_exit();
    pub fn fx_value_inner_html(value: *mut FxValue) -> FxString;
    pub fn fx_value_outer_html(value: *mut FxValue) -> FxString;
    pub fn fx_value_inner_text(value: *mut FxValue) -> FxString;
    pub fn fx_value_get_attribute(value: *mut FxValue, name: *const c_char, len: usize)
    -> FxString;
    pub fn fx_value_kind(value: *mut FxValue) -> c_int;
    pub fn fx_value_get_property(
        value: *mut FxValue,
        name: *const c_char,
        len: usize,
    ) -> *mut FxValue;
}

#[repr(C)]
pub struct FxString {
    pub ptr: *const c_char,
    pub len: usize,
}

/// Copies an `fx_string` into a Rust string and frees it.
pub fn take(s: FxString) -> String {
    let bytes = if s.ptr.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) }.to_vec()
    };
    unsafe { fx_string_free(s) };
    String::from_utf8(bytes).unwrap()
}

/// Owns a parsed document handle.
pub struct Doc(pub *mut FxDoc);

impl Doc {
    pub fn parse(html: &str) -> Doc {
        let doc = unsafe { fx_doc_parse(html.as_ptr().cast(), html.len()) };
        assert!(!doc.is_null());
        Doc(doc)
    }

    pub fn eval(&self, expr: &str) -> Value {
        self.eval_in(expr, None, false)
    }

    pub fn eval_in(&self, expr: &str, context: Option<&Value>, is_css: bool) -> Value {
        let context = context.map_or(std::ptr::null_mut(), |c| c.0);
        let value = unsafe {
            fx_eval(
                self.0,
                expr.as_ptr().cast(),
                expr.len(),
                context,
                c_int::from(is_css),
            )
        };
        assert!(!value.is_null(), "fx_eval never returns NULL");
        Value(value)
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        unsafe { fx_doc_free(self.0) }
    }
}

/// Owns a value handle.
pub struct Value(pub *mut FxValue);

impl Value {
    pub fn count(&self) -> i64 {
        unsafe { fx_value_count(self.0) }
    }

    /// 1-based, like `IXQValue.get`.
    pub fn get(&self, index: i64) -> Value {
        let value = unsafe { fx_value_get(self.0, index) };
        assert!(!value.is_null());
        Value(value)
    }

    pub fn string(&self) -> String {
        take(unsafe { fx_value_to_string(self.0) })
    }

    pub fn inner_html(&self) -> String {
        take(unsafe { fx_value_inner_html(self.0) })
    }

    pub fn outer_html(&self) -> String {
        take(unsafe { fx_value_outer_html(self.0) })
    }

    pub fn inner_text(&self) -> String {
        take(unsafe { fx_value_inner_text(self.0) })
    }

    pub fn kind(&self) -> c_int {
        unsafe { fx_value_kind(self.0) }
    }

    pub fn property(&self, name: &str) -> Value {
        let value = unsafe { fx_value_get_property(self.0, name.as_ptr().cast(), name.len()) };
        assert!(!value.is_null());
        Value(value)
    }

    pub fn attribute(&self, name: &str) -> String {
        take(unsafe { fx_value_get_attribute(self.0, name.as_ptr().cast(), name.len()) })
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        unsafe { fx_value_free(self.0) }
    }
}
