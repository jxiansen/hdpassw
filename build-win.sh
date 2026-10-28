#!/usr/bin/env bash
# Build the Windows binaries.
#
# Two ways to produce the same artifacts:
#
#   1. On Windows, with the MSVC toolchain (recommended for Windows work):
#        rustup target add x86_64-pc-windows-msvc
#        ./build-win.sh --msvc
#
#   2. Cross-compiling from Linux (what CI does), using mingw-w64:
#        sudo apt install gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64
#        ./build-win.sh              # GNU target, the default
#        ./build-win.sh --msvc       # MSVC target via cargo-xwin + LLVM
#
# Both targets link the C runtime statically (see .cargo/config.toml), so the
# resulting .exe files run on a clean Windows install with no redistributables
# and no mingw DLLs to copy alongside them.
#
# Output:
#   target/x86_64-pc-windows-{gnu,msvc}/release/hdpassw.exe
#   target/x86_64-pc-windows-{gnu,msvc}/release/hdpassw-gui.exe
set -euo pipefail
cd "$(dirname "$0")"

TARGET="x86_64-pc-windows-gnu"
USE_XWIN=0

for arg in "$@"; do
    case "$arg" in
        --msvc)
            TARGET="x86_64-pc-windows-msvc"
            USE_XWIN=1
            ;;
        --gnu)
            TARGET="x86_64-pc-windows-gnu"
            USE_XWIN=0
            ;;
        -h|--help)
            sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown argument: $arg (try --help)" >&2
            exit 2
            ;;
    esac
done

if [ "$USE_XWIN" = 1 ]; then
    # cargo-xwin downloads the MSVC CRT/SDK headers itself, so no Windows
    # machine or Visual Studio is needed. It does need LLVM's lib tools.
    if ! command -v cargo-xwin >/dev/null 2>&1; then
        echo "cargo-xwin not found. Install it with:  cargo install cargo-xwin" >&2
        exit 1
    fi
    if ! command -v llvm-lib >/dev/null 2>&1; then
        # Debian/Ubuntu ship it under a versioned directory.
        for d in /usr/lib/llvm-*/bin; do
            [ -x "$d/llvm-lib" ] && PATH="$d:$PATH" && break
        done
    fi
    command -v llvm-lib >/dev/null 2>&1 || {
        echo "llvm-lib not found. Install LLVM (e.g. apt install llvm)." >&2
        exit 1
    }
    cargo xwin build --release --target "$TARGET" --features gui --bins
else
    rustup target add "$TARGET"
    cargo build --release --target "$TARGET" --features gui --bins
fi

OUT="target/$TARGET/release"
echo
echo "Built ($TARGET):"
echo "  $OUT/hdpassw.exe"
echo "  $OUT/hdpassw-gui.exe"
echo
echo "Both link the C runtime statically — copy them to any 64-bit Windows"
echo "machine and run; no DLLs or redistributables needed."
