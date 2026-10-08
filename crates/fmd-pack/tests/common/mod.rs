#![allow(dead_code)] // Each test binary uses a different subset of these helpers.

use std::path::{Path, PathBuf};

use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

/// Writes a `width`x`height` image to `dir/name`, in the format its extension names.
pub fn write_image(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let path = dir.join(name);
    let format = ImageFormat::from_path(&path).unwrap();
    if format == ImageFormat::Png || format == ImageFormat::WebP {
        RgbaImage::from_pixel(width, height, Rgba([200, 30, 30, 255]))
            .save_with_format(&path, format)
            .unwrap();
    } else {
        RgbImage::from_pixel(width, height, Rgb([30, 200, 30]))
            .save_with_format(&path, format)
            .unwrap();
    }
    path
}

/// A chapter folder `<tmp>/Chapter 1` holding `2.jpg`, `10.png` and `1.jpg`.
pub fn chapter_dir(tmp: &Path) -> PathBuf {
    let dir = tmp.join("Chapter 1");
    std::fs::create_dir(&dir).unwrap();
    write_image(&dir, "2.jpg", 4, 6);
    write_image(&dir, "10.png", 5, 7);
    write_image(&dir, "1.jpg", 3, 2);
    dir
}
