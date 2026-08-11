use std::fs;

use kokoro_book::build::{AudiobookBuildOptions, build_audiobook};
use kokoro_book::input::read_book;
use kokoro_book::m4b::{ChapterPolicy, validate_m4b};
use kokoro_book::narration::{FootnoteMode, NarrationPolicy};
use kokoro_book::synthesis::{MockTtsProvider, SegmentCache, SynthesisSettings};
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn public_domain_text_runs_the_full_mock_audiobook_pipeline_and_resumes() {
    if !media_tools_available() {
        eprintln!("skipped: ffmpeg and ffprobe are required");
        return;
    }
    let temp = tempdir().expect("temp dir");
    let source = std::path::Path::new("tests/fixtures/on-the-eve-elena.txt");
    let book = read_book(source).expect("public-domain fixture imports");
    let output = temp.path().join("on-the-eve-elena");
    let cache = SegmentCache::new(temp.path().join("segments"));
    let options = build_options(output.clone(), "on-the-eve-elena");
    let mut provider = MockTtsProvider::new("public-domain-reader", 80, 8_000);

    let first = build_audiobook(&book, &mut provider, &cache, &options).expect("full build");

    assert_eq!(first.m4b_path, output.join("on-the-eve-elena.m4b"));
    assert!(first.audionav_path.is_file());
    assert!(first.manifest_path.is_file());
    assert!(first.cover_path.is_none());
    assert!(first.synthesis.generated_chunks > 0);
    let inspected = validate_m4b(&first.m4b_path).expect("independent M4B validation");
    assert_eq!(inspected.codec, "aac");
    assert_eq!(inspected.chapters.len(), 1);
    let navigation: Value =
        serde_json::from_slice(&fs::read(&first.audionav_path).expect("audionav"))
            .expect("valid audionav JSON");
    assert_eq!(navigation["schema_version"], 1);
    assert!(
        navigation["sentences"]
            .as_array()
            .is_some_and(|sentences| sentences.len() >= 3)
    );
    let manifest = fs::read_to_string(&first.manifest_path).expect("manifest");
    assert!(manifest.contains("public-domain-reader"));
    assert!(!manifest.to_ascii_lowercase().contains("api_key"));

    let mut resumed_provider = MockTtsProvider::new("public-domain-reader", 80, 8_000);
    let resumed = build_audiobook(&book, &mut resumed_provider, &cache, &options)
        .expect("resumed full build");
    assert_eq!(resumed.synthesis.generated_chunks, 0);
    assert_eq!(
        resumed.synthesis.cache_hits,
        resumed.synthesis.provider_chunks
    );
    assert!(resumed_provider.requests().is_empty());
}

#[test]
fn full_build_exports_source_cover_as_jpeg_and_embeds_it() {
    if !media_tools_available() {
        eprintln!("skipped: ffmpeg and ffprobe are required");
        return;
    }
    let temp = tempdir().expect("temp dir");
    let book =
        read_book("tests/fixtures/kindle/with-cover.azw3".as_ref()).expect("cover fixture imports");
    let output = temp.path().join("kindle-cover");
    let cache = SegmentCache::new(temp.path().join("segments"));
    let options = build_options(output.clone(), "kindle-cover");
    let mut provider = MockTtsProvider::new("cover-reader", 200, 8_000);

    let report =
        build_audiobook(&book, &mut provider, &cache, &options).expect("cover build succeeds");

    let cover = report.cover_path.expect("exported cover");
    assert_eq!(cover, output.join("cover.jpg"));
    assert!(
        fs::read(cover)
            .expect("cover bytes")
            .starts_with(&[0xff, 0xd8, 0xff])
    );
    assert!(report.m4b.has_cover);
    assert_eq!(report.m4b.chapters.len(), 2);
}

fn build_options(output_dir: std::path::PathBuf, base_name: &str) -> AudiobookBuildOptions {
    AudiobookBuildOptions {
        output_dir,
        base_name: base_name.to_owned(),
        chapters: ChapterPolicy::Chapters,
        narration: NarrationPolicy {
            footnotes: FootnoteMode::Inline,
        },
        synthesis: SynthesisSettings {
            speed: 1.0,
            pause_ms: 20,
            max_retries: 1,
            prepared: None,
        },
        pronunciation_overrides: vec!["Elena=ɪlˈeɪnə".to_owned()],
        build_timestamp_unix_seconds: 1_786_406_400,
    }
}

fn media_tools_available() -> bool {
    ["ffmpeg", "ffprobe"].into_iter().all(|tool| {
        std::process::Command::new(tool)
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}
