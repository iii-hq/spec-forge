use crate::types::*;
use iii_sdk::{III, TriggerAction, TriggerRequest};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn join_session(
    iii: &III,
    session_id: &str,
    worker_id: &str,
) -> Result<SessionInfo, Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    let peers_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({ "scope": scope, "key": "peers" }),
            action: None,
            timeout_ms: None,
        })
        .await
        .unwrap_or(json!([]));

    let mut peers: Vec<String> = serde_json::from_value(peers_val).unwrap_or_default();

    // Cap stale peers — keep only last 10 and deduplicate
    if peers.len() > 10 {
        peers = peers.into_iter().rev().take(10).collect::<Vec<_>>();
        peers.reverse();
    }

    if !peers.contains(&worker_id.to_string()) {
        peers.push(worker_id.to_string());
    }

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({ "scope": scope, "key": "peers", "value": peers }),
        action: None,
        timeout_ms: None,
    })
    .await?;

    let spec: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({ "scope": scope, "key": "spec" }),
            action: None,
            timeout_ms: None,
        })
        .await
        .unwrap_or(json!(null));

    Ok(SessionInfo {
        session_id: session_id.to_string(),
        peers,
        spec: if spec.is_null() { None } else { Some(spec) },
    })
}

pub async fn leave_session(
    iii: &III,
    session_id: &str,
    worker_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    let peers_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({ "scope": scope, "key": "peers" }),
            action: None,
            timeout_ms: None,
        })
        .await
        .unwrap_or(json!([]));

    let mut peers: Vec<String> = serde_json::from_value(peers_val).unwrap_or_default();
    peers.retain(|p| p != worker_id);

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({ "scope": scope, "key": "peers", "value": peers }),
        action: None,
        timeout_ms: None,
    })
    .await?;

    Ok(())
}

pub async fn fan_out_patch(
    iii: &III,
    session_id: &str,
    patch: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    let peers_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({ "scope": scope, "key": "peers" }),
            action: None,
            timeout_ms: None,
        })
        .await
        .unwrap_or(json!([]));

    let peers: Vec<String> = serde_json::from_value(peers_val).unwrap_or_default();
    tracing::info!("Fan-out to {} peers: {:?}", peers.len(), peers);

    for peer in &peers {
        let fn_id = format!("ui::render-patch::{}", peer);
        match iii
            .trigger(TriggerRequest {
                function_id: fn_id.clone(),
                payload: json!({ "patch": patch, "session": session_id }),
                action: Some(TriggerAction::Void),
                timeout_ms: None,
            })
            .await
        {
            Ok(_) => tracing::debug!("Fan-out to {} OK", fn_id),
            Err(e) => tracing::warn!("Fan-out to {} FAILED: {}", fn_id, e),
        }
    }

    Ok(())
}

pub async fn store_spec(
    iii: &III,
    session_id: &str,
    spec: &serde_json::Value,
    author: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let scope = format!("session::{}", session_id);

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({ "scope": scope, "key": "spec", "value": spec }),
        action: None,
        timeout_ms: None,
    })
    .await?;

    let history_val: serde_json::Value = iii
        .trigger(TriggerRequest {
            function_id: "state::get".to_string(),
            payload: json!({ "scope": scope, "key": "history" }),
            action: None,
            timeout_ms: None,
        })
        .await
        .unwrap_or(json!([]));

    let mut history: Vec<HistoryEntry> = serde_json::from_value(history_val).unwrap_or_default();

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    history.push(HistoryEntry {
        spec: spec.clone(),
        timestamp: ts,
        author: author.to_string(),
    });

    iii.trigger(TriggerRequest {
        function_id: "state::set".to_string(),
        payload: json!({ "scope": scope, "key": "history", "value": history }),
        action: None,
        timeout_ms: None,
    })
    .await?;

    Ok(())
}
