# Rawdinal

Rawdinal is an experimental native Rust decoder and converter for Sigma sd Quattro X3F files. It decodes X3F 4.2 containers, TRUE 0x25 sensor planes, and type-5 CAMF metadata, then provides an experimental linear-sRGB rendering path.

The current implementation is validated only against a small set of sd Quattro HI, ISO 100 files. It is not a general Foveon converter and does not yet provide fully validated camera calibration, clipping reconstruction, or editable sensor-domain white balance.

## Build and test

Rawdinal requires Rust 1.85 or newer and currently supports native Linux builds.

```sh
cargo test --workspace --locked
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
```

The CMake build exposes the `rawdinal` target when included with `add_subdirectory`. Standalone builds install `include/rawdinal.h` with the static library by default; embedded builds can opt in with `RAWDINAL_INSTALL=ON`.

## C API ownership

`rawdinal_preview` returns storage borrowed from the input buffer. `rawdinal_decode` returns an owned opaque image handle whose EXIF pointer remains valid until `rawdinal_free`. `rawdinal_copy_rgba` copies unbounded linear-sRGB RGBA values into caller-owned storage.

## License

Copyright (C) 2026 Paolo SANTUCCI

Rawdinal is distributed under GPL-3.0-only. See `LICENSE` for the complete license text.
Retained X3F Tools portions are licensed under the BSD 3-Clause License; see
`THIRD_PARTY_NOTICES`.
