# audiobook-forge

[![CI](https://github.com/tw4dl/audiobook-forge/actions/workflows/ci.yml/badge.svg)](https://github.com/tw4dl/audiobook-forge/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A local-first audiobook forge. The `audiobook-forge` CLI turns an English EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or UTF-8 TXT file into a navigable M4B audiobook. Kokoro is the default provider. An opt-in Qwen3-TTS provider adds a larger English voice model on Apple Silicon.

Navigation is a primary feature. `audiobook-forge` keeps authored parts, chapters, sections, pages, paragraphs, sentences, and source locations separate from internal TTS chunks. The M4B exposes useful player chapters. A richer sidecar keeps the detail needed for later page, paragraph, search, and synchronized-text navigation.

The default Kokoro path stays native Rust with one isolated MLX worker and no Python. The optional Qwen path uses one isolated Python/MLX worker. Neither path needs a server, GUI, API key, eSpeak, ONNX Runtime, or voice cloning.

## Install

You need:

- Apple Silicon macOS
- Rust 1.88 or newer
- Xcode Command Line Tools
- CMake
- FFmpeg, including `ffprobe`

```sh
brew install cmake ffmpeg
cargo install --git https://github.com/tw4dl/audiobook-forge --locked --root "$HOME"
```

This installs the executable at `~/bin/audiobook-forge`. Add `~/bin` to your
`PATH` if you want to invoke it without the full path.

The first conversion downloads one pinned 312 MiB BF16 Kokoro model and the selected 510 KiB voice. The two files total 312.5 MiB. Later runs work offline. Each extra voice adds about 510 KiB when first used.

Qwen is optional and experimental. It uses the pinned 6-bit `Qwen3-TTS-12Hz-0.6B-CustomVoice` model through `mlx-audio`. Install its isolated Python 3.12 runtime with:

```sh
brew install uv
scripts/setup-qwen-runtime.sh
```

The first Qwen run downloads 1.83 GB through the Hugging Face cache. The runtime source and model revisions are pinned. Qwen currently supports `Aiden` and `Ryan`, English, and speed `1.0`.

Qwen uses sampled generation with stable text-derived seeds. Its token budget scales with request length. A result that reaches the token ceiling is rejected instead of being cached as long unusable audio. See [QWEN_BENCHMARK.md](QWEN_BENCHMARK.md) for the full-chapter `Die with Zero` comparison, failure analysis, and blind listening clips.

## Use

Conversion is always explicit: use `convert` before the input path. A bare
`audiobook-forge` invocation prints the command list and exits with an error;
it never starts synthesis by accident.

```sh
audiobook-forge convert book.epub
audiobook-forge inspect book.epub
audiobook-forge inspect reference.pdf --tree
audiobook-forge inspect reference.azw3 --tree
audiobook-forge inspect notes.md --tree
audiobook-forge convert notes.txt --output ./notes-audiobook
audiobook-forge convert book.epub --voice af_sky --speed 1.1 --nav chapters
audiobook-forge convert book.epub --footnotes end
audiobook-forge voices
audiobook-forge convert book.epub --provider qwen
audiobook-forge convert book.epub --provider qwen --voice Ryan

# Compile narration before starting a TTS worker
audiobook-forge preflight book.epub --output prepared/
audiobook-forge convert book.epub --prepared prepared/
```

For `book.epub`, the default output is:

```text
book/
├── book.m4b
├── book.audionav.json
├── book.manifest.json
└── cover.jpg              optional
```

`--output DIR` changes the output directory. The base file name still comes from the source. The M4B contains mono 64 kbit/s AAC audio, title and author metadata, an embedded cover when present, and stable chapter markers. Kokoro is the default provider with voice `af_heart`. Qwen defaults to `Aiden`. Run `audiobook-forge voices` for the 28 Kokoro English presets.

`preflight` is a zero-audio Kokoro narration compiler. It applies exclusions and deterministic speech repairs, validates every sentence with Misaki and the Kokoro vocabulary, and writes `book.preflight.json`, `book.narration.jsonl`, `book.pronunciations.txt`, and `chapters/*.txt` under the prepared directory. Each narration record keeps an `id`, `status`, `original_text`, TTS-ready `tts_text`, source ranges, repair records, and validated phoneme chunks. A Kokoro build runs the same preflight automatically before it loads MLX. `--prepared DIR` consumes a matching JSONL artifact and does not rerun G2P. Use `--strict-preflight` to reject best-effort pronunciation fallbacks. Qwen uses normalized narration text directly; `--prepared`, `--strict-preflight`, `--pronunciation`, and non-`1.0` speed are not supported for Qwen yet.

`inspect` reports imported metadata and semantic sections without loading or downloading the TTS model. EPUB inspection includes the format version, author, language, cover, and authored page count when present. Explicit creator roles keep non-author contributors out of the author list. Use `--tree` to show the nested navigation structure. EPUB 3 navigation documents and EPUB 2 NCX files take priority over spine XHTML headings. EPUB 3 token lists and image alternative labels remain usable navigation. Page lists take priority over EPUB 3 `pagebreak` markers. The importer accepts UTF-8 and BOM-marked UTF-16 XML, follows manifest fallback chains around foreign or scripted resources, reads SVG text as source-mapped blocks, and preserves cover bytes, semantic containers, footnotes, source resources, source fragments, and spine order in `CanonicalBook`. Broken navigation links become warnings and do not hide readable spine content.

PDF inspection preserves title, author, PDF version, physical page number, and authored page label. Outline/bookmark hierarchy takes priority over deterministic `PART`, `BOOK`, and `CHAPTER` heading inference. A PDF with no usable outline reports the lower-confidence fallback before synthesis. Password-protected PDFs fail with a clear DRM-free-copy message. A PDF with no extractable text fails with a clear message that OCR is not supported. Tagged-PDF logical structure is not interpreted in V1.

DRM-free standalone KF8/AZW3 and legacy MOBI import through the same HTML structure path as EPUB content. The importer preserves title, authors, language, cover bytes, reading order, headings, and source ranges. It restores legacy MOBI chapters from authored `filepos` navigation and removes generated inline tables of contents from narration. It supports uncompressed and PalmDOC-compressed text. Encrypted books fail before content extraction. HUFF/CDIC compression is isolated behind a clear unsupported-format error. Combined MOBI/KF8 files currently use the legacy MOBI rendition. KF8 navigation that exists only in binary INDX records, embedded non-cover image assets, and KFX are not supported yet.

HTML uses its `<title>` when present. TXT and Markdown infer the title from the filename. TXT chapter detection recognizes common `PART`, `BOOK`, and `CHAPTER` headings. Markdown and HTML heading levels map directly into the semantic tree.

Raw TXT, Markdown, and HTML inputs are limited to 32 MiB. HTML preflight caps parser resource units at 100,000, total attributes at 4,096, and tree depth at 128. EPUB archives are limited to 512 MiB, 10,000 entries, 32 MiB per expanded entry, and 512 MiB total expanded content. EPUB preflight validates archive paths, declared entry counts, actual decompression, and `META-INF/encryption.xml` before book parsing. Protected resources fail with a clear error. Standard IDPF and Adobe font obfuscation remain supported.

PDF files are limited to 128 MiB, 200,000 parsed objects, 10,000 pages, 16 MiB per decompressed parser or page-text stream, 128 MiB total extracted text, 10,000 outline nodes, and 128 outline levels. Page trees, bookmark trees, named destinations, and page-label trees are checked for impossible counts, excessive depth, and reference cycles before recursive library outline processing. These checks run before model setup.

MOBI and AZW3 files are limited to 128 MiB, 50,000 Palm database records, 128 MiB decoded text, 4,096 EXTH metadata records, 10,000 legacy navigation entries, 100,000 HTML parser resource units, 4,096 HTML attributes, and 128 HTML levels. Palm database offsets, record counts, compression back-references, trailing data, EXTH lengths, encryption flags, and text encoding are validated before HTML parsing or model setup.

## Kokoro pronunciation overrides

The lean phonemizer has no eSpeak fallback. It may spell unknown names and rare words letter by letter. Add an IPA override for each book-specific word:

```sh
audiobook-forge convert book.epub --pronunciation 'Elena=ɪlˈeɪnə'
audiobook-forge convert book.epub \
  --pronunciation 'Elena=ɪlˈeɪnə' \
  --pronunciation 'Cormer=kˈɔɹmɚ'
```

Repeat `--pronunciation WORD=IPA` as needed. A regression test and a public-domain book check cover a name that lean Misaki otherwise spells out.

Kokoro compatibility normalization runs after Misaki G2P and before vocabulary validation. It silently converts attached syllabic consonants such as `n̩` to Kokoro-compatible forms such as `ən`. Known source-text repairs include zero-width cleanup, punctuation, currency and math symbols, copyright marks, URLs, and spaced ellipses. Safe best-effort pronunciation candidates are recorded in the prepared artifact. Unknown phonemes that still cannot produce a valid sequence remain hard errors. The manifest records the normalization version and automatic repair count.

## Navigation and footnotes

The default `--nav chapters` policy exposes parts, chapters, and other major divisions in the M4B. `--nav sections` also exposes meaningful lower-level sections. `--nav auto` uses major divisions when present and falls back to sections for documents without them. Provider chunk boundaries never become player chapters.

The default `--footnotes inline` narrates each note where it occurs. `--footnotes skip` keeps notes and source mappings in `CanonicalBook` but omits their speech and records warnings. `--footnotes end` moves their speech to the end of the narration. All three choices are recorded in the manifest.

The default narration policy omits non-narrative back matter such as Notes sections, indexes, credits, and publisher pages titled `Connect with HMH`. Inline footnotes still follow the selected `--footnotes` mode. The source remains unchanged; the exclusion is applied to the prepared narration plan and recorded in build warnings.

## How it works

```text
EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or TXT
  -> format-specific import
  -> CanonicalBook
  -> normalized narration text
  -> provider-sized requests
  -> Kokoro: validated phonemes -> isolated native MLX worker
  -> Qwen: raw text -> isolated Python/MLX worker
  -> resumable segment cache
  -> AudioTimeline
  -> M4B + audionav.json + manifest.json
```

All importers stop at the format-neutral `CanonicalBook`. They do not know about Kokoro, AAC, or M4B. Narration handles headings, paragraphs, lists, notes, figures with useful captions, source navigation, and code as distinct block types. It normalizes whitespace, line-wrap hyphenation, soft hyphens, common Unicode punctuation, numeric citations, URLs, and standalone page numbers before synthesis. The `AudioTimeline` records section, page, paragraph, sentence, footnote, and figure cues with source ranges when available.

The parent process never holds the full audiobook in memory. It sends bounded requests to one long-lived provider worker, stores each valid result as an atomic cache segment, and streams cached PCM during final assembly. Kokoro uses a hidden 200-phoneme limit and a 400-character cache unit. Qwen uses normalized raw text and a 280-character unit. Neither limit changes user-facing navigation.

After each request, the worker evaluates and copies the PCM, drops the MLX audio array, and clears the MLX allocation cache. Kokoro also checks native memory counters: cached memory must return to at most 1 MiB, and MLX active and peak memory must stay below 4 GiB. A failed worker is restarted. Provider requests have two bounded retries. The CLI never starts parallel TTS workers.

All unsafe MLX C calls live in `crates/mlx-memory-control`. The main library and CLI use `#![forbid(unsafe_code)]`.

## Cache and recovery

The default cache is `~/Library/Caches/audiobook-forge`.

Set `AUDIOBOOK_FORGE_CACHE_DIR` to override it. Kokoro model downloads come from one pinned Hugging Face revision. The model and every English voice have fixed SHA-256 hashes, which the CLI checks before use. The optional Qwen runtime lives under `qwen-runtime` in the same cache root. Its model uses a pinned Hugging Face revision and the standard Hugging Face model cache.

Successful speech segments remain under the same cache root. A retry after interruption reuses valid segments. Cache keys include normalized text, provider, pinned model, voice, language, speed, sample rate, provider limits, phoneme chunk policy, pronunciation configuration, and the phoneme normalization version. A narration-affecting change therefore creates a new cache entry. Corrupt or incomplete WAV cache entries are rejected and generated again.

Set `AUDIOBOOK_FORGE_SEGMENT_CACHE_DIR` to isolate segment WAVs while reusing the installed runtimes and model files. The chapter benchmark uses this override to prevent old cache hits.

## Generated metadata

`book.audionav.json` uses schema version 1. It contains a hierarchical TOC, duration, page cues when the source has meaningful pages, paragraph and sentence cues, timestamps, stable section IDs, and source ranges.

`book.manifest.json` uses schema version 1. It records the source path and SHA-256 hash, source format, tool and importer versions, detected metadata, provider/model/voice identity, configuration hash, pronunciation overrides, phoneme normalization version, automatic repair counts by rule, speed, footnote and chapter policies, pause/retry settings, AAC settings, warnings, output files, build time, duration, and counts. It does not record API keys or other credentials.

These JSON files are `audiobook-forge` sidecars, not a claim of conformance to an audiobook distribution standard. `book.audionav.json` is an application navigation index for mapping semantic source locations to millisecond offsets. `book.manifest.json` is a reproducibility and provenance record. Most audiobook players do not need either file; they open the M4B and read its embedded metadata and chapters.

The [W3C Audiobooks Recommendation](https://www.w3.org/TR/audiobooks/) and the [Readium Audiobook Profile](https://readium.org/webpub-manifest/profiles/audiobook.html) define interoperable JSON-LD manifests for distribution. Those manifests use fields such as `@context`, `conformsTo`, `readingOrder`, `resources`, `toc`, and `duration`, and they describe linked audio resources. Our sidecars use a different schema (`duration_ms`, source ranges, paragraph/sentence mappings, and build provenance) and describe one already-assembled M4B. They are therefore not drop-in W3C/Readium manifests and should not be labeled `application/audiobook+json`.

The generated M4B has been tested in VLC 3.0.23 on macOS, including forward and backward chapter selection. See [PLAYER_COMPATIBILITY.md](PLAYER_COMPATIBILITY.md) for the exact build, independent metadata check, and player transcript.

## Privacy and security

Book parsing, speech generation, caching, and AAC assembly run locally. The first use of a model or voice downloads only that pinned asset from Hugging Face. Book text is not sent to a cloud TTS service. The Qwen worker receives bounded text over a local pipe. `ffmpeg` and `ffprobe` receive local file paths as direct process arguments, not shell text.

Input files are untrusted. The importers do not run embedded scripts or fetch external HTML resources. EPUB paths and decompression are checked before parsing. HTML/XML nesting and attributes are bounded. PDF object, page, stream, outline, and tree work is bounded. MOBI records, offsets, compression, metadata, and encryption flags are validated. Terminal output replaces control and bidirectional override characters from book metadata and file names.

DRM removal is not supported. Purchasing a book does not make its encryption removable by this tool. Use a DRM-free copy that you are allowed to process. Protected EPUB resources, password-protected PDFs, and encrypted Kindle books fail before synthesis with a clear error.

## Scope

- Input: English `.epub`, DRM-free `.azw3`/`.mobi`, text-based `.pdf`, `.html`/`.xhtml`, `.md`, and UTF-8 `.txt`
- Output: one `.m4b`, one versioned navigation sidecar, one reproducibility manifest, and an optional JPEG cover
- Inference: native MLX through `mlx-rs` for Kokoro; optional pinned `mlx-audio` Python/MLX runtime for Qwen
- Phonemes: lean `misaki-rs` for Kokoro; normalized raw text for Qwen
- Voices: 28 English Kokoro presets; Qwen `Aiden` and `Ryan`

Current limitations: HUFF/CDIC MOBI, binary-only KF8 navigation, combined-file KF8 rendition selection, tagged-PDF logical structure, complex-table preservation/narration, OCR, translation, MP3, a custom player, a web UI, and voice cloning are not implemented. Qwen is Apple-Silicon-only, runs at speed `1.0`, and does not yet consume Kokoro prepared artifacts or pronunciation overrides. `--nav auto` does not yet promote sections based on listening duration. Source text remains in the semantic model and navigation sidecar, but there is no `locate` or synchronized-text command yet.

## Development checks

The Linux CI gate runs formatting, strict Clippy, all workspace tests, M4B validation with FFmpeg, and the dependency policy. It uses deterministic mock TTS and needs no model download, API key, or paid TTS account. The native MLX allocator test runs in the full local gate on Apple Silicon.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo deny check
```

## Performance

On a 16 GB M1 Pro, the 200-phoneme default reached median warm RTF `0.110`, or about 9.1 times real time. The five-run range was `0.106-0.118`. MLX peak allocation was `3.52 GiB`, maximum worker footprint was `3.20 GiB`, and post-chunk cached MLX memory was `0 B`. See [BENCHMARK.md](BENCHMARK.md).

## License

Original code is Apache-2.0. Every locked dependency has a permitted license path. `mach-sys` is dual-licensed, and this project selects its Apache-2.0 option. Kokoro model files are Apache-2.0. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

This is an independent repository. It is not a fork of `DrewThomasson/ebook2audiobook` and contains no code copied from it.
