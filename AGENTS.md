# AGENTS.md

## Repository Map

- The Rust 2024 workspace has two unpublished packages: `rawdinal` is the safe `rlib`, CLI, examples, tests, and benchmark; `ffi/` is the `rawdinal-ffi` C `staticlib`.
- The core public API is exported by `src/lib.rs`. Parsing and decoding live in `src/container.rs`; CAMF metadata in `src/camf.rs`; bitstream primitives in `src/entropy.rs`; EXIF parsing in `src/photo.rs`; rendering in the explicitly experimental `src/experimental.rs`.
- Keep the C contract synchronized across `include/rawdinal.h` and `ffi/src/lib.rs`. Parsing and rendering belong in the safe core; FFI functions own pointer validation, panic boundaries, and C lifetimes.
- `tests/reference_dump.c` is an external fixture generator requiring X3F Tools; Cargo and CMake do not build it.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p rawdinal --test decoding TEST_NAME -- --exact
cargo test -p rawdinal MODULE::TEST_NAME -- --exact
cargo test -p rawdinal-ffi tests::TEST_NAME -- --exact
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
```

- CMake requires native Linux and Cargo, then runs `cargo build --release --offline --locked -p rawdinal-ffi` into `build/cargo/`. Cross-compilation is intentionally rejected.
- Exercise independent real-file decoding with `X3F_SAMPLE=/path/file.x3f X3F_REFERENCE_DIR=/path/reference cargo test -p rawdinal --test decoding sample_matches_independent_reference_byte_for_byte -- --ignored --exact`.
- Run the benchmark with `X3F_SAMPLE=/path/file.x3f cargo bench -p rawdinal --bench decode`.
- Inspect a file with `cargo run --locked --bin rawdinal-inspect -- INPUT.x3f [EXISTING_DUMP_DIRECTORY]`; dump files must not already exist.
- Render with `cargo run --locked --example render -- INPUT.x3f OUTPUT.pfm WB_PRESET bilinear|guided`; the output file must not already exist.

## Constraints

- Rust 1.85+ and native Linux are required. Keep `Cargo.lock` tracked; the workspace currently has no third-party Rust dependencies.
- The safe core forbids `unsafe`; restrict required unsafe operations to explicit blocks in `rawdinal-ffi`.
- Current support is deliberately narrow: X3F 4.2, TRUE format `0x25`, type-5 CAMF, and validated sd Quattro geometry. Do not imply general Foveon support or production-grade rendering.
- Default tests use synthetic fixtures. A green workspace suite does not replace the ignored, byte-for-byte real-camera reference test.
- Preserve C ownership rules: preview bytes borrow the input; decoded handles own EXIF until `rawdinal_free`; RGBA is copied into caller-owned storage.
