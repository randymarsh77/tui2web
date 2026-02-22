#!/usr/bin/env bash
# build.sh – compile the tui2web example to WebAssembly and stage it for serving.
#
# Prerequisites:
#   - Rust toolchain with wasm32-unknown-unknown target:
#       rustup target add wasm32-unknown-unknown
#   - wasm-pack:
#       cargo install wasm-pack
#       # or: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
#
# Usage:
#   ./build.sh            # build
#   ./build.sh --serve    # build then serve locally via npx serve

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$REPO_ROOT/example"
WEB_DIR="$REPO_ROOT/web"
PKG_OUT="$WEB_DIR/pkg"

# ── Check dependencies ────────────────────────────────────────────────────────
if ! command -v wasm-pack &>/dev/null; then
  echo "ERROR: wasm-pack not found."
  echo "Install it with: cargo install wasm-pack"
  echo "or: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh"
  exit 1
fi

# ── Build ─────────────────────────────────────────────────────────────────────
echo "▶ Compiling Rust → WebAssembly …"
wasm-pack build \
  "$EXAMPLE_DIR" \
  --target web \
  --out-dir "$PKG_OUT" \
  --release

echo ""
echo "✓ Build complete. Output written to: web/pkg/"
echo ""

# ── Optionally serve ──────────────────────────────────────────────────────────
if [[ "${1:-}" == "--serve" ]]; then
  echo "▶ Starting local HTTP server at http://localhost:8080 …"
  echo "  (Press Ctrl-C to stop)"
  echo ""
  if command -v npx &>/dev/null; then
    npx serve "$WEB_DIR" -l 8080
  else
    echo "ERROR: npx not found. Install Node.js or use another HTTP server."
    exit 1
  fi
fi
