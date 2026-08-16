#!/usr/bin/env bash
# Builds every release artifact into dist/. Each one is a single file that runs
# on its own: no folders, no assets beside it.
#
#   ./scripts/release.sh            # everything that this machine can build
#   ./scripts/release.sh linux      # or: windows
set -euo pipefail

cd "$(dirname "$0")/.."
version=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
targets=("${@:-linux windows}")
mkdir -p dist

want() { [[ " ${targets[*]} " == *" $1 "* ]]; }
say() { printf '\n== %s ==\n' "$1"; }

if want linux; then
    say "Linux (elf)"
    cargo build --release
    install -m755 target/release/panoptes "dist/panoptes-${version}-linux-x86_64"
fi

if want windows; then
    say "Windows (exe)"
    # Arch's system rust has only the host target, so prefer a per-user rustup
    # toolchain when one carries the Windows standard library.
    windows_cargo=""
    for candidate in "$HOME/.cargo/bin/cargo" cargo; do
        [[ -x "$candidate" || -x "$(command -v "$candidate" 2>/dev/null)" ]] || continue
        if "$candidate" build --release --target x86_64-pc-windows-gnu --quiet 2>/dev/null; then
            windows_cargo="$candidate"
            break
        fi
    done
    if [[ -n "$windows_cargo" ]]; then
        install -m755 target/x86_64-pc-windows-gnu/release/panoptes.exe \
            "dist/panoptes-${version}-windows-x86_64.exe"
    else
        cat <<'MISSING'
skipped: no Rust standard library for x86_64-pc-windows-gnu.

Install it with a per-user rustup (no sudo, nothing outside ~/.cargo, ~/.rustup):

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    ~/.cargo/bin/rustup target add x86_64-pc-windows-gnu

The linker (x86_64-w64-mingw32-gcc) is already here.
MISSING
    fi
fi

say "dist"
ls -lh dist/
