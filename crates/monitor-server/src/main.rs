mod adapters;
mod api;
mod auth;
mod config;
mod db;
mod services;
mod web;

use std::sync::Arc;

use auth::AuthState;
use config::Config;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("monitor_server=info,tower_http=warn")),
        )
        .init();

    // Load config
    let config = Config::from_env();
    tracing::info!(
        bind = %config.bind_address,
        auth_mode = ?config.auth_mode,
        tokens_configured = config.api_tokens.len(),
        "loaded configuration"
    );
    tracing::info!("starting monitor-server");

    // Init DB
    let pool = db::init_db(&config.database_url).await.unwrap();
    tracing::info!(db = %config.database_url, "database initialized");
    let db = db::Db::new(pool);
    let service = Arc::new(services::AppService::new(db));
    let shutdown_service = service.clone();

    // Build auth state
    let auth_state = Arc::new(AuthState {
        mode: config.auth_mode.clone(),
        tokens: config.api_tokens.clone(),
    });

    // Build router
    let app = api::router(service, auth_state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.bind_address)
        .await
        .unwrap();
    let local_addr = listener.local_addr().unwrap();
    tracing::info!(
        bind = %config.bind_address,
        address = %local_addr,
        dashboard = %format!("http://{local_addr}/dashboard"),
        stream = %format!("http://{local_addr}/stream"),
        "monitor-server listening"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown_service.shutdown();
        })
        .await
        .unwrap();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
    }
    tracing::info!("shutdown signal received, stopping gracefully");
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod integration_tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{self, Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::{api, auth::AuthState, config::AuthMode, db, services};

    async fn app() -> axum::Router {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let db = db::Db::new(pool);
        let service = Arc::new(services::AppService::new(db));
        // No tokens → auth disabled (local dev mode) so existing tests pass.
        let auth_state = Arc::new(AuthState {
            mode: AuthMode::Relaxed,
            tokens: vec![],
        });
        api::router(service, auth_state)
    }

    async fn body_json(body: Body) -> Value {
        let bytes = body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn health_check() {
        let app = app().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn create_and_list_workstreams() {
        let app = app().await;

        // Create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "Test WS"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let ws = body_json(resp.into_body()).await;
        assert_eq!(ws["name"], "Test WS");
        assert_eq!(ws["status"], "active");
        let ws_id = ws["id"].as_str().unwrap().to_string();

        // List
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["total"], 1);
        assert_eq!(body["items"][0]["id"], ws_id);
    }

    #[tokio::test]
    async fn patch_workstream() {
        let app = app().await;

        // Create
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "WS1"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ws = body_json(resp.into_body()).await;
        let ws_id = ws["id"].as_str().unwrap().to_string();

        // Patch
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::PATCH)
                    .uri(format!("/api/workstreams/{ws_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"name": "WS1 Updated", "status": "archived"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["name"], "WS1 Updated");
        assert_eq!(body["status"], "archived");
    }

    #[tokio::test]
    async fn create_and_list_tasks() {
        let app = app().await;

        // Create workstream first
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "WS"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ws = body_json(resp.into_body()).await;
        let ws_id = ws["id"].as_str().unwrap().to_string();

        // Create task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"workstream_id": ws_id, "name": "Task 1"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let task = body_json(resp.into_body()).await;
        assert_eq!(task["name"], "Task 1");
        assert_eq!(task["status"], "active");

        // List tasks
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks?workstream_id={ws_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["total"], 1);
        assert_eq!(body["items"][0]["name"], "Task 1");
    }

    #[tokio::test]
    async fn patch_task() {
        let app = app().await;

        // Create workstream + task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "WS"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ws = body_json(resp.into_body()).await;
        let ws_id = ws["id"].as_str().unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"workstream_id": ws_id, "name": "T1"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let task = body_json(resp.into_body()).await;
        let task_id = task["id"].as_str().unwrap().to_string();

        // Patch
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::PATCH)
                    .uri(format!("/api/tasks/{task_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"status": "done", "name": "T1 Done"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["status"], "done");
        assert_eq!(body["name"], "T1 Done");
    }

    #[tokio::test]
    async fn ingest_manual_update_and_list() {
        let app = app().await;

        // Setup: workstream + task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "WS"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ws = body_json(resp.into_body()).await;
        let ws_id = ws["id"].as_str().unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"workstream_id": ws_id, "name": "Task"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let task = body_json(resp.into_body()).await;
        let task_id = task["id"].as_str().unwrap().to_string();

        // Ingest manual update
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/updates/manual")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "task_id": task_id,
                            "message": "Made progress",
                            "tags": ["v1"]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let update = body_json(resp.into_body()).await;
        assert_eq!(update["source"], "manual");
        assert_eq!(update["message"], "Made progress");

        // List updates
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/updates?task_id={task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["total"], 1);
        assert_eq!(body["items"][0]["message"], "Made progress");
    }

    #[tokio::test]
    async fn create_task_for_nonexistent_workstream_returns_404() {
        let app = app().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "workstream_id": "00000000-0000-0000-0000-000000000000",
                            "name": "Orphan"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ingest_update_for_nonexistent_task_returns_404() {
        let app = app().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/updates/manual")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "task_id": "00000000-0000-0000-0000-000000000000",
                            "message": "Ghost"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // SSE streaming tests
    // -----------------------------------------------------------------------

    /// Helper: start a real server on a random port and return the base URL.
    async fn start_server() -> String {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let db = db::Db::new(pool);
        let service = Arc::new(services::AppService::new(db));
        let auth_state = Arc::new(AuthState {
            mode: AuthMode::Relaxed,
            tokens: vec![],
        });
        let app = api::router(service, auth_state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn sse_stream_receives_live_update() {
        let base = start_server().await;
        let client = reqwest::Client::new();

        // Create workstream
        let ws: Value = client
            .post(format!("{base}/api/workstreams"))
            .json(&json!({"name": "SSE WS"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let ws_id = ws["id"].as_str().unwrap().to_string();

        // Create task
        let task: Value = client
            .post(format!("{base}/api/tasks"))
            .json(&json!({"workstream_id": ws_id, "name": "SSE Task"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();

        // Start SSE connection in background
        let base2 = base.clone();
        let client2 = client.clone();
        let sse_handle = tokio::spawn(async move {
            use futures::StreamExt;

            let resp = client2
                .get(format!("{base2}/api/stream"))
                .header("Accept", "text/event-stream")
                .send()
                .await
                .unwrap();

            let mut stream = resp.bytes_stream();
            let mut collected = String::new();

            // Read SSE data until we get an update event or timeout
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }

                match tokio::time::timeout(remaining, stream.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        let text = String::from_utf8_lossy(&bytes);
                        collected.push_str(&text);
                        if collected.contains("event: update") && collected.contains("data: ") {
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => panic!("Stream error: {e}"),
                    Ok(None) => break,
                    Err(_) => break,
                }
            }

            collected
        });

        // Give the SSE connection a moment to establish
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Post an update
        let update: Value = client
            .post(format!("{base}/api/updates/manual"))
            .json(&json!({
                "task_id": task_id,
                "message": "SSE test update"
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let sse_output = sse_handle.await.unwrap();

        // Verify the SSE output contains the update
        assert!(
            sse_output.contains("event: update"),
            "Expected 'event: update' in SSE output, got: {sse_output}"
        );
        assert!(
            sse_output.contains("SSE test update"),
            "Expected update message in SSE output, got: {sse_output}"
        );

        // Verify the id field is set to the seq
        let seq = update["seq"].as_i64().unwrap();
        assert!(
            sse_output.contains(&format!("id: {seq}")),
            "Expected 'id: {seq}' in SSE output, got: {sse_output}"
        );
    }

    #[tokio::test]
    async fn sse_stream_catchup_via_after_seq() {
        let base = start_server().await;
        let client = reqwest::Client::new();

        // Create workstream + task
        let ws: Value = client
            .post(format!("{base}/api/workstreams"))
            .json(&json!({"name": "Catchup WS"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let ws_id = ws["id"].as_str().unwrap().to_string();

        let task: Value = client
            .post(format!("{base}/api/tasks"))
            .json(&json!({"workstream_id": ws_id, "name": "Catchup Task"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let task_id = task["id"].as_str().unwrap().to_string();

        // Create two updates BEFORE connecting to SSE
        let _u1: Value = client
            .post(format!("{base}/api/updates/manual"))
            .json(&json!({"task_id": task_id, "message": "First update"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let u2: Value = client
            .post(format!("{base}/api/updates/manual"))
            .json(&json!({"task_id": task_id, "message": "Second update"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        // Connect with after_seq=0 to get all historical updates
        let base2 = base.clone();
        let client2 = client.clone();
        let sse_handle = tokio::spawn(async move {
            use futures::StreamExt;

            let resp = client2
                .get(format!("{base2}/api/stream?after_seq=0"))
                .header("Accept", "text/event-stream")
                .send()
                .await
                .unwrap();

            let mut stream = resp.bytes_stream();
            let mut collected = String::new();
            let mut update_count = 0;

            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }

                match tokio::time::timeout(remaining, stream.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        let text = String::from_utf8_lossy(&bytes);
                        collected.push_str(&text);
                        update_count = collected.matches("event: update").count();
                        if update_count >= 2 {
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => panic!("Stream error: {e}"),
                    Ok(None) => break,
                    Err(_) => break,
                }
            }

            (collected, update_count)
        });

        let (sse_output, count) = sse_handle.await.unwrap();

        // Should have received both historical updates
        assert!(
            count >= 2,
            "Expected at least 2 catch-up events, got {count}. Output: {sse_output}"
        );
        assert!(sse_output.contains("First update"));
        assert!(sse_output.contains("Second update"));

        // Verify the seq id for the second update
        let seq2 = u2["seq"].as_i64().unwrap();
        assert!(sse_output.contains(&format!("id: {seq2}")));
    }

    #[tokio::test]
    async fn sse_stream_filters_by_task_id() {
        let base = start_server().await;
        let client = reqwest::Client::new();

        // Create workstream + two tasks
        let ws: Value = client
            .post(format!("{base}/api/workstreams"))
            .json(&json!({"name": "Filter WS"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let ws_id = ws["id"].as_str().unwrap().to_string();

        let task_a: Value = client
            .post(format!("{base}/api/tasks"))
            .json(&json!({"workstream_id": ws_id, "name": "Task A"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let task_a_id = task_a["id"].as_str().unwrap().to_string();

        let task_b: Value = client
            .post(format!("{base}/api/tasks"))
            .json(&json!({"workstream_id": ws_id, "name": "Task B"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let task_b_id = task_b["id"].as_str().unwrap().to_string();

        // Start SSE filtered to task_a only
        let base2 = base.clone();
        let client2 = client.clone();
        let task_a_id2 = task_a_id.clone();
        let sse_handle = tokio::spawn(async move {
            use futures::StreamExt;

            let resp = client2
                .get(format!("{base2}/api/stream?task_id={task_a_id2}"))
                .header("Accept", "text/event-stream")
                .send()
                .await
                .unwrap();

            let mut stream = resp.bytes_stream();
            let mut collected = String::new();

            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }

                match tokio::time::timeout(remaining, stream.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        let text = String::from_utf8_lossy(&bytes);
                        collected.push_str(&text);
                        if collected.contains("Task A update") {
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => panic!("Stream error: {e}"),
                    Ok(None) => break,
                    Err(_) => break,
                }
            }

            collected
        });

        // Give SSE connection time to establish
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Post update to task_b (should be filtered out)
        client
            .post(format!("{base}/api/updates/manual"))
            .json(&json!({
                "task_id": task_b_id,
                "message": "Task B update"
            }))
            .send()
            .await
            .unwrap();

        // Post update to task_a (should come through)
        client
            .post(format!("{base}/api/updates/manual"))
            .json(&json!({
                "task_id": task_a_id,
                "message": "Task A update"
            }))
            .send()
            .await
            .unwrap();

        let sse_output = sse_handle.await.unwrap();

        assert!(
            sse_output.contains("Task A update"),
            "Expected Task A update in output, got: {sse_output}"
        );
        assert!(
            !sse_output.contains("Task B update"),
            "Task B update should have been filtered out, got: {sse_output}"
        );
    }

    #[tokio::test]
    async fn list_workstreams_include_archived() {
        let app = app().await;

        // Create and archive a workstream
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "WS"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ws = body_json(resp.into_body()).await;
        let ws_id = ws["id"].as_str().unwrap().to_string();

        app.clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::PATCH)
                    .uri(format!("/api/workstreams/{ws_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"status": "archived"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Default list (exclude archived)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["total"], 0);

        // Include archived
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams?include_archived=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["total"], 1);
    }

    // -----------------------------------------------------------------------
    // Claude hook adapter integration tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn ingest_claude_hook_post_tool_use() {
        let app = app().await;

        // Setup: workstream + task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "WS"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ws = body_json(resp.into_body()).await;
        let ws_id = ws["id"].as_str().unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"workstream_id": ws_id, "name": "Task"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let task = body_json(resp.into_body()).await;
        let task_id = task["id"].as_str().unwrap().to_string();

        // Ingest Claude hook
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/updates/claude-hook")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "task_id": task_id,
                            "payload": {
                                "session_id": "sess-abc",
                                "hook_event_name": "PostToolUse",
                                "tool_name": "Write",
                                "tool_input": {"file_path": "/tmp/test.txt"},
                                "tool_response": {"success": true},
                                "tool_use_id": "toolu_01"
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let update = body_json(resp.into_body()).await;
        assert_eq!(update["source"], "claude:sess-abc");
        assert_eq!(update["message"], "Tool `Write` completed");
        assert_eq!(update["kind"], "tool_use");
        assert_eq!(update["level"], "info");
        assert!(update["data"].is_object());
    }

    #[tokio::test]
    async fn ingest_claude_hook_for_nonexistent_task_returns_404() {
        let app = app().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/updates/claude-hook")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "task_id": "00000000-0000-0000-0000-000000000000",
                            "payload": {
                                "session_id": "sess-abc",
                                "hook_event_name": "PostToolUse",
                                "tool_name": "Bash"
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // -----------------------------------------------------------------------
    // GitHub webhook adapter integration tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn ingest_github_webhook_workflow_run() {
        let app = app().await;

        // Setup: workstream + task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"name": "WS"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let ws = body_json(resp.into_body()).await;
        let ws_id = ws["id"].as_str().unwrap();

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"workstream_id": ws_id, "name": "Task"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let task = body_json(resp.into_body()).await;
        let task_id = task["id"].as_str().unwrap().to_string();

        // Ingest GitHub webhook
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/updates/github-webhook")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "task_id": task_id,
                            "headers": {"x-github-event": "workflow_run"},
                            "payload": {
                                "action": "completed",
                                "repository": {"full_name": "org/repo"},
                                "sender": {"login": "octocat"},
                                "workflow_run": {
                                    "name": "CI",
                                    "conclusion": "success",
                                    "status": "completed"
                                }
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let update = body_json(resp.into_body()).await;
        assert_eq!(update["source"], "github:org/repo");
        assert_eq!(update["message"], "Workflow 'CI' completed");
        assert_eq!(update["kind"], "ci_run");
        assert_eq!(update["level"], "info");
        assert!(update["data"].is_object());
    }

    #[tokio::test]
    async fn ingest_github_webhook_for_nonexistent_task_returns_404() {
        let app = app().await;

        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/updates/github-webhook")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "task_id": "00000000-0000-0000-0000-000000000000",
                            "headers": {"x-github-event": "workflow_run"},
                            "payload": {
                                "action": "completed",
                                "repository": {"full_name": "org/repo"},
                                "workflow_run": {"name": "CI", "conclusion": "success"}
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}

// ---------------------------------------------------------------------------
// Auth middleware tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod auth_tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{self, Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::{api, auth::AuthState, config::AuthMode, db, services};

    async fn body_json(body: Body) -> Value {
        let bytes = body.collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn app_with_auth(mode: AuthMode, tokens: Vec<String>) -> axum::Router {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let db = db::Db::new(pool);
        let service = Arc::new(services::AppService::new(db));
        let auth_state = Arc::new(AuthState { mode, tokens });
        api::router(service, auth_state)
    }

    // -----------------------------------------------------------------------
    // No tokens configured:
    // - Relaxed mode passes (local dev mode)
    // - Strict mode fails closed as server misconfiguration
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn no_tokens_relaxed_get_passes() {
        let app = app_with_auth(AuthMode::Relaxed, vec![]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn no_tokens_relaxed_post_passes() {
        let app = app_with_auth(AuthMode::Relaxed, vec![]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"name": "Test"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn no_tokens_strict_get_fails_closed() {
        let app = app_with_auth(AuthMode::Strict, vec![]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn no_tokens_strict_post_fails_closed() {
        let app = app_with_auth(AuthMode::Strict, vec![]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"name": "Test"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // -----------------------------------------------------------------------
    // Relaxed mode: GET open, writes require token
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn relaxed_get_without_token_succeeds() {
        let app = app_with_auth(AuthMode::Relaxed, vec!["secret-token".to_string()]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn relaxed_post_without_token_returns_401() {
        let app = app_with_auth(AuthMode::Relaxed, vec!["secret-token".to_string()]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({"name": "Test"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["error"], "unauthorized");
    }

    #[tokio::test]
    async fn relaxed_post_with_valid_token_succeeds() {
        let app = app_with_auth(AuthMode::Relaxed, vec!["secret-token".to_string()]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer secret-token")
                    .body(Body::from(serde_json::json!({"name": "Test"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // -----------------------------------------------------------------------
    // Strict mode: all endpoints require token
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn strict_get_without_token_returns_401() {
        let app = app_with_auth(AuthMode::Strict, vec!["secret-token".to_string()]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn strict_get_with_valid_token_succeeds() {
        let app = app_with_auth(AuthMode::Strict, vec!["secret-token".to_string()]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/workstreams")
                    .header("authorization", "Bearer secret-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // Invalid token → 401
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn invalid_token_returns_401() {
        let app = app_with_auth(AuthMode::Relaxed, vec!["secret-token".to_string()]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer wrong-token")
                    .body(Body::from(serde_json::json!({"name": "Test"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = body_json(resp.into_body()).await;
        assert_eq!(body["error"], "unauthorized");
        assert_eq!(body["message"], "Invalid or missing bearer token");
    }

    // -----------------------------------------------------------------------
    // /health always exempt
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn health_always_exempt_in_strict_mode() {
        let app = app_with_auth(AuthMode::Strict, vec!["secret-token".to_string()]).await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // Multiple tokens: any valid token works
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn multiple_tokens_any_valid_works() {
        let app = app_with_auth(
            AuthMode::Relaxed,
            vec!["token-a".to_string(), "token-b".to_string()],
        )
        .await;

        // token-b should work
        let resp = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/api/workstreams")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer token-b")
                    .body(Body::from(serde_json::json!({"name": "Test"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
}

// ---------------------------------------------------------------------------
// Smoke test — full happy-path end-to-end
// ---------------------------------------------------------------------------

#[cfg(test)]
mod smoke_test {
    use std::sync::Arc;

    use serde_json::{json, Value};

    use crate::{api, auth::AuthState, config::AuthMode, db, services};

    /// Start a real server on a random port and return the base URL.
    async fn start_server() -> String {
        let pool = db::init_db("sqlite::memory:").await.unwrap();
        let db = db::Db::new(pool);
        let service = Arc::new(services::AppService::new(db));
        let auth_state = Arc::new(AuthState {
            mode: AuthMode::Relaxed,
            tokens: vec![],
        });
        let app = api::router(service, auth_state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn full_happy_path() {
        let base = start_server().await;
        let client = reqwest::Client::new();

        // ---------------------------------------------------------------
        // 1. Create workstream
        // ---------------------------------------------------------------
        let ws: Value = client
            .post(format!("{base}/api/workstreams"))
            .json(&json!({"name": "Smoke Test WS"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(ws["name"], "Smoke Test WS");
        assert_eq!(ws["status"], "active");
        let ws_id = ws["id"].as_str().unwrap().to_string();

        // ---------------------------------------------------------------
        // 2. Create task in workstream
        // ---------------------------------------------------------------
        let task: Value = client
            .post(format!("{base}/api/tasks"))
            .json(&json!({"workstream_id": ws_id, "name": "Smoke Task"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(task["name"], "Smoke Task");
        assert_eq!(task["status"], "active");
        assert_eq!(task["workstream_id"], ws_id);
        let task_id = task["id"].as_str().unwrap().to_string();

        // ---------------------------------------------------------------
        // 3. Start SSE connection BEFORE sending updates
        // ---------------------------------------------------------------
        let base_sse = base.clone();
        let client_sse = client.clone();
        let sse_handle = tokio::spawn(async move {
            use futures::StreamExt;

            let resp = client_sse
                .get(format!("{base_sse}/api/stream"))
                .header("Accept", "text/event-stream")
                .send()
                .await
                .unwrap();

            let mut stream = resp.bytes_stream();
            let mut collected = String::new();

            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }

                match tokio::time::timeout(remaining, stream.next()).await {
                    Ok(Some(Ok(bytes))) => {
                        let text = String::from_utf8_lossy(&bytes);
                        collected.push_str(&text);
                        // Wait until we have all 3 updates
                        if collected.matches("event: update").count() >= 3 {
                            break;
                        }
                    }
                    Ok(Some(Err(e))) => panic!("Stream error: {e}"),
                    Ok(None) => break,
                    Err(_) => break,
                }
            }

            collected
        });

        // Give the SSE connection time to establish
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // ---------------------------------------------------------------
        // 4. Send manual update
        // ---------------------------------------------------------------
        let manual_update: Value = client
            .post(format!("{base}/api/updates/manual"))
            .json(&json!({
                "task_id": task_id,
                "message": "Manual progress note",
                "kind": "note",
                "tags": ["smoke"]
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(manual_update["source"], "manual");
        assert_eq!(manual_update["message"], "Manual progress note");
        assert_eq!(manual_update["kind"], "note");

        // ---------------------------------------------------------------
        // 5. Send Claude hook update
        // ---------------------------------------------------------------
        let claude_update: Value = client
            .post(format!("{base}/api/updates/claude-hook"))
            .json(&json!({
                "task_id": task_id,
                "payload": {
                    "session_id": "sess-smoke",
                    "hook_event_name": "PostToolUse",
                    "tool_name": "Bash",
                    "tool_input": {"command": "echo hello"},
                    "tool_response": {"stdout": "hello"},
                    "tool_use_id": "toolu_smoke"
                }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(claude_update["source"], "claude:sess-smoke");
        assert_eq!(claude_update["kind"], "tool_use");

        // ---------------------------------------------------------------
        // 6. Send GitHub webhook update
        // ---------------------------------------------------------------
        let github_update: Value = client
            .post(format!("{base}/api/updates/github-webhook"))
            .json(&json!({
                "task_id": task_id,
                "headers": {"x-github-event": "workflow_run"},
                "payload": {
                    "action": "completed",
                    "repository": {"full_name": "org/smoke-repo"},
                    "sender": {"login": "smokebot"},
                    "workflow_run": {
                        "name": "CI",
                        "conclusion": "success",
                        "status": "completed"
                    }
                }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(github_update["source"], "github:org/smoke-repo");
        assert_eq!(github_update["kind"], "ci_run");

        // ---------------------------------------------------------------
        // 7. Query state — list workstreams, tasks, updates
        // ---------------------------------------------------------------
        let workstreams: Value = client
            .get(format!("{base}/api/workstreams"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(workstreams["total"], 1);
        assert_eq!(workstreams["items"][0]["name"], "Smoke Test WS");

        let tasks: Value = client
            .get(format!("{base}/api/tasks?workstream_id={ws_id}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(tasks["total"], 1);
        assert_eq!(tasks["items"][0]["name"], "Smoke Task");

        let updates: Value = client
            .get(format!("{base}/api/updates?task_id={task_id}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(updates["total"], 3);

        // ---------------------------------------------------------------
        // 8. Verify update filtering works
        // ---------------------------------------------------------------
        let filtered: Value = client
            .get(format!(
                "{base}/api/updates?task_id={task_id}&source=manual"
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(filtered["total"], 1);
        assert_eq!(filtered["items"][0]["source"], "manual");

        let filtered_kind: Value = client
            .get(format!("{base}/api/updates?task_id={task_id}&kind=ci_run"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(filtered_kind["total"], 1);
        assert_eq!(filtered_kind["items"][0]["kind"], "ci_run");

        // ---------------------------------------------------------------
        // 9. Verify SSE stream received the events
        // ---------------------------------------------------------------
        let sse_output = sse_handle.await.unwrap();
        let update_count = sse_output.matches("event: update").count();
        assert!(
            update_count >= 3,
            "Expected at least 3 SSE events, got {update_count}. Output: {sse_output}"
        );
        assert!(
            sse_output.contains("Manual progress note"),
            "SSE should contain manual update"
        );
        assert!(
            sse_output.contains("claude:sess-smoke"),
            "SSE should contain claude hook update"
        );
        assert!(
            sse_output.contains("github:org/smoke-repo"),
            "SSE should contain github webhook update"
        );

        // ---------------------------------------------------------------
        // 10. Verify after_seq filtering on updates
        // ---------------------------------------------------------------
        let first_seq = manual_update["seq"].as_i64().unwrap();
        let after: Value = client
            .get(format!(
                "{base}/api/updates?task_id={task_id}&after_seq={first_seq}"
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            after["total"], 2,
            "Should have 2 updates after seq {first_seq}"
        );
    }
}
