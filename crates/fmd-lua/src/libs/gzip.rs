//! `fmd.gzip` (baseunits/lua/LuaGZip.pas:14-58) over `unzipStream`
//! (baseunits/GZIPUtils.pas:168-284).

use flate2::{Crc, Decompress, FlushDecompress, Status};
use mlua::{Lua, MultiValue, Table, Value};

use super::lib_table;

/// The gzip header flag bits unzipStream acts on (baseunits/GZIPUtils.pas:30-36).
const FHCRC: u8 = 1 << 1;
const FEXTRA: u8 = 1 << 2;
const FNAME: u8 = 1 << 3;
const FCOMMENT: u8 = 1 << 4;

/// What follows the deflate data, to check the inflated bytes against.
enum Trailer {
    /// gzip: CRC32 and size of the original data.
    Gzip { crc: u32, size: u32 },
    /// zlib: Adler-32 of the original data.
    Zlib { adler: u32 },
    /// Raw deflate carries no checksum.
    Raw,
}

/// Reads the input like the `TMemoryStream` reads in unzipStream, where reading past the end
/// raises.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        let bytes = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(bytes)
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }

    fn skip_zero_terminated(&mut self) -> Option<()> {
        while self.u8()? != 0 {}
        Some(())
    }
}

/// Little-endian `u32` at `pos`.
fn u32_at(data: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(pos..pos + 4)?.try_into().ok()?))
}

/// Splits `data` into its deflate stream and trailer, sniffing the format like unzipStream
/// (baseunits/GZIPUtils.pas:179-244). The checks are loose bit masks, reproduced as they are:
/// any first byte with the bits of `0x78` set counts as a zlib header, so a raw stream
/// starting with such a byte is misread as zlib, as in FMD2.
///
/// `None` stands for the stream read errors FMD2 raises on truncated input.
fn split(data: &[u8]) -> Option<(&[u8], Trailer)> {
    let mut reader = Reader { data, pos: 0 };
    let header = reader.u32()?;
    if header & 0x0008_8B1F == 0x0008_8B1F {
        // :182-215
        reader.u32()?; // modification time
        reader.u16()?; // extra flags and operating system
        let flags = (header >> 24) as u8;
        if flags & FEXTRA != 0 {
            let len = reader.u16()?;
            reader.bytes(len.into())?;
        }
        if flags & FNAME != 0 {
            reader.skip_zero_terminated()?;
        }
        if flags & FCOMMENT != 0 {
            reader.skip_zero_terminated()?;
        }
        if flags & FHCRC != 0 {
            // FMD2 computes the header CRC16 but ignores a mismatch (:209-215).
            reader.u16()?;
        }
        let end = data.len().checked_sub(8)?;
        let trailer = Trailer::Gzip {
            crc: u32_at(data, end)?,
            size: u32_at(data, end + 4)?,
        };
        Some((data.get(reader.pos..end)?, trailer))
    } else if header & 0x0000_0078 == 0x0000_0078 {
        // :222-232: a preset dictionary (FDICT) adds 4 bytes to the header.
        let start = if header & 0x0000_2000 != 0 { 6 } else { 2 };
        let end = data.len().checked_sub(4)?;
        let adler = u32::from_be_bytes(data.get(end..)?.try_into().ok()?);
        Some((data.get(start..end)?, Trailer::Zlib { adler }))
    } else {
        Some((data, Trailer::Raw))
    }
}

/// Inflates a raw deflate stream until it ends or fails, keeping what was inflated so far, like
/// the `while inflate(...) = Z_OK` loop whose result only depends on `inflateEnd`
/// (baseunits/GZIPUtils.pas:257-266). A corrupt raw stream therefore yields partial output.
fn inflate_raw(input: &[u8]) -> Vec<u8> {
    // The output grows by the input size rounded up to 256 bytes each round (:248-262).
    let delta = (input.len() + 255) & !255;
    let mut inflater = Decompress::new(false);
    let mut out = Vec::new();
    loop {
        out.reserve(delta.max(256));
        let rest = input
            .get(inflater.total_in() as usize..)
            .unwrap_or_default();
        match inflater.decompress_vec(rest, &mut out, FlushDecompress::None) {
            Ok(Status::Ok) => {}
            // The stream ended, made no progress (input exhausted) or hit a data error.
            Ok(Status::StreamEnd | Status::BufError) | Err(_) => return out,
        }
    }
}

/// Adler-32 of `data`, as zlib's `adler32` computes it.
fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// `unzipStream`: the inflated data, or `None` when it fails (baseunits/GZIPUtils.pas:168-284).
fn unzip(data: &[u8]) -> Option<Vec<u8>> {
    let (deflated, trailer) = split(data)?;
    let out = inflate_raw(deflated);
    let ok = match trailer {
        // :270-274
        Trailer::Gzip { crc, size } => {
            let mut actual = Crc::new();
            actual.update(&out);
            actual.sum() == crc && u32::try_from(out.len()) == Ok(size)
        }
        // :275-280
        Trailer::Zlib { adler } => adler32(&out) == adler,
        Trailer::Raw => true,
    };
    ok.then_some(out)
}

/// Opens the library (baseunits/lua/LuaGZip.pas:46-56).
pub(super) fn open(lua: &Lua) -> mlua::Result<Table> {
    lib_table(
        lua,
        vec![(
            // baseunits/lua/LuaGZip.pas:14-44: returns nothing (and logs) when unzipping fails.
            // Unlike most FMD2 string arguments, the data is read with its length, so NULs stay.
            "Inflate",
            lua.create_function(|lua, data: Value| {
                let data = match lua.coerce_string(data)? {
                    Some(s) => s.as_bytes().to_vec(),
                    None => Vec::new(),
                };
                match unzip(&data) {
                    Some(out) => Ok(MultiValue::from_vec(vec![Value::String(
                        lua.create_string(out)?,
                    )])),
                    None => {
                        tracing::error!(target: "fmd.gzip", "GZip.Inflate() unzipStream failed");
                        Ok(MultiValue::new())
                    }
                }
            })?,
        )],
    )
}
