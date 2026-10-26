#!/usr/bin/env bash
# Cross-compile the Windows binaries from Linux or WSL.
#
# Produces 64-bit Windows executables under
#   target/x86_64-pc-windows-gnu/release/
#
# Requirements: rustup, and the mingw-w64 toolchain:
#   Debian/Ubuntu: sudo apt install gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64
#   Fedora:        sudo dnf install mingw64-gcc
set -euo pipefail
cd "$(dirname "$0")"

rustup target add x86_64-pc-windows-gnu

cargo build --release --target x86_64-pc-windows-gnu --features gui --bins

echo
echo "Built:"
echo "  target/x86_64-pc-windows-gnu/release/hdpassw.exe"
echo "  target/x86_64-pc-windows-gnu/release/hdpassw-gui.exe"
