#!/usr/bin/env bash
set -euo pipefail
RESULT=$(cargo tree --target x86_64-unknown-linux-gnu --edges normal 2>/dev/null \
  | grep -E '(freetype-sys v|fontconfig-sys v|harfbuzz-sys v|brotli v|flate2 v|miniz_oxide v|ring v)' || true)
if [ -n "$RESULT" ]; then
  echo "FFI AUDIT FAILED:"
  echo "$RESULT"
  exit 1
fi
echo "FFI audit PASSED"
