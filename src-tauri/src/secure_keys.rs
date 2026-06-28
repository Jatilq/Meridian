// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILENAME: &str = "secure-keys.json";
const API_KEY_ENTRY: &str = "aiApiKey";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyEntry {
    pub provider: String,
    pub key: String,
}

/// Generic string-keyed secret entry stored JSON-encoded so the tauri-plugin-store
/// can round-trip the value. Used for SSH passwords (key like `"ssh:<uuid>"`),
/// future auth tokens, etc.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SecretEntry {
    pub key: String,
    pub value: String,
}

fn store_path(app_handle: &AppHandle) -> std::path::PathBuf {
    app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(STORE_FILENAME)
}

fn ensure_store(
    app_handle: &AppHandle,
) -> Result<std::sync::Arc<tauri_plugin_store::Store<tauri::Wry>>, String> {
    app_handle
        .store(store_path(app_handle))
        .map_err(|e| format!("Failed to open secure key store: {e}"))
}

#[tauri::command]
pub fn secure_store_api_key(
    app_handle: AppHandle,
    provider: String,
    key: String,
) -> Result<(), String> {
    let store = ensure_store(&app_handle)?;
    store.set(
        API_KEY_ENTRY,
        serde_json::json!(ApiKeyEntry { provider, key }),
    );
    store
        .save()
        .map_err(|e| format!("Failed to save API key: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn secure_get_api_key(app_handle: AppHandle) -> Result<Option<ApiKeyEntry>, String> {
    let store = ensure_store(&app_handle)?;
    match store.get(API_KEY_ENTRY) {
        Some(value) => serde_json::from_value::<ApiKeyEntry>(value)
            .map(Some)
            .map_err(|e| format!("Failed to parse stored API key: {e}")),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn secure_delete_api_key(app_handle: AppHandle) -> Result<(), String> {
    let store = ensure_store(&app_handle)?;
    store.delete(API_KEY_ENTRY);
    store
        .save()
        .map_err(|e| format!("Failed to delete API key: {e}"))?;
    Ok(())
}

/// Resolve a bearer token for use by the AI panel. Prefers the stored API key,
/// but falls back to any explicitly provided key (e.g. from settings UI).
#[tauri::command]
pub fn secure_resolve_api_key(
    app_handle: AppHandle,
    override_key: Option<String>,
) -> Result<Option<String>, String> {
    if let Some(key) = override_key {
        return Ok(Some(key));
    }
    match secure_get_api_key(app_handle)? {
        Some(entry) => Ok(Some(entry.key)),
        None => Ok(None),
    }
}

/// Store a string secret at an arbitrary key. Mirrors `secure_store_api_key`:
/// the value lands in `secure-keys.json` (separate from user settings),
/// keeping secrets out of the main config blob. Use `secure_get_secret` /
/// `secure_delete_secret` to round-trip.
///
/// Security trade-off: this is isolation, not strong cryptographic encryption.
/// A determined attacker with code execution can still read the file. For
/// higher security, swap this implementation for Windows Credential Manager,
/// macOS Keychain, or libsecret-backed storage. The interface is stable so
/// the upgrade is local.
#[tauri::command]
pub fn secure_store_secret(
    app_handle: AppHandle,
    key: String,
    value: String,
) -> Result<(), String> {
    let store = ensure_store(&app_handle)?;
    store.set(
        &key,
        serde_json::json!(SecretEntry {
            key: key.clone(),
            value,
        }),
    );
    store
        .save()
        .map_err(|e| format!("Failed to save secret: {e}"))?;
    Ok(())
}

/// Retrieve a previously stored secret string by key. Returns `Ok(None)` if
/// the key is not present.
#[tauri::command]
pub fn secure_get_secret(
    app_handle: AppHandle,
    key: String,
) -> Result<Option<String>, String> {
    let store = ensure_store(&app_handle)?;
    match store.get(&key) {
        Some(value) => Ok(serde_json::from_value::<SecretEntry>(value)
            .ok()
            .map(|entry| entry.value)),
        None => Ok(None),
    }
}

/// Remove a previously stored secret. No-op if the key is not present.
#[tauri::command]
pub fn secure_delete_secret(app_handle: AppHandle, key: String) -> Result<(), String> {
    let store = ensure_store(&app_handle)?;
    store.delete(&key);
    store
        .save()
        .map_err(|e| format!("Failed to delete secret: {e}"))?;
    Ok(())
}
