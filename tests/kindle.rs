use assert_cmd::Command;
use audiobook_forge::book::{Block, Section};
use audiobook_forge::input::read_book;
use predicates::prelude::*;
use tempfile::tempdir;

const MOBI_FIXTURE: &str = "tests/fixtures/kindle/structured.mobi";
const PALMDOC_FIXTURE: &str = "tests/fixtures/kindle/structured-palmdoc.mobi";
const AZW3_FIXTURE: &str = "tests/fixtures/kindle/structured.azw3";
const COVER_FIXTURE: &str = "tests/fixtures/kindle/with-cover.azw3";

#[test]
fn imports_legacy_mobi_metadata_navigation_and_reading_order() {
    let book = read_book(MOBI_FIXTURE.as_ref()).expect("DRM-free MOBI imports");

    assert_eq!(book.source.format.to_string(), "MOBI");
    assert_eq!(book.source.format_version.as_deref(), Some("6"));
    assert_eq!(book.metadata.title.as_deref(), Some("Kindle Fixture"));
    assert_eq!(book.metadata.authors, ["Example Author"]);
    assert_eq!(book.metadata.language.as_deref(), Some("en"));
    assert_eq!(
        section_titles(&book.root),
        ["Chapter One", "A Useful Section", "Chapter Two"]
    );
    assert_ordered_text(&book.text);
    assert_eq!(book.text.matches("Chapter One").count(), 1);
    assert!(
        book.pages.is_empty(),
        "reflowable MOBI must not invent pages"
    );
    assert!(first_paragraph(&book.root).source_range.is_some());
}

#[test]
fn imports_azw3_kf8_metadata_html_and_navigation() {
    let book = read_book(AZW3_FIXTURE.as_ref()).expect("DRM-free AZW3 imports");

    assert_eq!(book.source.format.to_string(), "AZW3/KF8");
    assert_eq!(book.source.format_version.as_deref(), Some("8"));
    assert_eq!(book.metadata.title.as_deref(), Some("Kindle Fixture"));
    assert_eq!(book.metadata.authors, ["Example Author"]);
    assert_eq!(book.metadata.language.as_deref(), Some("en"));
    assert_eq!(
        section_titles(&book.root),
        ["Chapter One", "A Useful Section", "Chapter Two"]
    );
    assert_ordered_text(&book.text);
    assert_eq!(book.text.matches("Chapter One").count(), 1);
    assert!(
        book.pages.is_empty(),
        "reflowable KF8 must not invent pages"
    );
    assert!(first_paragraph(&book.root).source_range.is_some());
}

#[test]
fn imports_real_palmdoc_compressed_mobi() {
    let book = read_book(PALMDOC_FIXTURE.as_ref()).expect("PalmDOC-compressed MOBI imports");

    assert_eq!(book.source.format.to_string(), "MOBI");
    assert_eq!(
        section_titles(&book.root),
        ["Chapter One", "A Useful Section", "Chapter Two"]
    );
    assert_ordered_text(&book.text);
}

#[test]
fn imports_kf8_cover_bytes() {
    let book = read_book(COVER_FIXTURE.as_ref()).expect("KF8 cover imports");
    let cover = book.metadata.cover.expect("authored cover");

    assert_eq!(cover.media_type, "image/png");
    assert!(cover.source_id.starts_with("kindle:record:"));
    assert!(cover.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn inspect_azw3_needs_no_model_and_prints_the_structure() {
    let temp = tempdir().expect("temp dir");
    let cache = temp.path().join("cache");

    Command::cargo_bin("audiobook-forge")
        .expect("binary")
        .arg("inspect")
        .arg(AZW3_FIXTURE)
        .arg("--tree")
        .env("AUDIOBOOK_FORGE_CACHE_DIR", &cache)
        .assert()
        .success()
        .stdout(predicate::str::contains("Format: AZW3/KF8 8"))
        .stdout(predicate::str::contains("Author: Example Author"))
        .stdout(predicate::str::contains(
            "Kindle Fixture\n  Chapter One\n    A Useful Section\n  Chapter Two",
        ));

    assert!(!cache.exists());
}

#[test]
fn rejects_encrypted_mobi_before_content_extraction() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("encrypted.mobi");
    let mut bytes = fixture_bytes(MOBI_FIXTURE);
    let record_zero = record_offset(&bytes, 0);
    bytes[record_zero + 12..record_zero + 14].copy_from_slice(&1_u16.to_be_bytes());
    std::fs::write(&path, bytes).expect("fixture copy");

    let error = read_book(&path).expect_err("encrypted MOBI must fail");
    assert!(
        error
            .to_string()
            .contains("encrypted MOBI/KF8 is not supported; provide a DRM-free file")
    );
}

#[test]
fn isolates_huff_cdic_files_with_a_clear_limit() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("huff.mobi");
    let mut bytes = fixture_bytes(MOBI_FIXTURE);
    let record_zero = record_offset(&bytes, 0);
    bytes[record_zero..record_zero + 2].copy_from_slice(&17_480_u16.to_be_bytes());
    std::fs::write(&path, bytes).expect("fixture copy");

    let error = read_book(&path).expect_err("HUFF/CDIC needs an isolated error");
    assert!(error.to_string().contains(
        "HUFF/CDIC-compressed MOBI/KF8 is not supported; convert to an uncompressed or PalmDOC-compressed DRM-free file"
    ));
}

#[test]
fn rejects_malformed_palm_database_offsets_without_panicking() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("bad-offset.mobi");
    let mut bytes = fixture_bytes(MOBI_FIXTURE);
    bytes[78..82].copy_from_slice(&1_u32.to_be_bytes());
    std::fs::write(&path, bytes).expect("fixture copy");

    let error = read_book(&path).expect_err("bad record offset must fail");
    assert!(error.to_string().contains("MOBI record offsets"));
}

#[test]
fn rejects_excessive_record_counts_before_reading_the_table() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("too-many-records.mobi");
    let mut bytes = fixture_bytes(MOBI_FIXTURE);
    bytes[76..78].copy_from_slice(&50_001_u16.to_be_bytes());
    std::fs::write(&path, bytes).expect("fixture copy");

    let error = read_book(&path).expect_err("record bound must fail");
    assert!(error.to_string().contains("more than 50000 records"));
}

#[test]
fn rejects_excessive_declared_decoded_text_before_allocating_it() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("oversized-text.azw3");
    let mut bytes = fixture_bytes(AZW3_FIXTURE);
    let record_zero = record_offset(&bytes, 0);
    bytes[record_zero + 4..record_zero + 8].copy_from_slice(&134_217_729_u32.to_be_bytes());
    std::fs::write(&path, bytes).expect("fixture copy");

    let error = read_book(&path).expect_err("decoded text bound must fail");
    assert!(
        error
            .to_string()
            .contains("decoded MOBI text exceeds 128 MiB")
    );
}

#[test]
fn rejects_unknown_text_encoding_with_context() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("unknown-encoding.mobi");
    let mut bytes = fixture_bytes(MOBI_FIXTURE);
    let record_zero = record_offset(&bytes, 0);
    bytes[record_zero + 28..record_zero + 32].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(&path, bytes).expect("fixture copy");

    let error = read_book(&path).expect_err("unknown encoding must fail");
    assert!(
        error
            .to_string()
            .contains("MOBI/KF8 uses unsupported text encoding 999")
    );
}

#[test]
fn rejects_invalid_palmdoc_back_references_without_panicking() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("bad-palmdoc.mobi");
    let mut bytes = fixture_bytes(PALMDOC_FIXTURE);
    let record_one = record_offset(&bytes, 1);
    bytes[record_one..record_one + 2].copy_from_slice(&[0x80, 0x00]);
    std::fs::write(&path, bytes).expect("fixture copy");

    let error = read_book(&path).expect_err("invalid back-reference must fail");
    assert!(
        format!("{error:#}").contains("PalmDOC back-reference has invalid distance 0"),
        "{error:#}"
    );
}

fn fixture_bytes(path: &str) -> Vec<u8> {
    std::fs::read(path).expect("checked-in fixture")
}

fn record_offset(bytes: &[u8], index: usize) -> usize {
    let start = 78 + index * 8;
    u32::from_be_bytes(bytes[start..start + 4].try_into().expect("record offset")) as usize
}

fn section_titles(root: &Section) -> Vec<&str> {
    let mut titles = Vec::new();
    collect_titles(root, &mut titles);
    titles
}

fn collect_titles<'a>(section: &'a Section, titles: &mut Vec<&'a str>) {
    if section.level > 0
        && let Some(title) = section.title.as_deref()
    {
        titles.push(title);
    }
    for child in &section.children {
        collect_titles(child, titles);
    }
}

fn first_paragraph(root: &Section) -> &audiobook_forge::book::TextBlock {
    root.children
        .iter()
        .flat_map(|section| section.blocks.iter())
        .find_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .expect("fixture paragraph")
}

fn assert_ordered_text(text: &str) {
    let first = text
        .find("Opening public-domain-style fixture text.")
        .expect("opening");
    let nested = text.find("Nested section text.").expect("nested");
    let closing = text.find("Closing fixture text.").expect("closing");
    assert!(first < nested && nested < closing);
}
