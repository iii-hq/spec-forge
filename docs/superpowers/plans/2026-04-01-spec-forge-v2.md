# spec-forge v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite spec-forge as a standard iii worker with browser-sdk integration, using json-render for rendering and iii primitives for transport, state, and collaboration.

**Architecture:** spec-forge is a Rust iii-sdk worker that registers functions (`spec-forge::generate`, `spec-forge::stream`, `spec-forge::refine`, etc.) and HTTP triggers. The browser connects via `iii-browser-sdk`, triggers these functions, and receives patches via `registerFunction('ui::render-patch')`. State syncs via iii state triggers. json-render handles rendering.

**Tech Stack:** Rust (iii-sdk), TypeScript (iii-browser-sdk, @json-render/core, @json-render/react), React 19, Vitest

---

## File Map

### Rust Worker (existing, extended)
- `src/main.rs` — add session functions, change stream to push-based
- `src/session.rs` — NEW: session state, peer tracking, fan-out
- `src/types.rs` — add session types
- `iii-config.yaml` — add browser WorkerModule (:49135), StreamModule

### TypeScript Client SDK (`client/`)
- `client/package.json` — NEW deps: `iii-browser-sdk`, `@json-render/core`
- `client/src/index.ts` — REWRITE: `createSpecForge()` using iii primitives
- `client/src/state-store.ts` — NEW: `IIIStateStore` implementing json-render `StateStore`
- `client/src/action-router.ts` — NEW: routes `$trigger` actions to `iii.trigger()`
- `client/src/expressions.ts` — NEW: resolve `$stream`, `$sync`, `$push`
- `client/src/session.ts` — NEW: join/leave via `iii.trigger()`
- `client/src/types.ts` — NEW: extended expression types

### React Package (`react/`)
- `react/package.json` — NEW: `@iii-hq/spec-forge-react`
- `react/src/index.tsx` — exports
- `react/src/provider.tsx` — `SpecForgeProvider` wrapping json-render providers
- `react/src/hooks.ts` — `useSpecForge`, `useForgeStream`, `useForgeState`, etc.
- `react/src/renderer.tsx` — thin wrapper around json-render `Renderer`

### Tests
- `client/src/__tests__/state-store.test.ts`
- `client/src/__tests__/action-router.test.ts`
- `client/src/__tests__/expressions.test.ts`
- `client/src/__tests__/session.test.ts`
- `client/src/__tests__/index.test.ts`
- `react/src/__tests__/provider.test.tsx`
- `react/src/__tests__/hooks.test.tsx`

---

### Task 1: Update iii-config.yaml for Browser Workers

**Files:**
- Modify: `iii-config.yaml`

- [ ] **Step 1: Add browser WorkerModule and StreamModule**

```yaml
# Add after the existing WorkerModule entry in iii-config.yaml

  - class: modules::worker::WorkerModule
    config:
      port: 49135
      rbac:
        expose_functions:
          - match("spec-forge::*")
          - match("stream::*")
          - match("state::*")
          - match("ui::*")

  - class: modules::stream::StreamModule
    config:
      port: 3112
      host: 127.0.0.1
      adapter:
        class: modules::stream::adapters::KvStore
        config:
          store_method: file_based
          file_path: ./data/stream_store
```

Also add `http://localhost:5173` to CORS allowed_origins (for Vite dev server).

- [ ] **Step 2: Verify engine starts with new config**

Run: `iii --config iii-config.yaml`
Expected: Engine starts, logs show two WorkerModule instances on :49134 and :49135, StreamModule on :3112.

- [ ] **Step 3: Commit**

```bash
git add iii-config.yaml
git commit -m "feat: add browser WorkerModule and StreamModule to iii config"
```

---

### Task 2: Add Session Types and Module to Rust Worker

**Files:**
- Create: `src/session.rs`
- Modify: `src/types.rs`
- Modify: `src/main.rs` (add `mod session;`)

- [ ] **Step 1: Add session types to `src/types.rs`**

Append to the existing `types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinSessionRequest {
    pub session_id: String,
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaveSessionRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushPatchRequest {
    pub session_id: String,
    pub target: Option<String>,
    pub patch: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub peers: Vec<String>,
    pub spec: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub spec: serde_json::Value,
    pub timestamp: u64,
    pub author: String,
}
```

- [ ] **Step 2: Create `src/session.rs`**

```rust
use crate::types::*;
use iii_sdk::{III, TriggerAction, TriggerRequest};
use serde_json::json;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn join_session(
    iii: &III,
    session_id: &str,
    worker_id: &str,
) -> Result<SessionInfo, Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    let peers_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".into(),
            payload: json!({ "scope": scope, "key": "peers" }),
            ..Default::default()
        })
        .await
        .unwrap_or(json!([]));

    let mut peers: Vec<String> = serde_json::from_value(peers_val).unwrap_or_default();

    if !peers.contains(&worker_id.to_string()) {
        peers.push(worker_id.to_string());
    }

    iii.trigger(TriggerRequest {
        function_id: "state::set".into(),
        payload: json!({ "scope": scope, "key": "peers", "value": peers }),
        ..Default::default()
    })
    .await?;

    let spec: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".into(),
            payload: json!({ "scope": scope, "key": "spec" }),
            ..Default::default()
        })
        .await
        .unwrap_or(json!(null));

    Ok(SessionInfo {
        session_id: session_id.to_string(),
        peers,
        spec: if spec.is_null() { None } else { Some(spec) },
    })
}

pub async fn leave_session(
    iii: &III,
    session_id: &str,
    worker_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    let peers_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".into(),
            payload: json!({ "scope": scope, "key": "peers" }),
            ..Default::default()
        })
        .await
        .unwrap_or(json!([]));

    let mut peers: Vec<String> = serde_json::from_value(peers_val).unwrap_or_default();
    peers.retain(|p| p != worker_id);

    iii.trigger(TriggerRequest {
        function_id: "state::set".into(),
        payload: json!({ "scope": scope, "key": "peers", "value": peers }),
        ..Default::default()
    })
    .await?;

    Ok(())
}

pub async fn fan_out_patch(
    iii: &III,
    session_id: &str,
    patch: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    let peers_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".into(),
            payload: json!({ "scope": scope, "key": "peers" }),
            ..Default::default()
        })
        .await
        .unwrap_or(json!([]));

    let peers: Vec<String> = serde_json::from_value(peers_val).unwrap_or_default();

    for peer in &peers {
        let _ = iii
            .trigger(TriggerRequest {
                function_id: format!("ui::render-patch::{}", peer),
                payload: json!({ "patch": patch, "session": session_id }),
                action: Some(TriggerAction::Void()),
                ..Default::default()
            })
            .await;
    }

    Ok(())
}

pub async fn store_spec(
    iii: &III,
    session_id: &str,
    spec: &serde_json::Value,
    author: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    iii.trigger(TriggerRequest {
        function_id: "state::set".into(),
        payload: json!({ "scope": scope, "key": "spec", "value": spec }),
        ..Default::default()
    })
    .await?;

    let history_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".into(),
            payload: json!({ "scope": scope, "key": "history" }),
            ..Default::default()
        })
        .await
        .unwrap_or(json!([]));

    let mut history: Vec<HistoryEntry> =
        serde_json::from_value(history_val).unwrap_or_default();

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    history.push(HistoryEntry {
        spec: spec.clone(),
        timestamp: ts,
        author: author.to_string(),
    });

    iii.trigger(TriggerRequest {
        function_id: "state::set".into(),
        payload: json!({ "scope": scope, "key": "history", "value": history }),
        ..Default::default()
    })
    .await?;

    Ok(())
}
```

- [ ] **Step 3: Add `mod session;` to `src/main.rs`**

Add after the existing `mod` declarations at the top of `src/main.rs`:

```rust
mod session;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: No errors.

- [ ] **Step 5: Commit**

```bash
git add src/session.rs src/types.rs src/main.rs
git commit -m "feat: add session module with join, leave, fan-out, store via iii primitives"
```

---

### Task 3: Register Session Functions in Rust Worker

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add session function handlers to `register_functions()`**

In `src/main.rs`, inside the `register_functions()` function, add after the existing function registrations:

```rust
// Session management
{
    let state = shared.clone();
    iii.register_function_with_description(
        "spec-forge::join-session",
        "Join a collaborative session",
        move |input: ApiRequest<JoinSessionRequest>| {
            let state = state.clone();
            async move {
                let req = input.body;
                let worker_id = req.worker_id.unwrap_or_else(|| "anonymous".to_string());
                let info = session::join_session(&state.iii, &req.session_id, &worker_id).await
                    .map_err(|e| IIIError::Internal(e.to_string()))?;

                if let Some(spec) = &info.spec {
                    let _ = state.iii.trigger(TriggerRequest {
                        function_id: format!("ui::render-patch::{}", worker_id),
                        payload: json!({ "patch": { "op": "replace", "path": "", "value": spec }, "session": req.session_id }),
                        action: Some(TriggerAction::Void()),
                        ..Default::default()
                    }).await;
                }

                Ok(ApiResponse {
                    status_code: 200,
                    body: json!(info),
                    headers: json_headers(),
                })
            }
        },
    ).await;
}

{
    let state = shared.clone();
    iii.register_function_with_description(
        "spec-forge::leave-session",
        "Leave a collaborative session",
        move |input: ApiRequest<LeaveSessionRequest>| {
            let state = state.clone();
            async move {
                let req = input.body;
                let worker_id = "anonymous".to_string();
                session::leave_session(&state.iii, &req.session_id, &worker_id).await
                    .map_err(|e| IIIError::Internal(e.to_string()))?;

                Ok(ApiResponse {
                    status_code: 200,
                    body: json!({ "left": true }),
                    headers: json_headers(),
                })
            }
        },
    ).await;
}

{
    let state = shared.clone();
    iii.register_function_with_description(
        "spec-forge::push-patch",
        "Push a patch to all browsers in a session",
        move |input: ApiRequest<PushPatchRequest>| {
            let state = state.clone();
            async move {
                let req = input.body;
                session::fan_out_patch(&state.iii, &req.session_id, &req.patch).await
                    .map_err(|e| IIIError::Internal(e.to_string()))?;

                Ok(ApiResponse {
                    status_code: 200,
                    body: json!({ "pushed": true }),
                    headers: json_headers(),
                })
            }
        },
    ).await;
}
```

- [ ] **Step 2: Register HTTP triggers for session endpoints**

In `register_http_triggers()`, add:

```rust
iii.register_trigger(RegisterTriggerInput {
    r#type: "http".into(),
    function_id: "spec-forge::join-session".into(),
    config: json!({ "api_path": "/spec-forge/join", "http_method": "POST" }),
}).await;

iii.register_trigger(RegisterTriggerInput {
    r#type: "http".into(),
    function_id: "spec-forge::leave-session".into(),
    config: json!({ "api_path": "/spec-forge/leave", "http_method": "POST" }),
}).await;

iii.register_trigger(RegisterTriggerInput {
    r#type: "http".into(),
    function_id: "spec-forge::push-patch".into(),
    config: json!({ "api_path": "/spec-forge/push", "http_method": "POST" }),
}).await;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: register session functions and HTTP triggers"
```

---

### Task 4: Change Stream Function to Push-Based

**Files:**
- Modify: `src/main.rs` (the `stream_core` function)

- [ ] **Step 1: Modify `stream_core` to accept optional `session_id` and push patches via `iii.trigger()`**

Replace the channel-based streaming in `stream_core` with push-based delivery. The function should:

1. Check if `session_id` is provided in the request
2. If yes: fan out patches to all peers via `session::fan_out_patch()`
3. If no: use the requesting worker's ID from the trigger context and push directly
4. After all patches sent, store the final spec in session state if session mode

In the `GenerateRequest` struct in `types.rs`, add:
```rust
pub session_id: Option<String>,
```

Then in `stream_core`, replace the channel creation and writing with:

```rust
// Instead of: let channel = iii.create_channel(64).await?;
// Push patches directly to browser workers

for patch in &patches {
    let patch_json = json!({
        "type": "patch",
        "patch": patch,
    });

    if let Some(ref sid) = req.session_id {
        session::fan_out_patch(&state.iii, sid, &patch_json).await
            .map_err(|e| IIIError::Internal(e.to_string()))?;
    } else {
        // Single-user mode: push to requesting worker
        let _ = state.iii.trigger(TriggerRequest {
            function_id: "ui::render-patch".into(),
            payload: patch_json,
            action: Some(TriggerAction::Void()),
            ..Default::default()
        }).await;
    }
}

// Send done message
let done_json = json!({
    "type": "done",
    "spec": final_spec,
    "valid": validation_result.valid,
    "generation_ms": elapsed_ms,
});

if let Some(ref sid) = req.session_id {
    session::fan_out_patch(&state.iii, sid, &done_json).await
        .map_err(|e| IIIError::Internal(e.to_string()))?;
    session::store_spec(&state.iii, sid, &json!(final_spec), "browser").await
        .map_err(|e| IIIError::Internal(e.to_string()))?;
} else {
    let _ = state.iii.trigger(TriggerRequest {
        function_id: "ui::render-patch".into(),
        payload: done_json,
        action: Some(TriggerAction::Void()),
        ..Default::default()
    }).await;
}
```

- [ ] **Step 2: Keep the old channel-based stream as a fallback**

Add a `use_channel: Option<bool>` field to `GenerateRequest`. When `true`, use the old channel path. Default to push-based.

- [ ] **Step 3: Verify it compiles and existing tests pass**

Run: `cargo check && cargo test`
Expected: All 39 existing tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/types.rs
git commit -m "feat: change stream to push-based delivery via iii.trigger()"
```

---

### Task 5: Create TypeScript Client Types

**Files:**
- Create: `client/src/types.ts`

- [ ] **Step 1: Write types**

```typescript
import type { Spec, JsonPatch } from '@json-render/core'

export interface SpecForgeCatalog {
  components: Record<string, {
    description: string
    props?: Record<string, unknown>
    children?: boolean
  }>
  actions?: Record<string, { description: string }>
}

export interface SpecForgeOptions {
  catalog: SpecForgeCatalog
  scope?: string
  model?: string
  onPatch?: (event: PatchEvent) => void
  onStateChange?: (key: string, value: unknown) => void
  onNotification?: (event: NotificationEvent) => void
}

export interface PatchEvent {
  type: 'patch' | 'done'
  patch?: JsonPatch
  spec?: Spec
  valid?: boolean
  generation_ms?: number
  session?: string
}

export interface NotificationEvent {
  type: string
  payload: unknown
}

export interface GenerateResult {
  spec: Spec
  cached: boolean
  generation_ms: number
  model: string
}

export interface StreamExpression {
  $stream: string
}

export interface TriggerExpression {
  $trigger: string
  params?: Record<string, unknown>
}

export interface SyncExpression {
  $sync: string
}

export interface PushExpression {
  $push: string
}

export type IIIExpression =
  | StreamExpression
  | TriggerExpression
  | SyncExpression
  | PushExpression

export function isStreamExpr(v: unknown): v is StreamExpression {
  return typeof v === 'object' && v !== null && '$stream' in v
}

export function isTriggerExpr(v: unknown): v is TriggerExpression {
  return typeof v === 'object' && v !== null && '$trigger' in v
}

export function isSyncExpr(v: unknown): v is SyncExpression {
  return typeof v === 'object' && v !== null && '$sync' in v
}

export function isPushExpr(v: unknown): v is PushExpression {
  return typeof v === 'object' && v !== null && '$push' in v
}
```

- [ ] **Step 2: Commit**

```bash
git add client/src/types.ts
git commit -m "feat: add TypeScript types for spec-forge v2 expressions"
```

---

### Task 6: Implement IIIStateStore

**Files:**
- Create: `client/src/state-store.ts`
- Create: `client/src/__tests__/state-store.test.ts`

- [ ] **Step 1: Write failing test**

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createIIIStateStore } from '../state-store'

function createMockIII() {
  const triggers: Array<{ type: string; function_id: string; config: unknown }> = []
  const triggerCalls: Array<{ function_id: string; payload: unknown }> = []

  return {
    registerTrigger: vi.fn((input) => { triggers.push(input); return { unregister: vi.fn() } }),
    trigger: vi.fn(async (req) => {
      triggerCalls.push(req)
      return undefined
    }),
    registerFunction: vi.fn(() => ({ id: 'test', unregister: vi.fn() })),
    _triggers: triggers,
    _triggerCalls: triggerCalls,
  }
}

describe('IIIStateStore', () => {
  let iii: ReturnType<typeof createMockIII>
  let store: ReturnType<typeof createIIIStateStore>

  beforeEach(() => {
    iii = createMockIII()
    store = createIIIStateStore(iii as any, 'test-scope')
  })

  it('registers a state trigger on creation', () => {
    expect(iii.registerTrigger).toHaveBeenCalledWith({
      type: 'state',
      function_id: 'ui::state-update',
      config: { scope: 'test-scope' },
    })
  })

  it('set() calls iii.trigger with state::set', () => {
    store.set('/count', 42)
    expect(iii.trigger).toHaveBeenCalledWith({
      function_id: 'state::set',
      payload: { scope: 'test-scope', key: '/count', value: 42 },
    })
  })

  it('_applyRemoteUpdate updates snapshot and notifies listeners', () => {
    const listener = vi.fn()
    store.subscribe(listener)

    ;(store as any)._applyRemoteUpdate('/name', 'Alice')

    expect(store.get('/name')).toBe('Alice')
    expect(listener).toHaveBeenCalledTimes(1)
  })

  it('getSnapshot returns new reference after remote update', () => {
    const snap1 = store.getSnapshot()
    ;(store as any)._applyRemoteUpdate('/x', 1)
    const snap2 = store.getSnapshot()
    expect(snap1).not.toBe(snap2)
  })

  it('subscribe returns unsubscribe function', () => {
    const listener = vi.fn()
    const unsub = store.subscribe(listener)
    ;(store as any)._applyRemoteUpdate('/a', 1)
    expect(listener).toHaveBeenCalledTimes(1)

    unsub()
    ;(store as any)._applyRemoteUpdate('/b', 2)
    expect(listener).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd client && npx vitest run src/__tests__/state-store.test.ts`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `state-store.ts`**

```typescript
import { getByPath, setByPath } from '@json-render/core'
import type { StateStore, StateModel } from '@json-render/core'

export interface IIIStateStoreExtended extends StateStore {
  _applyRemoteUpdate(key: string, value: unknown): void
}

export function createIIIStateStore(
  iii: { trigger: (req: any) => Promise<any>; registerTrigger: (input: any) => any },
  scope: string,
): IIIStateStoreExtended {
  let snapshot: StateModel = {}
  const listeners = new Set<() => void>()

  iii.registerTrigger({
    type: 'state',
    function_id: 'ui::state-update',
    config: { scope },
  })

  return {
    get: (path: string) => getByPath(snapshot, path),

    set: (path: string, value: unknown) => {
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
      return () => { listeners.delete(listener) }
    },

    _applyRemoteUpdate(key: string, value: unknown) {
      setByPath(snapshot as Record<string, unknown>, key, value)
      snapshot = { ...snapshot }
      listeners.forEach((fn) => fn())
    },
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd client && npx vitest run src/__tests__/state-store.test.ts`
Expected: All 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src/state-store.ts client/src/__tests__/state-store.test.ts
git commit -m "feat: implement IIIStateStore backed by iii state primitives"
```

---

### Task 7: Implement IIIActionRouter

**Files:**
- Create: `client/src/action-router.ts`
- Create: `client/src/__tests__/action-router.test.ts`

- [ ] **Step 1: Write failing test**

```typescript
import { describe, it, expect, vi } from 'vitest'
import { createIIIActionRouter } from '../action-router'

describe('IIIActionRouter', () => {
  it('routes namespaced actions to iii.trigger()', async () => {
    const iii = { trigger: vi.fn(async () => ({ result: 'ok' })) }
    const router = createIIIActionRouter(iii as any)

    const result = await router.dispatch('ml::analyze', { text: 'hello' })

    expect(iii.trigger).toHaveBeenCalledWith({
      function_id: 'ml::analyze',
      payload: { text: 'hello' },
    })
    expect(result).toEqual({ result: 'ok' })
  })

  it('returns null for non-namespaced actions (json-render handles them)', async () => {
    const iii = { trigger: vi.fn() }
    const router = createIIIActionRouter(iii as any)

    const result = await router.dispatch('setState', { statePath: '/x', value: 1 })

    expect(iii.trigger).not.toHaveBeenCalled()
    expect(result).toBeNull()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd client && npx vitest run src/__tests__/action-router.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement**

```typescript
export interface IIIActionRouter {
  dispatch(action: string, params: Record<string, unknown>): Promise<unknown>
}

export function createIIIActionRouter(
  iii: { trigger: (req: any) => Promise<any> },
): IIIActionRouter {
  return {
    async dispatch(action: string, params: Record<string, unknown>) {
      if (action.includes('::')) {
        return iii.trigger({
          function_id: action,
          payload: params,
        })
      }
      return null
    },
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd client && npx vitest run src/__tests__/action-router.test.ts`
Expected: All 2 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src/action-router.ts client/src/__tests__/action-router.test.ts
git commit -m "feat: implement IIIActionRouter for $trigger expression routing"
```

---

### Task 8: Implement Expression Resolver

**Files:**
- Create: `client/src/expressions.ts`
- Create: `client/src/__tests__/expressions.test.ts`

- [ ] **Step 1: Write failing test**

```typescript
import { describe, it, expect } from 'vitest'
import { resolveExpressions } from '../expressions'
import type { Spec } from '@json-render/core'

describe('resolveExpressions', () => {
  it('converts $stream to $state path with __stream__ prefix', () => {
    const spec: any = {
      root: 'main',
      elements: {
        main: {
          type: 'Metric',
          props: { value: { $stream: 'metrics/users/active' } },
          children: [],
        },
      },
    }

    const { resolved, streams } = resolveExpressions(spec)

    expect(resolved.elements.main.props.value).toEqual({ $state: '/__streams__/metrics/users/active' })
    expect(streams).toEqual([{ stream: 'metrics', group: 'users', item: 'active' }])
  })

  it('converts $sync to $bindState', () => {
    const spec: any = {
      root: 'main',
      elements: {
        main: {
          type: 'Input',
          props: { value: { $sync: '/filters/region' } },
          children: [],
        },
      },
    }

    const { resolved } = resolveExpressions(spec)

    expect(resolved.elements.main.props.value).toEqual({ $bindState: '/filters/region' })
  })

  it('converts $trigger to json-render on binding with namespaced action', () => {
    const spec: any = {
      root: 'main',
      elements: {
        main: {
          type: 'Button',
          props: { label: 'Go' },
          on: { press: { $trigger: 'ml::analyze', params: { x: 1 } } },
          children: [],
        },
      },
    }

    const { resolved } = resolveExpressions(spec)

    expect(resolved.elements.main.on.press).toEqual({ action: 'ml::analyze', params: { x: 1 } })
  })

  it('leaves plain specs untouched', () => {
    const spec: any = {
      root: 'main',
      elements: {
        main: { type: 'Card', props: { title: 'Hello' }, children: [] },
      },
    }

    const { resolved, streams } = resolveExpressions(spec)

    expect(resolved).toEqual(spec)
    expect(streams).toEqual([])
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd client && npx vitest run src/__tests__/expressions.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement**

```typescript
import { isStreamExpr, isSyncExpr, isTriggerExpr, isPushExpr } from './types'
import type { StreamExpression } from './types'

export interface StreamBinding {
  stream: string
  group: string
  item: string
}

export interface ResolveResult {
  resolved: any
  streams: StreamBinding[]
  pushSlots: string[]
}

function parseStreamPath(path: string): StreamBinding {
  const parts = path.split('/')
  return {
    stream: parts[0] ?? '',
    group: parts[1] ?? 'default',
    item: parts[2] ?? 'value',
  }
}

function resolveProps(props: Record<string, unknown>, streams: StreamBinding[]): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(props)) {
    if (isStreamExpr(value)) {
      const binding = parseStreamPath((value as StreamExpression).$stream)
      streams.push(binding)
      out[key] = { $state: `/__streams__/${(value as StreamExpression).$stream}` }
    } else if (isSyncExpr(value)) {
      out[key] = { $bindState: (value as any).$sync }
    } else {
      out[key] = value
    }
  }
  return out
}

export function resolveExpressions(spec: any): ResolveResult {
  const streams: StreamBinding[] = []
  const pushSlots: string[] = []

  const resolved = {
    root: spec.root,
    elements: {} as Record<string, any>,
  }

  for (const [id, element] of Object.entries(spec.elements as Record<string, any>)) {
    const el = { ...element }

    el.props = resolveProps(el.props ?? {}, streams)

    if (el.on) {
      const resolvedOn: Record<string, any> = {}
      for (const [event, binding] of Object.entries(el.on as Record<string, any>)) {
        if (isTriggerExpr(binding)) {
          resolvedOn[event] = { action: binding.$trigger, params: binding.params ?? {} }
        } else {
          resolvedOn[event] = binding
        }
      }
      el.on = resolvedOn
    }

    if (isPushExpr(el)) {
      pushSlots.push((el as any).$push)
    }

    resolved.elements[id] = el
  }

  return { resolved, streams, pushSlots }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd client && npx vitest run src/__tests__/expressions.test.ts`
Expected: All 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src/expressions.ts client/src/__tests__/expressions.test.ts
git commit -m "feat: implement expression resolver for $stream, $sync, $trigger, $push"
```

---

### Task 9: Implement createSpecForge Core

**Files:**
- Rewrite: `client/src/index.ts`
- Create: `client/src/__tests__/index.test.ts`

- [ ] **Step 1: Write failing test**

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createSpecForge } from '../index'

function createMockIII() {
  const functions: Map<string, Function> = new Map()
  return {
    registerFunction: vi.fn((input, handler) => {
      functions.set(input.id, handler)
      return { id: input.id, unregister: vi.fn() }
    }),
    registerTrigger: vi.fn(() => ({ unregister: vi.fn() })),
    trigger: vi.fn(async (req) => {
      if (req.function_id === 'spec-forge::generate') {
        return { spec: { root: 'main', elements: {} }, cached: false, generation_ms: 100, model: 'test' }
      }
      return {}
    }),
    _functions: functions,
  }
}

describe('createSpecForge', () => {
  it('registers ui:: functions on creation', () => {
    const iii = createMockIII()
    createSpecForge(iii as any, { catalog: { components: {} } })

    const fnIds = iii.registerFunction.mock.calls.map((c: any) => c[0].id)
    expect(fnIds).toContain('ui::render-patch')
    expect(fnIds).toContain('ui::state-update')
    expect(fnIds).toContain('ui::notification')
    expect(fnIds).toContain('ui::stream-update')
  })

  it('generate() calls iii.trigger with spec-forge::generate', async () => {
    const iii = createMockIII()
    const forge = createSpecForge(iii as any, { catalog: { components: {} } })

    const result = await forge.generate('A dashboard')

    expect(iii.trigger).toHaveBeenCalledWith(expect.objectContaining({
      function_id: 'spec-forge::generate',
    }))
    expect(result.spec).toBeDefined()
  })

  it('stream() calls iii.trigger with spec-forge::stream', async () => {
    const iii = createMockIII()
    const forge = createSpecForge(iii as any, { catalog: { components: {} } })

    await forge.stream('A dashboard')

    expect(iii.trigger).toHaveBeenCalledWith(expect.objectContaining({
      function_id: 'spec-forge::stream',
    }))
  })

  it('join() calls iii.trigger with spec-forge::join-session and registers state trigger', async () => {
    const iii = createMockIII()
    const forge = createSpecForge(iii as any, { catalog: { components: {} } })

    await forge.join('session-abc')

    expect(iii.trigger).toHaveBeenCalledWith(expect.objectContaining({
      function_id: 'spec-forge::join-session',
      payload: expect.objectContaining({ session_id: 'session-abc' }),
    }))
    expect(iii.registerTrigger).toHaveBeenCalledWith(expect.objectContaining({
      type: 'state',
      config: expect.objectContaining({ scope: 'session::session-abc' }),
    }))
  })

  it('shutdown() unregisters all functions and triggers', async () => {
    const iii = createMockIII()
    const forge = createSpecForge(iii as any, { catalog: { components: {} } })
    await forge.shutdown()

    const unregCalls = iii.registerFunction.mock.results
      .map((r: any) => r.value.unregister)
    expect(unregCalls.every((fn: any) => fn.mock.calls.length > 0)).toBe(true)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd client && npx vitest run src/__tests__/index.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement `client/src/index.ts`**

```typescript
import type { Spec } from '@json-render/core'
import { createIIIStateStore, type IIIStateStoreExtended } from './state-store'
import { createIIIActionRouter, type IIIActionRouter } from './action-router'
import { resolveExpressions } from './expressions'
import type { SpecForgeOptions, SpecForgeCatalog, PatchEvent, GenerateResult } from './types'

export type { SpecForgeOptions, SpecForgeCatalog, PatchEvent, GenerateResult } from './types'
export { createIIIStateStore } from './state-store'
export { createIIIActionRouter } from './action-router'
export { resolveExpressions } from './expressions'
export * from './types'

export interface SpecForge {
  generate(prompt: string, model?: string): Promise<GenerateResult>
  stream(prompt: string, model?: string): Promise<void>
  refine(prompt: string, currentSpec: Spec, model?: string): Promise<GenerateResult>
  validate(spec: Spec): Promise<{ valid: boolean; errors: string[] }>
  join(sessionId: string): Promise<void>
  leave(): Promise<void>
  stateStore: IIIStateStoreExtended
  actionRouter: IIIActionRouter
  shutdown(): Promise<void>
}

export function createSpecForge(
  iii: any,
  opts: SpecForgeOptions,
): SpecForge {
  const scope = opts.scope ?? 'spec-forge'
  const catalog = opts.catalog
  const model = opts.model ?? 'claude-sonnet-4-20250514'
  let sessionId: string | null = null

  const stateStore = createIIIStateStore(iii, scope)
  const actionRouter = createIIIActionRouter(iii)

  const refs: Array<{ unregister: () => void }> = []

  refs.push(iii.registerFunction(
    { id: 'ui::render-patch' },
    async (data: PatchEvent) => {
      opts.onPatch?.(data)
      return { applied: true }
    },
  ))

  refs.push(iii.registerFunction(
    { id: 'ui::state-update' },
    async (data: { scope: string; key: string; value: unknown }) => {
      stateStore._applyRemoteUpdate(data.key, data.value)
      opts.onStateChange?.(data.key, data.value)
      return { received: true }
    },
  ))

  refs.push(iii.registerFunction(
    { id: 'ui::notification' },
    async (data: { type: string; payload: unknown }) => {
      opts.onNotification?.(data)
      return { displayed: true }
    },
  ))

  refs.push(iii.registerFunction(
    { id: 'ui::stream-update' },
    async (data: { stream: string; group: string; item: string; value: unknown }) => {
      stateStore._applyRemoteUpdate(
        `/__streams__/${data.stream}/${data.group}/${data.item}`,
        data.value,
      )
      return { updated: true }
    },
  ))

  return {
    stateStore,
    actionRouter,

    async generate(prompt: string, mdl?: string) {
      return iii.trigger({
        function_id: 'spec-forge::generate',
        payload: { prompt, catalog, model: mdl ?? model, session_id: sessionId },
      })
    },

    async stream(prompt: string, mdl?: string) {
      return iii.trigger({
        function_id: 'spec-forge::stream',
        payload: { prompt, catalog, model: mdl ?? model, session_id: sessionId },
      })
    },

    async refine(prompt: string, currentSpec: Spec, mdl?: string) {
      return iii.trigger({
        function_id: 'spec-forge::refine',
        payload: { prompt, current_spec: currentSpec, catalog, model: mdl ?? model, session_id: sessionId },
      })
    },

    async validate(spec: Spec) {
      return iii.trigger({
        function_id: 'spec-forge::validate',
        payload: { spec, catalog },
      })
    },

    async join(sid: string) {
      sessionId = sid
      await iii.trigger({
        function_id: 'spec-forge::join-session',
        payload: { session_id: sid },
      })
      refs.push(iii.registerTrigger({
        type: 'state',
        function_id: 'ui::state-update',
        config: { scope: `session::${sid}` },
      }))
    },

    async leave() {
      if (!sessionId) return
      await iii.trigger({
        function_id: 'spec-forge::leave-session',
        payload: { session_id: sessionId },
      })
      sessionId = null
    },

    async shutdown() {
      if (sessionId) await this.leave()
      refs.forEach((r) => r.unregister())
      refs.length = 0
    },
  }
}
```

- [ ] **Step 4: Run tests**

Run: `cd client && npx vitest run src/__tests__/index.test.ts`
Expected: All 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add client/src/index.ts client/src/__tests__/index.test.ts
git commit -m "feat: implement createSpecForge with iii primitives"
```

---

### Task 10: Update client/package.json with New Dependencies

**Files:**
- Modify: `client/package.json`

- [ ] **Step 1: Update package.json**

```json
{
  "name": "@iii-hq/spec-forge",
  "version": "2.0.0",
  "type": "module",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "exports": {
    ".": {
      "import": "./dist/index.js",
      "types": "./dist/index.d.ts"
    }
  },
  "scripts": {
    "build": "tsc",
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "dependencies": {
    "@json-render/core": "latest",
    "iii-browser-sdk": "latest"
  },
  "devDependencies": {
    "typescript": "^5.5.0",
    "vitest": "^3.0.0"
  }
}
```

- [ ] **Step 2: Install deps and verify tests pass**

Run: `cd client && npm install && npm test`
Expected: All tests PASS.

- [ ] **Step 3: Commit**

```bash
git add client/package.json client/package-lock.json
git commit -m "feat: update client package with iii-browser-sdk and json-render deps"
```

---

### Task 11: Create React Package — Provider and Hooks

**Files:**
- Create: `react/package.json`
- Create: `react/tsconfig.json`
- Create: `react/src/provider.tsx`
- Create: `react/src/hooks.ts`
- Create: `react/src/renderer.tsx`
- Create: `react/src/index.tsx`

- [ ] **Step 1: Create `react/package.json`**

```json
{
  "name": "@iii-hq/spec-forge-react",
  "version": "2.0.0",
  "type": "module",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc",
    "test": "vitest run"
  },
  "dependencies": {
    "@iii-hq/spec-forge": "file:../client",
    "@json-render/react": "latest",
    "iii-browser-sdk": "latest"
  },
  "peerDependencies": {
    "react": "^18.0.0 || ^19.0.0"
  },
  "devDependencies": {
    "@types/react": "^19.0.0",
    "react": "^19.0.0",
    "typescript": "^5.5.0",
    "vitest": "^3.0.0",
    "@testing-library/react": "^16.0.0",
    "jsdom": "^25.0.0"
  }
}
```

- [ ] **Step 2: Create `react/src/provider.tsx`**

```tsx
import React, { createContext, useContext, useEffect, useMemo, useRef, useState } from 'react'
import { registerWorker } from 'iii-browser-sdk'
import { createSpecForge, type SpecForge, type SpecForgeCatalog } from '@iii-hq/spec-forge'
import type { Spec } from '@json-render/core'

interface SpecForgeContextValue {
  forge: SpecForge
  iii: any
  spec: Spec | null
  setSpec: (spec: Spec | null) => void
  status: 'idle' | 'generating' | 'streaming' | 'error'
  setStatus: (s: 'idle' | 'generating' | 'streaming' | 'error') => void
  patches: unknown[]
  addPatch: (p: unknown) => void
  clearPatches: () => void
}

const SpecForgeContext = createContext<SpecForgeContextValue | null>(null)

export function useSpecForgeContext() {
  const ctx = useContext(SpecForgeContext)
  if (!ctx) throw new Error('useSpecForgeContext must be used within SpecForgeProvider')
  return ctx
}

interface SpecForgeProviderProps {
  engineUrl: string
  catalog: SpecForgeCatalog
  children: React.ReactNode
  scope?: string
  model?: string
}

export function SpecForgeProvider({ engineUrl, catalog, children, scope, model }: SpecForgeProviderProps) {
  const [spec, setSpec] = useState<Spec | null>(null)
  const [status, setStatus] = useState<'idle' | 'generating' | 'streaming' | 'error'>('idle')
  const [patches, setPatchList] = useState<unknown[]>([])
  const iiiRef = useRef<any>(null)
  const forgeRef = useRef<SpecForge | null>(null)

  if (!iiiRef.current) {
    iiiRef.current = registerWorker(engineUrl)
  }

  if (!forgeRef.current) {
    forgeRef.current = createSpecForge(iiiRef.current, {
      catalog,
      scope,
      model,
      onPatch: (event) => {
        if (event.type === 'patch') {
          setPatchList((prev) => [...prev, event.patch])
        }
        if (event.type === 'done' && event.spec) {
          setSpec(event.spec as Spec)
          setStatus('idle')
        }
      },
      onStateChange: () => {},
    })
  }

  useEffect(() => {
    return () => { forgeRef.current?.shutdown() }
  }, [])

  const value = useMemo(() => ({
    forge: forgeRef.current!,
    iii: iiiRef.current!,
    spec,
    setSpec,
    status,
    setStatus,
    patches,
    addPatch: (p: unknown) => setPatchList((prev) => [...prev, p]),
    clearPatches: () => setPatchList([]),
  }), [spec, status, patches])

  return <SpecForgeContext.Provider value={value}>{children}</SpecForgeContext.Provider>
}
```

- [ ] **Step 3: Create `react/src/hooks.ts`**

```typescript
import { useCallback } from 'react'
import { useSpecForgeContext } from './provider'
import type { Spec } from '@json-render/core'

export function useSpecForge() {
  const { forge, spec, setSpec, status, setStatus, clearPatches } = useSpecForgeContext()

  const generate = useCallback(async (prompt: string) => {
    setStatus('generating')
    clearPatches()
    try {
      const result = await forge.generate(prompt)
      setSpec(result.spec as Spec)
      setStatus('idle')
      return result
    } catch {
      setStatus('error')
      throw new Error('Generate failed')
    }
  }, [forge, setSpec, setStatus, clearPatches])

  const stream = useCallback(async (prompt: string) => {
    setStatus('streaming')
    clearPatches()
    try {
      await forge.stream(prompt)
    } catch {
      setStatus('error')
    }
  }, [forge, setStatus, clearPatches])

  const refine = useCallback(async (prompt: string) => {
    if (!spec) throw new Error('No spec to refine')
    setStatus('generating')
    try {
      const result = await forge.refine(prompt, spec)
      setSpec(result.spec as Spec)
      setStatus('idle')
      return result
    } catch {
      setStatus('error')
      throw new Error('Refine failed')
    }
  }, [forge, spec, setSpec, setStatus])

  return { generate, stream, refine, spec, status }
}

export function useForgeStream() {
  const { patches, status } = useSpecForgeContext()
  return { patches, isStreaming: status === 'streaming' }
}

export function useForgeState(path: string) {
  const { forge } = useSpecForgeContext()
  const value = forge.stateStore.get(path)
  const set = useCallback(
    (v: unknown) => forge.stateStore.set(path, v),
    [forge, path],
  )
  return [value, set] as const
}

export function useForgeAction(name: string) {
  const { forge } = useSpecForgeContext()
  return useCallback(
    (params: Record<string, unknown> = {}) => forge.actionRouter.dispatch(name, params),
    [forge, name],
  )
}

export function useForgeSession() {
  const { forge } = useSpecForgeContext()
  return {
    join: useCallback((id: string) => forge.join(id), [forge]),
    leave: useCallback(() => forge.leave(), [forge]),
  }
}
```

- [ ] **Step 4: Create `react/src/renderer.tsx`**

```tsx
import React, { useMemo } from 'react'
import { Renderer as JsonRenderRenderer } from '@json-render/react'
import { resolveExpressions } from '@iii-hq/spec-forge'
import { useSpecForgeContext } from './provider'
import type { Spec } from '@json-render/core'

interface ForgeRendererProps {
  spec: Spec
  registry: any
}

export function Renderer({ spec, registry }: ForgeRendererProps) {
  const { iii } = useSpecForgeContext()

  const resolvedSpec = useMemo(() => {
    const { resolved } = resolveExpressions(spec)
    return resolved
  }, [spec])

  return <JsonRenderRenderer spec={resolvedSpec} registry={registry} />
}
```

- [ ] **Step 5: Create `react/src/index.tsx`**

```typescript
export { SpecForgeProvider, useSpecForgeContext } from './provider'
export { useSpecForge, useForgeStream, useForgeState, useForgeAction, useForgeSession } from './hooks'
export { Renderer } from './renderer'
```

- [ ] **Step 6: Install deps and verify build**

Run: `cd react && npm install && npx tsc --noEmit`
Expected: No type errors.

- [ ] **Step 7: Commit**

```bash
git add react/
git commit -m "feat: add @iii-hq/spec-forge-react with Provider, hooks, and Renderer"
```

---

### Task 12: Update Demo with Browser SDK

**Files:**
- Modify: `demo/index.html`

- [ ] **Step 1: Add a minimal browser-sdk demo section**

Add a `<script type="module">` block to the existing demo that shows the iii-browser-sdk path alongside the existing HTTP demo. This demonstrates both paths — users can see the primitive-based approach:

```html
<script type="module">
  // Browser SDK path (spec-forge v2)
  // import { registerWorker } from 'iii-browser-sdk'
  // const iii = registerWorker('ws://localhost:49135')
  // iii.registerFunction({ id: 'ui::render-patch' }, async (data) => { ... })
  // await iii.trigger({ function_id: 'spec-forge::stream', payload: { prompt, catalog } })
  
  // For now, demo uses HTTP path which still works via registered HTTP triggers
</script>
```

- [ ] **Step 2: Commit**

```bash
git add demo/index.html
git commit -m "docs: add browser-sdk usage comments to demo"
```

---

### Task 13: Update README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add v2 section to README**

Add after the existing Quick Start section:

```markdown
## v2: Browser SDK (iii-browser-sdk)

spec-forge is a standard iii worker. From any browser-sdk project, just trigger its functions:

\`\`\`typescript
import { registerWorker } from 'iii-browser-sdk'

const iii = registerWorker('ws://localhost:49135')

// Register a function to receive streaming patches
iii.registerFunction({ id: 'ui::render-patch' }, async (data) => {
  console.log('Patch:', data.patch)
  return { applied: true }
})

// Generate
const spec = await iii.trigger({
  function_id: 'spec-forge::generate',
  payload: { prompt: 'A sales dashboard', catalog: { components: { Card: { description: 'Card' } } } }
})

// Stream (patches push to ui::render-patch)
await iii.trigger({
  function_id: 'spec-forge::stream',
  payload: { prompt: 'A sales dashboard', catalog }
})
\`\`\`

### React (convenience wrapper)

\`\`\`tsx
import { SpecForgeProvider, useSpecForge, Renderer } from '@iii-hq/spec-forge-react'

<SpecForgeProvider engineUrl="ws://localhost:49135" catalog={catalog} registry={registry}>
  <App />
</SpecForgeProvider>
\`\`\`

### Collaborative Sessions

\`\`\`typescript
// Join — all browsers in session see each other's changes
await iii.trigger({ function_id: 'spec-forge::join-session', payload: { session_id: 'team-dashboard' } })

// Generate — patches fan out to all peers
await iii.trigger({ function_id: 'spec-forge::stream', payload: { prompt, catalog, session_id: 'team-dashboard' } })
\`\`\`

### New Expressions

| Expression | Resolves To | iii Primitive |
|-----------|-------------|---------------|
| `{ "$stream": "metrics/users/active" }` | Live data binding | `registerTrigger({ type: 'stream' })` |
| `{ "$trigger": "ml::analyze" }` | Server-side action | `iii.trigger({ function_id })` |
| `{ "$sync": "/filters/region" }` | Collaborative state | `iii.trigger({ function_id: 'state::set' })` |
| `{ "$push": "alerts" }` | Server-push slot | `registerFunction('ui::render-patch')` |
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add v2 browser-sdk usage and new expressions to README"
```
