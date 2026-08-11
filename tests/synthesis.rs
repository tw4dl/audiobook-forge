use std::fs;

use kokoro_book::book::{
    Block, BookMetadata, CanonicalBook, Provenance, Section, SectionKind, SourceDocument,
    SourceFormat, TextBlock,
};
use kokoro_book::narration::{NarrationPolicy, plan_narration};
use kokoro_book::preflight::{PreparedNarration, PreparedNarrationUnit};
use kokoro_book::synthesis::{
    MockTtsProvider, PhonemeNormalizationReport, SegmentCache, SynthesisSettings, TtsInputMode,
    synthesize_plan,
};
use kokoro_book::timeline::CueKind;
use tempfile::tempdir;

#[test]
fn builds_sentence_and_paragraph_timing_from_real_sample_boundaries() {
    let temp = tempdir().expect("temp dir");
    let plan = plan_narration(&two_paragraph_book(), NarrationPolicy::default());
    let cache = SegmentCache::new(temp.path().join("cache"));
    let mut provider = MockTtsProvider::new("mock-voice", 8, 1_000);

    let result = synthesize_plan(
        &plan,
        &mut provider,
        &cache,
        &SynthesisSettings {
            pause_ms: 20,
            ..SynthesisSettings::default()
        },
    )
    .expect("mock synthesis");

    assert!(provider.requests().len() > plan.units.len());
    assert_eq!(
        result
            .timeline
            .cues
            .iter()
            .filter(|cue| cue.kind == CueKind::Sentence)
            .count(),
        3
    );
    assert_eq!(
        result
            .timeline
            .cues
            .iter()
            .filter(|cue| cue.kind == CueKind::Paragraph)
            .count(),
        2
    );
    assert!(result.timeline.duration_ms > 0);
    assert!(result.timeline.cues.iter().all(|cue| {
        cue.end_ms
            .is_some_and(|end_ms| end_ms >= cue.start_ms && end_ms <= result.timeline.duration_ms)
    }));
}

#[test]
fn reuses_valid_cached_chunks_and_invalidates_narration_changes() {
    let temp = tempdir().expect("temp dir");
    let plan = plan_narration(&two_paragraph_book(), NarrationPolicy::default());
    let cache = SegmentCache::new(temp.path().join("cache"));
    let settings = SynthesisSettings::default();
    let mut first = MockTtsProvider::new("voice-a", 200, 1_000);
    let first_result = synthesize_plan(&plan, &mut first, &cache, &settings).expect("first build");
    assert_eq!(first_result.cache_hits, 0);
    assert!(first_result.generated_chunks > 0);

    let mut same = MockTtsProvider::new("voice-a", 200, 1_000);
    let resumed = synthesize_plan(&plan, &mut same, &cache, &settings).expect("cached build");
    assert_eq!(same.requests().len(), 0);
    assert_eq!(resumed.cache_hits, first_result.generated_chunks);

    let mut changed_voice = MockTtsProvider::new("voice-b", 200, 1_000);
    let changed = synthesize_plan(&plan, &mut changed_voice, &cache, &settings)
        .expect("changed narration build");
    assert_eq!(changed.cache_hits, 0);
    assert!(!changed_voice.requests().is_empty());

    let mut changed_provider = MockTtsProvider::new("voice-a", 200, 1_000).with_provider("qwen");
    let changed = synthesize_plan(&plan, &mut changed_provider, &cache, &settings)
        .expect("changed provider build");
    assert_eq!(changed.cache_hits, 0);
    assert!(!changed_provider.requests().is_empty());
}

#[test]
fn a_failed_build_resumes_after_the_last_atomic_cached_chunk() {
    let temp = tempdir().expect("temp dir");
    let plan = plan_narration(&two_paragraph_book(), NarrationPolicy::default());
    let cache = SegmentCache::new(temp.path().join("cache"));
    let settings = SynthesisSettings {
        max_retries: 0,
        ..SynthesisSettings::default()
    };
    let mut failing = MockTtsProvider::new("resume-voice", 200, 1_000);
    failing.fail_on_request(2);

    let error = synthesize_plan(&plan, &mut failing, &cache, &settings)
        .expect_err("second request must fail");
    assert!(format!("{error:#}").contains("request 2"));

    let mut resumed = MockTtsProvider::new("resume-voice", 200, 1_000);
    let result = synthesize_plan(&plan, &mut resumed, &cache, &settings).expect("resume build");
    assert!(result.cache_hits >= 1);
    assert_eq!(
        resumed.requests().len() + result.cache_hits,
        result.provider_chunks
    );
}

#[test]
fn corrupt_cached_audio_is_rebuilt_instead_of_reused() {
    let temp = tempdir().expect("temp dir");
    let plan = plan_narration(&two_paragraph_book(), NarrationPolicy::default());
    let cache = SegmentCache::new(temp.path().join("cache"));
    let settings = SynthesisSettings::default();
    let mut first = MockTtsProvider::new("cache-voice", 200, 1_000);
    let result = synthesize_plan(&plan, &mut first, &cache, &settings).expect("first build");
    let cached_path = result.rendered_units[0].chunks[0].path.clone();
    fs::write(&cached_path, b"corrupt").expect("corrupt cache fixture");

    let mut rebuilt = MockTtsProvider::new("cache-voice", 200, 1_000);
    let rebuilt_result =
        synthesize_plan(&plan, &mut rebuilt, &cache, &settings).expect("rebuild cache");

    assert_eq!(
        rebuilt_result.cache_hits + 1,
        rebuilt_result.provider_chunks
    );
    assert_eq!(rebuilt.requests().len(), 1);
}

#[test]
fn consumes_prepared_phonemes_without_repeating_text_phonemization() {
    let temp = tempdir().expect("temp dir");
    let plan = plan_narration(&two_paragraph_book(), NarrationPolicy::default());
    let prepared = PreparedNarration {
        schema_version: 1,
        complete: true,
        source_sha256: "source".to_owned(),
        profile: "mock".to_owned(),
        provider_configuration_hash: "mock-v1".to_owned(),
        normalization_version: 1,
        max_phonemes: 200,
        max_characters: 200,
        units: plan
            .units
            .iter()
            .map(|unit| PreparedNarrationUnit {
                unit_id: unit.id.clone(),
                section_id: unit.section_id.clone(),
                status: "ready".to_owned(),
                original_text: unit.original_text.clone(),
                tts_text: unit.text.clone(),
                sentences: Vec::new(),
                phoneme_chunks: vec!["prepared".to_owned()],
                source_range: unit.source_range.clone(),
                text_normalization: unit.normalization.clone(),
                phoneme_normalization: PhonemeNormalizationReport::default(),
                repairs: Vec::new(),
            })
            .collect(),
    };
    let mut provider = MockTtsProvider::new("mock-voice", 200, 1_000)
        .with_input_mode(TtsInputMode::PreparedPhonemes);
    let result = synthesize_plan(
        &plan,
        &mut provider,
        &SegmentCache::new(temp.path().join("cache")),
        &SynthesisSettings {
            prepared: Some(prepared),
            ..SynthesisSettings::default()
        },
    )
    .expect("prepared synthesis");

    assert_eq!(result.provider_chunks, plan.units.len());
    assert!(provider.requests().iter().all(|request| {
        request
            .phoneme_chunks
            .as_ref()
            .is_some_and(|chunks| chunks == &vec!["prepared".to_owned()])
    }));
}

#[test]
fn raw_text_provider_uses_prepared_text_without_kokoro_phonemes() {
    let temp = tempdir().expect("temp dir");
    let plan = plan_narration(&two_paragraph_book(), NarrationPolicy::default());
    let prepared = PreparedNarration {
        schema_version: 1,
        complete: true,
        source_sha256: "source".to_owned(),
        profile: "mock".to_owned(),
        provider_configuration_hash: "mock-v1".to_owned(),
        normalization_version: 1,
        max_phonemes: 200,
        max_characters: 12,
        units: plan
            .units
            .iter()
            .map(|unit| PreparedNarrationUnit {
                unit_id: unit.id.clone(),
                section_id: unit.section_id.clone(),
                status: "ready".to_owned(),
                original_text: unit.original_text.clone(),
                tts_text: unit.text.clone(),
                sentences: Vec::new(),
                phoneme_chunks: vec!["kokoro-only-1".to_owned(), "kokoro-only-2".to_owned()],
                source_range: unit.source_range.clone(),
                text_normalization: unit.normalization.clone(),
                phoneme_normalization: PhonemeNormalizationReport::default(),
                repairs: Vec::new(),
            })
            .collect(),
    };
    let mut provider = MockTtsProvider::new("raw-text-voice", 12, 1_000);
    let result = synthesize_plan(
        &plan,
        &mut provider,
        &SegmentCache::new(temp.path().join("cache")),
        &SynthesisSettings {
            prepared: Some(prepared),
            ..SynthesisSettings::default()
        },
    )
    .expect("raw-text synthesis");

    assert!(result.provider_chunks > plan.units.len());
    assert!(
        provider
            .requests()
            .iter()
            .all(|request| request.phoneme_chunks.is_none())
    );
    assert!(provider.requests().iter().all(|request| {
        !request.text.contains("kokoro-only") && request.text.chars().count() <= 12
    }));
}

fn two_paragraph_book() -> CanonicalBook {
    let mut chapter = Section::new(
        "chapter",
        SectionKind::Chapter,
        Some("Chapter".to_owned()),
        1,
        Provenance::Authored,
    );
    chapter.blocks = vec![
        Block::Paragraph(TextBlock {
            text: "First sentence. Second sentence.".to_owned(),
            source_range: None,
        }),
        Block::Paragraph(TextBlock {
            text: "Third sentence.".to_owned(),
            source_range: None,
        }),
    ];
    let mut root = Section::new(
        "book",
        SectionKind::Book,
        Some("Timing Book".to_owned()),
        0,
        Provenance::Derived,
    );
    root.children.push(chapter);
    CanonicalBook {
        metadata: BookMetadata::default(),
        root,
        source: SourceDocument {
            path: "timing.txt".into(),
            format: SourceFormat::Text,
            format_version: None,
        },
        text: "raw".to_owned(),
        pages: Vec::new(),
        warnings: Vec::new(),
    }
}
