// Test helpers outside `#[test]` fns are not covered by clippy.toml's allow-unwrap-in-tests.
#![allow(clippy::unwrap_used)]

use fmd_pack::{
    RenameContext, RenameOptions, SymbolMode, custom_rename, fit_file_name, page_file_name,
};

fn posix() -> RenameOptions {
    RenameOptions::default()
}

/// FMD2r's POSIX mode replaces `RemoveSymbols` at uBaseUnit.pas:1809.
#[test]
fn posix_mode_replaces_slash_in_values_with_underscore() {
    let ctx = RenameContext {
        manga: "A/B",
        chapter: "Ch. 1",
        ..Default::default()
    };
    assert_eq!(
        custom_rename("%MANGA% - %CHAPTER%", &ctx, &posix()),
        "A_B - Ch. 1"
    );
}

fn chapter_ctx<'a>(chapter: &'a str, numbering: &'a str) -> RenameContext<'a> {
    RenameContext {
        website: "MangaDex",
        manga: "Manga",
        chapter,
        numbering,
        ..Default::default()
    }
}

/// uBaseUnit.pas:59-60, :1382: Windows mode deletes the `Symbols` set from values only.
#[test]
fn windows_mode_deletes_remove_symbols_set() {
    let ctx = RenameContext {
        manga: "A/B\\C:D*E?F\"G<H>I|J\tK;L",
        ..Default::default()
    };
    let opts = RenameOptions {
        symbols: SymbolMode::Windows,
        ..Default::default()
    };
    assert_eq!(custom_rename("[%MANGA%]", &ctx, &opts), "[ABCDEFGHIJKL]");
}

/// uBaseUnit.pas:1822-1823: no prefix when the template has %CHAPTER%.
#[test]
fn no_numbering_prefix_when_template_has_chapter() {
    assert_eq!(
        custom_rename("%CHAPTER%", &chapter_ctx("Ch. 1", "0001"), &posix()),
        "Ch. 1"
    );
}

/// uBaseUnit.pas:1822-1823: numbering is prepended verbatim, without a separator.
#[test]
fn numbering_prefixed_when_template_lacks_numbering_and_chapter() {
    assert_eq!(
        custom_rename("%WEBSITE%", &chapter_ctx("Ch. 1", "0001"), &posix()),
        "0001MangaDex"
    );
}

/// uBaseUnit.pas:1820: the prefix rule applies to chapter renames only.
#[test]
fn no_numbering_prefix_without_chapter() {
    assert_eq!(
        custom_rename("%WEBSITE%", &chapter_ctx("", "0001"), &posix()),
        "MangaDex"
    );
}

/// uBaseUnit.pas:1824: %NUMBERING% is substituted.
#[test]
fn numbering_token_is_replaced() {
    assert_eq!(
        custom_rename(
            "%NUMBERING% - %CHAPTER%",
            &chapter_ctx("Ch. 1", "0001"),
            &posix()
        ),
        "0001 - Ch. 1"
    );
}

/// uBaseUnit.pas:1786-1792: an empty value also removes the brackets around its token.
#[test]
fn empty_value_removes_surrounding_brackets() {
    assert_eq!(
        custom_rename(
            "[%NUMBERING%] %CHAPTER%",
            &chapter_ctx("Ch. 1", ""),
            &posix()
        ),
        "Ch. 1"
    );
    assert_eq!(
        custom_rename("%MANGA% (%AUTHOR%)", &chapter_ctx("", ""), &posix()),
        "Manga"
    );
}

/// uBaseUnit.pas:1845-1846: a chapter rename that ends up empty falls back to numbering.
#[test]
fn empty_chapter_result_falls_back_to_numbering() {
    assert_eq!(
        custom_rename("%CHAPTER%", &chapter_ctx("  ", "0007"), &posix()),
        "0007"
    );
}

/// uBaseUnit.pas:1854: an empty result falls back to the manga title.
#[test]
fn empty_result_falls_back_to_manga() {
    assert_eq!(
        custom_rename("%AUTHOR%", &chapter_ctx("", ""), &posix()),
        "Manga"
    );
}

/// uBaseUnit.pas:1858: leading and trailing path delimiters are removed.
#[test]
fn leading_and_trailing_path_delimiters_trimmed() {
    assert_eq!(
        custom_rename("/%MANGA%\\", &chapter_ctx("", ""), &posix()),
        "Manga"
    );
}

/// FMD2r's POSIX mode, applied after uBaseUnit.pas:1858.
#[test]
fn posix_mode_trims_trailing_dots_and_spaces() {
    let ctx = RenameContext {
        manga: "Who?..",
        ..Default::default()
    };
    assert_eq!(custom_rename("%MANGA%", &ctx, &posix()), "Who?");
}

fn manga(name: &str) -> RenameContext<'_> {
    RenameContext {
        manga: name,
        ..Default::default()
    }
}

/// uBaseUnit.pas:1807 (`CommonStringFilter`, :2119): HTML entities in values are decoded.
#[test]
fn html_entities_are_decoded() {
    assert_eq!(
        custom_rename("%MANGA%", &manga("Tom &amp; Jerry"), &posix()),
        "Tom & Jerry"
    );
    assert_eq!(
        custom_rename("%MANGA%", &manga("&#8230; Caf&eacute;"), &posix()),
        "... Café"
    );
}

/// uBaseUnit.pas:2072-2085: entities missing their `;` are decoded too.
#[test]
fn broken_entities_are_decoded() {
    assert_eq!(
        custom_rename("%MANGA%", &manga("Tom &AMP Jerry"), &posix()),
        "Tom & Jerry"
    );
}

/// uBaseUnit.pas:63, :1809: a newline becomes a literal `\n`, whose backslash
/// `RemoveSymbols` then deletes.
#[test]
fn newline_in_value_becomes_n_in_windows_mode() {
    let opts = RenameOptions {
        symbols: SymbolMode::Windows,
        ..Default::default()
    };
    assert_eq!(custom_rename("%MANGA%", &manga("A\nB"), &opts), "AnB");
}

/// uBaseUnit.pas:91-94: full-width brackets become ASCII ones.
#[test]
fn full_width_brackets_become_ascii() {
    assert_eq!(
        custom_rename("%MANGA%", &manga("［Raw］（Color）"), &posix()),
        "[Raw](Color)"
    );
}

/// uBaseUnit.pas:1811-1812 (`ReplaceUnicodeChar`, :817): every UTF-16 unit outside
/// 31..=127 is replaced, so a character outside the BMP is replaced twice.
#[test]
fn unicode_replacement_works_per_utf16_unit() {
    let opts = RenameOptions {
        replace_unicode: Some("_".into()),
        ..Default::default()
    };
    assert_eq!(
        custom_rename("%MANGA%", &manga("Café 🙂!"), &opts),
        "Caf_ __!"
    );
}

fn padded(chapter: &str, volume_len: usize, chapter_len: usize) -> String {
    let opts = RenameOptions {
        pad_volume: volume_len,
        pad_chapter: chapter_len,
        ..Default::default()
    };
    custom_rename("%CHAPTER%", &chapter_ctx(chapter, "0001"), &opts)
}

/// uBaseUnit.pas:1828-1838 with uMisc.pas:119 (`VolumeChapterPadZero`).
#[test]
fn chapter_number_is_zero_padded() {
    assert_eq!(padded("Ch. 5", 0, 3), "Ch. 005");
    // A number that ends the string (uMisc.pas:131-132).
    assert_eq!(padded("Ch. 12", 0, 3), "Ch. 012");
    assert_eq!(padded("Ch. 1234", 0, 3), "Ch. 1234");
    // Only the first number counts.
    assert_eq!(padded("Ch. 15.5", 0, 3), "Ch. 015.5");
}

/// uMisc.pas:155-187: the volume number follows `VOL` (any case).
#[test]
fn volume_number_is_zero_padded() {
    assert_eq!(padded("Vol. 1 Ch. 5", 2, 0), "Vol. 01 Ch. 5");
    assert_eq!(padded("vol.3 Chapter 15.5", 2, 3), "vol.03 Chapter 015.5");
}

/// uMisc.pas:159-160: `VOL` preceded by a letter is not a volume.
#[test]
fn vol_inside_a_word_is_not_a_volume() {
    assert_eq!(padded("Revolt 3", 2, 0), "Revolt 3");
}

/// uMisc.pas:161-162.
#[test]
fn volume_not_available_is_not_a_volume() {
    assert_eq!(
        padded("Ch. 2 Volume Not Available", 2, 3),
        "Ch. 002 Volume Not Available"
    );
}

/// uMisc.pas:208-217: the chapter search starts after the volume number. When it finds
/// nothing there, it searches again from the start but no longer pads the chapter.
#[test]
fn chapter_before_volume_is_not_padded() {
    assert_eq!(padded("Vol. 2 Ch. 7", 2, 3), "Vol. 02 Ch. 007");
    assert_eq!(padded("Ch. 7 Vol. 2 Extra", 2, 3), "Ch. 7 Vol. 02 Extra");
}

/// uDownloadsManager.pas:537-541: a missing or empty name becomes the 1-based work id
/// padded to three digits, substituted into the file-name template.
#[test]
fn page_file_name_defaults_to_padded_index() {
    assert_eq!(page_file_name("%FILENAME%", None, 0), "001");
    assert_eq!(page_file_name("%FILENAME%", Some(""), 41), "042");
    assert_eq!(page_file_name("%FILENAME%", None, 1233), "1234");
    assert_eq!(page_file_name("p_%FILENAME%", Some("cover"), 4), "p_cover");
}

/// uDownloadsManager.pas:544-548: an over-long name keeps its last characters.
#[test]
fn fit_file_name_keeps_the_tail() {
    assert_eq!(fit_file_name("abcdef", 4), "cdef");
    assert_eq!(fit_file_name("äbc", 3), "äbc");
    assert_eq!(fit_file_name("äbcd", 3), "bcd");
    // Pascal counts UTF-16 units; a surrogate pair that would be split is dropped whole.
    assert_eq!(fit_file_name("😀😀abc", 5), "😀abc");
    assert_eq!(fit_file_name("😀😀abc", 4), "abc");
}
