//! `fmd.imagepuzzle` (baseunits/lua/LuaImagePuzzle.pas:21-106) over `TImagePuzzle`
//! (baseunits/ImagePuzzle.pas:46-306).

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use mlua::{AnyUserData, Lua, Table, Value, Variadic};

use super::{JPEG_QUALITY, build_object, constructors, lib_table, read_stream, write_stream};

/// One puzzle (baseunits/ImagePuzzle.pas:11-26).
struct Puzzle {
    hor_block: i32,
    ver_block: i32,
    /// `property Multiply ... default 1` does not initialise the field (:23), so it starts at 0.
    multiply: i32,
    /// Where source block `i` goes in the output (:37-43: the identity at first).
    matrix: Vec<i32>,
    /// How source block `i` is mirrored: bit 1 left to right, bit 2 top to bottom.
    flips: Vec<i32>,
}

impl Puzzle {
    /// `TImagePuzzle.Create` (baseunits/ImagePuzzle.pas:32-44). A non-positive block count
    /// leaves the matrix empty, which `DeScramble` reports.
    fn new(hor_block: i32, ver_block: i32) -> Puzzle {
        let len = if hor_block > 0 && ver_block > 0 {
            hor_block.saturating_mul(ver_block)
        } else {
            0
        };
        Puzzle {
            hor_block,
            ver_block,
            multiply: 0,
            matrix: (0..len).collect(),
            flips: vec![0; len as usize],
        }
    }

    /// `TImagePuzzle.DeScramble` (baseunits/ImagePuzzle.pas:136-305) on encoded image bytes:
    /// the descrambled image encoded as PNG (for PNG and WebP input) or JPEG (anything else),
    /// or the reason it failed.
    fn descramble(&self, input: &[u8]) -> Result<Vec<u8>, String> {
        if self.matrix.is_empty() {
            return Err("Matrix is not set".into());
        }
        // :183-223: the output keeps PNG for WebP and PNG input, else saves as 'jpg'.
        let format = image::guess_format(input).map_err(|_| "could not decode image")?;
        let source = image::load_from_memory_with_format(input, format)
            .map_err(|_| "could not decode image")?
            .to_rgba8();
        let (width, height) = source.dimensions();
        if width == 0 || height == 0 {
            return Err("could not decode image".into());
        }
        // :230-234: the output starts out filled with 255 in every channel.
        let mut output = RgbaImage::from_pixel(width, height, Rgba([255; 4]));
        let hor = self.hor_block as u32;
        let ver = self.ver_block as u32;
        // :236-245
        let (block_width, block_height) = if self.multiply <= 1 {
            (width / hor, height / ver)
        } else {
            let m = self.multiply as u32;
            (
                width / hor.saturating_mul(m) * m,
                height / ver.saturating_mul(m) * m,
            )
        };
        // :246-291
        for (i, &target) in self.matrix.iter().enumerate() {
            if target < 0 || target as usize >= self.matrix.len() {
                return Err(format!("Matrix[{i}]={target} out of range"));
            }
            let (target, i) = (target as u32, i as u32);
            let dx = (target % hor) * block_width;
            let dy = (target / hor) * block_height;
            let sx = (i % hor) * block_width;
            let sy = (i / hor) * block_height;
            let flags = self.flips.get(i as usize).copied().unwrap_or(0);
            for k in 0..block_height {
                let src_row = if flags & 2 != 0 {
                    sy + block_height - 1 - k
                } else {
                    sy + k
                };
                for x in 0..block_width {
                    let src_col = if flags & 1 != 0 {
                        sx + block_width - 1 - x
                    } else {
                        sx + x
                    };
                    output.put_pixel(dx + x, dy + k, *source.get_pixel(src_col, src_row));
                }
            }
        }
        // :293-296
        let mut out = Cursor::new(Vec::new());
        let written = match format {
            ImageFormat::Png | ImageFormat::WebP => output.write_to(&mut out, ImageFormat::Png),
            _ => DynamicImage::ImageRgba8(output)
                .to_rgb8()
                .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)),
        };
        written.map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    }
}

/// Converts an array property key like `lua_tointeger`, which also parses the string form the
/// key arrives in.
fn index(lua: &Lua, key: Value) -> mlua::Result<i64> {
    Ok(lua.coerce_integer(key)?.unwrap_or(0))
}

/// Reads `items[i]`. FMD2 indexes the Pascal array unchecked; out of range reads 0 here.
fn get(items: &[i32], i: i64) -> i32 {
    usize::try_from(i)
        .ok()
        .and_then(|i| items.get(i))
        .copied()
        .unwrap_or(0)
}

/// Writes `items[i]`; out of range writes are ignored (FMD2 writes unchecked).
fn set(lua: &Lua, items: &mut [i32], i: i64, value: Value) -> mlua::Result<()> {
    let value = lua.coerce_integer(value)?.unwrap_or(0) as i32;
    if let Some(item) = usize::try_from(i).ok().and_then(|i| items.get_mut(i)) {
        *item = value;
    }
    Ok(())
}

/// Builds the Lua object (baseunits/lua/LuaImagePuzzle.pas:93-100).
fn object(lua: &Lua, puzzle: Puzzle) -> mlua::Result<AnyUserData> {
    let class = crate::LuaClass::new(Rc::new(RefCell::new(puzzle)))
        // baseunits/lua/LuaImagePuzzle.pas:28-34: does nothing unless both are objects; on
        // failure, logs and empties the output stream (baseunits/ImagePuzzle.pas:151-156).
        .method(
            "DeScramble",
            |lua, p: &mut Puzzle, (input, output): (Value, Value)| {
                let (Value::UserData(input), Value::UserData(output)) = (input, output) else {
                    return Ok(());
                };
                let data = read_stream(&input)?;
                let result = p.descramble(&data);
                match result {
                    Ok(image) => write_stream(lua, &output, &image),
                    Err(message) => {
                        tracing::error!(target: "fmd.imagepuzzle", "TImagePuzzle.DeScramble: {message}");
                        write_stream(lua, &output, &[])
                    }
                }
            },
        )
        // baseunits/lua/LuaImagePuzzle.pas:60-70, :78-81
        .read_only_property("HorBlock", |_, p: &mut Puzzle| Ok(p.hor_block))
        .read_only_property("VerBlock", |_, p: &mut Puzzle| Ok(p.ver_block))
        // baseunits/lua/LuaImagePuzzle.pas:36-58, :83-86
        .array_property(
            "Matrix",
            |lua, p: &mut Puzzle, key: Value| Ok(get(&p.matrix, index(lua, key)?)),
            |lua, p: &mut Puzzle, key: Value, value: Value| {
                set(lua, &mut p.matrix, index(lua, key)?, value)
            },
        )
        .array_property(
            "Flips",
            |lua, p: &mut Puzzle, key: Value| Ok(get(&p.flips, index(lua, key)?)),
            |lua, p: &mut Puzzle, key: Value, value: Value| {
                set(lua, &mut p.flips, index(lua, key)?, value)
            },
        )
        // baseunits/lua/LuaImagePuzzle.pas:99
        .integer_property("Multiply", |p: &mut Puzzle| &mut p.multiply);
    build_object(lua, class)
}

/// Opens the library (baseunits/lua/LuaImagePuzzle.pas:73-76, :102-106).
pub(super) fn open(lua: &Lua) -> mlua::Result<Table> {
    // baseunits/lua/LuaImagePuzzle.pas:21-26: only exactly two arguments make a puzzle.
    let create = |lua: &Lua, args: Variadic<Value>| {
        if args.len() != 2 {
            return Ok(Value::Nil);
        }
        let hor = lua.coerce_integer(args[0].clone())?.unwrap_or(0) as i32;
        let ver = lua.coerce_integer(args[1].clone())?.unwrap_or(0) as i32;
        Ok(Value::UserData(object(lua, Puzzle::new(hor, ver))?))
    };
    lib_table(lua, constructors(lua, create)?)
}
