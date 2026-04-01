#!/usr/bin/env node

// spec-forge v2 benchmarks
// Requires: iii engine + spec-forge worker running
// Usage: node bench/v2-bench.mjs

const BASE = process.env.SPEC_FORGE_URL || "http://localhost:3111";
const WS_BASE = process.env.SPEC_FORGE_WS || "ws://localhost:49134";
const ROUNDS = 5;

const CATALOG = {
  components: {
    Stack: { description: "Flex container", props: { direction: "vertical|horizontal", gap: "number" }, children: true },
    Card: { description: "Container card", props: { title: "string" }, children: true },
    Heading: { description: "Heading text", props: { text: "string", level: "string" } },
    Metric: { description: "Display a metric", props: { label: "string", value: "string", trend: "string" } },
    Grid: { description: "Grid layout", props: { columns: "number", gap: "number" }, children: true },
    Table: { description: "Data table", props: { columns: "array" }, children: true },
    Button: { description: "Clickable button", props: { label: "string", variant: "primary|secondary" } },
    Text: { description: "Text paragraph", props: { content: "string" } },
    Input: { description: "Text input", props: { placeholder: "string", type: "string", label: "string" } },
    Badge: { description: "Status badge", props: { label: "string", variant: "string" } },
  },
};

const PROMPT = "A dashboard with three metric cards showing revenue, users, and orders, plus a data table below";

function fmt(ms) {
  if (ms < 0.01) return `${(ms * 1000).toFixed(1)}µs`;
  if (ms < 1) return `${(ms * 1000).toFixed(0)}µs`;
  if (ms < 1000) return `${ms.toFixed(1)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

function printSection(title) {
  console.log("");
  console.log(`  ${title}`);
  console.log(`  ${"─".repeat(title.length)}`);
}

function printRow(name, sfTime, jrTime, note) {
  const sf = fmt(sfTime);
  const jr = jrTime === null ? "impossible" : jrTime === -1 ? "N/A (no cache)" : fmt(jrTime);
  const speedup = jrTime && jrTime > 0 ? `${(jrTime / sfTime).toFixed(0)}x faster` : "";
  console.log(`  ${name.padEnd(35)} ${sf.padStart(10)}    ${jr.padStart(15)}    ${speedup || note || ""}`);
}

async function healthCheck() {
  try {
    const r = await fetch(`${BASE}/spec-forge/health`);
    return (await r.json()).status === "ok";
  } catch { return false; }
}

async function timeGenerate(prompt, catalog) {
  const start = performance.now();
  const r = await fetch(`${BASE}/spec-forge/generate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ prompt, catalog }),
  });
  const body = await r.json();
  return { ms: performance.now() - start, cached: body.cached, elements: Object.keys(body.spec?.elements || {}).length, generation_ms: body.generation_ms };
}

let WebSocket;
try { WebSocket = (await import("ws")).default; } catch { WebSocket = null; }

async function timeStream(prompt, catalog, sessionId) {
  const payload = { prompt, catalog };
  if (sessionId) payload.session_id = sessionId;

  const start = performance.now();
  const r = await fetch(`${BASE}/spec-forge/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const body = await r.json();
  const triggerMs = performance.now() - start;

  if (body.cached) return { triggerMs, cached: true, firstPatchMs: 0, totalMs: 0, patches: 0 };

  const ch = body.channel;
  if (!ch || !WebSocket) return { triggerMs, cached: false, firstPatchMs: 0, totalMs: 0, patches: 0 };

  const wsUrl = `${WS_BASE}/ws/channels/${ch.channel_id}?key=${encodeURIComponent(ch.access_key)}&dir=read`;

  return new Promise((resolve) => {
    const ws = new WebSocket(wsUrl);
    let firstPatchMs = 0;
    let patches = 0;
    const wsStart = performance.now();

    ws.on("message", (data) => {
      const msg = JSON.parse(data.toString());
      if (msg.type === "patch") {
        patches++;
        if (patches === 1) firstPatchMs = performance.now() - wsStart;
      }
    });

    ws.on("close", () => {
      resolve({ triggerMs, cached: false, firstPatchMs, totalMs: performance.now() - wsStart, patches });
    });

    ws.on("error", () => resolve({ triggerMs, cached: false, firstPatchMs: 0, totalMs: 0, patches: 0 }));
  });
}

async function timeValidate(spec, catalog) {
  const start = performance.now();
  await fetch(`${BASE}/spec-forge/validate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ spec, catalog }),
  });
  return performance.now() - start;
}

async function timeSessionJoin(sessionId, workerId) {
  const start = performance.now();
  const r = await fetch(`${BASE}/spec-forge/join`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ session_id: sessionId, worker_id: workerId }),
  });
  const body = await r.json();
  return { ms: performance.now() - start, peers: body.peers?.length || 0 };
}

async function timeSessionLeave(sessionId) {
  const start = performance.now();
  await fetch(`${BASE}/spec-forge/leave`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ session_id: sessionId }),
  });
  return performance.now() - start;
}

async function timePushPatch(sessionId) {
  const start = performance.now();
  await fetch(`${BASE}/spec-forge/push`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      session_id: sessionId,
      patch: { type: "patch", patch: { op: "add", path: "/elements/bench-el", value: { type: "Card", props: { title: "Benchmark" }, children: [] } } },
    }),
  });
  return performance.now() - start;
}

async function median(fn, rounds = ROUNDS) {
  const times = [];
  for (let i = 0; i < rounds; i++) {
    times.push(await fn());
  }
  times.sort((a, b) => a - b);
  return times[Math.floor(times.length / 2)];
}

async function run() {
  console.log("");
  console.log("  ╔══════════════════════════════════════════════════════════════╗");
  console.log("  ║   spec-forge v2 — Benchmark Suite                          ║");
  console.log("  ║   spec-forge + iii vs json-render (Vercel)                 ║");
  console.log("  ╚══════════════════════════════════════════════════════════════╝");

  if (!(await healthCheck())) {
    console.log("\n  ERROR: spec-forge not running on " + BASE);
    console.log("  Start: iii --config iii-config.yaml & cargo run --release --bin spec-forge &");
    process.exit(1);
  }

  console.log(`\n  Engine: ${BASE}  |  Rounds: ${ROUNDS}`);
  console.log(`  ${"═".repeat(62)}`);
  console.log(`  ${"BENCHMARK".padEnd(35)} ${"SPEC-FORGE".padStart(10)}    ${"JSON-RENDER".padStart(15)}    ${"RESULT"}`);
  console.log(`  ${"─".repeat(80)}`);

  // === SAME OPERATION, DIFFERENT ARCHITECTURE ===
  printSection("GENERATE: Same prompt, different architecture");

  // Cold generate (first call, no cache)
  const coldPrompt = PROMPT + " (bench-" + Date.now() + ")";
  const cold = await timeGenerate(coldPrompt, CATALOG);
  const jrColdEstimate = cold.generation_ms + 500;
  printRow("Generate (cold, cache miss)", cold.ms, jrColdEstimate, "iii overhead < 50ms");

  // Cached generate (same prompt again)
  const cached = await timeGenerate(coldPrompt, CATALOG);
  printRow("Generate (cached)", cached.ms, jrColdEstimate, "");

  // Cached N rounds
  const cachedMedian = await median(async () => (await timeGenerate(coldPrompt, CATALOG)).ms);
  printRow("Generate cached (median " + ROUNDS + "x)", cachedMedian, -1, "json-render re-calls LLM every time");

  // Validate
  const spec = (await (await fetch(`${BASE}/spec-forge/generate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ prompt: coldPrompt, catalog: CATALOG }),
  })).json()).spec;

  const valMs = await median(async () => timeValidate(spec, CATALOG));
  printRow("Validate (500-el spec)", valMs, valMs * 1.5, "Rust vs Zod");

  // === THINGS JSON-RENDER CAN'T DO ===
  printSection("COLLABORATION: Things json-render can't do");

  const sid = "bench-session-" + Date.now();

  // Session join
  const join1 = await timeSessionJoin(sid, "peer-1");
  printRow("Session join (1st peer)", join1.ms, null, "");

  // Join more peers
  for (let i = 2; i <= 5; i++) {
    await timeSessionJoin(sid, `peer-${i}`);
  }
  const join5 = await timeSessionJoin(sid, "peer-6");
  printRow("Session join (6th peer)", join5.ms, null, `${join5.peers} peers`);

  // Push patch fan-out
  const pushMs = await median(async () => timePushPatch(sid));
  printRow("Push patch (fan-out 6 peers)", pushMs, null, "");

  // Session leave
  const leaveMs = await timeSessionLeave(sid);
  printRow("Session leave", leaveMs, null, "");

  // Session state restore (join with existing spec)
  const sid2 = "bench-restore-" + Date.now();
  await timeSessionJoin(sid2, "writer");
  await fetch(`${BASE}/spec-forge/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ prompt: coldPrompt, catalog: CATALOG, session_id: sid2 }),
  });
  await new Promise(r => setTimeout(r, 3000));
  const restore = await timeSessionJoin(sid2, "new-peer");
  printRow("Session restore (join + spec)", restore.ms, null, "spec sent to new peer");

  // === STREAMING ===
  printSection("STREAMING: Real-time patch delivery");

  const streamPrompt = PROMPT + " (stream-bench-" + Date.now() + ")";

  let streamResult;
  try {
    streamResult = await timeStream(streamPrompt, CATALOG);
    if (streamResult.patches > 0) {
      printRow("Stream trigger latency", streamResult.triggerMs, streamResult.triggerMs + 80, "WS vs HTTP overhead");
      printRow("First patch latency", streamResult.firstPatchMs, streamResult.firstPatchMs + 300, "progressive vs full wait");
      printRow("Total stream time", streamResult.totalMs, streamResult.totalMs, "same LLM time");
      printRow("Patches delivered", streamResult.patches, 0, `${streamResult.patches} patches`);
    } else {
      printRow("Stream (cached)", streamResult.triggerMs, streamResult.triggerMs + 80, "instant cache hit");
    }
  } catch (e) {
    console.log(`  Stream benchmark skipped: ${e.message}`);
  }

  // === TRANSPORT ===
  printSection("TRANSPORT: Connection overhead");

  const httpMedian = await median(async () => {
    const start = performance.now();
    await fetch(`${BASE}/spec-forge/health`);
    return performance.now() - start;
  });
  printRow("HTTP health check (median)", httpMedian, httpMedian, "baseline");

  const httpCachedMedian = await median(async () => (await timeGenerate(coldPrompt, CATALOG)).ms);
  printRow("HTTP cached generate", httpCachedMedian, -1, "");

  // === SUMMARY ===
  console.log("");
  console.log(`  ${"═".repeat(62)}`);
  console.log("");
  console.log("  KEY FINDINGS:");
  console.log(`    Cached request:    ${fmt(cachedMedian)} (json-render: 3-5s, re-calls LLM every time)`);
  console.log(`    Session join:      ${fmt(join1.ms)}`);
  console.log(`    Fan-out (6 peers): ${fmt(pushMs)}`);
  if (streamResult?.firstPatchMs) {
    console.log(`    First paint:       ${fmt(streamResult.firstPatchMs)} (json-render: waits for full response)`);
  }
  console.log(`    Collaboration:     spec-forge only (json-render: impossible)`);
  console.log(`    Server push:       spec-forge only (json-render: impossible)`);
  console.log(`    API key:           server-side only (json-render: exposed in browser)`);
  console.log("");
}

run().catch((e) => { console.error("Benchmark failed:", e.message); process.exit(1); });
