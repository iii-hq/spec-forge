# spec-forge Architecture

## Pure iii-sdk Worker

spec-forge is a **pure iii-sdk worker** — it registers functions and HTTP triggers, then the iii engine handles all HTTP serving, CORS, retry, and observability. No Axum, no standalone HTTP server.

```
┌──────────────────────────────────────────────────────────────┐
│                      iii-engine                                │
│                                                                │
│  HTTP TRIGGERS (RestApiModule :3111)                          │
│  ┌────────────────────────────────────────────────────────┐   │
│  │ POST /spec-forge/generate  →  api::post::spec-forge::generate │
│  │ POST /spec-forge/refine    →  api::post::spec-forge::refine   │
│  │ POST /spec-forge/validate  →  api::post::spec-forge::validate │
│  │ POST /spec-forge/prompt    →  api::post::spec-forge::prompt   │
│  │ GET  /spec-forge/stats     →  api::get::spec-forge::stats     │
│  │ GET  /spec-forge/health    →  api::get::spec-forge::health    │
│  └────────────────────────────────────────────────────────┘   │
│                                                                │
│  STATE (KV)                    STREAMS (Metrics)              │
│  ┌──────────────────┐         ┌──────────────────────────┐   │
│  │ spec:{hash}      │         │ spec-forge::metrics::     │   │
│  │ (cached specs)   │         │   cache: hits/misses      │   │
│  └──────────────────┘         │   generate: count/total_ms│   │
│                                └──────────────────────────┘   │
│                                                                │
│  OtelModule: traces, metrics, logs (memory exporter)          │
│  PubSubModule: local pub/sub                                  │
│  CronModule: KV-backed scheduled tasks                        │
└──────────────────────────────────────────────────────────────┘
         ↑ WebSocket (ws://127.0.0.1:49134)
┌──────────────────────────────────────────────────────────────┐
│                   spec-forge worker                            │
│                                                                │
│  SharedState                                                  │
│  ├── cache: SpecCache (DashMap + SHA-256 + TTL)               │
│  ├── semantic: SemanticCache (TF-IDF cosine similarity)       │
│  ├── limiter: RateLimiter (token bucket + semaphore)          │
│  ├── http: reqwest::Client                                    │
│  ├── api_key: String (ANTHROPIC_API_KEY)                      │
│  └── streams: iii Streams (atomic metric counters)            │
│                                                                │
│  Functions                                                    │
│  ├── generate: cache → semantic → rate limit → Claude → validate → store │
│  ├── refine:   diff-based patching (Add/Replace/Remove/SetRoot)         │
│  ├── validate: spec validation against catalog                          │
│  ├── prompt:   preview LLM prompt                                       │
│  ├── stats:    metrics from Streams + cache + limiter                   │
│  └── health:   liveness check                                           │
└──────────────────────────────────────────────────────────────┘
```

## Before vs After

### BEFORE (json-render alone)

```
Browser (JavaScript)
│
├── defineCatalog()          ← define Card, Button, Metric
├── build prompt             ← catalog → text prompt
├── fetch("claude API")      ← 1-5 sec, no cache, no retry, API key exposed
├── JSON.parse(response)     ← parse full response
├── Zod.validate(spec)       ← validate with Zod
├── <Renderer spec={spec} /> ← render with React/Vue/Svelte
│
└── Problems:
    - No caching — same prompt = same LLM call every time
    - No rate limiting — can burn through API quota
    - API key in browser — security risk
    - No observability — no tracing, no metrics
```

### AFTER (spec-forge + iii engine)

```
Browser                          iii-engine + spec-forge
│                                │
├── fetch("/spec-forge/generate")──>├── cache::check()       ← 0.1ms DashMap lookup
│                                   ├── semantic::check()    ← TF-IDF fuzzy match
│                                   ├── limiter::acquire()   ← token bucket
│                                   ├── prompt::build()      ← Rust string builder
│                                   ├── call_claude()        ← server-side (key safe)
│                                   ├── validate::check()    ← serde validation
│                                   ├── cache::store()       ← DashMap + semantic index
│   ← JSON response ──────────────├── streams::increment()  ← metrics
│                                   │
├── Progressive render in browser   │
├── <Render spec={spec} />         │
│                                │
└── Benefits:                    └── Benefits:
    - Instant cached responses       - SHA-256 + TF-IDF caching
    - No API key in browser          - Built-in OpenTelemetry
    - json-render compatible         - Rate limiting
    - Progressive rendering          - iii Streams metrics
```

## Performance

| Operation | json-render (JS) | spec-forge (Rust) | Speedup |
|-----------|-------------------|-------------------|---------|
| Schema validation | 2-5ms (Zod) | 0.05-0.2ms (serde) | 10-50x |
| JSON parsing | 1-3ms | 0.1-0.5ms (serde) | 5-10x |
| Prompt building | <1ms | <0.1ms | ~5x |
| Cache lookup | N/A (no cache) | 0.1ms (DashMap) | infinite |
| LLM call | 1-5 sec | 1-5 sec | 1x (same) |
| **Cached request** | **1-5 sec** | **~0.2ms** | **instant** |

## Request Flow (Generate)

```
1. HTTP request hits iii engine (port 3111)
2. Engine routes to spec-forge worker via WebSocket
3. Worker receives ApiRequest<GenerateRequest>
4. Check SHA-256 exact cache → hit? return immediately
5. Check TF-IDF semantic cache → hit? return immediately
6. Acquire rate limiter semaphore
7. Build LLM prompt from catalog + user request
8. Call Claude API (server-side, key never exposed)
9. Extract JSON from response
10. Validate spec against catalog (unknown types, missing refs, orphans)
11. Store in exact cache + semantic index
12. Increment Streams metrics (cache misses, generation count/ms)
13. Return ApiResponse with spec + metadata
```

## File Structure

```
spec-forge/
├── Cargo.toml          # Dependencies: iii-sdk 0.8, tokio, serde, reqwest, dashmap
├── iii-config.yaml     # iii engine config (REST, KV, OTel, PubSub, Cron)
├── src/
│   ├── main.rs         # Worker entry: SharedState, 6 functions, 6 HTTP triggers, core logic
│   ├── types.rs        # GenerateRequest, Catalog, ComponentDef, ActionDef, UISpec, UIElement
│   ├── cache.rs        # SHA-256 exact cache with TTL (DashMap)
│   ├── semantic.rs     # TF-IDF cosine similarity for fuzzy prompt matching
│   ├── diff.rs         # Spec patching: Add, Replace, Remove, SetRoot operations
│   ├── limiter.rs      # Token bucket + concurrency semaphore
│   ├── validate.rs     # Spec validation: unknown types, missing refs, orphans
│   ├── prompt.rs       # LLM prompt builder with design principles
│   ├── parser.rs       # Incremental JSON parser (available for future streaming)
│   └── bench.rs        # Benchmark binary
├── demo/
│   └── index.html      # Self-contained playground (served on port 3112)
├── client/
│   ├── src/index.ts               # JS/TS client SDK
│   └── src/json-render-adapter.ts # json-render <Render> adapter
├── client-example.tsx  # React integration example
└── data/               # Runtime KV store data (gitignored)
```
