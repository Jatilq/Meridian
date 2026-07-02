// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Meridian — Phase 11 day-4+ : Lemonade model-management commands.
//!
//! Drives Lemonade's native HTTP API for model acquisition + lifecycle:
//!   POST /v1/pull, POST /v1/load, POST /v1/unload, POST /v1/delete,
//!   GET /v1/models, GET /v1/downloads, GET /v1/health, GET /v1/system-info.
//!
//! Distinction from `lemonade_extras.rs`:
//!   * `lemonade_extras.rs` is the **inference** path — TTS / STT / image queries
//!     against the OpenAI-compat endpoints (`/v1/audio/speech`,
//!     `/v1/audio/transcriptions`, `/v1/chat/completions`). Tier-1 backend for Rain.
//!   * `lemonade_manager.rs` is the **management** path — install / register /
//!     load / unload / delete / list models + server health + auto-launch. Drives
//!     Lemonade backend lifecycle from a single binary.
//!
//! Both modules issue HTTP requests against a base URL resolved from a
//! caller-supplied `endpoint`. When `endpoint` is `None` / empty, the base
//! defaults to `http://localhost:13305` (the port that `lemonade_extras.rs`
//! defaults to AND the port JC's actual local install binds). Configured
//! paths flow through `meridian.backend.lemonade.installDir` + `backendPort`.
//!
//! Current state of this module (day-4 Commit 2 — Rust framework):
//!   * `resolve_lemonade_base` helper (5-line `resolve_base` analog)
//!   * `LemonadeHealth` response shape (status + loaded_models)
//!   * `lemonade_get_health` command (GET /v1/health)
//!   * Two unit tests covering the default + trailing-slash + `/v1` cases
//! More commands land in subsequent commits.

use std::time::Duration;
use serde::{Serialize, Deserialize};

/// Default Lemonade OpenAI-compatible base URL. Matches:
///   * `backend_manager::BackendKind::Lemonade::default_port()` resolution
///   * `lemonade_extras.rs::DEFAULT_LEMONADE_BASE` (13305 on JC's install)
///   * the catalog row in `src/data/backends.json` (`lemonade.embeddable.windows-x64`)
///
/// Env override `LEMONADE_PORT` (default `11434`) is honored upstream by
/// lemonade-server itself but Meridian deliberately pins to **13305** to
/// keep one port across all modules — changing would churn the AI Panel
/// `localEndpointUrl` default + every frontend consumer.
const DEFAULT_LEMONADE_BASE: &str = "http://localhost:13305";

/// Resolve the base URL the Lemonade server is listening on. Priority:
/// 1. Caller-supplied `endpoint` (strips trailing `/v1` + `/` + whitespace)
/// 2. Hard-coded fallback to `DEFAULT_LEMONADE_BASE`
///
/// The endpoint-takes-precedence path mirrors `lemonade_extras::resolve_base`
/// exactly so that consumers can swap between the inference and management
/// modules without re-stripping URL prefixes. If a follow-up commit wants
/// to introduce a config-driven base (e.g. read `meridian.backend.lemonade`
/// from the lazy store), the cheapest insertion point is here.
fn resolve_lemonade_base(endpoint: Option<&str>) -> String {
    let raw = endpoint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LEMONADE_BASE);
    let no_slash = raw.trim_end_matches('/');
    let no_v1 = no_slash.trim_end_matches("/v1");
    no_v1.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LemonadeHealth {
    pub status: String,
    pub loaded_models: Vec<String>,
}

/// Probe the Lemonade server's `/v1/health` endpoint. Returns current
/// server status + the list of currently-loaded model identifiers so the
/// Vue "Lemonade Models" sidebar entry can badge loaded rows in the
/// registered-models table.
#[tauri::command]
pub async fn lemonade_get_health(endpoint: Option<String>) -> Result<LemonadeHealth, String> {
    let base = resolve_lemonade_base(endpoint.as_deref());
    let url = format!("{}/v1/health", base);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Lemonade /v1/health failed: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("Lemonade /v1/health returned status {}", response.status()));
    }
    let raw = response.text().await.map_err(|e| format!("Failed to read Lemonade /v1/health response: {}", e))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse Lemonade /v1/health response: {}", e))?;
    let status = parsed
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let loaded_models: Vec<String> = parsed
        .get("loaded_models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(LemonadeHealth { status, loaded_models })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_lemonade_base_defaults_to_localhost_13305() {
        assert_eq!(resolve_lemonade_base(None), "http://localhost:13305");
        assert_eq!(resolve_lemonade_base(Some("")), "http://localhost:13305");
        assert_eq!(resolve_lemonade_base(Some("   ")), "http://localhost:13305");
    }

    #[test]
    fn resolve_lemonade_base_strips_trailing_slash_and_v1() {
        // The exact cases a Vue caller is likely to produce if they mirror
        // `aiPanelStore.localEndpointUrl` (always emits `/v1` suffix) and
        // then forget to strip it. `resolve_lemonade_base` should normalize
        // them all to the same bare base.
        assert_eq!(
            resolve_lemonade_base(Some("http://localhost:13305/")),
            "http://localhost:13305"
        );
        assert_eq!(
            resolve_lemonade_base(Some("http://localhost:13305/v1")),
            "http://localhost:13305"
        );
        assert_eq!(
            resolve_lemonade_base(Some("http://localhost:13305/v1/")),
            "http://localhost:13305"
        );
        assert_eq!(
            resolve_lemonade_base(Some("  http://localhost:13305/v1  ")),
            "http://localhost:13305"
        );
    }
}
