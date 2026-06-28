// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Meridian — Phase 11: Backend Manager (Step 1: local lifecycle only).
//!
//! Manages download + start/stop + status tracking + shutdown reap for
//! inference servers (llama.cpp CUDA/ROCm, llamafile, koboldcpp).
//!
//! **Step 1 scope** (per JC 2026-06-28 + AGENTS.md Phase 11):
//!   - 5 Tauri commands: `detect_local_gpu_vendor`, `download_backend`,
//!     `start_backend`, `stop_backend`, `get_backend_status`
//!   - 1 sync reap helper: `reap_backends` called from
//!     `lib.rs::run()`'s `WindowEvent::Destroyed` block alongside the existing
//!     `lan_share::stop_lan_share` call
//!
//! **Deferred to later phases**:
//!   - SFTP copy to worker (`copy_backend_to_worker`), RPC slave remote launch
//!     (`launch_rpc_slave_remote`), models tab (`scan_models`/`delete_model`),
//!     SQLite `backend_events` logging, Tauri dialog confirmations, process
//!     tree graceful shutdown.
//!
//! Catalog currently lives as a `const DOWNLOAD_TABLE` (per architecture
//! agreed with JC). Step 2 will swap to a bundled `resources/backend_catalog.json`
//! reader.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;

/// Default Windows install root for backends.
const DEFAULT_BACKEND_ROOT: &str = "E:\\ai\\Apps\\backends";

/// Probes have a 5s ceiling — nvidia-smi and rocm-smi are local and fast.
const SHORT_PROBE_TIMEOUT_MS: u64 = 5_000;

/// Public registry alias registered via Tauri's state system in `lib.rs`.
///
/// `Arc<Mutex<HashMap<u32, Child>>>` matches JC's literal Step 1 spec: a
/// PID-keyed map of running backend children. Step 2 may extend to
/// `(BackendKind, Child)` values to support multiple concurrent backends.
pub type BackendRegistry = Arc<Mutex<HashMap<u32, Child>>>;

// ============================================================================
// Data shapes
// ============================================================================

/// Backend identity. Serialized as the lowercase dotted string the rest of
/// Meridian uses ("llama.cpp", "llamafile", "koboldcpp") so the JS side can
/// pass those same strings to `download_backend` / `start_backend`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BackendKind {
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
    #[serde(rename = "llamafile")]
    Llamafile,
    #[serde(rename = "koboldcpp")]
    KoboldCpp,
}

impl BackendKind {
    fn all() -> Vec<BackendKind> {
        vec![BackendKind::LlamaCpp, BackendKind::Llamafile, BackendKind::KoboldCpp]
    }
}

/// Flat status payload returned by `get_backend_status`. One entry per
/// backend. The `status` field is one of `"notInstalled" | "installed" | "running"`
/// (camelCase) and other fields are populated based on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub kind: BackendKind,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,
}

/// GPU vendor info for backend selection. `vendor` is one of
/// `"nvidia" | "amd" | "cpu"`. `source` records which probe reported the result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuVendorInfo {
    pub vendor: String,
    pub gpu_name: Option<String>,
    pub source: String,
}

// ============================================================================
// Download catalog (Step 1 placeholder; Step 2 swaps for backend_catalog.json)
// ============================================================================

struct DownloadEntry {
    kind: BackendKind,
    /// Target vendor: "nvidia" / "amd" / "cpu" — or "all" for any GPU.
    vendor: &'static str,
    url: &'static str,
    binary_windows: &'static str,
    binary_linux: &'static str,
    /// "zip" | "tar.gz" | "binary"
    archive_format: &'static str,
}

/// Static catalog for Step 1. Step 2 will read this from a bundled JSON file
/// (`resources/backend_catalog.json`) loaded by Tauri at startup.
const DOWNLOAD_TABLE: &[DownloadEntry] = &[
    // NVIDIA — llama.cpp CUDA build for Windows.
    DownloadEntry {
        kind: BackendKind::LlamaCpp,
        vendor: "nvidia",
        url: "https://github.com/ggerganov/llama.cpp/releases/latest/download/llama-bin-win-cuda-x64.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip",
    },
    // AMD — llama.cpp ROCm build for Windows.
    DownloadEntry {
        kind: BackendKind::LlamaCpp,
        vendor: "amd",
        url: "https://github.com/ggerganov/llama.cpp/releases/latest/download/llama-bin-win-rocm-x64.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip",
    },
    // CPU — llamafile single binary (no GPU required).
    DownloadEntry {
        kind: BackendKind::Llamafile,
        vendor: "all",
        url: "https://github.com/Mozilla-Ocho/llamafile/releases/latest/download/llamafile",
        binary_windows: "llamafile.exe",
        binary_linux: "llamafile",
        archive_format: "binary",
    },
    // CPU — koboldcpp zip with Windows binary.
    DownloadEntry {
        kind: BackendKind::KoboldCpp,
        vendor: "cpu",
        url: "https://github.com/LostRuins/koboldcpp/releases/latest/download/koboldcpp-win-x64.zip",
        binary_windows: "koboldcpp.exe",
        binary_linux: "koboldcpp",
        archive_format: "zip",
    },
];

fn lookup_download(kind: &BackendKind, vendor: &str) -> Option<&'static DownloadEntry> {
    DOWNLOAD_TABLE
        .iter()
        .find(|e| &e.kind == kind && (e.vendor == vendor || e.vendor == "all"))
}

// ============================================================================
// Tauri commands
// ============================================================================

/// Detects the local GPU vendor so the rest of the backend pipeline can pick
/// the right downloadable artifact.
///
/// Primary path: parse vendor from GPU names returned by `nvidia-smi`. If any
/// name contains `"NVIDIA"` → `"nvidia"`; else if any name contains `"AMD"` or
/// `"Radeon"` → `"amd"`. Fallback: run local `rocm-smi --json` and check that
/// the JSON contains a top-level key starting with `"card"`; if so → `"amd"`.
/// Otherwise → `"cpu"`.
///
/// NOTE: `cluster.rs::get_local_hardware` only runs `nvidia-smi` locally (its
/// `rocm-smi` branch is SSH-only for remote nodes). We deliberately run both
/// probes here per the AGENTS.md Phase 11 §9 / Step 1 corrections.
#[tauri::command]
pub fn detect_local_gpu_vendor() -> Result<GpuVendorInfo, String> {
    let nvidia_args = ["--query-gpu=name", "--format=csv,noheader,nounits"];
    let names: Vec<String> = match run_short_probe("nvidia-smi", &nvidia_args, SHORT_PROBE_TIMEOUT_MS) {
        Ok(stdout) => String::from_utf8_lossy(&stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect(),
        Err(_) => Vec::new(),
    };

    let mut nvidia_count = 0usize;
    let mut amd_from_name_count = 0usize;
    let mut first_name: Option<String> = None;

    for name in &names {
        first_name.get_or_insert_with(|| name.clone());
        let upper = name.to_uppercase();
        if upper.contains("NVIDIA") {
            nvidia_count += 1;
        } else if upper.contains("AMD") || upper.contains("RADEON") {
            amd_from_name_count += 1;
        }
    }

    if nvidia_count > 0 {
        return Ok(GpuVendorInfo {
            vendor: "nvidia".to_string(),
            gpu_name: first_name,
            source: "nvidia-smi".to_string(),
        });
    }

    if amd_from_name_count > 0 {
        return Ok(GpuVendorInfo {
            vendor: "amd".to_string(),
            gpu_name: first_name,
            source: "nvidia-smi-name-parse".to_string(),
        });
    }

    if rocm_smi_local_has_card() {
        return Ok(GpuVendorInfo {
            vendor: "amd".to_string(),
            gpu_name: first_name.or_else(|| Some("AMD GPU".to_string())),
            source: "rocm-smi".to_string(),
        });
    }

    Ok(GpuVendorInfo {
        vendor: "cpu".to_string(),
        gpu_name: None,
        source: "none".to_string(),
    })
}

/// Download the backend artifact matching the currently-detected GPU vendor.
/// Saves to `<target_dir or default>\<kind>\` and extracts (zip) or writes the
/// binary (single-file variant). Returns the absolute install directory path.
#[tauri::command]
pub async fn download_backend(
    backend_kind: String,
    target_dir: Option<String>,
) -> Result<String, String> {
    let kind = parse_backend_kind(&backend_kind)?;
    let vendor = detect_local_gpu_vendor()?.vendor;
    let entry = lookup_download(&kind, &vendor).ok_or_else(|| {
        format!("No download entry for {:?} on vendor '{}'", kind, vendor)
    })?;

    let root = target_dir
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BACKEND_ROOT.to_string());
    let install_root = PathBuf::from(&root).join(kind_dir_name(&kind));
    std::fs::create_dir_all(&install_root).map_err(|e| {
        format!("Failed to create install dir {}: {}", install_root.display(), e)
    })?;

    let response = reqwest::get(entry.url)
        .await
        .map_err(|e| format!("HTTP GET failed for {}: {}", entry.url, e))?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {} for {}",
            response.status(),
            entry.url
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    write_archive(&install_root, &bytes, entry.archive_format, &kind)?;

    Ok(install_root.to_string_lossy().into_owned())
}

/// Spawn a previously-downloaded backend binary. Inserts Child into the
/// registry keyed by PID. Returns the assigned PID.
///
/// On Windows, `CREATE_NO_WINDOW` is applied so no console popup flashes
/// (mirrors `process_runner.rs::run_command_blocking`).
#[tauri::command]
pub fn start_backend(
    backend_kind: String,
    model_path: Option<String>,
    extra_args: Option<Vec<String>>,
    registry: State<'_, BackendRegistry>,
) -> Result<u32, String> {
    let kind = parse_backend_kind(&backend_kind)?;
    let install_root = backend_install_root(&kind, None)?;
    let binary_name = platform_binary_name(&kind);
    let binary_path = install_root.join(binary_name);

    if !binary_path.exists() {
        return Err(format!(
            "Backend binary not found at {}. Run download_backend first.",
            binary_path.display()
        ));
    }

    let mut command = Command::new(&binary_path);
    command.current_dir(&install_root);

    if let Some(model) = model_path.as_ref().filter(|s| !s.is_empty()) {
        command.arg("--model").arg(model);
    }

    if let Some(extra) = extra_args.as_ref() {
        for arg in extra {
            if !arg.is_empty() {
                command.arg(arg);
            }
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", binary_path.display(), e))?;

    let pid = child.id();
    let started_at = unix_timestamp_secs();

    let mut guard = registry
        .lock()
        .map_err(|e| format!("Mutex error: {}", e))?;
    guard.insert(pid, child);

    log::info!("Started backend {:?} pid={}", kind, pid);
    Ok(pid)
}

/// Kill a running backend by PID. Refuses to operate on PIDs the app didn't
/// fork (per AGENTS.md "NEVER execute kill -9 / TerminateProcess on a PID the
/// app didn't fork").
#[tauri::command]
pub fn stop_backend(
    pid: u32,
    registry: State<'_, BackendRegistry>,
) -> Result<(), String> {
    let mut guard = registry
        .lock()
        .map_err(|e| format!("Mutex error: {}", e))?;

    let mut child = guard.remove(&pid).ok_or_else(|| {
        format!("PID {} is not a tracked backend process — refusing to kill", pid)
    })?;
    drop(guard);

    let _ = child.kill();
    let _ = child.wait();
    log::info!("Stopped backend pid={}", pid);
    Ok(())
}

/// Return one `BackendInfo` per backend.
///
/// Step 1 simplifies the running marker: the global registry is keyed by PID
/// only (no per-kind tagging), so if any backend is running, the FIRST kind in
/// the iteration order reports `"running"` with that PID. Other kinds report
/// based on disk only. Step 2 will tag children by kind for accurate per-kind
/// running state.
#[tauri::command]
pub fn get_backend_status(
    backend_kind: Option<String>,
    registry: State<'_, BackendRegistry>,
) -> Result<Vec<BackendInfo>, String> {
    let kinds: Vec<BackendKind> = match backend_kind.as_deref() {
        Some(s) if !s.trim().is_empty() => vec![parse_backend_kind(s)?],
        _ => BackendKind::all(),
    };
    let started_at = unix_timestamp_secs();

    let guard = registry
        .lock()
        .map_err(|e| format!("Mutex error: {}", e))?;
    let any_running_pid: Option<u32> = guard.keys().next().copied();
    drop(guard);

    let mut out: Vec<BackendInfo> = Vec::with_capacity(kinds.len());
    for (i, kind) in kinds.iter().enumerate() {
        let info = match (i, any_running_pid) {
            (0, Some(pid)) => BackendInfo {
                kind: *kind,
                status: "running".to_string(),
                install_path: None,
                size_bytes: None,
                version: None,
                pid: Some(pid),
                started_at: Some(started_at),
                model_path: None,
            },
            _ => disk_status_for_kind(kind),
        };
        out.push(info);
    }
    Ok(out)
}

// ============================================================================
// Reap helper (NOT a Tauri command; called from lib.rs WindowEvent::Destroyed)
// ============================================================================

/// Drain the registry: kill each child and wait. Called from
/// `lib.rs::run()`'s `WindowEvent::Destroyed` block alongside
/// `lan_share::stop_lan_share`. Function SHAPE mirrors `omnix::kill_omnix`
/// (lock + drain + kill+wait); call-site WIRING mirrors `lan_share::stop_lan_share`.
pub fn reap_backends(registry: &BackendRegistry) -> Result<(), String> {
    let mut guard = registry
        .lock()
        .map_err(|e| format!("Mutex error: {}", e))?;
    for (pid, mut child) in guard.drain() {
        let _ = child.kill();
        let _ = child.wait();
        log::info!("Reaped backend pid={}", pid);
    }
    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

fn parse_backend_kind(s: &str) -> Result<BackendKind, String> {
    match s.trim().to_lowercase().as_str() {
        "llama.cpp" | "llama_cpp" | "llamacpp" | "llama-cpp" => Ok(BackendKind::LlamaCpp),
        "llamafile" => Ok(BackendKind::Llamafile),
        "kobold.cpp" | "kobold_cpp" | "koboldcpp" | "kobold-cpp" => Ok(BackendKind::KoboldCpp),
        other => Err(format!(
            "Unknown backend kind: '{}'. Expected llama.cpp | llamafile | koboldcpp",
            other
        )),
    }
}

fn kind_dir_name(kind: &BackendKind) -> &'static str {
    match kind {
        BackendKind::LlamaCpp => "llama.cpp",
        BackendKind::Llamafile => "llamafile",
        BackendKind::KoboldCpp => "koboldcpp",
    }
}

fn platform_binary_name(kind: &BackendKind) -> &'static str {
    DOWNLOAD_TABLE
        .iter()
        .find(|e| &e.kind == kind)
        .map(|e| {
            if cfg!(windows) {
                e.binary_windows
            } else {
                e.binary_linux
            }
        })
        .unwrap_or("unknown")
}

fn backend_install_root(
    kind: &BackendKind,
    override_dir: Option<String>,
) -> Result<PathBuf, String> {
    let root = override_dir
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BACKEND_ROOT.to_string());
    Ok(PathBuf::from(root).join(kind_dir_name(kind)))
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn disk_status_for_kind(kind: &BackendKind) -> BackendInfo {
    let install_root = backend_install_root(kind, None).ok();
    let binary_path = install_root
        .as_ref()
        .map(|r| r.join(platform_binary_name(kind)));
    match binary_path.and_then(|p| std::fs::metadata(p).ok()) {
        Some(meta) => BackendInfo {
            kind: *kind,
            status: "installed".to_string(),
            install_path: install_root.map(|p| p.to_string_lossy().into_owned()),
            size_bytes: Some(meta.len()),
            version: None,
            pid: None,
            started_at: None,
            model_path: None,
        },
        None => BackendInfo {
            kind: *kind,
            status: "notInstalled".to_string(),
            install_path: None,
            size_bytes: None,
            version: None,
            pid: None,
            started_at: None,
            model_path: None,
        },
    }
}

/// Run a short-lived helper command (nvidia-smi / rocm-smi), capture stdout,
/// and return it as bytes. Honors a hard timeout. Mirrors `process_runner.rs`
/// shape but is local-only — no stderr capture needed.
fn run_short_probe(
    program: &str,
    args: &[&str],
    timeout_ms: u64,
) -> Result<Vec<u8>, String> {
    use std::io::Read;

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", program, e))?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let mut buf = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_end(&mut buf);
                }
                let _ = child.wait();
                return Ok(buf);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} timed out after {}ms", program, timeout_ms));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("wait on {} failed: {}", program, e));
            }
        }
    }
}

/// Returns true when local `rocm-smi --json` reports at least one card key.
fn rocm_smi_local_has_card() -> bool {
    let Ok(stdout) = run_short_probe("rocm-smi", &["--json"], SHORT_PROBE_TIMEOUT_MS) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(&stdout) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    json.as_object()
        .map(|o| o.keys().any(|k| k.starts_with("card")))
        .unwrap_or(false)
}

/// Extracts a zip / writes a binary blob to `install_root`. Top-level zip
/// directory is stripped (llama.cpp zips ship as `llama-bin-win-cuda-x64/<files>`).
fn write_archive(
    install_root: &Path,
    bytes: &[u8],
    archive_format: &str,
    kind: &BackendKind,
) -> Result<(), String> {
    use std::io::Read;

    match archive_format {
        "binary" => {
            let target_name = platform_binary_name(kind);
            let target = install_root.join(target_name);
            std::fs::write(&target, bytes)
                .map_err(|e| format!("Failed to write binary {}: {}", target.display(), e))?;
            Ok(())
        }
        "zip" => {
            let cursor = std::io::Cursor::new(bytes);
            let mut archive = zip::ZipArchive::new(cursor)
                .map_err(|e| format!("zip::ZipArchive::new: {}", e))?;
            for i in 0..archive.len() {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| format!("zip entry {}: {}", i, e))?;
                let entry_path = match entry.enclosed_name() {
                    Some(p) => p.to_path_buf(),
                    None => continue,
                };
                let stripped = strip_top_dir(&entry_path);
                if stripped.as_os_str().is_empty() {
                    continue;
                }
                let target = install_root.join(&stripped);
                if entry.is_dir() {
                    std::fs::create_dir_all(&target)
                        .map_err(|e| format!("mkdir {}: {}", target.display(), e))?;
                } else {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("mkdir parent {}: {}", parent.display(), e))?;
                    }
                    let mut buf = Vec::new();
                    entry
                        .read_to_end(&mut buf)
                        .map_err(|e| format!("read zip entry {}: {}", i, e))?;
                    std::fs::write(&target, &buf)
                        .map_err(|e| format!("write extracted {}: {}", target.display(), e))?;
                }
            }
            Ok(())
        }
        other => Err(format!(
            "Archive format '{}' not yet supported in Step 1; defer to Step 2",
            other
        )),
    }
}

/// Strips the first path segment from `p`. llama.cpp zips ship top-level dirs.
fn strip_top_dir(p: &Path) -> PathBuf {
    let mut iter = p.iter();
    iter.next(); // drop the first segment
    iter.collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_backend_kind_variants_canonical() {
        assert_eq!(parse_backend_kind("llama.cpp").unwrap(), BackendKind::LlamaCpp);
        assert_eq!(parse_backend_kind("llamafile").unwrap(), BackendKind::Llamafile);
        assert_eq!(parse_backend_kind("koboldcpp").unwrap(), BackendKind::KoboldCpp);
    }

    #[test]
    fn parses_backend_kind_variants_loose() {
        assert_eq!(parse_backend_kind("llama_cpp").unwrap(), BackendKind::LlamaCpp);
        assert_eq!(parse_backend_kind("llama-cpp").unwrap(), BackendKind::LlamaCpp);
        assert_eq!(parse_backend_kind("kobold_cpp").unwrap(), BackendKind::KoboldCpp);
        assert_eq!(parse_backend_kind("KOBOLDCPP").unwrap(), BackendKind::KoboldCpp);
    }

    #[test]
    fn rejects_unknown_backend_kind() {
        assert!(parse_backend_kind("vllm").is_err());
        assert!(parse_backend_kind("").is_err());
        assert!(parse_backend_kind("garbage").is_err());
    }

    #[test]
    fn lookup_download_finds_cuda_for_nvidia() {
        let entry = lookup_download(&BackendKind::LlamaCpp, "nvidia").unwrap();
        assert!(entry.url.contains("cuda"), "expected CUDA URL, got {}", entry.url);
        assert_eq!(entry.archive_format, "zip");
    }

    #[test]
    fn lookup_download_finds_rocm_for_amd() {
        let entry = lookup_download(&BackendKind::LlamaCpp, "amd").unwrap();
        assert!(entry.url.contains("rocm"), "expected ROCm URL, got {}", entry.url);
    }

    #[test]
    fn lookup_download_finds_llamafile_for_cpu() {
        let entry = lookup_download(&BackendKind::Llamafile, "cpu").unwrap();
        assert_eq!(entry.vendor, "all");
        assert_eq!(entry.archive_format, "binary");
    }

    #[test]
    fn lookup_download_finds_koboldcpp_for_cpu() {
        let entry = lookup_download(&BackendKind::KoboldCpp, "cpu").unwrap();
        assert!(!entry.url.is_empty());
        assert_eq!(entry.archive_format, "zip");
    }

    #[test]
    fn kind_dir_names_match_installer_layout() {
        assert_eq!(kind_dir_name(&BackendKind::LlamaCpp), "llama.cpp");
        assert_eq!(kind_dir_name(&BackendKind::Llamafile), "llamafile");
        assert_eq!(kind_dir_name(&BackendKind::KoboldCpp), "koboldcpp");
    }

    #[test]
    fn strip_top_dir_drops_first_segment() {
        let stripped = strip_top_dir(Path::new("llama-bin-win-cuda-x64/build/bin/llama-server.exe"));
        assert_eq!(stripped, PathBuf::from("build/bin/llama-server.exe"));
    }

    #[test]
    fn strip_top_dir_handles_single_segment() {
        let stripped = strip_top_dir(Path::new("LICENSE"));
        assert!(stripped.as_os_str().is_empty());
    }

    #[test]
    fn backend_kind_all_returns_three_kinds() {
        let kinds = BackendKind::all();
        assert_eq!(kinds.len(), 3);
        assert_eq!(kinds[0], BackendKind::LlamaCpp);
        assert_eq!(kinds[1], BackendKind::Llamafile);
        assert_eq!(kinds[2], BackendKind::KoboldCpp);
    }

    #[test]
    fn backend_kind_serializes_to_canonical_strings() {
        let llama_cpp = serde_json::to_string(&BackendKind::LlamaCpp).unwrap();
        let llamafile = serde_json::to_string(&BackendKind::Llamafile).unwrap();
        let koboldcpp = serde_json::to_string(&BackendKind::KoboldCpp).unwrap();
        assert_eq!(llama_cpp, "\"llama.cpp\"");
        assert_eq!(llamafile, "\"llamafile\"");
        assert_eq!(koboldcpp, "\"koboldcpp\"");
    }
}
