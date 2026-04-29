use std::convert::Infallible;
use std::time::Duration;

use axum::http::HeaderMap;
use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures::stream::Stream;
use serde::Deserialize;
use uuid::Uuid;

use monitor_common::Update;

use super::AppState;
use crate::services::LiveEvent;

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct StreamQuery {
    task_id: Option<Uuid>,
    source: Option<String>,
    kind: Option<String>,
    #[serde(default, rename = "tag")]
    tags: Vec<String>,
    after_seq: Option<i64>,
}

// ---------------------------------------------------------------------------
// Filter helper
// ---------------------------------------------------------------------------

fn matches_update_filter(update: &Update, query: &StreamQuery) -> bool {
    if let Some(tid) = query.task_id {
        if update.task_id != tid {
            return false;
        }
    }
    if let Some(ref src) = query.source {
        if update.source != *src {
            return false;
        }
    }
    if let Some(ref k) = query.kind {
        match &update.kind {
            Some(uk) if uk == k => {}
            _ => return false,
        }
    }
    if !query.tags.is_empty()
        && !update
            .tags
            .iter()
            .any(|tag| query.tags.iter().any(|wanted| wanted == tag))
    {
        return false;
    }
    true
}

fn matches_live_filter(event: &LiveEvent, query: &StreamQuery) -> bool {
    match event {
        LiveEvent::UpdateCreated(update) => matches_update_filter(update, query),
        LiveEvent::TaskCreated(task) | LiveEvent::TaskUpdated(task) => {
            if let Some(tid) = query.task_id {
                task.id == tid
            } else {
                query.source.is_none() && query.kind.is_none() && query.tags.is_empty()
            }
        }
        LiveEvent::WorkstreamCreated(_) | LiveEvent::WorkstreamUpdated(_) => {
            query.task_id.is_none()
                && query.source.is_none()
                && query.kind.is_none()
                && query.tags.is_empty()
        }
    }
}

// ---------------------------------------------------------------------------
// SSE handler
// ---------------------------------------------------------------------------

pub async fn stream(
    State(service): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StreamQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Determine the starting sequence for catch-up.
    // Prefer Last-Event-ID header (per spec); fall back to after_seq query param.
    let after_seq = headers
        .get("Last-Event-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .or(query.after_seq);

    // Subscribe to the broadcast channel BEFORE fetching historical updates
    // so we don't miss any updates between the DB read and the live stream.
    let mut rx = service.subscribe();
    let mut shutdown_rx = service.subscribe_shutdown();

    // Fetch historical catch-up updates (if requested).
    let catchup: Vec<Update> = if let Some(seq) = after_seq {
        service
            .list_updates(
                query.task_id,
                query.source.as_deref(),
                query.kind.as_deref(),
                &query.tags,
                Some(seq),
                1000,
            )
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    // Track the highest seq we've sent so we can deduplicate when switching
    // from catch-up to live.
    let catchup_overflow = catchup.len() >= 1000;
    let mut last_sent_seq: Option<i64> = catchup.last().map(|u| u.seq);

    let stream = async_stream::stream! {
        // 1. Yield catch-up events
        for update in catchup {
            let data = serde_json::to_string(&update).unwrap_or_default();
            let event = Event::default()
                .id(update.seq.to_string())
                .event("update")
                .data(data);
            yield Ok::<_, Infallible>(event);
        }

        // 1b. If catch-up hit the limit, tell the client to refetch state
        if catchup_overflow {
            let event = Event::default()
                .event("catchup_overflow")
                .data("{\"message\":\"Catch-up limit reached. Refetch current state.\"}");
            yield Ok::<_, Infallible>(event);
        }

        // 2. Switch to live broadcast
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    break;
                }
                event = rx.recv() => {
                    match event {
                        Ok(event) => {
                            if !matches_live_filter(&event, &query) {
                                continue;
                            }

                            match event {
                                LiveEvent::UpdateCreated(update) => {
                                    // Skip duplicates that were already sent during catch-up
                                    if let Some(last) = last_sent_seq {
                                        if update.seq <= last {
                                            continue;
                                        }
                                    }

                                    last_sent_seq = Some(update.seq);

                                    let data = serde_json::to_string(&update).unwrap_or_default();
                                    let event = Event::default()
                                        .id(update.seq.to_string())
                                        .event("update")
                                        .data(data);
                                    yield Ok::<_, Infallible>(event);
                                }
                                LiveEvent::TaskCreated(task) => {
                                    let data = serde_json::to_string(&task).unwrap_or_default();
                                    let event = Event::default().event("task_created").data(data);
                                    yield Ok::<_, Infallible>(event);
                                }
                                LiveEvent::TaskUpdated(task) => {
                                    let data = serde_json::to_string(&task).unwrap_or_default();
                                    let event = Event::default().event("task_updated").data(data);
                                    yield Ok::<_, Infallible>(event);
                                }
                                LiveEvent::WorkstreamCreated(workstream) => {
                                    let data = serde_json::to_string(&workstream).unwrap_or_default();
                                    let event = Event::default().event("workstream_created").data(data);
                                    yield Ok::<_, Infallible>(event);
                                }
                                LiveEvent::WorkstreamUpdated(workstream) => {
                                    let data = serde_json::to_string(&workstream).unwrap_or_default();
                                    let event = Event::default().event("workstream_updated").data(data);
                                    yield Ok::<_, Infallible>(event);
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Some messages were dropped. Inform the client via a comment-like event.
                            tracing::warn!("SSE client lagged, missed {n} messages");
                            let event = Event::default()
                                .event("lagged")
                                .data(format!("{{\"missed\":{n}}}"));
                            yield Ok::<_, Infallible>(event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            // Sender dropped — server shutting down
                            break;
                        }
                    }
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
