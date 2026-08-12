# Qwen provider benchmark

Date: 2026-08-11

## Decision

Keep Kokoro as the default audiobook provider. It is much faster and uses less memory. Keep Qwen as an experimental opt-in provider for users who prefer its more dynamic delivery after a blind listen.

On this chapter, Qwen did not improve sampled intelligibility. Both providers produced a 1.645% Whisper word error rate over the same 304 reference words. Qwen took 5.93 times as long end to end and used 1.98 times the reported MLX peak memory.

## Scope and parity

- Source: local `target/die-with-zero/Die with Zero.epub`
- Chapter: `Optimize Your Life`, from `/OPS/c01.xhtml`
- Benchmark input: the chapter's existing prepared `tts_text`, saved as `target/qwen-benchmark/input/die-with-zero-c01.txt`
- Input SHA-256: `65c5fc78571a66d8e55ef2024aa8a3acbfa29fcc8f115caed23ab8c9167e0ee6`
- Text: 5,385 words, 29,841 narrated characters, 274 narration units
- Shared settings: English, speed `1.0`, chapter navigation, mono 24 kHz output, 64 kbit/s AAC
- Cache control: each provider used an empty `AUDIOBOOK_FORGE_SEGMENT_CACHE_DIR`; the two reported cache hits were duplicate text within the same run

The benchmark used an Apple M1 Pro MacBook Pro with 8 CPU cores and 16 GB RAM, macOS 26.1. Tool versions were Rust nightly 1.99, FFmpeg 8.0.1, Python 3.12.13, `mlx-audio` 0.4.8, and MLX 0.32.0.

Kokoro used `af_heart` with the repository's pinned `mlx-community/Kokoro-82M-bf16` assets. Qwen used `Aiden` with [`mlx-audio` revision `49596ac`](https://github.com/Blaizzy/mlx-audio/tree/49596ac8b69b9ed377db311a73df838795f38a3d) and [`mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-6bit` revision `7dc92af`](https://huggingface.co/mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-6bit/tree/7dc92af14613355896fcab13b268c19ede233139).

## Performance

| Measure | Kokoro `af_heart` | Qwen `Aiden` | Qwen relative to Kokoro |
|---|---:|---:|---:|
| End-to-end wall time | 190.24 s | 1,128.60 s | 5.93x slower |
| Provider synthesis time | 167.18 s | 1,097.37 s | 6.56x slower |
| Audio duration | 1,979.76 s | 1,979.96 s | +0.20 s |
| Synthesis RTF | 0.084 | 0.554 | 6.60x higher |
| Offline generation rate | 11.90x real time | 1.81x real time | — |
| Model load | 0.096 s | 2.043 s | 21.3x slower |
| Provider requests | 303 | 278 | 25 fewer |
| Worker restarts | 0 | 0 | same |
| Generation retries | not applicable | 0 | — |
| Reported MLX peak | 3.60 GiB | 7.14 GiB | 1.98x higher |
| M4B size | 16,762,386 bytes | 17,714,393 bytes | 5.68% larger |

Both final M4B files passed `ffprobe`: AAC, mono, 24,000 Hz. Output hashes:

- Kokoro: `6f08566594ce776e79ca8e13178f91a3c729459ac5648e0afe88497a093757f7`
- Qwen: `a3644d0d1fe3680a530ba8af12da183fa868136b480ed3d814c549f3a0c91c43`

## Quality evidence

Five matched-content excerpts cover the opening, two interior passages, the former failure area, and the chapter ending. Local Whisper `tiny.en` transcribed 304 reference words per provider.

| Measure | Kokoro | Qwen |
|---|---:|---:|
| Whisper substitutions | 4 | 2 |
| Whisper deletions | 1 | 1 |
| Whisper insertions | 0 | 2 |
| Whisper WER | 1.645% | 1.645% |
| Overall pace | 163.20 words/min | 163.19 words/min |
| Median sentence duration | 6.475 s | 6.400 s |
| 90th percentile sentence duration | 12.900 s | 12.880 s |
| Longest sentence | 25.850 s | 29.440 s |
| Detected silence share | 22.59% | 17.27% |
| Integrated loudness | -28.2 LUFS | -21.0 LUFS |
| Loudness range | 2.6 LU | 7.0 LU |
| True peak | -4.6 dBFS | -0.5 dBFS |

The ASR result supports equal sampled intelligibility. The duration data supports equal overall pacing. Qwen has a wider loudness range and less silence, which is consistent with more dynamic delivery, but these measures do not prove better prosody. Kokoro has more consistent level. A blind listen remains the source of truth for naturalness and cross-segment voice identity.

## Greedy-decoding failure and correction

The first Qwen run used greedy decoding with a fixed 1,024-token limit. It was not acceptable. Fourteen sentence segments lasted at least 60 seconds, 11 landed at exactly 81.92 seconds, and one short sentence expanded from 2.625 seconds with Kokoro to 81.92 seconds with Qwen. Whisper recovered only the opening words from that failed segment.

The final provider uses the upstream sampling defaults, a stable text-derived seed, a second stable fallback seed, and a text-sized token budget clamped from 96 to 640 tokens. It rejects any result that reaches the token limit. The corrected failed sentence took 3.68 seconds and Whisper recovered the complete sentence. In the final chapter, the largest Qwen-to-Kokoro sentence-duration ratio was 1.49 instead of 31.21. No generation retry or worker restart was needed.

The failed output remains under `target/qwen-benchmark/output/qwen-greedy-v1` for diagnosis. Do not use it as an audiobook.

## Blind listening rubric

Listen to the loudness-matched files under `target/qwen-benchmark/blind/matched`. Each A/B pair contains the same text. All clips are within 0.1 LU of -24 LUFS, so level should not reveal the provider.

Use a 1-to-5 score: 1 is unusable, 3 is usable with clear defects, and 5 is publish-ready with no distracting defect.

| Criterion | What to judge | A | B |
|---|---|---:|---:|
| Pronunciation | Names, numbers, stress, and word accuracy |  |  |
| Prosody | Natural emphasis, phrasing, and emotional fit |  |  |
| Pacing | Speed, pause length, and sentence flow |  |  |
| Clarity | Effort needed to understand every word |  |  |
| Voice stability | Same speaker identity and level across all five clips |  |  |

Score all five pairs before revealing the key.

<details>
<summary>Reveal provider key</summary>

A is Qwen `Aiden`. B is Kokoro `af_heart`.

</details>

## Artifacts

- Final Kokoro audiobook: `target/qwen-benchmark/output/kokoro/die-with-zero-c01.m4b`
- Final Qwen audiobook: `target/qwen-benchmark/output/qwen/die-with-zero-c01.m4b`
- Loudness-matched blind clips: `target/qwen-benchmark/blind/matched/`
- Whisper transcripts: `target/qwen-benchmark/asr/whisper-tiny-en/`
- Run and `/usr/bin/time` logs: `target/qwen-benchmark/logs/`
- Navigation and reproducibility manifests: beside each final audiobook

The generated benchmark tree is ignored by Git because it is under `target/`.
