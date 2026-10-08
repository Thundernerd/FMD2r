//! Turning what `OnGetInfo` filled in into a list row, as `TMangaInformation.GetInfoFromURL`
//! cleans it up (baseunits/uData.pas:85-208) and `AddInfoToData` stores it
//! (baseunits/uData.pas:217-227).

use std::collections::HashSet;

use fmd_lua::MangaInfo;
use fmd_store::MangaListing;

/// The row stored for the title listed as `name` at `link`, from the info its `OnGetInfo`
/// produced, first listed on day `jdn`.
///
/// This reproduces the clean-up that matters for the list: line breaks, no-break spaces and
/// byte order marks out of the one-line fields, trailing commas off the people and genre lists,
/// placeholder values (`-`, `:`, or HTML) dropped, and an empty title filled from the list
/// (`FillBaseMangaInfo`, baseunits/uBaseUnit.pas:2865-2875: FMD2 pre-fills `MANGAINFO.Title`
/// with the listed name) or else `N/A`. HTML entity decoding (`CommonStringFilter`) is left to
/// the modules, which decode with their XPath queries.
pub(super) fn listing(info: &MangaInfo, name: &str, link: &str, jdn: i64) -> MangaListing {
    let mut title = one_line(&info.title);
    if title.is_empty() {
        title = one_line(name);
    }
    if title.is_empty() {
        title = "N/A".into();
    }
    // `if Link = '' then Link := RemoveHostFromURL(MangaInfo.URL)` (baseunits/uData.pas:111-112);
    // `MANGAINFO.URL` is the listed link with the module's host.
    let link = match info.link.trim() {
        "" => link.to_owned(),
        own => remove_host(own),
    };
    MangaListing {
        link,
        title,
        alttitles: placeholder(one_line(&info.alt_titles)),
        authors: placeholder(list(&info.authors)),
        artists: placeholder(list(&info.artists)),
        genres: list(&info.genres),
        status: info.status.trim().to_owned(),
        // `StringBreaks` (baseunits/uBaseUnit.pas:2126-2133) on the trimmed summary.
        summary: match info.summary.trim() {
            "-" | ":" => String::new(),
            summary => fix_white_space(summary).replace("\\n", "\n"),
        },
        numchapter: chapter_count(&info.chapter_links),
        added_jdn: jdn,
    }
}

/// `FixWhiteSpace` (baseunits/uBaseUnit.pas:1913-1938): no-break spaces and byte order marks
/// become spaces.
fn fix_white_space(s: &str) -> String {
    s.replace(['\u{a0}', '\u{feff}'], " ")
}

/// `Trim(FixWhiteSpace(RemoveStringBreaks(...)))`: line breaks removed.
fn one_line(s: &str) -> String {
    fix_white_space(&s.replace(['\r', '\n'], ""))
        .trim()
        .to_owned()
}

/// [`one_line`] without trailing commas (`TrimRightChar(..., [','])`).
fn list(s: &str) -> String {
    one_line(s).trim_end_matches(',').trim().to_owned()
}

/// Drops the placeholders FMD2 drops: a value starting with `<`, or `-` or `:`
/// (baseunits/uData.pas:129-138).
fn placeholder(s: String) -> String {
    if s.starts_with('<') || s == "-" || s == ":" {
        String::new()
    } else {
        s
    }
}

/// `NumChapter := ChapterLinks.Count` after empty and duplicate (case-insensitive) links are
/// removed (baseunits/uData.pas:142-206).
fn chapter_count(links: &[String]) -> u32 {
    let distinct: HashSet<String> = links
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(str::to_lowercase)
        .collect();
    u32::try_from(distinct.len()).unwrap_or(u32::MAX)
}

/// `RemoveHostFromURL` (baseunits/uBaseUnit.pas:969-972): the path of `url` (`SplitURL`).
pub(super) fn remove_host(url: &str) -> String {
    let (_, path) = fmd_http::split_url_bytes(url.as_bytes());
    String::from_utf8_lossy(&path).into_owned()
}
