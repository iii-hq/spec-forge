# spec-forge vs json-render — Benchmark Suite

Comprehensive benchmarks comparing [spec-forge](https://github.com/iii-hq/spec-forge) (Rust iii-sdk worker) against [json-render](https://github.com/vercel-labs/json-render) (Vercel Labs, JS/TS).

## Quick Start

```bash
# Run everything (Rust + JS comparison + e2e if server running)
./bench/run.sh

# Individual benchmarks
./bench/run.sh rust      # Rust only (cargo build --release)
./bench/run.sh js        # JavaScript only (both frameworks)
./bench/run.sh compare   # Side-by-side comparison table
./bench/run.sh e2e       # End-to-end through iii engine
```

## What's Measured

### Phase 1: Rust Native (`src/bench.rs`)

Benchmarks the spec-forge Rust worker's actual production code with `std::hint::black_box` to prevent dead code elimination:

| Category | Tests |
|----------|-------|
| JSONL Patch Parsing | 3, 9 patches (RFC 6902) |
| JSON Parse (serde) | 3, 9, 50, 500, 2000 elements |
| Validation | 3, 9, 50, 500, 2000 elements |
| Prompt Build | Simple, medium, complex prompts |
| SHA-256 Cache Key | Single key generation |
| Exact Cache | Hit + miss with TTL |
| TF-IDF Semantic Cache | Hit/miss at 10 and 100 entries |
| JSON Stringify (serde) | 3, 9, 50, 500, 2000 elements |
| Full Pipeline | Parse → validate → stringify |
| Cold Pipeline | JSONL parse → cache key → validate → store |

### Phase 2: JavaScript Comparison (`bench/compare.mjs`)

Runs both frameworks in Node.js for apples-to-apples V8 comparison:

**json-render** (`json-render-bench.mjs`):
- `createSpecStreamCompiler` — JSONL patch streaming (bulk, chunked, token-by-token)
- `catalog.prompt()` — System prompt generation
- `specValidator` — Structural validation + orphan detection
- `resolveProps` — Dynamic prop expressions ($state, $cond, $template)
- `immutableSetByPath` — Structural sharing state updates
- JSON parse/stringify at all sizes

**spec-forge** (`spec-forge-bench.mjs`):
- `parse_jsonl_patches` — Server-side JSONL parsing
- WebSocket channel message processing
- `build_prompt` — JSONL-focused prompt generation
- `validate_spec` — Server-side validation
- SHA-256 exact cache (key generation, hit, miss)
- TF-IDF semantic cache (hit/miss at 10 and 100 entries)
- Rate limiter (token bucket + concurrency semaphore)
- Full pipeline (cache check → parse → validate → store)

### Phase 3: End-to-End (`bench/e2e-bench.mjs`)

Measures real HTTP latency through the iii engine (requires running server):

- `/health` — Baseline HTTP roundtrip
- `/stats` — Metrics endpoint
- `/validate` — Validation without LLM
- `/generate` cold — Full LLM call (unique prompts)
- `/generate` warm — Cached response (repeat prompts)
- `/stream` — WebSocket channel first-patch latency
- iii engine overhead calculation (total - LLM time)

## Architecture Comparison

| | json-render | spec-forge + iii |
|---|---|---|
| Runtime | Node.js (V8) | Rust (iii-sdk worker) |
| Streaming | Vercel AI SDK (SSE → line split) | iii Channels (WebSocket, per-patch) |
| First paint | After first complete JSONL line | After first patch (~200ms) |
| Caching | None | SHA-256 exact + TF-IDF semantic |
| Repeat request | Full LLM call (3-5s) | 0ms (cache hit) |
| Rate limiting | None | Token bucket + concurrency |
| API key | Client-side (exposed) | Server-side only |
| Observability | None | OpenTelemetry (traces, metrics, logs) |
| Validation | Client-side + auto-fix | Server-side (reject invalid) |
| Refinement | Full regeneration | JSONL patch diffing |
| Dynamic props | $state, $cond, $template, $computed | Static (render-time) |
| State management | Built-in StateStore + adapters | Client-side (any framework) |

## Files

```text
bench/
├── README.md              # This file
├── run.sh                 # Orchestrator script
├── data.json              # Test fixtures (catalogs, specs, JSONL samples)
├── json-render-bench.mjs  # json-render core operations (faithful reimplementation)
├── spec-forge-bench.mjs   # spec-forge TS client operations
├── compare.mjs            # Side-by-side comparison runner
└── e2e-bench.mjs          # End-to-end latency through iii engine
src/
└── bench.rs               # Rust native benchmark (with black_box)
```
