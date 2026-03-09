mod cache;
mod diff;
mod limiter;
mod parser;
mod prompt;
mod semantic;
mod types;
mod validate;

use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::StreamExt;
use reqwest::Client;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tower_http::cors::CorsLayer;

use cache::SpecCache;
use limiter::RateLimiter;
use parser::IncrementalJsonParser;
use semantic::SemanticCache;
use types::*;

struct AppState {
    cache: SpecCache,
    semantic: SemanticCache,
    limiter: RateLimiter,
    http: Client,
    api_key: String,
}

#[tokio::main]
async fn main() {
    dotenv::from_filename(std::env::var("DOTENV_PATH").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/agentsos/.env", home)
    }))
    .ok();

    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
    eprintln!("API key loaded: {}...{}", &api_key[..12], &api_key[api_key.len() - 4..]);

    let state = Arc::new(AppState {
        cache: SpecCache::new(Duration::from_secs(300)),
        semantic: SemanticCache::new(0.85),
        limiter: RateLimiter::new(60, 5), // 60 req/min, 5 concurrent
        http: Client::new(),
        api_key,
    });

    let app = Router::new()
        .route("/", get(demo_ui))
        .route("/health", get(health))
        .route("/generate", post(generate))
        .route("/refine", post(refine))
        .route("/stream", post(stream_generate))
        .route("/validate", post(validate_endpoint))
        .route("/prompt", post(prompt_endpoint))
        .route("/stats", get(stats_endpoint))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = "0.0.0.0:3112";
    println!("iii-render listening on {}", addr);
    println!("  GET  /          — interactive demo UI");
    println!("  POST /generate  — generate spec via Claude (exact + semantic cache)");
    println!("  POST /refine    — patch existing spec (diff-based, minimal LLM call)");
    println!("  POST /stream    — generate with SSE streaming");
    println!("  POST /validate  — validate a spec against a catalog");
    println!("  GET  /stats     — rate limiter stats");
    println!("  POST /prompt    — preview the LLM prompt");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn demo_ui() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../demo/index.html"),
    )
}

async fn health() -> &'static str {
    "ok"
}

async fn call_claude(
    http: &Client,
    api_key: &str,
    model: &str,
    prompt_text: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "messages": [{
            "role": "user",
            "content": prompt_text,
        }]
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
    let text = resp.text().await.map_err(|e| format!("Read error: {}", e))?;

    if !status.is_success() {
        return Err(format!("Claude API {}: {}", status, text));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("JSON parse: {}", e))?;

    let content = parsed["content"][0]["text"]
        .as_str()
        .ok_or_else(|| "No text in Claude response".to_string())?
        .to_string();

    Ok(content)
}

async fn call_claude_streaming(
    http: &Client,
    api_key: &str,
    model: &str,
    prompt_text: &str,
) -> Result<impl futures::Stream<Item = Result<String, String>>, String> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "stream": true,
        "messages": [{
            "role": "user",
            "content": prompt_text,
        }]
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

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Claude API error: {}", text));
    }

    let byte_stream = resp.bytes_stream();

    let text_stream = byte_stream.map(|chunk| {
        let bytes = chunk.map_err(|e| format!("Stream error: {}", e))?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        Ok(text)
    });

    Ok(text_stream)
}

fn extract_json_from_response(text: &str) -> Option<String> {
    if let Some(start) = text.find('{') {
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
    }
    None
}

async fn generate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GenerateRequest>,
) -> Response {
    let start = std::time::Instant::now();
    let catalog_json = serde_json::to_string(&req.catalog).unwrap();
    let key = SpecCache::cache_key(&req.prompt, &catalog_json);

    if let Some(spec) = state.cache.get(&key) {
        let resp = GenerateResponse {
            spec,
            cached: true,
            generation_ms: start.elapsed().as_millis() as u64,
            model: req.model,
        };
        return (StatusCode::OK, Json(resp)).into_response();
    }

    let catalog_hash = SpecCache::cache_key("", &catalog_json);
    if let Some(sem_key) = state.semantic.find_similar(&req.prompt, &catalog_hash) {
        if let Some(spec) = state.cache.get(&sem_key) {
            eprintln!("Semantic cache hit for: {}", req.prompt);
            let resp = GenerateResponse {
                spec,
                cached: true,
                generation_ms: start.elapsed().as_millis() as u64,
                model: req.model,
            };
            return (StatusCode::OK, Json(resp)).into_response();
        }
    }

    let _guard = match state.limiter.acquire().await {
        Ok(g) => g,
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Rate limited: {}", e),
                details: None,
            };
            return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
        }
    };

    let llm_prompt = prompt::build_prompt(&req.prompt, &req.catalog);
    eprintln!("Calling Claude ({})...", req.model);

    let raw = match call_claude(&state.http, &state.api_key, &req.model, &llm_prompt).await {
        Ok(text) => text,
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Claude API error: {}", e),
                details: None,
            };
            return (StatusCode::BAD_GATEWAY, Json(err)).into_response();
        }
    };

    let json_str = match extract_json_from_response(&raw) {
        Some(j) => j,
        None => {
            let err = ErrorResponse {
                error: "Could not extract JSON from Claude response".into(),
                details: Some(vec![raw]),
            };
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response();
        }
    };

    let spec: UISpec = match serde_json::from_str(&json_str) {
        Ok(s) => s,
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Invalid spec JSON: {}", e),
                details: Some(vec![json_str]),
            };
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response();
        }
    };

    let errors = validate::validate_spec(&spec, &req.catalog);
    if !errors.is_empty() {
        let err = ErrorResponse {
            error: "Spec validation failed".into(),
            details: Some(errors.iter().map(|e| e.to_string()).collect()),
        };
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response();
    }

    state.cache.set(key.clone(), spec.clone());
    state.semantic.store(&req.prompt, &catalog_hash, key);

    let elapsed = start.elapsed().as_millis() as u64;
    eprintln!("Generated in {}ms, {} elements", elapsed, spec.elements.len());

    let resp = GenerateResponse {
        spec,
        cached: false,
        generation_ms: elapsed,
        model: req.model,
    };

    (StatusCode::OK, Json(resp)).into_response()
}

async fn stream_generate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GenerateRequest>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let catalog_json = serde_json::to_string(&req.catalog).unwrap();
    let key = SpecCache::cache_key(&req.prompt, &catalog_json);
    let cached = state.cache.get(&key);

    let http = state.http.clone();
    let api_key = state.api_key.clone();
    let model = req.model.clone();
    let cache = state.cache.clone();

    let stream = async_stream::stream! {
        if let Some(spec) = cached {
            yield Ok(Event::default()
                .event("done")
                .json_data(&serde_json::json!({"done": true, "spec": spec, "cached": true}))
                .unwrap());
            return;
        }

        let llm_prompt = prompt::build_prompt(&req.prompt, &req.catalog);

        let claude_stream = match call_claude_streaming(&http, &api_key, &model, &llm_prompt).await {
            Ok(s) => s,
            Err(e) => {
                yield Ok(Event::default()
                    .event("error")
                    .json_data(&serde_json::json!({"error": e}))
                    .unwrap());
                return;
            }
        };

        let mut parser = IncrementalJsonParser::new();
        let mut sent_root = false;

        tokio::pin!(claude_stream);

        while let Some(chunk_result) = claude_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(_) => continue,
            };

            for line in chunk.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        continue;
                    }
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(text) = event["delta"]["text"].as_str() {
                            yield Ok(Event::default()
                                .event("text")
                                .json_data(&serde_json::json!({"text": text}))
                                .unwrap());

                            parser.feed(text);

                            if !sent_root {
                                if let Some(root) = parser.root() {
                                    yield Ok(Event::default()
                                        .event("root")
                                        .json_data(&serde_json::json!({"root": root}))
                                        .unwrap());
                                    sent_root = true;
                                }
                            }

                            while let Some((id, element)) = parser.next_element() {
                                yield Ok(Event::default()
                                    .event("element")
                                    .json_data(&serde_json::json!({"id": id, "element": element}))
                                    .unwrap());
                            }
                        }
                    }
                }
            }
        }

        if let Some(spec) = parser.finalize() {
            let errors = validate::validate_spec(&spec, &req.catalog);
            if errors.is_empty() {
                cache.set(key.clone(), spec.clone());
            }
            yield Ok(Event::default()
                .event("done")
                .json_data(&serde_json::json!({
                    "done": true,
                    "spec": spec,
                    "cached": false,
                    "errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                }))
                .unwrap());
        } else {
            yield Ok(Event::default()
                .event("error")
                .json_data(&serde_json::json!({"error": "Failed to parse complete spec from stream"}))
                .unwrap());
        }
    };

    Sse::new(stream)
}

async fn validate_endpoint(Json(body): Json<serde_json::Value>) -> Response {
    let spec: UISpec = match serde_json::from_value(body["spec"].clone()) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid spec: {}", e),
                    details: None,
                }),
            )
                .into_response()
        }
    };
    let catalog: Catalog = match serde_json::from_value(body["catalog"].clone()) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid catalog: {}", e),
                    details: None,
                }),
            )
                .into_response()
        }
    };

    let errors = validate::validate_spec(&spec, &catalog);
    if errors.is_empty() {
        (
            StatusCode::OK,
            Json(serde_json::json!({"valid": true, "errors": []})),
        )
            .into_response()
    } else {
        let details: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        (
            StatusCode::OK,
            Json(serde_json::json!({"valid": false, "errors": details})),
        )
            .into_response()
    }
}

async fn prompt_endpoint(Json(req): Json<GenerateRequest>) -> String {
    prompt::build_prompt(&req.prompt, &req.catalog)
}

#[derive(Debug, serde::Deserialize)]
struct RefineRequest {
    prompt: String,
    current_spec: UISpec,
    catalog: Catalog,
    #[serde(default = "types::default_model")]
    model: String,
}

async fn refine(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefineRequest>,
) -> Response {
    let start = std::time::Instant::now();

    let _guard = match state.limiter.acquire().await {
        Ok(g) => g,
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Rate limited: {}", e),
                details: None,
            };
            return (StatusCode::TOO_MANY_REQUESTS, Json(err)).into_response();
        }
    };

    let llm_prompt = diff::build_diff_prompt(&req.prompt, &req.current_spec, &req.catalog);
    eprintln!("Calling Claude for refine ({})...", req.model);

    let raw = match call_claude(&state.http, &state.api_key, &req.model, &llm_prompt).await {
        Ok(text) => text,
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Claude API error: {}", e),
                details: None,
            };
            return (StatusCode::BAD_GATEWAY, Json(err)).into_response();
        }
    };

    let json_str = if let Some(start) = raw.find('[') {
        if let Some(end) = raw.rfind(']') {
            raw[start..=end].to_string()
        } else {
            extract_json_from_response(&raw).unwrap_or_default()
        }
    } else if let Some(obj) = extract_json_from_response(&raw) {
        format!("[{}]", obj)
    } else {
        let err = ErrorResponse {
            error: "Could not extract patches from Claude response".into(),
            details: Some(vec![raw]),
        };
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response();
    };

    let patches: Vec<diff::SpecPatch> = match serde_json::from_str(&json_str) {
        Ok(p) => p,
        Err(e) => {
            let err = ErrorResponse {
                error: format!("Invalid patches JSON: {}", e),
                details: Some(vec![json_str]),
            };
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response();
        }
    };

    let new_spec = diff::apply_patches(&req.current_spec, &patches);

    let errors = validate::validate_spec(&new_spec, &req.catalog);
    if !errors.is_empty() {
        let err = ErrorResponse {
            error: "Refined spec validation failed".into(),
            details: Some(errors.iter().map(|e| e.to_string()).collect()),
        };
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(err)).into_response();
    }

    let elapsed = start.elapsed().as_millis() as u64;
    eprintln!("Refined in {}ms, {} patches applied", elapsed, patches.len());

    let resp = serde_json::json!({
        "spec": new_spec,
        "patches": patches,
        "patch_count": patches.len(),
        "generation_ms": elapsed,
        "model": req.model,
    });

    (StatusCode::OK, Json(resp)).into_response()
}

async fn stats_endpoint(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let stats = state.limiter.stats();
    Json(serde_json::json!({
        "rate_limiter": {
            "total_processed": stats.total_processed,
            "total_rejected": stats.total_rejected,
            "current_pending": stats.current_pending,
            "avg_wait_us": stats.avg_wait_us,
        },
        "cache": {
            "exact_entries": state.cache.len(),
        },
    }))
}
