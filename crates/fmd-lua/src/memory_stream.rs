//! FMD2's MemoryStream objects (`HTTP.Document`), reproducing
//! baseunits/lua/LuaMemoryStream.pas over FPC 3.2.2's `TMemoryStream`.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, Value};

use crate::LuaClass;
use crate::file::{file_path, read_file, write_file};

/// An FPC `TMemoryStream`: bytes with a read/write position.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryStream {
    data: Vec<u8>,
    position: usize,
}

impl MemoryStream {
    pub fn new() -> Self {
        MemoryStream::default()
    }

    /// All the bytes, regardless of the position.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// May lie past the end.
    pub fn position(&self) -> usize {
        self.position
    }

    pub fn set_position(&mut self, position: usize) {
        self.position = position;
    }

    /// The bytes from the position to the end (`StreamToString`,
    /// baseunits/uBaseUnit.pas:1089-1116). The position is left unchanged.
    pub fn remaining(&self) -> &[u8] {
        self.data.get(self.position..).unwrap_or_default()
    }

    /// Truncates or zero-extends the stream to `size` bytes, moving the position back to the
    /// end when it lay beyond it (`TMemoryStream.SetSize`).
    pub fn set_size(&mut self, size: usize) -> Result<(), std::collections::TryReserveError> {
        self.data
            .try_reserve(size.saturating_sub(self.data.len()))?;
        self.data.resize(size, 0);
        self.position = self.position.min(size);
        Ok(())
    }

    /// Replaces the content with `bytes`, keeping the position unless it now lies past the end
    /// (`TMemoryStream.LoadFromStream`, which `LoadFromFile` uses).
    pub fn load(&mut self, bytes: &[u8]) -> Result<(), std::collections::TryReserveError> {
        self.data.clear();
        self.set_size(bytes.len())?;
        self.data.copy_from_slice(bytes);
        Ok(())
    }

    /// `TMemoryStream.Clear`.
    pub fn clear(&mut self) {
        self.data.clear();
        self.position = 0;
    }

    /// Writes `bytes` at the position and moves past them (`TMemoryStream.Write`). Writing past
    /// the end fills the gap with zeros.
    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let end = self.position.saturating_add(bytes.len());
        if self.data.len() < end {
            self.data.resize(end, 0);
        }
        self.data[self.position..end].copy_from_slice(bytes);
        self.position = end;
    }
}

/// A shared [`MemoryStream`] exposed to Lua as a MemoryStream object, so a Host API object
/// (e.g. HTTP) can own the stream modules read and write.
#[derive(Debug, Clone, Default)]
pub struct LuaMemoryStream {
    stream: Rc<RefCell<MemoryStream>>,
}

impl LuaMemoryStream {
    pub fn new() -> Self {
        LuaMemoryStream::default()
    }

    pub fn stream(&self) -> &Rc<RefCell<MemoryStream>> {
        &self.stream
    }

    /// The stream behind a Lua MemoryStream object, if `object` is one.
    pub fn from_lua(object: &AnyUserData) -> Option<LuaMemoryStream> {
        LuaClass::<MemoryStream>::state(object).map(|stream| LuaMemoryStream { stream })
    }

    /// Creates a Lua MemoryStream object over this stream
    /// (baseunits/lua/LuaMemoryStream.pas:82-90).
    pub fn build(&self, lua: &Lua) -> crate::Result<AnyUserData> {
        LuaClass::new(self.stream.clone())
            // baseunits/lua/LuaMemoryStream.pas:21-25, :69-70
            .method("ToString", to_string)
            .method("ReadString", to_string)
            // baseunits/lua/LuaMemoryStream.pas:27-35, :71
            .method(
                "WriteString",
                |lua, stream: &mut MemoryStream, data: Value| {
                    // `lua_tolstring` writes nothing for a value that is not a string or number.
                    if let Some(data) = lua.coerce_string(data)? {
                        stream.write(&data.as_bytes());
                    }
                    Ok(())
                },
            )
            // baseunits/lua/LuaMemoryStream.pas:37-41, :72
            .method(
                "LoadFromFile",
                |lua, stream: &mut MemoryStream, path: Value| {
                    stream
                        .load(&read_file(&file_path(lua, path)?)?)
                        .map_err(mlua::Error::external)
                },
            )
            // baseunits/lua/LuaMemoryStream.pas:43-47, :73
            .method(
                "SaveToFile",
                |lua, stream: &mut MemoryStream, path: Value| {
                    write_file(&file_path(lua, path)?, stream.bytes())
                },
            )
            // baseunits/lua/LuaMemoryStream.pas:61-65, :74
            .method("Clear", |_, stream: &mut MemoryStream, ()| {
                stream.clear();
                Ok(())
            })
            // baseunits/lua/LuaMemoryStream.pas:49-59, :78
            .property(
                "Size",
                |_, stream: &mut MemoryStream| Ok(stream.bytes().len()),
                |lua, stream: &mut MemoryStream, size: Value| {
                    // `lua_tointeger`: anything but a number is 0. FPC stores a negative size
                    // as is, leaving a broken stream; it is empty here.
                    let size = lua.coerce_integer(size)?.unwrap_or(0);
                    stream
                        .set_size(usize::try_from(size).unwrap_or(0))
                        .map_err(mlua::Error::external)
                },
            )
            .build(lua)
    }
}

/// `mem_toString` (baseunits/lua/LuaMemoryStream.pas:21-25): the bytes from the position to
/// the end.
fn to_string(lua: &Lua, stream: &mut MemoryStream, (): ()) -> mlua::Result<mlua::LuaString> {
    lua.create_string(stream.remaining())
}
