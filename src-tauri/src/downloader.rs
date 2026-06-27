// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use crate::process_runner::run_command_blocking;
use std::collections::HashMap;
use std::fs;
use std::io::{Write, Seek, SeekFrom};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

const YTDLP_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_CHUNKS: u64 = 8;
const DB_FILE_NAME: &str = "meridian.db";
/// Sentinel error returned by the download path when a cancellation token fires,
/// so start_download can distinguish a user pause/cancel from a real failure.
const CANCELLED_MARKER: &str = "__meridian_cancelled__";

/// Registry of in-flight downloads so cancel/pause can actually stop a running
/// task, not just flip a DB status string (Gap 3).
#[derive(Default, Clone)]
pub struct DownloaderRegistry {
    tasks: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl DownloaderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn insert(&self, id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut map) = self.tasks.lock() {
            map.insert(id.to_string(), token.clone());
        }
        token
    }

    fn cancel(&self, id: &str) {
        if let Ok(map) = self.tasks.lock() {
            if let Some(token) = map.get(id) {
                token.cancel();
            }
        }
    }

    fn remove(&self, id: &str) {
        if let Ok(mut map) = self.tasks.lock() {
            map.remove(id);
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadItem {
    pub id: String,
    pub url: String,
    pub status: DownloadStatus,
    pub progress: f64,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub file_path: Option<String>,
    pub file_name: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadStatus::Pending => write!(f, "pending"),
            DownloadStatus::Downloading => write!(f, "downloading"),
            DownloadStatus::Paused => write!(f, "paused"),
            DownloadStatus::Completed => write!(f, "completed"),
            DownloadStatus::Failed => write!(f, "failed"),
            DownloadStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct YtDlpFormat {
    pub format_id: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub filesize: Option<u64>,
    pub format_note: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DownloadQueueState {
    pub queue: Vec<DownloadItem>,
    pub history: Vec<DownloadItem>,
}

pub(crate) struct DownloaderDb {
    conn: rusqlite::Connection,
}

impl DownloaderDb {
    fn open(app_data_dir: &str) -> Result<Self, String> {
        let db_dir = Path::new(app_data_dir);
        if !db_dir.exists() {
            fs::create_dir_all(db_dir).map_err(|e| format!("Failed to create db dir: {}", e))?;
        }
        let db_path = db_dir.join(DB_FILE_NAME);
        let conn = rusqlite::Connection::open(db_path).map_err(|e| format!("Failed to open db: {}", e))?;
        // Multiple short-lived connections are opened concurrently (per command
        // + per progress-callback tick). Without these, concurrent writes throw
        // "database is locked" and downloads fail (notably on a 2nd attempt).
        // WAL allows concurrent readers/writer; busy_timeout makes writers wait
        // for the lock instead of erroring immediately.
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| format!("Failed to set busy_timeout: {}", e))?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        let _ = conn.pragma_update(None, "synchronous", "NORMAL");
        let db = DownloaderDb { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<(), String> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS download_queue (
                    id TEXT PRIMARY KEY,
                    url TEXT NOT NULL,
                    status TEXT NOT NULL,
                    progress REAL NOT NULL,
                    total_bytes INTEGER,
                    downloaded_bytes INTEGER NOT NULL,
                    file_path TEXT,
                    file_name TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    finished_at INTEGER,
                    error TEXT
                )",
                [],
            )
            .map_err(|e| format!("Failed to create queue table: {}", e))?;

        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS download_history (
                    id TEXT PRIMARY KEY,
                    url TEXT NOT NULL,
                    status TEXT NOT NULL,
                    progress REAL NOT NULL,
                    total_bytes INTEGER,
                    downloaded_bytes INTEGER NOT NULL,
                    file_path TEXT,
                    file_name TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    finished_at INTEGER,
                    error TEXT
                )",
                [],
            )
            .map_err(|e| format!("Failed to create history table: {}", e))?;

        // One-time cleanup: remove bogus "completed" history rows that were
        // actually the HTML page of a YouTube URL fetched via the old direct-HTTP
        // path (filename 'watch', tiny size). Real videos are far larger and
        // come from the yt-dlp path now.
        self.conn
            .execute(
                "DELETE FROM download_history
                 WHERE (url LIKE '%youtube.com%' OR url LIKE '%youtu.be%')
                   AND COALESCE(downloaded_bytes, 0) < 102400",
                [],
            )
            .map_err(|e| format!("Failed to purge bogus history: {}", e))?;

        Ok(())
    }

    fn enqueue(&self, item: &DownloadItem) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO download_queue (id, url, status, progress, total_bytes, downloaded_bytes, file_path, file_name, created_at, finished_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    item.id,
                    item.url,
                    item.status.to_string(),
                    item.progress,
                    item.total_bytes,
                    item.downloaded_bytes,
                    item.file_path,
                    item.file_name,
                    item.created_at,
                    item.finished_at,
                    item.error,
                ],
            )
            .map_err(|e| format!("Failed to enqueue: {}", e))?;
        Ok(())
    }

    fn update(&self, item: &DownloadItem) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE download_queue SET status=?1, progress=?2, total_bytes=?3, downloaded_bytes=?4, file_path=?5, finished_at=?6, error=?7 WHERE id=?8",
                rusqlite::params![
                    item.status.to_string(),
                    item.progress,
                    item.total_bytes,
                    item.downloaded_bytes,
                    item.file_path,
                    item.finished_at,
                    item.error,
                    item.id,
                ],
            )
            .map_err(|e| format!("Failed to update: {}", e))?;
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM download_queue WHERE id=?1", [id])
            .map_err(|e| format!("Failed to remove: {}", e))?;
        Ok(())
    }

    fn update_progress(
        &self,
        id: &str,
        progress: f64,
        total_bytes: Option<u64>,
        downloaded_bytes: u64,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE download_queue SET progress=?1, total_bytes=?2, downloaded_bytes=?3 WHERE id=?4",
                rusqlite::params![progress, total_bytes, downloaded_bytes, id],
            )
            .map_err(|e| format!("Failed to update progress: {}", e))?;
        Ok(())
    }

    fn load_queue(&self) -> Result<Vec<DownloadItem>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, url, status, progress, total_bytes, downloaded_bytes, file_path, file_name, created_at, finished_at, error FROM download_queue ORDER BY created_at ASC")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DownloadItem {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    status: parse_status(&row.get::<_, String>(2)?),
                    progress: row.get(3)?,
                    total_bytes: row.get(4)?,
                    downloaded_bytes: row.get(5)?,
                    file_path: row.get(6)?,
                    file_name: row.get(7)?,
                    created_at: row.get(8)?,
                    finished_at: row.get(9)?,
                    error: row.get(10)?,
                })
            })
            .map_err(|e| format!("Failed to query: {}", e))?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }

    fn load_history(&self) -> Result<Vec<DownloadItem>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, url, status, progress, total_bytes, downloaded_bytes, file_path, file_name, created_at, finished_at, error FROM download_history ORDER BY created_at DESC LIMIT 200")
            .map_err(|e| format!("Failed to prepare: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DownloadItem {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    status: parse_status(&row.get::<_, String>(2)?),
                    progress: row.get(3)?,
                    total_bytes: row.get(4)?,
                    downloaded_bytes: row.get(5)?,
                    file_path: row.get(6)?,
                    file_name: row.get(7)?,
                    created_at: row.get(8)?,
                    finished_at: row.get(9)?,
                    error: row.get(10)?,
                })
            })
            .map_err(|e| format!("Failed to query: {}", e))?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
        }
        Ok(items)
    }

    fn move_to_history(&mut self, item: &DownloadItem) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| format!("Failed to start tx: {}", e))?;
        tx.execute(
            "INSERT OR REPLACE INTO download_history (id, url, status, progress, total_bytes, downloaded_bytes, file_path, file_name, created_at, finished_at, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                item.id,
                item.url,
                item.status.to_string(),
                item.progress,
                item.total_bytes,
                item.downloaded_bytes,
                item.file_path,
                item.file_name,
                item.created_at,
                item.finished_at,
                item.error,
            ],
        ).map_err(|e| format!("Failed to insert history: {}", e))?;
        tx.execute("DELETE FROM download_queue WHERE id=?1", [&item.id])
            .map_err(|e| format!("Failed to delete queue: {}", e))?;
        tx.commit().map_err(|e| format!("Failed to commit: {}", e))?;
        Ok(())
    }
}

fn parse_status(s: &str) -> DownloadStatus {
    match s {
        "pending" => DownloadStatus::Pending,
        "downloading" => DownloadStatus::Downloading,
        "paused" => DownloadStatus::Paused,
        "completed" => DownloadStatus::Completed,
        "failed" => DownloadStatus::Failed,
        "cancelled" => DownloadStatus::Cancelled,
        _ => DownloadStatus::Pending,
    }
}

fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn generate_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("dl_{}", nanos)
}

pub fn find_ytdlp() -> Option<String> {
    let mut candidates: Vec<String> = vec![
        "yt-dlp".to_string(),
        "yt-dlp.exe".to_string(),
        "./yt-dlp".to_string(),
        "./bin/yt-dlp".to_string(),
    ];

    // Bundled resource: Tauri places bundled binaries next to the executable.
    // Check exe-dir/yt-dlp.exe and exe-dir/resources/yt-dlp.exe so the shipped
    // copy is found even when yt-dlp isn't on PATH.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for rel in ["yt-dlp.exe", "yt-dlp", "resources/yt-dlp.exe", "resources/yt-dlp"] {
                candidates.push(dir.join(rel).to_string_lossy().to_string());
            }
        }
    }

    for candidate in &candidates {
        if let Ok(output) = Command::new(candidate).arg("--version").output() {
            if output.status.success() {
                return Some(candidate.clone());
            }
        }
    }
    None
}

#[tauri::command]
pub fn get_qt_downloader_status() -> Result<bool, String> {
    Ok(find_ytdlp().is_some())
}

/// Probe a URL with `yt-dlp --simulate --print filename`. Returns the filename
/// yt-dlp would use if it recognizes the URL, or None if yt-dlp is missing or
/// doesn't handle this URL (so the caller can fall back to direct HTTP).
pub fn probe_ytdlp_filename(url: &str) -> Option<String> {
    let binary = find_ytdlp()?;
    let result = run_command_blocking(
        &binary,
        &["--simulate", "--no-warnings", "--print", "filename", "-o", "%(title)s.%(ext)s", url],
        YTDLP_TIMEOUT,
    )
    .ok()?;

    if !result.is_success() {
        return None;
    }

    let name = String::from_utf8_lossy(&result.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[tauri::command]
pub async fn get_ytdlp_formats(url: String) -> Result<Vec<YtDlpFormat>, String> {
    let binary = find_ytdlp().ok_or_else(|| "yt-dlp not found".to_string())?;
    let result = run_command_blocking(&binary, &["-J", "--no-warnings", &url], YTDLP_TIMEOUT)
        .map_err(|e| format!("Failed to run yt-dlp: {:?}", e))?;

    if !result.is_success() {
        return Err(format!("yt-dlp exited with code: {:?}", result.status.code()));
    }

    let json_str = String::from_utf8_lossy(&result.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse yt-dlp JSON: {}", e))?;

    let mut formats = Vec::new();
    if let Some(formats_arr) = parsed.get("formats").and_then(|f| f.as_array()) {
        for fmt in formats_arr {
            let format_id = fmt.get("format_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let ext = fmt.get("ext").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let resolution = fmt.get("resolution").and_then(|v| v.as_str()).map(|s| s.to_string());
            let filesize = fmt.get("filesize").and_then(|v| v.as_u64());
            let format_note = fmt.get("format_note").and_then(|v| v.as_str()).map(|s| s.to_string());
            formats.push(YtDlpFormat {
                format_id,
                ext,
                resolution,
                filesize,
                format_note,
            });
        }
    }
    Ok(formats)
}

fn resolve_download_dir(app_data_dir: &str) -> String {
    let base = Path::new(app_data_dir).join("downloads");
    let _ = fs::create_dir_all(&base);
    base.to_string_lossy().to_string()
}

fn safe_file_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

async fn fetch_head(url: &str) -> Result<reqwest::Response, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    client.head(url).send().await.map_err(|e| format!("HEAD failed: {}", e))
}

async fn fetch_chunk(url: &str, start: u64, end: u64) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let range = format!("bytes={}-{}", start, end);
    let response = client
        .get(url)
        .header(reqwest::header::RANGE, range)
        .send()
        .await
        .map_err(|e| format!("Chunk fetch failed: {}", e))?;

    if !response.status().is_success() && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("Chunk HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read chunk: {}", e))?
        .to_vec();
    Ok(bytes)
}

async fn download_chunked(
    url: &str,
    dest_dir: &str,
    file_name: &str,
    total_size: u64,
    chunk_count: u64,
    token: CancellationToken,
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<String, String> {
    if token.is_cancelled() {
        return Err(CANCELLED_MARKER.to_string());
    }
    let chunk_count = chunk_count.max(1);
    let dest_path = Path::new(dest_dir).join(safe_file_name(file_name));
    let chunk_size = (total_size + chunk_count - 1) / chunk_count;

    // Pre-create the file at full size so we can write each chunk at its offset
    // as it arrives — reporting progress incrementally instead of buffering the
    // whole download in memory and only writing/reporting at the end.
    let mut file = fs::File::create(&dest_path).map_err(|e| format!("Failed to create file: {}", e))?;
    let mut downloaded: u64 = 0;

    for chunk_idx in 0..chunk_count {
        if token.is_cancelled() {
            drop(file);
            let _ = fs::remove_file(&dest_path);
            return Err(CANCELLED_MARKER.to_string());
        }
        let start = chunk_idx * chunk_size;
        if start >= total_size {
            break;
        }
        let end = if start + chunk_size > total_size { total_size - 1 } else { start + chunk_size - 1 };

        let data = fetch_chunk(url, start, end).await?;
        file.seek(SeekFrom::Start(start)).map_err(|e| format!("Seek error: {}", e))?;
        file.write_all(&data).map_err(|e| format!("Failed to write file: {}", e))?;
        downloaded += data.len() as u64;
        on_progress(downloaded, total_size);
    }
    file.flush().map_err(|e| format!("Flush error: {}", e))?;
    drop(file);

    let metadata = fs::metadata(&dest_path).map_err(|e| format!("Failed to stat output: {}", e))?;
    if metadata.len() != total_size {
        return Err(format!(
            "Size mismatch: expected {} got {}",
            total_size,
            metadata.len()
        ));
    }

    Ok(dest_path.to_string_lossy().to_string())
}

async fn download_direct(
    url: &str,
    dest_dir: &str,
    file_name: &str,
    chunk_count: u64,
    token: CancellationToken,
    on_progress: impl Fn(u64, Option<u64>) + Send + 'static,
) -> Result<String, String> {
    use futures_util::StreamExt;
    if token.is_cancelled() {
        return Err(CANCELLED_MARKER.to_string());
    }
    let head = fetch_head(url).await?;
    let total = head.content_length();
    let accept_ranges = head
        .headers()
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "bytes")
        .unwrap_or(false);

    let dest_path = Path::new(dest_dir).join(safe_file_name(file_name));

    if accept_ranges && total.is_some() && total.unwrap() > 1024 * 1024 {
        let total_size = total.unwrap();
        return download_chunked(url, dest_dir, file_name, total_size, chunk_count, token.clone(), move |downloaded, total| {
            on_progress(downloaded, Some(total));
        }).await;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let response = client.get(url).send().await.map_err(|e| format!("Download failed: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let total_bytes = response.content_length();
    let mut file = fs::File::create(&dest_path).map_err(|e| format!("Failed to create file: {}", e))?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if token.is_cancelled() {
            drop(file);
            let _ = fs::remove_file(&dest_path);
            return Err(CANCELLED_MARKER.to_string());
        }
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        file.write_all(&chunk).map_err(|e| format!("Write error: {}", e))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total_bytes);
    }
    drop(file);
    Ok(dest_path.to_string_lossy().to_string())
}

pub async fn start_download(
    app_data_dir: &str,
    registry: &DownloaderRegistry,
    url: String,
    file_name_hint: Option<String>,
    _format_id: Option<String>,
    auto_save_folder: Option<String>,
    chunk_count: u64,
) -> Result<DownloadItem, String> {
    // Decide route + filename up front:
    // - explicit format selected -> yt-dlp path
    // - no format -> probe yt-dlp; if it recognizes the URL, use yt-dlp + the
    //   name it reports; otherwise fall back to direct HTTP with URL-derived name.
    let explicit_format = _format_id.as_ref().filter(|f| !f.trim().is_empty()).cloned();
    let probed_name = if explicit_format.is_none() && file_name_hint.is_none() {
        probe_ytdlp_filename(&url)
    } else {
        None
    };
    let use_ytdlp = explicit_format.is_some() || probed_name.is_some();

    let file_name = file_name_hint
        .or_else(|| probed_name.clone())
        .unwrap_or_else(|| {
            url.rsplit('/').next().unwrap_or("download").split('?').next().unwrap_or("download").to_string()
        });

    let id = generate_id();
    let now = now_ts();
    let dest_dir = auto_save_folder
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| resolve_download_dir(app_data_dir));

    // Register a cancellation token so cancel/pause can stop this task (Gap 3).
    let token = registry.insert(&id);

    let dest_path = Path::new(&dest_dir).join(safe_file_name(&file_name));

    // Persist the item to the queue table on start (Gap 1).
    let mut item = DownloadItem {
        id: id.clone(),
        url: url.clone(),
        status: DownloadStatus::Downloading,
        progress: 0.0,
        total_bytes: None,
        downloaded_bytes: 0,
        file_path: Some(dest_path.to_string_lossy().to_string()),
        file_name: file_name.clone(),
        created_at: now,
        finished_at: None,
        error: None,
    };
    {
        let db = DownloaderDb::open(app_data_dir)?;
        db.enqueue(&item)?;
    }

    // Progress callback persists live progress to the DB row (Gap 2).
    let progress_data_dir = app_data_dir.to_string();
    let progress_id = id.clone();
    let progress_cb = move |downloaded: u64, total: Option<u64>| {
        if let Ok(db) = DownloaderDb::open(&progress_data_dir) {
            let progress = match total {
                Some(t) if t > 0 => (downloaded as f64 / t as f64).clamp(0.0, 1.0),
                _ => 0.0,
            };
            let _ = db.update_progress(&progress_id, progress, total, downloaded);
        }
    };

    // Route to yt-dlp when a format was selected (Gap 4); otherwise use the
    // direct/chunked HTTP path. The transfer runs in a BACKGROUND task so this
    // command returns immediately — the frontend polls downloader_get_state for
    // live progress instead of blocking on the whole download.
    let returned_item = item.clone();
    let bg_data_dir = app_data_dir.to_string();
    let bg_registry = registry.clone();
    let bg_token = token.clone();
    let bg_use_ytdlp = use_ytdlp;
    // Explicit format if chosen, else "best" when yt-dlp recognized the URL.
    let bg_format = explicit_format.clone().unwrap_or_else(|| "best".to_string());
    tauri::async_runtime::spawn(async move {
        let mut item = item;
        let result = if bg_use_ytdlp {
            let pct_data_dir = bg_data_dir.clone();
            let pct_id = id.clone();
            download_with_ytdlp(&url, &dest_dir, &file_name, &bg_format, bg_token.clone(), move |pct| {
                if let Ok(db) = DownloaderDb::open(&pct_data_dir) {
                    let _ = db.update_progress(&pct_id, (pct / 100.0).clamp(0.0, 1.0), None, 0);
                }
            }).await
        } else {
            download_direct(&url, &dest_dir, &file_name, chunk_count, bg_token.clone(), progress_cb).await
        };

        match result {
            Ok(path) => {
                item.status = DownloadStatus::Completed;
                item.progress = 1.0;
                item.file_path = Some(path);
                item.finished_at = Some(now_ts());
            }
            Err(err) if err == CANCELLED_MARKER => {
                // Cancelled/paused: status already set in DB by the command;
                // just release the registry slot and leave the row in place.
                bg_registry.remove(&id);
                return;
            }
            Err(err) => {
                item.status = DownloadStatus::Failed;
                item.error = Some(err);
                item.finished_at = Some(now_ts());
            }
        }

        // Move the finished/failed item from queue to history (Gap 1).
        if let Ok(mut db) = DownloaderDb::open(&bg_data_dir) {
            let _ = db.update(&item);
            let _ = db.move_to_history(&item);
        }
        bg_registry.remove(&id);
    });

    // Return the queued item immediately; progress is polled by the frontend.
    Ok(returned_item)
}

pub async fn cancel_download(db: &mut DownloaderDb, registry: &DownloaderRegistry, id: String) -> Result<(), String> {
    // Stop the in-flight task first (Gap 3), then flip the DB status.
    registry.cancel(&id);
    db.conn
        .execute(
            "UPDATE download_queue SET status=?1, finished_at=?2 WHERE id=?3",
            rusqlite::params![DownloadStatus::Cancelled.to_string(), now_ts(), id],
        )
        .map_err(|e| format!("Failed to cancel: {}", e))?;
    Ok(())
}

pub async fn pause_download(db: &mut DownloaderDb, registry: &DownloaderRegistry, id: String) -> Result<(), String> {
    // Persist Paused status (downloaded_bytes already tracked by progress
    // updates), then stop the in-flight task (Gap 3).
    db.conn
        .execute(
            "UPDATE download_queue SET status=?1 WHERE id=?2",
            rusqlite::params![DownloadStatus::Paused.to_string(), id],
        )
        .map_err(|e| format!("Failed to pause: {}", e))?;
    registry.cancel(&id);
    Ok(())
}

pub async fn resume_download(
    app_data_dir: &str,
    registry: &DownloaderRegistry,
    id: String,
) -> Result<(), String> {
    // Look up the paused item to recover its URL, destination, and byte offset.
    let (url, dest_path, file_name) = {
        let db = DownloaderDb::open(app_data_dir)?;
        let queue = db.load_queue()?;
        let item = queue
            .into_iter()
            .find(|i| i.id == id)
            .ok_or_else(|| "Item not found in queue".to_string())?;
        let path = item
            .file_path
            .clone()
            .ok_or_else(|| "No destination path recorded for resume".to_string())?;
        (item.url.clone(), path, item.file_name.clone())
        // db dropped here so the spawned task can reopen it
    };

    // Flip status back to Downloading.
    {
        let db = DownloaderDb::open(app_data_dir)?;
        db.conn
            .execute(
                "UPDATE download_queue SET status=?1 WHERE id=?2",
                rusqlite::params![DownloadStatus::Downloading.to_string(), id],
            )
            .map_err(|e| format!("Failed to resume: {}", e))?;
    }

    // Register a fresh token and relaunch the transfer from the saved offset.
    let token = registry.insert(&id);
    let data_dir = app_data_dir.to_string();
    let registry_clone = registry.clone();
    let resume_id = id.clone();

    tauri::async_runtime::spawn(async move {
        let existing_bytes = fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0);
        let progress_dir = data_dir.clone();
        let progress_id = resume_id.clone();
        let result = download_resumable(
            &url,
            &dest_path,
            existing_bytes,
            token.clone(),
            move |downloaded, total| {
                if let Ok(db) = DownloaderDb::open(&progress_dir) {
                    let progress = match total {
                        Some(t) if t > 0 => (downloaded as f64 / t as f64).clamp(0.0, 1.0),
                        _ => 0.0,
                    };
                    let _ = db.update_progress(&progress_id, progress, total, downloaded);
                }
            },
        )
        .await;

        if let Ok(mut db) = DownloaderDb::open(&data_dir) {
            if let Ok(queue) = db.load_queue() {
                if let Some(mut item) = queue.into_iter().find(|i| i.id == resume_id) {
                    match result {
                        Ok(_) => {
                            item.status = DownloadStatus::Completed;
                            item.progress = 1.0;
                            item.file_path = Some(dest_path.clone());
                            item.finished_at = Some(now_ts());
                            let _ = db.update(&item);
                            let _ = db.move_to_history(&item);
                            registry_clone.remove(&resume_id);
                        }
                        Err(err) if err == CANCELLED_MARKER => {
                            registry_clone.remove(&resume_id);
                        }
                        Err(err) => {
                            item.status = DownloadStatus::Failed;
                            item.error = Some(err);
                            item.finished_at = Some(now_ts());
                            let _ = db.update(&item);
                            let _ = db.move_to_history(&item);
                            registry_clone.remove(&resume_id);
                        }
                    }
                }
            }
        }
    });

    let _ = file_name;
    Ok(())
}

/// Resume a download by appending to an existing partial file using a Range
/// request from `start_offset` (Gap 3).
async fn download_resumable(
    url: &str,
    dest_path: &str,
    start_offset: u64,
    token: CancellationToken,
    on_progress: impl Fn(u64, Option<u64>) + Send + 'static,
) -> Result<String, String> {
    use futures_util::StreamExt;
    use std::io::Seek;

    if token.is_cancelled() {
        return Err(CANCELLED_MARKER.to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let mut request = client.get(url);
    if start_offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", start_offset));
    }
    let response = request.send().await.map_err(|e| format!("Resume request failed: {}", e))?;
    if !response.status().is_success() && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("HTTP {}", response.status()));
    }

    // If the server ignored the Range (200 not 206), restart from zero.
    let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT && start_offset > 0;
    let remaining = response.content_length();
    let total_bytes = remaining.map(|r| r + if resumed { start_offset } else { 0 });

    let mut file = if resumed {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .open(dest_path)
            .map_err(|e| format!("Failed to open partial file: {}", e))?;
        f.seek(std::io::SeekFrom::Start(start_offset))
            .map_err(|e| format!("Failed to seek partial file: {}", e))?;
        f
    } else {
        fs::File::create(dest_path).map_err(|e| format!("Failed to create file: {}", e))?
    };

    let mut downloaded: u64 = if resumed { start_offset } else { 0 };
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if token.is_cancelled() {
            drop(file);
            return Err(CANCELLED_MARKER.to_string());
        }
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        file.write_all(&chunk).map_err(|e| format!("Write error: {}", e))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total_bytes);
    }
    drop(file);
    Ok(dest_path.to_string())
}

/// Download via yt-dlp child process for a selected format (Gap 4). Streams
/// yt-dlp stdout, parses `[download] xx.x%` lines into progress, and kills the
/// child if the cancellation token fires.
async fn download_with_ytdlp(
    url: &str,
    dest_dir: &str,
    file_name: &str,
    format_id: &str,
    token: CancellationToken,
    on_percent: impl Fn(f64) + Send + 'static,
) -> Result<String, String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command as TokioCommand;

    if token.is_cancelled() {
        return Err(CANCELLED_MARKER.to_string());
    }

    let binary = find_ytdlp().ok_or_else(|| "yt-dlp not found".to_string())?;
    let out_template = Path::new(dest_dir).join(safe_file_name(file_name));
    let out_str = out_template.to_string_lossy().to_string();

    let mut command = TokioCommand::new(&binary);
    command
        .arg("-f")
        .arg(format_id)
        .arg("-o")
        .arg(&out_str)
        .arg("--newline")
        .arg("--no-playlist")
        .arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(|e| format!("Failed to spawn yt-dlp: {}", e))?;
    let stdout = child.stdout.take().ok_or_else(|| "No yt-dlp stdout".to_string())?;
    let mut lines = BufReader::new(stdout).lines();

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                let _ = child.kill().await;
                return Err(CANCELLED_MARKER.to_string());
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if let Some(pct) = parse_ytdlp_percent(&line) {
                            on_percent(pct);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => return Err(format!("yt-dlp read error: {}", e)),
                }
            }
        }
    }

    let status = child.wait().await.map_err(|e| format!("yt-dlp wait failed: {}", e))?;
    if !status.success() {
        return Err(format!("yt-dlp exited with status: {:?}", status.code()));
    }
    on_percent(100.0);
    Ok(out_str)
}

/// Parse a yt-dlp `[download]  42.3% of ...` progress line into a percentage.
fn parse_ytdlp_percent(line: &str) -> Option<f64> {
    if !line.contains("[download]") {
        return None;
    }
    let pct_token = line.split_whitespace().find(|t| t.ends_with('%'))?;
    pct_token.trim_end_matches('%').parse::<f64>().ok()
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DownloaderState {
    pub queue: Vec<DownloadItem>,
    pub history: Vec<DownloadItem>,
}

#[tauri::command]
pub async fn downloader_get_state(app: tauri::AppHandle) -> Result<DownloaderState, String> {
    let db = open_db(&app)?;
    let queue = db.load_queue()?;
    let history = db.load_history()?;
    Ok(DownloaderState { queue, history })
}

#[tauri::command]
pub async fn downloader_enqueue(
    app: tauri::AppHandle,
    registry: tauri::State<'_, DownloaderRegistry>,
    url: String,
    file_name: Option<String>,
    format_id: Option<String>,
    auto_save_folder: Option<String>,
    chunk_count: Option<u64>,
) -> Result<DownloadItem, String> {
    let app_data_dir = get_app_data_dir(&app)?;
    let registry = registry.inner().clone();
    let chunks = chunk_count.unwrap_or(DOWNLOAD_CHUNKS);
    start_download(&app_data_dir, &registry, url, file_name, format_id, auto_save_folder, chunks).await
}

#[tauri::command]
pub async fn downloader_cancel(
    app: tauri::AppHandle,
    registry: tauri::State<'_, DownloaderRegistry>,
    id: String,
) -> Result<(), String> {
    let mut db = open_db(&app)?;
    let registry = registry.inner().clone();
    cancel_download(&mut db, &registry, id).await
}

#[tauri::command]
pub async fn downloader_pause(
    app: tauri::AppHandle,
    registry: tauri::State<'_, DownloaderRegistry>,
    id: String,
) -> Result<(), String> {
    let mut db = open_db(&app)?;
    let registry = registry.inner().clone();
    pause_download(&mut db, &registry, id).await
}

#[tauri::command]
pub async fn downloader_resume(
    app: tauri::AppHandle,
    registry: tauri::State<'_, DownloaderRegistry>,
    id: String,
) -> Result<(), String> {
    let app_data_dir = get_app_data_dir(&app)?;
    let registry = registry.inner().clone();
    resume_download(&app_data_dir, &registry, id).await
}

#[tauri::command]
pub async fn downloader_get_ytdlp_formats(url: String) -> Result<Vec<YtDlpFormat>, String> {
    get_ytdlp_formats(url).await
}

fn open_db(app: &tauri::AppHandle) -> Result<DownloaderDb, String> {
    let app_data_dir = get_app_data_dir(app)?;
    DownloaderDb::open(&app_data_dir)
}

fn get_app_data_dir(app: &tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Gap 1 + Gap 2 verification: a download must insert into download_queue,
    // record progress, and end up in download_history.
    #[tokio::test]
    async fn start_download_persists_to_queue_then_history() {
        use axum::routing::get;
        use axum::Router;

        // Serve a small known-size body that supports a plain GET.
        let body = vec![7u8; 4096];
        let body_for_route = body.clone();
        let app = Router::new().route(
            "/file.bin",
            get(move || {
                let b = body_for_route.clone();
                async move { b }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let tmp = std::env::temp_dir().join(format!("meridian_dl_test_{}", now_ts()));
        std::fs::create_dir_all(&tmp).unwrap();
        let data_dir = tmp.to_string_lossy().to_string();

        let url = format!("http://{}/file.bin", addr);
        let registry = DownloaderRegistry::new();
        let item = start_download(&data_dir, &registry, url, Some("file.bin".to_string()), None, None, 8)
            .await
            .expect("download should succeed");

        assert_eq!(item.status, DownloadStatus::Completed);
        assert_eq!(item.progress, 1.0);

        // The queue should be drained and the item must be in history.
        let db = DownloaderDb::open(&data_dir).unwrap();
        let queue = db.load_queue().unwrap();
        let history = db.load_history().unwrap();
        assert!(queue.is_empty(), "queue should be empty after completion");
        assert!(
            history.iter().any(|h| h.id == item.id && h.status == DownloadStatus::Completed),
            "completed item must be persisted in history"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // Gap 3 verification: cancelling an in-flight download actually stops it.
    #[tokio::test]
    async fn cancel_stops_in_flight_download() {
        use axum::routing::get;
        use axum::body::Body;
        use axum::Router;
        use futures_util::stream;

        // Server that streams slowly and effectively never finishes within the
        // test window, so we can cancel mid-flight.
        let app = Router::new().route(
            "/slow.bin",
            get(|| async {
                let chunks = stream::unfold(0u64, |n| async move {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    let data = vec![0u8; 1024];
                    Some((Ok::<_, std::io::Error>(data), n + 1))
                });
                Body::from_stream(chunks)
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let tmp = std::env::temp_dir().join(format!("meridian_cancel_test_{}", now_ts()));
        std::fs::create_dir_all(&tmp).unwrap();
        let data_dir = tmp.to_string_lossy().to_string();
        let url = format!("http://{}/slow.bin", addr);

        let registry = DownloaderRegistry::new();
        let registry_for_task = registry.clone();
        let data_dir_for_task = data_dir.clone();

        // Launch the download in the background.
        let handle = tokio::spawn(async move {
            start_download(
                &data_dir_for_task,
                &registry_for_task,
                url,
                Some("slow.bin".to_string()),
                None,
                None,
                8,
            )
            .await
        });

        // Let it start, then locate the in-flight item and cancel it.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let id = {
            let db = DownloaderDb::open(&data_dir).unwrap();
            let queue = db.load_queue().unwrap();
            assert!(!queue.is_empty(), "an in-flight item must be in the queue");
            queue[0].id.clone()
        };

        let mut db = DownloaderDb::open(&data_dir).unwrap();
        cancel_download(&mut db, &registry, id.clone()).await.unwrap();

        // The background task must return promptly (cancelled), not hang.
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("download task must finish quickly after cancel")
            .expect("join ok");
        assert!(result.is_ok(), "cancelled download should return Ok with the item");

        // DB row reflects Cancelled.
        let db = DownloaderDb::open(&data_dir).unwrap();
        let queue = db.load_queue().unwrap();
        let cancelled = queue.iter().any(|i| i.id == id && i.status == DownloadStatus::Cancelled);
        assert!(cancelled, "item must be marked Cancelled in the queue");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // Gap 4 verification: yt-dlp progress-line parsing (binary-independent).
    #[test]
    fn parses_ytdlp_progress_lines() {
        assert_eq!(parse_ytdlp_percent("[download]   0.0% of 10.00MiB"), Some(0.0));
        assert_eq!(parse_ytdlp_percent("[download]  42.3% of 10.00MiB at 1.0MiB/s"), Some(42.3));
        assert_eq!(parse_ytdlp_percent("[download] 100% of 10.00MiB"), Some(100.0));
        assert_eq!(parse_ytdlp_percent("[info] downloading format"), None);
        assert_eq!(parse_ytdlp_percent("random line"), None);
    }
}
