# Batch preflight and narration normalization

Status: Proposed

## Summary

Add a zero-audio preflight phase that finds every narration problem before the
Kokoro worker starts. The phase must use the same text, sentence, phoneme, and
chunking rules as synthesis.

The current failure loop is expensive:

```text
run TTS for a long time -> fail on one sentence -> fix it -> rebuild -> find the next sentence
```

The required workflow is:

```text
import -> plan -> normalize -> preflight every unit -> repair or report all issues -> synthesize once
```

## Research findings

Open-source audiobook tools use several useful patterns:

| Project | Observed practice | Decision for kokoro-book |
| --- | --- | --- |
| `p0n1/epub_to_audiobook` | Preview mode reports chapter character counts before TTS. A regex search/replace file handles abbreviations and pronunciation. It supports chapter ranges, exported text, and parallel workers. | Add a machine-readable preview and a controlled replacement catalog. Keep exact source pointers. |
| `aedocw/epub2tts` | EPUB scan mode, exported editable text, endnote/reference cleanup, resumable output, chapter parallelism, and optional Whisper transcript-ratio checks. | Add batch preflight, resumable prepared units, and optional post-synthesis verification. |
| `denizsafak/abogen` | Optional LLM-assisted normalization with sentence, paragraph, or document context and a preview step. | Keep LLM normalization optional, offline by default, and subject to deterministic validation. |
| `DrewThomasson/ebook2audiobook` | Manual cleanup is expected because EPUB structure is not uniform. It supports explicit SML breaks and reports sentence-splitting/truncation issues. | Make exclusions and repair decisions explicit in the report instead of requiring a failed build or manual source rewrite. |
| Piper | Phonemization is tied to the voice's configured phoneme inventory. Piper supports raw phoneme input and a model-specific phoneme map. | Validate against the selected Kokoro vocabulary before synthesis and version the normalization profile in the cache key. |

References:

- https://github.com/p0n1/epub_to_audiobook
- https://github.com/aedocw/epub2tts
- https://github.com/denizsafak/abogen
- https://github.com/DrewThomasson/ebook2audiobook
- https://github.com/rhasspy/piper
- https://github.com/rhasspy/piper/blob/master/TRAINING.md

The gap is important: these projects provide preview or repair tools, but a
single deterministic pass that validates every narration sentence against the
selected model vocabulary is not the normal workflow. That is the main feature
of this spec.

## Goals

1. Find all text, G2P, phoneme, and chunking failures before audio synthesis.
2. Apply safe, deterministic fixes without changing the EPUB source.
3. Generate actionable pronunciation suggestions for unresolved words.
4. Preserve source ranges and stable narration-unit IDs for every issue.
5. Avoid loading the TTS worker or writing audio during preflight.
6. Let a clean preflight feed synthesis without repeating discovery work.
7. Keep all repairs and configuration changes auditable and cacheable.

## Non-goals

- Do not silently delete words or sentences.
- Do not rewrite the EPUB package.
- Do not use an LLM by default.
- Do not infer a pronunciation for an unresolved name and present it as
  correct without recording the choice.
- Do not use post-audio ASR as a replacement for deterministic preflight.

## Pipeline

```text
CanonicalBook
    |
    v
NarrationPlan
    |
    v
TextNormalizer
    |
    v
SentenceExtractor
    |
    v
G2P / pronunciation overrides
    |
    v
Kokoro phoneme normalization
    |
    v
Vocabulary + chunk validation
    |
    +--> PreflightReport
    +--> PreparedNarration cache
    |
    v
TTS synthesis
```

Preflight must iterate the exact `NarrationPlan` used by synthesis. It must not
scan a different flattened EPUB text stream.

## Normalization layers

### 1. Lossless text normalization

Apply deterministic rules before sentence extraction:

- normalize Unicode where safe;
- remove soft hyphens and zero-width formatting characters;
- convert non-breaking spaces to spaces;
- normalize curly quotes and dash variants;
- collapse spaced ellipses;
- expand symbols such as `$`, `×`, and `=` only in narration text;
- remove known numeric citation markers when the narration policy requests it;
- verbalize URLs and email addresses with a deterministic rule;
- preserve the original text and source range.

Every change receives a rule name and count in the report.

### 2. Lexical normalization

Use a book-level pronunciation catalog for names, abbreviations, foreign words,
and domain terms.

Preferred matching order:

1. exact token and case-preserving form;
2. case-insensitive token form;
3. explicit contextual rule;
4. opt-in regular expression rule.

Regular expressions must be opt-in because a broad replacement can damage
ordinary words. Store the original and replacement text for every match.

### 3. Phoneme normalization

Run the selected G2P engine, then normalize only known compatibility cases
before vocabulary validation.

The current Kokoro case is the syllabic marker `U+0329` (`̩`). Approved
deterministic mappings include:

```text
n̩  -> ən
l̩  -> əl
m̩  -> əm
ɹ̩ -> əɹ
r̩  -> ər
```

Unknown phonemes remain unresolved errors. They must not be dropped.

### 4. Chunk validation

Validate every prepared sentence and provider chunk against the selected
phoneme limit. Report the unit, sentence, chunk length, and chosen split point.

## Preflight command

Add a subcommand:

```text
kokoro-book preflight INPUT \
  --report target/book/preflight.json \
  --prepared target/book/prepared-narration.jsonl \
  --suggestions target/book/pronunciations.txt
```

Recommended options:

- `--nav chapters|sections|auto`
- `--footnotes inline|skip|end`
- `--voice VOICE`
- `--pronunciation WORD=IPA` (repeatable)
- `--chunk-phonemes N`
- `--format json|text`
- `--fail-on unresolved|none` (default: `unresolved`)

The existing build command may run the same preflight automatically. A separate
subcommand remains required for debugging and interactive repair.

## Batch error collection

Preflight must never use a short-circuiting `collect::<Result<_>>()` for the
whole book. Each narration unit and sentence gets an independent result.

For each failure:

1. record the failure;
2. continue scanning the remaining units;
3. deduplicate by issue signature;
4. retain occurrence count and representative source locations;
5. return a non-zero preflight status only after the complete scan.

For a G2P `❓` result, identify likely offending tokens by probing the token,
its punctuation-stripped form, and a small neighboring context window. This is
diagnostic only; the full sentence remains the source of truth.

## Report format

Write one JSON report per preflight run:

```json
{
  "schema_version": 1,
  "source_sha256": "...",
  "profile": "en-US-kokoro-v1",
  "normalization_version": 1,
  "scanned_units": 2681,
  "scanned_sentences": 3124,
  "automatic_repairs": 87,
  "unresolved": 4,
  "issues_by_kind": {
    "unknown_g2p_token": 3,
    "unsupported_phoneme": 1
  },
  "issues": [
    {
      "signature": "unknown_g2p_token:Scheinkman",
      "kind": "unknown_g2p_token",
      "token": "Scheinkman",
      "unit_id": "epub:/OPS/c08.xhtml:section-6:block:8",
      "sentence_index": 3,
      "source_range": {
        "source_id": "/OPS/c08.xhtml",
        "fragment": null,
        "character_offset": 4217
      },
      "text": "...",
      "occurrences": 2,
      "suggestion": null
    }
  ]
}
```

The report must distinguish:

- `automatic_repair`;
- `unknown_g2p_token`;
- `unsupported_phoneme`;
- `oversized_chunk`;
- `empty_narration_unit`;
- `excluded_by_policy`;
- `source_parse_warning`.

## Suggestions file

Write unresolved lexical suggestions separately from the report:

```text
# Generated by kokoro-book preflight. Review before use.
Scheinkman=
José=
Alzheimer’s=
```

Do not invent IPA when the engine cannot determine a safe pronunciation. The
user may fill in IPA, spelling, or a text replacement and rerun preflight.

## Prepared narration cache

The clean preflight should write prepared units containing:

- stable unit ID;
- normalized narration text;
- sentence boundaries;
- normalized phoneme chunks;
- source range;
- normalization and repair records;
- source hash;
- profile and configuration hash.

Synthesis should consume this artifact when its hashes match. This avoids
running G2P again and ensures the audio job cannot discover a new text failure
halfway through a long conversion.

The synthesis cache key must include:

- source hash;
- normalized text or prepared-narration hash;
- provider and model;
- voice and language;
- pronunciation catalog hash;
- normalization profile/version;
- phoneme chunk limit;
- speed and sample rate.

## Optional post-synthesis verification

Borrow the useful idea from `epub2tts`: optionally transcribe each generated
unit and compare it with the prepared text. This is a quality check, not a
preflight gate.

```text
--verify-transcript
--min-transcript-ratio 0.96
```

A failed ratio should mark the unit for review and allow a targeted retry. It
must not require rebuilding completed chapters.

## Acceptance criteria

1. `preflight` scans every narration unit and reports all failures in one run.
2. Preflight does not load MLX, invoke TTS, or write audio.
3. Known `U+0329` cases are repaired silently and counted.
4. Unknown words produce suggestions and source pointers.
5. Unresolved issues prevent a normal build from starting.
6. A clean prepared artifact lets synthesis start without repeating G2P.
7. Re-running with the same source and profile is cache-only.
8. Changing the normalization profile invalidates prepared and audio caches.
9. No source EPUB bytes or source text are modified.
10. Tests cover all issue kinds, duplicate aggregation, source pointers,
    suggestions, cache invalidation, and a full-book scan.
11. Die with Zero completes preflight without the old one-error-per-build loop.
12. A full build can resume at the failed unit or chapter, not at the start.

## Recommended implementation order

1. Extract a reusable `PreparedNarration` and `PreflightReport` model.
2. Refactor current synthesis phonemization to return per-unit results.
3. Replace whole-book short-circuit collection with batch issue collection.
4. Add the `preflight` CLI command and JSON report.
5. Add suggestion generation and exact-token pronunciation catalogs.
6. Add prepared-narration persistence and cache validation.
7. Add optional transcript verification.
8. Add the Die with Zero corpus regression.
