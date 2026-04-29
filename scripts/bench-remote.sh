#!/usr/bin/env bash
# Cross-compile Criterion benchmarks for aarch64 and run them on a remote device via SSH.
#
# Usage:
#   BENCH_DEVICE=user@192.168.1.10 ./scripts/bench-remote.sh
#   BENCH_DEVICE=mecha@192.168.0.119 BENCH_PASSWORD=mecha ./scripts/bench-remote.sh
#
# Env vars:
#   BENCH_DEVICE      SSH target (required). E.g.: root@imx8mp.local
#   BENCH_PASSWORD    SSH password (optional). Uses sshpass when set.
#   BENCH_REMOTE_DIR  Upload directory on the device. Default: /tmp/launcher-benches
#   SSH_OPTS          Extra ssh/scp flags. E.g.: "-i ~/.ssh/id_ed25519_imx8mp -p 2222"
#   WAYLAND_DISPLAY   Wayland socket name on the device. Default: wayland-0
#   XDG_RUNTIME_DIR   Runtime dir on the device. Default: /run/user/0
#
# Prerequisites (host):
#   - cross installed: cargo install cross
#   - launcher-cross-aarch64:latest image built:
#       docker build -f Dockerfile.cross -t launcher-cross-aarch64:latest .
#   - SSH key-based access to BENCH_DEVICE, or BENCH_PASSWORD set (requires sshpass)
#
# Prerequisites (device):
#   - /dev/dri/renderD128 accessible (renderer benches)
#   - Running Wayland compositor on $WAYLAND_DISPLAY (wayland-protocols benches)
#   - libgbm.so.1, libdrm.so.2, libEGL.so.1 present (Mesa / NXP Vivante stack)

set -euo pipefail

BENCH_DEVICE="${BENCH_DEVICE:?BENCH_DEVICE must be set (e.g. root@192.168.1.10)}"
BENCH_PASSWORD="${BENCH_PASSWORD:-}"
BENCH_REMOTE_DIR="${BENCH_REMOTE_DIR:-/tmp/launcher-benches}"
SSH_OPTS="${SSH_OPTS:-}"
WAYLAND_DISPLAY_REMOTE="${WAYLAND_DISPLAY:-wayland-0}"
XDG_RUNTIME_DIR_REMOTE="${XDG_RUNTIME_DIR:-/run/user/0}"

# Wrap ssh/scp with sshpass when a password is provided.
if [[ -n "${BENCH_PASSWORD}" ]]; then
    if ! command -v sshpass &>/dev/null; then
        echo "[bench-remote] ERROR: BENCH_PASSWORD is set but sshpass is not installed." >&2
        exit 1
    fi
    SSH_CMD="sshpass -p ${BENCH_PASSWORD} ssh -o StrictHostKeyChecking=no"
    SCP_CMD="sshpass -p ${BENCH_PASSWORD} scp -o StrictHostKeyChecking=no"
else
    SSH_CMD="ssh"
    SCP_CMD="scp"
fi

TARGET="aarch64-unknown-linux-gnu"
DEPS_DIR="target/${TARGET}/release/deps"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── Step 1: Build bench binaries ─────────────────────────────────────────────

echo "[bench-remote] Building benchmarks for ${TARGET} (--no-run)..."
cd "${REPO_ROOT}"
cross bench --target "${TARGET}" --workspace --no-run

# ── Step 2: Discover bench binaries ──────────────────────────────────────────
# Filter to only the real Criterion entry points:
#   renderer-<hash>           from crates/renderer [[bench]] name="renderer"
#   wayland_protocols-<hash>  from crates/wayland-protocols [[bench]] name="wayland-protocols"
#
# Individual bench source files (draw_rect-*, lavender-*, etc.) compile as
# default-harness stubs with no #[bench] fns — they exit immediately and are
# not useful, so we skip them via the name-prefix filter.

echo "[bench-remote] Discovering aarch64 bench binaries..."
BENCH_BINS=()
while IFS= read -r -d '' candidate; do
    name="$(basename "${candidate}")"
    if [[ ! "${name}" =~ ^(renderer|wayland_protocols)- ]]; then
        continue
    fi
    if file "${candidate}" 2>/dev/null | grep -q "ELF 64-bit LSB.*aarch64\|ELF 64-bit LSB.*ARM aarch64"; then
        BENCH_BINS+=("${candidate}")
        echo "[bench-remote]   Found: ${name}"
    fi
done < <(find "${DEPS_DIR}" -maxdepth 1 -type f \
    ! -name "*.d" ! -name "*.rlib" ! -name "*.rmeta" ! -name "*.so" -print0)

if [[ ${#BENCH_BINS[@]} -eq 0 ]]; then
    echo "[bench-remote] ERROR: No aarch64 bench binaries found in ${DEPS_DIR}/" >&2
    exit 1
fi

# ── Step 3: Upload to device ──────────────────────────────────────────────────

echo "[bench-remote] Uploading ${#BENCH_BINS[@]} binary/binaries to ${BENCH_DEVICE}:${BENCH_REMOTE_DIR}..."
# shellcheck disable=SC2086
${SSH_CMD} ${SSH_OPTS} "${BENCH_DEVICE}" "mkdir -p '${BENCH_REMOTE_DIR}'"
# shellcheck disable=SC2086
${SCP_CMD} ${SSH_OPTS} "${BENCH_BINS[@]}" "${BENCH_DEVICE}:${BENCH_REMOTE_DIR}/"

# ── Step 4: Run each bench remotely ──────────────────────────────────────────

OVERALL_EXIT=0
for bin in "${BENCH_BINS[@]}"; do
    bin_name="$(basename "${bin}")"
    echo ""
    echo "[bench-remote] ── ${bin_name} --bench ──────────────────────────────"
    # shellcheck disable=SC2086
    ${SSH_CMD} ${SSH_OPTS} "${BENCH_DEVICE}" \
        "XDG_RUNTIME_DIR='${XDG_RUNTIME_DIR_REMOTE}' \
         WAYLAND_DISPLAY='${WAYLAND_DISPLAY_REMOTE}' \
         LD_LIBRARY_PATH='/usr/lib/aarch64-linux-gnu:/usr/lib' \
         chmod +x '${BENCH_REMOTE_DIR}/${bin_name}' \
         && '${BENCH_REMOTE_DIR}/${bin_name}' --bench" \
    || { echo "[bench-remote] WARNING: ${bin_name} exited non-zero." >&2; OVERALL_EXIT=1; }
done

echo ""
if [[ ${OVERALL_EXIT} -eq 0 ]]; then
    echo "[bench-remote] All benchmarks completed successfully."
else
    echo "[bench-remote] One or more benchmarks exited non-zero. Check output above." >&2
fi

exit ${OVERALL_EXIT}
