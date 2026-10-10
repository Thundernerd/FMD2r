//! MemoryStream objects (`HTTP.Document`) (docs/tickets/T04-tstrings-memorystream.md). Expected
//! values were probed from FPC 3.2.2's `TMemoryStream`.

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_lua::{LuaMemoryStream, Runtime};

/// A runtime with a fresh stream in the global `m`, and the Rust handle to it.
fn runtime_with_stream() -> (Runtime, LuaMemoryStream) {
    let rt = Runtime::new().unwrap();
    let stream = LuaMemoryStream::new();
    let object = stream.build(rt.lua()).unwrap();
    rt.lua().globals().set("m", object).unwrap();
    (rt, stream)
}

#[test]
fn write_string_is_binary_safe() {
    let (rt, stream) = runtime_with_stream();
    rt.exec(r"m.WriteString('a\0b'); assert(m.Size == 3)")
        .unwrap();
    assert_eq!(stream.stream().borrow().bytes(), b"a\0b");
    // `ToString` reads from the position, which writing left at the end.
    rt.exec("assert(m.ToString() == '')").unwrap();
    stream.stream().borrow_mut().set_position(0);
    rt.exec(r"assert(m.ToString() == 'a\0b' and m:ReadString() == 'a\0b' and m.Size == 3)")
        .unwrap();
}

#[test]
fn write_string_overwrites_from_the_position() {
    // lua/modules/MangaPlus.lua:212-217's read-transform-write, position left at 0 by HTTP.
    let (rt, stream) = runtime_with_stream();
    stream.stream().borrow_mut().write(b"hello");
    stream.stream().borrow_mut().set_position(0);
    rt.exec(
        "local d = m.ToString(); assert(d == 'hello'); m.WriteString('HE')
             assert(m.Size == 5 and m.ToString() == 'llo')",
    )
    .unwrap();
    assert_eq!(stream.stream().borrow().bytes(), b"HEllo");
}

#[test]
fn size_grows_with_zeros_and_shrinking_clamps_the_position() {
    let (rt, stream) = runtime_with_stream();
    rt.exec(r"m.WriteString('abc'); m.Size = 5; assert(m.Size == 5)")
        .unwrap();
    assert_eq!(stream.stream().borrow().bytes(), b"abc\0\0");
    assert_eq!(stream.stream().borrow().position(), 3);
    rt.exec("m.Size = 2; assert(m.Size == 2)").unwrap();
    assert_eq!(stream.stream().borrow().position(), 2);
    rt.exec("m:Clear(); assert(m.Size == 0)").unwrap();
    assert_eq!(stream.stream().borrow().position(), 0);
}

#[test]
fn save_and_load_file_round_trip_binary_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.bin");
    let (rt, stream) = runtime_with_stream();
    rt.lua()
        .globals()
        .set("path", path.to_str().unwrap())
        .unwrap();
    rt.exec(r"m.WriteString('a\0b\255'); m.SaveToFile(path); assert(m.Size == 4)")
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"a\0b\xff");
    // Loading keeps the position (here 2), as FPC's `TMemoryStream.LoadFromStream` does.
    rt.exec(
        "m.Clear(); m.WriteString('xy'); m:LoadFromFile(path)
             assert(m.Size == 4 and m.ToString() == 'b\\255')",
    )
    .unwrap();
    assert_eq!(stream.stream().borrow().bytes(), b"a\0b\xff");
}

#[test]
fn loading_a_missing_file_is_a_lua_error() {
    let (rt, _) = runtime_with_stream();
    let err = rt
        .exec("m.LoadFromFile('/nonexistent/fmd2r/missing.bin')")
        .unwrap_err();
    assert!(err.to_string().contains("Unable to open file"), "{err}");
}

#[test]
fn every_lua_memory_stream_pas_member_is_present() {
    // baseunits/lua/LuaMemoryStream.pas:68-80.
    let (rt, _) = runtime_with_stream();
    rt.exec(
        "for _, name in ipairs({'ToString', 'ReadString', 'WriteString', 'LoadFromFile',
                 'SaveToFile', 'Clear'}) do
               assert(type(m[name]) == 'function', name)
             end
             m:WriteString('x'); assert(m.Size == 1)",
    )
    .unwrap();
}
