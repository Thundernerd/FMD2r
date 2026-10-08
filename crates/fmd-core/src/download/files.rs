//! Page image files: finding a saved page, and saving a downloaded one under the extension its
//! content shows (baseunits/uBaseUnit.pas:2391-2525, baseunits/ImgInfos.pas).

use std::fs;
use std::path::{Path, PathBuf};

use fmd_pack::ConvertTarget;

use crate::settings::{ImageSettings, PngCompression, WebpSaveAs};

/// The extensions of FMD2's image handlers, in registration order
/// (baseunits/ImgInfos.pas:634-639).
const IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "png", "webp", "gif", "bmp", "tif"];

/// `FindImageFile` (baseunits/uBaseUnit.pas:2506-2525): `base` with the extension `ext` (when
/// not empty) or with the first image extension under which a file exists.
pub(super) fn find_image_file(base: &Path, ext: &str) -> Option<PathBuf> {
    let with = |ext: &str| {
        let mut path = base.as_os_str().to_owned();
        path.push(".");
        path.push(ext);
        PathBuf::from(path)
    };
    if !ext.is_empty() {
        let path = with(ext);
        if path.is_file() {
            return Some(path);
        }
    }
    IMAGE_EXTENSIONS
        .iter()
        .map(|e| with(e))
        .find(|p| p.is_file())
}

/// `GetImageStreamExt` (baseunits/ImgInfos.pas:124-127): the extension of the first image
/// handler whose check accepts `data`, `None` when none does (baseunits/ImgInfos.pas:350-582).
fn image_ext(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xff, 0xd8]) {
        Some("jpg")
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("webp")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("gif")
    } else if data.starts_with(b"BM") {
        Some("bmp")
    } else if data.starts_with(b"II*\0") || data.starts_with(b"MM\0*") {
        Some("tif")
    } else {
        None
    }
}

/// `SaveImageStreamToFile` (baseunits/uBaseUnit.pas:2391-2480): saves `data` in `dir` as
/// `name` plus the extension its content shows, converting PNG and WebP as the image settings
/// say unless ImageMagick converts later. Returns the saved file, or `None` when `data` is
/// empty, no image, or could not be written. The file keeps the time it was written: FMD2's
/// `Last-Modified` file date (:2482-2494) is not reproduced.
pub(super) fn save_image(
    data: &[u8],
    dir: &Path,
    name: &str,
    images: &ImageSettings,
) -> Option<PathBuf> {
    if data.is_empty() || fs::create_dir_all(dir).is_err() {
        return None;
    }
    let mut ext = image_ext(data)?;
    let mut converted = None;
    if !images.imagemagick.enabled {
        let target = match ext {
            "png" if images.png_to_jpeg => Some(ConvertTarget::Jpeg {
                quality: jpeg_quality(images),
            }),
            "webp" => match images.webp_save_as {
                WebpSaveAs::Webp => None,
                WebpSaveAs::Png => Some(ConvertTarget::Png {
                    compression: png_compression(images.png_compression),
                }),
                WebpSaveAs::Jpeg => Some(ConvertTarget::Jpeg {
                    quality: jpeg_quality(images),
                }),
            },
            _ => None,
        };
        // A failed conversion keeps the original image (baseunits/uBaseUnit.pas:2416-2437).
        if let Some(target) = target
            && let Ok(Some(data)) = fmd_pack::convert_bytes(data, target)
        {
            ext = target.extension();
            converted = Some(data);
        }
    }
    let path = dir.join(format!("{name}.{ext}"));
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    match fs::write(&path, converted.as_deref().unwrap_or(data)) {
        Ok(()) => Some(path),
        Err(e) => {
            tracing::warn!(target: "fmd_core", "saving {}: {e}", path.display());
            None
        }
    }
}

fn jpeg_quality(images: &ImageSettings) -> u8 {
    u8::try_from(images.jpeg_quality.min(100)).unwrap_or(100)
}

fn png_compression(level: PngCompression) -> fmd_pack::PngCompression {
    match level {
        PngCompression::None => fmd_pack::PngCompression::None,
        PngCompression::Fastest => fmd_pack::PngCompression::Fastest,
        PngCompression::Default => fmd_pack::PngCompression::Default,
        PngCompression::Maximum => fmd_pack::PngCompression::Max,
    }
}
