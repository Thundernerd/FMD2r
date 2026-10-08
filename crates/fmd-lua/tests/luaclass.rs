//! `LuaClass` object semantics, exercised through Lua snippets run on the public runtime
//! (docs/tickets/T03-luaclass-binding-helper.md, "Seams under test").

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use std::cell::RefCell;
use std::rc::Rc;

use fmd_lua::{LuaClass, Runtime, mlua};

/// The test-only object type every snippet runs against.
#[derive(Default)]
struct TestObject {
    name: Vec<u8>,
    count: i32,
    enabled: bool,
    items: Vec<Vec<u8>>,
    named: std::collections::HashMap<Vec<u8>, Vec<u8>>,
    last_bytes: Vec<u8>,
}

/// The list index a default-array key names, if it is numeric (keys arrive as strings).
fn index(lua: &mlua::Lua, key: &mlua::Value) -> Option<usize> {
    let i = lua.coerce_integer(key.clone()).ok()??;
    usize::try_from(i).ok()
}

fn key_bytes(key: &mlua::Value) -> Vec<u8> {
    match key {
        mlua::Value::String(s) => s.as_bytes().to_vec(),
        _ => Vec::new(),
    }
}

fn runtime_with_obj() -> (Runtime, Rc<RefCell<TestObject>>) {
    let runtime = Runtime::new().unwrap();
    let storage = LuaClass::new(Rc::new(RefCell::new(())))
        .method("Ping", |_, _, ()| Ok("storage pong"))
        .build(runtime.lua())
        .unwrap();
    let state = Rc::new(RefCell::new(TestObject {
        items: vec![b"first".to_vec(), b"second".to_vec()],
        ..TestObject::default()
    }));
    let obj = LuaClass::new(state.clone())
        .method("Add", |_, _, (a, b): (i64, i64)| Ok(a + b))
        .method("Zero", |_, _, ()| Ok("zero"))
        .method("Argc", |_, _, args: mlua::Variadic<mlua::Value>| {
            Ok(args.len())
        })
        .method(
            "Bytes",
            |lua, o: &mut TestObject, bytes: mlua::LuaString| {
                o.last_bytes = bytes.as_bytes().to_vec();
                lua.create_string(&o.last_bytes)
            },
        )
        .string_property("Name", |o: &mut TestObject| &mut o.name)
        .integer_property("Count", |o: &mut TestObject| &mut o.count)
        .boolean_property("Enabled", |o: &mut TestObject| &mut o.enabled)
        .read_only_property("Upper", |lua, o: &mut TestObject| {
            lua.create_string(o.name.to_ascii_uppercase())
        })
        .object("Storage", storage)
        .array_property(
            "Letters",
            |lua, o: &mut TestObject, i: usize| {
                o.items
                    .get(i)
                    .map(|item| lua.create_string(item))
                    .transpose()
            },
            |_, o: &mut TestObject, i: usize, value: mlua::LuaString| {
                o.items[i] = value.as_bytes().to_vec();
                Ok(())
            },
        )
        .default_array_property(
            |lua, o: &mut TestObject, key: mlua::Value| {
                let item = match index(lua, &key) {
                    Some(i) => o.items.get(i),
                    None => o.named.get(&key_bytes(&key)),
                };
                item.map(|item| lua.create_string(item)).transpose()
            },
            |lua, o: &mut TestObject, key: mlua::Value, value: mlua::LuaString| {
                let value = value.as_bytes().to_vec();
                match index(lua, &key) {
                    Some(i) => o.items[i] = value,
                    None => {
                        o.named.insert(key_bytes(&key), value);
                    }
                }
                Ok(())
            },
        )
        .build(runtime.lua())
        .unwrap();
    runtime.lua().globals().set("obj", obj).unwrap();
    let bare = LuaClass::new(Rc::new(RefCell::new(())))
        .method("Ping", |_, _, ()| Ok("pong"))
        .build(runtime.lua())
        .unwrap();
    runtime.lua().globals().set("bare", bare).unwrap();
    (runtime, state)
}

#[test]
fn method_dot_call() {
    let (rt, _) = runtime_with_obj();
    rt.exec("assert(obj.Add(1, 2) == 3)").unwrap();
}

#[test]
fn method_colon_call_strips_self() {
    let (rt, _) = runtime_with_obj();
    rt.exec("assert(obj:Add(1, 2) == 3)").unwrap();
}

#[test]
fn zero_argument_method_dot_and_colon() {
    let (rt, _) = runtime_with_obj();
    rt.exec(
        r#"
        assert(obj.Zero() == 'zero')
        assert(obj:Zero() == 'zero')
        assert(obj.Argc() == 0)
        assert(obj:Argc() == 0)
        assert(obj.Argc(nil) == 1)
        assert(obj:Argc(nil, 2) == 2)
        "#,
    )
    .unwrap();
}

#[test]
fn string_property_round_trip() {
    let (rt, state) = runtime_with_obj();
    state.borrow_mut().name = b"initial".to_vec();
    rt.exec("assert(obj.Name == 'initial'); obj.Name = 'x'; assert(obj.Name == 'x')")
        .unwrap();
    assert_eq!(state.borrow().name, b"x");
}

#[test]
fn integer_property_converts_like_lua_tointeger() {
    let (rt, state) = runtime_with_obj();
    rt.exec(
        r#"
        assert(obj.Count == 0)
        obj.Count = 42;   assert(obj.Count == 42)
        obj.Count = '12'; assert(obj.Count == 12)
        obj.Count = 3.0;  assert(obj.Count == 3)
        obj.Count = 2.5;  assert(obj.Count == 0)
        obj.Count = 'x';  assert(obj.Count == 0)
        obj.Count = -7
        "#,
    )
    .unwrap();
    assert_eq!(state.borrow().count, -7);
    // FMD2 stores into a 32-bit Pascal `Integer`, keeping the low 32 bits.
    rt.exec("obj.Count = 2^32 + 5 // 1; assert(obj.Count == 5)")
        .unwrap();
}

#[test]
fn boolean_property_converts_like_lua_toboolean() {
    let (rt, state) = runtime_with_obj();
    rt.exec(
        r#"
        assert(obj.Enabled == false)
        obj.Enabled = true;  assert(obj.Enabled == true)
        obj.Enabled = nil;   assert(obj.Enabled == false)
        obj.Enabled = 0;     assert(obj.Enabled == true)
        obj.Enabled = false; assert(obj.Enabled == false)
        obj.Enabled = ''
        "#,
    )
    .unwrap();
    assert!(state.borrow().enabled);
}

#[test]
fn unknown_keys_read_nil_and_writes_are_ignored() {
    let (rt, _) = runtime_with_obj();
    rt.exec(
        r#"
        bare.DoesNotExist = 5
        assert(bare.DoesNotExist == nil)
        bare[0] = 'x'
        assert(bare[0] == nil)
        bare.Ping = 5
        assert(bare.Ping() == 'pong')
        "#,
    )
    .unwrap();
}

#[test]
fn keys_are_case_sensitive() {
    let (rt, state) = runtime_with_obj();
    rt.exec(
        r#"
        obj.Name = 'x'
        assert(obj.name == nil)
        assert(obj.NAME == nil)
        assert(obj.add == nil)
        obj.name = 'y'
        "#,
    )
    .unwrap();
    assert_eq!(state.borrow().name, b"x");
}

#[test]
fn writing_a_read_only_property_is_ignored() {
    let (rt, _) = runtime_with_obj();
    rt.exec("obj.Name = 'abc'; obj.Upper = 'zzz'; assert(obj.Upper == 'ABC')")
        .unwrap();
}

#[test]
fn default_array_property_receives_unknown_keys() {
    let (rt, state) = runtime_with_obj();
    rt.exec(
        r#"
        assert(obj[0] == 'first')
        assert(obj[1] == 'second')
        assert(obj[2] == nil)
        obj[1] = 'changed'
        assert(obj[1] == 'changed')
        assert(obj.DoesNotExist == nil)
        obj.Extra = 'routed'
        assert(obj.Extra == 'routed')
        obj.Name = 'still a property'
        assert(obj.Name == 'still a property')
        "#,
    )
    .unwrap();
    let state = state.borrow();
    assert_eq!(state.items[1], b"changed");
    assert_eq!(state.named[&b"Extra"[..]], b"routed");
    assert!(!state.named.contains_key(&b"Name"[..]));
}

#[test]
fn array_property_indexes_through_getter_and_setter() {
    let (rt, state) = runtime_with_obj();
    rt.exec(
        r#"
        assert(obj.Letters[0] == 'first')
        assert(obj.Letters[1] == 'second')
        assert(obj.Letters[2] == nil)
        obj.Letters[1] = 'changed'
        assert(obj.Letters[1] == 'changed')
        obj.Letters = 5
        assert(obj.Letters[0] == 'first')
        local letters = obj.Letters
        assert(letters[1] == 'changed')
        "#,
    )
    .unwrap();
    assert_eq!(state.borrow().items[1], b"changed");
}

#[test]
fn sub_object_is_exposed_as_a_property() {
    let (rt, _) = runtime_with_obj();
    rt.exec(
        r#"
        assert(obj.Storage.Ping() == 'storage pong')
        assert(obj.Storage:Ping() == 'storage pong')
        assert(obj.Storage == obj.Storage)
        local storage = obj.Storage
        obj.Storage = 5
        assert(obj.Storage == storage)
        "#,
    )
    .unwrap();
}

#[test]
fn every_object_has_self() {
    let (rt, _) = runtime_with_obj();
    rt.exec(
        r#"
        assert(obj.self() == obj)
        assert(obj:self() == obj)
        assert(bare.self() == bare)
        assert(obj.Storage.self() == obj.Storage)
        "#,
    )
    .unwrap();
}

#[test]
fn nul_bytes_survive_method_arguments_and_results() {
    let (rt, state) = runtime_with_obj();
    rt.exec(
        r#"
        assert(#obj.Bytes('a\0b') == 3)
        assert(obj:Bytes('a\0b\0') == 'a\0b\0')
        "#,
    )
    .unwrap();
    assert_eq!(state.borrow().last_bytes, b"a\0b\0");
}

#[test]
fn nul_bytes_survive_string_properties() {
    let (rt, state) = runtime_with_obj();
    rt.exec(r"obj.Name = 'x\0y'; assert(obj.Name == 'x\0y')")
        .unwrap();
    assert_eq!(state.borrow().name, b"x\0y");
    state.borrow_mut().name = b"\0z".to_vec();
    assert_eq!(
        rt.eval::<mlua::LuaString>("obj.Name")
            .unwrap()
            .as_bytes()
            .to_vec(),
        b"\0z"
    );
}

#[test]
fn number_keys_reach_array_getters_and_setters_as_strings() {
    let rt = Runtime::new().unwrap();
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let type_name = |key: &mlua::Value| key.type_name().to_owned();
    let keys = LuaClass::new(seen.clone())
        .array_property(
            "Arr",
            move |_, _, key: mlua::Value| Ok(type_name(&key)),
            |_, seen: &mut Vec<String>, key: mlua::Value, _: mlua::Value| {
                seen.push(key.type_name().to_owned());
                Ok(())
            },
        )
        .default_array_property(
            move |_, _, key: mlua::Value| Ok(type_name(&key)),
            |_, seen: &mut Vec<String>, key: mlua::Value, _: mlua::Value| {
                seen.push(key.type_name().to_owned());
                Ok(())
            },
        )
        .build(rt.lua())
        .unwrap();
    rt.lua().globals().set("keys", keys).unwrap();
    rt.exec(
        r#"
        assert(keys[0] == 'string')
        assert(keys[1.5] == 'string')
        assert(keys[true] == 'boolean')
        assert(keys.Arr[0] == 'string')
        assert(keys.Arr.__get == keys.Arr)
        keys[0] = 1
        keys.Arr[0] = 1
        keys.Arr.__set = 1
        "#,
    )
    .unwrap();
    assert_eq!(*seen.borrow(), ["string", "string"]);
}
