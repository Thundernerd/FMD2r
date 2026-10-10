//! The list titles' cover links in `lists.db` (T70).

use rusqlite::{OptionalExtension, params};

use crate::db::Db;
use crate::error::{Result, StoreError};

/// Where a title's cover link came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverSource {
    /// The title's accepted match in MangaBaka's database.
    MangaBaka,
    /// The website module's `GetInfo` (`MangaInfo.CoverLink`, baseunits/lua/LuaMangaInfo.pas:27).
    Website,
}

impl CoverSource {
    /// The name stored in `lists.db`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MangaBaka => "mangabaka",
            Self::Website => "website",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [Self::MangaBaka, Self::Website]
            .into_iter()
            .find(|c| c.as_str() == s)
    }
}

/// A list title's cover link, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverLink {
    /// The cover; `None` when the title is known to have none.
    pub url: Option<String>,
    /// A larger cover, when the source has one (MangaBaka's 350 px tall thumbnail).
    pub large_url: Option<String>,
    pub source: CoverSource,
    /// The MangaBaka series the link was taken from.
    pub series_id: Option<i64>,
    /// When the link was found, in seconds since the Unix epoch.
    pub checked_at: i64,
}

/// Repository for the list titles' cover links. Obtain it with [`crate::ListsDb::cover_links`].
pub struct CoverLinkRepo<'a> {
    pub(crate) db: &'a Db,
}

impl CoverLinkRepo<'_> {
    /// The stored cover link of `module_id`'s title at `link`.
    pub fn get(&self, module_id: &str, link: &str) -> Result<Option<CoverLink>> {
        let conn = self.db.lock();
        let row = conn
            .query_row(
                "SELECT url, large_url, source, series_id, checked_at FROM cover_links
                 WHERE module_id = ?1 AND link = ?2",
                [module_id, link],
                |r| {
                    Ok((
                        r.get::<_, String>(2)?,
                        CoverLink {
                            url: r.get(0)?,
                            large_url: r.get(1)?,
                            source: CoverSource::Website,
                            series_id: r.get(3)?,
                            checked_at: r.get(4)?,
                        },
                    ))
                },
            )
            .optional()?;
        let Some((source, stored)) = row else {
            return Ok(None);
        };
        let source = CoverSource::parse(&source).ok_or(StoreError::InvalidColumn {
            column: "cover_links.source",
            value: source,
        })?;
        Ok(Some(CoverLink { source, ..stored }))
    }

    /// Stores `cover` as the cover link of `module_id`'s title at `link`, replacing any other.
    pub fn put(&self, module_id: &str, link: &str, cover: &CoverLink) -> Result<()> {
        let conn = self.db.lock();
        conn.prepare_cached(
            "INSERT OR REPLACE INTO cover_links
                 (module_id, link, url, large_url, source, series_id, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            module_id,
            link,
            cover.url,
            cover.large_url,
            cover.source.as_str(),
            cover.series_id,
            cover.checked_at,
        ])?;
        Ok(())
    }
}
