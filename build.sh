#!/usr/bin/env bash
set -euo pipefail

if ! cargo install --list 2>/dev/null | grep -q '^worker-build v0\.1\.'; then
  echo "ERROR: worker-build 0.1.x is required for worker = 0.4.0."
  echo "       Install: cargo install worker-build --version 0.1.14 --force"
  exit 1
fi

echo "==> Building release WASM..."
worker-build --release

# Find the generated WASM file
WASM_FILE=$(find build -name "*.wasm" | head -1)
[ -z "$WASM_FILE" ] && { echo "ERROR: No .wasm file found in build/"; exit 1; }

echo "==> Running wasm-opt -Oz on $WASM_FILE..."
# Pin binaryen version 117 in CI (see .github/workflows/deploy.yml).
# Newer versions of wasm-opt have had miscompile issues with CF Workers.
wasm-opt --enable-bulk-memory -Oz "$WASM_FILE" -o "${WASM_FILE}.opt"
mv "${WASM_FILE}.opt" "$WASM_FILE"

echo "==> Bundle size (gzip):"
GZIP_SIZE=$(gzip -c "$WASM_FILE" | wc -c)
echo "    ${GZIP_SIZE} bytes ($(echo "scale=1; $GZIP_SIZE/1024" | bc) KB)"

LIMIT=$((800 * 1024))  # 800KB hard limit (CF platform limit is 1MB; we leave margin)
if [ "$GZIP_SIZE" -gt "$LIMIT" ]; then
  echo "ERROR: Bundle exceeds 800KB target!"
  echo "       Diagnose with: twiggy top $WASM_FILE | head -20"
  exit 1
fi
echo "==> Size OK"
