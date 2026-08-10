# M1 Pro benchmark

Date: 2026-08-10

## Setup

- MacBook Pro `MacBookPro18,3`
- Apple M1 Pro, 8 CPU cores, 16 GB unified memory
- macOS 26.1 (`25B78`)
- Rust 1.88.0 (`rustc 1.88.0`)
- Release build: `cargo build --release --locked`
- Runtime: `voice-tts` 0.2.1, `mlx-rs` 0.25.3, and MLX C from `mlx-sys` 0.2.0
- Model: `mlx-community/Kokoro-82M-bf16` revision `a71e4d38b236d968966a2002c4c895dbd12b1c3c`
- Model SHA-256: `4e9ecdf03b8b6cf906070390237feda473dc13327cb8d56a43deaa374c02acd8`
- Voice: `af_heart`
- Input: `tests/fixtures/smoke.txt`, 142 words
- One fresh CLI and one fresh worker per sample; model files already cached
- One synthesis worker; no parallel TTS requests

RTF is worker synthesis wall time divided by generated speech duration. It includes MLX evaluation and the copy from the MLX array. It excludes model loading, G2P, inserted sentence silence, and WAV I/O. Model loading is reported separately.

MLX peak and cached bytes come from the checked MLX C API after every chunk. Worker footprint comes from `/usr/bin/time -l`, wrapped around only the isolated worker through `KOKORO_BOOK_WORKER_TIME_LOG`.

## Phoneme-limit results

Each limit has five warm samples.

| Phoneme limit | Warm RTF values | Median RTF | Median synthesis | Median model load | MLX peak | Largest worker footprint | Post-chunk cache |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| **200 (default)** | `0.106, 0.110, 0.118, 0.114, 0.109` | **0.110** | 5.92 s | 0.044 s | **3.52 GiB** | **3.20 GiB** | **0 B** |
| 250 | `0.106, 0.106, 0.105, 0.106, 0.106` | **0.106** | 5.57 s | 0.039 s | **3.74 GiB** | **3.36 GiB** | **0 B** |
| 300 | `0.107, 0.105, 0.117, 0.108, 0.106` | **0.107** | 5.67 s | 0.041 s | **3.77 GiB** | **3.43 GiB** | **0 B** |

All three limits pass every target:

- Median RTF below `0.20`
- MLX peak below `4 GiB`
- Worker footprint below `6 GiB`
- Post-chunk cached MLX memory at or below `1 MiB`
- No worker restart in any sample

The default is 200 phonemes. It gives the lowest MLX peak and the most context headroom for a split retry. Its median RTF remains about 9.1 times real time.

## Cold install

A final empty-cache run downloaded and verified the model and `af_heart`, then synthesized the same input with the 200-phoneme default.

- Full command: 36.06 s
- Downloaded files: 327,637,472 bytes, or 312.5 MiB
- Model load after download: 0.136 s
- Synthesis: 5.89 s
- RTF: 0.109
- MLX peak: 3.52 GiB
- Post-chunk cache: 0 B

Network time is connection-specific. The model-load value measures loading the verified local files into the worker, not the download.

## Pronunciation and signal check

The real-book fixture uses a sentence from chapter IV of [*On the Eve* by Ivan Turgenev, translated by Constance Garnett](https://www.gutenberg.org/files/6902/6902-h/6902-h.htm). Project Gutenberg identifies its source text as unrestricted by U.S. copyright law. The repository fixture contains only the tested passage, without Project Gutenberg branding or license text.

Lean Misaki spells the unknown name `Elena` letter by letter. The override test used:

```sh
target/release/kokoro-book tests/fixtures/on-the-eve-elena.txt \
  --pronunciation 'Elena=ɪlˈeɪnə' \
  --output corrected.wav \
  --quiet
```

Homebrew `openai-whisper` `20250625_3` with `tiny.en` produced this exact recognition evidence:

| Audio | Transcript |
| --- | --- |
| No override | `Why so inquired, E-L-E-N, a one with thank you were speaking of some spiteful, disagreeable old woman. She is a pretty young girl.` |
| `Elena=ɪlˈeɪnə` | `Why so inquired Elena one would think you were speaking of some spiteful disagreeable old woman. She is a pretty young girl` |

The corrected WAV SHA-256 for this run was `66d9d1d7ff9b3561e8c25823816e8951105fc6b261177257c6de477d1b580ed4`. `ffprobe` reported mono `pcm_s16le` at 24 kHz and 7.90 seconds. `ffmpeg astats` reported peak `-10.53 dB`, RMS `-28.75 dB`, flat factor `0`, and 189,600 samples. The streaming writer rejects NaN, infinite, full-scale, and out-of-range PCM before it can commit the output file. Kokoro's active noise path means repeated WAV files are not expected to be byte-identical.

Whisper is an intelligibility proxy, not a human listening score.

## Why older Kokoro RTF values differ

These measurements came from different runtime states or harnesses. They are not one statistical series.

| Measurement | Median RTF | Range | Important difference |
| --- | ---: | --- | --- |
| Final CLI, 200 phonemes | 0.110 | `0.106-0.118` | Bounded chunks; cache cleared and verified after each chunk |
| Final CLI, 250 phonemes | 0.106 | `0.105-0.106` | Fewer, larger chunks |
| Final CLI, 300 phonemes | 0.107 | `0.105-0.117` | Fewer, larger chunks; highest MLX peak |
| Earlier native Rust harness | 0.121 | `0.116-0.145` | 450-character chunks reached about 492 phonemes |
| Earlier retained-cache run | 0.513 | not recorded | MLX retained 10.91 GiB and entered memory pressure |
| Retired ONNX CLI | 0.633 | `0.628-0.750` | CPU ONNX q8f16 model, not MLX BF16 |

The largest change came from cache control. Copying PCM, dropping the MLX array, and calling `mlx_clear_cache()` removed 10.91 GiB of retained cache from the exploratory harness. That ended memory pressure and reduced its median RTF from `0.513` to `0.121`.

The remaining spread comes from chunk shape, runtime and model precision, MLX scheduling, macOS load, and Kokoro's noise generation path. This report therefore compares medians from at least five fresh warm processes for each final chunk limit. The final selection uses memory safety first, then speed.
