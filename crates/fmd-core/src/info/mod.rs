//! A series' info as a module's `OnGetInfo` reports it, cleaned up as FMD2's
//! `TMangaInformation.GetInfoFromURL` does (baseunits/uData.pas:85-208).

mod text;

use std::sync::Arc;

use fmd_lua::{CallbackError, JobError, Module, WorkerPool};

use self::text::{
    clean_multilined_string, clean_string, clean_url, common_string_filter, fix_white_space,
    remove_string_breaks, trim, trim_right_commas,
};

/// What FMD2 shows of a series after `GetInfoFromURL`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MangaInfo {
    pub title: String,
    pub alt_titles: String,
    /// The series link relative to the module's `RootURL`.
    pub link: String,
    pub cover_link: String,
    pub authors: String,
    pub artists: String,
    pub genres: String,
    /// `MangaInfo_Status*` (baseunits/uBaseUnit.pas:230-233): `0` completed, `1` ongoing,
    /// `2` hiatus, `3` cancelled; anything else is unknown.
    pub status: String,
    pub summary: String,
    /// In module order.
    pub chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chapter {
    pub name: String,
    pub link: String,
}

/// The settings `GetInfoFromURL` reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct InfoOptions {
    /// `OptionRemoveMangaNameFromChapter`.
    pub remove_manga_name_from_chapter: bool,
}

/// Why there is no info.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InfoError {
    #[error("no such module")]
    UnknownModule,
    /// `information_not_found`, also what FMD2 makes of a module without `OnGetInfo` or a
    /// callback that raised an error (baseunits/lua/LuaWebsiteModules.pas:251-265).
    #[error("{0}")]
    NotFound(String),
    /// `net_problem`.
    #[error("the website could not be reached")]
    NetProblem,
    /// The worker pool failed (shut down, ...).
    #[error("{0}")]
    Failed(String),
}

/// `net_problem` (`NET_PROBLEM`, baseunits/uBaseUnit.pas:192).
const NET_PROBLEM: u8 = 1;
/// `no_error` (`NO_ERROR`, baseunits/uBaseUnit.pas:191).
const NO_ERROR: u8 = 0;

/// Runs `module`'s `OnGetInfo` for the series at `link` on `pool` and cleans up what it reports
/// (`GetInfoFromURL`, baseunits/uData.pas:85-208).
pub async fn get_info(
    pool: &WorkerPool,
    module: &Arc<Module>,
    link: &str,
    options: InfoOptions,
) -> Result<MangaInfo, InfoError> {
    // `Trim(AURL) = ''` (baseunits/uData.pas:92-93).
    if link.trim().is_empty() {
        return Err(InfoError::NotFound("no series link".into()));
    }
    let reply = match pool.on(module).get_info(link).await {
        Ok(reply) => reply.value,
        Err(JobError::NoCallback { .. }) => {
            return Err(InfoError::NotFound(
                "the module cannot show series info".into(),
            ));
        }
        Err(JobError::Callback(CallbackError { message, .. })) => {
            return Err(InfoError::NotFound(message));
        }
        Err(e) => return Err(InfoError::Failed(e.to_string())),
    };
    match reply.status {
        NO_ERROR => {}
        NET_PROBLEM => return Err(InfoError::NetProblem),
        _ => return Err(InfoError::NotFound("series not found".into())),
    }
    Ok(clean_up(reply.info, options))
}

/// The clean-up `GetInfoFromURL` runs once `OnGetInfo` returned (baseunits/uData.pas:111-205).
fn clean_up(raw: fmd_lua::MangaInfo, options: InfoOptions) -> MangaInfo {
    let link = if raw.link.is_empty() {
        // `RemoveHostFromURL(MangaInfo.URL)`.
        path_of(&raw.url)
    } else {
        raw.link
    };
    let text_field = |s: &str| {
        trim(&fix_white_space(&remove_string_breaks(
            &common_string_filter(s),
        )))
        .to_owned()
    };
    let name_field = |s: &str| {
        let s = trim(&fix_white_space(&remove_string_breaks(trim(s)))).to_owned();
        trim_right_commas(trim(&fix_white_space(&s))).to_owned()
    };
    // A field that is just markup or a placeholder dash counts as empty.
    let placeholder = |s: String| {
        if s.starts_with('<') || s == "-" || s == ":" {
            String::new()
        } else {
            s
        }
    };
    let mut title = text_field(&raw.title);
    if title.is_empty() {
        title = "N/A".into();
    }
    let summary = clean_multilined_string(&fix_white_space(&raw.summary));
    let summary = if summary == "-" || summary == ":" {
        String::new()
    } else {
        summary
    };
    MangaInfo {
        chapters: clean_chapters(raw.chapter_links, raw.chapter_names, &title, options),
        title,
        alt_titles: placeholder(text_field(&raw.alt_titles)),
        link,
        cover_link: clean_url(&raw.cover_link),
        authors: placeholder(name_field(&raw.authors)),
        artists: placeholder(name_field(&raw.artists)),
        genres: name_field(&raw.genres),
        status: raw.status,
        summary,
    }
}

/// The chapter clean-up of `GetInfoFromURL` (baseunits/uData.pas:150-202): names padded or cut to
/// the links, duplicate links dropped (the last one stays), hosts removed from links (dropping
/// the ones left empty), names cleaned and, when `options` say so, stripped of the title.
fn clean_chapters(
    links: Vec<String>,
    mut names: Vec<String>,
    title: &str,
    options: InfoOptions,
) -> Vec<Chapter> {
    names.resize(links.len(), String::new());
    let mut chapters: Vec<Chapter> = links
        .into_iter()
        .zip(names)
        .map(|(link, name)| Chapter {
            link: trim(&link).to_owned(),
            name: trim(&name).to_owned(),
        })
        .collect();
    // `SameText`: later duplicates win.
    let mut j = 0;
    while j + 1 < chapters.len() {
        let duplicate = chapters[j + 1..]
            .iter()
            .any(|c| c.link.eq_ignore_ascii_case(&chapters[j].link));
        if duplicate {
            chapters.remove(j);
        } else {
            j += 1;
        }
    }
    // `RemoveHostFromURLsPair` (baseunits/uBaseUnit.pas:983-999).
    chapters.retain_mut(|c| {
        c.link = path_of(&c.link);
        !c.link.is_empty()
    });
    for chapter in &mut chapters {
        chapter.name = trim(&clean_string(&remove_string_breaks(&common_string_filter(
            &chapter.name,
        ))))
        .to_owned();
    }
    if options.remove_manga_name_from_chapter {
        for chapter in &mut chapters {
            chapter.name = remove_manga_name(&chapter.name, title);
        }
    }
    chapters
}

/// `OptionRemoveMangaNameFromChapter` (baseunits/uData.pas:186-200): `name` without a leading
/// `title` (ignoring case) and a `- ` after it, when it is longer than the title.
pub(crate) fn remove_manga_name(name: &str, title: &str) -> String {
    if !title.is_empty()
        && name.len() > title.len()
        && name.is_char_boundary(title.len())
        && name[..title.len()].eq_ignore_ascii_case(title)
    {
        let rest = trim(&name[title.len()..]);
        rest.strip_prefix("- ").unwrap_or(rest).to_owned()
    } else {
        name.to_owned()
    }
}

/// `RemoveHostFromURL` (baseunits/uBaseUnit.pas:969-972): the path `SplitURL` finds.
fn path_of(url: &str) -> String {
    let (_, path) = fmd_http::split_url_bytes(url.as_bytes());
    String::from_utf8_lossy(&path).into_owned()
}
