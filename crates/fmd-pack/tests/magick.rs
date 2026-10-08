//! ImageMagick is optional: these tests return early when `magick` is not on PATH.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

mod common;

use fmd_pack::{MagickOptions, magick_available, magick_convert};

fn magick(save_as: &str) -> Option<MagickOptions> {
    let opts = MagickOptions {
        save_as: save_as.into(),
        quality: 85,
        ..Default::default()
    };
    if magick_available(&opts) {
        Some(opts)
    } else {
        eprintln!("magick not found; skipping");
        None
    }
}

/// uDownloadsManager.pas:613-711 with imagemagickmanager.pas:651-682: files not already in
/// the target format are converted in one `magick @list` call, then the originals deleted.
#[test]
fn converts_other_formats_and_removes_originals() {
    let Some(opts) = magick("jpg") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Chapter 1");
    std::fs::create_dir(&dir).unwrap();
    let png = common::write_image(&dir, "page 1.png", 4, 3);
    let jpg = common::write_image(&dir, "page 2.jpg", 4, 3);

    let out = magick_convert(&dir, &[png.clone(), jpg.clone()], &opts).unwrap();

    assert_eq!(out, [dir.join("page 1.jpg"), jpg.clone()]);
    let img = image::open(&out[0]).unwrap();
    assert_eq!((img.width(), img.height()), (4, 3));
    assert!(!png.exists());
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left.len(), 2, "the file list is removed: {left:?}");
}

/// uDownloadsManager.pas:632-637, imagemagickmanager.pas:670-679: JPEG XL goes through
/// `magick mogrify -path`.
#[test]
fn jxl_uses_mogrify() {
    let Some(opts) = magick("jxl") else { return };
    let tmp = tempfile::tempdir().unwrap();
    let png = common::write_image(tmp.path(), "001.png", 4, 3);

    let out = magick_convert(tmp.path(), std::slice::from_ref(&png), &opts).unwrap();

    assert_eq!(out, [tmp.path().join("001.jxl")]);
    assert!(out[0].exists());
    assert!(!png.exists());
}

#[test]
fn missing_executable_is_not_available() {
    let opts = MagickOptions {
        executable: "/nonexistent/magick".into(),
        ..Default::default()
    };
    assert!(!magick_available(&opts));
}
