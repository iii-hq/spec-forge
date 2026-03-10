import { readFileSync } from "fs";
import { createHash } from "crypto";

const data = JSON.parse(readFileSync(new URL("./data.json", import.meta.url), "utf8"));

// ---------------------------------------------------------------------------
// spec-forge TypeScript operations (matches Rust worker behavior)
// Source: src/main.rs, src/cache.rs, src/validate.rs, src/prompt.rs,
//         src/semantic.rs, src/limiter.rs, client/src/index.ts
// ---------------------------------------------------------------------------

// --- JSONL Patch Parser (from main.rs: parse_jsonl_patches + apply_patch) ---
function parseJsonlPatches(raw) {
  const patches = [];
  const spec = { root: "", elements: {} };

  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || !trimmed.startsWith("{")) continue;
    try {
      const patch = JSON.parse(trimmed);
      patches.push(patch);
      applyPatch(spec, patch);
    } catch {}
  }

  return { patches, spec };
}

function applyPatch(spec, patch) {
  const op = patch.op;
  const path = patch.path;

  if ((op === "add" || op === "replace") && path === "/root") {
    spec.root = patch.value;
    return;
  }
  if (path && path.startsWith("/elements/")) {
    const key = path.slice(10);
    if (op === "remove") {
      delete spec.elements[key];
    } else if (op === "add" || op === "replace") {
      spec.elements[key] = patch.value;
    }
  }
}

// --- WebSocket Channel Patch Receiver ---
// spec-forge streams individual patch messages over iii Channel WebSocket.
// Each message is already a complete JSON object (no line splitting needed).
function processChannelMessage(msg, spec) {
  const parsed = JSON.parse(msg);
  if (parsed.type === "patch") {
    applyPatch(spec, parsed.patch);
    return { type: "patch", patch: parsed.patch };
  }
  if (parsed.type === "done") {
    return { type: "done", spec: parsed.spec };
  }
  return parsed;
}

// --- SHA-256 Exact Cache (from cache.rs: SpecCache::cache_key) ---
function cacheKey(prompt, catalogJson) {
  const h = createHash("sha256");
  h.update(prompt);
  h.update("|");
  h.update(catalogJson);
  return "spec:" + h.digest("hex").slice(0, 32);
}

// --- SHA-256 Cache with TTL (from cache.rs: SpecCache) ---
class SpecCache {
  constructor(ttlMs = 300_000) {
    this.entries = new Map();
    this.ttlMs = ttlMs;
  }
  get(key) {
    const entry = this.entries.get(key);
    if (!entry) return null;
    if (Date.now() - entry.insertedAt > this.ttlMs) {
      this.entries.delete(key);
      return null;
    }
    return entry.spec;
  }
  set(key, spec) {
    this.entries.set(key, { spec, insertedAt: Date.now() });
  }
}

// --- TF-IDF Semantic Cache (from semantic.rs: SemanticCache) ---
class SemanticCache {
  constructor(threshold = 0.85) {
    this.threshold = threshold;
    this.entries = [];
    this.idfCache = new Map();
  }

  tokenize(text) {
    return text.toLowerCase().replace(/[^a-z0-9\s]/g, "").split(/\s+/).filter(Boolean);
  }

  termFrequency(tokens) {
    const tf = new Map();
    for (const t of tokens) tf.set(t, (tf.get(t) || 0) + 1);
    const len = tokens.length || 1;
    for (const [k, v] of tf) tf.set(k, v / len);
    return tf;
  }

  tfidfVector(tokens) {
    const tf = this.termFrequency(tokens);
    const vec = new Map();
    const n = this.entries.length + 1;
    for (const [term, freq] of tf) {
      let df = 1;
      for (const entry of this.entries) {
        if (entry.tokens.includes(term)) df++;
      }
      vec.set(term, freq * Math.log(n / df));
    }
    return vec;
  }

  cosineSimilarity(a, b) {
    let dot = 0, magA = 0, magB = 0;
    for (const [key, val] of a) {
      magA += val * val;
      if (b.has(key)) dot += val * b.get(key);
    }
    for (const [, val] of b) magB += val * val;
    const denom = Math.sqrt(magA) * Math.sqrt(magB);
    return denom === 0 ? 0 : dot / denom;
  }

  store(prompt, catalogHash, cacheKey) {
    const tokens = this.tokenize(prompt);
    this.entries.push({ prompt, catalogHash, cacheKey, tokens });
  }

  findSimilar(prompt, catalogHash) {
    const tokens = this.tokenize(prompt);
    const queryVec = this.tfidfVector(tokens);
    let bestKey = null;
    let bestScore = 0;

    for (const entry of this.entries) {
      if (entry.catalogHash !== catalogHash) continue;
      const entryVec = this.tfidfVector(entry.tokens);
      const score = this.cosineSimilarity(queryVec, entryVec);
      if (score > bestScore && score >= this.threshold) {
        bestScore = score;
        bestKey = entry.cacheKey;
      }
    }
    return bestKey;
  }
}

// --- Rate Limiter (from limiter.rs: token bucket + concurrency semaphore) ---
class RateLimiter {
  constructor(tokensPerMin = 60, maxConcurrent = 5) {
    this.tokens = tokensPerMin;
    this.maxTokens = tokensPerMin;
    this.lastRefill = Date.now();
    this.concurrent = 0;
    this.maxConcurrent = maxConcurrent;
    this.totalProcessed = 0;
    this.totalRejected = 0;
  }
  tryAcquire() {
    const now = Date.now();
    const elapsed = (now - this.lastRefill) / 60_000;
    this.tokens = Math.min(this.maxTokens, this.tokens + elapsed * this.maxTokens);
    this.lastRefill = now;

    if (this.tokens < 1 || this.concurrent >= this.maxConcurrent) {
      this.totalRejected++;
      return false;
    }
    this.tokens -= 1;
    this.concurrent++;
    this.totalProcessed++;
    return true;
  }
  release() {
    this.concurrent = Math.max(0, this.concurrent - 1);
  }
}

// --- Prompt Builder (from prompt.rs: build_prompt) ---
// spec-forge builds a more structured JSONL-focused prompt than json-render.
function buildPrompt(userPrompt, catalog) {
  let p = `You are a UI generator that outputs JSONL (one JSON object per line).

OUTPUT FORMAT (JSONL, RFC 6902 JSON Patch):
Output one JSON patch operation per line to build a UI spec progressively.
Each line MUST be a complete, valid JSON object. No markdown, no code fences, no explanation.

Start with the root, then add elements one at a time so the UI fills in progressively.

Example output (each line is a separate JSON object):

{"op":"add","path":"/root","value":"main"}
{"op":"add","path":"/elements/main","value":{"type":"Card","props":{"title":"Dashboard"},"children":["metric-1","chart"]}}
{"op":"add","path":"/elements/metric-1","value":{"type":"Metric","props":{"label":"Revenue","value":"$42K"},"children":[]}}
{"op":"add","path":"/elements/chart","value":{"type":"Card","props":{"title":"Sales"},"children":[]}}

`;

  p += "AVAILABLE COMPONENTS:\n";
  for (const [name, def] of Object.entries(catalog.components)) {
    let line = `- ${name}`;
    if (def.props) line += `: ${JSON.stringify(def.props)}`;
    line += ` - ${def.description}`;
    if (def.children) line += " [accepts children]";
    p += line + "\n";
  }

  if (catalog.actions && Object.keys(catalog.actions).length > 0) {
    p += "\nAVAILABLE ACTIONS:\n";
    for (const [name, def] of Object.entries(catalog.actions)) {
      p += `- ${name}: ${def.description}\n`;
    }
  }

  p += `
RULES:
1. Output ONLY JSONL patches — one JSON object per line, no markdown, no code fences, no text
2. First line MUST set root: {"op":"add","path":"/root","value":"<root-key>"}
3. Then add each element: {"op":"add","path":"/elements/<key>","value":{"type":"...","props":{...},"children":[...]}}
4. Use ONLY components listed above — never invent new types
5. Each element needs: type, props, children (array of child keys, empty [] for leaves)
6. Use unique, descriptive keys (e.g. "form-card", "email-input", "submit-btn")
7. Use layout components (Stack, Grid, Card) to group related elements
8. Props must match the component's defined props exactly
9. Generate 5-12 elements for a complete, well-structured UI
10. Text content should be realistic and specific
`;

  p += `\nUSER REQUEST: ${userPrompt}\n\nOutput JSONL patches now:\n`;
  return p;
}

// --- Spec Validation (from validate.rs: validate_spec) ---
function validateSpec(spec, catalog) {
  const errors = [];
  const componentNames = new Set(Object.keys(catalog.components));

  if (!spec.elements[spec.root]) {
    errors.push({ id: spec.root, message: `Root '${spec.root}' not found` });
    return errors;
  }

  for (const [id, el] of Object.entries(spec.elements)) {
    if (!componentNames.has(el.type)) {
      errors.push({ id, message: `Unknown type '${el.type}'` });
    }
    if (el.children) {
      for (const childId of el.children) {
        if (!spec.elements[childId]) {
          errors.push({ id, message: `Missing child '${childId}'` });
        }
      }
    }
  }

  const reachable = new Set();
  const stack = [spec.root];
  while (stack.length > 0) {
    const key = stack.pop();
    if (reachable.has(key)) continue;
    reachable.add(key);
    const el = spec.elements[key];
    if (el?.children) {
      for (const child of el.children) stack.push(child);
    }
  }

  for (const key of Object.keys(spec.elements)) {
    if (!reachable.has(key)) {
      errors.push({ id: key, message: "Orphaned element" });
    }
  }

  return errors;
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
console.log("║           spec-forge (iii-sdk) — JavaScript Benchmark          ║");
console.log("║           Rust worker operations reimplemented in JS           ║");
console.log("╚══════════════════════════════════════════════════════════════════╝\n");

// --- 1. JSONL Patch Parsing (server-side parse_jsonl_patches) ---
console.log("── JSONL Patch Parsing (parse_jsonl_patches) ──");

const jsonlTiny = data.jsonl_samples.tiny;
const jsonlSmall = data.jsonl_samples.small;

run("parse-patches-3", () => { parseJsonlPatches(jsonlTiny); }, 100_000);
run("parse-patches-9", () => { parseJsonlPatches(jsonlSmall); }, 50_000);

// WebSocket channel message processing (individual messages, no line splitting)
const channelMsgs3 = jsonlTiny.split("\n").filter(Boolean).map(line => {
  const patch = JSON.parse(line);
  return JSON.stringify({ type: "patch", patch });
});

run("channel-msg-3-patches", () => {
  const spec = { root: "", elements: {} };
  for (const msg of channelMsgs3) processChannelMessage(msg, spec);
}, 100_000);

const channelMsgs9 = jsonlSmall.split("\n").filter(Boolean).map(line => {
  const patch = JSON.parse(line);
  return JSON.stringify({ type: "patch", patch });
});

run("channel-msg-9-patches", () => {
  const spec = { root: "", elements: {} };
  for (const msg of channelMsgs9) processChannelMessage(msg, spec);
}, 50_000);

// --- 2. Prompt Generation (build_prompt) ---
console.log("\n── Prompt Generation (build_prompt — JSONL format) ──");

const catalogMinimal = data.catalogs.minimal;
const catalogDashboard = data.catalogs.dashboard;
const catalogForm = data.catalogs.form;

run("prompt-minimal-catalog", () => { buildPrompt(data.prompts.simple, catalogMinimal); }, 100_000);
run("prompt-dashboard-catalog", () => { buildPrompt(data.prompts.medium, catalogDashboard); }, 100_000);
run("prompt-form-catalog", () => { buildPrompt(data.prompts.complex, catalogForm); }, 100_000);

// --- 3. Spec Validation ---
console.log("\n── Spec Validation ──");

run("validate-3-elements", () => { validateSpec(data.specs.tiny, catalogMinimal); }, 100_000);
run("validate-9-elements", () => { validateSpec(data.specs.small, catalogDashboard); }, 100_000);

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

// --- 4. SHA-256 Cache Key ---
console.log("\n── SHA-256 Cache Key ──");

const catalogJson = JSON.stringify(catalogDashboard);
run("cache-key-sha256", () => { cacheKey(data.prompts.medium, catalogJson); }, 100_000);

// --- 5. Exact Cache Lookup ---
console.log("\n── Exact Cache (SHA-256 lookup + TTL) ──");

const cache = new SpecCache();
const testKey = cacheKey(data.prompts.medium, catalogJson);
cache.set(testKey, data.specs.small);

run("cache-hit", () => { cache.get(testKey); }, 500_000);
run("cache-miss", () => { cache.get("spec:0000000000000000000000000000"); }, 500_000);

// --- 6. TF-IDF Semantic Cache ---
console.log("\n── TF-IDF Semantic Cache ──");

const semCache = new SemanticCache(0.85);
const catalogHash = cacheKey("", catalogJson);

const seedPrompts = [
  "A sales dashboard with revenue metrics",
  "An admin panel with user management",
  "A login form with email and password",
  "An ecommerce product listing page",
  "A settings page with profile editing",
  "A chat interface with message history",
  "A project management kanban board",
  "A data table with sorting and filtering",
  "An analytics dashboard with charts",
  "A notification center with read/unread",
];

for (const p of seedPrompts) {
  semCache.store(p, catalogHash, cacheKey(p, catalogJson));
}

run("semantic-hit-10-entries", () => {
  semCache.findSimilar("A revenue dashboard for sales", catalogHash);
}, 50_000);

run("semantic-miss-10-entries", () => {
  semCache.findSimilar("A completely unrelated weather app", catalogHash);
}, 50_000);

// Scale to 100 entries
for (let i = 0; i < 90; i++) {
  semCache.store(`Generated prompt about topic ${i} with various details`, catalogHash, `key-${i}`);
}

run("semantic-hit-100-entries", () => {
  semCache.findSimilar("A revenue dashboard for sales", catalogHash);
}, 10_000);

run("semantic-miss-100-entries", () => {
  semCache.findSimilar("A completely unrelated weather app", catalogHash);
}, 10_000);

// --- 7. Rate Limiter ---
console.log("\n── Rate Limiter (token bucket + concurrency) ──");

run("rate-limiter-acquire", () => {
  const limiter = new RateLimiter(1_000_000, 1000);
  limiter.tryAcquire();
  limiter.release();
}, 500_000);

// --- 8. JSON Parse + Stringify ---
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

// --- 9. Full Pipeline ---
console.log("\n── Full Pipeline (cache check → parse → validate → store) ──");

run("pipeline-cached-hit", () => {
  const key = cacheKey(data.prompts.medium, catalogJson);
  const hit = cache.get(key);
  if (hit) return hit;
}, 500_000);

run("pipeline-cold-3-elements", () => {
  const key = cacheKey("unique-prompt-cold-3", catalogJson);
  const coldCache = new SpecCache();
  const hit = coldCache.get(key);
  if (!hit) {
    const { patches, spec } = parseJsonlPatches(jsonlTiny);
    validateSpec(spec, catalogMinimal);
    coldCache.set(key, spec);
  }
}, 50_000);

run("pipeline-cold-9-elements", () => {
  const key = cacheKey("unique-prompt-cold-9", catalogJson);
  const coldCache = new SpecCache();
  const hit = coldCache.get(key);
  if (!hit) {
    const { patches, spec } = parseJsonlPatches(jsonlSmall);
    validateSpec(spec, catalogDashboard);
    coldCache.set(key, spec);
  }
}, 20_000);

// --- 10. Sizes ---
console.log("\n── Payload Sizes ──");
console.log(`  Prompt (minimal catalog):   ${buildPrompt(data.prompts.simple, catalogMinimal).length} bytes`);
console.log(`  Prompt (dashboard catalog): ${buildPrompt(data.prompts.medium, catalogDashboard).length} bytes`);
console.log(`  Spec 3 elements:    ${jsonTiny.length} bytes`);
console.log(`  Spec 9 elements:    ${jsonSmall.length} bytes`);
console.log(`  Spec 50 elements:   ${json50.length} bytes`);
console.log(`  Spec 500 elements:  ${json500.length} bytes`);
console.log(`  Spec 2000 elements: ${json2000.length} bytes`);

// Output JSON for comparison script
console.log("\n__RESULTS_JSON__");
console.log(JSON.stringify(results));
