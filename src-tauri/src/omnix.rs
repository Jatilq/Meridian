// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;
use std::fs;
use tauri::Manager;

static OMNIX_CHILD: Mutex<Option<Child>> = Mutex::new(None);

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

#[tauri::command]
pub fn spawn_omnix(app: tauri::AppHandle, omnix_path: Option<String>) -> Result<(), String> {
    let dir = resolve_omnix_dir(&app, omnix_path)?;

    // Check if npm install is needed (no node_modules or marker file missing)
    let needs_npm = !dir.join("node_modules").exists() || !omnix_npm_done(&dir);

    if needs_npm {
        // Run npm install to populate node_modules
        let npm_result = std::process::Command::new("npm")
            .current_dir(&dir)
            .arg("install")
            .output()
            .map_err(|e| format!("Failed to run npm install: {}", e))?;

        if !npm_result.status.success() {
            let stderr = String::from_utf8_lossy(&npm_result.stderr);
            return Err(format!("npm install failed: {}", stderr));
        }

        // Mark that we've done npm install
        mark_omnix_npm_done(&dir)?;
    }

    // Launch the Electron desktop app (hidden/standalone) rather than bare
    // `node server.ts`. Only the Electron-hosted Chromium renderer provides the
    // WebGPU compute worker that Vision/TTS require; a plain-node launch starts
    // in "Standalone mode" with no relay and every request fails with
    // "No compute worker connected".
    let electron = Path::new("node_modules")
        .join("electron")
        .join("dist")
        .join("electron.exe");
    if !dir.join(&electron).exists() {
        return Err(format!(
            "Omnix Electron runtime not installed at {}. npm install may have failed.",
            dir.display()
        ));
    }

    let mut guard = OMNIX_CHILD.lock().map_err(|e| format!("Mutex error: {}", e))?;
    if guard.is_some() {
        return Ok(());
    }
    let child = std::process::Command::new(dir.join(&electron))
        .current_dir(&dir)
        .arg(".")
        .spawn()
        .map_err(|e| format!("Failed to spawn omnix: {}", e))?;
    *guard = Some(child);
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
        Ok(response) => Ok(response.status().is_success()),
        Err(_) => Ok(false),
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
/// (simple vs complex) so the caller can pick the 9Router target model.
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
