//! Metadata files comic readers look for inside a CBZ: ComicInfo.xml (Anansi Project schema
//! v2.0) and MetronInfo.xml (Metron Project schema v1.0). Elements without a value are left out.

/// What a chapter archive's metadata files say, in neither file's vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComicMetadata {
    pub series: String,
    /// The chapter's name.
    pub chapter: String,
    /// The series' page on the website it was downloaded from.
    pub url: String,
    /// Other names of the series, e.g. its native, romanized and English titles.
    pub alternative_names: Vec<AlternativeName>,
    pub summary: String,
    /// The year the series started.
    pub year: Option<i64>,
    pub writers: Vec<String>,
    pub artists: Vec<String>,
    pub genres: Vec<String>,
    /// MangaBaka's `type`: `manga`, `manhwa`, `manhua`, `oel`, `novel`, `other`, ...
    pub series_type: String,
    /// MangaBaka's `content_rating`: `safe`, `suggestive`, `erotica`, `pornographic`, ...
    pub content_rating: String,
    /// `(site, id)` pairs, `site` a MangaBaka `source` key such as `anilist`.
    pub ids: Vec<(String, String)>,
    /// More pages about the series, after [`Self::url`].
    pub urls: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AlternativeName {
    pub name: String,
    /// A two-letter language code such as `ja`.
    pub lang: Option<String>,
}

/// `ComicInfo.xml`, elements in the schema's `xs:sequence` order.
pub fn comic_info_xml(meta: &ComicMetadata) -> String {
    let mut xml = Xml::new("ComicInfo");
    xml.element("Title", &meta.chapter);
    xml.element("Series", &meta.series);
    xml.element("Summary", &meta.summary);
    xml.element("Notes", &meta.notes);
    xml.element("Year", &year(meta.year));
    xml.element("Writer", &meta.writers.join(", "));
    xml.element("Penciller", &meta.artists.join(", "));
    xml.element("Genre", &meta.genres.join(", "));
    xml.element("Web", &meta.url);
    if !meta.series_type.is_empty() {
        xml.element("Manga", comic_info_manga(&meta.series_type));
    }
    if !meta.content_rating.is_empty() {
        xml.element("AgeRating", comic_info_age_rating(&meta.content_rating));
    }
    xml.finish("ComicInfo")
}

/// ComicInfo's `Manga` for MangaBaka's `type`: right-to-left for Japanese manga only.
fn comic_info_manga(series_type: &str) -> &'static str {
    match series_type {
        "manga" => "YesAndRightToLeft",
        "manhwa" | "manhua" => "Yes",
        "oel" => "No",
        _ => "Unknown",
    }
}

/// ComicInfo's `AgeRating` for MangaBaka's `content_rating`.
fn comic_info_age_rating(content_rating: &str) -> &'static str {
    match content_rating {
        "safe" => "Everyone",
        "suggestive" => "Teen",
        "erotica" => "Mature 17+",
        "pornographic" => "Adults Only 18+",
        _ => "Unknown",
    }
}

/// Four digits, as MetronInfo's `xs:gYear` wants; empty for a year neither schema can hold.
fn year(year: Option<i64>) -> String {
    year.filter(|y| (1..=9999).contains(y))
        .map(|y| format!("{y:04}"))
        .unwrap_or_default()
}

/// `MetronInfo.xml`, elements in the order the schema lists them (it uses `xs:all`).
pub fn metron_info_xml(meta: &ComicMetadata) -> String {
    let mut xml = Xml::new("MetronInfo");
    let ids: Vec<_> = meta
        .ids
        .iter()
        .filter_map(|(site, id)| Some((metron_source(site)?, id.as_str())))
        .filter(|(_, id)| !id.is_empty())
        .collect();
    if !ids.is_empty() {
        xml.open("IDS");
        for (source, id) in ids {
            xml.element_with("ID", &[("source", source)], id);
        }
        xml.close("IDS");
    }

    xml.open("Series");
    xml.element("Name", &meta.series);
    xml.element("StartYear", &year(meta.year));
    let names: Vec<_> = meta
        .alternative_names
        .iter()
        .filter(|n| !n.name.is_empty())
        .collect();
    if !names.is_empty() {
        xml.open("AlternativeNames");
        for name in names {
            match name.lang.as_deref() {
                Some(lang) => xml.element_with("AlternativeName", &[("lang", lang)], &name.name),
                None => xml.element("AlternativeName", &name.name),
            }
        }
        xml.close("AlternativeNames");
    }
    xml.close("Series");

    xml.list("Stories", "Story", [meta.chapter.as_str()]);
    xml.element("Summary", &meta.summary);
    xml.element("Notes", &meta.notes);
    xml.list("Genres", "Genre", meta.genres.iter().map(String::as_str));
    if !meta.content_rating.is_empty() {
        xml.element("AgeRating", metron_age_rating(&meta.content_rating));
    }

    let urls: Vec<_> = meta.urls.iter().filter(|u| !u.is_empty()).collect();
    if !meta.url.is_empty() || !urls.is_empty() {
        xml.open("URLs");
        xml.element_with("URL", &[("primary", "true")], &meta.url);
        for url in urls {
            xml.element("URL", url);
        }
        xml.close("URLs");
    }

    let credits = credits(meta);
    if !credits.is_empty() {
        xml.open("Credits");
        for (creator, roles) in credits {
            xml.open("Credit");
            xml.element("Creator", creator);
            xml.list("Roles", "Role", roles);
            xml.close("Credit");
        }
        xml.close("Credits");
    }
    xml.finish("MetronInfo")
}

/// Each person once, in order of first mention, with the roles `Writer` (an author) and
/// `Artist`.
fn credits(meta: &ComicMetadata) -> Vec<(&str, Vec<&'static str>)> {
    let mut credits: Vec<(&str, Vec<&'static str>)> = Vec::new();
    let people = (meta.writers.iter().map(|w| (w, "Writer")))
        .chain(meta.artists.iter().map(|a| (a, "Artist")));
    for (person, role) in people {
        if person.is_empty() {
            continue;
        }
        match credits.iter_mut().find(|(p, _)| *p == person) {
            Some((_, roles)) if !roles.contains(&role) => roles.push(role),
            Some(_) => {}
            None => credits.push((person, vec![role])),
        }
    }
    credits
}

/// MetronInfo's `informationSource` for a MangaBaka `source` key; `None` for sites it lacks.
fn metron_source(site: &str) -> Option<&'static str> {
    match site {
        "anilist" => Some("AniList"),
        "kitsu" => Some("Kitsu"),
        "manga_updates" => Some("MangaUpdates"),
        "my_anime_list" => Some("MyAnimeList"),
        "mangadex" => Some("MangaDex"),
        _ => None,
    }
}

/// MetronInfo's `AgeRating` for MangaBaka's `content_rating`.
fn metron_age_rating(content_rating: &str) -> &'static str {
    match content_rating {
        "safe" => "Everyone",
        "suggestive" => "Teen",
        "erotica" => "Mature",
        "pornographic" => "Explicit",
        _ => "Unknown",
    }
}

struct Xml {
    out: String,
    depth: usize,
}

impl Xml {
    fn new(root: &str) -> Self {
        Self {
            out: format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<{root}>\n"),
            depth: 1,
        }
    }

    fn indent(&mut self) {
        self.out.push_str(&"  ".repeat(self.depth));
    }

    fn open(&mut self, name: &str) {
        self.indent();
        self.out.push_str(&format!("<{name}>\n"));
        self.depth += 1;
    }

    fn close(&mut self, name: &str) {
        self.depth = self.depth.saturating_sub(1);
        self.indent();
        self.out.push_str(&format!("</{name}>\n"));
    }

    /// Left out when `value` is empty.
    fn element(&mut self, name: &str, value: &str) {
        self.element_with(name, &[], value);
    }

    /// Left out when `value` is empty.
    fn element_with(&mut self, name: &str, attrs: &[(&str, &str)], value: &str) {
        if value.is_empty() {
            return;
        }
        self.indent();
        self.out.push_str(&format!("<{name}"));
        for (attr, attr_value) in attrs {
            self.out
                .push_str(&format!(" {attr}=\"{}\"", escape(attr_value)));
        }
        self.out.push_str(&format!(">{}</{name}>\n", escape(value)));
    }

    /// `<container>` holding one `<item>` per non-empty value; left out when there is none.
    fn list<'v>(&mut self, container: &str, item: &str, values: impl IntoIterator<Item = &'v str>) {
        let values: Vec<_> = values.into_iter().filter(|v| !v.is_empty()).collect();
        if values.is_empty() {
            return;
        }
        self.open(container);
        for value in values {
            self.element(item, value);
        }
        self.close(container);
    }

    fn finish(mut self, root: &str) -> String {
        self.out.push_str(&format!("</{root}>\n"));
        self.out
    }
}

/// Escapes markup characters and drops those XML 1.0 cannot hold (its `Char` production).
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' | '\n' | '\r' => out.push(c),
            '\u{0}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => {}
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}
