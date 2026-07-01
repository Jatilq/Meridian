// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Rain agent tools (Phase 8 step 2; Fix E 2026-07-01).
//!
//! Eight tools Rain can call via OpenAI-style function calling through the
//! local AI server (Ollama, LM Studio, Lemonade, or any OpenAI-compatible
//! endpoint). Each is a thin Tauri command that wraps an existing operation
//! and returns a JSON string the agent loop feeds back to the model as the
//! tool result.
//!
//! Read-only + create_folder execute immediately. move/rename/delete
//! and write_file / run_shell_command are confirmation-gated in the
//! FRONTEND (the panel shows a confirm card before invoking the execute
//! command); these backend commands perform the action when called, so
//! the frontend must only call them post-confirmation.
//!
//! Fix E changes: search_files has been REMOVED from the schema because
//! it had no Rust execution path (was advertised but every Rust arm
//! returned an error). Frontend readers retain their dedicated
//! global_search composable / store for client-side search.

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

/// Execute a Rain tool. Read-only tools (list_directory, read_file) plus
/// the non-destructive create_folder execute immediately. The 5 destructive
/// tools (move_files, rename_item, delete_item, write_file, run_shell_command)
/// ALSO execute here — the frontend must only invoke them AFTER the user
/// has confirmed via the in-panel confirmation card. Returns a JSON string
/// for the model.
///
/// Fix E: search_files has been REMOVED from the schema entirely; the model
/// can no longer ask Rust to run a search. Frontend readers retain their
/// dedicated global_search composable / store.
#[tauri::command]
pub async fn rain_run_tool(
    app: tauri::AppHandle,
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
        "write_file" => {
            // Execute via the existing dedicated command path so a single
            // source of truth handles the file-write contract (cap, bytesWritten,
            // error mapping). Frontend gates destructive write_file via the
            // confirmation card before any of these tool names reach here.
            let path = str_arg(&args, "path")?;
            let content = str_arg(&args, "content")?;
            let inner_json = rain_write_file(path, content).await?;
            let inner: Value = serde_json::from_str(&inner_json).unwrap_or(Value::Null);
            json!({
                "ok": inner.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
                "inner": inner,
            })
        }
        "run_shell_command" => {
            let command = str_arg(&args, "command")?;
            let timeout_secs = args.get("timeout_secs").and_then(|v| v.as_u64());
            let inner_json = rain_run_shell_command(app, command, timeout_secs).await?;
            let inner: Value = serde_json::from_str(&inner_json).unwrap_or(Value::Null);
            json!({
                "ok": inner.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
                "inner": inner,
            })
        }
        "move_files" => {
            let src_val = args.get("src").cloned()
                .ok_or_else(|| "Missing required argument: src".to_string())?;
            let src: Vec<String> = serde_json::from_value(src_val)
                .map_err(|e| format!("Invalid 'src' array: {}", e))?;
            let dest = str_arg(&args, "dest")?;
            let op = crate::file_operations::move_items(src, dest, None, None);
            file_op_result_json(&op)
        }
        "rename_item" => {
            let old_path = str_arg(&args, "old")?;
            let new_name = str_arg(&args, "new")?;
            let op = crate::file_operations::rename_item(old_path, new_name);
            file_op_result_json(&op)
        }
        "delete_item" => {
            let path = str_arg(&args, "path")?;
            let permanent = args.get("permanent").and_then(|v| v.as_bool()).unwrap_or(false);
            // permanent=true → use_trash=false (skip Recycle Bin); !permanent → 
            // use_trash=true (trash / recycle bin).
            let op = crate::file_operations::delete_items(vec![path], !permanent);
            file_op_result_json(&op)
        }
        other => json!({ "ok": false, "error": format!("Unknown or non-immediate tool: {}", other) }),
    };
    serde_json::to_string(&result).map_err(|e| format!("Failed to serialize tool result: {}", e))
}

/// Project a `FileOperationResult` (the move/rename/delete return shape
/// from `file_operations.rs`) into the same `{ok, error, copied/failed/
/// skipped count}` envelope Rain's other tool results already use. Keeps
/// the model consistent: every Rain tool result has the same top-level
/// shape regardless of which file-operation crate was underneath.
fn file_op_result_json(op: &crate::file_operations::FileOperationResult) -> Value {
    json!({
        "ok": op.success,
        "error": op.error,
        "copiedCount": op.copied_count,
        "failedCount": op.failed_count,
        "skippedCount": op.skipped_count,
    })
}

fn str_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Missing required argument: {}", key))
}

// ============================================================================
// Fix E: dedicated Tauri commands for the 3 destructive tools that previously
// had only `rain_run_tool` placeholder arms (and thus no end-to-end path).
// ============================================================================

/// Move one or more files/folders. Frontend invokes this after the user
/// confirms in the in-panel confirmation card (or directly when called
/// from the dedicated invoke surface). Delegates to
/// `file_operations::move_items` — the same engine the file-browser
/// context menu uses, with full conflict resolution support.
#[tauri::command]
pub fn rain_move_files(
    src: Vec<String>,
    dest: String,
) -> Result<String, String> {
    let op = crate::file_operations::move_items(src, dest, None, None);
    serde_json::to_string(&file_op_result_json(&op))
        .map_err(|e| format!("Serialize: {}", e))
}

/// Rename a file or folder. `new_name` is a LEAF, not a full path.
/// Old path remains absolute; new name is appended.
#[tauri::command]
pub fn rain_rename_item(
    old_path: String,
    new_name: String,
) -> Result<String, String> {
    let op = crate::file_operations::rename_item(old_path, new_name);
    serde_json::to_string(&file_op_result_json(&op))
        .map_err(|e| format!("Serialize: {}", e))
}

/// Delete a file or folder. `permanent = false` (default) routes the
/// item to the OS Recycle Bin via `trash`; `permanent = true` does an
/// unlink. Mirrors the file-browser delete context menu's surface
/// exactly.
#[tauri::command]
pub fn rain_delete_item(
    path: String,
    permanent: Option<bool>,
) -> Result<String, String> {
    let permanent = permanent.unwrap_or(false);
    let op = crate::file_operations::delete_items(vec![path], !permanent);
    serde_json::to_string(&file_op_result_json(&op))
        .map_err(|e| format!("Serialize: {}", e))
}
