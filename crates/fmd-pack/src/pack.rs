//! Packing a folder of page images (baseunits/uPacker.pas).

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::PackError;
use crate::epub::write_epub;
use crate::natural_sort::natural_cmp;
use crate::pdf::write_pdf;
use crate::whole::write_whole;

/// Output formats (`TPackerFormat`, baseunits/uPacker.pas:18, and `Compress`,
/// baseunits/uDownloadsManager.pas:566-571).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackFormat {
    /// Keep the images in a folder.
    Folder,
    Zip,
    Cbz,
    Pdf,
    Epub,
}

impl PackFormat {
    /// The extension appended to the output path (baseunits/uPacker.pas:280-285).
    pub fn extension(self) -> &'static str {
        match self {
            PackFormat::Folder => "",
            PackFormat::Zip => ".zip",
            PackFormat::Cbz => ".cbz",
            PackFormat::Pdf => ".pdf",
            PackFormat::Epub => ".epub",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackOptions {
    /// JPEG quality for PDF pages, 0-100 (`OptionPDFQuality`, default 95,
    /// baseunits/FMDOptions.pas:121).
    pub pdf_quality: u8,
    /// Delete the packed images, and the folder once empty, after a successful pack
    /// (baseunits/uPacker.pas:318-329).
    pub remove_sources: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            pdf_quality: 95,
            remove_sources: true,
        }
    }
}

/// Packs the images in `dir` into `out_path` plus the format's extension, and returns the
/// written path (`TPacker.Execute`, baseunits/uPacker.pas:255-330). The archive appears under
/// that path only once it is whole ([`write_whole`]).
///
/// Images are the files in `dir` (not subfolders) with an image extension, in natural order.
/// `Folder` moves `dir` to `out_path` unless they are the same.
pub fn pack(
    dir: &Path,
    format: PackFormat,
    out_path: &Path,
    opts: &PackOptions,
) -> Result<PathBuf, PackError> {
    if format == PackFormat::Folder {
        if dir != out_path {
            fs::rename(dir, out_path)?;
        }
        return Ok(out_path.to_path_buf());
    }

    let files = image_files(dir)?;
    let mut saved = out_path.as_os_str().to_owned();
    saved.push(format.extension());
    let saved = PathBuf::from(saved);

    // No images: succeed only if the output already exists (uPacker.pas:294-302).
    if files.is_empty() {
        return if saved.is_file() {
            Ok(saved)
        } else {
            Err(PackError::NoImages(dir.to_path_buf()))
        };
    }
    if saved.exists() {
        fs::remove_file(&saved)?;
    }

    // The book title is the folder's name (`GetLastDir(Path)`, baseunits/uPacker.pas:186, :225).
    write_whole(&saved, |part| match format {
        PackFormat::Zip | PackFormat::Cbz => write_zip(&files, part),
        PackFormat::Epub => write_epub(&files, &file_name(dir), part),
        PackFormat::Pdf => write_pdf(&files, &file_name(dir), opts.pdf_quality, part),
        PackFormat::Folder => Err(PackError::Io(io::Error::other("a folder is not written"))),
    })?;

    if opts.remove_sources {
        for file in &files {
            fs::remove_file(file)?;
        }
        if fs::read_dir(dir)?.next().is_none() {
            fs::remove_dir(dir)?;
        }
    }
    Ok(saved)
}

/// Extensions picked up from the folder: FMD2's search mask `*.jpg;*.png;*.gif;*.webp`
/// (baseunits/uPacker.pas:273), plus `jpeg` and the ImageMagick outputs `avif` and `jxl`.
const IMAGE_EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "gif", "webp", "avif", "jxl"];

fn image_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
        if is_image && path.is_file() {
            files.push(path);
        }
    }
    files.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));
    Ok(files)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `DoZipCbz` (baseunits/uPacker.pas:55-86): stored entries named after the files.
fn write_zip(files: &[PathBuf], saved: &Path) -> Result<(), PackError> {
    let mut zip = ZipWriter::new(BufWriter::new(File::create(saved)?));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for file in files {
        zip.start_file(file_name(file), options)?;
        io::copy(&mut File::open(file)?, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}
