//! FMD2's TStrings objects (`MANGAINFO.ChapterLinks`, `HTTP.Headers`, `fmd.strings`, ...),
//! reproducing baseunits/lua/LuaStrings.pas over the FPC 3.2.2 `TStringList` semantics that
//! FMD2 builds with (rtl/objpas/classes/stringl.inc).

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, Value};

use crate::class::{borrow, to_bytes};
use crate::file::{file_path, read_file, write_file};
use crate::{LuaClass, LuaMemoryStream, MemoryStream};

/// An FPC `TStringList`: a list of byte strings. Strings are kept binary-safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringList {
    items: Vec<Vec<u8>>,
    delimiter: u8,
    name_value_separator: u8,
    /// Whether the last load found a UTF-8 BOM, which saving then writes back.
    utf8_bom: bool,
}

impl Default for StringList {
    fn default() -> Self {
        StringList {
            items: Vec::new(),
            delimiter: b',',
            name_value_separator: b'=',
            utf8_bom: false,
        }
    }
}

/// The quote character of `DelimitedText` and `CommaText`; FMD2 never changes FPC's default.
const QUOTE: u8 = b'"';

impl StringList {
    /// An empty list.
    pub fn new() -> Self {
        StringList::default()
    }

    /// Removes every item (`TStringList.Clear`).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The `DelimitedText` separator, `,` by default.
    pub fn delimiter(&self) -> u8 {
        self.delimiter
    }

    /// Sets the `DelimitedText` separator.
    pub fn set_delimiter(&mut self, delimiter: u8) {
        self.delimiter = delimiter;
    }

    /// Replaces the items with the lines of `bytes`, as loaded from a file or stream
    /// (`TStrings.LoadFromStream`). A leading UTF-8 BOM is dropped and remembered, so that
    /// [`StringList::save`] writes it back, as FPC keeps the detected encoding. Other BOMs are
    /// not decoded.
    pub fn load(&mut self, bytes: &[u8]) {
        let text = bytes.strip_prefix(UTF8_BOM);
        self.utf8_bom = text.is_some();
        self.set_text(text.unwrap_or(bytes));
    }

    /// The bytes to save to a file or stream: [`StringList::text`], after a UTF-8 BOM when the
    /// last load found one (`TStrings.SaveToStream`).
    pub fn save(&self) -> Vec<u8> {
        let mut bytes = if self.utf8_bom {
            UTF8_BOM.to_vec()
        } else {
            Vec::new()
        };
        bytes.extend(self.text());
        bytes
    }

    /// Removes the item at `index` (`TStringList.Delete`); out of bounds is FPC's error.
    pub fn delete(&mut self, index: i32) -> Result<(), ListIndexError> {
        match usize::try_from(index) {
            Ok(i) if i < self.items.len() => {
                self.items.remove(i);
                Ok(())
            }
            _ => Err(ListIndexError(index)),
        }
    }

    /// Sorts the items ignoring ASCII case (`TStringList.Sort` with `CaseSensitive` off).
    ///
    /// FPC compares with `AnsiCompareText`, which on Windows is the user locale's collation;
    /// this compares the ASCII-lowercased bytes, which agrees with it for plain ASCII names
    /// except for punctuation that collation weighs differently. Equal items keep their order,
    /// where FPC's quicksort leaves it unspecified.
    pub fn sort(&mut self) {
        self.items.sort_by(|a, b| {
            a.iter()
                .map(u8::to_ascii_lowercase)
                .cmp(b.iter().map(u8::to_ascii_lowercase))
        });
    }

    /// Reverses the order of the items (`strings_reverse`,
    /// baseunits/lua/LuaStrings.pas:182-213).
    pub fn reverse(&mut self) {
        self.items.reverse();
    }

    /// The separator between a name and its value, `=` by default.
    pub fn name_value_separator(&self) -> u8 {
        self.name_value_separator
    }

    /// Sets the separator between a name and its value.
    pub fn set_name_value_separator(&mut self, separator: u8) {
        self.name_value_separator = separator;
    }

    /// The index of the first item equal to `item`, ignoring ASCII case, or -1
    /// (`TStringList.IndexOf` with `CaseSensitive` off).
    pub fn index_of(&self, item: &[u8]) -> i32 {
        found_index(self.items.iter().position(|i| i.eq_ignore_ascii_case(item)))
    }

    /// The index of the first item whose name (the part before the first name/value
    /// separator) equals `name`, ignoring ASCII case, or -1 (`TStrings.IndexOfName`). Items
    /// without a separator have no name.
    pub fn index_of_name(&self, name: &[u8]) -> i32 {
        found_index(self.items.iter().position(|item| {
            self.split_name(item)
                .is_some_and(|(n, _)| n.eq_ignore_ascii_case(name))
        }))
    }

    /// The value of the first item named `name`, or empty when there is none
    /// (`TStrings.GetValue`).
    pub fn value(&self, name: &[u8]) -> &[u8] {
        self.items
            .iter()
            .filter_map(|item| self.split_name(item))
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map_or(&[], |(_, value)| value)
    }

    /// Replaces the first item named `name` with `name=value`, or appends that line when there
    /// is none (`TStrings.SetValue`). FPC 3.2.2 writes `name=` for an empty value instead of
    /// deleting the line as Delphi does.
    pub fn set_value(&mut self, name: &[u8], value: &[u8]) {
        let mut line = name.to_vec();
        line.push(self.name_value_separator);
        line.extend_from_slice(value);
        match usize::try_from(self.index_of_name(name)) {
            Ok(i) => self.items[i] = line,
            Err(_) => self.items.push(line),
        }
    }

    /// Splits `item` at its first name/value separator.
    fn split_name<'a>(&self, item: &'a [u8]) -> Option<(&'a [u8], &'a [u8])> {
        let at = item.iter().position(|&b| b == self.name_value_separator)?;
        Some((&item[..at], &item[at + 1..]))
    }

    /// The items joined by the delimiter (`TStrings.GetDelimitedText`, not strict). An item is
    /// quoted when it holds a control character, space, quote or the delimiter; a single empty
    /// item is written as `""`.
    pub fn delimited_text(&self) -> Vec<u8> {
        join_delimited(&self.items, self.delimiter)
    }

    /// Replaces the items by parsing `text` (`TStrings.SetDelimitedText`, not strict).
    pub fn set_delimited_text(&mut self, text: &[u8]) {
        self.items = split_delimited(text, self.delimiter);
    }

    /// The items joined like [`StringList::delimited_text`] with delimiter `,`
    /// (`TStrings.GetCommaText`).
    pub fn comma_text(&self) -> Vec<u8> {
        join_delimited(&self.items, b',')
    }

    /// Replaces the items by parsing `text` like [`StringList::set_delimited_text`] with
    /// delimiter `,` (`TStrings.SetCommaText`).
    pub fn set_comma_text(&mut self, text: &[u8]) {
        self.items = split_delimited(text, b',');
    }

    /// The items, in order.
    pub fn items(&self) -> &[Vec<u8>] {
        &self.items
    }

    /// The number of items (`TStrings.Count`).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the list has no items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Appends `item` (`TStrings.Add`).
    pub fn add(&mut self, item: impl Into<Vec<u8>>) {
        self.items.push(item.into());
    }

    /// The item at `index`, or FPC's `EStringListError` message when it is out of bounds
    /// (`TStringList.Get`).
    pub fn get(&self, index: i32) -> Result<&[u8], ListIndexError> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.items.get(i))
            .map(Vec::as_slice)
            .ok_or(ListIndexError(index))
    }

    /// Replaces the item at `index` (`TStringList.Put`); out of bounds is FPC's error.
    pub fn set(&mut self, index: i32, item: impl Into<Vec<u8>>) -> Result<(), ListIndexError> {
        let slot = usize::try_from(index)
            .ok()
            .and_then(|i| self.items.get_mut(i))
            .ok_or(ListIndexError(index))?;
        *slot = item.into();
        Ok(())
    }

    /// Every item followed by a CRLF line break (`TStrings.GetTextStr`).
    pub fn text(&self) -> Vec<u8> {
        let mut text = Vec::new();
        for item in &self.items {
            text.extend_from_slice(item);
            text.extend_from_slice(LINE_BREAK);
        }
        text
    }

    /// Replaces the items with the lines of `text`, split on CR, LF and CRLF
    /// (`TStrings.SetTextStr`).
    pub fn set_text(&mut self, text: &[u8]) {
        self.items = split_lines(text);
    }

    /// Appends the lines of `text`, split like [`StringList::set_text`] (`TStrings.AddText`).
    pub fn add_text(&mut self, text: &[u8]) {
        self.items.extend(split_lines(text));
    }
}

/// The UTF-8 byte order mark.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// The line break `Text` ends every line with: `sLineBreak` on Windows, the only platform FMD2
/// ships for.
const LINE_BREAK: &[u8] = b"\r\n";

/// Splits `text` into lines like FPC's `GetNextLine`: a line ends at CR, LF or CRLF, and a
/// trailing line break does not start another line.
fn split_lines(text: &[u8]) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut p = 0;
    while p < text.len() {
        let end = text[p..]
            .iter()
            .position(|&b| b == b'\r' || b == b'\n')
            .map_or(text.len(), |i| p + i);
        lines.push(text[p..end].to_vec());
        p = end;
        if text.get(p) == Some(&b'\r') {
            p += 1;
        }
        if text.get(p) == Some(&b'\n') {
            p += 1;
        }
    }
    lines
}

/// `TStrings.GetDelimitedText` with `StrictDelimiter` off.
fn join_delimited(items: &[Vec<u8>], delimiter: u8) -> Vec<u8> {
    let mut text = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            text.push(delimiter);
        }
        if item
            .iter()
            .any(|&b| b <= b' ' || b == QUOTE || b == delimiter)
        {
            text.push(QUOTE);
            for &b in item {
                text.push(b);
                if b == QUOTE {
                    text.push(QUOTE);
                }
            }
            text.push(QUOTE);
        } else {
            text.extend_from_slice(item);
        }
    }
    if text.is_empty() && items.len() == 1 {
        text = vec![QUOTE, QUOTE];
    }
    text
}

/// `TStrings.SetDelimitedText` with `StrictDelimiter` off: items are separated by the
/// delimiter or by runs of control characters and spaces, and may be quoted, with doubled
/// quotes inside standing for one.
fn split_delimited(text: &[u8], delimiter: u8) -> Vec<Vec<u8>> {
    let is_space = |b: u8| b <= b' ';
    let mut items = Vec::new();
    let mut i = 0;
    let mut after_first_item = false;
    while i < text.len() {
        if after_first_item && text[i] == delimiter {
            i += 1;
        }
        while i < text.len() && is_space(text[i]) {
            i += 1;
        }
        if i < text.len() {
            if text[i] == QUOTE {
                // Up to the closing quote, skipping doubled quotes; an unterminated quote runs
                // to the end.
                let mut item = Vec::new();
                let mut j = i + 1;
                while j < text.len() {
                    if text[j] == QUOTE {
                        if text.get(j + 1) == Some(&QUOTE) {
                            item.push(QUOTE);
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    item.push(text[j]);
                    j += 1;
                }
                items.push(item);
                i = j + 1;
            } else {
                let j = text[i..]
                    .iter()
                    .position(|&b| is_space(b) || b == delimiter)
                    .map_or(text.len(), |n| i + n);
                items.push(text[i..j].to_vec());
                i = j;
            }
        } else if after_first_item {
            items.push(Vec::new());
        }
        while i < text.len() && is_space(text[i]) {
            i += 1;
        }
        after_first_item = true;
    }
    items
}

/// An index found by `Iterator::position` as a TStrings index: -1 when there is none.
fn found_index(index: Option<usize>) -> i32 {
    index.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1)
}

/// An out-of-bounds index, with FPC's `EStringListError` message (`SListIndexError`).
///
/// In FMD2 this is a Pascal exception, which unwinds past Lua's `pcall` and fails the whole
/// module call; here it becomes a Lua error, which `pcall` can catch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("List index ({0}) out of bounds")]
pub struct ListIndexError(pub i32);

/// A shareable handle to a [`StringList`] that can be exposed to Lua as a TStrings object, so a
/// Host API object (e.g. MANGAINFO) can own a list and read back what modules put in it.
#[derive(Debug, Clone, Default)]
pub struct LuaStrings {
    list: Rc<RefCell<StringList>>,
}

impl LuaStrings {
    /// A handle to a new, empty list.
    pub fn new() -> Self {
        LuaStrings::default()
    }

    /// The shared list.
    pub fn list(&self) -> &Rc<RefCell<StringList>> {
        &self.list
    }

    /// Creates a Lua TStrings object over this list (baseunits/lua/LuaStrings.pas:257-264).
    pub fn build(&self, lua: &Lua) -> crate::Result<AnyUserData> {
        LuaClass::new(self.list.clone())
            // baseunits/lua/LuaStrings.pas:74-78
            .method("Add", |lua, list: &mut StringList, item: Value| {
                list.add(to_bytes(lua, item)?);
                Ok(())
            })
            // baseunits/lua/LuaStrings.pas:26-30
            .method("LoadFromFile", |lua, list: &mut StringList, path: Value| {
                list.load(&read_file(&file_path(lua, path)?)?);
                Ok(())
            })
            // baseunits/lua/LuaStrings.pas:32-36: reads from the stream's position to its end.
            .method(
                "LoadFromStream",
                |_, list: &mut StringList, stream: Value| {
                    let stream = to_stream(stream)?;
                    let mut stream = borrow(&stream)?;
                    list.load(stream.remaining());
                    let end = stream.bytes().len().max(stream.position());
                    stream.set_position(end);
                    Ok(())
                },
            )
            // baseunits/lua/LuaStrings.pas:38-42
            .method("SaveToFile", |lua, list: &mut StringList, path: Value| {
                write_file(&file_path(lua, path)?, &list.save())
            })
            // baseunits/lua/LuaStrings.pas:44-48: writes at the stream's position.
            .method("SaveToStream", |_, list: &mut StringList, stream: Value| {
                borrow(&to_stream(stream)?)?.write(&list.save());
                Ok(())
            })
            // baseunits/lua/LuaStrings.pas:80-84
            .method("AddText", |lua, list: &mut StringList, text: Value| {
                list.add_text(&to_bytes(lua, text)?);
                Ok(())
            })
            // baseunits/lua/LuaStrings.pas:50-54, :227
            .method("SetText", set_text)
            // baseunits/lua/LuaStrings.pas:56-60, :228
            .method("GetText", |lua, list: &mut StringList, ()| {
                get_text(lua, list)
            })
            // baseunits/lua/LuaStrings.pas:158-162
            .method("Clear", |_, list: &mut StringList, ()| {
                list.clear();
                Ok(())
            })
            // baseunits/lua/LuaStrings.pas:170-174
            .method("IndexOf", |lua, list: &mut StringList, item: Value| {
                Ok(list.index_of(&to_bytes(lua, item)?))
            })
            // baseunits/lua/LuaStrings.pas:176-180
            .method("IndexOfName", |lua, list: &mut StringList, name: Value| {
                Ok(list.index_of_name(&to_bytes(lua, name)?))
            })
            // baseunits/lua/LuaStrings.pas:164-168
            .method("Delete", |lua, list: &mut StringList, index: Value| {
                list.delete(to_index(lua, index)?)
                    .map_err(mlua::Error::external)
            })
            // baseunits/lua/LuaStrings.pas:152-156
            .method("Sort", |_, list: &mut StringList, ()| {
                list.sort();
                Ok(())
            })
            // baseunits/lua/LuaStrings.pas:182-213
            .method("Reverse", |_, list: &mut StringList, ()| {
                list.reverse();
                Ok(())
            })
            // baseunits/lua/LuaStrings.pas:146-150, :233
            .method("GetCount", |_, list: &mut StringList, ()| Ok(list.len()))
            .method("Get", get)
            .method("Set", |lua, list: &mut StringList, (index, item)| {
                set(lua, list, index, item)
            })
            // baseunits/lua/LuaStrings.pas:146-150, :243
            .read_only_property("Count", |_, list: &mut StringList| Ok(list.len()))
            // baseunits/lua/LuaStrings.pas:244
            .property("Text", get_text, set_text)
            // baseunits/lua/LuaStrings.pas:62-72, :245
            .property(
                "CommaText",
                |lua, list: &mut StringList| lua.create_string(list.comma_text()),
                |lua, list: &mut StringList, text: Value| {
                    list.set_comma_text(&to_bytes(lua, text)?);
                    Ok(())
                },
            )
            // baseunits/lua/LuaStrings.pas:98-108, :246
            .property(
                "DelimitedText",
                |lua, list: &mut StringList| lua.create_string(list.delimited_text()),
                |lua, list: &mut StringList, text: Value| {
                    list.set_delimited_text(&to_bytes(lua, text)?);
                    Ok(())
                },
            )
            // baseunits/lua/LuaStrings.pas:110-120, :247
            .property(
                "Delimiter",
                |lua, list: &mut StringList| lua.create_string([list.delimiter]),
                |lua, list: &mut StringList, delimiter: Value| {
                    list.set_delimiter(first_char(lua, delimiter)?);
                    Ok(())
                },
            )
            // baseunits/lua/LuaStrings.pas:122-132, :248
            .property(
                "NameValueSeparator",
                |lua, list: &mut StringList| lua.create_string([list.name_value_separator]),
                |lua, list: &mut StringList, separator: Value| {
                    list.set_name_value_separator(first_char(lua, separator)?);
                    Ok(())
                },
            )
            // baseunits/lua/LuaStrings.pas:252
            .array_property("Strings", get, set)
            // baseunits/lua/LuaStrings.pas:134-144, :253
            .array_property(
                "Values",
                |lua, list: &mut StringList, name: Value| {
                    lua.create_string(list.value(&to_bytes(lua, name)?))
                },
                |lua, list: &mut StringList, name: Value, value: Value| {
                    list.set_value(&to_bytes(lua, name)?, &to_bytes(lua, value)?);
                    Ok(())
                },
            )
            // The default array property is `Get`/`Set` (baseunits/lua/LuaStrings.pas:263).
            .default_array_property(get, set)
            .build(lua)
    }
}

/// `strings_gettext` (baseunits/lua/LuaStrings.pas:56-60).
fn get_text(lua: &Lua, list: &mut StringList) -> mlua::Result<mlua::LuaString> {
    lua.create_string(list.text())
}

/// `strings_settext` (baseunits/lua/LuaStrings.pas:50-54).
fn set_text(lua: &Lua, list: &mut StringList, text: Value) -> mlua::Result<()> {
    list.set_text(&to_bytes(lua, text)?);
    Ok(())
}

/// `strings_get` (baseunits/lua/LuaStrings.pas:86-90): `Strings[lua_tointeger(index)]`. An
/// index that is not a number reads item 0, and an out-of-bounds index raises FPC's
/// `EStringListError` as a Lua error.
fn get(lua: &Lua, list: &mut StringList, index: Value) -> mlua::Result<mlua::LuaString> {
    let item = list
        .get(to_index(lua, index)?)
        .map_err(mlua::Error::external)?;
    lua.create_string(item)
}

/// `strings_set` (baseunits/lua/LuaStrings.pas:92-96), indexed like [`get`].
fn set(lua: &Lua, list: &mut StringList, index: Value, item: Value) -> mlua::Result<()> {
    let index = to_index(lua, index)?;
    list.set(index, to_bytes(lua, item)?)
        .map_err(mlua::Error::external)
}

/// Converts an index argument like `lua_tointeger`: non-numbers become 0. The Pascal `Integer`
/// parameter keeps only the low 32 bits.
fn to_index(lua: &Lua, value: Value) -> mlua::Result<i32> {
    // Keeping only the low 32 bits is the behaviour being reproduced.
    Ok(lua.coerce_integer(value)?.unwrap_or(0) as i32)
}

/// The MemoryStream argument of `LoadFromStream`/`SaveToStream`. FMD2 dereferences whatever
/// it is given as a stream (baseunits/lua/LuaStrings.pas:35, :47), failing the call for a
/// non-object; here anything but a MemoryStream is a Lua error.
fn to_stream(value: Value) -> mlua::Result<Rc<RefCell<MemoryStream>>> {
    match &value {
        Value::UserData(object) => LuaMemoryStream::from_lua(object),
        _ => None,
    }
    .map(|stream| stream.stream().clone())
    .ok_or_else(|| mlua::Error::runtime("expected a MemoryStream"))
}

/// The first character of a string argument, as `String(luaToString(L, 1))[1]` takes it
/// (baseunits/lua/LuaStrings.pas:119, :131). For an empty string that dereferences nil in
/// FMD2, which fails the call; here it is a Lua error.
fn first_char(lua: &Lua, value: Value) -> mlua::Result<u8> {
    to_bytes(lua, value)?.first().copied().ok_or_else(|| {
        mlua::Error::runtime("Access violation: empty string has no first character")
    })
}

/// Registers the `fmd.strings` library, whose `New` and `Create` make a standalone list
/// (baseunits/lua/LuaStrings.pas:20-24, :215-220, :266-273; the `fmd.` prefix is
/// baseunits/lua/LuaPackage.pas:23). It goes in `package.preload` until the package searcher
/// arrives (T06).
pub(crate) fn register(lua: &Lua) -> crate::Result<()> {
    let open = lua.create_function(|lua, ()| {
        let lib = lua.create_table()?;
        let create = lua.create_function(|lua, ()| Ok(LuaStrings::new().build(lua)?))?;
        lib.set("New", create.clone())?;
        lib.set("Create", create)?;
        Ok(lib)
    })?;
    let preload: mlua::Table = lua
        .globals()
        .get::<mlua::Table>("package")?
        .get("preload")?;
    preload.set("fmd.strings", open)?;
    Ok(())
}
