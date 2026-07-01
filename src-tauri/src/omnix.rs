// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::fs;
use tauri::Manager;

static OMNIX_CHILD: Mutex<Option<Child>> = Mutex::new(None);
/// True while a background spawn is in progress. Prevents duplicate spawns
/// and lets the frontend return immediately instead of blocking on npm install.
static OMNIX_SPAWNING: AtomicBool = AtomicBool::new(false);

/// Default install directory for the Omnix engine.
const DEFAULT_OMNIX_DIR: &str = "E:\\ai\\Apps\\Omnix";

/// Flag file to indicate npm install has been run for bundled Omnix.
const OMNIX_NPM_DONE_MARKER: &str = ".meridian-npm-install-done";

/// Check if bundled Omnix has node_modules (npm install already done).
fn omnix_npm_done(dir: &Path) -> bool {
    dir.join(OMNIX_NPM_DONE_MARKER).exists()
}

/// Mark that npm install has been run for bundled Omnix.
fn mark_omnix_npm_done(dir: &Path) -> Result<(), String> {
    fs::write(dir.join(OMNIX_NPM_DONE_MARKER), "")
        .map_err(|e| format!("Failed to create npm-done marker: {}", e))
}

/// Resolve the Omnix project directory. If it doesn't exist, extract from
/// bundled resources first. Returns an error if extraction fails or the
/// entry point is missing.
fn resolve_omnix_dir(app: &tauri::AppHandle, omnix_path: Option<String>) -> Result<PathBuf, String> {
    let target_dir = PathBuf::from(omnix_path
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OMNIX_DIR.to_string()));

    // If target exists and has server.ts, we're done.
    if target_dir.join("server.ts").exists() {
        return Ok(target_dir);
    }

    // Need to extract from bundled resources.
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource dir: {}", e))?
        .join("omnix");

    if !resource_dir.join("server.ts").exists() {
        return Err(format!(
            "Omnix not found at {} and bundled resources missing server.ts",
            target_dir.display()
        ));
    }

    // Copy bundled Omnix to target directory.
    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Failed to create {}: {}", target_dir.display(), e))?;

    copy_dir_recursive(&resource_dir, &target_dir)
        .map_err(|e| format!("Failed to extract Omnix: {}", e))?;

    Ok(target_dir)
}

/// Recursively copy a directory (preserves contents, overwrites if exists).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in fs::read_dir(src)
        .map_err(|e| format!("Failed to read {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)
                .map_err(|e| format!("Failed to create dir {}: {}", dst_path.display(), e))?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {} to {}: {}", src_path.display(), dst_path.display(), e))?;
        }
    }
    Ok(())
}

/// Non-blocking spawn: returns immediately so the frontend can start
/// polling `get_omnix_status`. All heavy work (npm install, electron
/// extraction, process spawn) runs on a background thread.
#[tauri::command]
pub async fn spawn_omnix(app: tauri::AppHandle, omnix_path: Option<String>) -> Result<(), String> {
    // Already running? Nothing to do.
    {
        let guard = OMNIX_CHILD.lock().map_err(|e| format!("Mutex error: {}", e))?;
        if guard.is_some() {
            log::info!("[omnix] already running, skipping spawn");
            return Ok(());
        }
    }

    // A background spawn is already in progress? Return immediately so the
    // frontend can start its health-poll loop.
    if OMNIX_SPAWNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        log::info!("[omnix] spawn already in progress, returning immediately");
        return Ok(());
    }

    log::info!("[omnix] spawn_omnix called with path={:?} (non-blocking)", omnix_path);

    // All heavy work on a blocking thread — the frontend never waits for this.
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> Result<PathBuf, String> {
            let dir = resolve_omnix_dir(&app, omnix_path.clone())?;
            log::info!("[omnix] resolved dir: {}", dir.display());

            // npm install if needed. On Windows, `Command::new("npm")` often
            // fails because npm is a .cmd shim. Wrapping in `cmd /c` is reliable.
            let has_node_modules = dir.join("node_modules").exists();
            let needs_npm = !has_node_modules || !omnix_npm_done(&dir);
            log::info!("[omnix] needs_npm={} (has_node_modules={}, marker={})", needs_npm, has_node_modules, omnix_npm_done(&dir));

            if needs_npm && !has_node_modules {
                // Only run npm install when node_modules is completely missing.
                // If node_modules exists but the marker is gone, just re-create
                // the marker (the deps are already installed).
                log::info!("[omnix] running npm install in {} (background thread)", dir.display());
                let npm_result = std::process::Command::new("cmd")
                    .current_dir(&dir)
                    .args(["/c", "npm", "install"])
                    .output()
                    .map_err(|e| format!("Failed to run npm install: {}", e))?;

                if !npm_result.status.success() {
                    let stderr = String::from_utf8_lossy(&npm_result.stderr);
                    log::error!("[omnix] npm install failed: {}", stderr);
                    return Err(format!("npm install failed: {}", stderr));
                }
                log::info!("[omnix] npm install succeeded");
                mark_omnix_npm_done(&dir)?;
            } else if needs_npm && has_node_modules {
                // node_modules exists but marker is missing — just mark it done.
                log::info!("[omnix] node_modules exists but marker missing, creating marker");
                mark_omnix_npm_done(&dir)?;
            }

            // Check electron binary
            let electron = Path::new("node_modules")
                .join("electron")
                .join("dist")
                .join("electron.exe");
            let electron_path = dir.join(&electron);
            log::info!("[omnix] electron binary: {} (exists={})", electron_path.display(), electron_path.exists());
            if !electron_path.exists() {
                log::error!("[omnix] Electron binary not found at {}", electron_path.display());
                return Err(format!(
                    "Omnix Electron runtime not installed at {}. npm install may have failed.",
                    dir.display()
                ));
            }

            // Spawn the Electron process
            let mut guard = OMNIX_CHILD.lock().map_err(|e| format!("Mutex error: {}", e))?;
            if guard.is_some() {
                log::info!("[omnix] already running (child exists in lock), skipping spawn");
                return Ok(dir);
            }
            log::info!("[omnix] spawning electron from {}", electron_path.display());
            let child = std::process::Command::new(&electron_path)
                .current_dir(&dir)
                .arg(".")
                .spawn()
                .map_err(|e| {
                    log::error!("[omnix] spawn failed: {}", e);
                    format!("Failed to spawn omnix: {}", e)
                })?;
            log::info!("[omnix] spawned successfully, pid={}", child.id());
            *guard = Some(child);
            Ok(dir)
        })();

        OMNIX_SPAWNING.store(false, Ordering::SeqCst);
        match &result {
            Ok(dir) => log::info!("[omnix] background spawn complete for {}", dir.display()),
            Err(e) => log::error!("[omnix] background spawn failed: {}", e),
        }
        // Error is NOT propagated — the frontend doesn't await this. It relies
        // on `get_omnix_status` to detect success/failure via the health endpoint.
    });

    Ok(())
}

#[tauri::command]
pub fn kill_omnix() -> Result<(), String> {
    let mut guard = OMNIX_CHILD.lock().map_err(|e| format!("Mutex error: {}", e))?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_omnix_status() -> Result<bool, String> {
    match reqwest::get("http://localhost:9777/api/health").await {
        Ok(response) => {
            let ok = response.status().is_success();
            if !ok {
                log::debug!("[omnix] health check returned status {}", response.status());
            }
            Ok(ok)
        }
        Err(e) => {
            log::trace!("[omnix] health check failed: {}", e);
            Ok(false)
        }
    }
}

/// Send an image file to Omnix's vision endpoint as multipart/form-data
/// (the contract /api/vision requires). Returns the model's text response.
#[tauri::command]
pub async fn omnix_vision(image_path: String, prompt: Option<String>) -> Result<String, String> {
    let bytes = std::fs::read(&image_path)
        .map_err(|e| format!("Failed to read image {}: {}", image_path, e))?;
    let file_name = Path::new(&image_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());

    let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
    let mut form = reqwest::multipart::Form::new().part("image", part);
    if let Some(p) = prompt {
        form = form.text("prompt", p);
    }

    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:9777/api/vision")
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Omnix vision request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Omnix vision returned status {}", response.status()));
    }
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read Omnix vision response: {}", e))?;
    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse Omnix vision response: {}", e))?;
    Ok(json
        .get("response")
        .and_then(|v| v.as_str())
        .unwrap_or("No response received.")
        .to_string())
}

/// Synthesize speech via Omnix TTS (Kokoro). Returns the raw float audio
/// samples as JSON. `voice_id` defaults to "af_heart" when omitted.
#[tauri::command]
pub async fn omnix_tts(text: String, voice_id: Option<String>) -> Result<String, String> {
    let voice = voice_id.filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "af_heart".to_string());
    let body = serde_json::json!({ "text": text, "voiceId": voice }).to_string();
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:9777/api/tts")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("Omnix TTS request failed: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("Omnix TTS returned status {}", response.status()));
    }
    response
        .text()
        .await
        .map_err(|e| format!("Failed to read Omnix TTS response: {}", e))
}

/// Classify intent via Omnix Director. Returns the raw JSON routing decision
/// (simple vs complex) so the caller can pick the local AI server target model.
#[tauri::command]
pub async fn omnix_director(prompt: String) -> Result<String, String> {
    let body = serde_json::json!({ "prompt": prompt }).to_string();
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:9777/api/director")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("Omnix Director request failed: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("Omnix Director returned status {}", response.status()));
    }
    response
        .text()
        .await
        .map_err(|e| format!("Failed to read Omnix Director response: {}", e))
}

/// One row of the HF cache: a single `models--<repo>` directory with at least
/// one snapshotted blob. Returned to the Vue "Omnix Models" tab so it can mark
/// already-installed entries with an Installed badge.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledHfModel {
    pub repo_id: String,
    pub path: String,
}

/// Resolve the local HuggingFace cache directory (`%USERPROFILE%\.cache\huggingface\hub`
/// on Windows, `$HOME/.cache/huggingface/hub` elsewhere). Returns Ok(None) when the
/// directory does not yet exist — the user has not downloaded anything via HF yet.
fn huggingface_cache_dir() -> Result<Option<PathBuf>, String> {
    let base = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| "Cannot resolve USERPROFILE or HOME for HF cache".to_string())?;
    let hub = base.join(".cache").join("huggingface").join("hub");
    if !hub.exists() {
        return Ok(None);
    }
    Ok(Some(hub))
}

/// Scan the local HuggingFace cache and return one entry per installed model
/// repository. The HF Transformers convention stores repos as `models--<org>/<name>`
/// directories (the `/` in the repo ID becomes `--`); this strips that prefix so
/// the Vue tab can match it against `modelID` and show an Installed badge.
#[tauri::command]
pub fn scan_huggingface_cache() -> Result<Vec<InstalledHfModel>, String> {
    let hub = match huggingface_cache_dir()? {
        Some(p) => p,
        None => return Ok(Vec::new()),
    };
    let mut out: Vec<InstalledHfModel> = Vec::new();
    let entries = fs::read_dir(&hub)
        .map_err(|e| format!("Failed to read HF cache {}: {}", hub.display(), e))?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name_str) = p.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(rest) = name_str.strip_prefix("models--") else { continue };
        // Repo IDs contain '/'; restore from the storage form (only the first
        // `--` is the org/model separator — later ones belong to model names).
        let repo_id = rest.replacen("--", "/", 1);
        out.push(InstalledHfModel {
            repo_id,
            path: p.to_string_lossy().to_string(),
        });
    }
    out.sort_by(|a, b| a.repo_id.cmp(&b.repo_id));
    Ok(out)
}
