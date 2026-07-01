// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Rain agent tools (Phase 8 step 2).
//!
//! Seven tools Rain can call via OpenAI-style function calling through the
//! local AI server (Ollama, LM Studio, Lemonade, or any OpenAI-compatible
//! endpoint). Each is a thin Tauri command that wraps an existing operation
//! and returns a JSON string the agent loop feeds back to the model as the
//! tool result.
//!
//! Read-only + create_folder execute immediately. move/rename/delete are
//! confirmation-gated in the FRONTEND (the panel shows a confirm card before
//! invoking the execute command); these backend commands perform the action
//! when called, so the frontend must only call them post-confirmation.

use serde_json::{json, Value};
use std::time::Duration;
use tauri::Emitter;


/// Run a shell command on the local machine. Captures stdout + stderr.
/// Gated behind user confirmation in the frontend.
#[tauri::command]
pub async fn rain_run_shell_command(
    app: tauri::AppHandle,
    command: String,
    timeout_secs: Option<u64>,
) -> Result<String, String> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(30));
    let result = crate::process_runner::run_command_blocking(
        if cfg!(windows) { "cmd.exe" } else { "sh" },
        &[if cfg!(windows) { "/C" } else { "-c" }, &command],
        timeout,
    );
    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            // Cap at 50 KB to avoid blowing up the model context.
            let capped: String = stdout.chars().take(50_000).collect();
            let result = json!({
                "ok": output.is_success(),
                "exitCode": output.status.code(),
                "stdout": capped,
            });
            Ok(serde_json::to_string(&result).map_err(|e| format!("Serialize: {}", e))?)
        }
        Err(crate::process_runner::ProcessRunError::TimedOut(d)) => {
            let result = json!({
                "ok": false,
                "error": format!("Command timed out after {} seconds", d.as_secs()),
                "stdout": "",
                "exitCode": null,
            });
            Ok(serde_json::to_string(&result).map_err(|e| format!("Serialize: {}", e))?)
        }
        Err(crate::process_runner::ProcessRunError::SpawnFailed(e)) => {
            Err(format!("Failed to spawn command: {}", e))
        }
        Err(crate::process_runner::ProcessRunError::WaitFailed(e)) => {
            Err(format!("Command wait failed: {}", e))
        }
    }
    .map(|r| {
        let _ = app.emit("rain-shell-output", &r);
        r
    })
}

/// Write/replace a file at the given path with the given content.
/// CONFIRMATION REQUIRED in the frontend before calling.
#[tauri::command]
pub async fn rain_write_file(
    path: String,
    content: String,
) -> Result<String, String> {
    match std::fs::write(&path, &content) {
        Ok(_) => {
            let result = json!({
                "ok": true,
                "path": path,
                "bytesWritten": content.len(),
            });
            serde_json::to_string(&result).map_err(|e| format!("Serialize: {}", e))
        }
        Err(e) => {
            let result = json!({
                "ok": false,
                "error": format!("{}", e),
            });
            Ok(serde_json::to_string(&result).map_err(|e| format!("Serialize: {}", e))?)
        }
    }
}

/// Returns the OpenAI `tools` array (function schemas) Rain advertises to the
/// model. Returned as a JSON string so the frontend can splice it straight into
/// the local AI server's chat-completion request body.
#[tauri::command]
pub fn rain_tool_schemas() -> Result<String, String> {
    let tools = json!([
        tool_schema("list_directory", "List the contents of a directory. Read-only.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute local path or ssh://host/path remote path" }
            },
            "required": ["path"]
        })),
        tool_schema("read_file", "Read the text contents of a file. Read-only.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute local path or ssh://host/path remote path" }
            },
            "required": ["path"]
        })),
        tool_schema("search_files", "Search for files by name/content across a scope. Read-only.", json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "scope": { "type": "string", "description": "'current' folder, 'all' drives, or a specific drive/path" }
            },
            "required": ["query"]
        })),
        tool_schema("create_folder", "Create a new directory. Non-destructive; executes immediately.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the directory to create" }
            },
            "required": ["path"]
        })),
        tool_schema("move_files", "Move one or more files/folders. REQUIRES user confirmation.", json!({
            "type": "object",
            "properties": {
                "src": { "type": "array", "items": { "type": "string" }, "description": "Source paths to move" },
                "dest": { "type": "string", "description": "Destination directory" }
            },
            "required": ["src", "dest"]
        })),
        tool_schema("rename_item", "Rename a file or folder. REQUIRES user confirmation.", json!({
            "type": "object",
            "properties": {
                "old": { "type": "string", "description": "Existing path" },
                "new": { "type": "string", "description": "New name (leaf, not full path)" }
            },
            "required": ["old", "new"]
        })),
        tool_schema("delete_item", "Delete a file or folder (to recycle bin by default). REQUIRES user confirmation.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to delete" },
                "permanent": { "type": "boolean", "description": "If true, permanent delete; default false = recycle bin" }
            },
            "required": ["path"]
        })),
        tool_schema("write_file", "Write or replace a text file. REQUIRES user confirmation.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the file to write" },
                "content": { "type": "string", "description": "Full text content to write" }
            },
            "required": ["path", "content"]
        })),
        tool_schema("run_shell_command", "Run a shell command on the local machine. REQUIRES user confirmation.", json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute (e.g. 'dir C:\\' on Windows, 'ls -la' on Linux)" },
                "timeout_secs": { "type": "number", "description": "Timeout in seconds (default 30)" }
            },
            "required": ["command"]
        }))
    ]);
    serde_json::to_string(&tools).map_err(|e| format!("Failed to serialize tool schemas: {}", e))
}

fn tool_schema(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters
        }
    })
}

/// Execute a READ-ONLY tool (list_directory, read_file, search_files) plus the
/// non-destructive create_folder. Destructive tools (move/rename/delete) are NOT
/// handled here — the frontend gates those behind a confirmation card and calls
/// the dedicated execute commands below. Returns a JSON string for the model.
#[tauri::command]
pub async fn rain_run_tool(
    _app: tauri::AppHandle,
    name: String,
    args: Value,
) -> Result<String, String> {
    let result: Value = match name.as_str() {
        "list_directory" => {
            let path = str_arg(&args, "path")?;
            match crate::dir_reader::read_dir(path.clone(), None) {
                Ok(contents) => json!({ "ok": true, "path": path, "contents": contents }),
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        "read_file" => {
            let path = str_arg(&args, "path")?;
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    // Cap very large files so we don't blow the model context.
                    let capped: String = text.chars().take(20_000).collect();
                    json!({ "ok": true, "path": path, "content": capped })
                }
                Err(e) => json!({ "ok": false, "error": format!("{}", e) }),
            }
        }
        "create_folder" => {
            let path = str_arg(&args, "path")?;
            match std::fs::create_dir_all(&path) {
                Ok(_) => json!({ "ok": true, "path": path, "created": true }),
                Err(e) => json!({ "ok": false, "error": format!("{}", e) }),
            }
        }
        "search_files" => {
            // Search is driven from the frontend (global_search store); the agent
            // loop handles this tool client-side. If the model calls it here,
            // return a hint rather than failing.
            json!({ "ok": false, "error": "search_files is handled client-side" })
        }
        "write_file" => {
            // Handled via the dedicated rain_write_file command (frontend-gated).
            json!({ "ok": false, "error": "write_file is handled via dedicated command" })
        }
        "run_shell_command" => {
            // Handled via the dedicated rain_run_shell_command command.
            json!({ "ok": false, "error": "run_shell_command is handled via dedicated command" })
        }
        other => json!({ "ok": false, "error": format!("Unknown or non-immediate tool: {}", other) }),
    };
    serde_json::to_string(&result).map_err(|e| format!("Failed to serialize tool result: {}", e))
}

fn str_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Missing required argument: {}", key))
}
