use kokoro_book::book::{
    Block, BookAsset, BookMetadata, CanonicalBook, Provenance, Section, SectionKind,
    SourceDocument, SourceFormat, TextBlock,
};
use kokoro_book::m4b::{ChapterPolicy, assemble_m4b, select_chapters, validate_m4b};
use kokoro_book::narration::{NarrationPolicy, plan_narration};
use kokoro_book::synthesis::{MockTtsProvider, SegmentCache, SynthesisSettings, synthesize_plan};
use tempfile::tempdir;

#[test]
fn exports_and_independently_validates_aac_metadata_cover_and_ordered_chapters() {
    if !tools_available() {
        eprintln!("skipped: ffmpeg and ffprobe are required");
        return;
    }
    let temp = tempdir().expect("temp dir");
    let book = chapter_book();
    let plan = plan_narration(&book, NarrationPolicy::default());
    let cache = SegmentCache::new(temp.path().join("cache"));
    let mut provider = MockTtsProvider::new("mock-reader", 200, 8_000);
    let synthesis = synthesize_plan(
        &plan,
        &mut provider,
        &cache,
        &SynthesisSettings {
            pause_ms: 10,
            ..SynthesisSettings::default()
        },
    )
    .expect("mock synthesis");
    let output = temp.path().join("book.m4b");

    let assembled =
        assemble_m4b(&book, &synthesis, &output, ChapterPolicy::Chapters).expect("M4B export");
    let inspected = validate_m4b(&output).expect("independent ffprobe validation");

    assert_eq!(assembled, inspected);
    assert_eq!(inspected.codec, "aac");
    assert_eq!(
        inspected.title.as_deref(),
        Some("Public # Domain = Test; Edition")
    );
    assert_eq!(inspected.artist.as_deref(), Some("Ada Author"));
    assert!(inspected.has_cover);
    assert!(inspected.duration_ms > 0);
    assert_eq!(
        inspected
            .chapters
            .iter()
            .map(|chapter| chapter.title.as_str())
            .collect::<Vec<_>>(),
        ["Chapter One", "Chapter Two"]
    );
    assert!(
        inspected.chapters.windows(2).all(|pair| {
            pair[0].start_ms < pair[1].start_ms && pair[0].end_ms <= pair[1].start_ms
        })
    );
}

#[test]
fn visible_chapter_policy_keeps_subsections_out_of_default_navigation() {
    let book = chapter_book();
    let plan = plan_narration(&book, NarrationPolicy::default());
    let temp = tempdir().expect("temp dir");
    let cache = SegmentCache::new(temp.path().join("cache"));
    let mut provider = MockTtsProvider::new("mock-reader", 200, 1_000);
    let synthesis = synthesize_plan(&plan, &mut provider, &cache, &SynthesisSettings::default())
        .expect("mock synthesis");

    let chapters = select_chapters(&book, &synthesis.timeline, ChapterPolicy::Chapters);
    let sections = select_chapters(&book, &synthesis.timeline, ChapterPolicy::Sections);

    assert_eq!(chapters.len(), 2);
    assert_eq!(sections.len(), 3);
    assert_eq!(sections[1].title, "A Useful Subsection");
}

fn chapter_book() -> CanonicalBook {
    let mut first = Section::new(
        "chapter-one",
        SectionKind::Chapter,
        Some("Chapter One".to_owned()),
        1,
        Provenance::Authored,
    );
    first.blocks.push(paragraph("First body."));
    let mut subsection = Section::new(
        "subsection",
        SectionKind::Section,
        Some("A Useful Subsection".to_owned()),
        2,
        Provenance::Authored,
    );
    subsection.blocks.push(paragraph("Subsection body."));
    first.children.push(subsection);
    let mut second = Section::new(
        "chapter-two",
        SectionKind::Chapter,
        Some("Chapter Two".to_owned()),
        1,
        Provenance::Authored,
    );
    second.blocks.push(paragraph("Second body."));
    let mut root = Section::new(
        "book",
        SectionKind::Book,
        Some("Public # Domain = Test; Edition".to_owned()),
        0,
        Provenance::Derived,
    );
    root.children = vec![first, second];
    CanonicalBook {
        metadata: BookMetadata {
            title: Some("Public # Domain = Test; Edition".to_owned()),
            authors: vec!["Ada Author".to_owned()],
            language: Some("en".to_owned()),
            cover: Some(BookAsset {
                source_id: "cover.png".to_owned(),
                media_type: "image/png".to_owned(),
                bytes: COVER_PNG.to_vec(),
            }),
        },
        root,
        source: SourceDocument {
            path: "public-domain.epub".into(),
            format: SourceFormat::Epub,
            format_version: Some("3.0".to_owned()),
        },
        text: "raw".to_owned(),
        pages: Vec::new(),
        warnings: Vec::new(),
    }
}

fn paragraph(text: &str) -> Block {
    Block::Paragraph(TextBlock {
        text: text.to_owned(),
        source_range: None,
    })
}

fn tools_available() -> bool {
    ["ffmpeg", "ffprobe"].into_iter().all(|tool| {
        std::process::Command::new(tool)
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

const COVER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c,
    0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00,
    0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x82,
];
