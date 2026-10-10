//! Thumbnails for `?w=`.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageFormat, ImageResult};

/// JPEG quality of thumbnails of JPEG covers.
const JPEG_QUALITY: u8 = 85;

/// JPEG covers stay JPEG, others become PNG. `None` (serve the cover itself) when the image is no
/// wider than `width` or not decodable.
pub(crate) fn thumbnail(body: &[u8], width: u32) -> ImageResult<Option<(Vec<u8>, &'static str)>> {
    let Ok(format) = image::guess_format(body) else {
        return Ok(None);
    };
    let Ok(img) = image::load_from_memory_with_format(body, format) else {
        return Ok(None);
    };
    if img.width() <= width {
        return Ok(None);
    }
    let thumb = img.resize(width, u32::MAX, FilterType::Triangle);
    let mut out = Cursor::new(Vec::new());
    if format == ImageFormat::Jpeg {
        thumb
            .to_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))?;
        Ok(Some((out.into_inner(), "image/jpeg")))
    } else {
        thumb.write_to(&mut out, ImageFormat::Png)?;
        Ok(Some((out.into_inner(), "image/png")))
    }
}
