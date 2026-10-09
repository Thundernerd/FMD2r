//! `fmd.fileutil` (baseunits/lua/LuaFileUtil.pas:14-46).

use std::cmp::Ordering;

use mlua::{Lua, Table, Value};

use crate::{LuaClass, StringList};

use super::{lib_table, to_string_arg};

/// Characters that end a directory or drive part of a path in FMD2's Windows build: FPC's
/// `AllowDirectorySeparators` (`\` and `/`) plus `AllowDriveSeparators` (`:`).
fn is_separator(b: u8) -> bool {
    matches!(b, b'\\' | b'/' | b':')
}

/// The part of `path` after its last separator, like FPC's `SysUtils.ExtractFileName`
/// (rtl/objpas/sysutils/fina.inc in FPC).
fn file_name(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| is_separator(b)) {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// The file name without its extension, like LazFileUtils' `ExtractFileNameOnly`
/// (components/lazutils/lazfileutils.pas:611-627 in Lazarus): everything from the last `.` on
/// is dropped, so `.hidden` yields an empty name.
fn file_name_only(path: &[u8]) -> &[u8] {
    let name = file_name(path);
    match name.iter().rposition(|&b| b == b'.') {
        Some(i) => &name[..i],
        None => name,
    }
}

/// Like `TStringList.Sort`'s `AnsiCompareText`, but folding ASCII case only rather than by
/// locale. FMD2's QuickSort is unstable, so names equal but for case may order differently.
fn compare_text(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
}

fn sorted(names: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut names = names.to_vec();
    names.sort_by(|a, b| compare_text(a, b));
    names
}

/// Pads the first run of digits in `s` with zeros to `width`, like `PadZero` with `PadAll` and
/// `StripZero` false (baseunits/uBaseUnit.pas:1591-1637). Like the Pascal, a single trailing
/// character after the digits is dropped (`'2a'` becomes `'002'`, :1635).
fn pad_zero(s: &[u8], width: usize) -> Vec<u8> {
    let Some(start) = s.iter().position(u8::is_ascii_digit) else {
        return s.to_vec();
    };
    let end = s[start..]
        .iter()
        .position(|b| !b.is_ascii_digit())
        .map_or(s.len(), |n| start + n);
    let digits = &s[start..end];
    let mut out = s[..start].to_vec();
    out.resize(out.len() + width.saturating_sub(digits.len()), b'0');
    out.extend_from_slice(digits);
    // `i < Length(S)` with `i` 1-based (:1635).
    if end + 1 < s.len() {
        out.extend_from_slice(&s[end..]);
    }
    out
}

/// Renames `names` so that they keep their order when sorted, like
/// `SerializeAndMaintainNames` (baseunits/uBaseUnit.pas:1669-1742): unchanged (`None`) if they
/// already sort, else zero-padded numbers if that sorts, else a `001_` counter prefix.
fn serialize_and_maintain_names(names: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    if names.is_empty() {
        return None;
    }
    if sorted(names) == names {
        // FMD2 replaces the list with its sorted copy, which is the same list (:1735-1738).
        return None;
    }
    let width = names.len().to_string().len().max(3);
    let padded: Vec<Vec<u8>> = names.iter().map(|n| pad_zero(n, width)).collect();
    if sorted(&padded) == padded {
        return Some(padded);
    }
    let numbered: Vec<Vec<u8>> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mut out = format!("{:0width$}_", i + 1).into_bytes();
            out.extend_from_slice(n);
            out
        })
        .collect();
    // The counter is at least as wide as the count, so the numbered names always sort; FMD2
    // still checks (:1728).
    (sorted(&numbered) == numbered).then_some(numbered)
}

/// Opens the library (baseunits/lua/LuaFileUtil.pas:34-45).
pub(super) fn open(lua: &Lua) -> mlua::Result<Table> {
    lib_table(
        lua,
        vec![
            (
                // baseunits/lua/LuaFileUtil.pas:14-18
                "ExtractFileName",
                lua.create_function(|lua, path: Value| {
                    lua.create_string(file_name(&to_string_arg(lua, path)?))
                })?,
            ),
            (
                // baseunits/lua/LuaFileUtil.pas:20-24
                "ExtractFileNameOnly",
                lua.create_function(|lua, path: Value| {
                    lua.create_string(file_name_only(&to_string_arg(lua, path)?))
                })?,
            ),
            (
                // baseunits/lua/LuaFileUtil.pas:26-31: anything but a TStrings object is
                // ignored.
                "SerializeAndMaintainNames",
                lua.create_function(|_, list: Value| {
                    let Value::UserData(list) = list else {
                        return Ok(());
                    };
                    let Some(list) = LuaClass::<StringList>::state(&list) else {
                        return Ok(());
                    };
                    let mut list = list.try_borrow_mut().map_err(mlua::Error::external)?;
                    if let Some(names) = serialize_and_maintain_names(list.items()) {
                        list.clear();
                        for name in names {
                            list.add(name);
                        }
                    }
                    Ok(())
                })?,
            ),
        ],
    )
}
