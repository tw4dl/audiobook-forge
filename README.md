# kokoro-book

A small offline CLI that turns an English EPUB or UTF-8 TXT file into a WAV audiobook with [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M).

One model. Preset voices only. No Python, server, GUI, eSpeak, or voice cloning.

## Install

You need Rust 1.88 or newer. The tested target is Apple Silicon macOS.

```sh
cargo install --git https://github.com/tw4dl/kokoro-book --locked
```

The first conversion downloads one pinned 82 MiB q8f16 Kokoro model and the selected 510 KiB voice. The active cache uses about 83 MiB. Later runs work offline. Each extra voice adds about 510 KiB when first used.

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

Repeat `--pronunciation WORD=IPA` as needed. A regression test covers a name that lean Misaki otherwise spells out.

## Model cache

The default cache is:

- macOS: `~/Library/Caches/kokoro-book`
- Linux: `~/.cache/kokoro-book`

Set `KOKORO_BOOK_CACHE_DIR` to override it. Downloads come from one pinned Hugging Face revision. The model and every voice have fixed SHA-256 hashes. The CLI checks them before use.

## Scope

- Input: English `.epub` and UTF-8 `.txt`
- Output: one `.wav`
- Inference: direct `ort` and ONNX Runtime
- Phonemes: `misaki-rs` with default features off
- Voices: English Kokoro presets only

PDF, OCR, translation, M4B, MP3, a web UI, and voice cloning are out of scope.

## Performance

On an M1 Pro, three release-build EPUB runs had RTF values of `0.750`, `0.628`, and `0.633`. Median RTF was `0.633`, or about 1.58 times faster than real time. Median model load was `1.05 s`. See [BENCHMARK.md](BENCHMARK.md).

## License

Original code is licensed under Apache-2.0. The locked build contains no GPL or LGPL package. Kokoro model files are Apache-2.0. See [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

This is an independent repository. It is not a fork of `DrewThomasson/ebook2audiobook` and contains no code copied from it.
