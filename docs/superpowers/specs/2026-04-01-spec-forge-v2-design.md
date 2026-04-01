# spec-forge v2: Distributed Generative UI on iii

**Date:** 2026-04-01
**Status:** Draft
**Author:** Rohit Ghumare

## What Is spec-forge?

spec-forge is a **standard iii worker**. Nothing special — it connects to the engine, registers functions, registers triggers. That's it.

Any iii project adds spec-forge as a worker. From the browser-sdk, you just trigger its functions — they return UI. It's live composable with every other iii worker in your system.

```typescript
// From any browser-sdk project — just trigger the functions
const iii = registerWorker('ws://localhost:49135')

// Generate UI from a prompt — spec-forge worker handles cache, Claude, validation
const spec = await iii.trigger({
  function_id: 'spec-forge::generate',
  payload: { prompt: 'A sales dashboard', catalog }
})

// Stream UI — register a function, spec-forge pushes patches to it
iii.registerFunction({ id: 'ui::render-patch' }, async (patch) => renderComponent(patch))
await iii.trigger({ function_id: 'spec-forge::stream', payload: { prompt, catalog } })

// Refine existing UI
const refined = await iii.trigger({
  function_id: 'spec-forge::refine',
  payload: { prompt: 'Add a chart', current_spec: spec, catalog }
})
```

No special SDK required. Raw `iii.trigger()` works. The `@iii-hq/spec-forge` and `@iii-hq/spec-forge-react` packages are convenience wrappers — React hooks, state management, session collaboration — but the core is just iii functions.

## Problem

Vercel's json-render is the leading generative UI framework, but it has fundamental architectural limitations:

- **Single-user, single-browser** — no collaboration, no shared state
- **Client-side LLM calls** — API key exposed, no caching, no rate limiting
- **One-way communication** — browser must initiate every interaction
- **Ephemeral state** — dies when the tab closes
- **Client-side actions only** — JavaScript handlers, no server-side logic
- **Zero observability** — no tracing, no metrics

spec-forge v1 solved caching, rate limiting, and streaming with a Rust iii-sdk worker. But the client still uses plain HTTP fetch + SSE — the same one-way pattern.

## Solution

Build on `@json-render/core` + `@json-render/react` for spec format, rendering, and components — but replace the transport and state layers with iii primitives via `iii-browser-sdk`. The browser becomes a full iii worker. json-render handles the UI, iii handles everything else.

**Dependency chain:**
```
iii-browser-sdk                    → WebSocket + primitives (registerFunction, registerTrigger, trigger)
@json-render/core                  → spec format, patch parsing, Zod validation, streaming compiler
@json-render/react                 → Renderer, defineRegistry, defineCatalog, 36 shadcn components
  └─ @iii-hq/spec-forge            → iii-backed StateStore, action routing, expression resolvers, sessions
    └─ @iii-hq/spec-forge-react    → React hooks, SpecForgeProvider, iii-connected Renderer
```

**What we use from json-render:**
- `defineCatalog()` + Zod schemas — type-safe component catalogs
- `defineRegistry()` — map catalog to React components
- `Renderer` — render specs as React component trees
- `createSpecStreamCompiler()` — parse JSONL patch streams
- `applySpecPatch()` — apply RFC 6902 patches to specs
- `StateStore` interface — we implement it with iii state
- `ActionProvider` — we extend it with iii trigger routing
- `@json-render/shadcn` — 36 pre-built components (optional)

**What we replace/extend with iii:**
- `StateStore` → iii-backed store (`state::get/set` via `iii.trigger()`)
- Action handlers → `$trigger` routes to iii functions (any language)
- Transport → `iii-browser-sdk` WebSocket (not HTTP fetch)
- Streaming → backend pushes to `registerFunction('ui::render-patch')` (not SSE)
- State sync → `registerTrigger({ type: 'state' })` (not local-only)
- New expressions: `$stream`, `$trigger`, `$sync`, `$push`

## Architecture

```
                    iii-engine
    ┌──────────────────────────────────────────────┐
    │                                              │
    │  WorkerModule (:49134)  — backend workers    │
    │  WorkerModule (:49135)  — browser workers    │
    │  RestApiModule (:3111)  — HTTP fallback      │
    │  StateModule            — persistent KV      │
    │  StreamModule (:3112)   — real-time streams  │
    │  OtelModule             — traces/metrics     │
    │  QueueModule            — async processing   │
    │  PubSubModule           — event fan-out      │
    │  CronModule             — scheduled tasks    │
    │                                              │
    └──────┬────────────┬────────────┬─────────────┘
           │            │            │
     ┌─────┴─────┐ ┌────┴────┐ ┌────┴──────┐
     │ Rust      │ │ Python  │ │ Browser   │
     │ Worker    │ │ Worker  │ │ Workers   │
     │           │ │(opt ML) │ │(iii-      │
     │ generate  │ │         │ │ browser-  │
     │ stream    │ │ analyze │ │ sdk)      │
     │ refine    │ │ predict │ │           │
     │ validate  │ │         │ │ ui::*     │
     │ cache     │ └─────────┘ │ render    │
     │ rate-limit│             │ state     │
     └───────────┘             └───────────┘
```

### Key Change: Two Worker Ports

The iii engine exposes two WorkerModule ports:
- **:49134** — backend workers (Rust, Python, TypeScript) with full access
- **:49135** — browser workers with RBAC restrictions (can only call exposed functions)

Browser workers connect to :49135 with scoped permissions. They can trigger `spec-forge::*` and `stream::*` functions but cannot access internal engine functions directly.

```yaml
# iii-config.yaml additions
modules:
  - class: modules::worker::WorkerModule
    config:
      port: 49135
      rbac:
        expose_functions:
          - match("spec-forge::*")
          - match("stream::*")
          - match("state::*")
          - match("ui::*")
```

## Core Design: Browser as iii Worker

### Primitive-First Principle

Every data flow in spec-forge v2 goes through exactly two iii primitives:
- **`registerFunction()`** — declare what a worker can do
- **`registerTrigger()`** — declare when a function should run

No raw HTTP fetch. No raw WebSocket connections. No bypassing the engine. The browser is a worker, the backend is a worker, and the engine routes everything.

### Browser Worker: Registered Functions

The browser worker registers these functions that the backend (or engine) can invoke:

```typescript
const iii = registerWorker('ws://localhost:49135')

// Function: receive a spec patch from backend
iii.registerFunction(
  { id: 'ui::render-patch' },
  async (data: { patch: SpecPatch; session?: string }) => {
    applyPatch(currentSpec, data.patch)
    rerender()
    return { applied: true }
  }
)

// Function: receive a state change notification
iii.registerFunction(
  { id: 'ui::state-update' },
  async (data: { scope: string; key: string; value: unknown }) => {
    updateLocalState(data.key, data.value)
    return { received: true }
  }
)

// Function: receive a server-initiated push
iii.registerFunction(
  { id: 'ui::notification' },
  async (data: { type: string; payload: unknown }) => {
    showNotification(data)
    return { displayed: true }
  }
)

// Function: receive live stream data update
iii.registerFunction(
  { id: 'ui::stream-update' },
  async (data: { stream: string; group: string; item: string; value: unknown }) => {
    updateStreamBinding(data)
    return { updated: true }
  }
)
```

### Browser Worker: Registered Triggers

The browser worker registers these triggers to react to engine events:

```typescript
// Trigger: when session state changes, invoke ui::state-update
iii.registerTrigger({
  type: 'state',
  function_id: 'ui::state-update',
  config: { scope: 'session::my-session' },
})

// Trigger: when a stream value changes, invoke ui::stream-update
iii.registerTrigger({
  type: 'stream',
  function_id: 'ui::stream-update',
  config: { stream_name: 'metrics', group_id: 'dashboard' },
})
```

### Browser Worker: Triggering Backend Functions

Every user action calls `iii.trigger()` — no fetch, no HTTP:

```typescript
// Generate a spec — triggers Rust worker's spec-forge::generate function
const result = await iii.trigger({
  function_id: 'spec-forge::generate',
  payload: { prompt: 'A sales dashboard', catalog },
})

// Refine a spec
const refined = await iii.trigger({
  function_id: 'spec-forge::refine',
  payload: { prompt: 'Add a revenue chart', current_spec: spec, catalog },
})

// Stream a spec (backend pushes patches to ui::render-patch)
await iii.trigger({
  function_id: 'spec-forge::stream',
  payload: { prompt: 'A sales dashboard', catalog },
})

// Join a collaborative session
await iii.trigger({
  function_id: 'spec-forge::join-session',
  payload: { session_id: 'session-abc' },
})

// Validate a spec
const validation = await iii.trigger({
  function_id: 'spec-forge::validate',
  payload: { spec, catalog },
})
```

### Rust Worker: Registered Functions

The Rust backend registers these functions via `register_function_with_description()`:

```rust
// Core generation
register_function("spec-forge::generate", "Generate UI spec from prompt")
register_function("spec-forge::stream", "Stream spec patches to browser worker")
register_function("spec-forge::refine", "Refine existing spec with patches")
register_function("spec-forge::validate", "Validate spec against catalog")
register_function("spec-forge::prompt", "Preview LLM prompt")
register_function("spec-forge::stats", "Rate limiter and cache metrics")
register_function("spec-forge::health", "Liveness check")

// Session management
register_function("spec-forge::join-session", "Join collaborative session")
register_function("spec-forge::leave-session", "Leave collaborative session")

// Expression resolution
register_function("spec-forge::resolve-stream", "Set up stream trigger for $stream binding")
register_function("spec-forge::resolve-trigger", "Route $trigger expression to target function")
```

### Rust Worker: Registered Triggers

HTTP triggers remain for non-browser clients (curl, Postman, other services):

```rust
// HTTP triggers (for non-browser-SDK clients)
register_trigger("http", "spec-forge::generate", { api_path: "/spec-forge/generate", http_method: "POST" })
register_trigger("http", "spec-forge::stream", { api_path: "/spec-forge/stream", http_method: "POST" })
register_trigger("http", "spec-forge::refine", { api_path: "/spec-forge/refine", http_method: "POST" })
register_trigger("http", "spec-forge::validate", { api_path: "/spec-forge/validate", http_method: "POST" })
register_trigger("http", "spec-forge::prompt", { api_path: "/spec-forge/prompt", http_method: "POST" })
register_trigger("http", "spec-forge::stats", { api_path: "/spec-forge/stats", http_method: "GET" })
register_trigger("http", "spec-forge::health", { api_path: "/spec-forge/health", http_method: "GET" })
register_trigger("http", "spec-forge::join-session", { api_path: "/spec-forge/join", http_method: "POST" })
register_trigger("http", "spec-forge::leave-session", { api_path: "/spec-forge/leave", http_method: "POST" })
```

### Complete Data Flow Map

Every interaction mapped to primitives:

```
USER ACTION              BROWSER PRIMITIVE           ENGINE ROUTES TO        BACKEND PRIMITIVE
─────────────────────────────────────────────────────────────────────────────────────────────
Click "Generate"    →  iii.trigger("spec-forge::    → Rust worker           → registerFunction
                       generate", {prompt,catalog})                           ("spec-forge::generate")

Backend sends patch →                               ← iii.trigger("ui::    ← registerFunction
                       registerFunction               render-patch",          handler pushes
                       ("ui::render-patch")            {patch})                via TriggerAction::Void

State changes       →  registerTrigger({type:       → engine detects        → state::set by any
                       'state', fn: 'ui::state-       state change,           worker triggers
                       update', scope})                invokes fn              browser fn

Stream data updates →  registerTrigger({type:       → engine detects        → stream::set by any
                       'stream', fn: 'ui::stream-     stream change,          worker triggers
                       update', stream_name})          invokes fn              browser fn

Button $trigger     →  iii.trigger("ml::analyze",   → Python/Rust worker   → registerFunction
                       {params})                                              ("ml::analyze")

$sync input change  →  iii.trigger("state::set",    → engine stores,       → registerTrigger
                       {scope,key,value})              fires state triggers    on all peer browsers

Server push         →                               ← iii.trigger("ui::    ← registerFunction
                       registerFunction                notification",          handler pushes
                       ("ui::notification")             {type,payload})         via cron/event/manual
```

### Client SDK: `@iii-hq/spec-forge`

The SDK wraps the primitives above into a clean API:

```typescript
import { createSpecForge } from '@iii-hq/spec-forge'
import { registerWorker } from 'iii-browser-sdk'

const iii = registerWorker('ws://localhost:49135')

const forge = createSpecForge(iii, {
  catalog: myCatalog,
  onPatch: (patch, spec) => { /* render progressively */ },
  onStateChange: (path, value) => { /* update UI */ },
  onPush: (event) => { /* handle server push */ },
})
// createSpecForge internally calls:
//   iii.registerFunction('ui::render-patch', ...)
//   iii.registerFunction('ui::state-update', ...)
//   iii.registerFunction('ui::notification', ...)
//   iii.registerFunction('ui::stream-update', ...)

// Generate — iii.trigger('spec-forge::generate', ...)
const spec = await forge.generate('A sales dashboard')

// Refine — iii.trigger('spec-forge::refine', ...)
const updated = await forge.refine('Add a revenue chart')

// Join session — iii.trigger('spec-forge::join-session', ...)
//   + iii.registerTrigger({ type: 'state', scope: 'session::abc' })
forge.join('session-abc')
```

### React Integration: `@iii-hq/spec-forge-react`

```tsx
import { SpecForgeProvider, useSpecForge, useForgeStream, Renderer } from '@iii-hq/spec-forge-react'

function App() {
  return (
    <SpecForgeProvider
      engineUrl="ws://localhost:49135"
      catalog={catalog}
      registry={registry}
    >
      <Dashboard />
    </SpecForgeProvider>
  )
}
// SpecForgeProvider internally:
//   registerWorker(engineUrl)
//   createSpecForge(iii, { catalog })
//   Registers all functions + triggers

function Dashboard() {
  const { generate, refine, spec, status } = useSpecForge()
  // generate() → iii.trigger('spec-forge::generate', ...)
  // refine()   → iii.trigger('spec-forge::refine', ...)

  const { streamSpec, patches } = useForgeStream()
  // Listens to ui::render-patch function invocations

  return (
    <div>
      <button onClick={() => generate('A sales dashboard')}>Generate</button>
      {status === 'streaming' && <p>Rendering... ({patches.length} patches)</p>}
      {spec && <Renderer spec={spec} registry={registry} />}
    </div>
  )
}
```

### React Hooks API

Every hook maps to iii primitives:

| Hook | iii Primitive Used |
|------|--------------------|
| `useSpecForge()` | `iii.trigger('spec-forge::generate/refine/validate')` |
| `useForgeStream()` | Listens to `registerFunction('ui::render-patch')` invocations |
| `useForgeState(path)` | `iii.trigger('state::get')` + `registerTrigger({ type: 'state' })` |
| `useForgeAction(name)` | `iii.trigger(name, payload)` |
| `useForgeSession()` | `iii.trigger('spec-forge::join/leave-session')` + `registerTrigger({ type: 'state', scope })` |
| `useForgeHistory()` | `iii.trigger('state::get', { key: 'history' })` |
| `useLiveData(stream)` | `registerTrigger({ type: 'stream' })` → `registerFunction('ui::stream-update')` |

## New Spec Format Extensions

spec-forge v2 extends the json-render spec format with iii-native expressions. These are **additive** — a plain json-render spec still works.

### `$stream` — Live Data Binding

Binds a prop to an iii Stream value. Updates in real-time as the stream changes.

```json
{
  "type": "Metric",
  "props": {
    "label": "Active Users",
    "value": { "$stream": "metrics/users/active" }
  }
}
```

**Resolution (all primitives):**
1. Client calls `iii.trigger({ function_id: 'spec-forge::resolve-stream', payload: { stream: 'metrics', group: 'users', item: 'active' } })`
2. Rust worker calls `iii.registerTrigger({ type: 'stream', function_id: 'ui::stream-update', config: { stream_name: 'metrics', group_id: 'users' } })` on behalf of the browser
3. When any worker calls `iii.trigger({ function_id: 'stream::set', payload: { stream_name: 'metrics', group_id: 'users', item_id: 'active', data: 42 } })`, the engine fires the stream trigger
4. Engine invokes browser's `registerFunction('ui::stream-update')` with the new value
5. Browser updates the Metric prop reactively

### `$trigger` — Server-Side Action Binding

Binds an event to an iii function trigger instead of a client-side action.

```json
{
  "type": "Button",
  "props": { "label": "Analyze Sentiment" },
  "on": {
    "press": {
      "$trigger": "ml::analyze-sentiment",
      "params": { "text": { "$state": "/input/text" } }
    }
  }
}
```

**Resolution (all primitives):**
1. Button press → browser calls `iii.trigger({ function_id: 'ml::analyze-sentiment', payload: { text: resolvedStateValue } })`
2. Engine routes to Python worker's `registerFunction('ml::analyze-sentiment')`
3. Python worker processes, then calls `iii.trigger({ function_id: 'state::set', payload: { scope: 'session::abc', key: '/results/sentiment', value: result } })`
4. Engine fires state trigger → invokes browser's `registerFunction('ui::state-update')` with the result
5. Browser updates UI reactively

### `$sync` — Collaborative State Binding

Like `$bindState` but synced across all browsers in the session via iii state.

```json
{
  "type": "Input",
  "props": {
    "value": { "$sync": "/filters/region" },
    "placeholder": "Filter by region"
  }
}
```

**Resolution (all primitives):**
1. Input change → browser calls `iii.trigger({ function_id: 'state::set', payload: { scope: 'session::abc', key: '/filters/region', value: 'US' } })`
2. Engine stores the value, detects state change in scope `session::abc`
3. All browsers in the session have `iii.registerTrigger({ type: 'state', function_id: 'ui::state-update', config: { scope: 'session::abc' } })` (set up during `forge.join()`)
4. Engine invokes each browser's `registerFunction('ui::state-update')` with `{ scope, key, value }`
5. Each browser updates the Input value reactively

### `$push` — Server-Push Slot

Marks an element as a target for server-initiated content.

```json
{
  "type": "Card",
  "props": { "title": "Alerts" },
  "$push": "alerts",
  "children": []
}
```

**Resolution (all primitives):**
1. Backend (any worker) calls `iii.trigger({ function_id: 'spec-forge::push-patch', payload: { session_id, target: 'alerts', patch: { op: 'add', path: '/elements/alert-1', value: {...} } } })`
2. Rust worker's `registerFunction('spec-forge::push-patch')` handler reads session peers via `iii.trigger({ function_id: 'state::get', payload: { scope: 'session::{id}', key: 'peers' } })`
3. For each peer, calls `iii.trigger({ function_id: 'ui::render-patch', payload: { patch, target: 'alerts' }, action: TriggerAction::Void() })` targeting that browser's worker
4. Engine routes to each browser's `registerFunction('ui::render-patch')`
5. Browser applies the patch to the `$push: "alerts"` slot

A cron trigger can also push patches on a schedule:
```rust
register_trigger("cron", "spec-forge::push-patch", {
    expression: "0 */5 * * * *",  // every 5 minutes
    payload: { session_id: "dashboard", target: "alerts", ... }
})
```

## Rust Worker Changes

### New Functions to Register

| Function | Purpose |
|----------|---------|
| `spec-forge::generate` | Existing — cache → semantic → Claude → validate → return |
| `spec-forge::stream` | **Changed** — pushes patches to browser function instead of channel |
| `spec-forge::refine` | Existing — JSONL refinement |
| `spec-forge::validate` | Existing — catalog validation |
| `spec-forge::join-session` | **New** — join collaborative session, set up state triggers |
| `spec-forge::leave-session` | **New** — leave session, clean up triggers |
| `spec-forge::push-patch` | **New** — push a patch to all browsers in a session |
| `spec-forge::resolve-stream` | **New** — resolve `$stream` expressions, subscribe browser |
| `spec-forge::resolve-trigger` | **New** — resolve `$trigger` expressions, route to target function |

### Stream Function Change (v1 → v2)

**v1 (current):** Creates an iii Channel, writes patches to it, browser reads via separate WebSocket.

```rust
// v1: Channel-based streaming
let channel = iii.create_channel(64).await?;
// ... write patches to channel.writer
// Browser connects to ws://engine:49134/ws/channels/{id}
```

**v2 (new):** Pushes patches directly to browser-registered functions via `iii.trigger()`.

The Rust worker discovers connected browsers by reading the session peer list from iii state (`session::{id}/peers`). Each peer entry contains the browser's worker ID, set when the browser calls `spec-forge::join-session`.

```rust
// v2: Direct function invocation on browser workers
let peers: Vec<String> = iii.trigger(TriggerRequest {
    function_id: "state::get".into(),
    payload: json!({ "scope": format!("session::{}", session_id), "key": "peers" }),
}).await?;

for patch in patches {
    for peer_worker_id in &peers {
        iii.trigger(TriggerRequest {
            function_id: format!("ui::render-patch::{}", peer_worker_id),
            payload: json!({ "patch": patch, "session": session_id }),
            action: TriggerAction::Void(), // fire-and-forget per browser
        }).await;
    }
}
```

The browser worker has registered `ui::render-patch` — the engine routes the trigger to the correct browser WebSocket. No channel setup, no separate connection. For single-user mode (no session), the Rust worker uses the requesting browser's worker ID from the trigger context.

### Session Management (All Primitives)

Sessions are iii state scopes. Each session has:

```
state scope: session::{session_id}
  /spec       — current spec (full JSON)
  /peers      — connected browser worker IDs
  /history    — version array [{spec, timestamp, author}]
  /cursor     — per-peer cursor positions (for collaborative editing)
```

**Join session (every step is registerFunction or trigger):**

1. Browser: `iii.trigger({ function_id: 'spec-forge::join-session', payload: { session_id } })`
2. Rust worker handler (registered via `registerFunction('spec-forge::join-session')`):
   - `iii.trigger({ function_id: 'state::get', payload: { scope: 'session::abc', key: 'peers' } })` → get current peers
   - `iii.trigger({ function_id: 'state::set', payload: { scope: 'session::abc', key: 'peers', value: [...peers, browser_worker_id] } })` → add peer
   - `iii.trigger({ function_id: 'state::get', payload: { scope: 'session::abc', key: 'spec' } })` → get current spec
   - `iii.trigger({ function_id: 'ui::render-patch', payload: { patch: fullSpec }, action: TriggerAction::Void() })` → push spec to browser
3. Browser: `iii.registerTrigger({ type: 'state', function_id: 'ui::state-update', config: { scope: 'session::abc' } })` → listen for state changes
4. Browser: `iii.registerTrigger({ type: 'stream', function_id: 'ui::stream-update', config: { stream_name: 'session-abc-data' } })` → listen for live data

**Generate within session (fan-out via primitives):**

1. Browser A: `iii.trigger({ function_id: 'spec-forge::stream', payload: { prompt, catalog, session_id } })`
2. Rust worker handler (registered via `registerFunction('spec-forge::stream')`):
   - Cache check: `iii.trigger({ function_id: 'state::get', ... })`
   - Claude API call (if cache miss)
   - Read peers: `iii.trigger({ function_id: 'state::get', payload: { scope, key: 'peers' } })`
   - For each patch, for each peer: `iii.trigger({ function_id: 'ui::render-patch', payload: { patch }, action: TriggerAction::Void() })` → all browsers get each patch
   - Store spec: `iii.trigger({ function_id: 'state::set', payload: { scope, key: 'spec', value: finalSpec } })`
   - Append history: `iii.trigger({ function_id: 'state::set', payload: { scope, key: 'history', value: [...history, entry] } })`

**Leave session:**

1. Browser: `iii.trigger({ function_id: 'spec-forge::leave-session', payload: { session_id } })`
2. Rust worker: removes browser from peers via `iii.trigger({ function_id: 'state::set', ... })`
3. Browser: calls `trigger.unregister()` on state/stream triggers (handles returned by `registerTrigger`)
4. Browser: calls `functionRef.unregister()` if tearing down (handles returned by `registerFunction`)

## Performance Targets

| Metric | json-render | spec-forge v1 | spec-forge v2 | Improvement |
|--------|-------------|---------------|---------------|-------------|
| Connection overhead | ~80ms/request (HTTP) | ~70ms (HTTP+WS) | **0ms** (persistent WS) | infinite |
| Cached request | 3-5s (no cache) | ~50ms | **~2ms** | ~2000x vs json-render |
| First paint (cache miss) | ~500ms | ~270ms | **~200ms** | 2.5x vs json-render |
| Collaborative push | impossible | N/A | **~5ms** | N/A |
| State sync (cross-browser) | impossible | N/A | **~8ms** | N/A |
| Action round-trip (server) | impossible | N/A | **~15ms** | N/A |
| Patches/second throughput | ~100 (SSE parsing) | ~500 (WS channel) | **~2000** (direct invoke) | 20x vs json-render |

### Benchmark Plan

1. **Micro-benchmarks** (Rust): cache lookup, validation, patch parsing (existing, keep)
2. **Connection overhead**: measure HTTP round-trip vs WebSocket message latency
3. **Streaming throughput**: patches/second — SSE vs Channel vs direct function invocation
4. **Collaborative latency**: time from one browser's action to another browser's render
5. **State sync latency**: time from `state::set` to browser `ui::state-update` firing
6. **Cold vs warm**: first generate (cache miss) vs repeat (cache hit)
7. **Fan-out**: push to N browsers simultaneously, measure P50/P99

## Package Structure

```
spec-forge/
├── Cargo.toml                    # Rust worker (existing, extended)
├── iii-config.yaml               # Updated: +browser WorkerModule, +StreamModule
├── src/                          # Rust worker source
│   ├── main.rs                   # Extended: new functions, session management
│   ├── session.rs                # NEW: session state, peer tracking, fan-out
│   ├── resolve.rs                # NEW: $stream/$trigger/$sync/$push resolution
│   ├── cache.rs                  # Existing
│   ├── semantic.rs               # Existing
│   ├── limiter.rs                # Existing
│   ├── validate.rs               # Extended: validate new expressions
│   ├── prompt.rs                 # Existing
│   ├── catalogs.rs               # Existing
│   └── types.rs                  # Extended: new expression types
├── client/                       # TypeScript client SDK
│   ├── package.json              # @iii-hq/spec-forge
│   │                             # deps: iii-browser-sdk, @json-render/core
│   ├── src/
│   │   ├── index.ts              # createSpecForge() — registers functions/triggers, wraps iii
│   │   ├── state-store.ts        # IIIStateStore — implements json-render's StateStore via iii primitives
│   │   │                         #   get() → iii.trigger('state::get')
│   │   │                         #   set() → iii.trigger('state::set')
│   │   │                         #   subscribe() → registerTrigger({ type: 'state' })
│   │   ├── action-router.ts      # IIIActionRouter — routes $trigger actions to iii.trigger()
│   │   │                         #   dispatch(action) → iii.trigger(action.function_id, params)
│   │   ├── expressions.ts        # Resolve $stream, $sync, $push expressions via iii primitives
│   │   ├── session.ts            # Join/leave sessions via iii.trigger('spec-forge::join/leave-session')
│   │   └── types.ts              # Extended expression types ($stream, $trigger, $sync, $push)
│   └── tsconfig.json
├── react/                        # React integration
│   ├── package.json              # @iii-hq/spec-forge-react
│   │                             # deps: @iii-hq/spec-forge, @json-render/react
│   ├── src/
│   │   ├── index.tsx             # Exports
│   │   ├── provider.tsx          # SpecForgeProvider — wraps json-render providers with iii connection
│   │   │                         #   <StateProvider store={iiiStateStore}>
│   │   │                         #   <ActionProvider actions={iiiActionRouter}>
│   │   │                         #   internally: registerWorker(), createSpecForge()
│   │   ├── hooks.ts              # useSpecForge, useForgeStream, useForgeState, etc.
│   │   │                         #   all hooks call iii primitives underneath
│   │   └── renderer.tsx          # Thin wrapper: json-render's <Renderer> + iii expression resolution
│   └── tsconfig.json
├── demo/
│   └── index.html                # Updated: uses browser SDK, shows collaboration
├── bench/                        # Extended benchmarks
│   ├── ws-vs-http.mjs            # NEW: connection overhead comparison
│   ├── streaming-throughput.mjs  # NEW: patches/sec across transport methods
│   ├── collab-latency.mjs        # NEW: cross-browser sync timing
│   └── ...                       # Existing benchmarks (keep)
└── examples/
    ├── react-usage.tsx           # Updated: uses hooks + json-render Renderer
    ├── collaborative.tsx         # NEW: multi-user dashboard
    ├── live-data.tsx             # NEW: $stream binding with real-time metrics
    ├── server-push.tsx           # NEW: backend-initiated UI updates
    └── shadcn.tsx                # NEW: using @json-render/shadcn components with iii
```

### Key Integration Points with json-render

**1. IIIStateStore (implements json-render's `StateStore` interface):**

```typescript
import type { StateStore, StateModel } from '@json-render/core'
import type { ISdk } from 'iii-browser-sdk'

export function createIIIStateStore(iii: ISdk, scope: string): StateStore {
  let snapshot: StateModel = {}
  const listeners = new Set<() => void>()

  // On creation, register state trigger via iii primitive
  iii.registerTrigger({
    type: 'state',
    function_id: 'ui::state-update',
    config: { scope },
  })

  return {
    get: (path: string) => getByPath(snapshot, path),

    set: (path: string, value: unknown) => {
      // Write through iii primitive — NOT local mutation
      iii.trigger({
        function_id: 'state::set',
        payload: { scope, key: path, value },
      })
    },

    update: (updates: Record<string, unknown>) => {
      for (const [path, value] of Object.entries(updates)) {
        iii.trigger({
          function_id: 'state::set',
          payload: { scope, key: path, value },
        })
      }
    },

    getSnapshot: () => snapshot,
    subscribe: (listener: () => void) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },

    // Called by ui::state-update registerFunction handler
    _applyRemoteUpdate(key: string, value: unknown) {
      setByPath(snapshot, key, value)
      snapshot = { ...snapshot } // new ref for useSyncExternalStore
      listeners.forEach(fn => fn())
    },
  }
}
```

json-render's `<StateProvider store={iiiStore}>` accepts this directly. All `$bindState` and `$state` expressions resolve through iii state automatically.

**2. IIIActionRouter (extends json-render's action system):**

```typescript
import type { ISdk } from 'iii-browser-sdk'

export function createIIIActionRouter(iii: ISdk) {
  return {
    // Called by json-render's ActionProvider when an action fires
    async dispatch(action: string, params: Record<string, unknown>) {
      // Built-in json-render actions (setState, pushState, etc.) handled by json-render
      // $trigger actions route to iii functions
      if (action.includes('::')) {
        // Namespaced = iii function trigger
        return iii.trigger({
          function_id: action,
          payload: params,
        })
      }
      // Non-namespaced = local json-render action (pass through)
      return null
    },
  }
}
```

**3. Renderer wrapper (json-render Renderer + iii expressions):**

```tsx
import { Renderer as JsonRenderRenderer } from '@json-render/react'

export function Renderer({ spec, registry, iii }) {
  // Pre-process spec: resolve $stream, $sync, $push into json-render-compatible expressions
  const resolvedSpec = useResolvedSpec(spec, iii)

  return (
    <JsonRenderRenderer
      spec={resolvedSpec}
      registry={registry}
    />
  )
}
```

`$stream` resolves to a `$state` path that gets updated by `registerTrigger({ type: 'stream' })`.
`$sync` resolves to a `$bindState` path backed by `IIIStateStore`.
`$push` resolves to a children array that gets updated by `registerFunction('ui::render-patch')`.
`$trigger` resolves to an `on` binding with the namespaced action routed by `IIIActionRouter`.

## Migration from v1

The v1 HTTP client (`IIIRenderClient`) stays in `client/src/compat.ts` as a fallback. The json-render adapter stays too. No breaking changes — v2 is additive.

Users upgrade by:
1. `npm install @iii-hq/spec-forge iii-browser-sdk`
2. Replace `new IIIRenderClient()` with `createSpecForge(iii, { catalog })`
3. For React: wrap app in `<SpecForgeProvider>`, use hooks

## Security

- Browser workers connect to dedicated port (:49135) with RBAC
- RBAC restricts browser workers to `spec-forge::*`, `stream::*`, `state::*`, `ui::*` functions
- Browser workers cannot access engine internals, queue management, or cron
- Session state is scoped — browser can only access sessions it has joined
- API key stays server-side (Rust worker only)
- Rate limiting applies to browser triggers (existing token bucket)

## Non-Goals

- Not forking json-render — we use it as a dependency, not a copy. Bug fixes go upstream.
- Not reimplementing json-render's expressions (`$computed`, `$template`, `$cond`, `$bindState`) — they work as-is through `IIIStateStore`. We only ADD new iii-native expressions (`$stream`, `$trigger`, `$sync`, `$push`).
- Not implementing CRDT-based conflict resolution — last-write-wins for v2, CRDT is a future enhancement
- Not supporting offline mode — browser SDK requires WebSocket connection
- Vue/Svelte/Solid renderers are possible (json-render supports them) but not in v2 scope — React first

## Success Criteria

1. Browser connects via single WebSocket, no HTTP requests for spec operations
2. Cached requests return in <5ms end-to-end (measured at browser)
3. Collaborative: two browsers see each other's changes in <50ms
4. Server push: backend-initiated patch arrives at browser in <10ms
5. Streaming throughput: >1000 patches/second sustained
6. All existing v1 benchmarks still pass (no regression)
7. json-render adapter still works (backward compatibility)
8. Demo shows: generate, stream, refine, collaborate, live data, server push
