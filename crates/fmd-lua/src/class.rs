//! The generic binding through which every Host API object is exposed to Lua, reproducing the
//! object semantics of FMD2's `LuaClass` (baseunits/lua/LuaClass.pas).

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{
    AnyUserData, FromLua, FromLuaMulti, Function, IntoLua, IntoLuaMulti, Lua, MetaMethod, Table,
    UserData, Value,
};

/// Key of an array property's indexable table in its member table. FMD2 returns that table
/// from its `__get` lookup instead (baseunits/lua/LuaClass.pas:175).
const ARRAY: &str = "__array";

/// Key of the default array property's getter (baseunits/lua/LuaClass.pas:452).
const DEFAULT_GET: &str = "__defaultget";
/// Key of the default array property's setter (baseunits/lua/LuaClass.pas:454).
const DEFAULT_SET: &str = "__defaultset";

/// Key of a property's getter in its member table (baseunits/lua/LuaClass.pas:379).
const GET: &str = "__get";
/// Key of a property's setter in its member table (baseunits/lua/LuaClass.pas:381).
const SET: &str = "__set";

/// Adds one member to an object's member table once the object exists.
type Member<T> = Box<dyn FnOnce(&Lua, &Rc<RefCell<T>>, &AnyUserData, &Table) -> mlua::Result<()>>;

/// Registry key of the Lua function that binds a method to its object.
const BIND_KEY: &str = "fmd.luaclass.bind";

/// Binds `f` to object `u`, like FMD2's C closures with the userdata as upvalue
/// (baseunits/lua/LuaClass.pas:297). A leading argument that is the object itself is dropped,
/// so `obj:Method(a)` behaves like `obj.Method(a)`, the way `luaClassGetClosure` drops the
/// object for functions called without it as upvalue (baseunits/lua/LuaClass.pas:303-308).
const BIND_SOURCE: &str = r#"
local f, u = ...
return function(...)
  if select('#', ...) > 0 and rawequal((...), u) then
    return f(select(2, ...))
  end
  return f(...)
end
"#;

/// Registry key of the Lua function that makes an object's `self` method.
const SELF_KEY: &str = "fmd.luaclass.self";

/// Makes `obj.self()`, returning the object (baseunits/lua/LuaClass.pas:232). FMD2 returns a
/// light userdata wrapping the same Pascal object; here it is the object itself, so
/// `obj.self() == obj` holds.
const SELF_SOURCE: &str = r#"
local u = ...
return function() return u end
"#;

/// Compiles `source` once per Lua state, caching the function in the registry under `key`.
fn cached_chunk(lua: &Lua, key: &str, source: &str) -> mlua::Result<Function> {
    if let Some(chunk) = lua.named_registry_value::<Option<Function>>(key)? {
        return Ok(chunk);
    }
    let chunk = lua
        .load(source)
        .set_name(format!("={key}"))
        .into_function()?;
    lua.set_named_registry_value(key, &chunk)?;
    Ok(chunk)
}

/// Returns `f` bound to `object` (see [`BIND_SOURCE`]).
fn bind(lua: &Lua, f: Function, object: &AnyUserData) -> mlua::Result<Function> {
    cached_chunk(lua, BIND_KEY, BIND_SOURCE)?.call((f, object))
}

/// Builds one Lua object over shared Rust state `T`.
///
/// FMD2 gives every object its own metatable holding its methods and properties
/// (baseunits/lua/LuaClass.pas:284). Here that per-object table is the userdata's user value.
///
/// Every callback gets `&mut T` and runs with `state` mutably borrowed, so a callback that
/// re-enters the same object from Lua gets a Lua error rather than a panic.
pub struct LuaClass<T> {
    state: Rc<RefCell<T>>,
    members: Vec<Member<T>>,
}

impl<T: 'static> LuaClass<T> {
    /// Starts an object over `state`, which the Rust side keeps to read back what Lua changed.
    pub fn new(state: Rc<RefCell<T>>) -> Self {
        LuaClass {
            state,
            members: Vec::new(),
        }
    }

    /// Adds a method bound to the object, so `obj.Name(...)` works without a self argument
    /// (baseunits/lua/LuaClass.pas:341, :297).
    pub fn method<A, R, F>(mut self, name: &str, f: F) -> Self
    where
        A: FromLuaMulti,
        R: IntoLuaMulti,
        F: Fn(&Lua, &mut T, A) -> mlua::Result<R> + 'static,
    {
        let name = name.to_owned();
        self.members
            .push(Box::new(move |lua, state, object, table| {
                let state = state.clone();
                let func =
                    lua.create_function(move |lua, args: A| f(lua, &mut *borrow(&state)?, args))?;
                table.raw_set(name, bind(lua, func, object)?)
            }));
        self
    }

    /// Adds a property read and written through `get` and `set`
    /// (baseunits/lua/LuaClass.pas:365).
    pub fn property<R, V, G, S>(self, name: &str, get: G, set: S) -> Self
    where
        R: IntoLua,
        V: FromLua,
        G: Fn(&Lua, &mut T) -> mlua::Result<R> + 'static,
        S: Fn(&Lua, &mut T, V) -> mlua::Result<()> + 'static,
    {
        self.accessor(name, get, Some(set))
    }

    /// Adds a property with only a getter; assigning it is silently ignored, as FMD2 registers
    /// no `__set` for it (baseunits/lua/LuaClass.pas:380, :147).
    pub fn read_only_property<R, G>(self, name: &str, get: G) -> Self
    where
        R: IntoLua,
        G: Fn(&Lua, &mut T) -> mlua::Result<R> + 'static,
    {
        self.accessor::<R, Value, G, fn(&Lua, &mut T, Value) -> mlua::Result<()>>(name, get, None)
    }

    /// Stores a property as a table holding its `__get` and `__set` functions, the layout
    /// `__index`/`__newindex` look up (baseunits/lua/LuaClass.pas:365-384).
    fn accessor<R, V, G, S>(mut self, name: &str, get: G, set: Option<S>) -> Self
    where
        R: IntoLua,
        V: FromLua,
        G: Fn(&Lua, &mut T) -> mlua::Result<R> + 'static,
        S: Fn(&Lua, &mut T, V) -> mlua::Result<()> + 'static,
    {
        let name = name.to_owned();
        self.members.push(Box::new(move |lua, state, _, table| {
            let property = lua.create_table()?;
            let getter = state.clone();
            property.raw_set(
                GET,
                lua.create_function(move |lua, ()| get(lua, &mut *borrow(&getter)?))?,
            )?;
            if let Some(set) = set {
                let setter = state.clone();
                property.raw_set(
                    SET,
                    lua.create_function(move |lua, value: V| {
                        set(lua, &mut *borrow(&setter)?, value)
                    })?,
                )?;
            }
            table.raw_set(name, property)
        }));
        self
    }

    /// Adds a string property backed by the byte field `field` returns; strings are kept
    /// binary-safe (baseunits/lua/LuaClass.pas:464-474, :520).
    ///
    /// Like `luaToString` (baseunits/lua/LuaUtils.pas:206), assigning a number stores its
    /// string form and assigning any other non-string stores an empty string.
    /// FMD2 truncates at the first NUL there; this does not, so binary data survives.
    pub fn string_property<F>(self, name: &str, field: F) -> Self
    where
        F: Fn(&mut T) -> &mut Vec<u8> + Clone + 'static,
    {
        let get_field = field.clone();
        self.property(
            name,
            move |lua, state| lua.create_string(&*get_field(state)),
            move |lua, state, value: Value| {
                *field(state) = to_bytes(lua, value)?;
                Ok(())
            },
        )
    }

    /// Adds an integer property backed by the 32-bit field `field` returns
    /// (baseunits/lua/LuaClass.pas:476-486, :526).
    ///
    /// Assignment converts like `lua_tointeger`: integral floats and numeric strings convert,
    /// anything else stores 0. Like the Pascal `Integer` it reproduces, only the low 32 bits
    /// are kept.
    pub fn integer_property<F>(self, name: &str, field: F) -> Self
    where
        F: Fn(&mut T) -> &mut i32 + Clone + 'static,
    {
        let get_field = field.clone();
        self.property(
            name,
            move |_, state| Ok(*get_field(state)),
            move |lua, state, value: Value| {
                // Keeping only the low 32 bits is the behaviour being reproduced.
                let value = lua.coerce_integer(value)?.unwrap_or(0) as i32;
                *field(state) = value;
                Ok(())
            },
        )
    }

    /// Adds a boolean property backed by the field `field` returns
    /// (baseunits/lua/LuaClass.pas:488-498, :532).
    ///
    /// Assignment converts like `lua_toboolean`: only `nil` and `false` store false.
    pub fn boolean_property<F>(self, name: &str, field: F) -> Self
    where
        F: Fn(&mut T) -> &mut bool + Clone + 'static,
    {
        let get_field = field.clone();
        self.property(
            name,
            move |_, state| Ok(*get_field(state)),
            move |_, state, value: Value| {
                *field(state) = !matches!(value, Value::Nil | Value::Boolean(false));
                Ok(())
            },
        )
    }

    /// Exposes `object` (typically another `LuaClass` object) as the member `name`, e.g.
    /// `MODULE.Storage`; assigning to `obj.Name` is silently ignored
    /// (baseunits/lua/LuaClass.pas:538-548).
    pub fn object(mut self, name: &str, object: AnyUserData) -> Self {
        let name = name.to_owned();
        self.members
            .push(Box::new(move |_, _, _, table| table.raw_set(name, object)));
        self
    }

    /// Adds an array property: `obj.Name` yields a table whose `[key]` reads go to `get` and
    /// writes to `set` (baseunits/lua/LuaClass.pas:399-433, :171, :187). Assigning to
    /// `obj.Name` itself is silently ignored.
    pub fn array_property<K, R, V, G, S>(mut self, name: &str, get: G, set: S) -> Self
    where
        K: FromLua,
        R: IntoLua,
        V: FromLua,
        G: Fn(&Lua, &mut T, K) -> mlua::Result<R> + 'static,
        S: Fn(&Lua, &mut T, K, V) -> mlua::Result<()> + 'static,
    {
        let name = name.to_owned();
        self.members.push(Box::new(move |lua, state, _, table| {
            let getter = state.clone();
            let setter = state.clone();
            let metatable = lua.create_table()?;
            metatable.raw_set(
                "__index",
                lua.create_function(move |lua, (_, key): (Value, K)| {
                    get(lua, &mut *borrow(&getter)?, key)
                })?,
            )?;
            metatable.raw_set(
                "__newindex",
                lua.create_function(move |lua, (_, key, value): (Value, K, V)| {
                    set(lua, &mut *borrow(&setter)?, key, value)
                })?,
            )?;
            let array = lua.create_table()?;
            array.set_metatable(Some(metatable))?;
            let property = lua.create_table()?;
            property.raw_set(ARRAY, array)?;
            table.raw_set(name, property)
        }));
        self
    }

    /// Adds the default array property: `obj[key]` reads and writes for every key that is not
    /// a member (method, property or sub-object), integer or string alike
    /// (baseunits/lua/LuaClass.pas:448-455, fallthrough at :115 and :154).
    pub fn default_array_property<K, R, V, G, S>(mut self, get: G, set: S) -> Self
    where
        K: FromLua,
        R: IntoLua,
        V: FromLua,
        G: Fn(&Lua, &mut T, K) -> mlua::Result<R> + 'static,
        S: Fn(&Lua, &mut T, K, V) -> mlua::Result<()> + 'static,
    {
        self.members.push(Box::new(move |lua, state, _, table| {
            let getter = state.clone();
            table.raw_set(
                DEFAULT_GET,
                lua.create_function(move |lua, key: K| get(lua, &mut *borrow(&getter)?, key))?,
            )?;
            let setter = state.clone();
            table.raw_set(
                DEFAULT_SET,
                lua.create_function(move |lua, (key, value): (K, V)| {
                    set(lua, &mut *borrow(&setter)?, key, value)
                })?,
            )
        }));
        self
    }

    /// Creates the Lua object.
    pub fn build(self, lua: &Lua) -> crate::Result<AnyUserData> {
        let object = lua.create_userdata(Object)?;
        let table = lua.create_table()?;
        // Registered first, as in FMD2, so a member named `self` replaces it
        // (baseunits/lua/LuaClass.pas:290).
        let self_method: Function = cached_chunk(lua, SELF_KEY, SELF_SOURCE)?.call(&object)?;
        table.raw_set("self", self_method)?;
        for member in self.members {
            member(lua, &self.state, &object, &table)?;
        }
        object.set_user_value(table)?;
        Ok(object)
    }
}

/// Borrows the object's state for a callback; a conflicting borrow becomes a Lua error.
fn borrow<T>(state: &Rc<RefCell<T>>) -> mlua::Result<std::cell::RefMut<'_, T>> {
    state.try_borrow_mut().map_err(mlua::Error::external)
}

/// Converts a Lua value to bytes like `luaToString` (baseunits/lua/LuaUtils.pas:206): strings
/// and numbers convert, anything else becomes empty. Unlike FMD2, NUL bytes are kept.
fn to_bytes(lua: &Lua, value: Value) -> mlua::Result<Vec<u8>> {
    Ok(match lua.coerce_string(value)? {
        Some(s) => s.as_bytes().to_vec(),
        None => Vec::new(),
    })
}

/// The userdata behind every `LuaClass` object; its members live in its user value.
struct Object;

/// Looks `key` up in the object's members. Like `lua_tostring` on the key, numbers match by
/// their string form and other non-string keys match nothing (baseunits/lua/LuaClass.pas:104).
fn member(lua: &Lua, members: &Table, key: &Value) -> mlua::Result<Value> {
    match key {
        Value::String(_) | Value::Integer(_) | Value::Number(_) => {
            match lua.coerce_string(key.clone())? {
                Some(name) => members.raw_get(name),
                None => Ok(Value::Nil),
            }
        }
        _ => Ok(Value::Nil),
    }
}

impl UserData for Object {
    fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
        // `__index` (baseunits/lua/LuaClass.pas:94): a property yields its getter's value, an
        // array property its indexable table, any other member (method, sub-object) yields
        // itself, and an unknown key goes to the default array property, or yields nil when
        // there is none.
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (object, key): (AnyUserData, Value)| {
                let members: Table = object.user_value()?;
                match member(lua, &members, &key)? {
                    Value::Table(property) => match property.raw_get::<Value>(GET)? {
                        Value::Function(get) => get.call(()),
                        _ => property.raw_get(ARRAY),
                    },
                    Value::Nil => match members.raw_get::<Value>(DEFAULT_GET)? {
                        Value::Function(get) => get.call(key),
                        _ => Ok(Value::Nil),
                    },
                    member => Ok(member),
                }
            },
        );
        // `__newindex` (baseunits/lua/LuaClass.pas:131): a property with a setter takes the
        // value, an unknown key goes to the default array property, and everything else
        // (read-only properties, methods, sub-objects, unknown keys without a default array
        // property) is silently ignored.
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (object, key, value): (AnyUserData, Value, Value)| {
                let members: Table = object.user_value()?;
                match member(lua, &members, &key)? {
                    Value::Table(property) => {
                        if let Value::Function(set) = property.raw_get::<Value>(SET)? {
                            set.call::<()>(value)?;
                        }
                    }
                    Value::Nil => {
                        if let Value::Function(set) = members.raw_get::<Value>(DEFAULT_SET)? {
                            set.call::<()>((key, value))?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            },
        );
    }
}
