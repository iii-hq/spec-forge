# spec-forge

Rust generation server for [json-render](https://github.com/vercel-labs/json-render) — adds caching, streaming, rate limiting, and spec diffing.

```
Browser → spec-forge (Rust) → Claude API
              ↓
        JSON UI spec
              ↓
Browser → json-render renderer (React/Vue/Svelte) → DOM
```

## Why

json-render calls the LLM on every request. No caching, no diffing, no rate limiting. API key exposed to client.

spec-forge sits between the browser and Claude API as a production layer:

| | json-render | spec-forge |
|---|---|---|
| Cache | None | SHA-256 exact + TF-IDF semantic |
| Repeat request | 3-5s LLM call | **0ms** |
| UI update | Full regeneration | Patch-based (Add/Replace/Remove) |
| Streaming | Basic | Incremental parser, live JSON + element SSE |
| Rate limiting | None | Token bucket + concurrency semaphore |
| API key | Client-side | Server-side only |

## Quick Start

```bash
export ANTHROPIC_API_KEY=sk-ant-...
cargo run --release
```

Open `http://localhost:3112` for the interactive demo.

## Endpoints

| Route | Description |
|-------|-------------|
| `GET /` | Interactive demo UI with live JSON streaming |
| `POST /generate` | Generate spec (exact cache → semantic cache → rate limit → Claude → validate) |
| `POST /stream` | SSE streaming with live JSON text + element-by-element emission |
| `POST /refine` | Patch-based update — sends only changes, not full regeneration |
| `POST /validate` | Validate a spec against a component catalog |
| `GET /stats` | Rate limiter + cache statistics |
| `POST /prompt` | Preview the LLM prompt that would be sent |
| `GET /health` | Liveness check |

## Usage

### Generate

```bash
curl -X POST http://localhost:3112/generate \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "A login form with email and password",
    "catalog": {
      "components": {
        "Card": {"description": "Container card", "children": true},
        "Input": {"description": "Text input", "props": {"placeholder": "string", "type": "string"}},
        "Button": {"description": "Button", "props": {"label": "string", "variant": "string"}}
      },
      "actions": {"submit": {"description": "Submit form"}}
    }
  }'
```

Response:
```json
{
  "spec": {
    "root": "card-1",
    "elements": {
      "card-1": {"type": "Card", "props": {}, "children": ["input-1", "input-2", "button-1"]},
      "input-1": {"type": "Input", "props": {"placeholder": "Email", "type": "email"}, "children": []},
      "input-2": {"type": "Input", "props": {"placeholder": "Password", "type": "password"}, "children": []},
      "button-1": {"type": "Button", "props": {"label": "Login", "variant": "primary"}, "children": []}
    }
  },
  "cached": false,
  "generation_ms": 3671,
  "model": "claude-opus-4-6"
}
```

Second request with same or similar prompt: `"cached": true, "generation_ms": 0`.

### Stream

```bash
curl -N -X POST http://localhost:3112/stream \
  -H "Content-Type: application/json" \
  -d '{"prompt": "A dashboard with metrics", "catalog": {...}}'
```

SSE events:
```
event: text
data: {"text":"{\n  \"root\""}     ← raw JSON appearing character by character

event: root
data: {"root":"card-1"}             ← root ID extracted

event: element
data: {"id":"card-1","element":{…}} ← complete element ready to render

event: done
data: {"done":true,"spec":{…}}      ← final validated spec
```

### Refine

Instead of regenerating the full spec, send a change request:

```bash
curl -X POST http://localhost:3112/refine \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "Add a forgot password link",
    "current_spec": {"root": "card-1", "elements": {…}},
    "catalog": {…}
  }'
```

Returns patches:
```json
{
  "patches": [
    {"op": "add", "id": "link-1", "element": {"type": "Link", "props": {"label": "Forgot Password?"}}},
    {"op": "replace", "id": "card-1", "element": {"type": "Card", "children": ["input-1", "input-2", "button-1", "link-1"]}}
  ],
  "patch_count": 2,
  "generation_ms": 2635,
  "spec": {…}
}
```

## JS Client SDK

```bash
cd client && npm install && npm run build
```

```typescript
import { IIIRenderClient } from '@iii-dev/render-client';

const client = new IIIRenderClient({ baseUrl: 'http://localhost:3112' });

// One-shot generation
const { spec, cached } = await client.generate('A login form', catalog);

// Streaming with progressive rendering
for await (const event of client.stream('A dashboard', catalog)) {
  if (event.type === 'element') renderElement(event.id, event.element);
  if (event.type === 'done') finalRender(event.spec);
}

// Incremental refinement
const { spec: updated, patches } = await client.refine('Add a header', currentSpec, catalog);
```

### With json-render's React renderer

```tsx
import { Render } from '@anthropic-ai/json-render-react';
import { IIIRenderClient } from '@iii-dev/render-client';

const client = new IIIRenderClient();
const { spec } = await client.generate('A dashboard', catalog);

<Render spec={spec} catalog={catalog} />
```

## Architecture

```
src/
├── main.rs        # Axum server, 8 routes, embedded demo UI
├── cache.rs       # SHA-256 exact cache with TTL (DashMap)
├── semantic.rs    # TF-IDF cosine similarity cache
├── diff.rs        # Spec patching (Add/Replace/Remove/SetRoot)
├── limiter.rs     # Rate limiter (req/min + concurrency semaphore)
├── parser.rs      # Incremental JSON streaming parser
├── validate.rs    # Spec validation against catalog
├── prompt.rs      # LLM prompt builder
├── types.rs       # Core types (UISpec, Catalog, UIElement)
└── bench.rs       # Benchmark binary
client/
├── src/index.ts               # JS/TS client SDK + DOM renderer
└── src/json-render-adapter.ts # Adapter for json-render's <Render>
demo/
└── index.html     # Self-contained demo (embedded in binary)
```

## Benchmarks

| Operation | json-render (TS) | spec-forge (Rust) |
|-----------|-----------------|-------------------|
| Parse 500 elements | 1.2ms | 0.45ms |
| Validate 500 elements | 0.8ms | 0.5ms |
| Stringify 500 elements | 0.9ms | 0.6ms |

Raw parsing is 1.5-2.7x faster, but the real value is architectural — cache hits are 0ms vs 3-5s LLM calls.

```bash
# Run Rust benchmarks
cargo run --release --bin bench

# Run TypeScript benchmarks (for comparison)
cd bench && node bench-ts.mjs
```

## Tests

```bash
cargo test
```

38 tests across all modules: cache (4), semantic (7), diff (7), limiter (5), parser (6), validate (6), prompt (3).

## Configuration

| Env Var | Default | Description |
|---------|---------|-------------|
| `ANTHROPIC_API_KEY` | required | Claude API key |
| `DOTENV_PATH` | `~/agentsos/.env` | Path to .env file |

Server defaults: port 3112, cache TTL 300s, semantic threshold 0.85, rate limit 60 req/min + 5 concurrent.

## License

MIT
