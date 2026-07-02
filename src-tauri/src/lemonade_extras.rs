// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Meridian — Phase 11 day-2: Lemonade endpoint extras (STT / TTS / Vision).
//!
//! Mirrors the shape of `omnix.rs` (single-binary Electron server with a
//! bespoke JSON contract) but targets Lemonade's OpenAI-compatible API on
//! the port chosen by
//! `backend_manager::BackendKind::Lemonade::default_port()` (13305). All
//! three endpoints accept an optional `endpoint` override so the frontend
//! can point them at a non-default Lemonade install (typically
//! `aiPanelStore.localEndpointUrl` with any trailing `/v1` stripped).
//!
//! Routing on the Vue side mirrors the Omnix-first / router-fallback
//! pattern already in `ai-panel.vue`:
//!
//!   * When `useOmnix && omnixOnline` -> the legacy `omnix_*` Tauri commands
//!     handle the call (preserves the existing Omnix Electron path so
//!     existing JC installs behave identically until the toggle is flipped).
//!   * Otherwise -> call `lemonade_*` so Rain picks up Lemonade as the new
//!     Tier-1 backend.
//!
//! Endpoint shape references:
//!   * TTS:    POST /v1/audio/speech         (OpenAI-compat; returns raw audio bytes)
//!   * STT:    POST /v1/audio/transcriptions (OpenAI-compat; multipart/form-data -> { "text": "..." })
//!   * Vision: POST /v1/chat/completions     (OpenAI-compat; image_url content type -> standard chat JSON)

use base64::Engine;
use std::path::Path;
use std::time::Duration;

/// Default Lemonade OpenAI-compatible base URL. Matches the port that
/// `backend_manager::BackendKind::Lemonade::default_port()` resolves to and
/// the catalog row in `src/data/backends.json` (also pinned in
/// `tauri.conf.json::bundle.resources`). Kept here (not read from
/// `backend_manager.rs`) so this module has zero coupling to the backend
/// lifecycle registry — these commands can be called before any backend
/// binary has been downloaded or started.
const DEFAULT_LEMONADE_BASE: &str = "http://localhost:13305";

/// Resolve an optional user-supplied endpoint to a stable base URL.
///
/// Trims surrounding whitespace and discards any trailing `/v1` so we
/// always get a base that we can append `/v1/<something>` onto. Empty
/// input falls back to `DEFAULT_LEMONADE_BASE`. We do NOT require the
/// caller to pass a `/v1` suffix — that pattern would otherwise force
/// every frontend (panel + rain-cli + settings + slide-in) to remember
/// to chop and re-add the same prefix.
fn resolve_base(endpoint: Option<&str>) -> String {
    let raw = endpoint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LEMONADE_BASE);
    let no_slash = raw.trim_end_matches('/');
    let no_v1 = no_slash.trim_end_matches("/v1");
    no_v1.to_string()
}

/// Synthesize speech via Lemonade's `/v1/audio/speech` endpoint. Returns the
/// raw audio bytes (mp3 / wav / pcm per content-type) so the frontend can
/// wrap them in a Blob and play them through a standard `<audio>` element.
///
/// Frontend consumer pattern (TS):
///   const byteArray = await invoke<number[]>('lemonade_tts', {...});
///   if (!byteArray.length) return;
///   const blob = new Blob([new Uint8Array(byteArray)], { type: 'audio/wav' });
///   const url = URL.createObjectURL(blob);
///   const audio = new Audio(url);
///   audio.onended = () => URL.revokeObjectURL(url);
///   void audio.play();
#[tauri::command]
pub async fn lemonade_tts(
    text: String,
    voice: Option<String>,
    model: Option<String>,
    endpoint: Option<String>,
) -> Result<Vec<u8>, String> {
    let base = resolve_base(endpoint.as_deref());
    let url = format!("{}/v1/audio/speech", base);
    let voice = voice
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or("af_heart".to_string())
        .to_string();
    let model = model
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or("kokoro".to_string())
        .to_string();

    let body = serde_json::json!({
        "model": model,
        "input": text,
        "voice": voice,
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("Lemonade TTS request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Lemonade TTS returned status {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read Lemonade TTS response: {}", e))?;
    Ok(bytes.to_vec())
}

/// Transcribe audio via Lemonade's `/v1/audio/transcriptions` endpoint
/// (multipart/form-data — Whisper-style). Returns the transcribed text.
///
/// `audio_base64` carries the recorded audio as a base64 string from the
/// frontend (the Web Audio API + MediaRecorder do not expose raw ArrayBuffers
/// cleanly via Tauri's `invoke`). Backend decodes then forwards as a
/// multipart `file` field so Lemonade sees the same shape OpenAI-compat
/// expects. The filename extension is preserved so Lemonade's MIME sniffer
/// picks the right decoder (wav / mp3 / webm / ogg). Whitelist of supported
/// extensions lives below.
#[tauri::command]
pub async fn lemonade_stt(
    audio_base64: String,
    filename: String,
    model: Option<String>,
    language: Option<String>,
    endpoint: Option<String>,
) -> Result<String, String> {
    let base = resolve_base(endpoint.as_deref());
    let url = format!("{}/v1/audio/transcriptions", base);
    let model = model
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or("whisper-large-v3-turbo".to_string())
        .to_string();
    let language = language.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(audio_base64.as_bytes())
        .map_err(|e| format!("Failed to decode audio base64: {}", e))?;
    if bytes.is_empty() {
        return Err("Recorded audio is empty".to_string());
    }

    // Sniff extension from the supplied filename so Lemonade picks the
    // right decoder. A blank / malformed filename falls back to wav —
    // MediaRecorder defaults to a webm container but Lemonade happily
    // sniffs by content; the filename is only a hint here.
    let safe_name = if filename.trim().is_empty() {
        "recording.wav".to_string()
    } else {
        Path::new(&filename)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "recording.wav".to_string())
    };

    let part = reqwest::multipart::Part::bytes(bytes).file_name(safe_name);
    let mut form = reqwest::multipart::Form::new().part("file", part);
    form = form.text("model", model);
    if let Some(lang) = language {
        form = form.text("language", lang);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let response = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Lemonade STT request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Lemonade STT returned status {}", response.status()));
    }

    // Try JSON wrapper (`{"text": "..."}` per OpenAI spec) first; Whisper-
    // compatible servers that return plain text fall through to the raw body.
    let raw = response
        .text()
        .await
        .map_err(|e| format!("Failed to read Lemonade STT response: {}", e))?;
    let parsed: Option<serde_json::Value> = serde_json::from_str(&raw).ok();
    let text = parsed
        .as_ref()
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or(raw);
    Ok(text)
}

/// Send an image file to Lemonade's `/v1/chat/completions` as an OpenAI-style
/// chat-completions request with an inline `image_url` data URL. Returns the
/// model's text response.
///
/// Lemonade doesn't expose a separate `/v1/vision` endpoint — vision is just
/// a chat-completions call with `image_url` content (mirrors GPT-4o, Llama 3.2
/// Vision, etc.). The same plumbing handles text-only calls, so the same
/// router endpoint config (routerEndpoint / localEndpointUrl) drives both.
///
/// Image bytes are base64-encoded inline rather than uploaded via multipart
/// because the OpenAI-compat chat-completions spec doesn't accept multipart on
/// this endpoint. Maximum sensible payload is ~20MB before base64 expansion,
/// which is enough for any sensible single-image vision call.
#[tauri::command]
pub async fn lemonade_image(
    image_path: String,
    prompt: Option<String>,
    model: Option<String>,
    endpoint: Option<String>,
) -> Result<String, String> {
    let bytes = std::fs::read(&image_path)
        .map_err(|e| format!("Failed to read image {}: {}", image_path, e))?;
    let mime = mime_guess::from_path(&image_path)
        .first_raw()
        .unwrap_or("image/png")
        .to_string();

    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let data_url = format!("data:{};base64,{}", mime, encoded);

    let base = resolve_base(endpoint.as_deref());
    let url = format!("{}/v1/chat/completions", base);
    let model = model
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or("default".to_string())
        .to_string();

    // Build the OpenAI-style content array. Only emit the text part when
    // the caller actually supplied a non-empty prompt — some image-only
    // queries should drop the empty text slot so the model sees ONLY the
    // image_url content.
    let mut content_parts: Vec<serde_json::Value> = Vec::new();
    if let Some(p) = prompt.as_ref().filter(|s| !s.trim().is_empty()) {
        content_parts.push(serde_json::json!({
            "type": "text",
            "text": p,
        }));
    }
    content_parts.push(serde_json::json!({
        "type": "image_url",
        "image_url": { "url": data_url },
    }));

    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "user", "content": content_parts }
        ],
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("Lemonade vision request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Lemonade vision returned status {}",
            response.status()
        ));
    }

    let raw = response
        .text()
        .await
        .map_err(|e| format!("Failed to read Lemonade vision response: {}", e))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse Lemonade vision response: {}", e))?;

    // OpenAI-compat preference order:
    //   1. choices[0].message.content (LoRA / instruct models)
    //   2. choices[0].text (legacy completions-style)
    //   3. raw body (last-resort)
    let text = parsed
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c0| c0.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .or_else(|| {
            parsed
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c0| c0.get("text"))
                .and_then(|v| v.as_str())
        })
        .map(str::to_string)
        .unwrap_or(raw);

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_base_defaults_to_localhost_13305() {
        assert_eq!(resolve_base(None), "http://localhost:13305");
        assert_eq!(resolve_base(Some("")), "http://localhost:13305");
        assert_eq!(resolve_base(Some("   ")), "http://localhost:13305");
    }

    #[test]
    fn resolve_base_strips_trailing_slash() {
        assert_eq!(resolve_base(Some("http://localhost:13305/")), "http://localhost:13305");
    }

    #[test]
    fn resolve_base_strips_trailing_v1() {
        assert_eq!(resolve_base(Some("http://localhost:13305/v1")), "http://localhost:13305");
        assert_eq!(resolve_base(Some("http://localhost:13305/v1/")), "http://localhost:13305");
    }

    #[test]
    fn resolve_base_trims_surrounding_whitespace() {
        assert_eq!(
            resolve_base(Some("  http://localhost:13305/v1  ")),
            "http://localhost:13305"
        );
    }

    #[test]
    fn resolve_base_preserves_custom_host() {
        assert_eq!(
            resolve_base(Some("http://192.168.1.50:13305/v1")),
            "http://192.168.1.50:13305"
        );
    }
}
