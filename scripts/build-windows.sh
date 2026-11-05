#!/bin/sh
# Cross-build the Windows preview from macOS/Linux using Rust + Zig.
set -eu
cd "$(dirname "$0")/.."
root=$(pwd)
command -v zig >/dev/null
command -v rustup >/dev/null
toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' rust-toolchain.toml)
# rustup must have installed the Windows target beforehand. Never silently
# install tools, modify the user's Rust default, or overwrite their Cargo config.
export RUSTC="$(rustup which --toolchain "$toolchain" rustc)"
export RUSTDOC="$(rustup which --toolchain "$toolchain" rustdoc)"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$root/scripts/windows-zig-linker.sh"
flag_sep=$(printf '\037')
# Encoded flags preserve checkout paths containing spaces. Remap build-host paths
# in panic locations, matching the macOS bundle's source-path hygiene.
export CARGO_ENCODED_RUSTFLAGS="-C${flag_sep}dlltool=$root/scripts/windows-zig-dlltool.sh${flag_sep}-C${flag_sep}strip=debuginfo${flag_sep}--remap-path-prefix=$HOME=/build${flag_sep}--remap-path-prefix=$root=/shimeji2"
export ZIG_GLOBAL_CACHE_DIR="$root/target/zig-cache"
"$(rustup which --toolchain "$toolchain" cargo)" build --locked --release \
    -p shimeji-windows --bin shimeji2 --target x86_64-pc-windows-gnu "$@"
python3 scripts/bundle-windows.py --exe target/x86_64-pc-windows-gnu/release/shimeji2.exe
