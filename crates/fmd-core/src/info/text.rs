//! FMD2's string clean-up helpers that `GetInfoFromURL` runs over a module's info
//! (baseunits/uBaseUnit.pas).

/// `StringFilterChar` (baseunits/uBaseUnit.pas:62-99), including its swapped `&euro;`/`&copy;`.
const STRING_FILTER_CHAR: [(&str, &str); 36] = [
    ("\n", "\\n"),
    ("\r", "\\r"),
    ("&#x27;", "'"),
    ("&#33;", "!"),
    ("&#36;", "$"),
    ("&#37;", "%"),
    ("&#38;", "&"),
    ("&#39;", "'"),
    ("&#033;", "!"),
    ("&#036;", "$"),
    ("&#037;", "%"),
    ("&#038;", "&"),
    ("&#039;", "'"),
    ("&#8211;", "-"),
    ("&gt;", ">"),
    ("&lt;", "<"),
    ("&amp;", "&"),
    ("&ldquo;", "\""),
    ("&rdquo;", "\""),
    ("&quot;", "\""),
    ("&lsquo;", "'"),
    ("&rsquo;", "'"),
    ("&nbsp;", " "),
    ("&cent;", "¢"),
    ("&pound;", "£"),
    ("&yen;", "¥"),
    ("&euro;", "©"),
    ("&copy;", "€"),
    ("&reg;", "®"),
    ("［", "["),
    ("］", "]"),
    ("（", "("),
    ("）", ")"),
    ("&frac12;", "½"),
    ("&deg;", "°"),
    ("&sup2;", "²"),
];

/// `HTMLEntitiesChar` (baseunits/uBaseUnit.pas:101-184).
const HTML_ENTITIES_CHAR: [(&str, &str); 83] = [
    ("&#171;", "«"),
    ("&#176;", "°"),
    ("&Agrave;", "À"),
    ("&#192;", "À"),
    ("&Aacute;", "Á"),
    ("&#193;", "Á"),
    ("&Acirc;", "Â"),
    ("&#194;", "Â"),
    ("&Atilde;", "Ã"),
    ("&ccedil;", "ç"),
    ("&Egrave;", "È"),
    ("&Eacute;", "É"),
    ("&Ecirc;", "Ê"),
    ("&#202;", "Ê"),
    ("&Etilde;", "Ẽ"),
    ("&Igrave;", "Ì"),
    ("&Iacute;", "Í"),
    ("&Itilde;", "Ĩ"),
    ("&ETH;", "Đ"),
    ("&Ograve;", "Ò"),
    ("&Oacute;", "Ó"),
    ("&Ocirc;", "Ô"),
    ("&#212;", "Ô"),
    ("&Otilde;", "Õ"),
    ("&Ugrave;", "Ù"),
    ("&Uacute;", "Ú"),
    ("&Yacute;", "Ý"),
    ("&#221;", "Ý"),
    ("&agrave;", "à"),
    ("&#224;", "à"),
    ("&aacute;", "á"),
    ("&#225;", "á"),
    ("&acirc;", "â"),
    ("&#226;", "â"),
    ("&atilde;", "ã"),
    ("&#227;", "ã"),
    ("&#231;", "ç"),
    ("&egrave;", "è"),
    ("&#232;", "è"),
    ("&eacute;", "é"),
    ("&#233;", "é"),
    ("&etilde;", "ẽ"),
    ("&ecirc;", "ê"),
    ("&#234;", "ê"),
    ("&igrave;", "ì"),
    ("&#236;", "ì"),
    ("&iacute;", "í"),
    ("&#237;", "í"),
    ("&itilde;", "ĩ"),
    ("&#238;", "î"),
    ("&eth;", "đ"),
    ("&ograve;", "ò"),
    ("&#242;", "ò"),
    ("&oacute;", "ó"),
    ("&#243;", "ó"),
    ("&ocirc;", "ô"),
    ("&#244;", "ô"),
    ("&otilde;", "õ"),
    ("&#245;", "õ"),
    ("&ugrave;", "ù"),
    ("&#249;", "ù"),
    ("&uacute;", "ú"),
    ("&#250;", "ú"),
    ("&yacute;", "ý"),
    ("&#253;", "ý"),
    ("&#8217;", "'"),
    ("&#8220;", "\""),
    ("&#8221;", "\""),
    ("&#8230;", "..."),
    ("&Auml;", "Ä"),
    ("&auml;", "ä"),
    ("&Ouml;", "Ö"),
    ("&ouml;", "ö"),
    ("&Uuml;", "Ü"),
    ("&uuml;", "ü"),
    ("&szlig;", "ß"),
    ("&mu;", "μ"),
    ("&#956;", "μ"),
    ("&raquo;", "»"),
    ("&laquo;", "«"),
    ("&#8216;", "‘"),
    ("&ndash;", "-"),
    ("&gamma;", "γ"),
];

/// FPC `Trim`: strips characters up to `' '` at both ends.
pub(super) fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// `StringReplace(s, from, to, [rfReplaceAll, rfIgnoreCase])`, ignoring ASCII case.
fn replace_ignore_case(s: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return s.to_owned();
    }
    let hay = s.to_ascii_lowercase();
    let needle = from.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (i, _) in hay.match_indices(&needle) {
        out.push_str(&s[last..i]);
        out.push_str(to);
        last = i + needle.len();
    }
    out.push_str(&s[last..]);
    out
}

/// The entity minus its final `;`, for the "broken entities" passes; `None` unless it is longer
/// than one character and ends with `;`.
fn broken(entity: &str) -> Option<&str> {
    entity
        .strip_suffix(';')
        .filter(|_| entity.chars().count() > 1)
}

/// `StringFilter` (baseunits/uBaseUnit.pas:2056-2086): line breaks become `\n`/`\r` escapes and
/// common entities are decoded, also when their `;` is missing.
fn string_filter(s: &str) -> String {
    let mut result = s.to_owned();
    if result.is_empty() {
        return result;
    }
    for (from, to) in STRING_FILTER_CHAR {
        if result.to_ascii_lowercase().contains(from) {
            result = replace_ignore_case(&result, from, to);
        }
    }
    for (from, to) in STRING_FILTER_CHAR {
        if let Some(from) = broken(from)
            && result.to_ascii_lowercase().contains(from)
        {
            result = replace_ignore_case(&result, from, to);
        }
    }
    result
}

/// `HTMLEntitiesFilter` (baseunits/uBaseUnit.pas:2088-2117): accented-letter and punctuation
/// entities are decoded, also when their `;` is missing.
fn html_entities_filter(s: &str) -> String {
    let mut result = s.to_owned();
    if result.is_empty() {
        return result;
    }
    for (from, to) in HTML_ENTITIES_CHAR {
        if result.contains(from) {
            result = replace_ignore_case(&result, from, to);
        }
    }
    for (from, to) in HTML_ENTITIES_CHAR {
        if let Some(from) = broken(from)
            && result.contains(from)
        {
            result = replace_ignore_case(&result, from, to);
        }
    }
    result
}

/// `CommonStringFilter` (baseunits/uBaseUnit.pas:2119-2124).
pub(super) fn common_string_filter(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    trim(&html_entities_filter(&string_filter(trim(s)))).to_owned()
}

/// `FixWhiteSpace` (baseunits/uBaseUnit.pas:1913-1926): drops no-break spaces and BOMs.
pub(super) fn fix_white_space(s: &str) -> String {
    s.replace(['\u{a0}', '\u{feff}'], "")
}

/// `RemoveStringBreaks` (baseunits/uBaseUnit.pas:2153-2162): drops line breaks and their `\n`/`\r`
/// escapes.
pub(super) fn remove_string_breaks(s: &str) -> String {
    s.replace(['\n', '\r'], "")
        .replace("\\n", "")
        .replace("\\r", "")
}

/// `TrimRightChar(s, [','])` (baseunits/uBaseUnit.pas:2193-2206).
pub(super) fn trim_right_commas(s: &str) -> &str {
    s.trim_end_matches(',')
}

/// `CleanString` (baseunits/uBaseUnit.pas:1928-1938): line breaks and tabs become spaces, runs of
/// spaces collapse.
pub(super) fn clean_string(s: &str) -> String {
    let mut result = trim(s).replace(['\r', '\n', '\t'], " ");
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }
    trim(&result).to_owned()
}

/// `CleanMultilinedString(s, 1)` (baseunits/uBaseUnit.pas:1940-1962): runs of blank lines
/// collapse to one line break.
pub(super) fn clean_multilined_string(s: &str) -> String {
    let mut result = trim(s).to_owned();
    while result.contains("\r\n\r\n") {
        result = result.replace("\r\n\r\n", "\r\n");
    }
    while result.contains("\n\n") {
        result = result.replace("\n\n", "\n");
    }
    result
}

/// `CleanURL` (baseunits/uBaseUnit.pas:1964-1987): drops a leading `:` and `//`, slashes right
/// after the scheme, and doubled slashes elsewhere.
pub(super) fn clean_url(url: &str) -> String {
    let mut result = trim(url);
    if result.is_empty() {
        return String::new();
    }
    result = result.strip_prefix(':').unwrap_or(result);
    result = result.strip_prefix("//").unwrap_or(result);
    let (scheme, rest) = match result.find("://") {
        Some(i) => (&result[..i + 3], result[i + 3..].trim_start_matches('/')),
        None => ("", result),
    };
    // `ReplaceRegExpr('([^:])[\/]{2,}', ..., '$1/')`: a character other than `:` followed by two
    // or more slashes keeps one of them.
    let chars: Vec<char> = rest.chars().collect();
    let mut out = String::with_capacity(rest.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        i += 1;
        if c != ':' && chars.get(i) == Some(&'/') && chars.get(i + 1) == Some(&'/') {
            out.push('/');
            while chars.get(i) == Some(&'/') {
                i += 1;
            }
        }
    }
    format!("{scheme}{out}")
}
