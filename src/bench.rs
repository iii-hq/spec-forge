#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;
use std::time::Instant;

mod cache;
mod prompt;
mod semantic;
mod types;
mod validate;

use cache::SpecCache;
use semantic::SemanticCache;
use types::*;

fn generate_spec(count: usize, catalog: &Catalog) -> UISpec {
    let comp_names: Vec<&String> = catalog.components.keys().collect();
    let mut elements = HashMap::new();
    let mut root_children = Vec::new();
    for i in 0..count {
        let name = &comp_names[i % comp_names.len()];
        let id = format!("{}-{}", name.to_lowercase(), i);
        elements.insert(
            id.clone(),
            UIElement {
                element_type: (*name).clone(),
                props: serde_json::json!({"label": format!("Item {}", i), "value": format!("{}", i)}),
                children: vec![],
            },
        );
        root_children.push(id);
    }
    elements.insert(
        "root-0".into(),
        UIElement {
            element_type: "Card".into(),
            props: serde_json::json!({"title": "Benchmark"}),
            children: root_children,
        },
    );
    UISpec {
        root: "root-0".into(),
        elements,
    }
}

fn make_dashboard_catalog() -> Catalog {
    let mut components = BTreeMap::new();
    for (name, desc, children) in [
        ("Stack", "Flex container", true),
        ("Card", "Container card", true),
        ("Grid", "Grid layout", true),
        ("Heading", "Section heading", false),
        ("Metric", "Display a KPI metric", false),
        ("Table", "Data table", false),
        ("Chart", "Chart visualization", false),
        ("Button", "Clickable button", false),
        ("Text", "Text paragraph", false),
        ("Badge", "Status badge", false),
        ("Divider", "Visual separator", false),
        ("Input", "Text input field", false),
    ] {
        components.insert(
            name.to_string(),
            ComponentDef {
                description: desc.to_string(),
                props: serde_json::json!({"label": "string", "value": "string"}),
                children,
            },
        );
    }
    let mut actions = BTreeMap::new();
    for (name, desc) in [
        ("export_report", "Export dashboard to PDF"),
        ("refresh_data", "Refresh all data"),
        ("save", "Save changes"),
        ("filter", "Apply data filter"),
    ] {
        actions.insert(name.to_string(), ActionDef { description: desc.to_string() });
    }
    Catalog { components, actions }
}

// Single source of truth: bench/data.json (shared with JS benchmarks)
const BENCH_DATA: &str = include_str!("../bench/data.json");

fn load_jsonl_samples() -> (String, String) {
    let data: serde_json::Value = serde_json::from_str(BENCH_DATA).unwrap();
    let tiny = data["jsonl_samples"]["tiny"].as_str().unwrap().to_string();
    let small = data["jsonl_samples"]["small"].as_str().unwrap().to_string();
    (tiny, small)
}

fn parse_jsonl_patches(raw: &str) -> (Vec<serde_json::Value>, UISpec) {
    let mut patches = Vec::new();
    let mut spec = UISpec {
        root: String::new(),
        elements: HashMap::new(),
    };

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(patch) = serde_json::from_str::<serde_json::Value>(line) {
            let op = patch["op"].as_str().unwrap_or("");
            let path = patch["path"].as_str().unwrap_or("");

            match (op, path) {
                ("add" | "replace", "/root") => {
                    if let Some(val) = patch["value"].as_str() {
                        spec.root = val.to_string();
                    }
                }
                ("add" | "replace", p) if p.starts_with("/elements/") => {
                    let key = &p[10..];
                    if !key.is_empty() {
                        if let Ok(el) = serde_json::from_value::<UIElement>(patch["value"].clone()) {
                            spec.elements.insert(key.to_string(), el);
                        }
                    }
                }
                ("remove", p) if p.starts_with("/elements/") => {
                    let key = &p[10..];
                    spec.elements.remove(key);
                }
                _ => {}
            }

            patches.push(patch);
        }
    }

    (patches, spec)
}

fn bench<T>(name: &str, iterations: usize, mut f: impl FnMut() -> T) {
    for _ in 0..std::cmp::min(100, iterations) {
        black_box(f());
    }

    let start = Instant::now();
    for _ in 0..iterations {
        black_box(f());
    }
    let elapsed = start.elapsed();
    let us_per_op = elapsed.as_micros() as f64 / iterations as f64;
    let ops_per_sec = (iterations as f64 / elapsed.as_secs_f64()) as u64;
    println!(
        "  {}: {:.2}µs/op ({} ops/sec) [{} iters, {:.1}ms]",
        name,
        us_per_op,
        ops_per_sec,
        iterations,
        elapsed.as_millis()
    );
}

fn main() {
    let catalog = make_dashboard_catalog();

    let small_spec = generate_spec(3, &catalog);
    let spec9 = generate_spec(9, &catalog);
    let med_spec = generate_spec(50, &catalog);
    let lg_spec = generate_spec(500, &catalog);
    let xl_spec = generate_spec(2000, &catalog);

    let small_json = serde_json::to_string(&small_spec).unwrap();
    let json9 = serde_json::to_string(&spec9).unwrap();
    let med_json = serde_json::to_string(&med_spec).unwrap();
    let lg_json = serde_json::to_string(&lg_spec).unwrap();
    let xl_json = serde_json::to_string(&xl_spec).unwrap();

    let catalog_json = serde_json::to_string(&catalog).unwrap();

    let (jsonl_tiny, jsonl_small) = load_jsonl_samples();

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║            spec-forge (Rust) Benchmark — with black_box        ║");
    println!("║            Prevents dead code elimination in --release          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // --- 1. JSONL Patch Parsing ---
    println!("── JSONL Patch Parsing ──");
    bench("parse-patches-3", 100_000, || {
        parse_jsonl_patches(&jsonl_tiny)
    });
    bench("parse-patches-9", 50_000, || {
        parse_jsonl_patches(&jsonl_small)
    });

    // --- 2. JSON Parse (serde) ---
    println!("\n── JSON Parse (serde_json::from_str) ──");
    bench("parse-3-elements", 100_000, || -> UISpec {
        serde_json::from_str(&small_json).unwrap()
    });
    bench("parse-9-elements", 100_000, || -> UISpec {
        serde_json::from_str(&json9).unwrap()
    });
    bench("parse-50-elements", 50_000, || -> UISpec {
        serde_json::from_str(&med_json).unwrap()
    });
    bench("parse-500-elements", 5_000, || -> UISpec {
        serde_json::from_str(&lg_json).unwrap()
    });
    bench("parse-2000-elements", 1_000, || -> UISpec {
        serde_json::from_str(&xl_json).unwrap()
    });

    // --- 3. Validate ---
    println!("\n── Validate ──");
    bench("validate-3-elements", 100_000, || {
        validate::validate_spec(&small_spec, &catalog)
    });
    bench("validate-9-elements", 100_000, || {
        validate::validate_spec(&spec9, &catalog)
    });
    bench("validate-50-elements", 50_000, || {
        validate::validate_spec(&med_spec, &catalog)
    });
    bench("validate-500-elements", 5_000, || {
        validate::validate_spec(&lg_spec, &catalog)
    });
    bench("validate-2000-elements", 1_000, || {
        validate::validate_spec(&xl_spec, &catalog)
    });

    // --- 4. Prompt Build ---
    println!("\n── Prompt Build ──");
    bench("prompt-simple", 100_000, || {
        prompt::build_prompt("A login form with email and password", &catalog)
    });
    bench("prompt-medium", 100_000, || {
        prompt::build_prompt(
            "A sales dashboard showing revenue metrics, a line chart of monthly sales, and a table of recent orders",
            &catalog,
        )
    });
    bench("prompt-complex", 100_000, || {
        prompt::build_prompt(
            "An admin panel with sidebar navigation, header with user profile, grid of metric cards, sales chart, orders table with sorting, and export buttons",
            &catalog,
        )
    });

    // --- 5. Cache Key (SHA-256) ---
    println!("\n── Cache Key (SHA-256) ──");
    bench("cache-key-sha256", 100_000, || {
        SpecCache::cache_key("A sales dashboard with revenue metrics", &catalog_json)
    });

    // --- 6. Exact Cache Lookup ---
    println!("\n── Exact Cache (SHA-256 lookup + TTL) ──");
    let cache = SpecCache::new(std::time::Duration::from_secs(300));
    let test_key = SpecCache::cache_key("A sales dashboard with revenue metrics", &catalog_json);
    cache.set(test_key.clone(), small_spec.clone());

    bench("cache-hit", 500_000, || {
        cache.get(&test_key)
    });
    bench("cache-miss", 500_000, || {
        cache.get("spec:00000000000000000000000000000000")
    });

    // --- 7. TF-IDF Semantic Cache ---
    println!("\n── TF-IDF Semantic Cache ──");
    let sem_cache = SemanticCache::new(0.85);
    let catalog_hash = SpecCache::cache_key("", &catalog_json);

    let seed_prompts = [
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

    for p in &seed_prompts {
        let key = SpecCache::cache_key(p, &catalog_json);
        sem_cache.store(p, &catalog_hash, key);
    }

    bench("semantic-hit-10-entries", 50_000, || {
        sem_cache.find_similar("A revenue dashboard for sales", &catalog_hash)
    });
    bench("semantic-miss-10-entries", 50_000, || {
        sem_cache.find_similar("A completely unrelated weather app", &catalog_hash)
    });

    for i in 0..90 {
        let p = format!("Generated prompt about topic {} with various details", i);
        sem_cache.store(&p, &catalog_hash, format!("key-{}", i));
    }

    bench("semantic-hit-100-entries", 10_000, || {
        sem_cache.find_similar("A revenue dashboard for sales", &catalog_hash)
    });
    bench("semantic-miss-100-entries", 10_000, || {
        sem_cache.find_similar("A completely unrelated weather app", &catalog_hash)
    });

    // --- 8. JSON Stringify (serde) ---
    println!("\n── JSON Stringify (serde_json::to_string) ──");
    bench("stringify-3-elements", 100_000, || {
        serde_json::to_string(&small_spec).unwrap()
    });
    bench("stringify-9-elements", 100_000, || {
        serde_json::to_string(&spec9).unwrap()
    });
    bench("stringify-50-elements", 50_000, || {
        serde_json::to_string(&med_spec).unwrap()
    });
    bench("stringify-500-elements", 5_000, || {
        serde_json::to_string(&lg_spec).unwrap()
    });
    bench("stringify-2000-elements", 1_000, || {
        serde_json::to_string(&xl_spec).unwrap()
    });

    // --- 9. Full Pipeline ---
    println!("\n── Full Pipeline (parse + validate + stringify) ──");
    bench("pipeline-3", 100_000, || {
        let s: UISpec = serde_json::from_str(&small_json).unwrap();
        let errors = validate::validate_spec(&s, &catalog);
        let out = serde_json::to_string(&s).unwrap();
        (errors, out)
    });
    bench("pipeline-9", 50_000, || {
        let s: UISpec = serde_json::from_str(&json9).unwrap();
        let errors = validate::validate_spec(&s, &catalog);
        let out = serde_json::to_string(&s).unwrap();
        (errors, out)
    });
    bench("pipeline-50", 20_000, || {
        let s: UISpec = serde_json::from_str(&med_json).unwrap();
        let errors = validate::validate_spec(&s, &catalog);
        let out = serde_json::to_string(&s).unwrap();
        (errors, out)
    });
    bench("pipeline-500", 2_000, || {
        let s: UISpec = serde_json::from_str(&lg_json).unwrap();
        let errors = validate::validate_spec(&s, &catalog);
        let out = serde_json::to_string(&s).unwrap();
        (errors, out)
    });
    bench("pipeline-2000", 500, || {
        let s: UISpec = serde_json::from_str(&xl_json).unwrap();
        let errors = validate::validate_spec(&s, &catalog);
        let out = serde_json::to_string(&s).unwrap();
        (errors, out)
    });

    // --- 10. Cold Pipeline (JSONL parse + cache key + validate + cache store) ---
    println!("\n── Cold Pipeline (JSONL parse + cache key + validate + cache store) ──");
    bench("cold-pipeline-3-patches", 50_000, || {
        let key = SpecCache::cache_key("unique cold prompt", &catalog_json);
        let cold_cache = SpecCache::new(std::time::Duration::from_secs(300));
        let (patches, spec) = parse_jsonl_patches(&jsonl_tiny);
        let errors = validate::validate_spec(&spec, &catalog);
        cold_cache.set(key, spec.clone());
        (patches, errors, spec)
    });
    bench("cold-pipeline-9-patches", 20_000, || {
        let key = SpecCache::cache_key("unique cold prompt", &catalog_json);
        let cold_cache = SpecCache::new(std::time::Duration::from_secs(300));
        let (patches, spec) = parse_jsonl_patches(&jsonl_small);
        let errors = validate::validate_spec(&spec, &catalog);
        cold_cache.set(key, spec.clone());
        (patches, errors, spec)
    });

    // --- 11. Sizes ---
    println!("\n── Payload Sizes ──");
    println!("  Prompt (simple):   {} bytes", prompt::build_prompt("A login form", &catalog).len());
    println!("  Prompt (complex):  {} bytes", prompt::build_prompt("An admin panel with sidebar, metrics, charts", &catalog).len());
    println!("  Spec 3 elements:    {} bytes", small_json.len());
    println!("  Spec 9 elements:    {} bytes", json9.len());
    println!("  Spec 50 elements:   {} bytes", med_json.len());
    println!("  Spec 500 elements:  {} bytes", lg_json.len());
    println!("  Spec 2000 elements: {} bytes", xl_json.len());
    println!("  JSONL 3 patches:    {} bytes", jsonl_tiny.len());
    println!("  JSONL 9 patches:    {} bytes", jsonl_small.len());
}
