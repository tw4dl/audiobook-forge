use std::fmt::Write as _;
use std::fs::File;

use kokoro_book::input::read_book;
use tempfile::tempdir;

#[path = "support/epub.rs"]
mod epub_fixture;
use epub_fixture::{
    patch_central_uncompressed_size, patch_eocd_entry_count, write_epub_fixture,
    write_epub_fixture_with_cross_package_font_relabel,
    write_epub_fixture_with_duplicate_manifest_url, write_epub_fixture_with_duplicate_rootfile,
    write_epub_fixture_with_encryption_manifest,
    write_epub_fixture_with_font_algorithm_for_non_font, write_epub_fixture_with_font_obfuscation,
    write_epub_fixture_with_foreign_font_item, write_epub_fixture_with_large_entry,
    write_epub_fixture_with_many_remote_manifest_items,
    write_epub_fixture_with_remote_manifest_item, write_epub_fixture_with_unsafe_path,
    write_epub_fixture_with_utf16_font_obfuscation,
};

#[test]
fn rejects_excessive_html_nesting_without_recursing() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("deep.html");
    let source = format!("{}text{}", "<div>".repeat(129), "</div>".repeat(129));
    std::fs::write(&path, source).expect("fixture");

    let error = read_book(&path).expect_err("deep HTML must fail");

    assert!(
        error
            .to_string()
            .contains("HTML nesting exceeds 128 levels")
    );
}

#[test]
fn rejects_excessive_html_parse_errors_before_dom_construction() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("parse-error-bomb.html");
    std::fs::write(&path, "</div>".repeat(100_001)).expect("fixture");

    let error = read_book(&path).expect_err("HTML parse-error bomb must fail");

    assert!(
        error
            .to_string()
            .contains("HTML exceeds 100000 parser resource units")
    );
}

#[test]
fn rejects_excessive_html_attributes_before_dom_construction() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("attribute-bomb.html");
    let attributes = html_attributes(4_097);
    std::fs::write(&path, format!("<p {attributes}>text</p>")).expect("fixture");

    let error = read_book(&path).expect_err("HTML attribute bomb must fail");

    assert!(
        error
            .to_string()
            .contains("HTML exceeds 4096 total attribute budget")
    );
}

#[test]
fn rejects_aggregate_html_attribute_work_before_tokenization() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("aggregate-attribute-bomb.html");
    let first = html_attributes(2_049);
    let second = html_attributes(2_049);
    std::fs::write(&path, format!("<p {first}>one</p><p {second}>two</p>")).expect("fixture");

    let error = read_book(&path).expect_err("aggregate HTML attribute bomb must fail");

    assert!(
        error
            .to_string()
            .contains("HTML exceeds 4096 total attribute budget")
    );
}

#[test]
fn rejects_attributes_on_html_end_tags() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("end-tag-attribute-bomb.html");
    let attributes = html_attributes(4_097);
    std::fs::write(&path, format!("<p>text</p {attributes}>")).expect("fixture");

    let error = read_book(&path).expect_err("end-tag attributes must share the budget");

    assert!(
        error
            .to_string()
            .contains("HTML exceeds 4096 total attribute budget")
    );
}

#[test]
fn rejects_nul_led_html_attribute_work_before_tokenization() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("nul-attribute-bomb.html");
    let attributes = "\0 ".repeat(4_097);
    std::fs::write(&path, format!("<p {attributes}>text</p>")).expect("fixture");

    let error = read_book(&path).expect_err("NUL attributes must share the raw budget");

    assert!(
        error
            .to_string()
            .contains("HTML exceeds 4096 total attribute budget")
    );
}

#[test]
fn rejects_vertical_tab_html_attribute_work_before_tokenization() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("vertical-tab-attribute-bomb.html");
    let attributes = "\u{000b} ".repeat(4_097);
    std::fs::write(&path, format!("<p {attributes}>text</p>")).expect("fixture");

    let error = read_book(&path).expect_err("vertical-tab attributes must share the raw budget");

    assert!(
        error
            .to_string()
            .contains("HTML exceeds 4096 total attribute budget")
    );
}

fn html_attributes(count: usize) -> String {
    let mut attributes = String::new();
    for index in 0..count {
        write!(&mut attributes, "a{index}=\"\"").expect("attribute fixture");
    }
    attributes
}

#[test]
fn rejects_oversized_raw_text_before_reading_it() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("oversized.md");
    let file = File::create(&path).expect("fixture");
    file.set_len(32 * 1_024 * 1_024 + 1)
        .expect("sparse fixture");

    let error = read_book(&path).expect_err("oversized input must fail");

    assert!(error.to_string().contains("input exceeds 32 MiB limit"));
}

#[test]
fn rejects_a_malformed_epub() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.epub");
    std::fs::write(&path, b"not an epub").expect("fixture");

    let error = read_book(&path).expect_err("malformed EPUB must fail");
    assert!(error.to_string().contains("failed to open EPUB"));
}

#[test]
fn reads_epub_spine_in_order_and_ignores_markup() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("sample.epub");
    write_epub_fixture(&path);

    let book = read_book(&path).expect("read epub");

    let first = book.text.find("Chapter One").expect("first chapter");
    let second = book.text.find("Chapter Two").expect("second chapter");
    assert!(first < second);
    assert!(book.text.contains("Hello & goodbye."));
    assert!(!book.text.contains("<h1>"));
}

#[test]
fn rejects_epub_with_oversized_expanded_entry_before_import() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("compressed-bomb.epub");
    write_epub_fixture_with_large_entry(&path);
    assert!(std::fs::metadata(&path).expect("metadata").len() < 1_024 * 1_024);

    let error = read_book(&path).expect_err("expanded entry must fail");

    assert!(
        error
            .to_string()
            .contains("EPUB entry exceeds 32 MiB expanded limit")
    );
}

#[test]
fn rejects_epub_with_forged_expanded_size_before_import() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("forged-size.epub");
    write_epub_fixture_with_large_entry(&path);
    patch_central_uncompressed_size(&path, "OEBPS/unused-large.bin", 1);

    let error = read_book(&path).expect_err("actual expansion must fail");

    assert!(
        error
            .to_string()
            .contains("EPUB entry exceeds 32 MiB expanded limit")
    );
}

#[test]
fn rejects_epub_entry_count_before_archive_allocation() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("too-many-declared-entries.epub");
    write_epub_fixture(&path);
    patch_eocd_entry_count(&path, 10_001);

    let error = read_book(&path).expect_err("declared entry count must fail");

    assert!(
        error
            .to_string()
            .contains("EPUB contains more than 10000 archive entries")
    );
}

#[test]
fn rejects_epub_resource_encryption_manifest_before_import() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("protected.epub");
    write_epub_fixture_with_encryption_manifest(&path);

    let error = read_book(&path).expect_err("protected EPUB must fail");

    assert!(
        error
            .to_string()
            .contains("Unsupported encrypted/DRM-protected input")
    );
}

#[test]
fn permits_standard_epub_font_obfuscation_manifest() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("font-obfuscation.epub");
    write_epub_fixture_with_font_obfuscation(&path);

    let book = read_book(&path).expect("font obfuscation is not DRM");

    assert!(book.text.contains("Chapter One"));
}

#[test]
fn permits_utf16_epub_font_obfuscation_metadata() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("utf16-font-obfuscation.epub");
    write_epub_fixture_with_utf16_font_obfuscation(&path);

    let book = read_book(&path).expect("UTF-16 font obfuscation is not DRM");

    assert!(book.text.contains("Chapter One"));
}

#[test]
fn permits_unrelated_remote_epub_manifest_resources() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("remote-manifest-resource.epub");
    write_epub_fixture_with_remote_manifest_item(&path);

    let book = read_book(&path).expect("remote manifest item must not be fetched");

    assert!(book.text.contains("Chapter One"));
}

#[test]
fn rejects_too_many_remote_epub_manifest_resources() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("too-many-remote-manifest-resources.epub");
    write_epub_fixture_with_many_remote_manifest_items(&path);

    let error = read_book(&path).expect_err("remote resources must share the manifest limit");

    assert!(error.to_string().contains("too many manifest resources"));
}

#[test]
fn rejects_font_obfuscation_algorithm_for_non_font_resource() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("non-font-obfuscation.epub");
    write_epub_fixture_with_font_algorithm_for_non_font(&path);

    let error = read_book(&path).expect_err("font algorithm must only cover fonts");

    assert!(
        error
            .to_string()
            .contains("Unsupported encrypted/DRM-protected input")
    );
}

#[test]
fn rejects_duplicate_epub_package_paths() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("duplicate-rootfile.epub");
    write_epub_fixture_with_duplicate_rootfile(&path);

    let error = read_book(&path).expect_err("duplicate package path must fail");

    assert!(
        error
            .to_string()
            .contains("duplicate package document path")
    );
}

#[test]
fn foreign_epub_item_cannot_reclassify_xhtml_as_a_font() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("foreign-font-item.epub");
    write_epub_fixture_with_foreign_font_item(&path);

    let error = read_book(&path).expect_err("foreign font item must not bypass protection");

    assert!(
        error
            .to_string()
            .contains("Unsupported encrypted/DRM-protected input")
    );
}

#[test]
fn rejects_duplicate_normalized_epub_manifest_urls() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("duplicate-manifest-url.epub");
    write_epub_fixture_with_duplicate_manifest_url(&path);

    let error = read_book(&path).expect_err("duplicate manifest URL must fail");

    assert!(error.to_string().contains("duplicate manifest URL"));
}

#[test]
fn rejects_cross_package_font_reclassification() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("cross-package-font-relabel.epub");
    write_epub_fixture_with_cross_package_font_relabel(&path);

    let error = read_book(&path).expect_err("packages must not disagree about media type");

    assert!(
        error
            .to_string()
            .contains("Unsupported encrypted/DRM-protected input")
    );
}

#[test]
fn rejects_unsafe_epub_archive_paths_before_import() {
    let temp = tempdir().expect("temp dir");

    for (index, unsafe_name) in ["../escape.txt", "/absolute.txt"].into_iter().enumerate() {
        let path = temp.path().join(format!("unsafe-{index}.epub"));
        write_epub_fixture_with_unsafe_path(&path, unsafe_name);

        let error = read_book(&path).expect_err("unsafe archive path must fail");

        assert!(
            error
                .to_string()
                .contains("EPUB contains an unsafe archive path")
        );
    }
}
