//! `GET /api/tasks/{id}/files`: the "Get files" button. Streams a task's single packed chapter
//! as it is, or a zip of everything the task saved.

use std::fs::File;
use std::io::{self, Seek, Write};
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use fmd_core::download::{ChapterStatus, TaskChapter};
use tokio_util::io::ReaderStream;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::state::off_thread;
use crate::tasks::find;
use crate::{ApiError, AppState, Problem};

/// The archive formats a chapter may be packed in and their media types
/// (`FMDSupportedPackedOutputExt`, baseunits/uBaseUnit.pas:235).
const PACKED: [(&str, &str); 4] = [
    ("cbz", "application/vnd.comicbook+zip"),
    ("zip", "application/zip"),
    ("pdf", "application/pdf"),
    ("epub", "application/epub+zip"),
];

/// Something a task saved.
enum Saved {
    /// A packed chapter.
    File { path: PathBuf, mime: &'static str },
    /// A chapter folder.
    Folder(PathBuf),
    /// The series folder, holding pages saved without chapter folders: its files, not its
    /// subfolders.
    Pages(PathBuf),
}

/// What the task's chapters left in `save_to`: each chapter's archive, else its folder. Saved
/// without chapter folders (`chapter_folders` off), a task with downloaded chapters that has
/// neither saved its pages straight into `save_to`, among those of other tasks of the series.
fn saved(save_to: &Path, chapters: &[TaskChapter], chapter_folders: bool) -> Vec<Saved> {
    let mut found = Vec::new();
    for chapter in chapters {
        let packed = PACKED.iter().find_map(|(ext, mime)| {
            let path = save_to.join(format!("{}.{ext}", chapter.name));
            path.is_file().then_some(Saved::File { path, mime })
        });
        let folder = save_to.join(&chapter.name);
        match packed {
            Some(file) => found.push(file),
            None if folder.is_dir() => found.push(Saved::Folder(folder)),
            None => {}
        }
    }
    let downloaded = chapters
        .iter()
        .any(|c| c.status == ChapterStatus::Downloaded);
    if found.is_empty() && !chapter_folders && downloaded && save_to.is_dir() {
        found.push(Saved::Pages(save_to.to_owned()));
    }
    found
}

/// Download a task's files: its one packed chapter as it is, or a zip of its archives and
/// chapter folders, as an attachment.
#[utoipa::path(get, path = "/api/tasks/{id}/files", tag = "tasks", operation_id = "getTaskFiles",
    params(("id" = i64, Path, description = "Task ID")),
    responses(
        (status = 200, description = "The chapter archive, or a zip of the task's files",
            content(
                ("application/vnd.comicbook+zip"),
                ("application/zip"),
                ("application/pdf"),
                ("application/epub+zip"),
            )),
        (status = 404, body = Problem, description = "No such task, or nothing saved yet"),
    ))]
pub(crate) async fn get(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
) -> Result<Response, ApiError> {
    let info = find(&state, id).await?;
    let save_to = PathBuf::from(&info.task.save_to);
    let title = info.task.title.clone();
    let chapters = info.chapters;
    let chapter_folders = state.settings.get().saveto.generate_chapter_folder;
    let (file, name, mime) = off_thread(move || -> Result<_, ApiError> {
        let mut saved = saved(&save_to, &chapters, chapter_folders);
        match saved.as_mut_slice() {
            [] => Err(ApiError::Missing("the task has saved no files yet".into())),
            [Saved::File { path, mime }] => {
                let name = file_name(path);
                Ok((File::open(&*path).map_err(io_error)?, name, *mime))
            }
            _ => {
                let zip = zip_all(&saved).map_err(io_error)?;
                Ok((zip, format!("{title}.zip"), "application/zip"))
            }
        }
    })
    .await??;
    let len = file.metadata().map_err(io_error)?.len();
    let body = Body::from_stream(ReaderStream::new(tokio::fs::File::from_std(file)));
    Ok((
        [
            (header::CONTENT_TYPE, mime.to_owned()),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::CONTENT_DISPOSITION, attachment(&name)),
        ],
        body,
    )
        .into_response())
}

fn io_error(err: io::Error) -> ApiError {
    ApiError::Internal(err.to_string())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Zips `saved` into an anonymous temporary file, gone once closed. Archives and chapter
/// folders keep their names; the series folder adds its contents at the top. Pages and
/// archives are compressed already, so they are stored as they are.
fn zip_all(saved: &[Saved]) -> io::Result<File> {
    let mut zip = ZipWriter::new(tempfile::tempfile()?);
    for item in saved {
        match item {
            Saved::File { path, .. } => add_file(&mut zip, path, &file_name(path))?,
            Saved::Folder(dir) => add_dir(&mut zip, dir, &format!("{}/", file_name(dir)))?,
            Saved::Pages(dir) => {
                for path in sorted_entries(dir)?.into_iter().filter(|p| p.is_file()) {
                    add_file(&mut zip, &path, &file_name(&path))?;
                }
            }
        }
    }
    let mut file = zip.finish().map_err(io::Error::other)?;
    file.rewind()?;
    Ok(file)
}

/// The entries of `dir`, by name.
fn sorted_entries(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    Ok(entries)
}

fn add_dir(zip: &mut ZipWriter<File>, dir: &Path, prefix: &str) -> io::Result<()> {
    for path in sorted_entries(dir)? {
        let name = format!("{prefix}{}", file_name(&path));
        if path.is_dir() {
            add_dir(zip, &path, &format!("{name}/"))?;
        } else {
            add_file(zip, &path, &name)?;
        }
    }
    Ok(())
}

fn add_file(zip: &mut ZipWriter<File>, path: &Path, name: &str) -> io::Result<()> {
    let mut source = File::open(path)?;
    let large = source.metadata()?.len() >= u64::from(u32::MAX);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(large);
    zip.start_file(name, options).map_err(io::Error::other)?;
    io::copy(&mut source, zip)?;
    zip.flush()
}

/// A `Content-Disposition` for downloading as `name` (RFC 6266): an ASCII `filename` with
/// anything else replaced by `_`, and the exact name as UTF-8 in `filename*` (RFC 8187).
fn attachment(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| match c {
            ' '..='~' if c != '"' && c != '\\' => c,
            _ => '_',
        })
        .collect();
    let mut encoded = String::new();
    for byte in name.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => encoded.push(char::from(byte)),
            b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}
