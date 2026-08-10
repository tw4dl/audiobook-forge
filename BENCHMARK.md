# M1 Pro benchmark

Date: 2026-08-10

## Setup

- MacBook Pro with Apple M1 Pro
- Release build: `cargo build --release --locked`
- Runtime: `sherpa-onnx 1.13.4`
- Model: `kokoro-multi-lang-v1_0`
- Archive SHA-256: `c133d26353d776da730870dac7da07dbfc9a5e3bc80cc5e8e83ab6e823be7046`
- Model SHA-256: `c436dc6a842b62aba06af67e40bafcfb9c60ac3af895358f1974ad9a7f7c026b`
- Voice: `af_heart`
- Input: `tests/fixtures/smoke.txt`, packaged as EPUB with Pandoc
- Command: `target/release/kokoro-book smoke.epub --output smoke.wav --quiet`

RTF is synthesis wall time divided by generated audio duration. Model loading is measured separately.

## Results

| Run | Model load | Synthesis | Audio | RTF |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 0.89 s | 24.01 s | 45.66 s | 0.526 |
| 2 | 0.93 s | 24.81 s | 45.66 s | 0.543 |
| 3 | 1.68 s | 27.35 s | 45.67 s | 0.599 |
| Median | 0.93 s | 24.81 s | 45.66 s | **0.543** |

Target: median RTF at or below `1.0`. Result: pass.

## Cold install and memory

A separate empty-cache run downloaded, verified, and extracted the 333 MiB
archive before synthesizing the same EPUB. The full command took 69.72 seconds.
Its synthesis RTF was 0.512 and its model load took 0.89 seconds.

`/usr/bin/time -l` measured 750.5 MiB maximum resident memory and 802.5 MiB
peak memory footprint on warm run 1.

## Output check

`ffprobe` reported mono `pcm_s16le` at 24 kHz for all three files. `ffmpeg`
`astats` reported a peak of `-5.56 dB` and RMS of `-24.00 dB` on the median
run. The output had no digital clipping. This is a mechanical signal check,
not a human listening score.
