# T19: `fmd-pack`: output formats, image conversion, filename templates
Deps: T01

## Goal
Everything that turns a folder of downloaded page images into the final output: folder/zip/cbz/pdf/epub packing, image format conversion, and FMD2-compatible naming (`CustomRename`) for manga folders, chapter folders/archives and page files.

## Scope (in/out)
In:
- `pack(dir, format, out_path, opts) -> Result<PathBuf>` for `Folder` (no-op/move), `Zip`, `Cbz` (`zip` crate; stored or deflate per FMD2; natural sort of entries), `Pdf` (one image per page at native size, JPEG passthrough, PNG/WebP re-encoded; `lopdf` or `printpdf`; quality option), `Epub` (custom writer on `zip`: `mimetype` first and stored, OPF, NCX/nav, one XHTML page per image, matching `uEpub.pas` structure).
- Image conversion with `image` (+ `libwebp-sys` only if `image` can't do it): PNG→JPEG (quality), WebP→PNG or JPEG; optional ImageMagick via `magick` on PATH with FMD2's options (format, quality, compression, mogrify).
- Naming:
  - `custom_rename(template, ctx)` implementing `CustomRename`: tokens `%MANGA%`, `%CHAPTER%`, `%NUMBERING%`, `%WEBSITE%`, `%AUTHOR%`, `%ARTIST%`, `%FILENAME%`; auto-prefix numbering when neither `%NUMBERING%` nor `%CHAPTER%` present (chapter rename only); digit padding for chapter/volume numbers; HTML entity cleanup; optional unicode replacement.
  - Illegal-character stripping configurable: `Posix` (default: `/` and NUL, plus trimming trailing dots/spaces) or `Windows` (FMD2's `RemoveSymbols` set).
  - Max filename length handling (FMD2's limits as an option).
- Natural sort (`naturalsortunit.pas`) used for page ordering.

Out: the download pipeline (T20).

## Seams under test
Public `fmd-pack` API on temp dirs:
- `custom_rename("%MANGA% - %CHAPTER%", ctx{manga:"A/B", chapter:"Ch. 1"})` with POSIX stripping → `"A_B - Ch. 1"` (choose and document the replacement; Windows mode matches `RemoveSymbols`).
- `custom_rename("%CHAPTER%", …)` with numbering `0001` and template lacking `%NUMBERING%`… (cover the auto-prefix rule exactly as Pascal).
- `pack(dir_with_3_imgs, Cbz)` → zip with 3 entries in natural order (`2.jpg` before `10.jpg`).
- `pack(…, Epub)` → valid EPUB: first entry `mimetype` stored uncompressed with `application/epub+zip`; OPF lists every image.
- `pack(…, Pdf)` → PDF with 3 pages.
- `convert(png, Jpeg{quality:90})` → decodable JPEG with same dimensions; WebP→PNG likewise.

## Acceptance criteria
- [ ] All five formats produced and verified structurally in tests.
- [ ] `CustomRename` behaviour matches `uBaseUnit.pas:1798+` on documented cases (each case cites the line it exercises).
- [ ] Natural sort matches `naturalsortunit.pas` on mixed alphanumerics.
- [ ] ImageMagick path optional and skipped in tests when `magick` is absent.

## FMD2 references
- `baseunits/uPacker.pas:50-357` (`DoZipCbz` :55, `Do7Zip` :104, `DoPdf` :179, `DoEpub` :218, `Execute` :255)
- `baseunits/uEpub.pas:143-326` (EPUB structure: content, toc, pages)
- `baseunits/Img2Pdf.pas` (PDF writer, compression quality)
- `baseunits/uBaseUnit.pas:250-257` (tokens), `:1382` (`RemoveSymbols`), `:1798-1900` (`CustomRename`), `:1928` (`CleanString`)
- `baseunits/uDownloadsManager.pas:530-552` (`GetFileName`), `:553-612` (`Compress`), `:613-711` (`Convert`)
- `baseunits/imagemagickmanager.pas:504-690` (`ExecuteMagickCommand`, `ConvertImage`)
- `baseunits/naturalsortunit.pas` (natural sort), `baseunits/webp.pas` (WebP handling)
