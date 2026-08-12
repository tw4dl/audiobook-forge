# Contributing

Thank you for helping improve `audiobook-forge`.

## Before you start

- Read the [Code of Conduct](CODE_OF_CONDUCT.md).
- For security issues, follow [SECURITY.md](SECURITY.md). Do not open a public issue.
- Check existing issues and pull requests before starting a large change.
- Keep book files, model weights, generated audio, caches, credentials, and local `.lavish` artifacts out of commits.

## Local setup

Install Rust 1.88 or newer, Xcode Command Line Tools, CMake, and FFmpeg with `ffprobe`.

```sh
brew install cmake ffmpeg
cargo test --workspace --all-targets --all-features --locked
```

The optional Qwen provider needs Apple Silicon macOS, `uv`, and its separate runtime:

```sh
brew install uv
scripts/setup-qwen-runtime.sh
```

The normal test and CI paths do not download TTS models. Use the Qwen and Kokoro benchmark instructions only with an input book you are allowed to process.

## Making changes

- Add or update a regression test for behavior changes.
- Keep input parsing bounded and fail closed on malformed files.
- Preserve source mappings and navigation metadata.
- Keep provider-specific behavior behind the provider seam.
- Do not add cloud TTS, telemetry, API keys, or network access to book content.
- Update user-facing documentation when commands, formats, limits, or licenses change.

Run the full local gate before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo deny check
```

## Pull requests

Use a focused branch and a clear title. The pull request body should explain:

1. What changed.
2. Why it changed.
3. How it was tested.
4. Any platform, model, license, or migration limits.

Keep generated benchmark outputs in `target/` unless a small, reviewable fixture is required. A maintainer may ask for a smaller patch when unrelated generated files or refactors obscure the change.

By contributing, you agree that your work is provided under the repository's [Apache-2.0 license](LICENSE).
