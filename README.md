# spec-forge

Pure iii-sdk worker for [json-render](https://github.com/vercel-labs/json-render) UI and 3D scene generation — JSONL patch streaming, caching, rate limiting, and validation. Generates both 2D UI components and Three.js 3D scenes from natural language. No standalone HTTP server; all endpoints are iii functions with HTTP triggers served by the iii engine.

![spec-forge demo](demo/demo.gif)

```
Browser  ──POST──>  iii-engine (:3111)  ──>  spec-forge worker (Rust)  ──>  Claude API
                         │                           │
                         │                    JSONL patches (RFC 6902)
                         │                           │
Browser  <──WebSocket──  iii Channel (:49134)  <─────┘
     │
     └──>  Progressive render (each patch = 1 component)
```

## Why

json-render calls the LLM on every request. No caching, no streaming, no rate limiting. API key exposed to client.

spec-forge is a pure iii-sdk worker that streams JSONL patches (RFC 6902) through iii Channels — each patch arrives at the browser via WebSocket the moment Claude generates it, so the UI fills in progressively.

| | json-render | spec-forge + iii |
|---|---|---|
| Architecture | Client-side LLM calls | iii worker with HTTP triggers |
| Output format | Full JSON response | JSONL patches (RFC 6902) |
| Streaming | Vercel AI SDK `streamText` | iii Channels (WebSocket) |
| First paint | After full LLM response | After first patch (~200ms) |
| 3D scenes | Not supported | 43 Three.js components, live preview |
| Cache | None | SHA-256 exact + TF-IDF semantic |
| Repeat request | 3-5s LLM call | **0ms** cached |
| Rate limiting | None | Token bucket + concurrency semaphore |
| API key | Client-side | Server-side only |
| Observability | None | OpenTelemetry (built-in via iii) |

## Benchmarks

All numbers measured on Apple M-series. Reproduce: `./bench/run.sh`

### Compute: JavaScript (V8 apples-to-apples)

Both frameworks running the same operations in Node.js — no language advantage, pure algorithm comparison:

| Operation | json-render | spec-forge | Winner | Speedup |
|-----------|-------------|------------|--------|---------|
| JSONL 3 patches (bulk) | 4.83 µs | 3.13 µs | spec-forge | **1.5x** |
| JSONL 9 patches (chunked) | 16.77 µs | 10.11 µs | spec-forge | **1.7x** |
| Prompt build (minimal) | 3.02 µs | 1.42 µs | spec-forge | **2.1x** |
| Prompt build (dashboard) | 6.10 µs | 3.48 µs | spec-forge | **1.8x** |
| Pipeline 9 elements | 15.90 µs | 11.94 µs | spec-forge | **1.3x** |
| Validate 50 elements | 9.77 µs | 9.82 µs | tie | 1.0x |
| Parse 500 elements | 328 µs | 326 µs | tie | 1.0x |

spec-forge's JSONL parser is leaner — no dedup `Set`, no object spread on every batch. json-render's `createSpecStreamCompiler` buffers text, deduplicates lines, and copies the result object on every patch.

### Compute: Rust (spec-forge native worker)

The Rust worker is where the real gap opens. These are actual production code paths with `std::hint::black_box`:

| Operation | Rust | JavaScript | Speedup |
|-----------|------|------------|---------|
| Stringify 3 elements | 0.56 µs | 1.12 µs | **2.0x** |
| Stringify 500 elements | 63.6 µs | 141.5 µs | **2.2x** |
| Stringify 2000 elements | 253 µs | 618 µs | **2.4x** |
| Validate 500 elements | 74.0 µs | 112.2 µs | **1.5x** |
| Validate 2000 elements | 312 µs | 586 µs | **1.9x** |
| Parse 2000 elements | 1,062 µs | 1,497 µs | **1.4x** |

### Caching: the real game changer

json-render has **zero caching**. Every request is a fresh LLM call (3-5 seconds).

| Operation | spec-forge | json-render |
|-----------|------------|-------------|
| SHA-256 exact cache hit | **0.04 µs** | N/A (no cache) |
| TF-IDF semantic hit (10 entries) | 13.7 µs | N/A |
| TF-IDF semantic hit (100 entries) | 905 µs | N/A |
| Rate limiter acquire + release | 0.07 µs | N/A |

On repeat requests, spec-forge returns in **< 1 µs** (SHA-256 cache hit). json-render has no cache and re-invokes Claude every time: **3,000,000 - 5,000,000 µs** (3-5 seconds).

In practice this means repeat-request latency drops from **3-5 seconds** to **sub-microsecond** — the user gets an instant response instead of waiting for a full LLM round-trip.

The TF-IDF semantic cache catches _similar_ prompts too — "A revenue dashboard" matches "A sales dashboard with revenue metrics" at the 0.85 cosine threshold without burning an LLM call.

### Cold pipeline overhead

When spec-forge _does_ call the LLM (cache miss), the total overhead from caching + validation + storage:

| Stage | Time |
|-------|------|
| SHA-256 cache key | 2.65 µs |
| Exact cache lookup (miss) | 0.01 µs |
| TF-IDF semantic check (100 entries) | 905 µs |
| Rate limiter acquire | 0.07 µs |
| JSONL patch parsing (9 patches) | 8.27 µs |
| Spec validation (9 elements) | 0.90 µs |
| Cache store | ~1 µs |
| **Total overhead** | **< 1 ms** |

Negligible against the 2-5 second LLM call. You get caching, rate limiting, validation, and observability essentially for free.

### Run the benchmarks

```bash
./bench/run.sh            # Everything (Rust + JS comparison + e2e)
./bench/run.sh rust       # Rust native only (cargo build --release)
./bench/run.sh compare    # Side-by-side JS comparison table
./bench/run.sh e2e        # End-to-end through iii engine (server must be running)
```

See [`bench/README.md`](bench/README.md) for full methodology and all test categories.

## Prerequisites

### Install iii engine

spec-forge runs on the [iii engine](https://github.com/iii-hq/iii). Install it first:

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

This installs the `iii` CLI to `~/.local/bin/iii`. Make sure `~/.local/bin` is in your `PATH`.

Verify the installation:

```bash
iii --version
```

### Install Rust

spec-forge is a Rust worker. Install Rust if you don't have it:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Quick Start

```bash
# 1. Clone and enter
git clone https://github.com/iii-hq/spec-forge.git
cd spec-forge

# 2. Set your Anthropic API key (in .env or environment)
echo "ANTHROPIC_API_KEY=sk-ant-..." > .env

# 3. Start iii engine
iii --config iii-config.yaml &

# 4. Build and run the worker
cargo build --release
./target/release/spec-forge &

# 5. Serve demo UI
cd demo && python3 -m http.server 3112 &
```

Open `http://localhost:3112` for the interactive playground.

## Endpoints

All endpoints are iii functions with HTTP triggers, served by the engine on port 3111:

| Route | Method | Description |
|-------|--------|-------------|
| `/spec-forge/generate` | POST | Generate spec (cache -> semantic -> Claude -> validate) |
| `/spec-forge/stream` | POST | Stream patches via iii Channel (WebSocket) — real-time progressive rendering |
| `/spec-forge/refine` | POST | Patch existing spec with incremental changes |
| `/spec-forge/validate` | POST | Validate spec against component catalog |
| `/spec-forge/prompt` | POST | Preview the LLM prompt that would be sent |
| `/spec-forge/stats` | GET | Rate limiter, cache, and stream metrics |
| `/spec-forge/health` | GET | Liveness check |

## JSONL Patch Protocol (RFC 6902)

Claude outputs one JSON patch operation per line. Each line is independently parseable, enabling real-time streaming:

```jsonl
{"op":"add","path":"/root","value":"main"}
{"op":"add","path":"/elements/main","value":{"type":"Card","props":{"title":"Dashboard"},"children":["metric-1","chart"]}}
{"op":"add","path":"/elements/metric-1","value":{"type":"Metric","props":{"label":"Revenue","value":"$42K"},"children":[]}}
{"op":"add","path":"/elements/chart","value":{"type":"Chart","props":{"title":"Sales"},"children":[]}}
```

Operations: `add`, `replace`, `remove` on paths `/root` and `/elements/{key}`.

## Streaming via iii Channels

The `/stream` endpoint creates an iii Channel (WebSocket-backed pipe) and returns reader credentials:

```bash
curl -X POST http://localhost:3111/spec-forge/stream \
  -H "Content-Type: application/json" \
  -d '{"prompt": "A sales dashboard", "catalog": {...}}'
```

Response:
```json
{
  "cached": false,
  "channel": {
    "channel_id": "ch_abc123",
    "access_key": "key_xyz"
  }
}
```

Connect to the WebSocket to receive patches in real-time:

```
ws://localhost:49134/ws/channels/{channel_id}?key={access_key}&dir=read
```

Messages arrive as:
```json
{"type": "patch", "patch": {"op": "add", "path": "/elements/metric-1", "value": {...}}}
{"type": "done", "spec": {...}, "valid": true, "generation_ms": 1823}
```

Each `patch` message triggers a progressive UI render. The browser never waits for the full LLM response.

## Usage

### Generate (non-streaming)

```bash
curl -X POST http://localhost:3111/spec-forge/generate \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "A login form with email and password",
    "catalog": {
      "components": {
        "Stack": {"description": "Flex container", "props": {"direction": "vertical|horizontal", "gap": "number"}, "children": true},
        "Card": {"description": "Container card", "props": {"title": "string"}, "children": true},
        "Input": {"description": "Text input", "props": {"placeholder": "string", "type": "string", "label": "string"}},
        "Button": {"description": "Button", "props": {"label": "string", "variant": "primary|secondary"}}
      },
      "actions": {"submit": {"description": "Submit form"}}
    }
  }'
```

Response includes both the final spec and the JSONL patches used to build it:
```json
{
  "spec": {
    "root": "form-card",
    "elements": {
      "form-card": {"type": "Card", "props": {"title": "Login"}, "children": ["form-stack"]},
      "email-input": {"type": "Input", "props": {"placeholder": "you@example.com", "type": "email", "label": "Email"}, "children": []}
    }
  },
  "patches": [
    {"op": "add", "path": "/root", "value": "form-card"},
    {"op": "add", "path": "/elements/form-card", "value": {"type": "Card", "props": {"title": "Login"}, "children": ["form-stack"]}}
  ],
  "cached": false,
  "generation_ms": 2841,
  "model": "claude-sonnet-4-6"
}
```

Second request with same or similar prompt: `"cached": true, "generation_ms": 0`.

### Refine

Send a change request with JSONL patches instead of regenerating from scratch:

```bash
curl -X POST http://localhost:3111/spec-forge/refine \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "Add a forgot password link",
    "current_spec": {"root": "form-card", "elements": {...}},
    "catalog": {...}
  }'
```

## 3D Scene Generation

spec-forge supports generating Three.js 3D scenes using the same JSONL patch protocol. Use the `3d` or `3d-product` catalog presets:

```bash
curl -X POST http://localhost:3111/spec-forge/stream \
  -H "Content-Type: application/json" \
  -d '{"prompt": "A product showroom with a metallic sphere on a reflective floor", "catalog_preset": "3d"}'
```

The 3D catalog includes 43 components:

| Category | Components |
|----------|-----------|
| Geometry | Box, Sphere, Cylinder, Cone, Torus, Plane, Capsule, TorusKnot, RoundedBox |
| Lights | AmbientLight, DirectionalLight, PointLight, SpotLight |
| Special Materials | GlassSphere, GlassBox, DistortSphere |
| Environment | Environment, Fog, GridHelper |
| Particles | Sparkles, Stars, Sky, Cloud |
| Shadows & Reflection | ContactShadows, Float, ReflectorPlane, Backdrop |
| Animation | WarpTunnel, Spin, Orbit, Pulse, CameraShake |
| Portals | MeshPortalMaterial, HtmlLabel |
| Post-Processing | EffectComposer, Bloom, Glitch, Vignette |
| Camera & Controls | PerspectiveCamera, OrbitControls |
| Structure | Group, Model, Text3D |

The demo playground renders 3D specs live using Three.js with PBR materials, environment mapping, and bloom post-processing.

## Demo Playground

The `demo/index.html` playground connects to the iii engine and provides:

- **JSON tab** — syntax-highlighted spec output with JSONL patches
- **STREAM tab** — real-time log of WebSocket patch messages
- **LIVE RENDER** — progressive rendering as patches arrive via WebSocket
- **STATIC CODE** — copy-pasteable React code using `@anthropic-ai/json-render-react`
- **Catalog drawer** — 6 presets (dashboard, form, ecommerce, minimal, 3d, 3d-product) with editable JSON
- **3D Live Preview** — Three.js renderer with PBR materials, environment maps, post-processing (bloom), and auto-rotating camera
- **Refine** — iteratively modify existing specs without full regeneration

### Component Presets

| Preset | Components |
|--------|-----------|
| Dashboard | Stack, Card, Grid, Heading, Metric, Table, Chart, Button, Text, Badge, Divider, Input |
| Form | Stack, Card, Heading, Input, Textarea, Select, Checkbox, Radio, Button, Text, Divider, Badge |
| Ecommerce | Stack, Grid, Card, Heading, Image, Text, Button, Metric, Badge, Divider, List |
| Minimal | Stack, Card, Heading, Text, Button, Input |
| 3D | 43 Three.js components across 11 categories (geometry, lights, cameras, controls, effects, environment, animation, helpers, advanced, portals, post-processing) |
| 3D Product | Subset optimized for product visualization (sphere, floor, studio lighting, bloom) |

## Architecture

spec-forge is a **pure iii-sdk worker** — no Axum, no standalone HTTP server. All endpoints are registered as iii functions with HTTP triggers.

```
iii-engine (iii-config.yaml)
├── RestApiModule (port 3111, CORS)
├── StateModule (file-based KV)
├── OtelModule (traces, metrics, logs)
├── PubSubModule (local)
├── CronModule
└── Channels (WebSocket port 49134)

spec-forge worker (connects via WebSocket)
├── register_functions()     7 iii functions
├── register_http_triggers() 7 HTTP trigger bindings
└── business logic
    ├── generate_core()  cache → semantic → rate limit → Claude → validate → store
    ├── stream_core()    iii Channel → Claude streaming → JSONL patches → WebSocket
    ├── refine_core()    JSONL patch-based refinement (Add/Replace/Remove)
    ├── validate_core()  spec validation against catalog
    ├── prompt_core()    preview LLM prompt
    ├── stats_core()     metrics from iii Streams
    └── health_core()    liveness check
```

### Source Files

```
src/
├── main.rs        # iii worker: SharedState, 7 functions, 7 HTTP triggers, core logic
├── types.rs       # GenerateRequest, Catalog, ComponentDef, ActionDef, UISpec, UIElement
├── cache.rs       # SHA-256 exact cache with TTL (DashMap)
├── semantic.rs    # TF-IDF cosine similarity cache for fuzzy prompt matching
├── limiter.rs     # Rate limiter (token bucket + concurrency semaphore)
├── validate.rs    # Spec validation against component catalog (UI + 3D)
├── prompt.rs      # LLM prompt builder (UI + 3D scene modes)
├── catalogs.rs    # 6 catalog presets (4 UI + 2 3D with 43 Three.js components)
└── bench.rs       # Benchmark binary
demo/
├── index.html     # Self-contained playground with WebSocket streaming
└── demo.gif       # Demo recording
client/
├── src/index.ts               # JS/TS client SDK
└── src/json-render-adapter.ts # Adapter for json-render <Render>
```

### iii-sdk Primitives Used

| Primitive | Usage |
|-----------|-------|
| `III::init()` | Initialize worker, connect to engine via WebSocket |
| `register_function_with_description()` | Register 7 named functions with descriptions |
| `register_trigger("http", ...)` | Bind functions to HTTP routes (`api_path` + `http_method`) |
| `ApiRequest<T>` / `ApiResponse<T>` | Typed HTTP request/response wrappers |
| `create_channel()` + `ChannelWriter` | WebSocket-backed streaming pipe for real-time patches |
| `Streams` | Atomic metric counters (cache hits/misses, generation count/ms) |
| `get_context().logger` | Structured logging (info, warn, error) |
| OtelModule | Automatic OpenTelemetry traces, metrics, and logs |

## Tests

```bash
cargo test
```

39 tests: cache (4), semantic (7), limiter (5), validate (12), prompt (11).

## Configuration

### Environment

| Env Var | Default | Description |
|---------|---------|-------------|
| `ANTHROPIC_API_KEY` | required | Claude API key |
| `DOTENV_PATH` | `.env` | Path to .env file |

### iii Engine (iii-config.yaml)

| Module | Config |
|--------|--------|
| RestApiModule | Port 3111, CORS for localhost:3112/3111/3000/5173 |
| StateModule | File-based KV at `./data/state_store.db` |
| OtelModule | Memory exporter, all signals enabled |
| PubSubModule | Local adapter |
| CronModule | KV-backed cron |

### Worker Defaults

Rate limit: 60 req/min + 5 concurrent. Cache TTL: 300s. Semantic threshold: 0.85. Default model: `claude-sonnet-4-6`.

## License

MIT
