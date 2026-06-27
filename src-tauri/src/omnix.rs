// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;

static OMNIX_CHILD: Mutex<Option<Child>> = Mutex::new(None);

/// Default install directory for the Omnix engine.
const DEFAULT_OMNIX_DIR: &str = "E:\\ai\\Apps\\Omnix";

/// Resolve the Omnix project directory. Uses the caller-provided path when set,
/// otherwise the default install location. Returns an error if it does not
/// contain the expected `server.ts` entry point.
fn resolve_omnix_dir(omnix_path: Option<String>) -> Result<PathBuf, String> {
    let dir = omnix_path
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OMNIX_DIR.to_string());
    let dir = PathBuf::from(dir);
    if !dir.join("server.ts").exists() {
        return Err(format!(
            "Omnix not found at {}. Set the Omnix path in Settings and run `npm install` there once.",
            dir.display()
        ));
    }
    Ok(dir)
}

#[tauri::command]
pub fn spawn_omnix(omnix_path: Option<String>) -> Result<(), String> {
    let dir = resolve_omnix_dir(omnix_path)?;
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
            "Omnix Electron runtime not installed at {}. Run `npm install` in that directory once.",
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
