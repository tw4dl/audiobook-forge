# Third-party licenses

`kokoro-book` original code is Apache-2.0. This repository does not commit downloaded model weights or third-party native binaries.

## License gate

The optional native features that could add eSpeak are disabled:

```toml
misaki-rs = { version = "0.3.0", default-features = false }
rbook = { version = "0.7.10", default-features = false }

[target.'cfg(all(target_os = "macos", target_arch = "aarch64"))'.dependencies]
mlx-memory-control = { version = "0.1.0", path = "crates/mlx-memory-control" }
voice-tts = "=0.2.1"
```

`Cargo.lock` contains no `ort`, `ort-sys`, ONNX Runtime, `sherpa-onnx`, `espeak-rs`, or eSpeak data package.

The required audit is:

```sh
cargo deny check licenses bans sources
```

The accepted list in `deny.toml` contains no GPL or LGPL identifier. Every resolved package has a permitted license path. Two file-backed clarifications make incomplete or old manifest metadata auditable:

- `language-tokenizer` 0.1.0: its packaged `LICENSE.md` is WTFPL; opaque content hash `0x2915ab65`.
- `mach-sys` 0.5.4: the project selects its Apache-2.0 option; packaged `LICENSE-APACHE-2.0` hash `0xb5518783`.

If either file changes, the clarification stops matching and the audit fails or reports the unresolved license.

## Runtime code and native components

| Component | Use | License | Source |
| --- | --- | --- | --- |
| `voice-tts` 0.2.1 | Kokoro model loading and synthesis | MIT | [`rgbkrk/voicers`](https://github.com/rgbkrk/voicers/tree/58139b9aca4826135b1e549cf14cd357c9c82f56/crates/voice-tts) |
| `voice-dsp` 0.2.0 | MLX signal processing | MIT | [`rgbkrk/voicers`](https://github.com/rgbkrk/voicers) |
| `voice-nn` 0.2.0 | Kokoro neural-network layers | MIT | [`rgbkrk/voicers`](https://github.com/rgbkrk/voicers) |
| `mlx-rs` 0.25.3 | Safe Rust MLX API | MIT OR Apache-2.0 | [`oxideai/mlx-rs`](https://github.com/oxideai/mlx-rs) |
| `mlx-sys` 0.2.0 | MLX C build and raw bindings | MIT OR Apache-2.0 | [`oxideai/mlx-rs`](https://github.com/oxideai/mlx-rs) |
| MLX C sources bundled by `mlx-sys` | Apple MLX native runtime | MIT | [`ml-explore/mlx`](https://github.com/ml-explore/mlx) |
| `mach-sys` 0.5.4 | Mach memory APIs used by `mlx-rs` | GPL-3.0 OR Apache-2.0; this project uses Apache-2.0 | [`delta4chat/mach`](https://github.com/delta4chat/mach) |
| `misaki-rs` 0.3.0 | English G2P | MIT | [`MicheleYin/misaki-rs`](https://github.com/MicheleYin/misaki-rs) |
| Original Misaki English lexicons and G2P data | Four embedded US/GB gold/silver dictionaries plus POS classes, tags, and weights carried by `misaki-rs` | Apache-2.0 | [`hexgrad/misaki`](https://github.com/hexgrad/misaki) |
| `language-tokenizer` 0.1.0 | Transitive Misaki tokenization | WTFPL | [`savannstm/language-tokenizer`](https://github.com/savannstm/language-tokenizer/tree/86f2cbc67384d9913186c3ae0b3e862359349c31) |
| `rbook` 0.7.10 | EPUB 2 and 3 reading | Apache-2.0 | [`DevinSterling/rbook`](https://github.com/DevinSterling/rbook) |

The remaining locked Rust packages use a license expression with at least one branch in the allow list: Apache-2.0, BSD-3-Clause, CDLA-Permissive-2.0, ISC, MIT, MPL-2.0, Unicode-3.0, WTFPL, or Zlib. Exact package versions, registry checksums, target conditions, and full expressions are fixed in `Cargo.lock` and checked by `cargo-deny` for both `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`.

Apple operating-system frameworks are provided by macOS and are not redistributed by this repository.

## Data embedded by `voice-tts`

`voice-tts` 0.2.1 declares MIT and compiles a model configuration plus seven preset voice embedding files into the Rust library package:

- `af_heart`
- `af_bella`
- `af_sarah`
- `af_sky`
- `am_michael`
- `am_adam`
- `bf_emma`

Those voice embeddings are Kokoro model data, whose upstream model license is Apache-2.0. `kokoro-book` does not call the built-in voice loader; it downloads and verifies the selected full 510-frame voice from the pinned model revision. The embedded data remains part of the linked `voice-tts` package, so both its MIT package license and the Kokoro Apache-2.0 model license are documented here.

## Downloaded Kokoro assets

The CLI pins [`mlx-community/Kokoro-82M-bf16`](https://huggingface.co/mlx-community/Kokoro-82M-bf16/tree/a71e4d38b236d968966a2002c4c895dbd12b1c3c) at revision `a71e4d38b236d968966a2002c4c895dbd12b1c3c`. The repository metadata declares Apache-2.0.

| Files | Use | License and integrity |
| --- | --- | --- |
| `kokoro-v1_0.safetensors` | Only model weights used by the CLI | Apache-2.0; SHA-256 `4e9ecdf03b8b6cf906070390237feda473dc13327cb8d56a43deaa374c02acd8` |
| `voices/<preset>.safetensors` | One selected English preset voice | Apache-2.0; all 28 SHA-256 values are pinned in `src/voice.rs` |
| Kokoro v1.0 configuration embedded by `voice-tts` | Model architecture and vocabulary | Apache-2.0 model data; MIT `voice-tts` package |
| Kokoro v1.0 phoneme vocabulary in `src/vocab.rs` | Input validation and normalization | Apache-2.0; copied from [`hexgrad/Kokoro-82M` revision `f3ff3571`](https://huggingface.co/hexgrad/Kokoro-82M/blob/f3ff3571791e39611d31c381e3a41a3af07b4987/config.json) |

The CLI does not download another model, a combined voice bundle, eSpeak, or ONNX Runtime.
