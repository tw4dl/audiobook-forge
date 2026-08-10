# kokoro-book

A small offline CLI that turns an English EPUB or UTF-8 TXT file into a WAV audiobook with [Kokoro-82M-bf16](https://huggingface.co/mlx-community/Kokoro-82M-bf16).

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
kokoro-book notes.txt --output notes.wav
kokoro-book book.epub --voice af_sky --speed 1.1
kokoro-book voices
```

The default output sits beside the input with a `.wav` extension. The audio format is mono, 16-bit PCM, 24 kHz. The default voice is `af_heart`. Run `kokoro-book voices` for the 28 English presets.

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
EPUB or TXT
  -> sentence extraction
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

- Input: English `.epub` and UTF-8 `.txt`
- Output: one `.wav`
- Inference: native MLX through `mlx-rs`
- Phonemes: lean `misaki-rs`
- Voices: English Kokoro presets only

PDF, OCR, translation, M4B, MP3, a web UI, and voice cloning are out of scope.

## Performance

On a 16 GB M1 Pro, the 200-phoneme default reached median warm RTF `0.110`, or about 9.1 times real time. The five-run range was `0.106-0.118`. MLX peak allocation was `3.52 GiB`, maximum worker footprint was `3.20 GiB`, and post-chunk cached MLX memory was `0 B`. See [BENCHMARK.md](BENCHMARK.md).

## License

Original code is Apache-2.0. Every locked dependency has a permitted license path. `mach-sys` is dual-licensed, and this project selects its Apache-2.0 option. Kokoro model files are Apache-2.0. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

This is an independent repository. It is not a fork of `DrewThomasson/ebook2audiobook` and contains no code copied from it.
