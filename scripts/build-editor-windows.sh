#!/bin/sh
# Cross-build the no-Python editor for Windows from macOS/Linux using Rust + Zig.
set -eu
cd "$(dirname "$0")/.."
root=$(pwd)
command -v zig >/dev/null
command -v rustup >/dev/null
toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' rust-toolchain.toml)
export RUSTC="$(rustup which --toolchain "$toolchain" rustc)"
export RUSTDOC="$(rustup which --toolchain "$toolchain" rustdoc)"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$root/scripts/windows-zig-linker.sh"
flag_sep=$(printf '\037')
export CARGO_ENCODED_RUSTFLAGS="-C${flag_sep}dlltool=$root/scripts/windows-zig-dlltool.sh${flag_sep}-C${flag_sep}strip=debuginfo${flag_sep}--remap-path-prefix=$HOME=/build${flag_sep}--remap-path-prefix=$root=/shimeji2"
export ZIG_GLOBAL_CACHE_DIR="$root/target/zig-cache"
"$(rustup which --toolchain "$toolchain" cargo)" build --locked --release \
    --config 'http.proxy=""' --config 'https.proxy=""' \
    -p shimeji-editor --bin shimeji-editor --target x86_64-pc-windows-gnu "$@"
python3 scripts/bundle-editor-bin.py \
    --exe target/x86_64-pc-windows-gnu/release/shimeji-editor.exe \
    --platform windows
