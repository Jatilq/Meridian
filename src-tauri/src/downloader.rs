// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use crate::process_runner::run_command_blocking;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const YTDLP_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_CHUNKS: u64 = 4;
const DB_FILE_NAME: &str = "meridian.db";

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
    format!("dl_{}", now_ts())
}

pub fn find_ytdlp() -> Option<String> {
    let candidates = vec![
        "yt-dlp",
        "yt-dlp.exe",
        "./yt-dlp",
        "./bin/yt-dlp",
    ];
    for candidate in candidates {
        if let Ok(output) = Command::new(candidate).arg("--version").output() {
            if output.status.success() {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

#[tauri::command]
pub fn get_qt_downloader_status() -> Result<bool, String> {
    Ok(find_ytdlp().is_some())
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
    on_progress: impl Fn(u64, u64) + Send + 'static,
) -> Result<String, String> {
    let dest_path = Path::new(dest_dir).join(safe_file_name(file_name));
    let chunk_size = (total_size + DOWNLOAD_CHUNKS - 1) / DOWNLOAD_CHUNKS;
    let mut handles = Vec::new();

    for chunk_idx in 0..DOWNLOAD_CHUNKS {
        let start = chunk_idx * chunk_size;
        let end = if start + chunk_size > total_size { total_size - 1 } else { start + chunk_size - 1 };
        if start >= total_size {
            break;
        }
        let url = url.to_string();
        let handle = tauri::async_runtime::spawn(async move { fetch_chunk(&url, start, end).await });
        handles.push((chunk_idx, handle));
    }

    let mut parts: Vec<(u64, Vec<u8>)> = Vec::new();
    for (chunk_idx, handle) in handles {
        let data = handle.await.map_err(|e| format!("Chunk task failed: {}", e))??;
        parts.push((chunk_idx, data));
    }

    parts.sort_by_key(|(idx, _)| *idx);

    let mut file = fs::File::create(&dest_path).map_err(|e| format!("Failed to create file: {}", e))?;
    let mut downloaded: u64 = 0;
    for (_, data) in &parts {
        file.write_all(data).map_err(|e| format!("Failed to write file: {}", e))?;
        downloaded += data.len() as u64;
        on_progress(downloaded, total_size);
    }
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
    on_progress: impl Fn(u64, Option<u64>) + Send + 'static,
) -> Result<String, String> {
    use futures_util::StreamExt;
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
        return download_chunked(url, dest_dir, file_name, total_size, move |downloaded, total| {
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
    url: String,
    file_name_hint: Option<String>,
    _format_id: Option<String>,
    auto_save_folder: Option<String>,
) -> Result<DownloadItem, String> {
    let file_name = file_name_hint.unwrap_or_else(|| {
        url.rsplit('/').next().unwrap_or("download").split('?').next().unwrap_or("download").to_string()
    });

    let id = generate_id();
    let now = now_ts();
    let dest_dir = auto_save_folder
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| resolve_download_dir(app_data_dir));

    let result = download_direct(&url, &dest_dir, &file_name, move |_downloaded, _total| {
        // progress is polled by frontend via get_state
    }).await;

    let mut final_item = DownloadItem {
        id: id.clone(),
        url: url.clone(),
        status: DownloadStatus::Completed,
        progress: 1.0,
        total_bytes: None,
        downloaded_bytes: 0,
        file_path: None,
        file_name: file_name.clone(),
        created_at: now,
        finished_at: None,
        error: None,
    };

    match result {
        Ok(path) => {
            final_item.file_path = Some(path);
            final_item.finished_at = Some(now_ts());
        }
        Err(err) => {
            final_item.status = DownloadStatus::Failed;
            final_item.error = Some(err);
            final_item.finished_at = Some(now_ts());
        }
    }

    Ok(final_item)
}

pub async fn cancel_download(db: &mut DownloaderDb, id: String) -> Result<(), String> {
    db.conn
        .execute(
            "UPDATE download_queue SET status=?1, finished_at=?2 WHERE id=?3",
            rusqlite::params![DownloadStatus::Cancelled.to_string(), now_ts(), id],
        )
        .map_err(|e| format!("Failed to cancel: {}", e))?;
    Ok(())
}

pub async fn pause_download(db: &mut DownloaderDb, id: String) -> Result<(), String> {
    db.conn
        .execute(
            "UPDATE download_queue SET status=?1 WHERE id=?2",
            rusqlite::params![DownloadStatus::Paused.to_string(), id],
        )
        .map_err(|e| format!("Failed to pause: {}", e))?;
    Ok(())
}

pub async fn resume_download(db: &mut DownloaderDb, id: String) -> Result<(), String> {
    db.conn
        .execute(
            "UPDATE download_queue SET status=?1 WHERE id=?2",
            rusqlite::params![DownloadStatus::Downloading.to_string(), id],
        )
        .map_err(|e| format!("Failed to resume: {}", e))?;
    Ok(())
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
    url: String,
    file_name: Option<String>,
    format_id: Option<String>,
    auto_save_folder: Option<String>,
) -> Result<DownloadItem, String> {
    let app_data_dir = get_app_data_dir(&app)?;
    start_download(&app_data_dir, url, file_name, format_id, auto_save_folder).await
}

#[tauri::command]
pub async fn downloader_cancel(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut db = open_db(&app)?;
    cancel_download(&mut db, id).await
}

#[tauri::command]
pub async fn downloader_pause(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut db = open_db(&app)?;
    pause_download(&mut db, id).await
}

#[tauri::command]
pub async fn downloader_resume(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut db = open_db(&app)?;
    resume_download(&mut db, id).await
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
