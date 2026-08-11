# Player compatibility

## VLC 3.0.23 on macOS

Verified on August 10, 2026, on Apple Silicon macOS with repository commit `f9b917146170`.

The public CLI built a real Kokoro audiobook from the checked-in two-chapter, DRM-free Kindle fixture:

```sh
cargo run -- \
  tests/fixtures/kindle/with-cover.azw3 \
  --output target/player-proof/kindle-cover \
  --nav chapters
```

The build produced 13.00 seconds of mono AAC audio, embedded PNG cover art, title `Kindle Fixture`, author `Example Author`, and two visible chapters. The tested M4B SHA-256 was:

```text
e8aee7485b7928b347ee30ee908866dd4f9d4df236a8aadc3007ca721f6999d0
```

FFprobe 8.0.1 independently reported:

```text
AAC audio
00:00.000–00:09.280  Chapter One
00:09.280–00:13.000  Chapter Two
title:  Kindle Fixture
artist: Example Author
cover:  present
```

VLC 3.0.23 Vetinari opened the M4B, advanced playback time, and exposed its chapter controls. A silent remote-control run returned:

```text
get_title   -> Kindle Fixture
get_length  -> 13
chapter     -> 0
get_time    -> 1
chapter_n
chapter     -> 1
get_time    -> 9
chapter_p
chapter     -> 0
get_time    -> 0
```

This proves playback plus forward and backward chapter navigation in the documented V1 target player. VLC uses zero-based chapter indexes. Automated structural checks also validate non-zero duration, AAC codec, metadata, cover presence, ordered chapters, and chapter bounds on every full test run.

Apple Books has not been used for this proof. VLC 3.0.23 is the explicitly documented V1 target player.
