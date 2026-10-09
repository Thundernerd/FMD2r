//! Reading FMD2's storage conventions: SQLite files, `TStrings.Text` fields and `TDateTime`s.

use std::path::Path;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::ImportOptions;
use crate::error::ImportError;

/// Opens an FMD2 database read-only, or `None` when the file does not exist.
pub(crate) fn open_db(path: &Path) -> Result<Option<Connection>, ImportError> {
    if !path.exists() {
        return Ok(None);
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map(Some)
        .map_err(|source| sqlite_error(path, source))
}

pub(crate) fn sqlite_error(path: &Path, source: rusqlite::Error) -> ImportError {
    ImportError::Sqlite {
        path: path.to_path_buf(),
        source,
    }
}

/// The lines of a `TStrings.Text` value, as `TStrings.SetTextStr` splits them: CRLF, LF and CR
/// all end a line, a final line ending does not start another line, and empty lines in between
/// are kept. FMD2 joins its list columns this way (baseunits/uDownloadsManager.pas:1464-1470,
/// read back at :1675-1680).
pub(crate) fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find(['\r', '\n']) {
            Some(end) => {
                out.push(&rest[..end]);
                let skip = if rest[end..].starts_with("\r\n") {
                    2
                } else {
                    1
                };
                rest = &rest[end + skip..];
            }
            None => {
                out.push(rest);
                rest = "";
            }
        }
    }
    out
}

/// A text column, with NULL (and non-text values) as ''.
pub(crate) fn sql_text(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(r) => r.to_string(),
        ValueRef::Null | ValueRef::Blob(_) => String::new(),
    }
}

/// An integer column; NULL or non-numeric text is 0, like `TField.AsInteger` on an empty field.
pub(crate) fn sql_int(value: ValueRef<'_>) -> i64 {
    match value {
        ValueRef::Integer(i) => i,
        ValueRef::Real(r) => r as i64,
        ValueRef::Text(t) => std::str::from_utf8(t)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0),
        ValueRef::Null | ValueRef::Blob(_) => 0,
    }
}

/// A BOOLEAN column, written by FMD2 as '1'/'0' (baseunits/SQLiteData.pas:154-157).
pub(crate) fn sql_bool(value: ValueRef<'_>) -> bool {
    match value {
        ValueRef::Text(t) => {
            let t = String::from_utf8_lossy(t);
            let t = t.trim();
            t.eq_ignore_ascii_case("true") || t.parse::<i64>().is_ok_and(|n| n != 0)
        }
        other => sql_int(other) != 0,
    }
}

/// Milliseconds since 1970-01-01 on FMD2's local clock (see [`wall_clock_to_utc`]) of a DATETIME column, or `None` when it is empty, unreadable or FMD2's zero
/// date (`TDateTime` 0, 1899-12-30).
///
/// FMD2 writes `'YYYY-MM-DD hh:nn:ss.zzz'` (`PrepSQLValue`, baseunits/SQLiteData.pas:159-167);
/// a number is read as a `TDateTime` (days since 1899-12-30).
pub(crate) fn datetime(value: ValueRef<'_>) -> Option<i64> {
    match value {
        ValueRef::Text(t) => parse_iso_datetime(std::str::from_utf8(t).ok()?.trim()),
        ValueRef::Integer(i) => tdatetime_to_ms(i as f64),
        ValueRef::Real(r) => tdatetime_to_ms(r),
        ValueRef::Null | ValueRef::Blob(_) => None,
    }
}

/// Unix milliseconds of a wall-clock time read by [`datetime`] or [`parse_datetime_text`], in
/// the zone FMD2 ran in.
pub(crate) fn wall_clock_to_utc(wall_clock: Option<i64>, opts: &ImportOptions) -> Option<i64> {
    opts.timezone.to_utc(wall_clock?)
}

/// Days from 1899-12-30 (`TDateTime` 0) to 1970-01-01.
const TDATETIME_UNIX_EPOCH: f64 = 25_569.0;

pub(crate) fn tdatetime_to_ms(days: f64) -> Option<i64> {
    if days == 0.0 || !days.is_finite() {
        return None;
    }
    Some(((days - TDATETIME_UNIX_EPOCH) * 86_400_000.0).round() as i64)
}

/// `YYYY-MM-DD[ T]hh:nn[:ss[.zzz]]` or `YYYY-MM-DD`, as milliseconds since 1970-01-01 on the same
/// clock.
pub(crate) fn parse_iso_datetime(s: &str) -> Option<i64> {
    let (date, time) = match s.find([' ', 'T']) {
        Some(i) => (&s[..i], s[i + 1..].trim()),
        None => (s, ""),
    };
    let mut d = date.split('-');
    let (y, mo, day) = (num(d.next()?)?, num(d.next()?)?, num(d.next()?)?);
    if d.next().is_some() {
        return None;
    }
    let ms = if time.is_empty() {
        0
    } else {
        parse_time(time)?
    };
    let days = days_from_civil(y, mo, day)?;
    if days == -25_569 && ms == 0 {
        // FMD2's zero date.
        return None;
    }
    Some(days * 86_400_000 + ms)
}

/// `hh:nn[:ss[.zzz]]` as milliseconds since midnight.
pub(crate) fn parse_time(s: &str) -> Option<i64> {
    let (hms, frac) = match s.split_once(['.', ',']) {
        Some((hms, frac)) => (hms, Some(frac)),
        None => (s, None),
    };
    let mut parts = hms.split(':');
    let h = num(parts.next()?)?;
    let mi = num(parts.next()?)?;
    let sec = parts.next().map(num).unwrap_or(Some(0))?;
    if parts.next().is_some() || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    let milli = match frac {
        Some(f) if !f.is_empty() && f.len() <= 3 => num(f)? * 10_i64.pow(3 - f.len() as u32),
        Some(_) => return None,
        None => 0,
    };
    Some(((h * 60 + mi) * 60 + sec) * 1000 + milli)
}

fn num(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Days from 1970-01-01 to `y-m-d` in the proleptic Gregorian calendar.
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Milliseconds since 1970-01-01 on FMD2's local clock of a `TDateTime` written as text by `DateTimeToStr`, whose date format
/// follows the Windows locale FMD2 ran under: ISO `YYYY-MM-DD`, or `D-M-YYYY`/`D.M.YYYY` and
/// `M/D/YYYY` (swapped when the first field cannot be the month), each optionally followed by a
/// time. A plain number is a `TDateTime`.
pub(crate) fn parse_datetime_text(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(ms) = parse_iso_datetime(s) {
        return Some(ms);
    }
    if let Ok(days) = s.parse::<f64>() {
        return tdatetime_to_ms(days);
    }
    let (date, time) = match s.split_once(' ') {
        Some((date, time)) => (date, time.trim()),
        None => (s, ""),
    };
    let sep = date.chars().find(|c| matches!(c, '/' | '-' | '.'))?;
    let parts: Vec<&str> = date.split(sep).collect();
    let [a, b, c] = parts[..] else {
        return None;
    };
    let (a, b, c, year_last) = (num(a)?, num(b)?, num(c)?, c.len() == 4);
    let (y, m, d) = match (year_last, sep) {
        (false, _) => (a, b, c),
        (true, '/') if a > 12 => (c, b, a),
        (true, '/') => (c, a, b),
        (true, _) if b > 12 => (c, a, b),
        (true, _) => (c, b, a),
    };
    let ms = if time.is_empty() {
        0
    } else {
        parse_time(time)?
    };
    Some(days_from_civil(y, m, d)? * 86_400_000 + ms)
}

/// Parses the FMD2 JSON file at `path`, or `None` when it does not exist.
pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, ImportError> {
    let json_error = |reason: String| ImportError::Json {
        path: path.to_path_buf(),
        reason,
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(json_error(e.to_string())),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| json_error(e.to_string()))
}

/// The non-blank lines of a newline-joined chapter link list.
pub(crate) fn chapter_links(text: &str) -> Vec<&str> {
    lines(text)
        .into_iter()
        .filter(|c| !c.trim().is_empty())
        .collect()
}

/// A JSON value as text: strings as they are, numbers and booleans spelled out, anything else ''.
pub(crate) fn json_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// A JSON boolean as FMD2's JSON readers accept it: `true`/`false`, a number (non-zero is true),
/// or the text `true`/`false`/`1`/`0`.
pub(crate) fn json_bool(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|n| n != 0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// A JSON integer: a number, numeric text, or a boolean as 0/1.
pub(crate) fn json_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}
