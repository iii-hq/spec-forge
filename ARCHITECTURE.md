# spec-forge Architecture

## Pure iii-sdk Worker

spec-forge is a **pure iii-sdk worker** — it registers functions and HTTP triggers, then the iii engine handles all HTTP serving, CORS, retry, and observability. No Axum, no standalone HTTP server.

```
┌───────────────────────────────────────────────────────────────┐
│                      iii-engine                               │
│                                                               │
│  HTTP TRIGGERS (RestApiModule :3111)                          │
│  ┌────────────────────────────────────────────────────────┐   │
│  │ POST /spec-forge/generate  →  generate (cache+Claude)      │
│  │ POST /spec-forge/stream    →  stream (iii Channel+Claude)  │
│  │ POST /spec-forge/refine    →  refine (JSONL patches)       │
│  │ POST /spec-forge/validate  →  validate (catalog check)     │
│  │ POST /spec-forge/prompt    →  prompt (preview LLM prompt)  │
│  │ GET  /spec-forge/stats     →  stats (metrics)              │
│  │ GET  /spec-forge/health    →  health (liveness)            │
│  └────────────────────────────────────────────────────────┘   │
│                                                               │
│  STATE (KV)                    STREAMS (Metrics)              │
│  ┌──────────────────┐         ┌───────────────────────────┐   │
│  │ spec:{hash}      │         │ spec-forge::metrics::     │   │
│  │ (cached specs)   │         │   cache: hits/misses      │   │
│  └──────────────────┘         │   generate: count/total_ms│   │
│                                └──────────────────────────┘   │
│                                                               │
│  CHANNELS (WebSocket :49134)                                  │
│  ┌──────────────────────────────────────────────────────┐     │
│  │ ws://engine:49134/ws/channels/{id}?key={key}&dir=read│     │
│  │ Worker writes JSONL patches → Browser reads real-time│     │
│  └──────────────────────────────────────────────────────┘     │
│                                                               │
│  OtelModule: traces, metrics, logs (memory exporter)          │
│  PubSubModule: local pub/sub                                  │
│  CronModule: KV-backed scheduled tasks                        │
└───────────────────────────────────────────────────────────────┘
         ↑ WebSocket (ws://127.0.0.1:49134)
┌──────────────────────────────────────────────────────────────────────────┐
│                   spec-forge worker                                      │
│                                                                          │
│  SharedState                                                             │
│  ├── iii: III (for channel creation)                                     │
│  ├── cache: SpecCache (DashMap + SHA-256 + TTL)                          │
│  ├── semantic: SemanticCache (TF-IDF cosine similarity)                  │
│  ├── limiter: RateLimiter (token bucket + semaphore)                     │
│  ├── http: reqwest::Client                                               │
│  ├── api_key: String (ANTHROPIC_API_KEY)                                 │
│  └── streams: iii Streams (atomic metric counters)                       │
│                                                                          │
│  Functions                                                               │
│  ├── generate: cache → semantic → rate limit → Claude → validate → store │
│  ├── stream:    create Channel → Claude streaming → JSONL patches → WS   │
│  ├── refine:    JSONL patch-based refinement (Add/Replace/Remove)        │
│  ├── validate:  spec validation against catalog                          │
│  ├── prompt:    preview LLM prompt                                       │
│  ├── stats:     metrics from Streams + cache + limiter                   │
│  └── health:    liveness check                                           │
└──────────────────────────────────────────────────────────────────────────┘
```

## JSONL Patch Protocol

Claude outputs RFC 6902 JSON Patch operations, one per line:

```
{"op":"add","path":"/root","value":"main"}                         ← set root key
{"op":"add","path":"/elements/main","value":{...}}                 ← add element
{"op":"replace","path":"/elements/main","value":{...}}             ← update element
{"op":"remove","path":"/elements/old-item"}                        ← remove element
```

Each line is independently parseable. As Claude streams tokens, the worker accumulates text until a newline, parses the JSONL line, applies the patch to the in-memory spec, and forwards it to the browser via iii Channel.

```
Claude API (SSE)                    spec-forge worker               iii Channel (WS)           Browser
     │                                    │                              │                        │
     ├─ content_block_delta ──────>  accumulate tokens                   │                        │
     ├─ content_block_delta ──────>  token_buf += text                   │                        │
     ├─ content_block_delta ──────>  newline found!                      │                        │
     │                               parse JSONL line                    │                        │
     │                               apply_patch(spec)                   │                        │
     │                               ├─ send {"type":"patch",...} ──────>├─ forward to WS ──────> render component
     │                                    │                              │                        │
     ├─ content_block_delta ──────>  next patch...                       │                        │
     │                               ├─ send {"type":"patch",...} ──────>├─ forward ────────────> render next
     │                                    │                              │                        │
     ├─ message_stop ────────────>  send {"type":"done",...} ──────────> ├─ forward ────────────> show final spec
```

## Before vs After

### BEFORE (json-render alone)

```
Browser (JavaScript)
│
├── defineCatalog()          ← define Card, Button, Metric
├── build prompt             ← catalog → text prompt
├── fetch("claude API")      ← 1-5 sec, no cache, no retry, API key exposed
├── JSON.parse(response)     ← parse full response (wait for everything)
├── Zod.validate(spec)       ← validate with Zod
├── <Renderer spec={spec} /> ← render with React/Vue/Svelte
│
└── Problems:
    - Must wait for full LLM response before rendering
    - No caching — same prompt = same LLM call every time
    - No rate limiting — can burn through API quota
    - API key in browser — security risk
```

### AFTER (spec-forge + iii engine)

```
Browser                          iii-engine + spec-forge
│                                │
├── POST /spec-forge/stream ────>├── cache::check()       ← 0.1ms DashMap lookup
│                                ├── create_channel()     ← iii Channel (WS pipe)
│   ← { channel_id, access_key }│
│                                ├── spawn background task
│                                │   ├── call_claude_streaming()
├── ws://engine:49134/channels/──│   │   ├── SSE stream from Claude
│   {id}?key={key}&dir=read      │   │   ├── accumulate tokens → JSONL lines
│                                │   │   ├── apply_patch() → write to Channel
│   ← {"type":"patch",...} ──────│   │   ├── (browser renders each component)
│   ← {"type":"patch",...} ──────│   │   ├── ...
│   ← {"type":"done",...}  ──────│   │   └── validate + cache + close
│                                │   └──
│                                │
├── Progressive render in browser│
│   (each patch = one component  │
│    rendered immediately)       │
│                                │
└── Benefits:                    └── Benefits:
    - First paint at ~200ms          - SHA-256 + TF-IDF caching
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
| LLM call | 1-5 sec (full wait) | 1-5 sec total, ~200ms first paint | 10x perceived |
| **Cached request** | **1-5 sec** | **~0.2ms** | **instant** |

## Request Flow (Stream)

```
1. POST /spec-forge/stream hits iii engine (port 3111)
2. Engine routes to spec-forge worker via WebSocket
3. Worker receives ApiRequest<GenerateRequest>
4. Check SHA-256 exact cache → hit? return spec immediately (no channel needed)
5. Acquire rate limiter semaphore
6. Create iii Channel (buffer=64)
7. Return channel_id + access_key to browser
8. Spawn background task:
   a. Build JSONL/RFC 6902 prompt from catalog + user request
   b. Call Claude streaming API (SSE)
   c. Accumulate tokens → detect newlines → parse JSONL lines
   d. For each patch: apply_patch(spec) + write to ChannelWriter
   e. On completion: validate spec, cache if valid, send "done" message
   f. Close channel
9. Browser connects to ws://engine:49134/ws/channels/{id}
10. Browser receives patches in real-time, renders each component progressively
```

## File Structure

```
spec-forge/
├── Cargo.toml          # Dependencies: iii-sdk 0.8, tokio, serde, reqwest, dashmap, futures-util
├── iii-config.yaml     # iii engine config (REST, KV, OTel, PubSub, Cron)
├── src/
│   ├── main.rs         # Worker entry: SharedState, 7 functions, 7 HTTP triggers, core logic
│   ├── types.rs        # GenerateRequest, Catalog, ComponentDef, ActionDef, UISpec, UIElement
│   ├── cache.rs        # SHA-256 exact cache with TTL (DashMap)
│   ├── semantic.rs     # TF-IDF cosine similarity for fuzzy prompt matching
│   ├── limiter.rs      # Token bucket + concurrency semaphore
│   ├── validate.rs     # Spec validation: unknown types, missing refs, orphans
│   ├── prompt.rs       # LLM prompt builder (JSONL/RFC 6902 instructions)
│   └── bench.rs        # Benchmark binary
├── demo/
│   └── index.html      # Self-contained playground with WebSocket streaming
├── client/
│   ├── src/index.ts               # JS/TS client SDK
│   └── src/json-render-adapter.ts # json-render <Render> adapter
├── client-example.tsx  # React integration example
└── data/               # Runtime KV store data (gitignored)
```
