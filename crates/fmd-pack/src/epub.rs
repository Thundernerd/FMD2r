//! EPUB writer matching FMD2's `TEpubBuilder` (baseunits/uEpub.pas).

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::PackError;

const CONTAINER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

const STYLE: &str = "img {\n  max-width: 100%;\n  max-height: 100%;\n}\n";

/// Writes `images` as an EPUB 2 book titled `title` (`TEpubBuilder.SaveToStream`,
/// baseunits/uEpub.pas:193-240). `mimetype` is the first entry and stored uncompressed,
/// as the EPUB OCF spec requires; everything else is deflated.
pub(crate) fn write_epub(images: &[PathBuf], title: &str, saved: &Path) -> Result<(), PackError> {
    let uuid = uuid::Uuid::new_v4().to_string();
    let pages: Vec<Page> = images
        .iter()
        .enumerate()
        .map(|(i, path)| Page::new(i + 1, path, title))
        .collect();

    let mut zip = ZipWriter::new(BufWriter::new(File::create(saved)?));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("mimetype", stored)?;
    zip.write_all(b"application/epub+zip")?;
    zip.start_file("META-INF/container.xml", deflated)?;
    zip.write_all(CONTAINER.as_bytes())?;
    zip.start_file("OEBPS/style.css", deflated)?;
    zip.write_all(STYLE.as_bytes())?;
    for page in &pages {
        zip.start_file(format!("OEBPS/images/{}", page.image_name), deflated)?;
        io::copy(&mut File::open(page.image_path)?, &mut zip)?;
        zip.start_file(format!("OEBPS/{}", page.page_name()), deflated)?;
        zip.write_all(page.xhtml().as_bytes())?;
    }
    zip.start_file("OEBPS/content.opf", deflated)?;
    zip.write_all(content(&pages, title, &uuid).as_bytes())?;
    zip.start_file("OEBPS/toc.ncx", deflated)?;
    zip.write_all(toc(&pages, title, &uuid).as_bytes())?;
    zip.finish()?;
    Ok(())
}

/// `TPage` (baseunits/uEpub.pas:263-312).
struct Page<'a> {
    index: usize,
    image_path: &'a Path,
    image_name: String,
    title: String,
}

impl<'a> Page<'a> {
    /// `TEpubBuilder.AddImage` (baseunits/uEpub.pas:183-191).
    fn new(index: usize, image_path: &'a Path, book_title: &str) -> Self {
        let ext = image_path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        Self {
            index,
            image_path,
            image_name: format!("{index:04}{ext}"),
            title: format!("{book_title} - {index:04}"),
        }
    }

    fn page_name(&self) -> String {
        format!("{:04}.xhtml", self.index)
    }

    /// `GetContentItem` (baseunits/uEpub.pas:96-97, :263-267).
    fn content_items(&self) -> String {
        let i = self.index;
        format!(
            "    <item id=\"image{i:04}\" href=\"images/{}\" media-type=\"{}\"/>\n    \
             <item id=\"page{i:04}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>\n",
            self.image_name,
            mime_type(self.image_path),
            self.page_name()
        )
    }

    /// `GetPage` (baseunits/uEpub.pas:124-135, :294-297).
    fn xhtml(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <link href="style.css" rel="stylesheet" type="text/css"/>
  <title>{}</title>
</head>
<body>
  <div><img src="images/{}"/></div>
</body>
</html>
"#,
            escape_html(&self.title),
            self.image_name
        )
    }

    /// `GetNavPoint` (baseunits/uEpub.pas:116-122, :306-309).
    fn nav_point(&self) -> String {
        let i = self.index;
        format!(
            "    <navPoint id=\"toc_{i}\" playOrder=\"{i}\">\n      <navLabel>\n        \
             <text>Page {i}</text>\n      </navLabel>\n      <content src=\"{}\"/>\n    \
             </navPoint>\n",
            self.page_name()
        )
    }
}

/// `CreateContent` (baseunits/uEpub.pas:76-94, :160-171).
fn content(pages: &[Page], title: &str, uuid: &str) -> String {
    let items: String = pages.iter().map(Page::content_items).collect();
    let refs: String = pages
        .iter()
        .map(|p| format!("    <itemref idref=\"page{:04}\"/>\n", p.index))
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:dcterms="http://purl.org/dc/terms/" unique-identifier="bookid" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>{}</dc:title>
    <dc:language>und</dc:language>
    <dc:identifier id="bookid" opf:scheme="UUID">urn:uuid:{uuid}</dc:identifier>
  </metadata>
  <manifest>
    <item id="_toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="_style" href="style.css" media-type="text/css"/>
{items}
  </manifest>
  <spine toc="_toc">
{refs}
  </spine>
</package>"#,
        escape_html(title)
    )
}

/// `CreateToc` (baseunits/uEpub.pas:102-114, :173-181). FMD2 inserts the title unescaped;
/// it is escaped here so the NCX stays well-formed.
fn toc(pages: &[Page], title: &str, uuid: &str) -> String {
    let nav_points: String = pages.iter().map(Page::nav_point).collect();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content="urn:uuid:{uuid}"/>
  </head>
  <docTitle>
    <text>{}</text>
  </docTitle>
  <navMap>
{nav_points}
  </navMap>
</ncx>
"#,
        escape_html(title)
    )
}

/// `GetMimeType` (baseunits/uBaseUnit.pas:2639-2649), matching the extension in any case,
/// plus the ImageMagick outputs that `pack` also accepts.
pub(crate) fn mime_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "jxl" => "image/jxl",
        _ => "",
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
