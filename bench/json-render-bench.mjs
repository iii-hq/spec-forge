import { readFileSync } from "fs";
import { createHash } from "crypto";

const data = JSON.parse(readFileSync(new URL("./data.json", import.meta.url), "utf8"));

// ---------------------------------------------------------------------------
// json-render core reimplementation (faithful to @json-render/core v0.12.0)
// Source: packages/core/src/types.ts, schema.ts, spec-validator.ts, prompt.ts
// ---------------------------------------------------------------------------

// --- createSpecStreamCompiler (from types.ts) ---
// json-render's main patch processing pipeline.
// Each chunk of text is buffered, split into lines, deduplicated, parsed as
// JSON patches, and applied to an accumulating spec object.
function createSpecStreamCompiler() {
  let result = { root: "", elements: {}, state: {} };
  let buffer = "";
  const seen = new Set();

  function applyPatch(patch) {
    const op = patch.op;
    const path = patch.path;
    if (!op || !path) return false;

    if (path === "/root") {
      if (op === "add" || op === "replace") result.root = patch.value;
      return true;
    }

    if (path.startsWith("/state/")) {
      const key = path.slice(7);
      if (op === "remove") {
        delete result.state[key];
      } else {
        result.state[key] = patch.value;
      }
      return true;
    }

    if (path.startsWith("/elements/")) {
      const key = path.slice(10);
      if (op === "remove") {
        delete result.elements[key];
      } else if (op === "add" || op === "replace") {
        result.elements[key] = patch.value;
      }
      return true;
    }

    return false;
  }

  return {
    push(text) {
      buffer += text;
      const newPatches = [];
      let idx;
      while ((idx = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, idx).trim();
        buffer = buffer.slice(idx + 1);
        if (!line || seen.has(line)) continue;
        seen.add(line);
        try {
          const patch = JSON.parse(line);
          if (applyPatch(patch)) newPatches.push(patch);
        } catch {}
      }
      if (newPatches.length > 0) {
        result = { ...result };
      }
      return { result, newPatches };
    },
    flush() {
      const remaining = buffer.trim();
      buffer = "";
      if (!remaining || seen.has(remaining)) return { result, newPatches: [] };
      seen.add(remaining);
      try {
        const patch = JSON.parse(remaining);
        if (applyPatch(patch)) {
          result = { ...result };
          return { result, newPatches: [patch] };
        }
      } catch {}
      return { result, newPatches: [] };
    },
    get current() { return result; },
  };
}

// --- catalogPrompt (from schema.ts) ---
// json-render's catalog.prompt() generates a system prompt that includes
// component definitions, built-in actions, default rules, and output format.
function catalogPrompt(catalog, options = {}) {
  const parts = [];

  parts.push("You are a UI generator that outputs structured JSON specifications.");
  parts.push("");
  parts.push("# Available Components");
  parts.push("");

  for (const [name, def] of Object.entries(catalog.components)) {
    let line = `## ${name}`;
    parts.push(line);
    parts.push(`Description: ${def.description}`);
    if (def.props && Object.keys(def.props).length > 0) {
      parts.push(`Props: ${JSON.stringify(def.props)}`);
    }
    if (def.children) {
      parts.push("Accepts children: yes");
    }
    parts.push("");
  }

  if (catalog.actions && Object.keys(catalog.actions).length > 0) {
    parts.push("# Available Actions");
    parts.push("");
    for (const [name, def] of Object.entries(catalog.actions)) {
      parts.push(`- **${name}**: ${def.description}`);
    }
    parts.push("");
  }

  // Built-in actions (from react/schema.ts)
  parts.push("# Built-in Actions");
  parts.push("- **setState**: Set a value in state");
  parts.push("- **pushState**: Push a value to a state array");
  parts.push("- **removeState**: Remove a value from state by index");
  parts.push("- **validateForm**: Validate all form fields");
  parts.push("");

  // Default rules (from react/schema.ts defaultRules)
  parts.push("# Rules");
  parts.push("1. Output ONLY valid JSONL patches — one JSON object per line");
  parts.push("2. First line MUST set root: {\"op\":\"add\",\"path\":\"/root\",\"value\":\"<key>\"}");
  parts.push("3. Then add elements: {\"op\":\"add\",\"path\":\"/elements/<key>\",\"value\":{...}}");
  parts.push("4. Use ONLY components from the catalog — never invent types");
  parts.push("5. Each element needs: type, props, children (array of child keys)");
  parts.push("6. Use unique descriptive keys (e.g. form-card, email-input)");
  parts.push("7. Use layout components to group related elements");
  parts.push("8. Props must match the component schema exactly");
  parts.push("9. Generate 5-15 elements for a complete UI");
  parts.push("10. Use realistic, specific content");
  parts.push("");

  parts.push("# Output Format");
  parts.push("Return ONLY valid JSONL. Each line is a JSON object with op, path, value.");
  parts.push("");

  if (options.userPrompt) {
    parts.push("# User Request");
    parts.push(options.userPrompt);
  }

  return parts.join("\n");
}

// --- specValidator (from spec-validator.ts) ---
// json-render's validation: structural checks + auto-fix for common AI errors.
// We implement the validation checks only (not auto-fix) for fair comparison.
function validateSpec(spec, catalog) {
  const errors = [];
  const componentNames = new Set(Object.keys(catalog.components));

  if (!spec.root) {
    errors.push({ id: "(root)", message: "Missing root" });
    return errors;
  }

  if (!spec.elements || !spec.elements[spec.root]) {
    errors.push({ id: spec.root, message: `Root '${spec.root}' not found in elements` });
    return errors;
  }

  for (const [id, el] of Object.entries(spec.elements)) {
    if (!el.type) {
      errors.push({ id, message: "Missing type" });
      continue;
    }
    if (!componentNames.has(el.type)) {
      errors.push({ id, message: `Unknown component '${el.type}'` });
    }
    if (el.children && Array.isArray(el.children)) {
      for (const childId of el.children) {
        if (!spec.elements[childId]) {
          errors.push({ id, message: `Missing child '${childId}'` });
        }
      }
    }
  }

  // Orphan detection (reachability from root)
  const reachable = new Set();
  const walk = (key) => {
    if (reachable.has(key)) return;
    reachable.add(key);
    const el = spec.elements[key];
    if (el?.children) {
      for (const child of el.children) walk(child);
    }
  };
  walk(spec.root);

  for (const key of Object.keys(spec.elements)) {
    if (!reachable.has(key)) {
      errors.push({ id: key, message: "Orphaned element" });
    }
  }

  return errors;
}

// --- Dynamic prop resolution (from props.ts) ---
// json-render resolves $state, $cond, $template, $computed expressions at render time.
// This is a hot path — called per-element per-render.
function resolveProps(props, state) {
  const resolved = {};
  for (const [key, value] of Object.entries(props)) {
    if (value && typeof value === "object" && !Array.isArray(value)) {
      if ("$state" in value) {
        resolved[key] = getByPath(state, value.$state);
      } else if ("$cond" in value) {
        const condVal = getByPath(state, value.$cond);
        resolved[key] = condVal ? value.$then : value.$else;
      } else if ("$template" in value) {
        resolved[key] = value.$template.replace(/\{\{(\w+(?:\.\w+)*)\}\}/g, (_, path) => {
          const v = getByPath(state, path);
          return v !== undefined ? String(v) : "";
        });
      } else if ("$computed" in value) {
        resolved[key] = `[computed:${value.$computed}]`;
      } else {
        resolved[key] = resolveProps(value, state);
      }
    } else {
      resolved[key] = value;
    }
  }
  return resolved;
}

function getByPath(obj, path) {
  const parts = path.split(".");
  let current = obj;
  for (const part of parts) {
    if (current == null) return undefined;
    current = current[part];
  }
  return current;
}

// --- immutableSetByPath (from state-store.ts) ---
// json-render's structural sharing state updates.
function immutableSetByPath(obj, path, value) {
  const parts = path.split(".");
  if (parts.length === 0) return obj;
  if (parts.length === 1) return { ...obj, [parts[0]]: value };
  const [head, ...rest] = parts;
  return { ...obj, [head]: immutableSetByPath(obj[head] ?? {}, rest.join("."), value) };
}

// ---------------------------------------------------------------------------
// Benchmark harness
// ---------------------------------------------------------------------------

function bench(name, fn, iterations) {
  for (let i = 0; i < Math.min(100, iterations); i++) fn();
  const start = performance.now();
  for (let i = 0; i < iterations; i++) fn();
  const elapsed = performance.now() - start;
  const perOp = (elapsed / iterations * 1000).toFixed(2);
  const opsPerSec = Math.round(iterations / (elapsed / 1000));
  return { name, us_per_op: parseFloat(perOp), ops_per_sec: opsPerSec, elapsed_ms: parseFloat(elapsed.toFixed(1)), iterations };
}

function printResult(r) {
  console.log(`  ${r.name}: ${r.us_per_op}µs/op (${r.ops_per_sec.toLocaleString()} ops/sec) [${r.iterations} iters, ${r.elapsed_ms}ms]`);
}

// ---------------------------------------------------------------------------
// Run benchmarks
// ---------------------------------------------------------------------------

const results = [];
function run(name, fn, iters) {
  const r = bench(name, fn, iters);
  printResult(r);
  results.push(r);
}

console.log("╔══════════════════════════════════════════════════════════════════╗");
console.log("║         json-render (Vercel Labs) — JavaScript Benchmark       ║");
console.log("║         @json-render/core v0.12.0 faithful reimplementation    ║");
console.log("╚══════════════════════════════════════════════════════════════════╝\n");

// --- 1. JSONL Patch Streaming (createSpecStreamCompiler) ---
console.log("── JSONL Patch Streaming (createSpecStreamCompiler) ──");

const jsonlTiny = data.jsonl_samples.tiny;
const jsonlSmall = data.jsonl_samples.small;

run("stream-compile-3-patches", () => {
  const compiler = createSpecStreamCompiler();
  compiler.push(jsonlTiny);
  compiler.flush();
}, 100_000);

run("stream-compile-9-patches", () => {
  const compiler = createSpecStreamCompiler();
  compiler.push(jsonlSmall);
  compiler.flush();
}, 50_000);

// Simulate token-by-token streaming (how AI SDK delivers text)
const tinyChars = jsonlTiny.split("");
run("stream-token-by-token-3-patches", () => {
  const compiler = createSpecStreamCompiler();
  for (const ch of tinyChars) compiler.push(ch);
  compiler.flush();
}, 10_000);

const smallChars = jsonlSmall.split("");
run("stream-token-by-token-9-patches", () => {
  const compiler = createSpecStreamCompiler();
  for (const ch of smallChars) compiler.push(ch);
  compiler.flush();
}, 5_000);

// Simulate chunked delivery (5-20 tokens per chunk, realistic SSE)
function chunkString(str, size) {
  const chunks = [];
  for (let i = 0; i < str.length; i += size) {
    chunks.push(str.slice(i, i + size));
  }
  return chunks;
}
const tinyChunks = chunkString(jsonlTiny, 15);
run("stream-chunked-3-patches", () => {
  const compiler = createSpecStreamCompiler();
  for (const chunk of tinyChunks) compiler.push(chunk);
  compiler.flush();
}, 50_000);

const smallChunks = chunkString(jsonlSmall, 15);
run("stream-chunked-9-patches", () => {
  const compiler = createSpecStreamCompiler();
  for (const chunk of smallChunks) compiler.push(chunk);
  compiler.flush();
}, 20_000);

// --- 2. Prompt Generation (catalog.prompt()) ---
console.log("\n── Prompt Generation (catalog.prompt()) ──");

const catalogMinimal = data.catalogs.minimal;
const catalogDashboard = data.catalogs.dashboard;
const catalogForm = data.catalogs.form;

run("prompt-minimal-catalog", () => {
  catalogPrompt(catalogMinimal, { userPrompt: data.prompts.simple });
}, 100_000);

run("prompt-dashboard-catalog", () => {
  catalogPrompt(catalogDashboard, { userPrompt: data.prompts.medium });
}, 100_000);

run("prompt-form-catalog", () => {
  catalogPrompt(catalogForm, { userPrompt: data.prompts.complex });
}, 100_000);

// --- 3. Spec Validation (specValidator) ---
console.log("\n── Spec Validation ──");

run("validate-3-elements", () => {
  validateSpec(data.specs.tiny, catalogMinimal);
}, 100_000);

run("validate-9-elements", () => {
  validateSpec(data.specs.small, catalogDashboard);
}, 100_000);

// Generated large specs for scaling comparison
function generateSpec(count, catalog) {
  const compNames = Object.keys(catalog.components);
  const elements = {};
  const rootChildren = [];
  for (let i = 0; i < count; i++) {
    const name = compNames[i % compNames.length];
    const id = `${name.toLowerCase()}-${i}`;
    elements[id] = { type: name, props: { label: `Item ${i}`, value: `${i}` }, children: [] };
    rootChildren.push(id);
  }
  elements["root-0"] = { type: "Card", props: { title: "Benchmark" }, children: rootChildren };
  return { root: "root-0", elements };
}

const spec50 = generateSpec(50, catalogDashboard);
const spec500 = generateSpec(500, catalogDashboard);
const spec2000 = generateSpec(2000, catalogDashboard);

run("validate-50-elements", () => { validateSpec(spec50, catalogDashboard); }, 50_000);
run("validate-500-elements", () => { validateSpec(spec500, catalogDashboard); }, 5_000);
run("validate-2000-elements", () => { validateSpec(spec2000, catalogDashboard); }, 1_000);

// --- 4. JSON Parse + Stringify ---
console.log("\n── JSON Parse + Stringify ──");

const jsonTiny = JSON.stringify(data.specs.tiny);
const jsonSmall = JSON.stringify(data.specs.small);
const json50 = JSON.stringify(spec50);
const json500 = JSON.stringify(spec500);
const json2000 = JSON.stringify(spec2000);

run("parse-3-elements", () => { JSON.parse(jsonTiny); }, 100_000);
run("parse-9-elements", () => { JSON.parse(jsonSmall); }, 100_000);
run("parse-50-elements", () => { JSON.parse(json50); }, 50_000);
run("parse-500-elements", () => { JSON.parse(json500); }, 5_000);
run("parse-2000-elements", () => { JSON.parse(json2000); }, 1_000);

run("stringify-3-elements", () => { JSON.stringify(data.specs.tiny); }, 100_000);
run("stringify-9-elements", () => { JSON.stringify(data.specs.small); }, 100_000);
run("stringify-50-elements", () => { JSON.stringify(spec50); }, 50_000);
run("stringify-500-elements", () => { JSON.stringify(spec500); }, 5_000);
run("stringify-2000-elements", () => { JSON.stringify(spec2000); }, 1_000);

// --- 5. Dynamic Prop Resolution ---
console.log("\n── Dynamic Prop Resolution ($state, $cond, $template) ──");

const sampleState = {
  user: { name: "Alice", email: "alice@example.com", role: "admin" },
  metrics: { revenue: 42000, users: 1234, orders: 89 },
  ui: { theme: "dark", sidebarOpen: true },
};

const staticProps = { label: "Revenue", value: "$42K", trend: "up" };
const dynamicProps = {
  label: { $state: "metrics.revenue" },
  userName: { $template: "Welcome, {{user.name}}!" },
  showAdmin: { $cond: "ui.sidebarOpen", $then: "visible", $else: "hidden" },
  theme: { $state: "ui.theme" },
};

run("resolve-static-props", () => { resolveProps(staticProps, sampleState); }, 500_000);
run("resolve-dynamic-props", () => { resolveProps(dynamicProps, sampleState); }, 200_000);

// --- 6. State Updates (immutableSetByPath) ---
console.log("\n── State Updates (structural sharing) ──");

run("state-set-shallow", () => { immutableSetByPath(sampleState, "ui.theme", "light"); }, 500_000);
run("state-set-deep", () => { immutableSetByPath(sampleState, "user.name", "Bob"); }, 500_000);

// --- 7. Full Pipeline (stream → validate → stringify) ---
console.log("\n── Full Pipeline (stream → validate → stringify) ──");

run("pipeline-3-elements", () => {
  const compiler = createSpecStreamCompiler();
  compiler.push(jsonlTiny);
  const { result } = compiler.flush();
  validateSpec(result, catalogMinimal);
  JSON.stringify(result);
}, 50_000);

run("pipeline-9-elements", () => {
  const compiler = createSpecStreamCompiler();
  compiler.push(jsonlSmall);
  const { result } = compiler.flush();
  validateSpec(result, catalogDashboard);
  JSON.stringify(result);
}, 20_000);

// --- 8. Caching (json-render has NONE) ---
console.log("\n── Caching (json-render has NO built-in cache) ──");
console.log("  (no-op — json-render makes a fresh LLM call on every request)");

// --- 9. Sizes ---
console.log("\n── Payload Sizes ──");
console.log(`  Prompt (minimal catalog):   ${catalogPrompt(catalogMinimal, { userPrompt: data.prompts.simple }).length} bytes`);
console.log(`  Prompt (dashboard catalog): ${catalogPrompt(catalogDashboard, { userPrompt: data.prompts.medium }).length} bytes`);
console.log(`  Spec 3 elements:    ${jsonTiny.length} bytes`);
console.log(`  Spec 9 elements:    ${jsonSmall.length} bytes`);
console.log(`  Spec 50 elements:   ${json50.length} bytes`);
console.log(`  Spec 500 elements:  ${json500.length} bytes`);
console.log(`  Spec 2000 elements: ${json2000.length} bytes`);

// Output JSON for comparison script
console.log("\n__RESULTS_JSON__");
console.log(JSON.stringify(results));
