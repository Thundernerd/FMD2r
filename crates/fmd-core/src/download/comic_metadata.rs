//! The metadata files written into CBZ archives: ComicInfo.xml and MetronInfo.xml.

use fmd_pack::{AlternativeName, ArchiveEntry, ComicMetadata, comic_info_xml, metron_info_xml};

use super::SeriesMatch;
use crate::settings::ComicMetadataFiles;

/// MangaBaka's data licence asks for it (crates/fmd-core/tests/fixtures/mangabaka/README.md).
const ATTRIBUTION: &str =
    "Series metadata from MangaBaka (https://mangabaka.org), CC BY-NC-SA 4.0.";

/// The files `files` asks for, about chapter `chapter` of the series `title` at `url`, filled
/// from `matched` when MangaBaka has the series.
pub(super) fn entries(
    files: ComicMetadataFiles,
    title: &str,
    chapter: &str,
    url: &str,
    matched: Option<&SeriesMatch>,
) -> Vec<ArchiveEntry> {
    let meta = comic_metadata(title, chapter, url, matched);
    let mut entries = Vec::new();
    if files.comic_info() {
        entries.push(ArchiveEntry {
            name: "ComicInfo.xml".into(),
            bytes: comic_info_xml(&meta).into_bytes(),
        });
    }
    if files.metron_info() {
        entries.push(ArchiveEntry {
            name: "MetronInfo.xml".into(),
            bytes: metron_info_xml(&meta).into_bytes(),
        });
    }
    entries
}

fn comic_metadata(
    title: &str,
    chapter: &str,
    url: &str,
    matched: Option<&SeriesMatch>,
) -> ComicMetadata {
    let minimal = ComicMetadata {
        series: title.to_owned(),
        chapter: chapter.to_owned(),
        url: url.to_owned(),
        ..ComicMetadata::default()
    };
    let Some(SeriesMatch { series, xids }) = matched else {
        return minimal;
    };
    let native = native_language(&series.kind).map(str::to_owned);
    let names = [
        (&series.native_title, native.clone()),
        (&series.romanized_title, native),
        (&series.english_title, Some("en".to_owned())),
    ];
    ComicMetadata {
        series: if series.title.is_empty() {
            minimal.series
        } else {
            series.title.clone()
        },
        alternative_names: names
            .into_iter()
            .filter_map(|(name, lang)| {
                Some(AlternativeName {
                    name: name.clone()?,
                    lang,
                })
            })
            .collect(),
        summary: series.description.clone(),
        year: series.year,
        writers: series.authors.clone(),
        artists: series.artists.clone(),
        genres: series.genres.clone(),
        series_type: series.kind.clone(),
        content_rating: series.content_rating.clone(),
        ids: xids.clone(),
        urls: vec![format!("https://mangabaka.org/{}", series.id)],
        notes: ATTRIBUTION.to_owned(),
        ..minimal
    }
}

/// The language a series of MangaBaka's `type` is first published in.
fn native_language(kind: &str) -> Option<&'static str> {
    match kind {
        "manga" => Some("ja"),
        "manhwa" => Some("ko"),
        "manhua" => Some("zh"),
        _ => None,
    }
}
