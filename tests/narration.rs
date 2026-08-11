use kokoro_book::book::{
    Block, BookMetadata, CanonicalBook, ListBlock, Provenance, Section, SectionKind,
    SourceDocument, SourceFormat, TextBlock,
};
use kokoro_book::narration::{FootnoteMode, NarrationPolicy, normalize_for_speech, plan_narration};

#[test]
fn normalizes_document_text_for_speech_without_changing_source_content() {
    let raw = "Soft\u{00ad}ware—see https://example.com/a-b [12].\nwrapped hy-\nphen.";

    let spoken = normalize_for_speech(raw);

    assert!(!spoken.contains('\u{00ad}'));
    assert!(!spoken.contains("https://"));
    assert!(!spoken.contains("[12]"));
    assert!(spoken.contains("example dot com slash a dash b"));
    assert!(spoken.contains("hyphen"));
    assert_eq!(
        raw,
        "Soft\u{00ad}ware—see https://example.com/a-b [12].\nwrapped hy-\nphen."
    );
}

#[test]
fn narration_uses_semantic_blocks_and_hides_source_navigation_and_page_numbers() {
    let book = semantic_book();

    let plan = plan_narration(&book, NarrationPolicy::default());
    let spoken = plan
        .units
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(spoken.contains("Chapter One"));
    assert!(spoken.contains("First item. Second item."));
    assert!(spoken.contains("Footnote."));
    assert!(!spoken.contains("Source table of contents"));
    assert!(!spoken.contains(" 17 "));
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.contains("navigation"))
    );
}

#[test]
fn footnotes_can_be_skipped_or_moved_to_the_end() {
    let book = semantic_book();

    let skipped = plan_narration(
        &book,
        NarrationPolicy {
            footnotes: FootnoteMode::Skip,
        },
    );
    assert!(
        skipped
            .units
            .iter()
            .all(|unit| !unit.text.contains("Citation note"))
    );

    let at_end = plan_narration(
        &book,
        NarrationPolicy {
            footnotes: FootnoteMode::End,
        },
    );
    assert!(
        at_end
            .units
            .last()
            .is_some_and(|unit| unit.text.contains("Citation note"))
    );
}

fn semantic_book() -> CanonicalBook {
    let mut chapter = Section::new(
        "chapter-one",
        SectionKind::Chapter,
        Some("Chapter One".to_owned()),
        1,
        Provenance::Authored,
    );
    chapter.blocks = vec![
        Block::Paragraph(TextBlock {
            text: "17".to_owned(),
            source_range: None,
        }),
        Block::Paragraph(TextBlock {
            text: "A body paragraph.".to_owned(),
            source_range: None,
        }),
        Block::List(ListBlock {
            ordered: true,
            items: vec!["First item".to_owned(), "Second item".to_owned()],
            text: "First item. Second item".to_owned(),
            source_range: None,
        }),
        Block::Footnote(TextBlock {
            text: "Citation note [12].".to_owned(),
            source_range: None,
        }),
        Block::Navigation(TextBlock {
            text: "Source table of contents".to_owned(),
            source_range: None,
        }),
    ];
    let mut root = Section::new(
        "book",
        SectionKind::Book,
        Some("Test Book".to_owned()),
        0,
        Provenance::Derived,
    );
    root.children.push(chapter);
    CanonicalBook {
        metadata: BookMetadata {
            title: Some("Test Book".to_owned()),
            ..BookMetadata::default()
        },
        root,
        source: SourceDocument {
            path: "test.txt".into(),
            format: SourceFormat::Text,
            format_version: None,
        },
        text: "raw source text".to_owned(),
        pages: Vec::new(),
        warnings: Vec::new(),
    }
}
