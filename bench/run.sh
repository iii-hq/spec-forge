#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

case "${1:-v2}" in
  v2)
    node "$SCRIPT_DIR/v2-bench.mjs"
    ;;

  rust)
    echo ""
    echo "  Rust native benchmarks"
    echo "  ━━━━━━━━━━━━━━━━━━━━━━"
    cd "$ROOT_DIR"
    cargo build --release --bin bench 2>&1 | tail -1
    ./target/release/bench
    ;;

  all)
    echo ""
    echo "  Phase 1: Rust native"
    cd "$ROOT_DIR"
    if command -v cargo &>/dev/null; then
      cargo build --release --bin bench 2>&1 | tail -1
      ./target/release/bench
    else
      echo "  cargo not found — skipping"
    fi

    echo ""
    echo "  Phase 2: v2 benchmarks (requires engine + worker)"
    E2E_URL="${SPEC_FORGE_URL:-http://localhost:3111}"
    if curl -sf "$E2E_URL/spec-forge/health" >/dev/null 2>&1; then
      node "$SCRIPT_DIR/v2-bench.mjs"
    else
      echo "  spec-forge not running — skipping"
      echo "  Start: iii --config iii-config.yaml & cargo run --release --bin spec-forge &"
    fi
    ;;

  *)
    echo "Usage: $0 [v2|rust|all]"
    echo ""
    echo "  v2   — v2 benchmarks: transport, caching, collaboration, streaming (default)"
    echo "  rust — Rust native: validation, parsing, cache operations"
    echo "  all  — Everything"
    exit 1
    ;;
esac
