//! Where a download would save a chapter's first page, for previewing the naming settings.

use std::path::PathBuf;

use super::NewDownload;
use super::files::final_page_ext;
use super::manager::{chapter_name, manga_folder, pack_format, save_to};
use super::task::{archive_path, custom_file_name, page_file_name, working_dir};
use crate::info::remove_manga_name;
use crate::settings::Settings;

#[derive(Debug, Clone, Default)]
pub struct SampleChapter<'a> {
    /// `%WEBSITE%`.
    pub website: &'a str,
    pub title: &'a str,
    pub authors: &'a str,
    pub artists: &'a str,
    pub chapter: &'a str,
    /// 1-based (`%NUMBERING%`).
    pub number: u32,
    /// The first page's content type as an extension, e.g. `jpg`.
    pub page_ext: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagePlacement {
    /// The manga folder's name, whether or not one is generated.
    pub manga_folder: String,
    /// Its folder, or its archive without extension.
    pub chapter: String,
    /// Without extension.
    pub filename: String,
    pub page: String,
    /// The page's file, or the chapter's archive when chapters are packed.
    pub path: PathBuf,
}

/// `settings` may be an unsaved draft; it is normalised as saving would.
pub fn first_page(settings: &Settings, sample: &SampleChapter<'_>) -> PagePlacement {
    let mut settings = settings.clone();
    crate::settings::normalize(&mut settings);
    // A draft from a client that only knows `default_dir` sets the default destination's path.
    crate::settings::sync_default_dir(&Default::default(), &mut settings.saveto);
    let saveto = &settings.saveto;
    let listed = if saveto.remove_manga_name_from_chapter {
        remove_manga_name(sample.chapter, sample.title)
    } else {
        sample.chapter.to_owned()
    };
    let download = NewDownload {
        title: sample.title.to_owned(),
        authors: sample.authors.to_owned(),
        artists: sample.artists.to_owned(),
        ..NewDownload::default()
    };
    let chapter = chapter_name(saveto, sample.website, &download, &listed, sample.number);
    let dir = PathBuf::from(save_to(saveto, sample.website, "", &download));
    let template = custom_file_name(saveto, sample.website, sample.title, &chapter);
    let filename = page_file_name(&template, None, 0);
    let page = format!(
        "{filename}.{}",
        final_page_ext(&settings.images, sample.page_ext)
    );
    let path = match pack_format(settings.output.format) {
        Some(format) => archive_path(&dir, &chapter, format),
        None => working_dir(saveto, &dir, &chapter).join(&page),
    };
    PagePlacement {
        manga_folder: manga_folder(saveto, sample.website, &download),
        chapter,
        filename,
        page,
        path,
    }
}
