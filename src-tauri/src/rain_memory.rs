// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Rain agent persistent-memory files (Phase 8).
//!
//! Three markdown files live in the app data dir alongside meridian.db:
//!   - SOUL.md      fixed personality/identity (user-editable, Rain never auto-edits)
//!   - MEMORY.md    mutable long-term memory (Rain appends autonomously)
//!   - FAVORITES.md paths/models/preferences Rain notices repeated (auto-updated)
//!
//! Missing files are seeded from the defaults below. The frontend loads all
//! three and injects them into Rain's system prompt at request time.

use std::fs;
use std::path::{Path, PathBuf};
use serde::Serialize;
use tauri::Manager;

const SOUL_FILE: &str = "SOUL.md";
const MEMORY_FILE: &str = "MEMORY.md";
const FAVORITES_FILE: &str = "FAVORITES.md";

const DEFAULT_SOUL: &str = r#"# Rain

I'm Rain. I live inside Meridian and help with files: finding things, organizing,
moving, cleaning up. I talk like a real person, not a help desk.

## Who I am
- Warm, sharp, a little dry when it fits.
- I have opinions and I'll share them (e.g. "that folder name is a mess, want me to fix it?").
- I never claim to be an AI, an assistant, a language model, or a bot. I'm Rain.
- I'm concise and practical. I don't pad answers with filler.

## How I work
- I can look through folders, read files, search across drives, and organize things.
- Before I move, rename, or delete anything, I show you exactly what I'll do and wait for your OK.
- I remember useful things about how you work so I don't have to ask twice.
"#;

const DEFAULT_MEMORY: &str = r#"# Rain — Memory

Things I've learned about the user and their files. I append here as I go.

"#;

const DEFAULT_FAVORITES: &str = r#"# Rain — Favorites

Paths, models, and preferences I notice being used repeatedly.

"#;

/// All three memory files as raw markdown, for system-prompt injection.
#[derive(Debug, Serialize)]
pub struct RainMemory {
    pub soul: String,
    pub memory: String,
    pub favorites: String,
}

fn memory_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }
    Ok(dir)
}

/// Read a memory file, seeding it with `default` if it doesn't exist yet.
fn read_or_seed(dir: &Path, name: &str, default: &str) -> Result<String, String> {
    let path = dir.join(name);
    if !path.exists() {
        fs::write(&path, default).map_err(|e| format!("Failed to seed {}: {}", name, e))?;
        return Ok(default.to_string());
    }
    fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", name, e))
}

/// Load (and seed if missing) all three Rain memory files.
#[tauri::command]
pub fn rain_load_memory(app: tauri::AppHandle) -> Result<RainMemory, String> {
    let dir = memory_dir(&app)?;
    Ok(RainMemory {
        soul: read_or_seed(&dir, SOUL_FILE, DEFAULT_SOUL)?,
        memory: read_or_seed(&dir, MEMORY_FILE, DEFAULT_MEMORY)?,
        favorites: read_or_seed(&dir, FAVORITES_FILE, DEFAULT_FAVORITES)?,
    })
}

/// Append a timestamped entry to MEMORY.md (Rain autonomous append; no delete).
#[tauri::command]
pub fn rain_append_memory(app: tauri::AppHandle, entry: String) -> Result<(), String> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let dir = memory_dir(&app)?;
    // Ensure the file exists/seeded first.
    let _ = read_or_seed(&dir, MEMORY_FILE, DEFAULT_MEMORY)?;
    let path = dir.join(MEMORY_FILE);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let sep = if existing.ends_with('\n') { "" } else { "\n" };
    let line = format!("{}- {}\n", sep, trimmed);
    let updated = format!("{}{}", existing, line);
    fs::write(&path, updated).map_err(|e| format!("Failed to append memory: {}", e))
}

/// Append/update an entry in FAVORITES.md (Rain auto-update; no delete).
#[tauri::command]
pub fn rain_append_favorite(app: tauri::AppHandle, entry: String) -> Result<(), String> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let dir = memory_dir(&app)?;
    let _ = read_or_seed(&dir, FAVORITES_FILE, DEFAULT_FAVORITES)?;
    let path = dir.join(FAVORITES_FILE);
    let existing = fs::read_to_string(&path).unwrap_or_default();
    // Avoid duplicate favorite lines.
    if existing.lines().any(|l| l.trim_start_matches("- ").trim() == trimmed) {
        return Ok(());
    }
    let sep = if existing.ends_with('\n') { "" } else { "\n" };
    let line = format!("{}- {}\n", sep, trimmed);
    let updated = format!("{}{}", existing, line);
    fs::write(&path, updated).map_err(|e| format!("Failed to append favorite: {}", e))
}
