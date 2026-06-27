// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! SSH/SFTP remote file browsing (Phase 7) using russh + russh-sftp.
//!
//! Returns the SAME DirContents/DirEntry shape as the local dir_reader so the
//! existing file-pane Vue component can render remote directories unchanged.
//! Reuses the SshCredentials shape from the cluster module (key-based auth via
//! the passphrase-less meridian_black key).

use std::sync::Arc;
use async_trait::async_trait;
use russh::client;
use russh::keys::key;
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};

/// SSH connection parameters (mirrors cluster::SshCredentials so the frontend
/// can pass the same object). Key-based auth only — no password/agent.
#[derive(Debug, Clone, Deserialize)]
pub struct SftpCredentials {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    /// Absolute path to an UNENCRYPTED private key file.
    pub key_path: String,
}

fn default_port() -> u16 {
    22
}

/// One remote entry — matches the local dir_reader DirEntry field names so the
/// same Vue component renders it. Times are unix seconds; unsupported fields
/// (links, mime, hidden) are best-effort/None for remote.
#[derive(Debug, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub ext: Option<String>,
    pub path: String,
    pub size: u64,
    pub item_count: Option<u32>,
    pub modified_time: u64,
    pub accessed_time: u64,
    pub created_time: u64,
    pub mime: Option<String>,
    pub is_file: bool,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
}

/// Mirrors local DirContents so the frontend treats remote + local identically.
#[derive(Debug, Serialize)]
pub struct SftpContents {
    pub path: String,
    pub entries: Vec<SftpEntry>,
    pub total_count: usize,
    pub dir_count: usize,
    pub file_count: usize,
}

/// Minimal russh client handler — accepts the server key (homelab LAN hosts).
struct SftpClientHandler;

#[async_trait]
impl client::Handler for SftpClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Open an SSH session + SFTP subsystem using key-based auth.
async fn open_sftp(creds: &SftpCredentials) -> Result<(client::Handle<SftpClientHandler>, SftpSession), String> {
    if creds.key_path.trim().is_empty() {
        return Err("No SSH key path configured".to_string());
    }
    let key_pair = russh::keys::load_secret_key(&creds.key_path, None)
        .map_err(|e| format!("Failed to load SSH key {}: {}", creds.key_path, e))?;

    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, (creds.host.as_str(), creds.port), SftpClientHandler)
        .await
        .map_err(|e| format!("SSH connect to {}:{} failed: {}", creds.host, creds.port, e))?;

    let authed = session
        .authenticate_publickey(&creds.username, Arc::new(key_pair))
        .await
        .map_err(|e| format!("SSH auth failed: {}", e))?;
    if !authed {
        return Err(format!("SSH key auth rejected for {}@{}", creds.username, creds.host));
    }

    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("SSH channel open failed: {}", e))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("SFTP subsystem request failed: {}", e))?;

    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP session init failed: {}", e))?;

    Ok((session, sftp))
}

/// List a remote directory over SFTP, returning the same shape as local
/// read_dir so the file pane renders it unchanged. `path` is the remote
/// absolute path (defaults to the user's home / "." when empty).
#[tauri::command]
pub async fn sftp_read_dir(creds: SftpCredentials, path: String) -> Result<SftpContents, String> {
    let (session, sftp) = open_sftp(&creds).await?;

    // Resolve the directory to list: empty => canonicalized "." (home).
    let dir = if path.trim().is_empty() {
        sftp.canonicalize(".").await.unwrap_or_else(|_| ".".to_string())
    } else {
        path.clone()
    };

    let mut read_dir = sftp
        .read_dir(&dir)
        .await
        .map_err(|e| format!("Failed to read remote dir {}: {}", dir, e))?;

    let mut entries: Vec<SftpEntry> = Vec::new();
    let mut dir_count = 0usize;
    let mut file_count = 0usize;

    while let Some(item) = read_dir.next() {
        let name = item.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let meta = item.metadata();
        let is_dir = meta.is_dir();
        let is_file = !is_dir && meta.file_type().is_file();
        let is_symlink = meta.file_type().is_symlink();
        let size = meta.size.unwrap_or(0);
        let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let accessed = meta.accessed().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let ext = if is_file {
            std::path::Path::new(&name).extension().and_then(|e| e.to_str()).map(|s| s.to_lowercase())
        } else {
            None
        };
        let sep = if dir.ends_with('/') { "" } else { "/" };
        let full_path = format!("{}{}{}", dir, sep, name);

        if is_dir { dir_count += 1; } else { file_count += 1; }
        entries.push(SftpEntry {
            name: name.clone(),
            ext,
            path: full_path,
            size,
            item_count: None,
            modified_time: modified,
            accessed_time: accessed,
            created_time: 0,
            mime: None,
            is_file,
            is_dir,
            is_symlink,
            is_hidden: name.starts_with('.'),
        });
    }

    // Best-effort: close the session (ignore errors on teardown).
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;

    let total_count = entries.len();
    Ok(SftpContents { path: dir, entries, total_count, dir_count, file_count })
}

/// Create a remote directory over SFTP.
#[tauri::command]
pub async fn sftp_mkdir(creds: SftpCredentials, path: String) -> Result<(), String> {
    let (session, sftp) = open_sftp(&creds).await?;
    let res = sftp.create_dir(&path).await.map_err(|e| format!("Failed to create {}: {}", path, e));
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    res
}

/// Rename / move a remote entry over SFTP.
#[tauri::command]
pub async fn sftp_rename(creds: SftpCredentials, from: String, to: String) -> Result<(), String> {
    let (session, sftp) = open_sftp(&creds).await?;
    let res = sftp.rename(&from, &to).await.map_err(|e| format!("Failed to rename {} -> {}: {}", from, to, e));
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    res
}

/// Delete a remote file or (empty) directory over SFTP. `is_dir` selects rmdir vs remove.
#[tauri::command]
pub async fn sftp_delete(creds: SftpCredentials, path: String, is_dir: bool) -> Result<(), String> {
    let (session, sftp) = open_sftp(&creds).await?;
    let res = if is_dir {
        sftp.remove_dir(&path).await
    } else {
        sftp.remove_file(&path).await
    }
    .map_err(|e| format!("Failed to delete {}: {}", path, e));
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    res
}

/// Download a remote file to a local path over SFTP.
#[tauri::command]
pub async fn sftp_download(creds: SftpCredentials, remote_path: String, local_path: String) -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    let (session, sftp) = open_sftp(&creds).await?;
    let result: Result<(), String> = async {
        let mut remote = sftp.open(&remote_path).await.map_err(|e| format!("Open remote {} failed: {}", remote_path, e))?;
        let mut buf = Vec::new();
        remote.read_to_end(&mut buf).await.map_err(|e| format!("Read remote failed: {}", e))?;
        std::fs::write(&local_path, &buf).map_err(|e| format!("Write local {} failed: {}", local_path, e))?;
        Ok(())
    }
    .await;
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    result
}

/// Upload a local file to a remote path over SFTP.
#[tauri::command]
pub async fn sftp_upload(creds: SftpCredentials, local_path: String, remote_path: String) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let (session, sftp) = open_sftp(&creds).await?;
    let result: Result<(), String> = async {
        let data = std::fs::read(&local_path).map_err(|e| format!("Read local {} failed: {}", local_path, e))?;
        let mut remote = sftp.create(&remote_path).await.map_err(|e| format!("Create remote {} failed: {}", remote_path, e))?;
        remote.write_all(&data).await.map_err(|e| format!("Write remote failed: {}", e))?;
        remote.flush().await.map_err(|e| format!("Flush remote failed: {}", e))?;
        Ok(())
    }
    .await;
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    result
}
