# spec-forge

iii worker that generates UI from natural language. Define a component catalog, call `spec-forge::generate`, get a rendered UI spec back. Add a `session_id` for real-time collaboration across browsers.

Built on [json-render](https://github.com/vercel-labs/json-render) for rendering — all their renderers (React, React Native, PDF, email, video, 3D, terminal) work with specs that spec-forge generates.

![spec-forge demo](demo/demo.gif)

## How It Works

spec-forge is a standard iii worker. It registers functions and triggers. You call them.

```
Browser                              iii engine                    spec-forge worker
   │                                    │                               │
   ├─ iii.trigger('spec-forge::        │                               │
   │   generate', { prompt, catalog }) ─┤──> routes to worker ────────>│──> Claude API
   │                                    │                               │
   │                                    │<── patches stream back ──────│
   │<── ui::render-patch ──────────────│    (fan-out to all peers)     │
   │    each patch = 1 component        │                               │
```

### From the Browser

```typescript
import { registerWorker } from 'iii-browser-sdk'

const iii = registerWorker('ws://localhost:49135')

// Register a function to receive streaming patches
iii.registerFunction({ id: 'ui::render-patch::my-tab' }, async (data) => {
  renderComponent(data)
  return { applied: true }
})

// Generate UI from a prompt
const result = await iii.trigger({
  function_id: 'api::post::spec-forge::generate',
  payload: { body: { prompt: 'A sales dashboard with revenue metrics', catalog } }
})

// result.body.spec → { root: "main", elements: { ... } }
```

### Collaborative Sessions

Open two browser tabs. Both join the same session. One generates — both see it.

```typescript
// Tab 1 and Tab 2 both join
await iii.trigger({
  function_id: 'spec-forge::join-session',
  payload: { body: { session_id: 'team-dashboard', worker_id: 'tab-1' } }
})

// Tab 1 generates — spec-forge fans out patches to all peers
await iii.trigger({
  function_id: 'api::post::spec-forge::stream',
  payload: { body: { prompt: 'Revenue dashboard', catalog, session_id: 'team-dashboard' } }
})

// Tab 2 receives patches via ui::render-patch::tab-2 — renders automatically
```

### Via HTTP (curl, Postman, other services)

Every function also has an HTTP trigger:

```bash
curl -X POST http://localhost:3111/spec-forge/generate \
  -H "Content-Type: application/json" \
  -d '{"prompt": "A login form", "catalog": {"components": {"Input": {"description": "Text input"}, "Button": {"description": "Button"}}}}'
```

## Quick Start

```bash
# 1. Install iii engine
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh

# 2. Clone
git clone https://github.com/iii-hq/spec-forge.git && cd spec-forge

# 3. Set API key
echo "ANTHROPIC_API_KEY=sk-ant-..." > .env

# 4. Start engine
iii --config iii-config.yaml &

# 5. Start worker
cargo build --release && ./target/release/spec-forge &

# 6. Open demo
cd demo && python3 -m http.server 3112
```

Open `http://localhost:3112`. Two tabs = collaborative mode.

## Functions

spec-forge registers these iii functions:

| Function | Description |
|----------|-------------|
| `api::post::spec-forge::generate` | Prompt → cache check → Claude → validate → spec |
| `api::post::spec-forge::stream` | Same but streams JSONL patches via iii Channel |
| `api::post::spec-forge::refine` | Patch existing spec incrementally |
| `api::post::spec-forge::validate` | Validate spec against catalog |
| `api::post::spec-forge::prompt` | Preview the LLM prompt |
| `api::get::spec-forge::stats` | Cache + rate limiter metrics |
| `api::get::spec-forge::health` | Liveness check |
| `api::get::spec-forge::catalogs` | List built-in catalog presets |
| `spec-forge::join-session` | Join collaborative session |
| `spec-forge::leave-session` | Leave session |
| `spec-forge::push-patch` | Push patch to all session peers |

Each function has an HTTP trigger on port 3111 (e.g. `POST /spec-forge/generate`).

## Catalog Presets

| Preset | Components |
|--------|-----------|
| `dashboard` | Stack, Card, Grid, Heading, Metric, Table, Chart, Button, Text, Badge, Divider, Input |
| `form` | Stack, Card, Heading, Input, Textarea, Select, Checkbox, Radio, Button, Text |
| `ecommerce` | Stack, Grid, Card, Heading, Image, Text, Button, Metric, Badge, List |
| `minimal` | Stack, Card, Heading, Text, Button, Input |
| `3d` | 43 Three.js components (geometry, lights, cameras, effects, animation, portals) |
| `3d-product` | Product visualization (sphere, floor, studio lighting, bloom) |

Use `catalog_preset: "dashboard"` instead of defining components manually.

## Renderers

spec-forge generates [json-render](https://github.com/vercel-labs/json-render) specs. Any json-render renderer works:

| Target | Package |
|--------|---------|
| Web | `@json-render/react` |
| 3D | `@json-render/react-three-fiber` |
| PDF | `@json-render/react-pdf` |
| Email | `@json-render/react-email` |
| Video | `@json-render/remotion` |
| Terminal | `@json-render/ink` |
| Mobile | `@json-render/react-native` |
| Next.js | `@json-render/next` |
| shadcn/ui | `@json-render/shadcn` (36 components) |

## Architecture

```
iii-engine
├── WorkerModule :49134     ← backend workers (Rust, Python, TS)
├── WorkerModule :49135     ← browser workers (RBAC: expose_functions)
├── RestApiModule :3111     ← HTTP triggers
├── StateModule             ← session state (peers, specs, history)
├── StreamModule :3113      ← real-time streams
└── PubSubModule            ← event fan-out

spec-forge worker (Rust, connects to :49134)
├── generate    cache → semantic → rate limit → Claude → validate → store
├── stream      generate + fan-out patches to session peers
├── refine      JSONL patch-based incremental changes
├── validate    spec validation against catalog
├── session     join, leave, peer tracking, fan-out
└── catalogs    6 built-in presets (4 UI + 2 3D)
```

### Source

```
src/
├── main.rs      ← worker entry, function registration, core logic
├── session.rs   ← collaborative sessions (join, leave, fan-out, store)
├── types.rs     ← request/response types
├── cache.rs     ← SHA-256 exact cache + TTL
├── semantic.rs  ← TF-IDF cosine similarity cache
├── limiter.rs   ← token bucket + concurrency semaphore
├── validate.rs  ← spec validation (UI + 3D)
├── prompt.rs    ← LLM prompt builder
├── catalogs.rs  ← 6 preset catalogs
└── bench.rs     ← benchmarks
```

## Benchmarks

| Operation | spec-forge | json-render |
|-----------|------------|-------------|
| Cached request | **< 1 µs** | 3-5s (no cache) |
| First paint (streaming) | ~200ms | ~500ms |
| JSONL parsing (9 patches) | 10.1 µs | 16.8 µs |
| Validation (500 elements) | 74 µs (Rust) | 112 µs (JS) |

```bash
./bench/run.sh  # Run all benchmarks
```

## Configuration

| Env Var | Default | Description |
|---------|---------|-------------|
| `ANTHROPIC_API_KEY` | required | Claude API key |
| `DOTENV_PATH` | `.env` | Path to .env file |

Rate limit: 60 req/min + 5 concurrent. Cache TTL: 300s. Semantic threshold: 0.85. Model: `claude-sonnet-4-6`.

## Tests

```bash
cargo test  # 39 tests
```

## License

MIT
