#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

mod cache;
mod prompt;
mod types;
mod validate;

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

fn make_catalog() -> Catalog {
    let mut components = BTreeMap::new();
    for (name, desc) in [
        ("Card", "A card container"),
        ("Metric", "Display a metric"),
        ("Button", "Clickable button"),
        ("Text", "Text paragraph"),
        ("Table", "Data table"),
        ("Chart", "Chart visualization"),
        ("Input", "Text input field"),
        ("Select", "Dropdown selector"),
    ] {
        components.insert(
            name.to_string(),
            ComponentDef {
                description: desc.to_string(),
                props: serde_json::json!({"label": "string", "value": "string"}),
                children: name == "Card",
            },
        );
    }
    let mut actions = BTreeMap::new();
    actions.insert(
        "export_report".into(),
        ActionDef {
            description: "Export to PDF".into(),
        },
    );
    actions.insert(
        "refresh_data".into(),
        ActionDef {
            description: "Refresh all".into(),
        },
    );
    actions.insert(
        "save".into(),
        ActionDef {
            description: "Save changes".into(),
        },
    );
    Catalog {
        components,
        actions,
    }
}

fn bench(name: &str, iterations: usize, mut f: impl FnMut()) {
    // warmup
    for _ in 0..std::cmp::min(100, iterations) {
        f();
    }

    let start = Instant::now();
    for _ in 0..iterations {
        f();
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
    let catalog = make_catalog();

    let small_spec = generate_spec(3, &catalog);
    let med_spec = generate_spec(50, &catalog);
    let lg_spec = generate_spec(500, &catalog);
    let xl_spec = generate_spec(2000, &catalog);

    let small_json = serde_json::to_string(&small_spec).unwrap();
    let med_json = serde_json::to_string(&med_spec).unwrap();
    let lg_json = serde_json::to_string(&lg_spec).unwrap();
    let xl_json = serde_json::to_string(&xl_spec).unwrap();

    let catalog_json = serde_json::to_string(&catalog).unwrap();

    println!("=== iii-render (Rust) Benchmark ===\n");

    println!("--- JSON Parse (serde) ---");
    bench("parse-3-elements", 100000, || {
        let _: UISpec = serde_json::from_str(&small_json).unwrap();
    });
    bench("parse-50-elements", 50000, || {
        let _: UISpec = serde_json::from_str(&med_json).unwrap();
    });
    bench("parse-500-elements", 5000, || {
        let _: UISpec = serde_json::from_str(&lg_json).unwrap();
    });
    bench("parse-2000-elements", 1000, || {
        let _: UISpec = serde_json::from_str(&xl_json).unwrap();
    });

    println!("\n--- Validate ---");
    bench("validate-3-elements", 100000, || {
        validate::validate_spec(&small_spec, &catalog);
    });
    bench("validate-50-elements", 50000, || {
        validate::validate_spec(&med_spec, &catalog);
    });
    bench("validate-500-elements", 5000, || {
        validate::validate_spec(&lg_spec, &catalog);
    });
    bench("validate-2000-elements", 1000, || {
        validate::validate_spec(&xl_spec, &catalog);
    });

    println!("\n--- Prompt Build ---");
    bench("prompt-build", 100000, || {
        prompt::build_prompt("Sales dashboard", &catalog);
    });

    println!("\n--- Cache Key (SHA-256) ---");
    bench("cache-key", 100000, || {
        cache::SpecCache::cache_key("Sales dashboard", &catalog_json);
    });

    println!("\n--- JSON Stringify (serde) ---");
    bench("stringify-3-elements", 100000, || {
        serde_json::to_string(&small_spec).unwrap();
    });
    bench("stringify-50-elements", 50000, || {
        serde_json::to_string(&med_spec).unwrap();
    });
    bench("stringify-500-elements", 5000, || {
        serde_json::to_string(&lg_spec).unwrap();
    });
    bench("stringify-2000-elements", 1000, || {
        serde_json::to_string(&xl_spec).unwrap();
    });

    println!("\n--- Full Pipeline (parse+validate+stringify) ---");
    bench("pipeline-3", 100000, || {
        let s: UISpec = serde_json::from_str(&small_json).unwrap();
        validate::validate_spec(&s, &catalog);
        serde_json::to_string(&s).unwrap();
    });
    bench("pipeline-50", 20000, || {
        let s: UISpec = serde_json::from_str(&med_json).unwrap();
        validate::validate_spec(&s, &catalog);
        serde_json::to_string(&s).unwrap();
    });
    bench("pipeline-500", 2000, || {
        let s: UISpec = serde_json::from_str(&lg_json).unwrap();
        validate::validate_spec(&s, &catalog);
        serde_json::to_string(&s).unwrap();
    });
    bench("pipeline-2000", 500, || {
        let s: UISpec = serde_json::from_str(&xl_json).unwrap();
        validate::validate_spec(&s, &catalog);
        serde_json::to_string(&s).unwrap();
    });

    println!("\n--- Spec Sizes ---");
    println!("  3 elements:    {} bytes", small_json.len());
    println!("  50 elements:   {} bytes", med_json.len());
    println!("  500 elements:  {} bytes", lg_json.len());
    println!("  2000 elements: {} bytes", xl_json.len());
}
