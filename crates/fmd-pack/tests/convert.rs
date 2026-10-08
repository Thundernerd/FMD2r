// Test helpers outside `#[test]` fns are not covered by clippy.toml's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

mod common;

use fmd_pack::{ConvertTarget, PngCompression, convert};
use image::ImageFormat;

fn format_of(path: &std::path::Path) -> ImageFormat {
    image::guess_format(&std::fs::read(path).unwrap()).unwrap()
}

/// uBaseUnit.pas:2364-2386, :2417-2423: PNG saved as JPEG at the given quality.
#[test]
fn png_to_jpeg_keeps_dimensions() {
    let tmp = tempfile::tempdir().unwrap();
    let png = common::write_image(tmp.path(), "001.png", 9, 5);
    let out = convert(&png, ConvertTarget::Jpeg { quality: 90 }).unwrap();

    assert_eq!(out, tmp.path().join("001.jpg"));
    assert_eq!(format_of(&out), ImageFormat::Jpeg);
    let img = image::open(&out).unwrap();
    assert_eq!((img.width(), img.height()), (9, 5));
    assert!(!png.exists(), "the source is replaced");
}

/// uBaseUnit.pas:2312-2336, :2428: WebP saved as PNG.
#[test]
fn webp_to_png_keeps_dimensions() {
    let tmp = tempfile::tempdir().unwrap();
    let webp = common::write_image(tmp.path(), "002.webp", 6, 8);
    let out = convert(
        &webp,
        ConvertTarget::Png {
            compression: PngCompression::Fastest,
        },
    )
    .unwrap();

    assert_eq!(out, tmp.path().join("002.png"));
    assert_eq!(format_of(&out), ImageFormat::Png);
    let img = image::open(&out).unwrap();
    assert_eq!((img.width(), img.height()), (6, 8));
    // uBaseUnit.pas:2326: alpha is only written when a pixel is transparent.
    assert!(!img.color().has_alpha());
    assert!(!webp.exists());
}

/// uBaseUnit.pas:2338-2362: WebP saved as JPEG.
#[test]
fn webp_to_jpeg_keeps_dimensions() {
    let tmp = tempfile::tempdir().unwrap();
    let webp = common::write_image(tmp.path(), "003.webp", 7, 3);
    let out = convert(&webp, ConvertTarget::Jpeg { quality: 80 }).unwrap();

    assert_eq!(format_of(&out), ImageFormat::Jpeg);
    let img = image::open(&out).unwrap();
    assert_eq!((img.width(), img.height()), (7, 3));
}

/// Converting to the format the file already has leaves it alone.
#[test]
fn same_format_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let jpg = common::write_image(tmp.path(), "004.jpg", 2, 2);
    let before = std::fs::read(&jpg).unwrap();
    let out = convert(&jpg, ConvertTarget::Jpeg { quality: 10 }).unwrap();

    assert_eq!(out, jpg);
    assert_eq!(std::fs::read(&out).unwrap(), before);
}
