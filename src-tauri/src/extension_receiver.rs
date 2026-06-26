// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::{
    Json,
    response::IntoResponse,
    routing::post,
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    extract::Request,
    response::Response,
    Router,
};
use tauri::{AppHandle, Emitter};

static RECEIVER_RUNNING: AtomicBool = AtomicBool::new(false);

/// Inject permissive CORS headers on every response so the browser extension
/// (a cross-origin chrome-extension:// caller) can POST to the receiver.
async fn add_cors_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type"),
    );
    response
}

/// CORS preflight handler — browsers send OPTIONS before a cross-origin JSON
/// POST. Returns 204 so the actual POST is allowed through.
async fn handle_preflight() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

pub async fn start_extension_receiver(app: AppHandle) -> Result<u16, String> {
    if RECEIVER_RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(7771);
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], 7771));

    let app_state = Arc::new(app);

    let router = Router::new()
        .route("/download", post(handle_download).options(handle_preflight))
        .route("/ping", post(handle_ping).options(handle_preflight))
        .layer(middleware::from_fn(add_cors_headers))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind extension receiver on 7771: {}", e))?;

    let state = listener.local_addr().unwrap().port();

    tauri::async_runtime::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .ok();
        RECEIVER_RUNNING.store(false, Ordering::SeqCst);
    });

    Ok(state)
}

async fn handle_ping() -> impl IntoResponse {
    "pong"
}

async fn handle_download(
    app: axum::extract::State<Arc<AppHandle>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let url = payload
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let file_name = payload
        .get("fileName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let format_id = payload
        .get("formatId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let auto_save_folder = payload
        .get("autoSaveFolder")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let app_handle = app.as_ref().clone();

    tauri::async_runtime::spawn(async move {
        let _ = download_via_tauri(app_handle, url, file_name, format_id, auto_save_folder).await;
    });

    axum::http::StatusCode::OK
}

async fn download_via_tauri(
    app: AppHandle,
    url: String,
    file_name: Option<String>,
    format_id: Option<String>,
    auto_save_folder: Option<String>,
) {
    let _ = app.emit("extension-download-request", serde_json::json!({
        "url": url,
        "fileName": file_name,
        "formatId": format_id,
        "autoSaveFolder": auto_save_folder,
    }));
}
