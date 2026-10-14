# Conventions

shimeji2 is a Cargo workspace. These rules hold for every change.

## Crate boundaries

| Crate | Role | Depends on |
|---|---|---|
| `shimeji-expr` | Expression evaluator and JavaRandom | — |
| `shimeji-core` | Model, compile, engine, `Diagnostic` | `shimeji-expr` |
| `shimeji-packs` | Loaders produce pure data; export | `shimeji-core` |
| `shimeji-app` | Settings, library, window rules, journal, agent state, `runtime` | `shimeji-packs`, `shimeji-proto` |
| `shimeji-macos` | Binary. The macOS shell | `shimeji-app`, and the pinned objc2 crates on macOS only |
| `shimeji-proto` | Versioned socket messages (serde only, once the protocol exists) | — |
| `shimeji-hook` | Hook binary | `shimeji-proto` only |
| `shimeji-forge` | Generation CLI | `shimeji-packs`, `shimeji-proto` |

The link direction is `expr → core → packs → app → macos`. `proto` is used by the app, the hook, and forge. Do not add a dependency that points the other way. `shimeji-core` does not depend on `shimeji-packs`. Golden and loader tests that need both live in `shimeji-packs`.

`Diagnostic` lives in `shimeji-core`, because the engine emits runtime diagnostics. `shimeji-app::runtime` owns the fixed-step accumulator, the resync rule, commands, and effects. `shimeji-macos` only supplies timestamps and applies render diffs and effects, so the clock can be tested without AppKit.

## Platform code

Only `shimeji-macos` may contain platform code: objc2, AppKit, CoreGraphics, QuartzCore, ApplicationServices, Foundation, ServiceManagement, AVFAudio, Carbon, or any other Apple framework. Gate it with `cfg(target_os = "macos")`. The objc2 crates are pinned with `=x.y.z` in `[workspace.dependencies]`, and only `shimeji-macos` depends on them, as target-specific dependencies.

Every other crate must build and test on any OS. `shimeji-packs` dependencies stay pure Rust. Pack bytes are decoded in Rust, not by a C library in the app process.

## Corpus

Never commit third-party pack art or corpus data. `corpus/` is gitignored except `manifest.toml` and `README.md`. Record sources and hashes in the manifest. Corpus traces stay on the machine that generated them.

The bundled shime pack is the exception. It is upstream zlib material under `assets/packs/shime/`, copied byte-for-byte. Do not add `Thumbs.db`, `FixPngImage.jar`, or other upstream files that are not part of that pack.

## Reference behavior

- The behavioral reference is the vendored Shimeji-Desktop source in `tools/ee-reference/vendor/` (pinned in `tools/ee-reference/PIN`). When porting engine semantics, read that code. When older summaries disagree with the pinned source, the pinned source wins.
- The tick is 40 ms (25 Hz). Durations and velocities stay in ticks and pixels per tick inside the engine; only the macOS shell deals in wall-clock time.
- Expected expression results come from `tools/ee-reference/dump-vectors.sh` (the reference's own Nashorn), never hand-edited.

## Testing

- Logic outside the AppKit shell is tested headlessly.
- Automated tests in `shimeji-macos` cover pure logic only (coordinate conversion, parsing, and the like). Behavior that needs a window server goes in a manual checklist under `docs/testing/` when the unit asks for one.
- These commands pass before a unit is done:

  ```sh
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo test --workspace
  ```

- When a unit's execution note says test-first or characterization-first, write that test before the behavior.
- Commit golden traces only for zlib-licensed fixtures (the bundled shime, and synthetic packs). A test that needs the corpus is `#[ignore]`.
- Generate hostile fixtures (zip bombs, symlinks, fifos) inside the test. Do not commit them.
- `assets/packs/shime/CHECKSUMS.sha256` is the byte-identity check for the 46 PNGs and the two XML files. `bundled_shime_files_match_checksums` in `shimeji-packs` reads it. Update the list only when the upstream bytes themselves change, and say so.
