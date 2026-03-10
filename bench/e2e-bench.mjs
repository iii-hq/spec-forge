import { readFileSync } from "fs";

const data = JSON.parse(readFileSync(new URL("./data.json", import.meta.url), "utf8"));

// ---------------------------------------------------------------------------
// End-to-end benchmark: measures real HTTP latency through the iii engine.
// Requires: iii engine running on :3111 + spec-forge worker connected.
//
// Usage:
//   # Start iii engine + worker first:
//   iii --config iii-config.yaml &
//   cargo run --release &
//
//   # Then run:
//   node bench/e2e-bench.mjs
// ---------------------------------------------------------------------------

const BASE_URL = process.env.SPEC_FORGE_URL || "http://localhost:3111";
const WS_URL = process.env.SPEC_FORGE_WS || "ws://localhost:49134";
const WARMUP_ROUNDS = 2;
const BENCH_ROUNDS = 10;

async function checkHealth() {
  try {
    const resp = await fetch(`${BASE_URL}/spec-forge/health`);
    const body = await resp.json();
    return body.status === "ok";
  } catch {
    return false;
  }
}

async function measureGenerate(prompt, catalog) {
  const start = performance.now();
  const resp = await fetch(`${BASE_URL}/spec-forge/generate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ prompt, catalog }),
  });
  const body = await resp.json();
  const elapsed = performance.now() - start;
  return { elapsed, cached: body.cached, generation_ms: body.generation_ms, elements: Object.keys(body.spec?.elements || {}).length };
}

async function measureValidate(spec, catalog) {
  const start = performance.now();
  const resp = await fetch(`${BASE_URL}/spec-forge/validate`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ spec, catalog }),
  });
  await resp.json();
  return performance.now() - start;
}

async function measureStream(prompt, catalog) {
  const start = performance.now();
  const resp = await fetch(`${BASE_URL}/spec-forge/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ prompt, catalog }),
  });
  const body = await resp.json();
  const httpTime = performance.now() - start;

  if (body.cached) {
    return { httpTime, cached: true, firstPatch: 0, totalPatches: 0, totalTime: httpTime };
  }

  // Connect to WebSocket channel and measure first patch + total time
  const channelId = body.channel?.channel_id;
  const accessKey = body.channel?.access_key;

  if (!channelId) {
    return { httpTime, cached: false, firstPatch: -1, totalPatches: 0, totalTime: httpTime, error: "No channel returned" };
  }

  return new Promise((resolve) => {
    const wsUrl = `${WS_URL}/ws/channels/${channelId}?key=${accessKey}&dir=read`;
    let firstPatchTime = 0;
    let patchCount = 0;
    let ws;

    try {
      ws = new WebSocket(wsUrl);
    } catch (e) {
      resolve({ httpTime, cached: false, firstPatch: -1, totalPatches: 0, totalTime: httpTime, error: `WebSocket failed: ${e.message}` });
      return;
    }

    const timeout = setTimeout(() => {
      ws.close();
      resolve({ httpTime, cached: false, firstPatch: firstPatchTime || -1, totalPatches: patchCount, totalTime: performance.now() - start, error: "Timeout" });
    }, 30_000);

    ws.onmessage = (event) => {
      const msg = JSON.parse(event.data);
      if (msg.type === "patch") {
        patchCount++;
        if (patchCount === 1) firstPatchTime = performance.now() - start;
      }
      if (msg.type === "done" || msg.type === "error") {
        clearTimeout(timeout);
        ws.close();
        resolve({
          httpTime,
          cached: false,
          firstPatch: firstPatchTime,
          totalPatches: patchCount,
          totalTime: performance.now() - start,
          error: msg.type === "error" ? msg.error : undefined,
        });
      }
    };

    ws.onerror = () => {
      clearTimeout(timeout);
      resolve({ httpTime, cached: false, firstPatch: -1, totalPatches: 0, totalTime: performance.now() - start, error: "WebSocket error" });
    };
  });
}

async function measureStats() {
  const start = performance.now();
  const resp = await fetch(`${BASE_URL}/spec-forge/stats`);
  await resp.json();
  return performance.now() - start;
}

async function measureHealth() {
  const start = performance.now();
  await fetch(`${BASE_URL}/spec-forge/health`);
  return performance.now() - start;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  console.log("╔══════════════════════════════════════════════════════════════════╗");
  console.log("║      spec-forge End-to-End Benchmark (through iii engine)      ║");
  console.log("╚══════════════════════════════════════════════════════════════════╝\n");

  const healthy = await checkHealth();
  if (!healthy) {
    console.log("  ERROR: spec-forge is not running on " + BASE_URL);
    console.log("  Start the iii engine and worker first:");
    console.log("");
    console.log("    iii --config iii-config.yaml &");
    console.log("    cargo run --release &");
    console.log("");
    console.log("  Then re-run: node bench/e2e-bench.mjs");
    process.exit(1);
  }

  console.log(`  Target: ${BASE_URL}`);
  console.log(`  WebSocket: ${WS_URL}`);
  console.log(`  Warmup: ${WARMUP_ROUNDS} rounds, Bench: ${BENCH_ROUNDS} rounds\n`);

  const catalog = data.catalogs.dashboard;

  // --- Health + Stats (lightweight endpoints) ---
  console.log("── Lightweight Endpoints ──");

  const healthTimes = [];
  for (let i = 0; i < 50; i++) healthTimes.push(await measureHealth());
  const healthAvg = healthTimes.reduce((a, b) => a + b) / healthTimes.length;
  const healthP50 = healthTimes.sort((a, b) => a - b)[Math.floor(healthTimes.length * 0.5)];
  const healthP99 = healthTimes.sort((a, b) => a - b)[Math.floor(healthTimes.length * 0.99)];
  console.log(`  /health:  avg=${healthAvg.toFixed(1)}ms  p50=${healthP50.toFixed(1)}ms  p99=${healthP99.toFixed(1)}ms  [50 reqs]`);

  const statsTimes = [];
  for (let i = 0; i < 50; i++) statsTimes.push(await measureStats());
  const statsAvg = statsTimes.reduce((a, b) => a + b) / statsTimes.length;
  const statsP50 = statsTimes.sort((a, b) => a - b)[Math.floor(statsTimes.length * 0.5)];
  console.log(`  /stats:   avg=${statsAvg.toFixed(1)}ms  p50=${statsP50.toFixed(1)}ms  [50 reqs]`);

  // --- Validate (no LLM call) ---
  console.log("\n── Validate Endpoint (no LLM) ──");

  const validateTimes = [];
  for (let i = 0; i < 50; i++) {
    validateTimes.push(await measureValidate(data.specs.small, catalog));
  }
  const valAvg = validateTimes.reduce((a, b) => a + b) / validateTimes.length;
  const valP50 = validateTimes.sort((a, b) => a - b)[Math.floor(validateTimes.length * 0.5)];
  console.log(`  /validate: avg=${valAvg.toFixed(1)}ms  p50=${valP50.toFixed(1)}ms  [50 reqs]`);

  // --- Generate (cold + warm) ---
  console.log("\n── Generate Endpoint (cold = LLM, warm = cached) ──");

  // Warmup
  for (let i = 0; i < WARMUP_ROUNDS; i++) {
    await measureGenerate(`warmup-${i}-${Date.now()}`, catalog);
  }

  // Cold (unique prompts force LLM call)
  const coldTimes = [];
  for (let i = 0; i < BENCH_ROUNDS; i++) {
    const uniquePrompt = `Benchmark dashboard variant ${i} with ${Date.now()} metrics`;
    const result = await measureGenerate(uniquePrompt, catalog);
    if (!result.cached) coldTimes.push(result);
    console.log(`    cold[${i}]: ${result.elapsed.toFixed(0)}ms total, ${result.generation_ms}ms LLM, ${result.elements} elements, cached=${result.cached}`);
  }

  if (coldTimes.length > 0) {
    const coldAvg = coldTimes.reduce((a, b) => a + b.elapsed, 0) / coldTimes.length;
    const coldLlmAvg = coldTimes.reduce((a, b) => a + b.generation_ms, 0) / coldTimes.length;
    console.log(`  Cold avg: ${coldAvg.toFixed(0)}ms total, ${coldLlmAvg.toFixed(0)}ms LLM`);
    console.log(`  Overhead (total - LLM): ${(coldAvg - coldLlmAvg).toFixed(0)}ms (iii dispatch + validate + cache store)`);
  }

  // Warm (repeat a prompt that's now cached)
  console.log("");
  const warmPrompt = data.prompts.medium;
  await measureGenerate(warmPrompt, catalog); // seed cache

  const warmTimes = [];
  for (let i = 0; i < 50; i++) {
    const result = await measureGenerate(warmPrompt, catalog);
    if (result.cached) warmTimes.push(result.elapsed);
  }

  if (warmTimes.length > 0) {
    const warmAvg = warmTimes.reduce((a, b) => a + b) / warmTimes.length;
    const warmP50 = warmTimes.sort((a, b) => a - b)[Math.floor(warmTimes.length * 0.5)];
    const warmP99 = warmTimes.sort((a, b) => a - b)[Math.floor(warmTimes.length * 0.99)];
    console.log(`  Warm (cached): avg=${warmAvg.toFixed(1)}ms  p50=${warmP50.toFixed(1)}ms  p99=${warmP99.toFixed(1)}ms  [${warmTimes.length} hits]`);
  }

  // --- Stream Endpoint ---
  console.log("\n── Stream Endpoint (WebSocket channel) ──");

  // Cold stream
  for (let i = 0; i < WARMUP_ROUNDS; i++) {
    await measureStream(`stream-warmup-${i}-${Date.now()}`, catalog);
  }

  for (let i = 0; i < Math.min(3, BENCH_ROUNDS); i++) {
    const uniquePrompt = `Stream benchmark ${i} with ${Date.now()}`;
    const result = await measureStream(uniquePrompt, catalog);
    console.log(`    stream[${i}]: first_patch=${result.firstPatch?.toFixed(0) || "—"}ms, patches=${result.totalPatches}, total=${result.totalTime.toFixed(0)}ms, http=${result.httpTime.toFixed(0)}ms${result.error ? `, error=${result.error}` : ""}`);
  }

  // Cached stream
  const cachedStreamPrompt = data.prompts.simple;
  await measureStream(cachedStreamPrompt, catalog);

  const cachedStreamTimes = [];
  for (let i = 0; i < 20; i++) {
    const result = await measureStream(cachedStreamPrompt, catalog);
    if (result.cached) cachedStreamTimes.push(result.totalTime);
  }

  if (cachedStreamTimes.length > 0) {
    const csAvg = cachedStreamTimes.reduce((a, b) => a + b) / cachedStreamTimes.length;
    console.log(`  Cached stream: avg=${csAvg.toFixed(1)}ms  [${cachedStreamTimes.length} hits] (returns spec directly, no WS needed)`);
  }

  // --- Summary ---
  console.log("\n── iii Engine Overhead Summary ──");
  console.log(`  Health roundtrip:     ~${healthAvg.toFixed(1)}ms (baseline HTTP overhead)`);
  console.log(`  Validate roundtrip:   ~${valAvg.toFixed(1)}ms (HTTP + function dispatch + validation)`);
  if (warmTimes.length > 0) {
    const warmAvg = warmTimes.reduce((a, b) => a + b) / warmTimes.length;
    console.log(`  Cached generate:      ~${warmAvg.toFixed(1)}ms (HTTP + SHA-256 lookup + response)`);
  }
  if (coldTimes.length > 0) {
    const coldAvg = coldTimes.reduce((a, b) => a + b.elapsed, 0) / coldTimes.length;
    const coldLlmAvg = coldTimes.reduce((a, b) => a + b.generation_ms, 0) / coldTimes.length;
    console.log(`  Cold generate:        ~${coldAvg.toFixed(0)}ms total (~${coldLlmAvg.toFixed(0)}ms LLM + ~${(coldAvg - coldLlmAvg).toFixed(0)}ms overhead)`);
  }
  console.log("");
  console.log("  json-render comparison: every request is a cold LLM call (no caching layer).");
  console.log("  On repeat requests, spec-forge is effectively instant while json-render");
  console.log("  re-invokes the LLM every time (3-5s per call).");
  console.log("");
}

main().catch(console.error);
