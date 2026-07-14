#!/usr/bin/env bash
set -euo pipefail

FAILED=0

echo "[pre-commit] Checking formatting."

cargo fmt --all --check || FAILED=1
cargo fmt --all --check --manifest-path compositor/Cargo.toml || FAILED=1
cargo fmt --all --check --manifest-path launcher/Cargo.toml || FAILED=1

if [[ $FAILED -ne 0 ]]; then
    echo ""
    echo "[pre-commit] Formatting issues found. Run 'just fmt' to fix them."
    exit 1
fi

echo "[pre-commit] Checking build."

cargo check --workspace || FAILED=1
cargo check --manifest-path compositor/Cargo.toml || FAILED=1
cargo check --manifest-path launcher/Cargo.toml || FAILED=1

if [[ $FAILED -ne 0 ]]; then
    echo ""
    echo "[pre-commit] Compilation failed"
    exit 1
fi
