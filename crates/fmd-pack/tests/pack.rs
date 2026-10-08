// Test helpers outside `#[test]` fns are not covered by clippy.toml's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs::File;

use fmd_pack::{PackFormat, PackOptions, pack};
use zip::ZipArchive;

fn entries(path: &std::path::Path) -> Vec<(String, zip::CompressionMethod)> {
    let mut zip = ZipArchive::new(File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let f = zip.by_index(i).unwrap();
            (f.name().to_string(), f.compression())
        })
        .collect()
}

/// uPacker.pas:55-81, :279: one stored entry per image, named after the file, in natural order.
#[test]
fn cbz_has_stored_entries_in_natural_order() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    let out = pack(
        &dir,
        PackFormat::Cbz,
        &tmp.path().join("Ch. 1"),
        &PackOptions::default(),
    )
    .unwrap();

    assert_eq!(out, tmp.path().join("Ch. 1.cbz"));
    let names: Vec<_> = entries(&out)
        .into_iter()
        .map(|(n, m)| {
            assert_eq!(m, zip::CompressionMethod::Stored);
            n
        })
        .collect();
    assert_eq!(names, ["1.jpg", "2.jpg", "10.png"]);
}

/// uPacker.pas:318-329: packed images are deleted, then the folder once empty.
#[test]
fn zip_removes_packed_sources_and_empty_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    let out = pack(
        &dir,
        PackFormat::Zip,
        &tmp.path().join("Ch. 1"),
        &PackOptions::default(),
    )
    .unwrap();

    assert_eq!(out, tmp.path().join("Ch. 1.zip"));
    assert_eq!(entries(&out).len(), 3);
    assert!(!dir.exists());
}

#[test]
fn sources_kept_when_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    std::fs::write(dir.join("notes.txt"), "x").unwrap();
    let opts = PackOptions {
        remove_sources: false,
        ..Default::default()
    };
    let out = pack(&dir, PackFormat::Zip, &tmp.path().join("out"), &opts).unwrap();

    assert_eq!(entries(&out).len(), 3, "only images are packed");
    assert!(dir.join("1.jpg").exists());
}

#[test]
fn folder_format_moves_the_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    let target = tmp.path().join("Ch. 1");
    let out = pack(&dir, PackFormat::Folder, &target, &PackOptions::default()).unwrap();

    assert_eq!(out, target);
    assert!(target.join("10.png").exists());
    assert!(!dir.exists());
}

/// uPacker.pas:294-302.
#[test]
fn empty_folder_is_an_error_unless_the_output_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("empty");
    std::fs::create_dir(&dir).unwrap();
    let out = tmp.path().join("Ch. 1");

    assert!(pack(&dir, PackFormat::Cbz, &out, &PackOptions::default()).is_err());
    std::fs::write(tmp.path().join("Ch. 1.cbz"), "old").unwrap();
    assert_eq!(
        pack(&dir, PackFormat::Cbz, &out, &PackOptions::default()).unwrap(),
        tmp.path().join("Ch. 1.cbz")
    );
}

fn read_entry(path: &std::path::Path, name: &str) -> String {
    let mut zip = ZipArchive::new(File::open(path).unwrap()).unwrap();
    std::io::read_to_string(zip.by_name(name).unwrap()).unwrap()
}

/// uEpub.pas:250-283: `mimetype` first and stored, then the container, style, one image
/// plus XHTML page per file, the OPF and the NCX.
#[test]
fn epub_structure_matches_fmd2() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    let out = pack(
        &dir,
        PackFormat::Epub,
        &tmp.path().join("Ch. 1"),
        &PackOptions::default(),
    )
    .unwrap();

    assert_eq!(out, tmp.path().join("Ch. 1.epub"));
    let entries = entries(&out);
    assert_eq!(
        entries[0],
        ("mimetype".to_string(), zip::CompressionMethod::Stored)
    );
    assert_eq!(read_entry(&out, "mimetype"), "application/epub+zip");
    let names: Vec<_> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "mimetype",
            "META-INF/container.xml",
            "OEBPS/style.css",
            "OEBPS/images/0001.jpg",
            "OEBPS/0001.xhtml",
            "OEBPS/images/0002.jpg",
            "OEBPS/0002.xhtml",
            "OEBPS/images/0003.png",
            "OEBPS/0003.xhtml",
            "OEBPS/content.opf",
            "OEBPS/toc.ncx",
        ]
    );

    assert!(
        read_entry(&out, "META-INF/container.xml").contains(r#"full-path="OEBPS/content.opf""#)
    );

    // uEpub.pas:73-91, :233-237: title is the folder name; every image and page is listed.
    let opf = read_entry(&out, "OEBPS/content.opf");
    assert!(opf.contains("<dc:title>Chapter 1</dc:title>"));
    for (i, (ext, mime)) in [
        ("jpg", "image/jpeg"),
        ("jpg", "image/jpeg"),
        ("png", "image/png"),
    ]
    .iter()
    .enumerate()
    {
        let n = i + 1;
        assert!(opf.contains(&format!(
            r#"<item id="image{n:04}" href="images/{n:04}.{ext}" media-type="{mime}"/>"#
        )));
        assert!(opf.contains(&format!(
            r#"<item id="page{n:04}" href="{n:04}.xhtml" media-type="application/xhtml+xml"/>"#
        )));
        assert!(opf.contains(&format!(r#"<itemref idref="page{n:04}"/>"#)));
    }

    // uEpub.pas:117-125: one navPoint per page.
    let ncx = read_entry(&out, "OEBPS/toc.ncx");
    assert!(ncx.contains(r#"<navPoint id="toc_3" playOrder="3">"#));
    assert!(ncx.contains(r#"<content src="0003.xhtml"/>"#));

    // uEpub.pas:127-138, :226: page title is "<title> - <index>".
    let page = read_entry(&out, "OEBPS/0002.xhtml");
    assert!(page.contains("<title>Chapter 1 - 0002</title>"));
    assert!(page.contains(r#"<img src="images/0002.jpg"/>"#));
}

struct PdfPage {
    media_box: Vec<i64>,
    filter: String,
    color_space: String,
    data: Vec<u8>,
}

fn pdf_pages(path: &std::path::Path) -> (lopdf::Document, Vec<PdfPage>) {
    let doc = lopdf::Document::load(path).unwrap();
    let pages = doc
        .get_pages()
        .values()
        .map(|&id| {
            let page = doc.get_dictionary(id).unwrap();
            let media_box = page
                .get(b"MediaBox")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o.as_i64().unwrap())
                .collect();
            let resources = doc.get_dict_in_dict(page, b"Resources").unwrap();
            let xobjects = doc.get_dict_in_dict(resources, b"XObject").unwrap();
            let (_, image) = xobjects.iter().next().unwrap();
            let image = doc
                .get_object(image.as_reference().unwrap())
                .unwrap()
                .as_stream()
                .unwrap();
            let name = |key: &[u8]| {
                let o = image.dict.get(key).unwrap();
                let o = o.as_array().map(|a| a[0].clone()).unwrap_or(o.clone());
                String::from_utf8(o.as_name().unwrap().to_vec()).unwrap()
            };
            PdfPage {
                media_box,
                filter: name(b"Filter"),
                color_space: name(b"ColorSpace"),
                data: image.content.clone(),
            }
        })
        .collect();
    (doc, pages)
}

/// Img2Pdf.pas:575-599: one page per image, sized to the image; with the default quality
/// (95) JPEGs are embedded as is and other images re-encoded as JPEG (:500-506).
#[test]
fn pdf_has_one_page_per_image_at_native_size() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    let jpeg = std::fs::read(dir.join("1.jpg")).unwrap();
    let out = pack(
        &dir,
        PackFormat::Pdf,
        &tmp.path().join("Ch. 1"),
        &PackOptions::default(),
    )
    .unwrap();

    assert_eq!(out, tmp.path().join("Ch. 1.pdf"));
    let (doc, pages) = pdf_pages(&out);
    assert_eq!(pages.len(), 3);
    let sizes: Vec<_> = pages.iter().map(|p| p.media_box.clone()).collect();
    assert_eq!(
        sizes,
        [vec![0, 0, 3, 2], vec![0, 0, 4, 6], vec![0, 0, 5, 7]]
    );
    assert!(
        pages
            .iter()
            .all(|p| p.filter == "DCTDecode" && p.color_space == "DeviceRGB")
    );
    assert_eq!(pages[0].data, jpeg, "JPEG passthrough");
    let png_page = image::load_from_memory(&pages[2].data).unwrap();
    assert_eq!((png_page.width(), png_page.height()), (5, 7));

    // DoPdf (uPacker.pas:186): the title is the folder name.
    let info = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let title = doc.get_dictionary(info).unwrap().get(b"Title").unwrap();
    assert_eq!(title.as_str().unwrap(), b"Chapter 1");
}

/// Img2Pdf.pas:500-512: at quality 100 non-JPEG images are stored losslessly (Flate RGB).
#[test]
fn pdf_quality_100_stores_png_losslessly() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    let opts = PackOptions {
        pdf_quality: 100,
        ..Default::default()
    };
    let out = pack(&dir, PackFormat::Pdf, &tmp.path().join("Ch. 1"), &opts).unwrap();

    let (_, pages) = pdf_pages(&out);
    assert_eq!(pages[2].filter, "FlateDecode");
    assert_eq!(pages[2].color_space, "DeviceRGB");
}

/// Img2Pdf.pas:500-504: below quality 75 JPEGs are re-encoded too.
#[test]
fn pdf_low_quality_reencodes_jpeg() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = common::chapter_dir(tmp.path());
    let jpeg = std::fs::read(dir.join("1.jpg")).unwrap();
    let opts = PackOptions {
        pdf_quality: 50,
        ..Default::default()
    };
    let out = pack(&dir, PackFormat::Pdf, &tmp.path().join("Ch. 1"), &opts).unwrap();

    let (_, pages) = pdf_pages(&out);
    assert_eq!(pages[0].filter, "DCTDecode");
    assert_ne!(pages[0].data, jpeg);
}
