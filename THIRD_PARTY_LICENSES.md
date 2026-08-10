# Third-party licenses

`kokoro-book` original code is Apache-2.0. This repository does not commit model weights or third-party binary artifacts.

## License gate

The three runtime dependencies that can grow optional native features have defaults disabled in `Cargo.toml`:

```toml
misaki-rs = { version = "0.3.0", default-features = false }
ort = { version = "=2.0.0-rc.11", default-features = false, features = ["copy-dylibs", "download-binaries", "ndarray", "std", "tls-rustls"] }
rbook = { version = "0.7.10", default-features = false }
```

`Cargo.lock` contains no `espeak-rs`, `sherpa-onnx`, GPL, or LGPL package. `cargo deny check licenses bans sources` checks every locked Rust package. The accepted license list in `deny.toml` contains no GPL or LGPL identifier.

## Main Rust and native components

| Component | Use | License | Source |
| --- | --- | --- | --- |
| `rbook` 0.7.10 | EPUB 2 and 3 reading | Apache-2.0 | [DevinSterling/rbook](https://github.com/DevinSterling/rbook) |
| `misaki-rs` 0.3.0 | English G2P, lexicons, and POS data | MIT for the Rust port; its upstream documentation points underlying dictionary data to Apache-2.0 Misaki | [crates.io package](https://crates.io/crates/misaki-rs/0.3.0), [MicheleYin/misaki-rs](https://github.com/MicheleYin/misaki-rs), [hexgrad/misaki](https://github.com/hexgrad/misaki) |
| `language-tokenizer` 0.1.0 | Transitive tokenization support | WTFPL | [savannstm/language-tokenizer at the packaged revision](https://github.com/savannstm/language-tokenizer/tree/86f2cbc67384d9913186c3ae0b3e862359349c31) |
| `ort` and `ort-sys` 2.0.0-rc.11 | Safe Rust ONNX Runtime API and bindings | MIT OR Apache-2.0 | [pykeio/ort](https://github.com/pykeio/ort) |
| ONNX Runtime 1.23.2 | Native inference runtime selected by `ort-sys` | MIT | [microsoft/onnxruntime v1.23.2](https://github.com/microsoft/onnxruntime/tree/v1.23.2) |

`language-tokenizer` omits a manifest license field. Its packaged `LICENSE.md` is WTFPL. `deny.toml` pins that conclusion to cargo-deny's opaque file hash `0x2915ab65`, so a changed license file fails the clarification.

Other locked Rust crates use one or more of: 0BSD, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-3-Clause, CDLA-Permissive-2.0, ISC, MIT, MPL-2.0, Unicode-3.0, Unlicense, WTFPL, or Zlib. Exact versions and checksums are in `Cargo.lock`. Feature selections are in `Cargo.toml`. The `cargo deny` report checks the license expressions.

## Downloaded Kokoro assets

The CLI pins [`onnx-community/Kokoro-82M-v1.0-ONNX`](https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/tree/1939ad2a8e416c0acfeecc08a694d14ef25f2231) at revision `1939ad2a8e416c0acfeecc08a694d14ef25f2231`.

| Files | Use | License and integrity |
| --- | --- | --- |
| `onnx/model_q8f16.onnx` | Kokoro v1.0 inference | Apache-2.0; SHA-256 `04c658aec1b6008857c2ad10f8c589d4180d0ec427e7e6118ceb487e215c3cd0` |
| `voices/<preset>.bin` | One selected preset voice | Apache-2.0; every English voice SHA-256 is pinned in `src/voice.rs` |
| Kokoro v1.0 vocabulary | Embedded phoneme-to-token map | Apache-2.0; copied from [`hexgrad/Kokoro-82M` config revision `f3ff3571`](https://huggingface.co/hexgrad/Kokoro-82M/blob/f3ff3571791e39611d31c381e3a41a3af07b4987/config.json) |

The model repository declares Apache-2.0. The project does not download an eSpeak binary, eSpeak data, a combined voice bundle, or another model.
