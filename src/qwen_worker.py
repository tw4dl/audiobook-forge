"""Length-framed Qwen3-TTS worker used by the Rust parent process."""

from __future__ import annotations

import argparse
import contextlib
import json
import sys
import time
from typing import Any


PROTOCOL_OUTPUT = sys.stdout.buffer


def write_header(payload: dict[str, Any]) -> None:
    PROTOCOL_OUTPUT.write(
        json.dumps(payload, ensure_ascii=True, separators=(",", ":")).encode("utf-8")
    )
    PROTOCOL_OUTPUT.write(b"\n")
    PROTOCOL_OUTPUT.flush()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--model", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--voice", required=True)
    parser.add_argument("--language", default="English")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        with contextlib.redirect_stdout(sys.stderr):
            import mlx.core as mx
            import numpy as np
            from mlx_audio.tts import load

            started = time.perf_counter()
            model = load(args.model, revision=args.revision)
            model_load_seconds = time.perf_counter() - started
            supported = {
                str(voice).casefold()
                for voice in getattr(model, "supported_speakers", ())
            }
            if args.voice.casefold() not in supported:
                raise ValueError(
                    f"unsupported Qwen voice {args.voice!r}; available: "
                    f"{', '.join(sorted(supported))}"
                )
            sample_rate = int(model.sample_rate)
        write_header(
            {
                "status": "ready",
                "model_load_seconds": model_load_seconds,
                "peak_bytes": int(mx.get_peak_memory()),
                "sample_rate": sample_rate,
            }
        )
    except Exception as error:  # noqa: BLE001 - error crosses a process boundary
        write_header({"status": "error", "message": str(error)})
        return 1

    for raw_request in sys.stdin.buffer:
        try:
            request = json.loads(raw_request)
            text = request.get("text")
            speed = request.get("speed")
            max_tokens = request.get("max_tokens")
            seeds = request.get("seeds")
            if not isinstance(text, str) or not text.strip():
                raise ValueError("Qwen request text must be a non-empty string")
            if speed != 1.0:
                raise ValueError("Qwen worker currently supports only speed 1.0")
            if not isinstance(max_tokens, int) or not 1 <= max_tokens <= 640:
                raise ValueError("Qwen max_tokens must be an integer from 1 to 640")
            if (
                not isinstance(seeds, list)
                or len(seeds) != 2
                or not all(isinstance(seed, int) and 0 <= seed <= 0xFFFFFFFF for seed in seeds)
            ):
                raise ValueError("Qwen seeds must contain two unsigned 32-bit integers")

            with contextlib.redirect_stdout(sys.stderr):
                started = time.perf_counter()
                results = []
                generation_attempts = 0
                for seed in seeds:
                    generation_attempts += 1
                    mx.random.seed(seed)
                    results = list(
                        model.generate(
                            text=text,
                            voice=args.voice,
                            lang_code=args.language,
                            speed=1.0,
                            temperature=0.9,
                            top_k=50,
                            top_p=1.0,
                            repetition_penalty=1.05,
                            max_tokens=max_tokens,
                            verbose=False,
                            stream=False,
                        )
                    )
                    if results and all(
                        int(result.token_count) < max_tokens for result in results
                    ):
                        break
                    del results
                    results = []
                    mx.clear_cache()
                if not results:
                    raise ValueError(
                        "Qwen generation reached its token limit on both seeded attempts"
                    )
                synthesis_seconds = time.perf_counter() - started
                chunks = [
                    np.asarray(result.audio, dtype=np.float32).reshape(-1)
                    for result in results
                ]
                if not chunks:
                    raise ValueError("Qwen returned no audio chunks")
                audio = np.ascontiguousarray(np.concatenate(chunks), dtype="<f4")
                if audio.size == 0:
                    raise ValueError("Qwen returned empty audio")
                if not np.isfinite(audio).all():
                    raise ValueError("Qwen returned non-finite PCM")
                payload = audio.tobytes()
                sample_count = int(audio.size)
                del audio, chunks, results
                mx.clear_cache()

            write_header(
                {
                    "status": "audio",
                    "sample_count": sample_count,
                    "sample_rate": sample_rate,
                    "synthesis_seconds": synthesis_seconds,
                    "peak_bytes": int(mx.get_peak_memory()),
                    "generation_attempts": generation_attempts,
                }
            )
            PROTOCOL_OUTPUT.write(payload)
            PROTOCOL_OUTPUT.flush()
        except Exception as error:  # noqa: BLE001 - error crosses a process boundary
            with contextlib.suppress(Exception):
                mx.clear_cache()
            write_header({"status": "error", "message": str(error)})

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
