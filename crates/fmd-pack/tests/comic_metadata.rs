// Test helpers outside `#[test]` fns are not covered by clippy.toml's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fmd_pack::{AlternativeName, ComicMetadata, comic_info_xml, metron_info_xml};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Validates `xml` against the vendored XSD with libxml2's `xmllint`. MetronInfo.xsd's
/// `xs:assert`s are XSD 1.1, which libxml2 cannot compile; they are dropped here, and the
/// tests check what they assert (at most one primary URL and ID) themselves.
fn assert_valid(xml: &str, xsd: &str) {
    let dir = tempfile::tempdir().unwrap();
    let schema = std::fs::read_to_string(fixture(xsd)).unwrap();
    let schema: String = schema
        .lines()
        .filter(|l| !l.trim_start().starts_with("<xs:assert "))
        .map(|l| format!("{l}\n"))
        .collect();
    let schema_path = dir.path().join(xsd);
    std::fs::write(&schema_path, schema).unwrap();
    let doc = dir.path().join("doc.xml");
    std::fs::write(&doc, xml).unwrap();
    let out = Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(&schema_path)
        .arg(&doc)
        .output()
        .expect("xmllint (libxml2) is needed to validate against the XSDs");
    assert!(
        out.status.success(),
        "{xsd} rejects:\n{xml}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn minimal() -> ComicMetadata {
    ComicMetadata {
        series: "Solo Leveling".into(),
        chapter: "Ch. 1 - I'm Used to It".into(),
        url: "https://example.org/manga/solo-leveling".into(),
        ..ComicMetadata::default()
    }
}

#[test]
fn minimal_comic_info_has_series_chapter_and_url_only() {
    let xml = comic_info_xml(&minimal());

    assert_valid(&xml, "ComicInfo.xsd");
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <ComicInfo>\n\
         \x20 <Title>Ch. 1 - I&apos;m Used to It</Title>\n\
         \x20 <Series>Solo Leveling</Series>\n\
         \x20 <Web>https://example.org/manga/solo-leveling</Web>\n\
         </ComicInfo>\n"
    );
}

#[test]
fn minimal_metron_info_has_series_story_and_primary_url_only() {
    let xml = metron_info_xml(&minimal());

    assert_valid(&xml, "MetronInfo.xsd");
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <MetronInfo>\n\
         \x20 <Series>\n\
         \x20   <Name>Solo Leveling</Name>\n\
         \x20 </Series>\n\
         \x20 <Stories>\n\
         \x20   <Story>Ch. 1 - I&apos;m Used to It</Story>\n\
         \x20 </Stories>\n\
         \x20 <URLs>\n\
         \x20   <URL primary=\"true\">https://example.org/manga/solo-leveling</URL>\n\
         \x20 </URLs>\n\
         </MetronInfo>\n"
    );
}

fn full() -> ComicMetadata {
    ComicMetadata {
        alternative_names: vec![
            AlternativeName {
                name: "나 혼자만 레벨업".into(),
                lang: Some("ko".into()),
            },
            AlternativeName {
                name: "Na Honjaman Rebeleob".into(),
                lang: Some("ko".into()),
            },
            AlternativeName {
                name: "Solo Leveling".into(),
                lang: Some("en".into()),
            },
        ],
        summary: "E-rank hunter <Sung Jinwoo> & \"friends\".".into(),
        year: Some(2018),
        writers: vec!["Chugong".into()],
        artists: vec!["DUBU (REDICE STUDIO)".into(), "Chugong".into()],
        genres: vec!["Action".into(), "Fantasy".into()],
        series_type: "manhwa".into(),
        content_rating: "suggestive".into(),
        ids: vec![
            ("anilist".into(), "105398".into()),
            ("anime_planet".into(), "solo-leveling".into()),
            ("manga_updates".into(), "15".into()),
        ],
        urls: vec!["https://mangabaka.org/42".into()],
        notes: "Series metadata from MangaBaka (https://mangabaka.org), CC BY-NC-SA 4.0.".into(),
        ..minimal()
    }
}

#[test]
fn full_comic_info_fills_every_element_in_schema_order() {
    let xml = comic_info_xml(&full());

    assert_valid(&xml, "ComicInfo.xsd");
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <ComicInfo>\n\
         \x20 <Title>Ch. 1 - I&apos;m Used to It</Title>\n\
         \x20 <Series>Solo Leveling</Series>\n\
         \x20 <Summary>E-rank hunter &lt;Sung Jinwoo&gt; &amp; &quot;friends&quot;.</Summary>\n\
         \x20 <Notes>Series metadata from MangaBaka (https://mangabaka.org), CC BY-NC-SA 4.0.</Notes>\n\
         \x20 <Year>2018</Year>\n\
         \x20 <Writer>Chugong</Writer>\n\
         \x20 <Penciller>DUBU (REDICE STUDIO), Chugong</Penciller>\n\
         \x20 <Genre>Action, Fantasy</Genre>\n\
         \x20 <Web>https://example.org/manga/solo-leveling</Web>\n\
         \x20 <Manga>Yes</Manga>\n\
         \x20 <AgeRating>Teen</AgeRating>\n\
         </ComicInfo>\n"
    );
}

#[test]
fn full_metron_info_fills_every_element() {
    let xml = metron_info_xml(&full());

    assert_valid(&xml, "MetronInfo.xsd");
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <MetronInfo>\n\
         \x20 <IDS>\n\
         \x20   <ID source=\"AniList\">105398</ID>\n\
         \x20   <ID source=\"MangaUpdates\">15</ID>\n\
         \x20 </IDS>\n\
         \x20 <Series>\n\
         \x20   <Name>Solo Leveling</Name>\n\
         \x20   <StartYear>2018</StartYear>\n\
         \x20   <AlternativeNames>\n\
         \x20     <AlternativeName lang=\"ko\">나 혼자만 레벨업</AlternativeName>\n\
         \x20     <AlternativeName lang=\"ko\">Na Honjaman Rebeleob</AlternativeName>\n\
         \x20     <AlternativeName lang=\"en\">Solo Leveling</AlternativeName>\n\
         \x20   </AlternativeNames>\n\
         \x20 </Series>\n\
         \x20 <Stories>\n\
         \x20   <Story>Ch. 1 - I&apos;m Used to It</Story>\n\
         \x20 </Stories>\n\
         \x20 <Summary>E-rank hunter &lt;Sung Jinwoo&gt; &amp; &quot;friends&quot;.</Summary>\n\
         \x20 <Notes>Series metadata from MangaBaka (https://mangabaka.org), CC BY-NC-SA 4.0.</Notes>\n\
         \x20 <Genres>\n\
         \x20   <Genre>Action</Genre>\n\
         \x20   <Genre>Fantasy</Genre>\n\
         \x20 </Genres>\n\
         \x20 <AgeRating>Teen</AgeRating>\n\
         \x20 <URLs>\n\
         \x20   <URL primary=\"true\">https://example.org/manga/solo-leveling</URL>\n\
         \x20   <URL>https://mangabaka.org/42</URL>\n\
         \x20 </URLs>\n\
         \x20 <Credits>\n\
         \x20   <Credit>\n\
         \x20     <Creator>Chugong</Creator>\n\
         \x20     <Roles>\n\
         \x20       <Role>Writer</Role>\n\
         \x20       <Role>Artist</Role>\n\
         \x20     </Roles>\n\
         \x20   </Credit>\n\
         \x20   <Credit>\n\
         \x20     <Creator>DUBU (REDICE STUDIO)</Creator>\n\
         \x20     <Roles>\n\
         \x20       <Role>Artist</Role>\n\
         \x20     </Roles>\n\
         \x20   </Credit>\n\
         \x20 </Credits>\n\
         </MetronInfo>\n"
    );
}

/// The text of the first `<name>` element, if any.
fn element<'x>(xml: &'x str, name: &str) -> Option<&'x str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))?;
    Some(&xml[start..start + end])
}

#[test]
fn manga_follows_the_reading_direction_of_the_series_type() {
    for (series_type, manga) in [
        ("manga", "YesAndRightToLeft"),
        ("manhwa", "Yes"),
        ("manhua", "Yes"),
        ("oel", "No"),
        ("novel", "Unknown"),
        ("other", "Unknown"),
    ] {
        let meta = ComicMetadata {
            series_type: series_type.into(),
            ..minimal()
        };
        let xml = comic_info_xml(&meta);
        assert_valid(&xml, "ComicInfo.xsd");
        assert_eq!(element(&xml, "Manga"), Some(manga), "{series_type}");
    }
    assert_eq!(element(&comic_info_xml(&minimal()), "Manga"), None);
}

#[test]
fn age_rating_follows_the_content_rating() {
    for (rating, comic_info, metron_info) in [
        ("safe", "Everyone", "Everyone"),
        ("suggestive", "Teen", "Teen"),
        ("erotica", "Mature 17+", "Mature"),
        ("pornographic", "Adults Only 18+", "Explicit"),
        ("unrated", "Unknown", "Unknown"),
    ] {
        let meta = ComicMetadata {
            content_rating: rating.into(),
            ..minimal()
        };
        let comic = comic_info_xml(&meta);
        let metron = metron_info_xml(&meta);
        assert_valid(&comic, "ComicInfo.xsd");
        assert_valid(&metron, "MetronInfo.xsd");
        assert_eq!(element(&comic, "AgeRating"), Some(comic_info), "{rating}");
        assert_eq!(element(&metron, "AgeRating"), Some(metron_info), "{rating}");
    }
    assert_eq!(element(&metron_info_xml(&minimal()), "AgeRating"), None);
}

/// XML 1.0 has no way to write most control characters, not even escaped.
#[test]
fn characters_xml_cannot_hold_are_dropped() {
    let meta = ComicMetadata {
        summary: "Line one\nline\u{1} two\u{fffe}\tend".into(),
        ..minimal()
    };
    let comic = comic_info_xml(&meta);
    let metron = metron_info_xml(&meta);

    assert_valid(&comic, "ComicInfo.xsd");
    assert_valid(&metron, "MetronInfo.xsd");
    assert_eq!(element(&comic, "Summary"), Some("Line one\nline two\tend"));
}

#[test]
fn values_that_are_empty_are_left_out() {
    let meta = ComicMetadata {
        genres: vec![String::new()],
        writers: vec![String::new()],
        urls: vec![String::new()],
        alternative_names: vec![AlternativeName::default()],
        ids: vec![("anilist".into(), String::new())],
        year: Some(0),
        ..minimal()
    };

    assert_eq!(comic_info_xml(&meta), comic_info_xml(&minimal()));
    assert_eq!(metron_info_xml(&meta), metron_info_xml(&minimal()));
}
