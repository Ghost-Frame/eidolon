#!/usr/bin/env bash
# generate-instincts.sh -- generate instincts.bin for a fresh Eidolon instance
#
# Usage: bash generate-instincts.sh [output_path]
# Default output: /data/instincts.bin

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EIDOLON_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
RUST_DIR="${EIDOLON_ROOT}"
OUTPUT_PATH="${1:-${EIDOLON_ROOT}/data/instincts.bin}"

echo "[generate-instincts] building Eidolon release binary..."
cd "${RUST_DIR}"
cargo build --release

BINARY="${RUST_DIR}/target/release/eidolon"

if [[ ! -x "${BINARY}" ]]; then
    echo "[generate-instincts] ERROR: binary not found at ${BINARY}" >&2
    exit 1
fi

echo "[generate-instincts] generating instincts corpus to: ${OUTPUT_PATH}"

# Ensure data directory exists
mkdir -p "$(dirname "${OUTPUT_PATH}")"

RESPONSE=$(echo '{"cmd":"generate_instincts","seq":1,"output_path":"'"${OUTPUT_PATH}"'"}' | "${BINARY}")

echo "[generate-instincts] response: ${RESPONSE}"

if [[ "${RESPONSE}" == *'"ok":true'* ]]; then
    FILE_SIZE=$(stat -c%s "${OUTPUT_PATH}" 2>/dev/null || echo "unknown")
    echo "[generate-instincts] SUCCESS: instincts.bin written, size: ${FILE_SIZE} bytes"
else
    echo "[generate-instincts] ERROR: generation failed" >&2
    echo "${RESPONSE}" >&2
    exit 1
fi
