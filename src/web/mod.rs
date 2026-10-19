//! HTTP server: JSON API, live WebSocket, Prometheus metrics and the embedded web UI.

pub mod api;
pub mod auth;
pub mod health;
pub mod metrics;

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use rust_embed::RustEmbed;

use crate::state::AppState;

/// Built web UI (`web/dist`), embedded in the binary at compile time.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/web/dist"]
struct Assets;

pub fn router(state: Arc<AppState>) -> Router {
    let protected = Router::new()
        .route("/auth/password", post(api::change_password))
        .route("/status", get(api::status))
        .route("/live", get(api::live))
        .route("/events", get(api::events))
        .route("/config", get(api::get_config).put(api::put_config))
        .route("/config/reload", post(api::reload_config))
        .route("/zones/{id}/manual", put(api::set_manual))
        .route("/zones/{id}/calibration", post(api::start_calibration).delete(api::cancel_calibration))
        .route("/history", get(api::history))
        .route("/notifications/test", post(api::test_notification))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_session));

    let api = Router::new()
        .route("/auth/status", get(api::auth_status))
        .route("/auth/setup", post(api::setup))
        .route("/auth/login", post(api::login))
        .route("/auth/logout", post(api::logout))
        .merge(protected);

    Router::new().nest("/api", api).route("/metrics", get(metrics_handler)).fallback(static_handler).with_state(state)
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> Response {
    if !state.config().web.metrics_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], metrics::render(&state.snapshot())).into_response()
}

/// Serves the UI; unknown paths get `index.html` (client-side routing).
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (file, path) = match Assets::get(path).filter(|_| !path.is_empty()) {
        Some(file) => (file, path),
        None => match Assets::get("index.html") {
            Some(file) => (file, "index.html"),
            None => return (StatusCode::NOT_FOUND, "web UI not built (run `npm run build` in web/)").into_response(),
        },
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    // Vite puts a content hash in asset names: they can be cached forever.
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())], file.data)
        .into_response()
}
