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
fn collapses_spaced_ellipsis_without_creating_punctuation_only_units() {
    let spoken =
        normalize_for_speech("“But\u{00a0}.\u{00a0}.\u{00a0}. But\u{00a0}.\u{00a0}.\u{00a0}.”");

    assert_eq!(spoken, "\"But ... But ...\"");
    assert!(
        kokoro_book::pipeline::extract_sentences(&spoken)
            .iter()
            .all(|sentence| sentence
                .trim_matches(['.', '"'])
                .chars()
                .any(char::is_alphabetic))
    );
}

#[test]
fn speaks_currency_and_math_symbols_without_mutating_source_text() {
    let raw = "($12,000 × 25 = $300,000)";

    let spoken = normalize_for_speech(raw);

    assert!(spoken.contains("dollars"));
    assert!(spoken.contains("times"));
    assert!(spoken.contains("equals"));
    assert!(!spoken.contains('$'));
    assert!(!spoken.contains('×'));
    assert!(!spoken.contains('='));
    assert_eq!(raw, "($12,000 × 25 = $300,000)");
}

#[test]
fn removes_ignorable_zero_width_source_marks_without_mutating_source_text() {
    let raw = "ra\u{200b}dio and co\u{200d}operate";

    assert_eq!(normalize_for_speech(raw), "radio and cooperate");
    assert_eq!(raw, "ra\u{200b}dio and co\u{200d}operate");
}

#[test]
fn expands_copyright_symbol_without_mutating_source_text() {
    let raw = "© Blue Glass Photography";

    assert_eq!(
        normalize_for_speech(raw),
        "copyright Blue Glass Photography"
    );
    assert_eq!(raw, "© Blue Glass Photography");
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

#[test]
fn skips_non_narrative_back_matter_by_default() {
    let mut book = semantic_book();
    for (id, kind, title, text) in [
        (
            "notes",
            SectionKind::Notes,
            "Notes",
            "Endnotes should not be narrated.",
        ),
        (
            "credits",
            SectionKind::BackMatter,
            "Illustration Credits",
            "Credit text should not be narrated.",
        ),
        (
            "index",
            SectionKind::Index,
            "Index",
            "Index entries should not be narrated.",
        ),
        (
            "publisher",
            SectionKind::BackMatter,
            "Connect with HMH",
            "Publisher promotion should not be narrated.",
        ),
    ] {
        let mut section = Section::new(id, kind, Some(title.to_owned()), 2, Provenance::Authored);
        section.blocks.push(Block::Paragraph(TextBlock {
            text: text.to_owned(),
            source_range: None,
        }));
        book.root.children.push(section);
    }

    let plan = plan_narration(&book, NarrationPolicy::default());
    let spoken = plan
        .units
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    for excluded in [
        "Endnotes should not be narrated",
        "Credit text should not be narrated",
        "Index entries should not be narrated",
        "Publisher promotion should not be narrated",
    ] {
        assert!(
            !spoken.contains(excluded),
            "unexpected narration: {excluded}"
        );
    }
    assert_eq!(
        plan.warnings
            .iter()
            .filter(|warning| warning.contains("default narration policy"))
            .count(),
        4
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
