// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Rain agent tool-call audit log (Phase 8 caveat).
//!
//! Persists every Rain tool invocation to a `rain_tool_log` table in the app
//! data dir (same SQLite file location convention as the downloader). Records
//! timestamp, tool name, args JSON, outcome JSON, and the confirmation state.

use std::fs;
use std::path::Path;
use tauri::Manager;

const DB_FILE_NAME: &str = "meridian.db";

fn open_conn(app: &tauri::AppHandle) -> Result<rusqlite::Connection, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }
    let db_path = Path::new(&dir).join(DB_FILE_NAME);
    let conn = rusqlite::Connection::open(db_path).map_err(|e| format!("Failed to open db: {}", e))?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| format!("Failed to set busy_timeout: {}", e))?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "NORMAL");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS rain_tool_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            tool TEXT NOT NULL,
            args TEXT NOT NULL,
            outcome TEXT NOT NULL,
            confirmation TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("Failed to create rain_tool_log table: {}", e))?;
    Ok(conn)
}

/// Append one tool-call record. `confirmation` is one of: 'immediate',
/// 'confirmed', 'cancelled'. `args`/`outcome` are JSON strings.
#[tauri::command]
pub fn rain_log_tool_call(
    app: tauri::AppHandle,
    tool: String,
    args: String,
    outcome: String,
    confirmation: String,
) -> Result<(), String> {
    let conn = open_conn(&app)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO rain_tool_log (ts, tool, args, outcome, confirmation) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![ts, tool, args, outcome, confirmation],
    )
    .map_err(|e| format!("Failed to insert tool log: {}", e))?;
    Ok(())
}
