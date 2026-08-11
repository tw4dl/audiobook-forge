# kokoro-book

A small offline CLI that turns an English EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or UTF-8 TXT file into a WAV audiobook with [Kokoro-82M-bf16](https://huggingface.co/mlx-community/Kokoro-82M-bf16).

One model. One MLX worker. Preset voices only. No Python, server, GUI, eSpeak, ONNX Runtime, or voice cloning.

## Install

You need:

- Apple Silicon macOS
- Rust 1.88 or newer
- Xcode Command Line Tools
- CMake

```sh
brew install cmake
cargo install --git https://github.com/tw4dl/kokoro-book --locked
```

The first conversion downloads one pinned 312 MiB BF16 Kokoro model and the selected 510 KiB voice. The two files total 312.5 MiB. Later runs work offline. Each extra voice adds about 510 KiB when first used.

## Use

```sh
kokoro-book book.epub
kokoro-book inspect book.epub
kokoro-book inspect reference.pdf --tree
kokoro-book inspect reference.azw3 --tree
kokoro-book inspect notes.md --tree
kokoro-book notes.txt --output notes.wav
kokoro-book book.epub --voice af_sky --speed 1.1
kokoro-book voices
```

The default output sits beside the input with a `.wav` extension. The audio format is mono, 16-bit PCM, 24 kHz. The default voice is `af_heart`. Run `kokoro-book voices` for the 28 English presets.

`inspect` reports imported metadata and semantic sections without loading or downloading the TTS model. EPUB inspection includes the format version, author, language, cover, and authored page count when present. Explicit creator roles keep non-author contributors out of the author list. Use `--tree` to show the nested navigation structure. EPUB 3 navigation documents and EPUB 2 NCX files take priority over spine XHTML headings. EPUB 3 token lists and image alternative labels remain usable navigation. Page lists take priority over EPUB 3 `pagebreak` markers. The importer accepts UTF-8 and BOM-marked UTF-16 XML, follows manifest fallback chains around foreign or scripted resources, reads SVG text as source-mapped blocks, and preserves cover bytes, semantic containers, footnotes, source resources, source fragments, and spine order in `CanonicalBook`. Broken navigation links become warnings and do not hide readable spine content.

PDF inspection preserves title, author, PDF version, physical page number, and authored page label. Outline/bookmark hierarchy takes priority over deterministic `PART`, `BOOK`, and `CHAPTER` heading inference. A PDF with no usable outline reports the lower-confidence fallback before synthesis. Password-protected PDFs fail with a clear DRM-free-copy message. A PDF with no extractable text fails with a clear message that OCR is not supported. Tagged-PDF logical structure is not interpreted in V1.

DRM-free standalone KF8/AZW3 and legacy MOBI import through the same HTML structure path as EPUB content. The importer preserves title, authors, language, cover bytes, reading order, headings, and source ranges. It restores legacy MOBI chapters from authored `filepos` navigation and removes generated inline tables of contents from narration. It supports uncompressed and PalmDOC-compressed text. Encrypted books fail before content extraction. HUFF/CDIC compression is isolated behind a clear unsupported-format error. Combined MOBI/KF8 files currently use the legacy MOBI rendition. KF8 navigation that exists only in binary INDX records, embedded non-cover image assets, and KFX are not supported yet.

HTML uses its `<title>` when present. TXT and Markdown infer the title from the filename. TXT chapter detection recognizes common `PART`, `BOOK`, and `CHAPTER` headings. Markdown and HTML heading levels map directly into the semantic tree.

Raw TXT, Markdown, and HTML inputs are limited to 32 MiB. HTML preflight caps parser resource units at 100,000, total attributes at 4,096, and tree depth at 128. EPUB archives are limited to 512 MiB, 10,000 entries, 32 MiB per expanded entry, and 512 MiB total expanded content. EPUB preflight validates archive paths, declared entry counts, actual decompression, and `META-INF/encryption.xml` before book parsing. Protected resources fail with a clear error. Standard IDPF and Adobe font obfuscation remain supported.

PDF files are limited to 128 MiB, 200,000 parsed objects, 10,000 pages, 16 MiB per decompressed parser or page-text stream, 128 MiB total extracted text, 10,000 outline nodes, and 128 outline levels. Page trees, bookmark trees, named destinations, and page-label trees are checked for impossible counts, excessive depth, and reference cycles before recursive library outline processing. These checks run before model setup.

MOBI and AZW3 files are limited to 128 MiB, 50,000 Palm database records, 128 MiB decoded text, 4,096 EXTH metadata records, 10,000 legacy navigation entries, 100,000 HTML parser resource units, 4,096 HTML attributes, and 128 HTML levels. Palm database offsets, record counts, compression back-references, trailing data, EXTH lengths, encryption flags, and text encoding are validated before HTML parsing or model setup.

## Pronunciation overrides

The lean phonemizer has no eSpeak fallback. It may spell unknown names and rare words letter by letter. Add an IPA override for each book-specific word:

```sh
kokoro-book book.epub --pronunciation 'Elena=ɪlˈeɪnə'
kokoro-book book.epub \
  --pronunciation 'Elena=ɪlˈeɪnə' \
  --pronunciation 'Cormer=kˈɔɹmɚ'
```

Repeat `--pronunciation WORD=IPA` as needed. A regression test and a public-domain book check cover a name that lean Misaki otherwise spells out.

## How it works

```text
EPUB, DRM-free AZW3/MOBI, text-based PDF, HTML, Markdown, or TXT
  -> format-specific import
  -> CanonicalBook
  -> semantic normalization
  -> G2P
  -> bounded phoneme chunks
  -> one isolated MLX worker
  -> streamed PCM
  -> atomic WAV
```

The parent process never holds the full audiobook in memory. It sends length-prefixed requests to one long-lived worker. The default limit is 200 phonemes per request.

After each request, the worker evaluates and copies the PCM, drops the MLX audio array, clears the MLX allocation cache, and checks the native memory counters. Cached memory must return to at most 1 MiB. MLX active and peak memory must stay below 4 GiB. A failed request restarts the worker, splits that chunk once, and retries both halves. The CLI never starts parallel TTS workers.

All unsafe MLX C calls live in `crates/mlx-memory-control`. The main library and CLI use `#![forbid(unsafe_code)]`.

## Model cache

The default cache is `~/Library/Caches/kokoro-book`.

Set `KOKORO_BOOK_CACHE_DIR` to override it. Downloads come from one pinned Hugging Face revision. The model and every English voice have fixed SHA-256 hashes. The CLI checks each file before use.

## Scope

- Input: English `.epub`, DRM-free `.azw3`/`.mobi`, text-based `.pdf`, `.html`/`.xhtml`, `.md`, and UTF-8 `.txt`
- Output: one `.wav`
- Inference: native MLX through `mlx-rs`
- Phonemes: lean `misaki-rs`
- Voices: English Kokoro presets only

Current limitations: conversion still emits WAV rather than M4B plus navigation sidecars. The new M4B, semantic narration, resumable segment cache, and sidecar modules are not wired into the public conversion command yet. HUFF/CDIC MOBI, binary-only KF8 navigation, combined-file KF8 rendition selection, tagged-PDF logical structure, OCR, translation, MP3, a web UI, and voice cloning are not implemented yet. DRM removal is out of scope; unsupported encrypted resources fail before parsing or synthesis.

## Performance

On a 16 GB M1 Pro, the 200-phoneme default reached median warm RTF `0.110`, or about 9.1 times real time. The five-run range was `0.106-0.118`. MLX peak allocation was `3.52 GiB`, maximum worker footprint was `3.20 GiB`, and post-chunk cached MLX memory was `0 B`. See [BENCHMARK.md](BENCHMARK.md).

## License

Original code is Apache-2.0. Every locked dependency has a permitted license path. `mach-sys` is dual-licensed, and this project selects its Apache-2.0 option. Kokoro model files are Apache-2.0. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

This is an independent repository. It is not a fork of `DrewThomasson/ebook2audiobook` and contains no code copied from it.
