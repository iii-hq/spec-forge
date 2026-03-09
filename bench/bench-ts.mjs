import { readFileSync } from "fs";
import { createHash } from "crypto";

const data = JSON.parse(readFileSync(new URL("./specs.json", import.meta.url), "utf8"));

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

function validateSpec(spec, catalog) {
  const errors = [];
  const compNames = new Set(Object.keys(catalog.components));
  if (!spec.elements[spec.root]) {
    errors.push(`Root '${spec.root}' not found`);
    return errors;
  }
  for (const [id, el] of Object.entries(spec.elements)) {
    if (!compNames.has(el.type)) errors.push(`[${id}] Unknown type '${el.type}'`);
    for (const child of el.children || []) {
      if (!spec.elements[child]) errors.push(`[${id}] Missing child '${child}'`);
    }
  }
  const reachable = new Set();
  const walk = (key) => { if (reachable.has(key)) return; reachable.add(key); const el = spec.elements[key]; if (el?.children) el.children.forEach(walk); };
  walk(spec.root);
  for (const key of Object.keys(spec.elements)) {
    if (!reachable.has(key)) errors.push(`[${key}] Orphaned`);
  }
  return errors;
}

function buildPrompt(userPrompt, catalog) {
  let p = "You are a UI generator. Generate a JSON spec using ONLY these components:\n\n## Available Components\n\n";
  for (const [name, def] of Object.entries(catalog.components)) {
    p += `### ${name}\nDescription: ${def.description}\nProps: ${JSON.stringify(def.props)}\n`;
    if (def.children) p += "Can have children: yes\n";
    p += "\n";
  }
  if (catalog.actions && Object.keys(catalog.actions).length > 0) {
    p += "## Available Actions\n\n";
    for (const [name, def] of Object.entries(catalog.actions)) {
      p += `- **${name}**: ${def.description}\n`;
    }
    p += "\n";
  }
  p += `## Output Format\n\nReturn ONLY valid JSON...\n\n## User Request\n\n${userPrompt}\n`;
  return p;
}

function cacheKey(prompt, catalogJson) {
  const h = createHash("sha256");
  h.update(prompt);
  h.update("|");
  h.update(catalogJson);
  return "spec:" + h.digest("hex").slice(0, 32);
}

function bench(name, fn, iterations) {
  // warmup
  for (let i = 0; i < Math.min(100, iterations); i++) fn();

  const start = performance.now();
  for (let i = 0; i < iterations; i++) fn();
  const elapsed = performance.now() - start;
  const perOp = (elapsed / iterations * 1000).toFixed(2); // microseconds
  const opsPerSec = Math.round(iterations / (elapsed / 1000));
  console.log(`  ${name}: ${perOp}µs/op (${opsPerSec.toLocaleString()} ops/sec) [${iterations} iters, ${elapsed.toFixed(1)}ms]`);
  return { name, us_per_op: parseFloat(perOp), ops_per_sec: opsPerSec };
}

console.log("=== json-render (TypeScript) Benchmark ===\n");

const smallSpec = data.small;
const medSpec = generateSpec(50, data.catalog);
const lgSpec = generateSpec(500, data.catalog);
const xlSpec = generateSpec(2000, data.catalog);
const catalogJson = JSON.stringify(data.catalog);

const results = [];

console.log("--- JSON Parse ---");
const smallJson = JSON.stringify(smallSpec);
const medJson = JSON.stringify(medSpec);
const lgJson = JSON.stringify(lgSpec);
const xlJson = JSON.stringify(xlSpec);
results.push(bench("parse-3-elements", () => JSON.parse(smallJson), 100000));
results.push(bench("parse-50-elements", () => JSON.parse(medJson), 50000));
results.push(bench("parse-500-elements", () => JSON.parse(lgJson), 5000));
results.push(bench("parse-2000-elements", () => JSON.parse(xlJson), 1000));

console.log("\n--- Validate ---");
results.push(bench("validate-3-elements", () => validateSpec(smallSpec, data.catalog), 100000));
results.push(bench("validate-50-elements", () => validateSpec(medSpec, data.catalog), 50000));
results.push(bench("validate-500-elements", () => validateSpec(lgSpec, data.catalog), 5000));
results.push(bench("validate-2000-elements", () => validateSpec(xlSpec, data.catalog), 1000));

console.log("\n--- Prompt Build ---");
results.push(bench("prompt-build", () => buildPrompt("Sales dashboard", data.catalog), 100000));

console.log("\n--- Cache Key (SHA-256) ---");
results.push(bench("cache-key", () => cacheKey("Sales dashboard", catalogJson), 100000));

console.log("\n--- JSON Stringify ---");
results.push(bench("stringify-3-elements", () => JSON.stringify(smallSpec), 100000));
results.push(bench("stringify-50-elements", () => JSON.stringify(medSpec), 50000));
results.push(bench("stringify-500-elements", () => JSON.stringify(lgSpec), 5000));
results.push(bench("stringify-2000-elements", () => JSON.stringify(xlSpec), 1000));

console.log("\n--- Full Pipeline (parse+validate+stringify) ---");
results.push(bench("pipeline-3", () => { const s = JSON.parse(smallJson); validateSpec(s, data.catalog); JSON.stringify(s); }, 100000));
results.push(bench("pipeline-50", () => { const s = JSON.parse(medJson); validateSpec(s, data.catalog); JSON.stringify(s); }, 20000));
results.push(bench("pipeline-500", () => { const s = JSON.parse(lgJson); validateSpec(s, data.catalog); JSON.stringify(s); }, 2000));
results.push(bench("pipeline-2000", () => { const s = JSON.parse(xlJson); validateSpec(s, data.catalog); JSON.stringify(s); }, 500));

console.log("\n" + JSON.stringify(results));
