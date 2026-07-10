#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (c) 2024-2026 vi-im contributors
# ============================================================================
# compile.sh — Build vi-ime from source with all optimizations
# Usage: ./deploy/compile.sh [--release|--debug]
# ============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

PROFILE="${1:-release}"

cd "$PROJECT_DIR"

echo "============================================"
echo " vi-ime — Build Script"
echo "============================================"
echo " Project:  $PROJECT_DIR"
echo " Profile:  $PROFILE"
echo " Rust:     $(rustc --version)"
echo "============================================"

# Check system dependencies
echo ""
echo "[1/4] Checking system deps..."
MISSING=""
for lib in libwayland-client; do
    if ! pkg-config --exists "$lib" 2>/dev/null; then
        echo "  ✗ MISSING: $lib"
        MISSING="$MISSING $lib"
    else
        echo "  ✓ Found: $lib"
    fi
done

if [ -n "$MISSING" ]; then
    echo ""
    echo "  Install missing deps:"
    echo "    Ubuntu: sudo apt install libwayland-dev"
    echo "    Arch:   sudo pacman -S wayland"
fi

# Check Rust toolchain
echo ""
echo "[2/4] Checking Rust toolchain..."
if ! command -v rustc &>/dev/null; then
    echo "  ✗ rustc not found! Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi
echo "  ✓ rustc $(rustc --version)"
echo "  ✓ cargo $(cargo --version)"

# Clean previous build artifacts (optional, skip for incremental)
if [ "${CLEAN:-0}" = "1" ]; then
    echo ""
    echo "[3/4] Cleaning old build..."
    cargo clean
fi

# Build
echo ""
echo "[4/4] Building vi-ime ($PROFILE)..."
if [ "$PROFILE" = "release" ]; then
    cargo build --release -p vi-ime
    BINARY="target/release/vi-ime"
else
    cargo build -p vi-ime
    BINARY="target/debug/vi-ime"
fi

echo ""
echo "============================================"
echo " Build complete!"
echo " Binary: $BINARY"
echo " Size:   $(du -h "$BINARY" | cut -f1)"
echo "============================================"
echo ""
echo "Next: ./deploy/install.sh"
