#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "╔══════════════════════════════════════════════════════════════════════════╗"
echo "║     spec-forge vs json-render — Full Benchmark Suite                   ║"
echo "╚══════════════════════════════════════════════════════════════════════════╝"
echo ""

case "${1:-all}" in
  js|node)
    echo "━━━ Running JavaScript benchmarks only ━━━"
    echo ""
    echo "── json-render (Vercel Labs) ──"
    node "$SCRIPT_DIR/json-render-bench.mjs"
    echo ""
    echo "── spec-forge (iii-sdk) ──"
    node "$SCRIPT_DIR/spec-forge-bench.mjs"
    ;;

  rust)
    echo "━━━ Running Rust benchmark only ━━━"
    echo ""
    cd "$ROOT_DIR"
    cargo build --release --bin bench 2>&1 | tail -1
    ./target/release/bench
    ;;

  compare)
    echo "━━━ Running side-by-side comparison ━━━"
    echo ""
    node "$SCRIPT_DIR/compare.mjs"
    ;;

  e2e)
    echo "━━━ Running end-to-end benchmark ━━━"
    echo "(requires iii engine + spec-forge worker running)"
    echo ""
    node "$SCRIPT_DIR/e2e-bench.mjs"
    ;;

  all)
    echo "━━━ Phase 1: Rust (spec-forge native) ━━━"
    echo ""
    cd "$ROOT_DIR"
    if command -v cargo &>/dev/null; then
      cargo build --release --bin bench 2>&1 | tail -1
      ./target/release/bench
    else
      echo "  cargo not found — skipping Rust benchmark"
      echo "  Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    fi

    echo ""
    echo "━━━ Phase 2: JavaScript comparison (json-render vs spec-forge) ━━━"
    echo ""
    node "$SCRIPT_DIR/compare.mjs"

    echo ""
    echo "━━━ Phase 3: End-to-end (if server running) ━━━"
    echo ""
    if curl -sf http://localhost:3111/spec-forge/health >/dev/null 2>&1; then
      node "$SCRIPT_DIR/e2e-bench.mjs"
    else
      echo "  spec-forge not running on :3111 — skipping e2e benchmark"
      echo "  To run: iii --config iii-config.yaml & cargo run --release &"
    fi
    ;;

  *)
    echo "Usage: $0 [js|rust|compare|e2e|all]"
    echo ""
    echo "  js      — Run JavaScript benchmarks only (json-render + spec-forge)"
    echo "  rust    — Run Rust benchmark only (cargo build --release)"
    echo "  compare — Run side-by-side JS comparison with formatted output"
    echo "  e2e     — Run end-to-end latency through iii engine (server must be running)"
    echo "  all     — Run everything (default)"
    exit 1
    ;;
esac
