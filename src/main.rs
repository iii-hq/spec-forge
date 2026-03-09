mod cache;
mod diff;
mod limiter;
mod prompt;
mod semantic;
mod types;
mod validate;

use iii_sdk::{ApiRequest, ApiResponse, III, IIIError, Streams, get_context};
use reqwest::Client;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};

use cache::SpecCache;
use limiter::RateLimiter;
use semantic::SemanticCache;
use types::*;

struct SharedState {
    cache: SpecCache,
    semantic: SemanticCache,
    limiter: RateLimiter,
    http: Client,
    api_key: String,
    streams: Streams,
}

fn json_headers() -> HashMap<String, String> {
    HashMap::from([("Content-Type".into(), "application/json".into())])
}

#[derive(Debug, Default, serde::Deserialize)]
struct RefineInput {
    #[serde(default)]
    prompt: String,
    #[serde(default)]
    current_spec: UISpec,
    #[serde(default)]
    catalog: Catalog,
    #[serde(default = "types::default_model")]
    model: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    dotenv::from_filename(std::env::var("DOTENV_PATH").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/agentsos/.env", home)
    }))
    .ok();

    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
    let engine_url =
        std::env::var("III_ENGINE_URL").unwrap_or_else(|_| "ws://127.0.0.1:49134".into());

    let iii = III::new(&engine_url);
    let streams = Streams::new(iii.clone());

    let shared = Arc::new(SharedState {
        cache: SpecCache::new(Duration::from_secs(300)),
        semantic: SemanticCache::new(0.85),
        limiter: RateLimiter::new(60, 5),
        http: Client::new(),
        api_key,
        streams,
    });

    register_functions(&iii, shared.clone());
    register_http_triggers(&iii);

    iii.connect()
        .await
        .expect("Failed to connect to iii engine — is it running?");

    println!("spec-forge worker connected to {engine_url}");
    println!();
    println!("  iii functions (6) + HTTP triggers (port 3111):");
    println!("    POST /spec-forge/generate  — cache → Claude → validate");
    println!("    POST /spec-forge/refine    — patch-based diff");
    println!("    POST /spec-forge/validate  — catalog validation");
    println!("    POST /spec-forge/prompt    — preview LLM prompt");
    println!("    GET  /spec-forge/stats     — metrics");
    println!("    GET  /spec-forge/health    — liveness");
    println!();
    println!("  Demo: open demo/index.html in browser");

    tokio::signal::ctrl_c().await.ok();
    iii.shutdown_async().await;
}

fn register_functions(iii: &III, shared: Arc<SharedState>) {
    let s = shared.clone();
    iii.register_function_with_description(
        "api::post::spec-forge::generate",
        "Generate UI spec from prompt + catalog via Claude, with SHA-256 exact + TF-IDF semantic caching",
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<GenerateRequest> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                let ctx = get_context();
                ctx.logger
                    .info("generate", Some(json!({"prompt": req.body.prompt})));
                match generate_core(&s, req.body).await {
                    Ok(body) => Ok(serde_json::to_value(ApiResponse {
                        status_code: 200,
                        headers: json_headers(),
                        body,
                    })?),
                    Err((code, body)) => Ok(serde_json::to_value(ApiResponse {
                        status_code: code,
                        headers: json_headers(),
                        body,
                    })?),
                }
            }
        },
    );

    let s = shared.clone();
    iii.register_function_with_description(
        "api::post::spec-forge::refine",
        "Patch existing UI spec with incremental changes (Add/Replace/Remove)",
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<RefineInput> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                let ctx = get_context();
                ctx.logger
                    .info("refine", Some(json!({"prompt": req.body.prompt})));
                match refine_core(&s, req.body).await {
                    Ok(body) => Ok(serde_json::to_value(ApiResponse {
                        status_code: 200,
                        headers: json_headers(),
                        body,
                    })?),
                    Err((code, body)) => Ok(serde_json::to_value(ApiResponse {
                        status_code: code,
                        headers: json_headers(),
                        body,
                    })?),
                }
            }
        },
    );

    iii.register_function_with_description(
        "api::post::spec-forge::validate",
        "Validate a UI spec against a component catalog",
        |input| async move {
            let req: ApiRequest = serde_json::from_value(input)
                .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
            let spec: UISpec = serde_json::from_value(req.body["spec"].clone())
                .map_err(|e| IIIError::Handler(format!("Invalid spec: {}", e)))?;
            let catalog: Catalog = serde_json::from_value(req.body["catalog"].clone())
                .map_err(|e| IIIError::Handler(format!("Invalid catalog: {}", e)))?;
            let body = validate_core(&spec, &catalog);
            Ok(serde_json::to_value(ApiResponse {
                status_code: 200,
                headers: json_headers(),
                body,
            })?)
        },
    );

    iii.register_function_with_description(
        "api::post::spec-forge::prompt",
        "Preview the LLM prompt for a given request",
        |input| async move {
            let req: ApiRequest<GenerateRequest> = serde_json::from_value(input)
                .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
            let body = prompt_core(&req.body.prompt, &req.body.catalog);
            Ok(serde_json::to_value(ApiResponse {
                status_code: 200,
                headers: json_headers(),
                body,
            })?)
        },
    );

    let s = shared.clone();
    iii.register_function_with_description(
        "api::get::spec-forge::stats",
        "Rate limiter + cache statistics",
        move |_input| {
            let s = s.clone();
            async move {
                let body = stats_core(&s);
                Ok(serde_json::to_value(ApiResponse {
                    status_code: 200,
                    headers: json_headers(),
                    body,
                })?)
            }
        },
    );

    iii.register_function_with_description(
        "api::get::spec-forge::health",
        "Liveness check",
        |_input| async move {
            let body = json!({"status": "ok", "service": "spec-forge"});
            Ok(serde_json::to_value(ApiResponse {
                status_code: 200,
                headers: json_headers(),
                body,
            })?)
        },
    );

    iii.register_service(
        "spec-forge",
        Some(
            "Rust generation server for json-render — caching, streaming, rate limiting, spec diffing"
                .into(),
        ),
    );
}

fn register_http_triggers(iii: &III) {
    let triggers = [
        (
            "api::post::spec-forge::generate",
            "spec-forge/generate",
            "POST",
            "Generate UI spec from prompt + catalog",
        ),
        (
            "api::post::spec-forge::refine",
            "spec-forge/refine",
            "POST",
            "Patch existing spec with changes",
        ),
        (
            "api::post::spec-forge::validate",
            "spec-forge/validate",
            "POST",
            "Validate spec against catalog",
        ),
        (
            "api::post::spec-forge::prompt",
            "spec-forge/prompt",
            "POST",
            "Preview LLM prompt",
        ),
        (
            "api::get::spec-forge::stats",
            "spec-forge/stats",
            "GET",
            "Cache + rate limiter stats",
        ),
        (
            "api::get::spec-forge::health",
            "spec-forge/health",
            "GET",
            "Liveness check",
        ),
    ];

    for (function_id, api_path, method, description) in triggers {
        if let Err(e) = iii.register_trigger(
            "http",
            function_id,
            json!({
                "api_path": api_path,
                "http_method": method,
                "description": description,
                "metadata": { "tags": ["spec-forge"] }
            }),
        ) {
            tracing::warn!("HTTP trigger {} failed: {}", api_path, e);
        }
    }
}

async fn generate_core(s: &SharedState, req: GenerateRequest) -> Result<Value, (u16, Value)> {
    let start = std::time::Instant::now();
    let catalog_json = serde_json::to_string(&req.catalog).unwrap();
    let key = SpecCache::cache_key(&req.prompt, &catalog_json);

    if let Some(spec) = s.cache.get(&key) {
        s.streams
            .increment("spec-forge::metrics::cache", "hits", 1)
            .await
            .ok();
        return Ok(json!({
            "spec": spec,
            "patches": [],
            "cached": true,
            "generation_ms": 0u64,
            "model": req.model,
        }));
    }

    let catalog_hash = SpecCache::cache_key("", &catalog_json);
    if let Some(sem_key) = s.semantic.find_similar(&req.prompt, &catalog_hash) {
        if let Some(spec) = s.cache.get(&sem_key) {
            s.streams
                .increment("spec-forge::metrics::cache", "semantic_hits", 1)
                .await
                .ok();
            return Ok(json!({
                "spec": spec,
                "patches": [],
                "cached": true,
                "generation_ms": 0u64,
                "model": req.model,
            }));
        }
    }

    s.streams
        .increment("spec-forge::metrics::cache", "misses", 1)
        .await
        .ok();

    let _guard = s
        .limiter
        .acquire()
        .await
        .map_err(|e| (429, json!({"error": format!("Rate limited: {}", e)})))?;

    let llm_prompt = prompt::build_prompt(&req.prompt, &req.catalog);
    tracing::info!("Calling Claude ({})...", req.model);

    let raw = call_claude(&s.http, &s.api_key, &req.model, &llm_prompt)
        .await
        .map_err(|e| (502, json!({"error": format!("Claude API error: {}", e)})))?;

    let (patches, spec) = parse_jsonl_patches(&raw).map_err(|e| {
        (
            422,
            json!({"error": format!("Failed to parse JSONL patches: {}", e), "raw": raw}),
        )
    })?;

    let errors = validate::validate_spec(&spec, &req.catalog);
    if !errors.is_empty() {
        return Err((
            422,
            json!({
                "error": "Spec validation failed",
                "details": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
            }),
        ));
    }

    s.cache.set(key.clone(), spec.clone());
    s.semantic.store(&req.prompt, &catalog_hash, key);

    let elapsed = start.elapsed().as_millis() as u64;
    s.streams
        .increment("spec-forge::metrics::generate", "count", 1)
        .await
        .ok();
    s.streams
        .increment("spec-forge::metrics::generate", "total_ms", elapsed as i64)
        .await
        .ok();
    s.streams
        .increment(
            "spec-forge::metrics::generate",
            "total_patches",
            patches.len() as i64,
        )
        .await
        .ok();
    tracing::info!(
        "Generated in {}ms, {} patches, {} elements",
        elapsed,
        patches.len(),
        spec.elements.len()
    );

    Ok(json!({
        "spec": spec,
        "patches": patches,
        "cached": false,
        "generation_ms": elapsed,
        "model": req.model,
    }))
}

fn parse_jsonl_patches(raw: &str) -> Result<(Vec<Value>, UISpec), String> {
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
        let patch: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let op = patch["op"].as_str().unwrap_or("");
        let path = patch["path"].as_str().unwrap_or("");

        match (op, path) {
            ("add", "/root") | ("replace", "/root") => {
                if let Some(val) = patch["value"].as_str() {
                    spec.root = val.to_string();
                }
            }
            ("add", p) | ("replace", p) if p.starts_with("/elements/") => {
                let key = p.strip_prefix("/elements/").unwrap_or("");
                if !key.is_empty() {
                    if let Ok(el) = serde_json::from_value::<UIElement>(patch["value"].clone()) {
                        spec.elements.insert(key.to_string(), el);
                    }
                }
            }
            ("remove", p) if p.starts_with("/elements/") => {
                let key = p.strip_prefix("/elements/").unwrap_or("");
                spec.elements.remove(key);
            }
            _ => {}
        }

        patches.push(patch);
    }

    if spec.root.is_empty() {
        return Err("No /root patch found in JSONL output".to_string());
    }
    if spec.elements.is_empty() {
        return Err("No /elements patches found in JSONL output".to_string());
    }

    Ok((patches, spec))
}

async fn refine_core(s: &SharedState, req: RefineInput) -> Result<Value, (u16, Value)> {
    let start = std::time::Instant::now();

    let _guard = s
        .limiter
        .acquire()
        .await
        .map_err(|e| (429, json!({"error": format!("Rate limited: {}", e)})))?;

    let llm_prompt =
        prompt::build_refine_prompt(&req.prompt, &req.current_spec, &req.catalog);
    tracing::info!("Calling Claude for refine ({})...", req.model);

    let raw = call_claude(&s.http, &s.api_key, &req.model, &llm_prompt)
        .await
        .map_err(|e| (502, json!({"error": format!("Claude API error: {}", e)})))?;

    let mut patches = Vec::new();
    let mut new_spec = req.current_spec.clone();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        let patch: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let op = patch["op"].as_str().unwrap_or("");
        let path = patch["path"].as_str().unwrap_or("");

        match (op, path) {
            ("add", "/root") | ("replace", "/root") => {
                if let Some(val) = patch["value"].as_str() {
                    new_spec.root = val.to_string();
                }
            }
            ("add", p) | ("replace", p) if p.starts_with("/elements/") => {
                let key = p.strip_prefix("/elements/").unwrap_or("");
                if !key.is_empty() {
                    if let Ok(el) = serde_json::from_value::<UIElement>(patch["value"].clone()) {
                        new_spec.elements.insert(key.to_string(), el);
                    }
                }
            }
            ("remove", p) if p.starts_with("/elements/") => {
                let key = p.strip_prefix("/elements/").unwrap_or("");
                new_spec.elements.remove(key);
            }
            _ => {}
        }

        patches.push(patch);
    }

    if patches.is_empty() {
        return Err((
            422,
            json!({"error": "No patches found in refine response", "raw": raw}),
        ));
    }

    let errors = validate::validate_spec(&new_spec, &req.catalog);
    if !errors.is_empty() {
        return Err((
            422,
            json!({
                "error": "Refined spec validation failed",
                "details": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
            }),
        ));
    }

    let elapsed = start.elapsed().as_millis() as u64;
    s.streams
        .increment("spec-forge::metrics::refine", "count", 1)
        .await
        .ok();
    tracing::info!("Refined in {}ms, {} patches", elapsed, patches.len());

    Ok(json!({
        "spec": new_spec,
        "patches": patches,
        "patch_count": patches.len(),
        "generation_ms": elapsed,
        "model": req.model,
    }))
}

fn validate_core(spec: &UISpec, catalog: &Catalog) -> Value {
    let errors = validate::validate_spec(spec, catalog);
    json!({
        "valid": errors.is_empty(),
        "errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
    })
}

fn prompt_core(prompt_text: &str, catalog: &Catalog) -> Value {
    json!({ "prompt": prompt::build_prompt(prompt_text, catalog) })
}

fn stats_core(s: &SharedState) -> Value {
    let stats = s.limiter.stats();
    json!({
        "rate_limiter": {
            "total_processed": stats.total_processed,
            "total_rejected": stats.total_rejected,
            "current_pending": stats.current_pending,
            "avg_wait_us": stats.avg_wait_us,
        },
        "cache": {
            "exact_entries": s.cache.len(),
        },
    })
}

async fn call_claude(
    http: &Client,
    api_key: &str,
    model: &str,
    prompt_text: &str,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "max_tokens": 2048,
        "stream": true,
        "messages": [{"role": "user", "content": prompt_text}]
    });

    let resp = http
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {}", e))?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.map_err(|e| format!("Read error: {}", e))?;
        return Err(format!("Claude API {}: {}", status, text));
    }

    let mut full_text = String::new();
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    let mut buf = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].to_string();
            buf = buf[pos + 1..].to_string();

            let line = line.trim();
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<Value>(data) {
                    if event["type"] == "content_block_delta" {
                        if let Some(text) = event["delta"]["text"].as_str() {
                            full_text.push_str(text);
                        }
                    }
                    if event["type"] == "error" {
                        return Err(format!(
                            "Claude stream error: {}",
                            event["error"]["message"].as_str().unwrap_or("unknown")
                        ));
                    }
                }
            }
        }
    }

    if full_text.is_empty() {
        return Err("No text received from Claude stream".to_string());
    }

    Ok(full_text)
}

fn extract_json_from_response(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let bytes = text[start..].as_bytes();
    let mut depth = 0;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}
