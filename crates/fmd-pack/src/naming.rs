//! FMD2's `CustomRename` and the string filters it applies.

/// Template tokens (baseunits/uBaseUnit.pas:251-257).
pub const CR_NUMBERING: &str = "%NUMBERING%";
pub const CR_CHAPTER: &str = "%CHAPTER%";
pub const CR_WEBSITE: &str = "%WEBSITE%";
pub const CR_MANGA: &str = "%MANGA%";
pub const CR_AUTHOR: &str = "%AUTHOR%";
pub const CR_ARTIST: &str = "%ARTIST%";
pub const CR_FILENAME: &str = "%FILENAME%";

/// The values substituted into a rename template.
#[derive(Debug, Default, Clone)]
pub struct RenameContext<'a> {
    pub website: &'a str,
    pub manga: &'a str,
    pub author: &'a str,
    pub artist: &'a str,
    /// Non-empty only when renaming a chapter; enables the numbering and padding rules.
    pub chapter: &'a str,
    pub numbering: &'a str,
    pub filename: &'a str,
}

/// How characters that are illegal in file names are handled.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SymbolMode {
    /// Replace `/` and NUL in each value with `_`, and trim trailing dots and spaces from the
    /// final name. Replacing rather than deleting keeps "A/B" readable as "A_B".
    #[default]
    Posix,
    /// Delete FMD2's `Symbols` set `\ / : * ? " < > |`, tab and `;` from each value
    /// (`RemoveSymbols`, baseunits/uBaseUnit.pas:59-60, :1382).
    Windows,
}

#[derive(Debug, Default, Clone)]
pub struct RenameOptions {
    pub symbols: SymbolMode,
    /// When set, every UTF-16 unit outside 31..=127 in a value is replaced by this string
    /// (`ReplaceUnicodeChar`, baseunits/uBaseUnit.pas:817-833).
    pub replace_unicode: Option<String>,
    /// Zero-pad the volume number to this many digits; 0 disables
    /// (`OptionConvertDigitVolumeLength`, baseunits/uBaseUnit.pas:1829-1838).
    pub pad_volume: usize,
    /// Zero-pad the chapter number to this many digits; 0 disables
    /// (`OptionConvertDigitChapterLength`, baseunits/uBaseUnit.pas:1829-1838).
    pub pad_chapter: usize,
}

/// Expands a rename template like FMD2's `CustomRename` (baseunits/uBaseUnit.pas:1798-1859).
///
/// Every substituted value is cleaned first; the template text itself is kept as is.
pub fn custom_rename(template: &str, ctx: &RenameContext, opts: &RenameOptions) -> String {
    let fix = |s: &str| fix_string(s, opts);
    let mut result = template.to_string();

    // Chapter rename only (uBaseUnit.pas:1820-1847).
    if !ctx.chapter.is_empty() {
        if !result.contains(CR_NUMBERING) && !result.contains(CR_CHAPTER) {
            result.insert_str(0, ctx.numbering);
        }
        result = replace_brackets(&result, CR_NUMBERING, ctx.numbering);

        let mut chapter = pas_trim(ctx.chapter).to_string();
        volume_chapter_pad_zero(&mut chapter, opts.pad_volume, opts.pad_chapter);
        let chapter = fix(&chapter);
        result = replace_brackets(&result, CR_CHAPTER, &chapter);
        if result.is_empty() {
            result = ctx.numbering.to_string();
        }
    }

    for (token, value) in [
        (CR_WEBSITE, ctx.website),
        (CR_MANGA, ctx.manga),
        (CR_AUTHOR, ctx.author),
        (CR_ARTIST, ctx.artist),
        (CR_FILENAME, ctx.filename),
    ] {
        result = replace_brackets(&result, token, &fix(value));
    }
    if result.is_empty() {
        result = fix(ctx.manga);
    }
    if result.is_empty() {
        return result;
    }

    // Remove leading and trailing path delimiters (uBaseUnit.pas:1858).
    let mut result = result.trim_matches(['/', '\\']).to_string();
    if opts.symbols == SymbolMode::Posix {
        result.truncate(result.trim_end_matches(['.', ' ']).len());
    }
    result
}

/// `FixStringLocal` (baseunits/uBaseUnit.pas:1804-1813).
fn fix_string(s: &str, opts: &RenameOptions) -> String {
    let result = remove_symbols(&common_string_filter(s), opts.symbols);
    match &opts.replace_unicode {
        Some(replacement) => replace_unicode_char(&result, replacement),
        None => result,
    }
}

/// `CommonStringFilter` (baseunits/uBaseUnit.pas:2119-2124).
fn common_string_filter(s: &str) -> String {
    let filtered = string_filter(pas_trim(s));
    pas_trim(&html_entities_filter(&filtered)).to_string()
}

/// `StringFilter` (baseunits/uBaseUnit.pas:2058-2086): the presence check is against the
/// lowercased text, the replacement ignores ASCII case. A second pass decodes entities
/// that lack their trailing `;`.
fn string_filter(s: &str) -> String {
    entity_filter(s, &STRING_FILTER_CHAR, |result, entity| {
        result.to_ascii_lowercase().contains(entity)
    })
}

/// `HTMLEntitiesFilter` (baseunits/uBaseUnit.pas:2088-2117): like `string_filter`, but the
/// presence check is case-sensitive.
fn html_entities_filter(s: &str) -> String {
    entity_filter(s, &HTML_ENTITIES_CHAR, |result, entity| {
        result.contains(entity)
    })
}

fn entity_filter(s: &str, table: &[(&str, &str)], present: impl Fn(&str, &str) -> bool) -> String {
    let mut result = s.to_string();
    for (entity, value) in table {
        if present(&result, entity) {
            result = replace_ignore_ascii_case(&result, entity, value);
        }
    }
    for (entity, value) in table {
        if entity.len() > 1
            && let Some(broken) = entity.strip_suffix(';')
            && present(&result, broken)
        {
            result = replace_ignore_ascii_case(&result, broken, value);
        }
    }
    result
}

/// `StringReplace(..., [rfIgnoreCase, rfReplaceAll])`.
fn replace_ignore_ascii_case(s: &str, from: &str, to: &str) -> String {
    let haystack = s.to_ascii_lowercase();
    let needle = from.to_ascii_lowercase();
    let mut result = String::with_capacity(s.len());
    let mut last = 0;
    for (i, _) in haystack.match_indices(&needle) {
        result.push_str(&s[last..i]);
        result.push_str(to);
        last = i + needle.len();
    }
    result.push_str(&s[last..]);
    result
}

/// `ReplaceUnicodeChar` (baseunits/uBaseUnit.pas:817-833). Pascal walks UTF-16 units, so a
/// character outside the BMP is replaced twice.
fn replace_unicode_char(s: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        let code = u32::from(c);
        if (31..=127).contains(&code) {
            result.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                result.push_str(replacement);
            }
        }
    }
    result
}

/// `VolumeChapterPadZero` (baseunits/uMisc.pas:119-259), quirks included. Positions are
/// 1-based as in Pascal; `Copy`/`Delete` clamp at the end of the string.
fn volume_chapter_pad_zero(s: &mut String, vol_length: usize, chap_length: usize) {
    let t = s.as_bytes();
    let len = t.len();
    let at = |p: usize| t[p - 1];
    let digit = |p: usize| at(p).is_ascii_digit();

    // Volume number (uMisc.pas:155-191).
    let mut i = 1;
    let mut vol = None;
    if vol_length > 0 {
        let upper = s.to_ascii_uppercase();
        if let Some(vp) = upper.find("VOL").map(|p| p + 1) {
            let mut is_vol = !(vp > 2 && !matches!(at(vp - 1), b',' | b'.' | b'-' | b'_' | b' '))
                && !upper.contains("VOLUME NOT AVAILABLE");
            if is_vol {
                let mut vstart = None;
                let mut vlength = 1;
                // A completed Pascal `for` leaves its variable at the final value.
                i = len;
                for p in vp..=len {
                    match vstart {
                        None if digit(p) => vstart = Some(p),
                        Some(start) if !digit(p) || p == len => {
                            vlength = if p == len { p - start + 1 } else { p - start };
                            i = p;
                            break;
                        }
                        _ => {}
                    }
                }
                match vstart {
                    Some(start) => vol = Some((start, vlength)),
                    None => is_vol = false,
                }
            }
            if !is_vol {
                vol = None;
            }
        }
    }

    // Chapter number (uMisc.pas:193-226).
    let search_chap = |from: usize| -> Option<(usize, usize)> {
        let mut cstart = None;
        let mut clength = 1;
        for j in from..=len {
            match cstart {
                None if digit(j) => cstart = Some(j),
                Some(start) if !digit(j) || j == len => {
                    clength = if j == len { j - start + 2 } else { j - start };
                    break;
                }
                _ => {}
            }
        }
        cstart.map(|start| (start, clength))
    };
    let mut cha = None;
    if chap_length > 0 {
        if i == len {
            i = 1;
        }
        let found = search_chap(i);
        if found.is_none() && i != 1 {
            // The retry result is never used: `cha` stays false (uMisc.pas:213-217).
        } else {
            cha = found;
        }
    }

    let pad = |start: usize, length: usize, width: usize| -> (usize, usize, String) {
        let end = (start - 1 + length).min(len);
        let mut number = s[start - 1..end].to_string();
        while number.len() < width {
            number.insert(0, '0');
        }
        (start - 1, end, number)
    };
    let mut edits = Vec::new();
    match (cha, vol) {
        (Some((cs, cl)), Some((vs, vl))) if vs != cs => {
            edits.push(pad(cs, cl, chap_length));
            edits.push(pad(vs, vl, vol_length));
        }
        (_, Some((vs, vl))) => edits.push(pad(vs, vl, vol_length)),
        (Some((cs, cl)), None) => edits.push(pad(cs, cl, chap_length)),
        (None, None) => {}
    }
    // Apply the later position first so earlier offsets stay valid.
    edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    for (start, end, number) in edits {
        s.replace_range(start..end, &number);
    }
}

fn remove_symbols(s: &str, mode: SymbolMode) -> String {
    match mode {
        SymbolMode::Posix => s.replace(['/', '\0'], "_"),
        SymbolMode::Windows => s
            .chars()
            .filter(|c| {
                !matches!(
                    c,
                    '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\t' | ';'
                )
            })
            .collect(),
    }
}

/// Pascal `Trim`: strips characters up to and including space from both ends.
fn pas_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// `StringReplaceBrackets` (baseunits/uBaseUnit.pas:1772-1796): replaces `pattern` in the
/// trimmed `s`. When the first occurrence is wrapped in `(`/`[`/`{` or `)`/`]`/`}`, those
/// brackets become part of the pattern, so an empty value removes them too.
fn replace_brackets(s: &str, pattern: &str, value: &str) -> String {
    let result = pas_trim(s);
    let mut pattern = pas_trim(pattern).to_string();
    let mut value = pas_trim(value).to_string();
    let Some(i) = result.find(&pattern) else {
        return result.to_string();
    };
    let bytes = result.as_bytes();
    let before = i.checked_sub(1).map(|j| bytes[j]);
    let after = bytes.get(i + pattern.len()).copied();
    let open = before.filter(|b| matches!(b, b'(' | b'[' | b'{'));
    let close = after.filter(|b| matches!(b, b')' | b']' | b'}'));
    if let Some(b) = open {
        pattern.insert(0, b as char);
    }
    if let Some(b) = close {
        pattern.push(b as char);
    }
    if !value.is_empty() {
        if let Some(b) = open {
            value.insert(0, b as char);
        }
        if let Some(b) = close {
            value.push(b as char);
        }
    }
    result.replace(&pattern, &value)
}

/// baseunits/uBaseUnit.pas:62-98.
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

/// baseunits/uBaseUnit.pas:100-184.
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

/// FMD2's limit for an image file path (`FMDMaxImageFilePath`, baseunits/uBaseUnit.pas:244).
/// FMD2 subtracts the working directory's length from it to get the file-name limit on
/// Windows (baseunits/uDownloadsManager.pas:768-770).
pub const MAX_IMAGE_FILE_PATH: usize = 255;

/// The file name (without extension) of page `work_id` (0-based), from the custom file-name
/// template and the module-supplied name (`GetFileName`, baseunits/uDownloadsManager.pas:530-552).
pub fn page_file_name(template: &str, name: Option<&str>, work_id: usize) -> String {
    let name = match name {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => format!("{:03}", work_id + 1),
    };
    template.replace(CR_FILENAME, &name)
}

/// Shortens `name` to at most `max_len` characters by dropping characters from the front
/// (baseunits/uDownloadsManager.pas:544-548).
pub fn fit_file_name(name: &str, max_len: usize) -> String {
    let count = name.chars().count();
    name.chars().skip(count.saturating_sub(max_len)).collect()
}
