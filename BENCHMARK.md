# M1 Pro benchmark

Date: 2026-08-10

## Setup

- MacBook Pro with Apple M1 Pro
- Release build: `cargo build --release --locked`
- Runtime: direct `ort` 2.0.0-rc.11 with ONNX Runtime 1.23.2, two CPU threads
- Model: `model_q8f16.onnx`, revision `1939ad2a8e416c0acfeecc08a694d14ef25f2231`
- Model SHA-256: `04c658aec1b6008857c2ad10f8c589d4180d0ec427e7e6118ceb487e215c3cd0`
- Voice: `af_heart`
- Input: `tests/fixtures/smoke.txt`, packaged as EPUB with Pandoc
- Command: `target/release/kokoro-book smoke.epub --output smoke.wav --quiet`

RTF is synthesis wall time divided by generated audio duration. Model loading is measured separately.

## Warm results

| Run | Model load | Synthesis | Audio | RTF | Maximum RSS |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1.10 s | 37.87 s | 50.49 s | 0.750 | 920.1 MiB |
| 2 | 1.05 s | 31.72 s | 50.49 s | 0.628 | 853.6 MiB |
| 3 | 1.05 s | 31.95 s | 50.49 s | 0.633 | 891.5 MiB |
| Median | **1.05 s** | **31.95 s** | **50.49 s** | **0.633** | **891.5 MiB** |

Target: median RTF at or below `1.0`. Result: pass.

The three WAV files were byte-identical. `/usr/bin/time -l` measured a median peak memory footprint of 942.2 MiB.

## Cold install

The first run started without this model revision in the cache. It downloaded and verified the 82 MiB model and the 510 KiB voice, then synthesized the same text. The full command took 46.09 seconds. Model load was 1.13 seconds and synthesis RTF was 0.624. Maximum RSS was 851.1 MiB. Network time makes this result machine- and connection-specific.

The active model cache is 83 MiB. The prior sherpa bundle used 383 MiB.

## Pronunciation and signal checks

### Public-domain book excerpt

The end-to-end pronunciation check used a sentence from chapter IV of [*On the Eve* by Ivan Turgenev, translated by Constance Garnett](https://www.gutenberg.org/ebooks/6902). Project Gutenberg marks this book public domain in the USA. The test EPUB preserved the source sentence beginning “Why so?” inquired Elena.

Lean Misaki spelled “Elena” letter by letter. The override changed the generated audio:

```sh
kokoro-book on-the-eve-elena-excerpt.epub \
  --pronunciation 'Elena=ɪlˈeɪnə' \
  --output corrected.wav \
  --quiet
```

A temporary local Whisper `tiny.en` transcription gave this exact recognition evidence:

| Audio | Relevant transcript |
| --- | --- |
| No override | `Why so had inquired E-L-E-N?` |
| `Elena=ɪlˈeɪnə` | `Why so inquired Elena` |

The corrected WAV was 18.85 seconds. `ffprobe` reported mono `pcm_s16le` at 24 kHz. `ffmpeg astats` reported RMS `-22.85 dB`, peak `-4.67 dB`, and flat factor `0` across 452,400 samples.

### Synthetic regression corpus

The 142-word benchmark fixture also checked the override in a longer deterministic sample. A temporary local Whisper `tiny.en` transcription measured 12 word edits without the override and 3 with it: WER `8.5%` versus `2.1%`. The corrected transcript recognized “Elena” both times.

`ffprobe` reported mono `pcm_s16le` at 24 kHz. `ffmpeg astats` reported RMS `-23.04 dB`, one absolute full-scale sample across 1,211,880 samples, and no repeated flat peak. The deterministic output had no NaN or infinite sample before WAV conversion.

The temporary Whisper model was deleted after both checks. ASR is an intelligibility proxy, not a human listening score.

## Previous runtime comparison

| Runtime | Median RTF | Model cache | Memory evidence | GPL/LGPL in runtime |
| --- | ---: | ---: | --- | --- |
| sherpa-onnx plus eSpeak | 0.543 | 383 MiB | 750.5 MiB maximum RSS, one warm run | Yes, GPL |
| direct `ort` plus lean Misaki | 0.633 | 83 MiB | 891.5 MiB median maximum RSS | No |

The permissive runtime is about 17% slower and uses more working memory. It remains faster than real time and cuts the model cache by about 300 MiB.
