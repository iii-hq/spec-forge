mod cache;
mod catalogs;
mod limiter;
mod prompt;
mod semantic;
mod session;
mod types;
mod validate;

use iii_sdk::{ApiRequest, ApiResponse, III, IIIError, InitOptions, RegisterTriggerInput, Streams, protocol::RegisterServiceMessage, protocol::RegisterFunctionMessage, register_worker};
use reqwest::Client;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};

use cache::SpecCache;
use limiter::RateLimiter;
use semantic::SemanticCache;
use types::*;

struct SharedState {
    iii: III,
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
    #[serde(default)]
    catalog_preset: Option<String>,
    #[serde(default = "types::default_model")]
    model: String,
}

fn resolve_catalog(catalog: &Catalog, preset: &Option<String>) -> Result<Catalog, (u16, Value)> {
    if let Some(name) = preset {
        catalogs::get_preset(name).ok_or_else(|| {
            (
                400,
                json!({
                    "error": format!("Unknown catalog preset: '{}'", name),
                    "available": catalogs::list_presets(),
                }),
            )
        })
    } else if catalog.components.is_empty() {
        Err((
            400,
            json!({
                "error": "No catalog provided. Send 'catalog' object or 'catalog_preset' name.",
                "available_presets": catalogs::list_presets(),
            }),
        ))
    } else {
        Ok(catalog.clone())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    dotenv::from_filename(
        std::env::var("DOTENV_PATH").unwrap_or_else(|_| ".env".into()),
    )
    .ok();

    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
    let engine_url =
        std::env::var("III_ENGINE_URL").unwrap_or_else(|_| "ws://127.0.0.1:49134".into());

    let iii = register_worker(&engine_url, InitOptions::default());
    let streams = Streams::new(iii.clone());

    let shared = Arc::new(SharedState {
        iii: iii.clone(),
        cache: SpecCache::new(Duration::from_secs(300)),
        semantic: SemanticCache::new(0.85),
        limiter: RateLimiter::new(60, 5),
        http: Client::new(),
        api_key,
        streams,
    });

    register_functions(&iii, shared.clone());
    register_http_triggers(&iii);

    println!("spec-forge worker connected to {engine_url}");
    println!();
    println!("  iii functions (8) + HTTP triggers (port 3111):");
    println!("    POST /spec-forge/generate  — cache → Claude → validate");
    println!("    POST /spec-forge/stream    — real-time patches via iii Channel (WebSocket)");
    println!("    POST /spec-forge/refine    — patch-based diff");
    println!("    POST /spec-forge/validate  — catalog validation");
    println!("    POST /spec-forge/prompt    — preview LLM prompt");
    println!("    GET  /spec-forge/catalogs  — built-in presets (3d, dashboard, form, ...)");
    println!("    GET  /spec-forge/stats     — metrics");
    println!("    GET  /spec-forge/health    — liveness");
    println!();
    println!("  Catalog presets: {:?}", catalogs::list_presets());
    println!("  Demo: open demo/index.html in browser");

    tokio::signal::ctrl_c().await.ok();
    iii.shutdown_async().await;
}

fn register_functions(iii: &III, shared: Arc<SharedState>) {
    let s = shared.clone();
    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::post::spec-forge::generate".to_string())
            .with_description("Generate UI spec from prompt + catalog via Claude, with SHA-256 exact + TF-IDF semantic caching".to_string()),
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<GenerateRequest> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                tracing::info!(action = "generate", prompt_len = req.body.prompt.len());
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
    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::post::spec-forge::refine".to_string())
            .with_description("Patch existing UI spec with incremental changes (Add/Replace/Remove)".to_string()),
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<RefineInput> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                tracing::info!(action = "refine", prompt_len = req.body.prompt.len());
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

    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::post::spec-forge::validate".to_string())
            .with_description("Validate a UI spec against a component catalog".to_string()),
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

    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::post::spec-forge::prompt".to_string())
            .with_description("Preview the LLM prompt for a given request".to_string()),
        |input| async move {
            let req: ApiRequest<GenerateRequest> = serde_json::from_value(input)
                .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
            match prompt_core(&req.body.prompt, &req.body.catalog, &req.body.catalog_preset) {
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
        },
    );

    let s = shared.clone();
    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::get::spec-forge::stats".to_string())
            .with_description("Rate limiter + cache statistics".to_string()),
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

    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::get::spec-forge::health".to_string())
            .with_description("Liveness check".to_string()),
        |_input| async move {
            let body = json!({"status": "ok", "service": "spec-forge"});
            Ok(serde_json::to_value(ApiResponse {
                status_code: 200,
                headers: json_headers(),
                body,
            })?)
        },
    );

    let s = shared.clone();
    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::post::spec-forge::stream".to_string())
            .with_description("Stream UI spec patches via iii Channel — returns WebSocket reader ref for real-time patches".to_string()),
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<GenerateRequest> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                tracing::info!(action = "stream", prompt_len = req.body.prompt.len());
                match stream_core(&s, req.body).await {
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

    iii.register_function_with(
        RegisterFunctionMessage::with_id("api::get::spec-forge::catalogs".to_string())
            .with_description("List available built-in catalog presets (minimal, dashboard, form, ecommerce, 3d, 3d-product)".to_string()),
        |input| async move {
            let req: ApiRequest = serde_json::from_value(input)
                .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
            let name = req.path_params.get("name").map(|s| s.as_str());
            let body = match name {
                Some(n) => {
                    if let Some(catalog) = catalogs::get_preset(n) {
                        json!({ "name": n, "catalog": catalog, "component_count": catalog.components.len() })
                    } else {
                        return Ok(serde_json::to_value(ApiResponse {
                            status_code: 404,
                            headers: json_headers(),
                            body: json!({"error": format!("Unknown preset: '{}'", n), "available": catalogs::list_presets()}),
                        })?);
                    }
                }
                None => {
                    let presets: Vec<Value> = catalogs::list_presets()
                        .iter()
                        .map(|name| {
                            let cat = catalogs::get_preset(name).unwrap();
                            json!({"name": name, "component_count": cat.components.len()})
                        })
                        .collect();
                    json!({"presets": presets})
                }
            };
            Ok(serde_json::to_value(ApiResponse {
                status_code: 200,
                headers: json_headers(),
                body,
            })?)
        },
    );

    // Session management functions
    let s = shared.clone();
    iii.register_function_with(
        RegisterFunctionMessage::with_id("spec-forge::join-session".to_string())
            .with_description("Join a collaborative session — adds browser worker to peer list, pushes current spec".to_string()),
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<JoinSessionRequest> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                req.body.validate().map_err(|e| IIIError::Handler(e))?;
                let worker_id = req.body.worker_id.unwrap_or_else(|| "anonymous".to_string());
                let info = session::join_session(&s.iii, &req.body.session_id, &worker_id)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;

                if let Some(ref spec) = info.spec {
                    let fn_id = format!("ui::render-patch::{}", worker_id);
                    let _ = s.iii.trigger(iii_sdk::TriggerRequest {
                        function_id: fn_id,
                        payload: json!({ "type": "done", "spec": spec, "session": req.body.session_id }),
                        action: Some(iii_sdk::TriggerAction::Void),
                        timeout_ms: None,
                    }).await;
                }

                Ok(serde_json::to_value(ApiResponse {
                    status_code: 200,
                    headers: json_headers(),
                    body: json!(info),
                })?)
            }
        },
    );

    let s = shared.clone();
    iii.register_function_with(
        RegisterFunctionMessage::with_id("spec-forge::leave-session".to_string())
            .with_description("Leave a collaborative session — removes browser worker from peer list".to_string()),
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<LeaveSessionRequest> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                req.body.validate().map_err(|e| IIIError::Handler(e))?;
                let wid = req.body.worker_id.clone();
                session::leave_session(&s.iii, &req.body.session_id, &wid)
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                Ok(serde_json::to_value(ApiResponse {
                    status_code: 200,
                    headers: json_headers(),
                    body: json!({ "left": true }),
                })?)
            }
        },
    );

    let s = shared.clone();
    iii.register_function_with(
        RegisterFunctionMessage::with_id("spec-forge::push-patch".to_string())
            .with_description("Push a patch to all browsers in a session via fan-out".to_string()),
        move |input| {
            let s = s.clone();
            async move {
                let req: ApiRequest<PushPatchRequest> = serde_json::from_value(input)
                    .map_err(|e| IIIError::Handler(format!("Bad request: {}", e)))?;
                req.body.validate().map_err(|e| IIIError::Handler(e))?;
                session::fan_out_patch(&s.iii, &req.body.session_id, &req.body.patch, req.body.target.as_deref())
                    .await
                    .map_err(|e| IIIError::Handler(e.to_string()))?;
                Ok(serde_json::to_value(ApiResponse {
                    status_code: 200,
                    headers: json_headers(),
                    body: json!({ "pushed": true }),
                })?)
            }
        },
    );

    iii.register_service(RegisterServiceMessage {
        id: "spec-forge".to_string(),
        name: "spec-forge".to_string(),
        description: Some("Rust generation server for json-render — caching, streaming, rate limiting, collaboration, 3D scene support".to_string()),
        parent_service_id: None,
    });
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
        (
            "api::post::spec-forge::stream",
            "spec-forge/stream",
            "POST",
            "Stream patches via iii Channel (WebSocket)",
        ),
        (
            "api::get::spec-forge::catalogs",
            "spec-forge/catalogs",
            "GET",
            "List available catalog presets",
        ),
        (
            "api::get::spec-forge::catalogs",
            "spec-forge/catalogs/:name",
            "GET",
            "Get a specific catalog preset by name",
        ),
        (
            "spec-forge::join-session",
            "spec-forge/join",
            "POST",
            "Join a collaborative session",
        ),
        (
            "spec-forge::leave-session",
            "spec-forge/leave",
            "POST",
            "Leave a collaborative session",
        ),
        (
            "spec-forge::push-patch",
            "spec-forge/push",
            "POST",
            "Push a patch to all browsers in a session",
        ),
    ];

    for (function_id, api_path, method, description) in triggers {
        if let Err(e) = iii.register_trigger(RegisterTriggerInput {
            trigger_type: "http".to_string(),
            function_id: function_id.to_string(),
            config: json!({
                "api_path": api_path,
                "http_method": method,
                "description": description,
                "metadata": { "tags": ["spec-forge"] }
            }),
        }) {
            tracing::warn!("HTTP trigger {} failed: {}", api_path, e);
        }
    }
}

async fn generate_core(s: &SharedState, req: GenerateRequest) -> Result<Value, (u16, Value)> {
    let start = std::time::Instant::now();
    let catalog = resolve_catalog(&req.catalog, &req.catalog_preset)?;
    let catalog_json = serde_json::to_string(&catalog).unwrap();
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

    let llm_prompt = prompt::build_prompt(&req.prompt, &catalog);
    tracing::info!("Calling Claude ({})...", req.model);

    let raw = call_claude(&s.http, &s.api_key, &req.model, &llm_prompt, req.max_tokens)
        .await
        .map_err(|e| (502, json!({"error": format!("Claude API error: {}", e)})))?;

    let (patches, spec) = parse_jsonl_patches(&raw).map_err(|e| {
        (
            422,
            json!({"error": format!("Failed to parse JSONL patches: {}", e), "raw": raw}),
        )
    })?;

    let errors = validate::validate_spec(&spec, &catalog);
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
    let is_3d = catalog.components.contains_key("PerspectiveCamera")
        && catalog.components.contains_key("AmbientLight");
    let metric_key = if is_3d {
        "spec-forge::metrics::generate_3d"
    } else {
        "spec-forge::metrics::generate"
    };
    s.streams
        .increment(metric_key, "count", 1)
        .await
        .ok();
    s.streams
        .increment(metric_key, "total_ms", elapsed as i64)
        .await
        .ok();
    s.streams
        .increment(
            metric_key,
            "total_patches",
            patches.len() as i64,
        )
        .await
        .ok();
    tracing::info!(
        "Generated {}in {}ms, {} patches, {} elements",
        if is_3d { "3D scene " } else { "" },
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

fn apply_patch(spec: &mut UISpec, patch: &Value) {
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
}

fn parse_jsonl_to_patches(raw: &str) -> Vec<Value> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty()
                || line.starts_with("```")
                || line.starts_with("//")
                || line.starts_with('#')
            {
                return None;
            }
            let line = line.trim_start_matches(|c: char| !c.is_ascii_punctuation() || c == '-');
            let line = line.trim();
            if !line.starts_with('{') {
                return None;
            }
            serde_json::from_str(line).ok()
        })
        .collect()
}

fn try_extract_spec_from_raw(raw: &str) -> Option<UISpec> {
    let text = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(spec) = serde_json::from_str::<UISpec>(text) {
        if !spec.root.is_empty() && !spec.elements.is_empty() {
            return Some(spec);
        }
    }
    if let Ok(val) = serde_json::from_str::<Value>(text) {
        if let Some(spec_val) = val.get("spec").or(Some(&val)) {
            if let Ok(spec) = serde_json::from_value::<UISpec>(spec_val.clone()) {
                if !spec.root.is_empty() && !spec.elements.is_empty() {
                    return Some(spec);
                }
            }
        }
    }
    None
}

fn parse_jsonl_patches(raw: &str) -> Result<(Vec<Value>, UISpec), String> {
    let patches = parse_jsonl_to_patches(raw);
    let mut spec = UISpec {
        root: String::new(),
        elements: HashMap::new(),
    };

    for patch in &patches {
        apply_patch(&mut spec, patch);
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
    let catalog = resolve_catalog(&req.catalog, &req.catalog_preset)?;

    let _guard = s
        .limiter
        .acquire()
        .await
        .map_err(|e| (429, json!({"error": format!("Rate limited: {}", e)})))?;

    let llm_prompt =
        prompt::build_refine_prompt(&req.prompt, &req.current_spec, &catalog);
    tracing::info!("Calling Claude for refine ({})...", req.model);

    let raw = call_claude(&s.http, &s.api_key, &req.model, &llm_prompt, 4096)
        .await
        .map_err(|e| (502, json!({"error": format!("Claude API error: {}", e)})))?;

    let preview: String = raw.chars().take(500).collect();
    tracing::debug!("Refine raw response ({} chars): {}", raw.len(), preview);
    let patches = parse_jsonl_to_patches(&raw);
    tracing::info!("Parsed {} patches from refine response", patches.len());
    let mut new_spec = req.current_spec.clone();

    for patch in &patches {
        apply_patch(&mut new_spec, patch);
    }

    if patches.is_empty() {
        tracing::info!("No patches from refine — attempting full spec extraction");
        let fallback = try_extract_spec_from_raw(&raw);
        let elapsed = start.elapsed().as_millis() as u64;
        match fallback {
            Some(extracted) => {
                let errors = validate::validate_spec(&extracted, &catalog);
                let warnings: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
                let changed = extracted.root != req.current_spec.root
                    || extracted.elements.len() != req.current_spec.elements.len();
                return Ok(json!({
                    "spec": extracted,
                    "patches": [],
                    "patch_count": if changed { 1 } else { 0 },
                    "generation_ms": elapsed,
                    "model": req.model,
                    "valid": warnings.is_empty(),
                    "warnings": if changed { warnings } else { vec!["No changes needed — spec already matches the request".to_string()] },
                }));
            }
            None => {
                return Ok(json!({
                    "spec": req.current_spec,
                    "patches": [],
                    "patch_count": 0,
                    "generation_ms": elapsed,
                    "model": req.model,
                    "valid": true,
                    "warnings": ["No changes needed — spec already matches the request"],
                }));
            }
        }
    }

    let errors = validate::validate_spec(&new_spec, &catalog);
    let warnings: Vec<String> = errors.iter().map(|e| e.to_string()).collect();

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
        "valid": warnings.is_empty(),
        "warnings": warnings,
    }))
}

fn validate_core(spec: &UISpec, catalog: &Catalog) -> Value {
    let errors = validate::validate_spec(spec, catalog);
    json!({
        "valid": errors.is_empty(),
        "errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
    })
}

fn prompt_core(prompt_text: &str, catalog: &Catalog, catalog_preset: &Option<String>) -> Result<Value, (u16, Value)> {
    let resolved = resolve_catalog(catalog, catalog_preset)?;
    Ok(json!({ "prompt": prompt::build_prompt(prompt_text, &resolved) }))
}

async fn stream_core(s: &SharedState, req: GenerateRequest) -> Result<Value, (u16, Value)> {
    let catalog = resolve_catalog(&req.catalog, &req.catalog_preset)?;
    let catalog_json = serde_json::to_string(&catalog).unwrap();
    let key = SpecCache::cache_key(&req.prompt, &catalog_json);
    let session_id = req.session_id.clone();

    if let Some(spec) = s.cache.get(&key) {
        s.streams
            .increment("spec-forge::metrics::cache", "hits", 1)
            .await
            .ok();

        // If in session mode, push cached spec to all peers
        if let Some(ref sid) = session_id {
            let done_msg = json!({"type": "done", "spec": spec, "valid": true, "generation_ms": 0});
            session::fan_out_patch(&s.iii, sid, &done_msg, None).await.ok();
            session::store_spec(&s.iii, sid, &json!(spec), "cached").await.ok();
        }

        return Ok(json!({
            "cached": true,
            "spec": spec,
            "channel": null,
        }));
    }

    let _guard = s
        .limiter
        .acquire()
        .await
        .map_err(|e| (429, json!({"error": format!("Rate limited: {}", e)})))?;

    let channel = s
        .iii
        .create_channel(Some(64))
        .await
        .map_err(|e| (500, json!({"error": format!("Channel creation failed: {}", e)})))?;

    let reader_ref = channel.reader_ref.clone();
    let writer = channel.writer;
    let http = s.http.clone();
    let api_key = s.api_key.clone();
    let model = req.model.clone();
    let prompt_text = prompt::build_prompt(&req.prompt, &catalog);
    let catalog = catalog.clone();
    let max_tokens = req.max_tokens;
    let cache = s.cache.clone();
    let semantic = s.semantic.clone();
    let streams = s.streams.clone();
    let prompt_str = req.prompt.clone();
    let catalog_hash = SpecCache::cache_key("", &catalog_json);
    let iii_clone = s.iii.clone();

    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let sid_clone = session_id.clone();
        let iii_for_patches = iii_clone.clone();
        match call_claude_streaming(&http, &api_key, &model, &prompt_text, &writer, max_tokens, sid_clone.as_deref(), &iii_for_patches).await {
            Ok(spec) => {
                let errors = validate::validate_spec(&spec, &catalog);
                let elapsed = start.elapsed().as_millis() as u64;
                if errors.is_empty() {
                    cache.set(key.clone(), spec.clone());
                    semantic.store(&prompt_str, &catalog_hash, key);
                }
                let done_msg = json!({
                    "type": "done",
                    "spec": spec,
                    "valid": errors.is_empty(),
                    "errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                    "generation_ms": elapsed,
                });
                writer.send_message(&done_msg.to_string()).await.ok();

                // Session mode: fan out patches to all peers
                if let Some(ref sid) = session_id {
                    session::fan_out_patch(&iii_clone, sid, &done_msg, None).await.ok();
                    session::store_spec(&iii_clone, sid, &json!(spec), "browser").await.ok();
                }

                streams.increment("spec-forge::metrics::generate", "count", 1).await.ok();
                streams.increment("spec-forge::metrics::generate", "total_ms", elapsed as i64).await.ok();
            }
            Err(e) => {
                let err_msg = json!({"type": "error", "error": e});
                writer.send_message(&err_msg.to_string()).await.ok();
                if let Some(ref sid) = session_id {
                    session::fan_out_patch(&iii_clone, sid, &err_msg, None).await.ok();
                }
            }
        }
        writer.close().await.ok();
    });

    Ok(json!({
        "cached": false,
        "channel": {
            "channel_id": reader_ref.channel_id,
            "access_key": reader_ref.access_key,
        },
    }))
}

async fn call_claude_streaming(
    http: &Client,
    api_key: &str,
    model: &str,
    prompt_text: &str,
    writer: &iii_sdk::channels::ChannelWriter,
    max_tokens: u32,
    session_id: Option<&str>,
    iii: &III,
) -> Result<UISpec, String> {
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
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

    let mut spec = UISpec {
        root: String::new(),
        elements: HashMap::new(),
    };
    let mut token_buf = String::new();
    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    let mut sse_buf = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        sse_buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(pos) = sse_buf.find('\n') {
            let line = sse_buf[..pos].to_string();
            sse_buf = sse_buf[pos + 1..].to_string();

            let line = line.trim();
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<Value>(data) {
                    if event["type"] == "content_block_delta" {
                        if let Some(text) = event["delta"]["text"].as_str() {
                            token_buf.push_str(text);

                            while let Some(nl) = token_buf.find('\n') {
                                let jsonl_line = token_buf[..nl].trim().to_string();
                                token_buf = token_buf[nl + 1..].to_string();

                                if jsonl_line.is_empty() || !jsonl_line.starts_with('{') {
                                    continue;
                                }
                                if let Ok(patch) = serde_json::from_str::<Value>(&jsonl_line) {
                                    apply_patch(&mut spec, &patch);
                                    let msg = json!({"type": "patch", "patch": patch});
                                    writer.send_message(&msg.to_string()).await.ok();
                                    if let Some(sid) = session_id {
                                        session::fan_out_patch(iii, sid, &msg, None).await.ok();
                                    }
                                }
                            }
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

    let remaining = token_buf.trim();
    if !remaining.is_empty() && remaining.starts_with('{') {
        if let Ok(patch) = serde_json::from_str::<Value>(remaining) {
            apply_patch(&mut spec, &patch);
            let msg = json!({"type": "patch", "patch": patch});
            writer.send_message(&msg.to_string()).await.ok();
            if let Some(sid) = session_id {
                session::fan_out_patch(iii, sid, &msg, None).await.ok();
            }
        }
    }

    if spec.root.is_empty() {
        return Err("No /root patch found in JSONL output".to_string());
    }

    Ok(spec)
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
    max_tokens: u32,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
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

