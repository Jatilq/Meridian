// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;

static OMNIX_CHILD: Mutex<Option<Child>> = Mutex::new(None);

fn find_omnix() -> Option<PathBuf> {
    let candidates = vec![
        "omnix",
        "omnix.exe",
        "./omnix",
        "./bin/omnix",
    ];
    for candidate in candidates {
        if let Ok(path) = which::which(candidate) {
            return Some(path);
        }
    }
    None
}

#[tauri::command]
pub fn spawn_omnix() -> Result<(), String> {
    let binary = find_omnix().ok_or_else(|| "omnix binary not found".to_string())?;
    let mut guard = OMNIX_CHILD.lock().map_err(|e| format!("Mutex error: {}", e))?;
    if guard.is_some() {
        return Ok(());
    }
    let pid = std::process::id();
    let child = std::process::Command::new(binary)
        .args(["--silent", "--dependent-pid", &pid.to_string()])
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
    match reqwest::get("http://localhost:7770/api/text").await {
        Ok(response) => Ok(response.status().is_success()),
        Err(_) => Ok(false),
    }
}
