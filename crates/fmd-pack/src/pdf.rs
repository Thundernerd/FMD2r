//! PDF writer following FMD2's `TImg2PDF` (baseunits/Img2Pdf.pas).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat};
use lopdf::{Document, Object, Stream, dictionary};

use crate::PackError;
use crate::convert::on_white;

/// An image ready to embed: its pixel size, colour space, filter and encoded data
/// (`TPageInfo`, baseunits/Img2Pdf.pas:40-60).
struct PageImage {
    width: u32,
    height: u32,
    color_space: &'static str,
    filter: &'static str,
    data: Vec<u8>,
}

/// Writes one page per image, each page the image's pixel size (`TImg2PDF.SaveToStream`,
/// baseunits/Img2Pdf.pas:555-700). Images that cannot be read are skipped, as FMD2 does
/// (baseunits/uPacker.pas:190-195).
pub(crate) fn write_pdf(
    images: &[PathBuf],
    title: &str,
    quality: u8,
    saved: &Path,
) -> Result<(), PackError> {
    let mut doc = Document::with_version("1.3");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for path in images {
        let Ok(image) = load_page_image(path, quality) else {
            continue;
        };
        let (w, h) = (i64::from(image.width), i64::from(image.height));
        let mut xobject = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w,
                "Height" => h,
                "ColorSpace" => Object::Name(image.color_space.into()),
                "BitsPerComponent" => 8,
                "Filter" => Object::Name(image.filter.into()),
            },
            image.data,
        );
        xobject.allows_compression = false;
        let image_id = doc.add_object(xobject);
        // Img2Pdf.pas:582-584: scale the unit square to the page.
        let content = format!("q {w} 0 0 {h} 0 0 cm /I1 Do Q");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
            "Resources" => dictionary! {
                "ProcSet" => vec!["PDF".into(), "ImageB".into(), "ImageC".into()],
                "XObject" => dictionary! { "I1" => image_id },
            },
            "Contents" => content_id,
        });
        kids.push(page_id);
    }
    let Some(&first_page) = kids.first() else {
        return Err(PackError::NoImages(saved.to_path_buf()));
    };

    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => count,
        }),
    );
    // Img2Pdf.pas:640-664: info and a catalog that opens on the first page, fit to width.
    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal(title),
        "Creator" => Object::string_literal("FMD2r"),
        "Producer" => Object::string_literal("FMD2r"),
    });
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "OpenAction" => vec![first_page.into(), "FitH".into(), Object::Null],
    });
    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);
    doc.save(saved)?;
    Ok(())
}

/// `TPageInfo.LoadImageData` (baseunits/Img2Pdf.pas:495-517): a JPEG is embedded as is when
/// the quality is at least 75; otherwise any image below quality 100 is re-encoded as JPEG;
/// at quality 100 non-JPEG images are stored Flate-compressed.
fn load_page_image(path: &Path, quality: u8) -> Result<PageImage, PackError> {
    let bytes = std::fs::read(path)?;
    let format = image::guess_format(&bytes)?;
    if format == ImageFormat::Jpeg && quality >= 75 {
        return jpeg_passthrough(path, bytes);
    }
    let image = image::load_from_memory_with_format(&bytes, format)?;
    let (width, height) = (image.width(), image.height());
    let gray = !image.color().has_color();
    if quality < 100 {
        // JPEGCompressToPageInfo (Img2Pdf.pas:197-233).
        let mut data = Vec::new();
        let encoder = JpegEncoder::new_with_quality(Cursor::new(&mut data), quality);
        let (color_space, encoded) = if gray && format == ImageFormat::Jpeg {
            ("DeviceGray", DynamicImage::ImageLuma8(image.to_luma8()))
        } else {
            ("DeviceRGB", DynamicImage::ImageRgb8(on_white(&image)))
        };
        encoded.write_with_encoder(encoder)?;
        return Ok(PageImage {
            width,
            height,
            color_space,
            filter: "DCTDecode",
            data,
        });
    }
    // PNGToPageInfo / ImageToPageInfo (Img2Pdf.pas:235-332, :420-470): grey PNGs stay
    // grey, everything else becomes RGB blended over white. Palette PNGs are expanded
    // to RGB rather than written as an indexed colour space.
    let (color_space, raw) = if gray && format == ImageFormat::Png {
        ("DeviceGray", image.to_luma8().into_raw())
    } else {
        ("DeviceRGB", on_white(&image).into_raw())
    };
    let mut stream = Stream::new(dictionary! {}, raw);
    stream.compress()?;
    Ok(PageImage {
        width,
        height,
        color_space,
        filter: "FlateDecode",
        data: stream.content,
    })
}

/// `JPEGToPageInfo` (baseunits/Img2Pdf.pas:166-195): the colour space follows the JPEG's
/// component count.
fn jpeg_passthrough(path: &Path, data: Vec<u8>) -> Result<PageImage, PackError> {
    let (width, height, components) =
        jpeg_frame(&data).ok_or_else(|| PackError::UnsupportedImage(path.to_path_buf()))?;
    let color_space = match components {
        1 => "DeviceGray",
        4 => "DeviceCMYK",
        _ => "DeviceRGB",
    };
    Ok(PageImage {
        width,
        height,
        color_space,
        filter: "DCTDecode",
        data,
    })
}

/// Reads width, height and component count from a JPEG's start-of-frame marker.
fn jpeg_frame(data: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([data[i + 2], data[i + 3]]));
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let frame = data.get(i + 4..i + 10)?;
            let height = u32::from(u16::from_be_bytes([frame[1], frame[2]]));
            let width = u32::from(u16::from_be_bytes([frame[3], frame[4]]));
            return Some((width, height, frame[5]));
        }
        i += 2 + len;
    }
    None
}
