# spec-forge

Pure iii-sdk worker for [json-render](https://github.com/anthropics/json-render) UI generation — caching, rate limiting, spec diffing, and validation. No standalone HTTP server; all endpoints are iii functions with HTTP triggers served by the iii engine.

```
Browser  ──>  iii-engine (HTTP triggers)  ──>  spec-forge worker (Rust)  ──>  Claude API
                      ↓
                JSON UI spec
                      ↓
Browser  ──>  json-render renderer (React/Vue/Svelte)  ──>  DOM
```

## Why

json-render calls the LLM on every request. No caching, no diffing, no rate limiting. API key exposed to client.

spec-forge is a pure iii-sdk worker that registers functions and HTTP triggers — the iii engine handles HTTP serving, CORS, retry, observability, and state management.

| | json-render | spec-forge + iii |
|---|---|---|
| Architecture | Client-side LLM calls | iii worker with HTTP triggers |
| Cache | None | SHA-256 exact + TF-IDF semantic |
| Repeat request | 3-5s LLM call | **0ms** cached |
| UI update | Full regeneration | Patch-based (Add/Replace/Remove) |
| Rate limiting | None | Token bucket + concurrency semaphore |
| API key | Client-side | Server-side only |
| Observability | None | OpenTelemetry (built-in via iii) |
| Streaming metrics | None | iii Streams (atomic KV counters) |

## Quick Start

**Prerequisites:** [iii engine](https://github.com/iii-hq/engine) installed and available as `iii` CLI.

```bash
# 1. Set API key
export ANTHROPIC_API_KEY=sk-ant-...

# 2. Start iii engine
iii --config iii-config.yaml &

# 3. Build and run the worker
cargo build --release
./target/release/spec-forge &

# 4. Serve demo UI
cd demo && python3 -m http.server 3112 &
```

Open `http://localhost:3112` for the interactive playground.

## Endpoints

All endpoints are iii functions with HTTP triggers, served by the engine on port 3111:

| Route | Method | Description |
|-------|--------|-------------|
| `/spec-forge/generate` | POST | Generate spec (exact cache -> semantic cache -> rate limit -> Claude -> validate) |
| `/spec-forge/refine` | POST | Patch-based update (Add/Replace/Remove ops) |
| `/spec-forge/validate` | POST | Validate spec against component catalog |
| `/spec-forge/prompt` | POST | Preview the LLM prompt that would be sent |
| `/spec-forge/stats` | GET | Rate limiter, cache, and stream metrics |
| `/spec-forge/health` | GET | Liveness check |

## Usage

### Generate

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

Response:
```json
{
  "spec": {
    "root": "form-card",
    "elements": {
      "form-card": {"type": "Card", "props": {"title": "Login"}, "children": ["form-stack"]},
      "form-stack": {"type": "Stack", "props": {"direction": "vertical", "gap": 16}, "children": ["email-input", "pass-input", "submit-btn"]},
      "email-input": {"type": "Input", "props": {"placeholder": "you@example.com", "type": "email", "label": "Email"}, "children": []},
      "pass-input": {"type": "Input", "props": {"placeholder": "Enter password", "type": "password", "label": "Password"}, "children": []},
      "submit-btn": {"type": "Button", "props": {"label": "Sign In", "variant": "primary"}, "children": []}
    }
  },
  "cached": false,
  "generation_ms": 2841,
  "model": "claude-sonnet-4-6"
}
```

Second request with same or similar prompt: `"cached": true, "generation_ms": 0`.

### Refine

Send a change request instead of regenerating from scratch:

```bash
curl -X POST http://localhost:3111/spec-forge/refine \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "Add a forgot password link",
    "current_spec": {"root": "form-card", "elements": {...}},
    "catalog": {...}
  }'
```

Returns patches:
```json
{
  "patches": [
    {"op": "add", "id": "link-1", "element": {"type": "Link", "props": {"text": "Forgot Password?"}}},
    {"op": "replace", "id": "form-stack", "element": {"type": "Stack", "children": ["email-input", "pass-input", "submit-btn", "link-1"]}}
  ],
  "patch_count": 2,
  "generation_ms": 1823,
  "spec": {...}
}
```

## Demo Playground

The `demo/index.html` playground connects to the iii engine at port 3111 and provides:

- **JSON tab** — syntax-highlighted spec output
- **STREAM tab** — real-time log of generation events (calling, root, element, done)
- **LIVE RENDER** — progressive rendering of UI components as they're generated
- **STATIC CODE** — copy-pasteable React code using `@anthropic-ai/json-render-react`
- **Catalog drawer** — 4 presets (dashboard, form, ecommerce, minimal) with editable JSON
- **Refine** — iteratively modify existing specs without full regeneration

### Component Presets

| Preset | Components |
|--------|-----------|
| Dashboard | Stack, Card, Grid, Heading, Metric, Table, Button, Text, Badge, Divider, Input |
| Form | Stack, Card, Heading, Input, Textarea, Select, Checkbox, Radio, Button, Text, Divider, Badge |
| Ecommerce | Stack, Grid, Card, Heading, Image, Text, Button, Metric, Badge, Divider, List |
| Minimal | Stack, Card, Heading, Text, Button |

## JS Client SDK

```bash
cd client && npm install && npm run build
```

```typescript
import { IIIRenderClient } from '@iii-dev/render-client';

const client = new IIIRenderClient({ baseUrl: 'http://localhost:3111' });

const { spec, cached } = await client.generate('A login form', catalog);

const { spec: updated, patches } = await client.refine('Add a header', currentSpec, catalog);
```

### With json-render React renderer

```tsx
import { Render } from '@anthropic-ai/json-render-react';
import { IIIRenderClient } from '@iii-dev/render-client';

const client = new IIIRenderClient();
const { spec } = await client.generate('A dashboard', catalog);

<Render spec={spec} catalog={catalog} />
```

## Architecture

spec-forge is a **pure iii-sdk worker** — no Axum, no standalone HTTP server. All endpoints are registered as iii functions with HTTP triggers.

```
iii-engine (iii-config.yaml)
├── RestApiModule (port 3111, CORS)
├── StateModule (file-based KV)
├── OtelModule (traces, metrics, logs)
├── PubSubModule (local)
└── CronModule

spec-forge worker (connects via WebSocket)
├── register_functions()     6 iii functions
├── register_http_triggers() 6 HTTP trigger bindings
└── business logic
    ├── generate_core()  cache -> semantic -> rate limit -> Claude -> validate -> store
    ├── refine_core()    diff-based patching (Add/Replace/Remove/SetRoot)
    ├── validate_core()  spec validation against catalog
    ├── prompt_core()    preview LLM prompt
    ├── stats_core()     metrics from iii Streams
    └── health_core()    liveness check
```

### Source Files

```
src/
├── main.rs        # iii worker: SharedState, function registration, HTTP triggers, core logic
├── types.rs       # Core types: GenerateRequest, Catalog, UISpec, UIElement
├── cache.rs       # SHA-256 exact cache (DashMap with TTL)
├── semantic.rs    # TF-IDF cosine similarity cache for fuzzy prompt matching
├── diff.rs        # Spec patching engine (Add/Replace/Remove/SetRoot ops)
├── limiter.rs     # Rate limiter (token bucket + concurrency semaphore)
├── validate.rs    # Spec validation against component catalog
├── prompt.rs      # LLM prompt builder with design principles
├── parser.rs      # Incremental JSON streaming parser (unused in pure-worker mode)
└── bench.rs       # Benchmark binary
demo/
└── index.html     # Self-contained playground (served separately)
client/
├── src/index.ts               # JS/TS client SDK
└── src/json-render-adapter.ts # Adapter for json-render <Render>
```

### iii-sdk Primitives Used

| Primitive | Usage |
|-----------|-------|
| `III::init()` | Initialize worker, connect to engine via WebSocket |
| `register_function_with_description()` | Register 6 named functions with descriptions |
| `register_trigger("http", ...)` | Bind functions to HTTP routes (`api_path` + `http_method`) |
| `ApiRequest<T>` / `ApiResponse<T>` | Typed HTTP request/response wrappers |
| `Streams` | Atomic metric counters (cache hits/misses, generation count/ms) |
| `get_context().logger` | Structured logging (info, warn, error) |
| OtelModule | Automatic OpenTelemetry traces, metrics, and logs |

## Tests

```bash
cargo test
```

33 tests: cache (4), semantic (7), diff (7), limiter (5), validate (6), prompt (4).

## Configuration

### Environment

| Env Var | Default | Description |
|---------|---------|-------------|
| `ANTHROPIC_API_KEY` | required | Claude API key |
| `DOTENV_PATH` | `~/agentsos/.env` | Path to .env file |

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
