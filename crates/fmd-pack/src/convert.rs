//! Built-in image conversion (baseunits/uBaseUnit.pas:2312-2386).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ImageFormat, RgbImage};

use crate::PackError;

/// FMD2's PNG compression levels (`Tcompressionlevel`, `OptionPNGCompressionLevel`,
/// default `clfastest`, baseunits/FMDOptions.pas:125).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PngCompression {
    None,
    #[default]
    Fastest,
    Default,
    Max,
}

/// What to convert an image to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertTarget {
    /// `WebPToPNGStream` (baseunits/uBaseUnit.pas:2312-2336).
    Png { compression: PngCompression },
    /// `PNGToJPEGStream` / `WebPToJPEGStream` (baseunits/uBaseUnit.pas:2338-2386);
    /// `OptionJPEGQuality` defaults to 80 (baseunits/FMDOptions.pas:126).
    Jpeg { quality: u8 },
}

impl ConvertTarget {
    fn format(self) -> ImageFormat {
        match self {
            ConvertTarget::Png { .. } => ImageFormat::Png,
            ConvertTarget::Jpeg { .. } => ImageFormat::Jpeg,
        }
    }

    /// The extension FMD2 saves the format under (baseunits/uBaseUnit.pas:2420, :2430).
    pub fn extension(self) -> &'static str {
        match self {
            ConvertTarget::Png { .. } => "png",
            ConvertTarget::Jpeg { .. } => "jpg",
        }
    }
}

/// Converts the image file at `path` to `target`, writing it next to the source with the
/// target's extension and deleting the source. Returns the new path, or `path` unchanged
/// when the file already is in the target format.
pub fn convert(path: &Path, target: ConvertTarget) -> Result<PathBuf, PackError> {
    let bytes = std::fs::read(path)?;
    let Some(data) = convert_bytes(&bytes, target)? else {
        return Ok(path.to_path_buf());
    };
    let out = path.with_extension(target.extension());
    std::fs::write(&out, data)?;
    if out != path {
        std::fs::remove_file(path)?;
    }
    Ok(out)
}

/// Converts encoded image `bytes` to `target`; `None` when they already are in that format.
pub fn convert_bytes(bytes: &[u8], target: ConvertTarget) -> Result<Option<Vec<u8>>, PackError> {
    let format = image::guess_format(bytes)?;
    if format == target.format() {
        return Ok(None);
    }
    let image = image::load_from_memory_with_format(bytes, format)?;
    let mut data = Vec::new();
    match target {
        ConvertTarget::Png { compression } => {
            let compression = match compression {
                PngCompression::None => CompressionType::Uncompressed,
                PngCompression::Fastest => CompressionType::Fast,
                PngCompression::Default => CompressionType::Default,
                PngCompression::Max => CompressionType::Best,
            };
            let encoder = PngEncoder::new_with_quality(
                Cursor::new(&mut data),
                compression,
                FilterType::Adaptive,
            );
            // `UseAlpha := HasTransparentPixels` (baseunits/uBaseUnit.pas:2326).
            let transparent = image.to_rgba8().pixels().any(|p| p.0[3] < 255);
            let image = if transparent {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else if image.color().has_color() {
                DynamicImage::ImageRgb8(image.to_rgb8())
            } else {
                DynamicImage::ImageLuma8(image.to_luma8())
            };
            image.write_with_encoder(encoder)?;
        }
        ConvertTarget::Jpeg { quality } => {
            let encoder = JpegEncoder::new_with_quality(Cursor::new(&mut data), quality);
            // JPEG has no alpha channel; transparent pixels are blended over white.
            let image = if image.color().has_color() || image.color().has_alpha() {
                DynamicImage::ImageRgb8(on_white(&image))
            } else {
                DynamicImage::ImageLuma8(image.to_luma8())
            };
            image.write_with_encoder(encoder)?;
        }
    }
    Ok(Some(data))
}

/// Blends transparent pixels over white (`AlphaBlend(CLW, C)`, baseunits/Img2Pdf.pas:319-320).
pub(crate) fn on_white(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let blend =
            |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
        image::Rgb([blend(r), blend(g), blend(b)])
    })
}
