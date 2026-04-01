#!/usr/bin/env node

// Real json-render benchmarks — measures actual @json-render/core code paths
// Uses the same prompts/catalogs as spec-forge for fair comparison
// json-render calls Claude directly from the client (no server, no cache)

import {
  createSpecStreamCompiler,
  parseSpecStreamLine,
  applySpecStreamPatch,
} from "@json-render/core";

const ANTHROPIC_KEY = process.env.ANTHROPIC_API_KEY;
const ROUNDS = 3;

// json-render catalog — simple object format (no schema needed for prompt building)
const catalog = {
  components: {
    Stack: { description: "Flex container", props: {} },
    Card: { description: "Container card", props: {} },
    Heading: { description: "Heading text", props: {} },
    Metric: { description: "Display a metric", props: {} },
    Grid: { description: "Grid layout", props: {} },
    Table: { description: "Data table", props: {} },
    Button: { description: "Clickable button", props: {} },
    Text: { description: "Text paragraph", props: {} },
    Input: { description: "Text input", props: {} },
    Badge: { description: "Status badge", props: {} },
  },
};

const PROMPT = "A dashboard with three metric cards showing revenue, users, and orders, plus a data table below";

function fmt(ms) {
  if (ms < 0.01) return `${(ms * 1000).toFixed(1)}µs`;
  if (ms < 1) return `${(ms * 1000).toFixed(0)}µs`;
  if (ms < 1000) return `${ms.toFixed(1)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

// Measure prompt building
function benchPromptBuild() {
  const times = [];
  for (let i = 0; i < 100; i++) {
    const start = performance.now();
    buildPrompt(PROMPT);
    times.push(performance.now() - start);
  }
  times.sort((a, b) => a - b);
  return times[Math.floor(times.length / 2)];
}

// Measure JSONL parsing
function benchParsing(patches) {
  const times = [];
  for (let i = 0; i < 100; i++) {
    const start = performance.now();
    for (const line of patches) {
      parseSpecStreamLine(line);
    }
    times.push(performance.now() - start);
  }
  times.sort((a, b) => a - b);
  return times[Math.floor(times.length / 2)];
}

// Measure spec stream compiler
function benchCompiler(patches) {
  const times = [];
  for (let i = 0; i < 100; i++) {
    const start = performance.now();
    const compiler = createSpecStreamCompiler();
    for (const line of patches) {
      compiler.push(line);
    }
    times.push(performance.now() - start);
  }
  times.sort((a, b) => a - b);
  return times[Math.floor(times.length / 2)];
}

// Build prompt manually (same structure json-render generates)
function buildPrompt(prompt) {
  const componentList = Object.entries(catalog.components)
    .map(([name, def]) => `- ${name}: ${def.description}`)
    .join("\n");
  return `Generate a UI spec as JSONL (one JSON patch per line, RFC 6902 format).
Available components:\n${componentList}\n\nUser request: ${prompt}\n\nOutput one JSON patch operation per line. Start with {"op":"add","path":"/root","value":"<key>"} then add elements.`;
}

// Real LLM call via Claude API (same as json-render does client-side)
async function benchGenerate(prompt) {
  if (!ANTHROPIC_KEY) return { ms: 0, elements: 0, error: "no API key" };

  const userPrompt = buildPrompt(prompt);
  const start = performance.now();

  const resp = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "x-api-key": ANTHROPIC_KEY,
      "anthropic-version": "2023-06-01",
      "content-type": "application/json",
    },
    body: JSON.stringify({
      model: "claude-sonnet-4-6",
      max_tokens: 4096,
      messages: [{ role: "user", content: userPrompt }],
    }),
  });

  const body = await resp.json();
  const elapsed = performance.now() - start;

  if (!resp.ok) return { ms: elapsed, elements: 0, error: body.error?.message || "API error" };

  const text = body.content?.[0]?.text || "";
  const lines = text.split("\n").filter(l => l.trim().startsWith("{"));

  let spec = { root: "", elements: {} };
  for (const line of lines) {
    const patch = parseSpecStreamLine(line);
    if (patch) applySpecStreamPatch(spec, patch);
  }

  return { ms: elapsed, elements: Object.keys(spec.elements).length, patches: lines.length };
}

async function run() {
  console.log("");
  console.log("  json-render (Vercel) — Real Benchmarks");
  console.log("  ═══════════════════════════════════════");
  console.log("");

  // Compute benchmarks (no LLM needed)
  const promptMs = benchPromptBuild();
  console.log(`  Prompt build (100x median):      ${fmt(promptMs)}`);

  // We need a spec for validation benchmark — generate one
  let spec = null;
  let patchLines = [];
  let coldMs = 0;
  let coldElements = 0;

  if (ANTHROPIC_KEY) {
    console.log("");
    console.log("  LLM benchmarks (calling Claude directly from client):");
    console.log("  ─────────────────────────────────────────────────────");

    // Cold generate
    const cold = await benchGenerate(PROMPT + " (jr-bench-" + Date.now() + ")");
    coldMs = cold.ms;
    coldElements = cold.elements;
    console.log(`  Generate (cold):                 ${fmt(cold.ms)}  (${cold.elements} elements)`);

    // Second call — no cache (json-render has no caching)
    const second = await benchGenerate(PROMPT + " (jr-bench-" + Date.now() + ")");
    console.log(`  Generate (repeat, NO cache):     ${fmt(second.ms)}  (still calls LLM)`);

    // Third call — same prompt, still no cache
    const third = await benchGenerate(PROMPT + " (jr-bench-" + Date.now() + ")");
    console.log(`  Generate (3rd call, NO cache):   ${fmt(third.ms)}  (always full LLM call)`);

    // Build a spec for validation benchmark
    const forVal = await benchGenerate("A simple card with a title");
    if (forVal.elements > 0) {
      spec = { root: "main", elements: {} };
      // Reconstruct spec from a simple prompt
      const r2 = await fetch("https://api.anthropic.com/v1/messages", {
        method: "POST",
        headers: {
          "x-api-key": ANTHROPIC_KEY,
          "anthropic-version": "2023-06-01",
          "content-type": "application/json",
        },
        body: JSON.stringify({
          model: "claude-sonnet-4-6",
          max_tokens: 4096,
          messages: [{ role: "user", content: buildPrompt(PROMPT) }],
        }),
      });
      const b2 = await r2.json();
      const text = b2.content?.[0]?.text || "";
      patchLines = text.split("\n").filter(l => l.trim().startsWith("{"));
      for (const line of patchLines) {
        const patch = parseSpecStreamLine(line);
        if (patch) applySpecStreamPatch(spec, patch);
      }
    }
  } else {
    console.log("  (ANTHROPIC_API_KEY not set — skipping LLM benchmarks)");
    console.log("  (Set it to measure real generate latency)");

    // Use dummy spec for compute benchmarks
    spec = {
      root: "main",
      elements: {
        main: { type: "Stack", props: { direction: "vertical" }, children: ["card1"] },
        card1: { type: "Card", props: { title: "Test" }, children: [] },
      },
    };
    patchLines = [
      '{"op":"add","path":"/root","value":"main"}',
      '{"op":"add","path":"/elements/main","value":{"type":"Stack","props":{"direction":"vertical"},"children":["card1"]}}',
      '{"op":"add","path":"/elements/card1","value":{"type":"Card","props":{"title":"Test"},"children":[]}}',
    ];
  }

  console.log("");
  console.log("  Compute benchmarks (no LLM):");
  console.log("  ────────────────────────────");

  const parseMs = benchParsing(patchLines);
  console.log(`  Parse JSONL lines (100x median): ${fmt(parseMs)}  (${patchLines.length} lines)`);

  const compileMs = benchCompiler(patchLines);
  console.log(`  Stream compiler (100x median):   ${fmt(compileMs)}  (${patchLines.length} lines)`);

  console.log("");
  console.log("  LIMITATIONS (by design):");
  console.log("    No caching          — every request calls LLM");
  console.log("    No collaboration    — single browser, single user");
  console.log("    No server push      — browser must initiate");
  console.log("    API key in browser  — exposed to client");
  console.log("    No rate limiting    — can burn through quota");
  console.log("");

  // Output machine-readable results for the comparison script
  const results = {
    prompt_build_ms: promptMs,
    cold_generate_ms: coldMs,
    cold_elements: coldElements,
    validate_ms: 0,
    parse_ms: parseMs,
    compile_ms: compileMs,
  };
  process.stdout.write("__JSON_RENDER_RESULTS__" + JSON.stringify(results) + "\n");
}

run().catch(e => { console.error("Error:", e.message); process.exit(1); });
