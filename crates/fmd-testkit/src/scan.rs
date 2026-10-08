//! Static scan of a Lua source for the Host API names it references.

use std::collections::BTreeSet;

/// Objects FMD2 injects as globals before running a callback
/// (baseunits/lua/LuaWebsiteModules.pas:162-421, `L.LoadObject`; `MODULE` at :822).
const HOST_OBJECTS: &[&str] = &[
    "HTTP",
    "LINKS",
    "MANGAINFO",
    "MODULE",
    "NAMES",
    "TASK",
    "UPDATELIST",
];

/// Global functions and values FMD2 registers in a module's Lua state.
const HOST_GLOBALS: &[&str] = &[
    // baseunits/lua/LuaBase.pas:75-76
    "print",
    "sleep",
    // baseunits/lua/LuaBaseUnit.pas:52-54
    "Trim",
    "MaybeFillHost",
    "MangaInfoStatusIfPos",
    // baseunits/lua/LuaSynaUtil.pas:37-39
    "GetBetween",
    "SeparateLeft",
    "SeparateRight",
    // baseunits/lua/LuaXQuery.pas:198
    "CreateTXQuery",
    // baseunits/lua/LuaWebsiteModules.pas:490
    "NewWebsiteModule",
    // Per-callback values (baseunits/lua/LuaWebsiteModules.pas:205-402)
    "PAGENUMBER",
    "WORKPTR",
    "URL",
    "WORKID",
    "PATH",
    "FILENAME",
    // baseunits/lua/LuaWebsiteModules.pas:827-829
    "no_error",
    "net_problem",
    "information_not_found",
    // baseunits/lua/LuaWebsiteModules.pas:834-837
    "asUnknown",
    "asChecking",
    "asValid",
    "asInvalid",
];

/// Prefix that `require` resolves to a host library (baseunits/lua/LuaPackage.pas:23).
const HOST_LIB_PREFIX: &str = "fmd.";

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Name(&'a str),
    Str(&'a str),
    Punct(char),
}

/// Lists the Host API names a Lua source references: `OBJECT.Member` for members of the
/// injected objects, the bare name for host globals, and `fmd.<lib>` for required host
/// libraries. Names inside comments and strings, fields of other tables and local
/// definitions that shadow a global are not references.
pub fn scan_host_api_names(source: &str) -> BTreeSet<String> {
    let tokens = tokenize(source);
    let mut names = BTreeSet::new();
    for (i, token) in tokens.iter().enumerate() {
        let Token::Name(name) = token else { continue };
        let next = tokens.get(i + 1);
        let prev = i.checked_sub(1).and_then(|p| tokens.get(p));
        if matches!(
            prev,
            Some(Token::Punct('.' | ':') | Token::Name("local" | "function"))
        ) {
            // A field of some other table (`self.HTTP`) or a definition shadowing a global.
            continue;
        }
        if HOST_GLOBALS.contains(name) {
            names.insert((*name).to_string());
        } else if HOST_OBJECTS.contains(name) {
            // Colon calls reach the same method: LuaClass strips the redundant self argument
            // (baseunits/lua/LuaClass.pas:296), so both spellings name one Host API member.
            if let (Some(Token::Punct('.' | ':')), Some(Token::Name(member))) =
                (next, tokens.get(i + 2))
            {
                names.insert(format!("{name}.{member}"));
            }
        } else if *name == "require"
            && let (Some(Token::Str(lib)), _) | (Some(Token::Punct('(')), Some(Token::Str(lib))) =
                (next, tokens.get(i + 2))
            && lib.starts_with(HOST_LIB_PREFIX)
        {
            names.insert((*lib).to_string());
        }
    }
    names
}

fn tokenize(source: &str) -> Vec<Token<'_>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while let Some(&c) = bytes.get(i) {
        if c.is_ascii_whitespace() {
            i += 1;
        } else if bytes[i..].starts_with(b"--") {
            i += 2;
            i = match long_bracket_level(bytes, i) {
                Some(level) => skip_long_bracket(source, i, level).1,
                None => bytes[i..]
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(bytes.len(), |n| i + n),
            };
        } else if let Some(level) = long_bracket_level(bytes, i) {
            let (content, end) = skip_long_bracket(source, i, level);
            tokens.push(Token::Str(content));
            i = end;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            i = skip_while(bytes, i, |b| b.is_ascii_alphanumeric() || b == b'_');
            tokens.push(Token::Name(&source[start..i]));
        } else if c.is_ascii_digit() {
            // Covers hex, fractions and exponents; a trailing `..` stays its own token.
            i = skip_while(bytes, i, |b| b.is_ascii_alphanumeric() || b == b'_');
            while bytes.get(i) == Some(&b'.') && bytes.get(i + 1) != Some(&b'.') {
                i = skip_while(bytes, i + 1, |b| b.is_ascii_alphanumeric());
            }
            if matches!(
                bytes.get(i.wrapping_sub(1)),
                Some(b'e' | b'E' | b'p' | b'P')
            ) && matches!(bytes.get(i), Some(b'+' | b'-'))
            {
                i = skip_while(bytes, i + 1, |b| b.is_ascii_alphanumeric());
            }
        } else if c == b'\'' || c == b'"' {
            let start = i + 1;
            i = start;
            while let Some(&b) = bytes.get(i) {
                if b == c || b == b'\n' {
                    break;
                }
                i += if b == b'\\' { 2 } else { 1 };
            }
            let end = i.min(bytes.len());
            tokens.push(Token::Str(&source[start..end]));
            i = end + 1;
        } else if bytes[i..].starts_with(b"..") || bytes[i..].starts_with(b"::") {
            // Concatenation, varargs and labels: never a field access.
            i = skip_while(bytes, i, |b| b == c);
            tokens.push(Token::Punct(' '));
        } else {
            tokens.push(Token::Punct(char::from(c)));
            i += 1;
        }
    }
    tokens
}

fn skip_while(bytes: &[u8], mut i: usize, pred: impl Fn(u8) -> bool) -> usize {
    while bytes.get(i).is_some_and(|b| pred(*b)) {
        i += 1;
    }
    i
}

/// The level of a long bracket `[==[` opening at `i` (the number of `=`), if one does.
fn long_bracket_level(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes.get(i) != Some(&b'[') {
        return None;
    }
    let after = skip_while(bytes, i + 1, |b| b == b'=');
    (bytes.get(after) == Some(&b'[')).then_some(after - i - 1)
}

/// Skips the long bracket of `level` opening at `i`, returning its content and the index
/// after the closing bracket (the end of the source when it is unterminated).
fn skip_long_bracket(source: &str, i: usize, level: usize) -> (&str, usize) {
    let start = i + level + 2;
    let close = format!("]{}]", "=".repeat(level));
    match source[start..].find(&close) {
        Some(n) => (&source[start..start + n], start + n + close.len()),
        None => (&source[start..], source.len()),
    }
}
