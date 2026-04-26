use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};

use subtle::ConstantTimeEq;

use crate::config::AuthMode;

/// Shared authentication state, derived from [`Config`](crate::config::Config).
#[derive(Debug, Clone)]
pub struct AuthState {
    pub mode: AuthMode,
    pub tokens: Vec<String>,
}

/// Axum middleware that enforces bearer-token authentication.
///
/// Rules:
/// - In **Relaxed** mode, no configured tokens means auth is disabled (local dev mode).
/// - `/health` is always exempt.
/// - In **Relaxed** mode, only write methods (POST, PATCH, DELETE) require a token.
/// - In **Strict** mode, all methods require a token.
pub async fn auth_middleware(
    State(auth): State<Arc<AuthState>>,
    request: Request,
    next: Next,
) -> Response {
    // /health is always exempt
    if request.uri().path() == "/health" {
        return next.run(request).await;
    }

    if auth.tokens.is_empty() {
        if matches!(auth.mode, AuthMode::Relaxed) {
            return next.run(request).await;
        }

        tracing::error!(
            mode = ?auth.mode,
            "strict auth mode configured without any API tokens"
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "server_misconfigured",
                "message": "Strict auth mode requires at least one API token"
            })),
        )
            .into_response();
    }

    let needs_auth = match auth.mode {
        AuthMode::Strict => true,
        AuthMode::Relaxed => matches!(
            *request.method(),
            Method::POST | Method::PATCH | Method::DELETE | Method::PUT
        ),
    };

    if !needs_auth {
        return next.run(request).await;
    }

    // Extract and validate bearer token
    let token_valid = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|token| {
            auth.tokens
                .iter()
                .any(|t| bool::from(t.as_bytes().ct_eq(token.as_bytes())))
        })
        .unwrap_or(false);

    if !token_valid {
        tracing::warn!(
            method = %request.method(),
            path = %request.uri().path(),
            "auth failure: invalid or missing bearer token"
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "message": "Invalid or missing bearer token"
            })),
        )
            .into_response();
    }

    next.run(request).await
}
