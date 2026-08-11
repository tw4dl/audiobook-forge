use kokoro_book::book::{Block, Provenance, SectionKind, SourcePosition};
use kokoro_book::input::read_book;
use tempfile::tempdir;

#[test]
fn reads_and_normalizes_utf8_text() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.TXT");
    std::fs::write(&path, "  First line.\r\n\r\nSecond   line.  ").expect("fixture");

    let book = read_book(&path).expect("read text");

    assert_eq!(book.text, "First line.\n\nSecond line.");
}

#[test]
fn maps_txt_paragraphs_back_to_source_byte_ranges() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("mapped.txt");
    let source = "CHAPTER ONE\nFirst paragraph.\n";
    std::fs::write(&path, source).expect("fixture");

    let book = read_book(&path).expect("read text");
    let Block::Paragraph(paragraph) = &book.root.children[0].blocks[0] else {
        panic!("expected paragraph block");
    };
    let range = paragraph.source_range.as_ref().expect("source range");

    assert_eq!(range.source_id, "mapped.txt");
    let SourcePosition::Text { byte_offset: start } = &range.start else {
        panic!("TXT range must use text offsets");
    };
    let SourcePosition::Text { byte_offset: end } = &range.end else {
        panic!("TXT range must use text offsets");
    };
    assert_eq!(&source[*start..*end], "First paragraph.");

    let section_range = book.root.children[0]
        .source_range
        .as_ref()
        .expect("heading source range");
    assert_eq!(section_range.source_id, "mapped.txt");
    assert_eq!(section_range.start, SourcePosition::Text { byte_offset: 0 });
    assert_eq!(
        section_range.end,
        SourcePosition::Text {
            byte_offset: "CHAPTER ONE".len(),
        }
    );
    assert_eq!(book.root.children[0].provenance, Provenance::Inferred);
}

#[test]
fn preserves_html_semantic_blocks_and_ignores_scripts() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("semantic.html");
    std::fs::write(
        &path,
        r##"<html><body>
<h1>Chapter One</h1>
<p>Opening paragraph.</p>
<blockquote>A quoted thought.</blockquote>
<ul><li>First item</li><li>Second item</li></ul>
<figure><img alt="Revenue chart"><figcaption>Revenue rose.</figcaption></figure>
<aside>A short side note.</aside>
<nav><a href="#one">Chapter One link</a></nav>
<script>this must never be narrated</script>
</body></html>"##,
    )
    .expect("fixture");

    let book = read_book(&path).expect("read HTML");
    let blocks = &book.root.children[0].blocks;

    assert!(matches!(&blocks[0], Block::Paragraph(block) if block.text == "Opening paragraph."));
    assert!(matches!(&blocks[1], Block::Quote(block) if block.text == "A quoted thought."));
    assert!(
        matches!(&blocks[2], Block::List(block) if !block.ordered && block.items == ["First item", "Second item"])
    );
    assert!(
        matches!(&blocks[3], Block::Figure(block) if block.alt_text.as_deref() == Some("Revenue chart") && block.caption.as_deref() == Some("Revenue rose."))
    );
    assert!(matches!(&blocks[4], Block::Aside(block) if block.text == "A short side note."));
    assert!(matches!(&blocks[5], Block::Navigation(block) if block.text == "Chapter One link"));
    assert!(!book.text.contains("this must never be narrated"));
}

#[test]
fn html_tree_builder_honors_optional_paragraph_end_tags() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("optional-tags.html");
    std::fs::write(
        &path,
        "<h1>Chapter One</h1><p>First paragraph<p>Second paragraph",
    )
    .expect("fixture");

    let book = read_book(&path).expect("read HTML");
    let blocks = &book.root.children[0].blocks;

    assert!(matches!(&blocks[0], Block::Paragraph(block) if block.text == "First paragraph"));
    assert!(matches!(&blocks[1], Block::Paragraph(block) if block.text == "Second paragraph"));
}

#[test]
fn standalone_xhtml_preserves_xml_cdata_and_self_closing_elements() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("structured.xhtml");
    std::fs::write(
        &path,
        r#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><span id="marker"/><h1>Chapter One</h1><p><![CDATA[Visible XML text.]]></p></body></html>"#,
    )
    .expect("fixture");

    let book = read_book(&path).expect("valid XHTML");

    assert_eq!(book.root.children[0].title.as_deref(), Some("Chapter One"));
    assert!(
        matches!(&book.root.children[0].blocks[0], Block::Paragraph(block) if block.text == "Visible XML text.")
    );
}

#[test]
fn maps_markdown_headings_and_paragraphs_to_source_offsets() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("mapped.md");
    let source = "# Part I\n\n## Chapter 1\nMapped prose.\n";
    std::fs::write(&path, source).expect("fixture");

    let book = read_book(&path).expect("read Markdown");
    let chapter = &book.root.children[0].children[0];
    assert_eq!(chapter.provenance, Provenance::Authored);
    let heading_range = chapter.source_range.as_ref().expect("heading range");
    assert_eq!(
        heading_range.start,
        SourcePosition::Text { byte_offset: 10 }
    );
    assert_eq!(heading_range.end, SourcePosition::Text { byte_offset: 22 });
    let Block::Paragraph(paragraph) = &chapter.blocks[0] else {
        panic!("expected paragraph block");
    };
    let paragraph_range = paragraph.source_range.as_ref().expect("paragraph range");
    assert_eq!(
        paragraph_range.start,
        SourcePosition::Text { byte_offset: 23 }
    );
    assert_eq!(
        paragraph_range.end,
        SourcePosition::Text {
            byte_offset: source.len() - 1,
        }
    );
}

#[test]
fn markdown_code_does_not_create_navigation_headings() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("code.md");
    std::fs::write(
        &path,
        "    # Indented code\n\n```markdown\n# Fenced code\n```\n\n# Real Heading\nNarrated prose.\n",
    )
    .expect("fixture");

    let book = read_book(&path).expect("read Markdown");

    assert_eq!(book.root.children.len(), 1);
    assert_eq!(book.root.children[0].title.as_deref(), Some("Real Heading"));
    assert!(
        matches!(&book.root.blocks[0], Block::Code(block) if block.text.contains("# Indented code"))
    );
    assert!(
        matches!(&book.root.blocks[1], Block::Code(block) if block.text.contains("# Fenced code"))
    );
}

#[test]
fn does_not_treat_txt_prose_as_a_chapter_heading() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("prose.txt");
    std::fs::write(
        &path,
        "Chapter one explains the model.\nThe paragraph continues here.",
    )
    .expect("fixture");

    let book = read_book(&path).expect("read text");

    assert_eq!(book.root.children[0].kind, SectionKind::BodyMatter);
    assert_eq!(
        book.text,
        "Chapter one explains the model. The paragraph continues here."
    );
}

#[test]
fn rejects_empty_text() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("empty.txt");
    std::fs::write(&path, " \n\t").expect("fixture");

    let error = read_book(&path).expect_err("empty input must fail");
    assert!(error.to_string().contains("contains no readable text"));
}

#[test]
fn rejects_unsupported_input_types() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.docx");
    std::fs::write(&path, b"not a docx").expect("fixture");

    let error = read_book(&path).expect_err("unsupported input must fail");
    assert!(
        error
            .to_string()
            .contains("supported input types: .epub, .azw3, .mobi, .pdf, .html, .md, and .txt")
    );
}

#[test]
fn rejects_invalid_utf8_text() {
    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("book.txt");
    std::fs::write(&path, [0xff, 0xfe]).expect("fixture");

    let error = read_book(&path).expect_err("invalid UTF-8 must fail");
    assert!(error.to_string().contains("failed to read UTF-8 text"));
}
