//! The links of a series' MangaBaka match: MangaBaka itself, the tracker sites it knows the
//! series' ID on, and the other URLs it lists.

use std::collections::HashSet;

use serde::Serialize;
use utoipa::ToSchema;

/// A link to the series on another site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SeriesLink {
    /// The site's name, or the URL's host without `www.`.
    pub label: String,
    pub url: String,
}

/// The series on tracker sites, by MangaBaka's `source` key; each template was checked against
/// a live page. Kitsu and Anime-Planet are left out: their pages could not be checked.
const TRACKERS: &[(&str, &str, &str)] = &[
    ("anilist", "AniList", "https://anilist.co/manga/"),
    (
        "manga_updates",
        "MangaUpdates",
        "https://www.mangaupdates.com/series/",
    ),
    (
        "my_anime_list",
        "MyAnimeList",
        "https://myanimelist.net/manga/",
    ),
    ("shikimori", "Shikimori", "https://shikimori.io/mangas/"),
];

/// MangaBaka's page of series `id`, then the tracker sites of `xids` in [`TRACKERS`] order,
/// then `urls`; each URL once, and none that is `website_url`.
pub(crate) fn mangabaka_links(
    id: i64,
    xids: &[(String, String)],
    urls: &[String],
    website_url: Option<&str>,
) -> Vec<SeriesLink> {
    let mangabaka = SeriesLink {
        label: "MangaBaka".to_owned(),
        url: format!("https://mangabaka.org/{id}"),
    };
    let trackers = TRACKERS.iter().flat_map(|(site, label, prefix)| {
        xids.iter()
            .filter(move |(s, _)| s == site)
            .map(move |(_, xid)| SeriesLink {
                label: (*label).to_owned(),
                url: format!("{prefix}{xid}"),
            })
    });
    let listed = urls.iter().filter_map(|url| {
        let host = url::Url::parse(url).ok()?.host_str()?.to_owned();
        Some(SeriesLink {
            label: host.strip_prefix("www.").unwrap_or(&host).to_owned(),
            url: url.clone(),
        })
    });
    let mut seen: HashSet<String> = website_url.map(str::to_owned).into_iter().collect();
    std::iter::once(mangabaka)
        .chain(trackers)
        .chain(listed)
        .filter(|link| seen.insert(link.url.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(label: &str, url: &str) -> SeriesLink {
        SeriesLink {
            label: label.to_owned(),
            url: url.to_owned(),
        }
    }

    #[test]
    fn mangabaka_comes_first_then_trackers_then_its_urls_without_duplicates() {
        // Series 189's `source` and `links` in the recorded MangaBaka dump, as metadata.db
        // keeps them (`xids_of` orders by site).
        let xids = [
            ("anilist", "158766"),
            ("anime_planet", "reborn-rich"),
            ("kitsu", "65643"),
            ("manga_updates", "4v8x87y"),
            ("my_anime_list", "154318"),
            ("shikimori", "154318"),
            ("some_new_site", "1"),
        ]
        .map(|(s, x)| (s.to_owned(), x.to_owned()));
        let urls = [
            "https://sevenseasentertainment.com/series/reborn-rich-comic/",
            "https://www.webtoons.com/en/drama/reborn-rich/list?title_no=4956",
            "https://mangabaka.org/189",
        ]
        .map(str::to_owned);

        assert_eq!(
            mangabaka_links(189, &xids, &urls, None),
            vec![
                link("MangaBaka", "https://mangabaka.org/189"),
                link("AniList", "https://anilist.co/manga/158766"),
                link(
                    "MangaUpdates",
                    "https://www.mangaupdates.com/series/4v8x87y"
                ),
                link("MyAnimeList", "https://myanimelist.net/manga/154318"),
                link("Shikimori", "https://shikimori.io/mangas/154318"),
                link(
                    "sevenseasentertainment.com",
                    "https://sevenseasentertainment.com/series/reborn-rich-comic/"
                ),
                link(
                    "webtoons.com",
                    "https://www.webtoons.com/en/drama/reborn-rich/list?title_no=4956"
                ),
            ]
        );
    }

    #[test]
    fn the_website_link_shown_before_them_is_left_out() {
        let urls = [
            "https://www.webtoons.com/en/drama/reborn-rich/list?title_no=4956",
            "https://comic.naver.com/webtoon/list?titleId=800770",
        ]
        .map(str::to_owned);

        let links = mangabaka_links(
            189,
            &[],
            &urls,
            Some("https://www.webtoons.com/en/drama/reborn-rich/list?title_no=4956"),
        );

        assert_eq!(
            links,
            vec![
                link("MangaBaka", "https://mangabaka.org/189"),
                link(
                    "comic.naver.com",
                    "https://comic.naver.com/webtoon/list?titleId=800770"
                ),
            ]
        );
    }
}
