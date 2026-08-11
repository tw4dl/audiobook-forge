# V1 acceptance record

Status: 44 of 44 product criteria implemented and covered by code, tests, or the named-player check. The verification commands are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo deny check
```

## Core architecture

- [x] A format-neutral `CanonicalBook` exists. See `src/book.rs` and importer tests.
- [x] Input importers are separate from TTS and audio output. See `src/input.rs`, `src/build.rs`, and `src/tts.rs`.
- [x] An `AudioTimeline` maps source content to audio. See `src/timeline.rs` and `builds_sentence_and_paragraph_timing_from_real_sample_boundaries`.
- [x] Semantic structure and audio timing are separate. `CanonicalBook` has no audio state; `AudioTimeline` has no format parser state.

## Input support

- [x] EPUB 2 and EPUB 3 import reliably. `tests/epub.rs`, `tests/epub_fallback.rs`, and `tests/input_security.rs` cover metadata, navigation, spine order, pages, notes, fallbacks, UTF-16, malformed input, and bounds.
- [x] HTML and XHTML import reliably. `tests/input.rs` covers HTML recovery, semantic blocks, script omission, XHTML CDATA, and self-closing XML elements.
- [x] Markdown imports reliably. `tests/input.rs` covers heading levels, paragraphs, source offsets, and fenced code.
- [x] TXT uses deterministic chapter inference. `tests/input.rs` and CLI tree tests cover `PART`, `BOOK`, and `CHAPTER` patterns.
- [x] Text-based PDF imports. `tests/pdf.rs` covers metadata, pages, labels, encrypted input, malformed input, stream limits, and the no-OCR error.
- [x] PDF bookmarks and outlines win when available. `imports_text_pdf_with_metadata_pages_and_bookmark_hierarchy` verifies their hierarchy.
- [x] DRM-free AZW3 has an implemented importer path. `tests/kindle.rs` covers KF8 metadata, HTML, navigation, cover, bounds, and documented limits.
- [x] DRM-free MOBI has an implemented importer path. `tests/kindle.rs` covers legacy navigation, reading order, uncompressed text, and PalmDOC compression.
- [x] Unsupported DRM and encryption fail clearly. EPUB, PDF, and Kindle security tests verify failure before synthesis.

## Inspection

- [x] `inspect` prints metadata and detected sections. CLI tests cover TXT, EPUB, PDF, and AZW3.
- [x] `inspect --tree` prints the semantic hierarchy. CLI tests cover nested TXT, Markdown, HTML, PDF, and AZW3 trees.
- [x] Inspection needs no TTS credentials or model. CLI tests set an empty cache and verify that it stays absent.
- [x] Structural warnings print before synthesis. `conversion_reports_structural_warnings_before_synthesis` verifies ordering before a pre-model settings error.

## Narration

- [x] Narration text is normalized without changing imported source text. `normalizes_document_text_for_speech_without_changing_source_content` covers whitespace, hyphens, punctuation, citations, and URLs.
- [x] Headings, paragraphs, lists, figures, navigation blocks, code, and notes have separate semantic behavior. See `src/narration.rs` and `tests/narration.rs`.
- [x] Provider chunk limits do not appear in user navigation. `visible_chapter_policy_keeps_subsections_out_of_default_navigation` and synthesis timing tests keep provider chunks below the timeline layer.
- [x] Interrupted builds resume from valid atomic cache entries. `a_failed_build_resumes_after_the_last_atomic_cached_chunk` and the full-build resume test verify this.

## Audiobook

- [x] Default conversion creates one M4B. `src/cli.rs`, `tests/build.rs`, and the real public CLI proof cover this path.
- [x] M4B chapters use semantic divisions. M4B and full-build tests verify names, count, order, and valid bounds.
- [x] Cover art is embedded when available. M4B and full-build Kindle-cover tests inspect the attached picture stream.
- [x] Title and author metadata are preserved. Independent FFprobe validation checks both fields.
- [x] The output plays in a mainstream player. VLC 3.0.23 opened and advanced through a real Kokoro AAC build.
- [x] Named-player chapter navigation works. `PLAYER_COMPATIBILITY.md` records forward and backward VLC chapter selection with the exact fixture, versions, hash, and transcript.

## Rich navigation

- [x] Every complete build writes `book.audionav.json`. `tests/build.rs` verifies the file.
- [x] The navigation schema starts at version 1. `tests/sidecars.rs` checks `schema_version`.
- [x] Chapter timestamps are present. Sidecar and full-build tests check timed TOC entries.
- [x] Page timestamps are present for meaningful source pages. Sidecar tests check authored page cues; EPUB and PDF tests check source page import.
- [x] Paragraph mappings are present. Sidecar and synthesis tests check paragraph cue bounds.
- [x] Sentence mappings are present when segmentation supports them. Sidecar and synthesis tests check sentence cue bounds.
- [x] Source ranges survive into navigation JSON. Sidecar tests check format-neutral serialized positions.

## Reproducibility

- [x] Every complete build writes `book.manifest.json`. `tests/build.rs` verifies the file.
- [x] The manifest records the source SHA-256. `tests/sidecars.rs` checks the exact digest.
- [x] Tool and importer versions are recorded. Manifest serialization tests check both.
- [x] Narration configuration is recorded. Provider, pinned model, voice, language, configuration hash, speed, notes, pauses, retries, pronunciation overrides, character limit, and sample rate are serialized.
- [x] No credentials or secrets are written. The provider is local, the manifest has an explicit field allowlist, and tests scan the output for credential names.

## Quality

- [x] A public-domain EPUB fixture produces correct chapter order. `public_domain_epub_fixture_produces_correct_chapter_ordering` packages the documented *On the Eve* passage with a reversed TOC and verifies spine order.
- [x] A PDF bookmark fixture uses bookmark structure. `imports_text_pdf_with_metadata_pages_and_bookmark_hierarchy` verifies nested authored outline entries.
- [x] A TXT fixture detects common chapter patterns. Input and CLI tree tests cover parts, books, numbered chapters, and word-number chapters.
- [x] CI needs no paid TTS credentials. `.github/workflows/ci.yml` installs FFmpeg and runs the mock-provider suite on Linux. Native MLX remains a local Apple Silicon gate.
- [x] Automated M4B structural validation exists. `validate_m4b` uses independent FFprobe output and checks AAC, duration, metadata, cover, chapter order, and bounds.
