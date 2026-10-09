//! `OnGetInfo` results as list rows (`GetInfoFromURL`, baseunits/uData.pas:85-208;
//! `AddInfoToData`, :217-227).

use std::collections::HashSet;

use fmd_lua::MangaInfo;
use fmd_store::MangaListing;

/// The row for the title listed as `name` at `link`, first listed on day `jdn`.
///
/// An empty title falls back to the listed name (`FillBaseMangaInfo`,
/// baseunits/uBaseUnit.pas:2865-2875), else `N/A`. HTML entity decoding (`CommonStringFilter`)
/// is left to the modules' XPath queries.
pub(super) fn listing(info: &MangaInfo, name: &str, link: &str, jdn: i64) -> MangaListing {
    let mut title = one_line(&info.title);
    if title.is_empty() {
        title = one_line(name);
    }
    if title.is_empty() {
        title = "N/A".into();
    }
    // `if Link = '' then Link := RemoveHostFromURL(MangaInfo.URL)` (baseunits/uData.pas:111-112).
    let link = match info.link.trim() {
        "" => link.to_owned(),
        own => remove_host(own),
    };
    MangaListing {
        link,
        title,
        alttitles: drop_placeholder(one_line(&info.alt_titles)),
        authors: drop_placeholder(comma_list(&info.authors)),
        artists: drop_placeholder(comma_list(&info.artists)),
        genres: comma_list(&info.genres),
        status: info.status.trim().to_owned(),
        // `StringBreaks` (baseunits/uBaseUnit.pas:2126-2133) on the trimmed summary.
        summary: match info.summary.trim() {
            "-" | ":" => String::new(),
            summary => fix_white_space(summary)
                .replace("\\n", "\n")
                .replace("\\r", "\r"),
        },
        numchapter: chapter_count(&info.chapter_links),
        added_jdn: jdn,
    }
}

/// `FixWhiteSpace` (baseunits/uBaseUnit.pas:1913-1926).
fn fix_white_space(s: &str) -> String {
    s.replace(['\u{a0}', '\u{feff}'], "")
}

/// `Trim(FixWhiteSpace(RemoveStringBreaks(...)))`.
fn one_line(s: &str) -> String {
    fix_white_space(&s.replace(['\r', '\n'], ""))
        .trim()
        .to_owned()
}

/// [`one_line`] plus `TrimRightChar(..., [','])`.
fn comma_list(s: &str) -> String {
    one_line(s).trim_end_matches(',').trim().to_owned()
}

/// baseunits/uData.pas:129-138.
fn drop_placeholder(s: String) -> String {
    if s.starts_with('<') || s == "-" || s == ":" {
        String::new()
    } else {
        s
    }
}

/// `NumChapter` after removing empty and case-insensitively duplicate links
/// (baseunits/uData.pas:142-206).
fn chapter_count(links: &[String]) -> u32 {
    let distinct: HashSet<String> = links
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(str::to_lowercase)
        .collect();
    u32::try_from(distinct.len()).unwrap_or(u32::MAX)
}

/// `RemoveHostFromURL` (baseunits/uBaseUnit.pas:969-972).
pub(super) fn remove_host(url: &str) -> String {
    let (_, path) = fmd_http::split_url_bytes(url.as_bytes());
    String::from_utf8_lossy(&path).into_owned()
}
