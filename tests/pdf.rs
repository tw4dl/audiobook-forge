#[path = "support/pdf_fixture.rs"]
mod pdf_fixture;

use kokoro_book::book::{Block, Provenance, SectionKind, SourceFormat, SourcePosition};
use kokoro_book::input::read_book;
use pdf_fixture::{
    write_blank_pdf, write_encrypted_pdf, write_pdf_with_bookmarks, write_pdf_with_outline_cycle,
    write_pdf_with_oversized_page_stream, write_pdf_with_wrong_page_count,
    write_pdf_without_bookmarks, write_pdf_without_headings,
};
use tempfile::tempdir;

#[test]
fn imports_text_pdf_with_metadata_pages_and_bookmark_hierarchy() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("bookmarked.pdf");
    write_pdf_with_bookmarks(&path);

    let book = read_book(&path).expect("bookmarked PDF");

    assert_eq!(book.source.format, SourceFormat::Pdf);
    assert_eq!(book.source.format_version.as_deref(), Some("1.7"));
    assert_eq!(book.metadata.title.as_deref(), Some("Public Domain Sample"));
    assert_eq!(book.metadata.authors, ["Example Author"]);
    assert_eq!(book.pages.len(), 3);
    assert_eq!(book.pages[0].label, "i");
    assert_eq!(book.pages[1].label, "A-3");
    assert_eq!(book.pages[2].label, "A-4");
    assert_eq!(
        book.pages[1].position,
        SourcePosition::Pdf {
            page_number: 2,
            character_offset: Some(0),
        }
    );

    let chapter_one = &book.root.children[0];
    assert_eq!(chapter_one.title.as_deref(), Some("Chapter One"));
    assert_eq!(chapter_one.kind, SectionKind::Chapter);
    assert_eq!(chapter_one.provenance, Provenance::Authored);
    assert!(matches!(
        chapter_one.blocks.as_slice(),
        [Block::Paragraph(block)] if block.text == "Opening paragraph."
    ));
    assert_eq!(
        chapter_one.children[0].title.as_deref(),
        Some("A Closer Look")
    );
    assert!(
        chapter_one.children[0].blocks[0]
            .text()
            .contains("Nested section text.")
    );

    let chapter_two = &book.root.children[1];
    assert_eq!(chapter_two.title.as_deref(), Some("Chapter Two"));
    assert!(chapter_two.blocks[0].text().contains("Closing paragraph."));
    assert!(
        !book
            .warnings
            .iter()
            .any(|warning| warning.contains("Page labels unavailable"))
    );
}

#[test]
fn imports_pdf_without_bookmarks_using_deterministic_heading_inference() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("inferred.pdf");
    write_pdf_without_bookmarks(&path);

    let book = read_book(&path).expect("PDF without bookmarks");

    assert_eq!(book.root.children.len(), 2);
    assert_eq!(book.root.children[0].title.as_deref(), Some("Chapter One"));
    assert_eq!(book.root.children[0].provenance, Provenance::Inferred);
    assert_eq!(book.root.children[1].title.as_deref(), Some("Chapter Two"));
    assert!(
        book.warnings
            .iter()
            .any(|warning| warning == "PDF has no outline; inferred 2 chapter headings")
    );
}

#[test]
fn pdf_without_outline_or_headings_uses_a_low_confidence_body_fallback() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("prose.pdf");
    write_pdf_without_headings(&path);

    let book = read_book(&path).expect("prose PDF");

    assert_eq!(book.root.children.len(), 1);
    assert_eq!(book.root.children[0].kind, SectionKind::BodyMatter);
    assert_eq!(book.root.children[0].provenance, Provenance::Derived);
    assert!(book.text.contains("ordinary prose on the first page"));
    assert!(
        book.warnings
            .iter()
            .any(|warning| warning.contains("no high-confidence chapter headings"))
    );
}

#[test]
fn rejects_pdf_without_extractable_text_with_a_clear_no_ocr_error() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("scan.pdf");
    write_blank_pdf(&path);

    let error = read_book(&path).expect_err("image-only PDF must fail");

    assert!(error.to_string().contains("contains no extractable text"));
    assert!(error.to_string().contains("OCR is not supported"));
}

#[test]
fn rejects_encrypted_pdf_without_attempting_decryption() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("protected.pdf");
    write_encrypted_pdf(&path);

    let error = read_book(&path).expect_err("encrypted PDF must fail");

    assert!(
        error
            .to_string()
            .contains("encrypted PDF is not supported; provide a DRM-free PDF")
    );
}

#[test]
fn rejects_malformed_pdf_with_parser_context() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("malformed.pdf");
    std::fs::write(&path, b"not a PDF").expect("malformed fixture");

    let error = read_book(&path).expect_err("malformed PDF must fail");

    assert!(error.to_string().contains("failed to parse PDF"));
}

#[test]
fn rejects_outline_reference_cycles_before_outline_recursion() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("outline-cycle.pdf");
    write_pdf_with_outline_cycle(&path);

    let error = read_book(&path).expect_err("cyclic outline must fail");

    assert!(
        error
            .to_string()
            .contains("PDF outline contains a reference cycle")
    );
}

#[test]
fn rejects_a_page_tree_with_a_false_declared_count() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("wrong-count.pdf");
    write_pdf_with_wrong_page_count(&path);

    let error = read_book(&path).expect_err("false page count must fail");

    assert!(error.to_string().contains("declared 2 pages but found 1"));
}

#[test]
fn rejects_a_compressed_page_stream_beyond_the_extraction_limit() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("oversized-stream.pdf");
    write_pdf_with_oversized_page_stream(&path);

    let error = read_book(&path).expect_err("oversized page stream must fail");
    let chain = format!("{error:#}");

    assert!(chain.contains("decompressed output exceeded the 16777216-byte limit"));
}
