use std::fs;

use kokoro_book::book::{
    Block, BookMetadata, CanonicalBook, PageMarker, Provenance, Section, SectionKind,
    SourceDocument, SourceFormat, SourcePosition, SourceRange, TextBlock,
};
use kokoro_book::m4b::ChapterPolicy;
use kokoro_book::narration::{FootnoteMode, NarrationPolicy, plan_narration};
use kokoro_book::sidecar::{ManifestOptions, write_audionav, write_manifest};
use kokoro_book::synthesis::{
    MockTtsProvider, SegmentCache, SynthesisSettings, TtsProvider, synthesize_plan,
};
use kokoro_book::timeline::CueKind;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn writes_versioned_navigation_with_toc_pages_paragraphs_sentences_and_sources() {
    let temp = tempdir().expect("temp dir");
    let source = temp.path().join("book.txt");
    fs::write(&source, b"First paragraph.\n\nSecond paragraph.").expect("source fixture");
    let book = mapped_book(source);
    let plan = plan_narration(&book, NarrationPolicy::default());
    let cache = SegmentCache::new(temp.path().join("cache"));
    let mut provider = MockTtsProvider::new("sidecar-voice", 200, 1_000);
    let synthesis = synthesize_plan(&plan, &mut provider, &cache, &SynthesisSettings::default())
        .expect("mock synthesis");
    let output = temp.path().join("book.audionav.json");

    write_audionav(&book, &synthesis.timeline, &output).expect("navigation sidecar");
    let first = fs::read_to_string(&output).expect("navigation JSON");
    write_audionav(&book, &synthesis.timeline, &output).expect("deterministic rewrite");
    assert_eq!(first, fs::read_to_string(&output).expect("rewritten JSON"));
    let json: Value = serde_json::from_str(&first).expect("valid navigation JSON");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["title"], "Mapped Book");
    assert!(json["duration_ms"].as_u64().is_some_and(|value| value > 0));
    assert_eq!(json["toc"][0]["id"], "chapter");
    assert!(json["toc"][0]["start_ms"].is_number());
    assert_eq!(json["pages"][0]["label"], "2");
    assert!(json["pages"][0]["start_ms"].is_number());
    assert_eq!(json["paragraphs"].as_array().map(Vec::len), Some(2));
    assert_eq!(json["sentences"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        json["paragraphs"][0]["source_range"]["start"]["type"],
        "text"
    );
    assert!(
        synthesis
            .timeline
            .cues
            .iter()
            .any(|cue| matches!(&cue.kind, CueKind::Page { label } if label == "2"))
    );
}

#[test]
fn writes_reproducible_manifest_inputs_without_credentials() {
    let temp = tempdir().expect("temp dir");
    let source = temp.path().join("book.txt");
    fs::write(&source, b"First paragraph.\n\nSecond paragraph.").expect("source fixture");
    let book = mapped_book(source);
    let plan = plan_narration(&book, NarrationPolicy::default());
    let cache = SegmentCache::new(temp.path().join("cache"));
    let mut provider = MockTtsProvider::new("manifest-voice", 200, 1_000);
    let synthesis = synthesize_plan(&plan, &mut provider, &cache, &SynthesisSettings::default())
        .expect("mock synthesis");
    let output = temp.path().join("book.manifest.json");
    let options = ManifestOptions {
        provider: provider.identity().clone(),
        speed: 1.0,
        footnotes: FootnoteMode::Inline,
        chapters: ChapterPolicy::Chapters,
        pause_ms: 120,
        max_retries: 2,
        pronunciation_overrides: vec!["Example=ɪɡzˈæmpəl".to_owned()],
        output_files: vec!["book.m4b".into(), "book.audionav.json".into()],
        build_timestamp_unix_seconds: 1_786_406_400,
    };

    write_manifest(&book, &plan, &synthesis, &options, &output).expect("build manifest");
    let text = fs::read_to_string(output).expect("manifest JSON");
    let json: Value = serde_json::from_str(&text).expect("valid manifest JSON");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(
        json["source"]["sha256"],
        "8b3fc5d6135a0b4d6ab515fb6ce3cc503984845fc4ff38a4a91a2dc2e9adec0f"
    );
    assert_eq!(json["source"]["format"], "TXT");
    assert_eq!(json["narration"]["provider"], "mock");
    assert_eq!(json["narration"]["voice"], "manifest-voice");
    assert_eq!(json["narration"]["configuration_hash"], "mock-v1");
    assert_eq!(json["narration"]["pause_ms"], 120);
    assert_eq!(json["narration"]["max_retries"], 2);
    assert_eq!(json["narration"]["phoneme_normalization_version"], 1);
    assert_eq!(json["narration"]["automatic_repairs"]["count"], 0);
    assert_eq!(
        json["narration"]["automatic_repairs"]["by_rule"]["syllabic_consonant"],
        0
    );
    assert_eq!(
        json["narration"]["pronunciation_overrides"][0],
        "Example=ɪɡzˈæmpəl"
    );
    assert_eq!(json["encoding"]["codec"], "aac");
    assert_eq!(json["chapter_policy"], "chapters");
    assert_eq!(json["build_timestamp_unix_seconds"], 1_786_406_400_u64);
    assert!(json["tool"]["version"].as_str().is_some());
    assert!(json["counts"]["narrated_characters"].as_u64().is_some());
    for forbidden in ["api_key", "password", "credential", "secret", "token"] {
        assert!(!text.to_ascii_lowercase().contains(forbidden));
    }
}

fn mapped_book(path: std::path::PathBuf) -> CanonicalBook {
    let mut chapter = Section::new(
        "chapter",
        SectionKind::Chapter,
        Some("Chapter One".to_owned()),
        1,
        Provenance::Authored,
    );
    chapter.source_range = Some(text_range(0, 37));
    chapter.blocks = vec![
        Block::Paragraph(TextBlock {
            text: "First sentence. Another sentence.".to_owned(),
            source_range: Some(text_range(0, 16)),
        }),
        Block::Paragraph(TextBlock {
            text: "Second paragraph.".to_owned(),
            source_range: Some(text_range(18, 35)),
        }),
    ];
    let mut root = Section::new(
        "book",
        SectionKind::Book,
        Some("Mapped Book".to_owned()),
        0,
        Provenance::Derived,
    );
    root.children.push(chapter);
    CanonicalBook {
        metadata: BookMetadata {
            title: Some("Mapped Book".to_owned()),
            authors: vec!["Example Author".to_owned()],
            language: Some("en".to_owned()),
            cover: None,
        },
        root,
        source: SourceDocument {
            path,
            format: SourceFormat::Text,
            format_version: None,
        },
        text: "First paragraph.\n\nSecond paragraph.".to_owned(),
        pages: vec![PageMarker {
            label: "2".to_owned(),
            position: SourcePosition::Text { byte_offset: 17 },
        }],
        warnings: vec!["fixture warning".to_owned()],
    }
}

fn text_range(start: usize, end: usize) -> SourceRange {
    SourceRange {
        source_id: "book.txt".to_owned(),
        start: SourcePosition::Text { byte_offset: start },
        end: SourcePosition::Text { byte_offset: end },
    }
}
