# kokoro-book

A small offline CLI that turns an English EPUB or UTF-8 TXT file into a WAV audiobook with [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M).

One model. Preset voices only. No Python, server, GUI, or voice cloning.

## Install

You need Rust 1.88 or newer. The current tested target is Apple Silicon macOS.

```sh
cargo install --git https://github.com/tw4dl/kokoro-book --locked
```

The first conversion downloads one pinned 333 MiB Kokoro v1.0 bundle. The extracted cache uses about 383 MiB. Later runs work offline.

## Use

```sh
kokoro-book book.epub
kokoro-book notes.txt --output notes.wav
kokoro-book book.epub --voice af_sky --speed 1.1
kokoro-book voices
```

The default output sits beside the input with a `.wav` extension. The audio format is mono, 16-bit PCM, 24 kHz.

The default voice is `af_heart`. Run `kokoro-book voices` for the 28 English presets.

## Model cache

The default cache is:

- macOS: `~/Library/Caches/kokoro-book`
- Linux: `~/.cache/kokoro-book`

Set `KOKORO_BOOK_CACHE_DIR` to override it. Downloads use the pinned `kokoro-multi-lang-v1_0` archive. Core files are checked against fixed SHA-256 hashes before first use.

## Scope

- Input: English `.epub` and UTF-8 `.txt`
- Output: one `.wav`
- Engine: Kokoro v1.0 through the `sherpa-onnx` Rust API
- Voices: English presets only

PDF, OCR, translation, M4B, MP3, a web UI, and voice cloning are out of scope.

## Performance

On an M1 Pro, three release-build EPUB runs had RTF values of `0.526`, `0.543`, and `0.599`. Median RTF was `0.543`, or about 1.8 times faster than real time. See [BENCHMARK.md](BENCHMARK.md).

## License

Original code is licensed under `GPL-3.0-or-later`. This is compatible with the GPL eSpeak NG text frontend included by the native runtime. Kokoro model files remain Apache-2.0. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

This is an independent repository. It is not a fork of `DrewThomasson/ebook2audiobook` and contains no code copied from it.
