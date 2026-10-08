//! `CreateTXQuery`, the TXQuery object and `IXQValue` (baseunits/lua/LuaXQuery.pas,
//! baseunits/lua/LuaIXQValue.pas) over an [`XPathEngine`].
//!
//! Every argument list is read as a whole, so overloads dispatch on the argument count and
//! Lua types exactly like the Pascal's `lua_gettop` cases. A bound method gets its arguments
//! without the object (crate::class), so the counts match FMD2's dot calls.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fmd_xpath::{Document, XPathEngine, XPathValue};
use mlua::{AnyUserData, Lua, MultiValue, Value, Variadic};

use crate::LuaClass;

/// A TXQuery object: FMD2's `TXQueryEngineHTML` (baseunits/XQueryEngineHTML.pas:14-100).
struct TXQuery {
    engine: Rc<dyn XPathEngine>,
    doc: Box<dyn Document>,
}

impl TXQuery {
    /// `Eval` (baseunits/XQueryEngineHTML.pas:252-284): XPath against the last parsed
    /// document, or against `context` when given; errors yield an empty value.
    fn eval(&self, expr: &str, context: Option<&dyn XPathValue>) -> Box<dyn XPathValue> {
        self.doc.eval(expr, context, false)
    }
}

/// Registers the global `CreateTXQuery` (baseunits/lua/LuaXQuery.pas:196-199) over `engine`.
pub fn register(lua: &Lua, engine: Rc<dyn XPathEngine>) -> crate::Result<()> {
    let create =
        lua.create_function(move |lua, args: Variadic<Value>| create_txquery(lua, &engine, &args))?;
    lua.globals().set("CreateTXQuery", create)?;
    Ok(())
}

/// `CreateTXQuery([html | stream])` (baseunits/lua/LuaXQuery.pas:22-43). With one argument, a
/// string (or number) is parsed and a userdata is read as a memory stream; one argument of any
/// other type returns nothing. Any other argument count gives an empty document.
fn create_txquery(
    lua: &Lua,
    engine: &Rc<dyn XPathEngine>,
    args: &[Value],
) -> mlua::Result<MultiValue> {
    let html = match args {
        [arg] => match html_arg(lua, arg)? {
            Some(html) => html,
            None => return Ok(MultiValue::new()),
        },
        _ => Vec::new(),
    };
    let doc = engine.parse(&html).map_err(mlua::Error::external)?;
    let state = Rc::new(RefCell::new(TXQuery {
        engine: engine.clone(),
        doc,
    }));
    let object = LuaClass::new(state)
        .method(
            "ParseHTML",
            |lua, q: &mut TXQuery, args: Variadic<Value>| parse_html(lua, q, &args),
        )
        .method("XPath", |lua, q: &mut TXQuery, args: Variadic<Value>| {
            push_value(lua, eval_args(lua, q, &args)?)
        })
        .method(
            "XPathString",
            |lua, q: &mut TXQuery, args: Variadic<Value>| xpath_string(lua, q, &args),
        )
        .method(
            "XPathStringAll",
            |lua, q: &mut TXQuery, args: Variadic<Value>| xpath_string_all(lua, q, &args),
        )
        .method(
            "XPathHREFAll",
            |lua, q: &mut TXQuery, args: Variadic<Value>| xpath_href_all(lua, q, &args),
        )
        .method(
            "XPathHREFTitleAll",
            |lua, q: &mut TXQuery, args: Variadic<Value>| xpath_href_title_all(lua, q, &args),
        )
        .method(
            "XPathCount",
            |lua, q: &mut TXQuery, args: Variadic<Value>| xpath_count(lua, q, &args),
        )
        .build(lua)
        .map_err(mlua::Error::external)?;
    Ok(MultiValue::from_iter([Value::UserData(object)]))
}

/// `ParseHTML(html | stream)` (baseunits/lua/LuaXQuery.pas:45-56): parses a string (or
/// number) or a memory stream's content as the new document; empty HTML keeps the current
/// one (baseunits/XQueryEngineHTML.pas:417-421), and so does an argument of another type.
/// Values from the previous document stay valid.
fn parse_html(lua: &Lua, q: &mut TXQuery, args: &[Value]) -> mlua::Result<()> {
    let Some(html) = args
        .first()
        .map(|arg| html_arg(lua, arg))
        .transpose()?
        .flatten()
    else {
        return Ok(());
    };
    if !html.is_empty() {
        q.doc = q.engine.parse(&html).map_err(mlua::Error::external)?;
    }
    Ok(())
}

/// Evaluates `expr[, context]`: with exactly two arguments the second is the context
/// `IXQValue`, any other count ignores everything after the expression
/// (baseunits/lua/LuaXQuery.pas:64-67, :78-87, :170-174).
fn eval_args(lua: &Lua, q: &TXQuery, args: &[Value]) -> mlua::Result<Box<dyn XPathValue>> {
    let context = if args.len() == 2 { args.get(1) } else { None };
    eval_in(q, &arg_string(lua, args, 0)?, context)
}

/// `XPathString(expr[, context])` (baseunits/lua/LuaXQuery.pas:72-89): the value's
/// `toString` (baseunits/XQueryEngineHTML.pas:302-307).
fn xpath_string(lua: &Lua, q: &mut TXQuery, args: &[Value]) -> mlua::Result<String> {
    Ok(eval_args(lua, q, args)?.string())
}

/// `XPathStringAll` (baseunits/lua/LuaXQuery.pas:91-135), by argument count:
/// - `(expr)`: the items joined with `', '`;
/// - `(expr, sep)`, `(expr, sep, context)`: joined with `sep` (a string or number);
/// - `(expr, list)`, `(expr, list, context)`: each item added to the TStrings `list`, returning
///   nothing.
///
/// A second argument that is neither, or any other count, returns nothing.
fn xpath_string_all(lua: &Lua, q: &mut TXQuery, args: &[Value]) -> mlua::Result<MultiValue> {
    let expr = arg_string(lua, args, 0)?;
    match args {
        [_] => join_all(lua, q, &expr, ", ", None),
        [_, sep] | [_, sep, _] if is_string(sep) => {
            join_all(lua, q, &expr, &arg_string(lua, args, 1)?, args.get(2))
        }
        [_, list] | [_, list, _] if is_userdata(list) => {
            // The TStrings form trims each item but keeps blank ones
            // (baseunits/XQueryEngineHTML.pas:375-382).
            let list = strings(list)?;
            for item in items(eval_in(q, &expr, args.get(2))?.as_ref()) {
                add(&list, trim(&item.string()))?;
            }
            Ok(MultiValue::new())
        }
        _ => Ok(MultiValue::new()),
    }
}

/// The string forms of `XPathStringAll`: the items' strings, trimmed, blank ones skipped,
/// joined with `separator` (baseunits/XQueryEngineHTML.pas:342-350).
fn join_all(
    lua: &Lua,
    q: &TXQuery,
    expr: &str,
    separator: &str,
    context: Option<&Value>,
) -> mlua::Result<MultiValue> {
    let mut joined = String::new();
    for item in items(eval_in(q, expr, context)?.as_ref()) {
        add_separator_string(&mut joined, &item.string(), separator);
    }
    Ok(MultiValue::from_iter([Value::String(
        lua.create_string(joined)?,
    )]))
}

/// `XPathHREFAll(expr, links, texts[, context])` (baseunits/lua/LuaXQuery.pas:137-149): adds
/// each item's `href` to `links` and its trimmed string to `texts`
/// (baseunits/XQueryEngineHTML.pas:516-526). Other argument counts do nothing.
fn xpath_href_all(lua: &Lua, q: &mut TXQuery, args: &[Value]) -> mlua::Result<()> {
    href_all(lua, q, args, |item| trim(&item.string()).to_owned())
}

/// `XPathHREFTitleAll(expr, links, titles[, context])` (baseunits/lua/LuaXQuery.pas:151-163):
/// adds each item's `href` to `links` and its `title` to `titles`
/// (baseunits/XQueryEngineHTML.pas:534-544). Other argument counts do nothing.
fn xpath_href_title_all(lua: &Lua, q: &mut TXQuery, args: &[Value]) -> mlua::Result<()> {
    href_all(lua, q, args, |item| item.attribute("title"))
}

/// The shared body of `XPathHREFAll` and `XPathHREFTitleAll`: `second` gives what goes into
/// the second list.
fn href_all(
    lua: &Lua,
    q: &TXQuery,
    args: &[Value],
    second: impl Fn(&dyn XPathValue) -> String,
) -> mlua::Result<()> {
    let ([_, links, others] | [_, links, others, _]) = args else {
        return Ok(());
    };
    let expr = arg_string(lua, args, 0)?;
    let (links, others) = (strings(links)?, strings(others)?);
    for item in items(eval_in(q, &expr, args.get(3))?.as_ref()) {
        add(&links, &item.attribute("href"))?;
        add(&others, &second(item.as_ref()))?;
    }
    Ok(())
}

/// Evaluates `expr` against the document, or against `context` when given.
fn eval_in(q: &TXQuery, expr: &str, context: Option<&Value>) -> mlua::Result<Box<dyn XPathValue>> {
    match context {
        Some(context) => {
            let context = self::context(context)?;
            let context = context.borrow();
            Ok(q.eval(expr, Some(context.value.as_ref())))
        }
        None => Ok(q.eval(expr, None)),
    }
}

/// A TStrings argument. FMD2 reads any userdata as one (`luaToUserData`,
/// baseunits/lua/LuaUtils.pas:201); here it is any object with an `Add` method.
fn strings(arg: &Value) -> mlua::Result<AnyUserData> {
    match arg {
        Value::UserData(list) => Ok(list.clone()),
        _ => Err(mlua::Error::runtime("bad argument (TStrings expected)")),
    }
}

/// `TStrings.Add`, through the list's own Lua method.
fn add(list: &AnyUserData, s: &str) -> mlua::Result<()> {
    use mlua::ObjectLike;
    list.get::<mlua::Function>("Add")?.call::<MultiValue>(s)?;
    Ok(())
}

/// The items of a value, as `for v in value` enumerates them in Pascal: a single item yields
/// itself, the empty sequence nothing.
fn items(value: &dyn XPathValue) -> impl Iterator<Item = Box<dyn XPathValue>> + '_ {
    (1..=value.count()).map(|i| value.get(i))
}

/// `AddSeparatorString` (baseunits/XQueryEngineHTML.pas:130-137): appends the trimmed `s`
/// unless it is blank, trimming what is there already.
fn add_separator_string(dest: &mut String, s: &str, separator: &str) {
    let s = trim(s);
    if s.is_empty() {
        return;
    }
    *dest = if trim(dest).is_empty() {
        s.to_owned()
    } else {
        format!("{}{separator}{s}", trim(dest))
    };
}

/// Pascal's `Trim`: strips every character up to `' '`, control characters included.
fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// `XPathCount(expr[, context])` (baseunits/lua/LuaXQuery.pas:165-176): the number of items,
/// as a Pascal `Integer` (baseunits/XQueryEngineHTML.pas:325-330).
fn xpath_count(lua: &Lua, q: &mut TXQuery, args: &[Value]) -> mlua::Result<i32> {
    // `Integer` keeps the low 32 bits.
    Ok(eval_args(lua, q, args)?.count() as i32)
}

/// An `IXQValue` object (`TLuaIXQValue`, baseunits/lua/LuaIXQValue.pas:13-18).
struct XQValue {
    value: Rc<dyn XPathValue>,
    /// The position every `Get()` iterator of this value shares (`TLuaIXQValue.Current`,
    /// baseunits/lua/LuaIXQValue.pas:15), so a second loop continues where the first stopped.
    current: Rc<Cell<u32>>,
}

/// Pushes an `IXQValue` object (`luaIXQValuePush`, baseunits/lua/LuaIXQValue.pas:158-161)
/// with its methods and `Count` property (:134-156).
fn push_value(lua: &Lua, value: Box<dyn XPathValue>) -> mlua::Result<AnyUserData> {
    let state = XQValue {
        value: Rc::from(value),
        current: Rc::new(Cell::new(0)),
    };
    LuaClass::new(Rc::new(RefCell::new(state)))
        .method("Get", |lua, v: &mut XQValue, args: Variadic<Value>| {
            value_get(lua, v, &args)
        })
        // `GetAttribute(name)` (baseunits/lua/LuaIXQValue.pas:43-48); FMD2 crashes on a
        // non-node, here it is empty.
        .method(
            "GetAttribute",
            |lua, v: &mut XQValue, args: Variadic<Value>| {
                Ok(v.value.attribute(&arg_string(lua, &args, 0)?))
            },
        )
        // `GetProperty(name)` (baseunits/lua/LuaIXQValue.pas:50-54).
        .method(
            "GetProperty",
            |lua, v: &mut XQValue, args: Variadic<Value>| {
                push_value(lua, v.value.property(&arg_string(lua, &args, 0)?))
            },
        )
        // `InnerHTML()` (baseunits/lua/LuaIXQValue.pas:56-60).
        .method("InnerHTML", |_, v: &mut XQValue, ()| {
            Ok(v.value.inner_html())
        })
        // `OuterHTML()` (baseunits/lua/LuaIXQValue.pas:62-66).
        .method("OuterHTML", |_, v: &mut XQValue, ()| {
            Ok(v.value.outer_html())
        })
        // `InnerText()` (baseunits/lua/LuaIXQValue.pas:68-72).
        .method("InnerText", |_, v: &mut XQValue, ()| {
            Ok(v.value.inner_text())
        })
        // `ToString()` (baseunits/lua/LuaIXQValue.pas:37-41).
        .method("ToString", |_, v: &mut XQValue, ()| Ok(v.value.string()))
        // `Count` (baseunits/lua/LuaIXQValue.pas:74-78, :146-149).
        .read_only_property("Count", |_, v: &mut XQValue| Ok(v.value.count()))
        .build(lua)
        .map_err(mlua::Error::external)
}

/// `Get(i)` / `Get()` (baseunits/lua/LuaIXQValue.pas:110-132).
///
/// With any argument: the item at `lua_tointeger` of the first one, 1-based (a non-integer is
/// 0, an empty value). Without: an iterator over the items, one that yields nothing when the
/// value is empty (:117-121). FMD2 also returns the iterator's registry reference as a second
/// value (:124-127); that is an implementation detail no `for` loop sees, so it is left out.
fn value_get(lua: &Lua, v: &mut XQValue, args: &[Value]) -> mlua::Result<MultiValue> {
    let Some(arg) = args.first() else {
        if v.value.count() == 0 {
            let none = lua.create_function(|_, ()| Ok(MultiValue::new()))?;
            return Ok(MultiValue::from_iter([Value::Function(none)]));
        }
        let (value, current) = (v.value.clone(), v.current.clone());
        let next = lua.create_function(move |lua, ()| get_next(lua, value.as_ref(), &current))?;
        return Ok(MultiValue::from_iter([Value::Function(next)]));
    };
    let index = lua.coerce_integer(arg.clone())?.unwrap_or(0);
    let item = push_value(lua, v.value.get(index))?;
    Ok(MultiValue::from_iter([Value::UserData(item)]))
}

/// One step of a `Get()` iterator (`ixqvalue_geti`, baseunits/lua/LuaIXQValue.pas:85-108):
/// advances the value's position, and yields nothing once it passes the last item or reaches
/// an empty one.
fn get_next(lua: &Lua, value: &dyn XPathValue, current: &Cell<u32>) -> mlua::Result<MultiValue> {
    // `Current` is a `Cardinal`; it never gets near wrapping.
    current.set(current.get().saturating_add(1));
    if i64::from(current.get()) > value.count() {
        return Ok(MultiValue::new());
    }
    let item = value.get(i64::from(current.get()));
    if item.is_undefined() {
        return Ok(MultiValue::new());
    }
    Ok(MultiValue::from_iter([Value::UserData(push_value(
        lua, item,
    )?)]))
}

/// The `IXQValue` behind a context argument. FMD2 reads any userdata as one and crashes on
/// anything else (`luaToUserData`, baseunits/lua/LuaUtils.pas:201); here that is a Lua error.
fn context(arg: &Value) -> mlua::Result<Rc<RefCell<XQValue>>> {
    match arg {
        Value::UserData(object) => LuaClass::<XQValue>::state_of(object),
        _ => None,
    }
    .ok_or_else(|| mlua::Error::runtime("bad context argument (IXQValue expected)"))
}

/// The HTML an argument of `CreateTXQuery` or `ParseHTML` holds: a string (or number) through
/// `luaToString`, a userdata as a memory stream, anything else none
/// (baseunits/lua/LuaXQuery.pas:30-34, :51-55).
fn html_arg(lua: &Lua, arg: &Value) -> mlua::Result<Option<Vec<u8>>> {
    if is_string(arg) {
        to_string_bytes(lua, arg).map(Some)
    } else if is_userdata(arg) {
        stream_bytes(arg).map(Some)
    } else {
        Ok(None)
    }
}

/// `lua_isstring`: strings and numbers.
fn is_string(value: &Value) -> bool {
    matches!(
        value,
        Value::String(_) | Value::Integer(_) | Value::Number(_)
    )
}

/// `lua_isuserdata`: full and light userdata.
fn is_userdata(value: &Value) -> bool {
    matches!(value, Value::UserData(_) | Value::LightUserData(_))
}

/// `luaToString` (baseunits/lua/LuaUtils.pas:206-213): strings and numbers convert, anything
/// else is empty, and the result stops at the first NUL (it goes through a `PAnsiChar`).
fn to_string_bytes(lua: &Lua, value: &Value) -> mlua::Result<Vec<u8>> {
    let Some(s) = lua.coerce_string(value.clone())? else {
        return Ok(Vec::new());
    };
    let bytes = s.as_bytes();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Ok(bytes[..end].to_vec())
}

/// `luaToString` of argument `i` (1-based in Pascal, 0-based here); a missing argument is
/// empty, as `lua_tolstring` on an empty slot is NULL.
fn arg_string(lua: &Lua, args: &[Value], i: usize) -> mlua::Result<String> {
    let bytes = match args.get(i) {
        Some(value) => to_string_bytes(lua, value)?,
        None => Vec::new(),
    };
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The whole content of a memory stream argument, like `StreamToString`
/// (baseunits/XQueryEngineHTML.pas:118-128), read through its `ToString` method.
fn stream_bytes(value: &Value) -> mlua::Result<Vec<u8>> {
    use mlua::ObjectLike;
    let Value::UserData(stream) = value else {
        return Err(mlua::Error::runtime(
            "bad stream argument (MemoryStream expected)",
        ));
    };
    let content: mlua::LuaString = stream.get::<mlua::Function>("ToString")?.call(())?;
    Ok(content.as_bytes().to_vec())
}
