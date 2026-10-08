//! `HTMLEncode` and `HTMLDecode`.

/// `HTMLEncode` is FPC's `EscapeHTML`: `& < > " '` become `&amp; &lt; &gt; &quot; &#39;`
/// (baseunits/lua/LuaCrypto.pas:128-132, FPC packages/fcl-xml/src/htmlelements.pp:146).
pub fn encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// FPC's `Val` for a `LongInt`: optional blanks, sign and `$`/`0x`/`%`/`&` base prefix, then
/// digits; anything malformed or out of range yields 0.
fn fpc_val(s: &[u8]) -> i32 {
    let mut s = s;
    while let [b' ' | b'\t', rest @ ..] = s {
        s = rest;
    }
    let negative = matches!(s.first(), Some(b'-'));
    if let [b'-' | b'+', rest @ ..] = s {
        s = rest;
    }
    let (base, digits) = match s {
        [b'$', rest @ ..] => (16, rest),
        [b'%', rest @ ..] => (2, rest),
        [b'&', rest @ ..] => (8, rest),
        [b'0', b'x' | b'X', rest @ ..] => (16, rest),
        _ => (10, s),
    };
    if digits.is_empty() {
        return 0;
    }
    let mut v: i64 = 0;
    for &d in digits {
        let Some(d) = char::from(d).to_digit(base) else {
            return 0;
        };
        v = v * i64::from(base) + i64::from(d);
        if v > i64::from(i32::MAX) + 1 {
            return 0;
        }
    }
    let v = if negative { -v } else { v };
    i32::try_from(v).unwrap_or(0)
}

/// `HTMLDecode` (baseunits/uBaseUnit.pas:1312-1380): decodes `&amp; &lt; &gt; &nbsp; &quot;` and
/// `&#N;` (N read with `Val`, so hex `&#x41;` is 0, and wrapped to a byte by `Chr`), stopping
/// at the first NUL byte as the Pascal's `PChar` walk does.
///
/// Where the Pascal leaves output bytes uninitialised, this keeps the input instead: an `&`
/// followed by `a`, `l`, `g`, `n` or `q` that does not spell a known entity is copied through,
/// and an unknown entity (the Pascal's early `Exit`) copies the rest of the input verbatim.
pub fn decode(s: &[u8]) -> Vec<u8> {
    let s = s.split(|&c| c == 0).next().unwrap_or_default();
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'&' {
            out.push(s[i]);
            i += 1;
            continue;
        }
        let rest = &s[i + 1..];
        let named: Option<(&[u8], u8)> = match rest.first() {
            Some(b'a') => Some((b"amp;", b'&')),
            Some(b'l') => Some((b"lt;", b'<')),
            Some(b'g') => Some((b"gt;", b'>')),
            Some(b'n') => Some((b"nbsp;", b' ')),
            Some(b'q') => Some((b"quot;", b'"')),
            _ => None,
        };
        match (named, rest.first()) {
            (Some((entity, ch)), _) => {
                if rest.starts_with(entity) {
                    out.push(ch);
                    i += 1 + entity.len();
                } else {
                    out.extend_from_slice(&s[i..i + 2]);
                    i += 2;
                }
            }
            (None, Some(b'#')) => {
                let num = &rest[1..];
                let end = num.iter().position(|&c| c == b';').unwrap_or(num.len());
                out.push(fpc_val(&num[..end]) as u8);
                i += 2 + end + 1;
            }
            _ => {
                out.extend_from_slice(&s[i..]);
                break;
            }
        }
    }
    out
}
