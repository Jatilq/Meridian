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

use walkdir::WalkDir;
use serde::{Deserialize, Serialize};
use tauri::State;

/// Default Windows install root for backends.
const DEFAULT_BACKEND_ROOT: &str = "E:\\ai\\Apps\\backends";

/// Probes have a 5s ceiling — nvidia-smi and rocm-smi are local and fast.
const SHORT_PROBE_TIMEOUT_MS: u64 = 5_000;

/// Public type alias registered via Tauri's state system in `lib.rs`.
///
/// Tracks running backends: each PID maps to its child process plus the
/// metadata needed for API probing (kind / port / model path / started-at).
/// Step 2 may extend with health-check timestamps and restart counters.
pub type BackendRegistry = Arc<Mutex<HashMap<u32, TrackedBackend>>>;

/// A single running backend process plus the metadata needed to probe
/// its HTTP API, surface it in the UI, and route user requests.
/// `child` owns the running process so `stop_backend` and `reap_backends`
/// can call `.kill()` + `.wait()` on the exact PID we spawned.
pub struct TrackedBackend {
    pub child: Child,
    pub kind: BackendKind,
    pub port: u16,
    pub model_path: Option<String>,
    pub binary_path: PathBuf,
    pub started_at: u64,
}

// ============================================================================
// Data shapes
// ============================================================================

/// Backend identity. Serialized as the lowercase dotted string the rest of
/// Meridian uses ("llama.cpp", "llamafile", "koboldcpp", "turboquant") so
/// the JS side can pass those same strings to `download_backend` /
/// `start_backend`. TurboQuant is AtomicBot-AI's TurboQuant+TriAttention
/// fork of llama.cpp (same CLI interface plus `--triattention-*` flags).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BackendKind {
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
    #[serde(rename = "llamafile")]
    Llamafile,
    #[serde(rename = "koboldcpp")]
    KoboldCpp,
    #[serde(rename = "turboquant")]
    TurboQuant,
    #[serde(rename = "lemonade")]
    Lemonade,
}

impl BackendKind {
    fn all() -> Vec<BackendKind> {
        vec![
            BackendKind::LlamaCpp,
            BackendKind::Llamafile,
            BackendKind::KoboldCpp,
            BackendKind::TurboQuant,
            BackendKind::Lemonade,
        ]
    }

    /// Default HTTP listen port if the user hasn't overridden it. llama.cpp,
    /// llamafile, and turboquant default to 8080 (matching upstream);
    /// koboldcpp defaults to 5001.
    fn default_port(&self) -> u16 {
        match self {
            BackendKind::LlamaCpp => 8080,
            BackendKind::Llamafile => 8080,
            BackendKind::KoboldCpp => 5001,
            BackendKind::TurboQuant => 8080,
            // Lemonade's OpenAI-compatible API listens at 13305 upstream.
            BackendKind::Lemonade => 13305,
        }
    }
}

/// Flat status payload returned by `get_backend_status`. One entry per
/// backend. The `status` field is one of `"notInstalled" | "installed" | "running"`
/// (camelCase) and other fields are populated based on it. `port` reports
/// the actual HTTP listen port used by a running backend so the panel can
/// surface a working API URL and the probe command knows where to hit.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// Probe result returned by `probe_backend_api`. Always populates `port`,
/// `url_tested`, and `elapsed_ms` so the UI can show what was tested even
/// when the request fails. `ok` reflects whether the server returned a
/// 2xx-class response within the 2-second timeout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendApiStatus {
    pub ok: bool,
    pub kind: BackendKind,
    pub port: u16,
    pub url_tested: String,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One row in the local-models list returned by `list_gguf_models`. Sorted
/// by `modified_at` descending on the front-end so the most-recently-touched
/// model surfaces first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GgufModelEntry {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: u64,
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

/// One file in a HuggingFace model repo, returned by
/// `hf_resolve_model_files`. `url` is the direct download URL the
/// `downloader_enqueue` Tauri command accepts. Files are returned
/// quantized-first so the front-end can pick `files[0]` and get the
/// best on-device inference asset without having to re-sort.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HfModelFile {
    pub filename: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

// ============================================================================
// Download catalog (Step 1 placeholder; Step 2 swaps for backend_catalog.json)
// ============================================================================

struct DownloadEntry {
    kind: BackendKind,
    /// Stable id matching `backends.json` `variant.id` (e.g. "llama.cpp.nvidia",
    /// "turboquant.cuda-12.4"). Used when the front-end wants to download the
    /// exact row the user picked from the catalog — bypasses the
    /// vendor-detection branch when present.
    id: &'static str,
    /// Target vendor: "nvidia" / "amd" / "cpu" — or "all" for any GPU.
    /// Also shows up as `variant.hardware` in the JSON.
    vendor: &'static str,
    url: &'static str,
    binary_windows: &'static str,
    binary_linux: &'static str,
    /// "zip" | "zip-flat" | "tar.gz" | "binary"
    archive_format: &'static str,
}

/// Static catalog for Step 1. Step 2 will read this from a bundled JSON file
/// (`resources/backend_catalog.json`) loaded by Tauri at startup.
const DOWNLOAD_TABLE: &[DownloadEntry] = &[
    // NVIDIA — llama.cpp CUDA build for Windows.
    DownloadEntry {
        kind: BackendKind::LlamaCpp,
        id: "llama.cpp.nvidia",
        vendor: "nvidia",
        url: "https://github.com/ggerganov/llama.cpp/releases/latest/download/llama-bin-win-cuda-x64.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip",
    },
    // AMD — llama.cpp ROCm build for Windows.
    DownloadEntry {
        kind: BackendKind::LlamaCpp,
        id: "llama.cpp.amd",
        vendor: "amd",
        url: "https://github.com/ggerganov/llama.cpp/releases/latest/download/llama-bin-win-rocm-x64.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip",
    },
    // CPU — llamafile single binary (no GPU required).
    DownloadEntry {
        kind: BackendKind::Llamafile,
        id: "llamafile.universal",
        vendor: "all",
        url: "https://github.com/Mozilla-Ocho/llamafile/releases/latest/download/llamafile",
        binary_windows: "llamafile.exe",
        binary_linux: "llamafile",
        archive_format: "binary",
    },
    // CPU — koboldcpp zip with Windows binary.
    DownloadEntry {
        kind: BackendKind::KoboldCpp,
        id: "koboldcpp.cpu",
        vendor: "cpu",
        url: "https://github.com/LostRuins/koboldcpp/releases/latest/download/koboldcpp-win-x64.zip",
        binary_windows: "koboldcpp.exe",
        binary_linux: "koboldcpp",
        archive_format: "zip",
    },
    // TurboQuant — AtomicBot-AI fork of llama.cpp with TriAttention pruning.
    // All Windows tags share the same `d86eb0b` commit prefix and ship as
    // FLAT zips (no top-level parent directory), so `archive_format` is
    // `zip-flat` to bypass `strip_top_dir` during extraction. Linux variants
    // not listed here — Meridian currently targets Windows for inference.
    // turboquant CUDA 13.3 — primary NVIDIA path.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.cuda-13.3",
        vendor: "nvidia",
        url: "https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-cuda-13.3-d86eb0b/llama-turboquant-windows-x64-cuda-13.3.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip-flat",
    },
    // turboquant CUDA 12.4 — older NVIDIA driver compatibility.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.cuda-12.4",
        vendor: "nvidia-cuda12",
        url: "https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-cuda-12.4-d86eb0b/llama-turboquant-windows-x64-cuda-12.4.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip-flat",
    },
    // turboquant CPU — no GPU required.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.cpu",
        vendor: "cpu",
        url: "https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-cpu-d86eb0b/llama-turboquant-windows-x64-cpu.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip-flat",
    },
    // turboquant Vulkan — AMD / fallback path.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.vulkan",
        vendor: "amd",
        url: "https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-vulkan-d86eb0b/llama-turboquant-windows-x64-vulkan.zip",
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip-flat",
    },
    // Lemonade — single-binary embeddable runtime that auto-detects the best
    // hardware path on startup (NVIDIA CUDA / AMD ROCm / Intel NPU / CPU).
    // Pinned to v10.8.1. Drop a new `${version}` row in the catalog table when
    // the user wants to upgrade. `archive_format = "zip"` because the embeddable
    // archive has a top-level folder; the regular llama.cpp "zip" extractor
    // handles it correctly.
    DownloadEntry {
        kind: BackendKind::Lemonade,
        id: "lemonade.embeddable.windows-x64",
        vendor: "all",
        url: "https://github.com/lemonade-sdk/lemonade/releases/download/v10.8.1/lemonade-embeddable-10.8.1-windows-x64.zip",
        binary_windows: "lemonade-server.exe",
        binary_linux: "lemonade-server",
        archive_format: "zip",
    },
];

fn lookup_download(kind: &BackendKind, vendor: &str) -> Option<&'static DownloadEntry> {
    DOWNLOAD_TABLE
        .iter()
        .find(|e| &e.kind == kind && (e.vendor == vendor || e.vendor == "all"))
}

/// Direct variant-id lookup. Used when the front-end explicitly passes the
/// `variant.id` from the catalog (e.g. "turboquant.cuda-12.4") so the
/// download is exactly what the user picked and not re-derandomised by
/// hardware detection.
fn lookup_download_by_variant(kind: &BackendKind, variant_id: &str) -> Option<&'static DownloadEntry> {
    DOWNLOAD_TABLE
        .iter()
        .find(|e| &e.kind == kind && e.id == variant_id)
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

/// Download the backend artifact for the given backend kind.
///
/// If `variant_id` is supplied (the UI passes the picked `variant.id` from
/// the catalog, e.g. `"turboquant.cuda-12.4"`), the matching row is fetched
/// directly — bypassing GPU detection so the user's exact choice is
/// honoured. Otherwise we fall back to vendor detection.
#[tauri::command]
pub async fn download_backend(
    backend_kind: String,
    variant_id: Option<String>,
    target_dir: Option<String>,
) -> Result<String, String> {
    let kind = parse_backend_kind(&backend_kind)?;

    let entry = if let Some(vid) = variant_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        lookup_download_by_variant(&kind, vid).ok_or_else(|| {
            format!("No catalog variant '{}' for {:?}", vid, kind)
        })?
    } else {
        let detected = detect_local_gpu_vendor()?;
        let vendor = vendor_for_kind(&kind, &detected.vendor);
        lookup_download(&kind, &vendor).ok_or_else(|| {
            format!("No download entry for {:?} on vendor '{}'", kind, vendor)
        })?
    };

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
/// `port` defaults to the per-kind default when None (8080 for llama.cpp /
/// llamafile, 5001 for koboldcpp). The chosen port is forwarded to the
/// backend via `--port <N>` (all three backends accept this flag) and
/// recorded in the registry so `probe_backend_api` knows where to hit.
///
/// On Windows, `CREATE_NO_WINDOW` is applied so no console popup flashes
/// (mirrors `process_runner.rs::run_command_blocking`).
#[tauri::command]
pub fn start_backend(
    backend_kind: String,
    model_path: Option<String>,
    extra_args: Option<Vec<String>>,
    port: Option<u16>,
    registry: State<'_, BackendRegistry>,
) -> Result<u32, String> {
    let kind = parse_backend_kind(&backend_kind)?;
    let install_root = backend_install_root(&kind, None)?;
    let binary_name = platform_binary_name(&kind);
    let binary_path = install_root.join(&binary_name);

    if !binary_path.exists() {
        return Err(format!(
            "Backend binary not found at {}. Run download_backend first.",
            binary_path.display()
        ));
    }

    let actual_port = port.unwrap_or_else(|| kind.default_port());
    let model_path = model_path.filter(|s| !s.is_empty());

    let mut command = Command::new(&binary_path);
    command.current_dir(&install_root);

    // All three backends accept `--port <N>` per the docs. Keeping the
    // flag order (port + model) consistent across kinds avoids surprising
    // the user with positional-argument quirks when they switch runtimes.
    command.arg("--port").arg(actual_port.to_string());

    if let Some(model) = model_path.as_ref() {
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
    guard.insert(
        pid,
        TrackedBackend {
            child,
            kind,
            port: actual_port,
            model_path: model_path.clone(),
            binary_path: binary_path.clone(),
            started_at,
        },
    );

    log::info!(
        "Started backend {:?} pid={} port={} model={:?}",
        kind,
        pid,
        actual_port,
        model_path
    );
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

    // Remove from registry first, then drop the lock before killing so a
    // long-running reap (e.g. Windows TerminateProcess on a hung server)
    // doesn't block other registry call sites.
    let mut tracked = guard.remove(&pid).ok_or_else(|| {
        format!("PID {} is not a tracked backend process — refusing to kill", pid)
    })?;
    drop(guard);

    let _ = tracked.child.kill();
    let _ = tracked.child.wait();
    log::info!("Stopped backend pid={}", pid);
    Ok(())
}

/// Return one `BackendInfo` per backend.
///
/// Looks up the running process in the global registry by KIND (each backend
/// kind is tracked independently now — no longer "first kind wins"). When a
/// process is registered for the queried kind, the port + model path come
/// from the registry entry. When no process is running, falls back to disk
/// status (installed iff the binary exists on disk).
#[tauri::command]
pub fn get_backend_status(
    backend_kind: Option<String>,
    registry: State<'_, BackendRegistry>,
) -> Result<Vec<BackendInfo>, String> {
    let kinds: Vec<BackendKind> = match backend_kind.as_deref() {
        Some(s) if !s.trim().is_empty() => vec![parse_backend_kind(s)?],
        _ => BackendKind::all(),
    };

    let guard = registry
        .lock()
        .map_err(|e| format!("Mutex error: {}", e))?;

    let mut out: Vec<BackendInfo> = Vec::with_capacity(kinds.len());
    for kind in kinds.iter() {
        // Find the FIRST tracked process for this kind. Concurrent processes
        // of the same kind can exist in theory; we surface the earliest.
        let tracked = guard
            .values()
            .find(|t| t.kind == *kind)
            .map(|t| (t.kind, t.port, t.model_path.clone(), t.started_at));

        let info = match tracked {
            Some((found_kind, port, model_path, started_at)) => BackendInfo {
                kind: found_kind,
                status: "running".to_string(),
                install_path: None,
                size_bytes: None,
                version: None,
                pid: guard
                    .iter()
                    .find(|(_, t)| t.kind == found_kind)
                    .map(|(pid, _)| *pid),
                started_at: Some(started_at),
                model_path,
                port: Some(port),
            },
            None => disk_status_for_kind(kind),
        };
        out.push(info);
    }
    Ok(out)
}

/// Probe the HTTP API of a running backend. Used by the UI's "Test API"
/// button to surface a concrete health-check status + latency.
///
/// If multiple processes are registered for the same kind, picks the most
/// recently started. Returns `Ok(false)` with `http_status=None` if no
/// process is running for the queried kind.
#[tauri::command]
pub async fn probe_backend_api(
    backend_kind: String,
    registry: State<'_, BackendRegistry>,
) -> Result<BackendApiStatus, String> {
    let kind = parse_backend_kind(&backend_kind)?;

    let tracked = {
        let guard = registry
            .lock()
            .map_err(|e| format!("Mutex error: {}", e))?;
        // Pick the most-recently-started tracked process for this kind.
        guard
            .values()
            .filter(|t| t.kind == kind)
            .max_by_key(|t| t.started_at)
            .map(|t| (t.port, t.started_at))
    };

    let (port, started_at) = match tracked {
        Some(t) => t,
        None => {
            return Ok(BackendApiStatus {
                ok: false,
                kind,
                port: kind.default_port(),
                url_tested: format!("http://localhost:{}/health", kind.default_port()),
                elapsed_ms: 0,
                http_status: None,
                error: Some("No running process for this backend kind.".to_string()),
            });
        }
    };

    // Health endpoints vary per backend; try a sequence from cheapest to
    // most-specific. /health is universal enough that we use it as the
    // primary probe; if a backend exposes an alternative, we still report
    // a single attempt here (the URL is recorded so the user can verify).
    let candidates: &[&str] = match kind {
        // TurboQuant is a llama.cpp fork with the same HTTP surface area
        // (endpoints, slashes, OpenAI compat). Probe the same paths as
        // LlamaCpp — but spell out the variant so future divergence
        // (e.g. a `/triattention` endpoint) is a one-line edit.
        BackendKind::LlamaCpp | BackendKind::TurboQuant => &["/health", "/v1/models"],
        // Lemonade exposes an OpenAI-compatible surface; /v1/models is the
        // most common probe target. /health is a fallback for older builds.
        BackendKind::Lemonade => &["/v1/models", "/health"],
        BackendKind::Llamafile => &["/health", "/v1/models"],
        BackendKind::KoboldCpp => &["/api/v1/model", "/health"],
    };

    let started = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    for path in candidates {
        let url = format!("http://localhost:{}{}", port, path);
        match client.get(&url).send().await {
            Ok(response) => {
                let elapsed_ms = started.elapsed().as_millis() as u64;
                let status = response.status();
                if status.is_success() {
                    return Ok(BackendApiStatus {
                        ok: true,
                        kind,
                        port,
                        url_tested: url,
                        elapsed_ms,
                        http_status: Some(status.as_u16()),
                        error: None,
                    });
                }
                // Non-2xx: continue to next candidate (e.g. /health returns
                // 404 on a backend that doesn't expose it; try /v1/models).
                continue;
            }
            Err(_err) => continue,
        }
    }

    Ok(BackendApiStatus {
        ok: false,
        kind,
        port,
        url_tested: format!("http://localhost:{}{}", port, candidates[0]),
        elapsed_ms: started.elapsed().as_millis() as u64,
        http_status: None,
        error: Some(format!(
            "No health endpoint responded within 2s on port {} (last started at {})",
            port, started_at
        )),
    })
}

/// Walks a directory tree recursively, returning every `.gguf` file found.
///
/// Replaces the previous single-level `read_dir` based scan. Models on the
/// host typically live under nested vendor/size subdirectories (e.g.
/// `E:\ai\Models\Qwen\7B\Qwen2.5-7B-Instruct-Q4_K_M.gguf`), so a recursive
/// walk is required to surface them in the Models tab.
///
/// `max_depth` defaults to 6 — deep enough for the typical `models/<vendor>/<size>/<file>.gguf`
/// layout, shallow enough to skip runaway trees on the user's drive. Pass
/// `Some(0)` for top-level only; `Some(usize::MAX)` for unbounded. Returns
/// an empty Vec when the path exists but contains no `.gguf` files. Bad
/// paths surface as `Err` so the UI can render a useful error.
#[tauri::command]
pub fn list_gguf_models(
    path: String,
    max_depth: Option<usize>,
) -> Result<Vec<GgufModelEntry>, String> {
    let root = PathBuf::from(&path);
    if !root.exists() {
        return Err(format!("Path does not exist: {}", root.display()));
    }
    if !root.is_dir() {
        return Err(format!("Path is not a directory: {}", root.display()));
    }
    let depth = max_depth.unwrap_or(6);

    let mut out: Vec<GgufModelEntry> = Vec::new();
    for entry in WalkDir::new(&root)
        .max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let entry_path = entry.path();
        let ext_ok = entry_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.eq_ignore_ascii_case("gguf"))
            .unwrap_or(false);
        if !ext_ok {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let name = entry_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(GgufModelEntry {
            name,
            path: entry_path.to_string_lossy().into_owned(),
            size_bytes: metadata.len(),
            modified_at,
        });
    }
    // Surface the freshest models first; fall back to name for stable ordering.
    out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at).then(a.name.cmp(&b.name)));
    Ok(out)
}

// ============================================================================
// HuggingFace repo → concrete file resolution
// ============================================================================
//
// `hf_resolve_model_files` is the missing piece behind the Backend Manager
// "Get on HF" button (and the hardware tab's download flow). Until this
// command shipped, the Omnix tab queued `https://huggingface.co/<repo>`
// — an HTML page, not a model — so the downloader fetched the repo's
// README instead of an actual asset. The fix is one round-trip to
// HuggingFace's `GET /api/models/<repo_id>` endpoint: it returns a
// `siblings` array listing every file in the repo with `rfilename` +
// optional `size`. We filter to recognised model extensions and rank
// quantized assets first so the UI's `files[0]` is the best pick.

/// Extensions accepted as downloadable model files. The list intentionally
/// excludes `.json` (config), `.txt` (tokenizer / README), and `.md` so
/// those don't get mistakenly offered as a model.
const HF_MODEL_EXTENSIONS: &[&str] = &["onnx", "gguf", "bin", "safetensors", "pt"];

/// Resolve a HuggingFace repo ID to a list of concrete downloadable model
/// file URLs. Files are sorted quantized-first, then by filename length
/// (shorter filenames usually = smaller quantizations), then alphabetically
/// for stable ordering when scores tie. The front-end typically picks
/// `files[0]` and enqueues it via `downloader_enqueue`.
///
/// Errors are user-visible (returned via the invoke promise), so messages
/// stay short and actionable.
#[tauri::command]
pub async fn hf_resolve_model_files(repo_id: String) -> Result<Vec<HfModelFile>, String> {
    let trimmed = repo_id.trim();
    if trimmed.is_empty() {
        return Err("HuggingFace repo id is empty".to_string());
    }
    let api_url = format!("https://huggingface.co/api/models/{}", trimmed);

    // Single-budget timeout: a stalled connect, a stalled header read, OR
    // a stalled body read all share the same wall-clock ceiling. Worst-case
    // UX is "Resolving…" for HF_API_TIMEOUT seconds — short enough to fail
    // fast on a dead network, long enough to absorb transient slowness.
    let work = async {
        let response = reqwest::get(&api_url)
            .await
            .map_err(|e| format!("HuggingFace API request failed: {}", e))?;
        let response = response
            .error_for_status()
            .map_err(|e| format!("HuggingFace API returned {}", e))?;
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse HuggingFace API JSON: {}", e))?;
        Ok::<_, String>(body)
    };
    let body = match tokio::time::timeout(HF_API_TIMEOUT, work).await {
        Ok(Ok(body)) => body,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err(format!(
                "HuggingFace did not respond within {}s — check your connection and try again",
                HF_API_TIMEOUT.as_secs()
            ));
        }
    };
    parse_hf_siblings(&body, trimmed)
}

/// Hard timeout for the HuggingFace API round-trip. A stalled connection
/// (connect, headers, or body) must not block the UI button forever. The
/// entire network round-trip shares this single budget so the worst-case
/// wall-clock wait is `HF_API_TIMEOUT` seconds, not 2×.
const HF_API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Parse the HuggingFace `GET /api/models/<repo_id>` response into a
/// sorted list of `HfModelFile`. Extracted from `hf_resolve_model_files`
/// so the parse + sort + URL-build logic is testable without making real
/// HTTP calls.
fn parse_hf_siblings(
    body: &serde_json::Value,
    repo_id: &str,
) -> Result<Vec<HfModelFile>, String> {
    let siblings = body
        .get("siblings")
        .and_then(|s| s.as_array())
        .ok_or_else(|| {
            "HuggingFace API response missing `siblings` array — repo may not exist".to_string()
        })?;

    let mut files: Vec<HfModelFile> = siblings
        .iter()
        .filter_map(|s| {
            let rfilename = s.get("rfilename")?.as_str()?.to_string();
            let lower = rfilename.to_lowercase();
            let ext_ok = HF_MODEL_EXTENSIONS
                .iter()
                .any(|ext| lower.ends_with(&format!(".{}", ext)));
            if !ext_ok {
                return None;
            }
            let url = format!(
                "https://huggingface.co/{}/resolve/main/{}",
                repo_id, rfilename
            );
            // The `size` field on a sibling is in bytes when present.
            // Older API responses omit it; serde keeps it as None in that
            // case (omitted from the serialized output).
            let size_bytes = s
                .get("size")
                .and_then(|v| v.as_u64())
                .filter(|n| *n > 0);
            Some(HfModelFile {
                filename: rfilename,
                url,
                size_bytes,
            })
        })
        .collect();

    // Quantized-first, then shorter filename, then alphabetical. This
    // means `files[0]` is the smallest quantized asset most repos host,
    // which is exactly what an on-device inference caller wants.
    files.sort_by(|a, b| {
        hf_quant_score(&b.filename)
            .cmp(&hf_quant_score(&a.filename))
            .then(a.filename.len().cmp(&b.filename.len()))
            .then(a.filename.cmp(&b.filename))
    });

    Ok(files)
}

/// Score a HuggingFace filename by quantization preference. Higher = more
/// preferred (q4/int4 over fp16 over fp32, etc.). The score is the sum of
/// the matched bucket; tokens are matched against the lowercased name.
fn hf_quant_score(filename: &str) -> i32 {
    let lower = filename.to_lowercase();
    let mut score = 0;
    // Most-preferred: aggressive 4-bit quantizations.
    if lower.contains("q4_") || lower.contains("q4-") || lower.contains("int4") {
        score += 100;
    }
    if lower.contains("q5_") || lower.contains("q5-") {
        score += 70;
    }
    if lower.contains("q6_") || lower.contains("q6-") {
        score += 60;
    }
    if lower.contains("q8_") || lower.contains("q8-") || lower.contains("int8") {
        score += 50;
    }
    if lower.contains("q3_") || lower.contains("q3-") {
        score += 45;
    }
    if lower.contains("q2_") || lower.contains("q2-") {
        score += 40;
    }
    // Half precision is preferred over full precision.
    if lower.contains("fp16") || lower.contains("f16") || lower.contains("bf16") {
        score += 30;
    }
    if lower.contains("fp32") || lower.contains("f32") {
        score += 10;
    }
    score
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
    for (pid, mut tracked) in guard.drain() {
        let _ = tracked.child.kill();
        let _ = tracked.child.wait();
        log::info!("Reaped backend pid={}", pid);
    }
    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

/// Map a detected GPU vendor to the download-table vendor key for a
/// particular backend kind. Different backends ask for different flavors
/// (e.g. llama.cpp has `nvidia` and `amd`; turboquant has `nvidia`,
/// `nvidia-cuda12`, `cpu`, and `amd` (vulkan)).
fn vendor_for_kind(kind: &BackendKind, detected: &str) -> String {
    match (kind, detected) {
        // Turboquant CUDA 13.3 is the default NVIDIA path; fall back to
        // CUDA 12.4 only when explicitly hinted (e.g. user clicked the
        // `nvidia-cuda12` variant in the UI catalog).
        (BackendKind::TurboQuant, "nvidia") => "nvidia".to_string(),
        (BackendKind::TurboQuant, "amd") => "amd".to_string(),
        (BackendKind::TurboQuant, "cpu") => "cpu".to_string(),
        // Lemonade is a single-binary embeddable runtime that auto-detects
        // the best hardware path on startup (NVIDIA / AMD / Intel NPU / CPU).
        // The DOWNLOAD_TABLE has exactly one Lemonade row marked
        // `vendor = "all"` so any detected vendor falls through to it.
        (BackendKind::Lemonade, _) => "all".to_string(),
        _ => detected.to_string(),
    }
}

fn parse_backend_kind(s: &str) -> Result<BackendKind, String> {
    match s.trim().to_lowercase().as_str() {
        "llama.cpp" | "llama_cpp" | "llamacpp" | "llama-cpp" => Ok(BackendKind::LlamaCpp),
        "llamafile" => Ok(BackendKind::Llamafile),
        "kobold.cpp" | "kobold_cpp" | "koboldcpp" | "kobold-cpp" => Ok(BackendKind::KoboldCpp),
        "turboquant" | "turbo_quant" | "turbo-quant" => Ok(BackendKind::TurboQuant),
        "lemonade" => Ok(BackendKind::Lemonade),            other => Err(format!(
                "Unknown backend kind: '{}'. Expected llama.cpp | llamafile | koboldcpp | turboquant | lemonade",
                other
            )),
    }
}

fn kind_dir_name(kind: &BackendKind) -> &'static str {
    match kind {
        BackendKind::LlamaCpp => "llama.cpp",
        BackendKind::Llamafile => "llamafile",
        BackendKind::KoboldCpp => "koboldcpp",
        BackendKind::TurboQuant => "turboquant",
        BackendKind::Lemonade => "lemonade",
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
    // Surface the default port even when not running so the UI shows the
    // URL the backend would listen on once started.
    let default_port = kind.default_port();
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
            port: Some(default_port),
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
            port: Some(default_port),
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

/// Extracts a zip / writes a binary blob to `install_root`.
///
/// Two zip flavours are supported:
/// - `"zip"`:    llama.cpp / llama-bin-win-* archives that ship a single
///               top-level parent directory (`llama-bin-win-cuda-x64/<files>`).
///               We strip the first path segment so the binary lands at the
///               install root rather than nested under the archive's own
///               folder.
/// - `"zip-flat"`: turboquant /AtomicBot-AI archives that ship files at the
///               zip root with no parent directory. Stripping the first
///               segment would discard a real filename, so we extract as-is.
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
        "zip" | "zip-flat" => {
            // Both flavours iterate the archive identically; only the
            // path-mapping differs.
            let strip = matches!(archive_format, "zip");
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
                let mapped = if strip {
                    let s = strip_top_dir(&entry_path);
                    if s.as_os_str().is_empty() {
                        continue;
                    }
                    s
                } else {
                    entry_path
                };
                let target = install_root.join(&mapped);
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
        assert_eq!(parse_backend_kind("turboquant").unwrap(), BackendKind::TurboQuant);
        assert_eq!(parse_backend_kind("lemonade").unwrap(), BackendKind::Lemonade);
    }

    #[test]
    fn parses_backend_kind_variants_loose() {
        assert_eq!(parse_backend_kind("llama_cpp").unwrap(), BackendKind::LlamaCpp);
        assert_eq!(parse_backend_kind("llama-cpp").unwrap(), BackendKind::LlamaCpp);
        assert_eq!(parse_backend_kind("kobold_cpp").unwrap(), BackendKind::KoboldCpp);
        assert_eq!(parse_backend_kind("KOBOLDCPP").unwrap(), BackendKind::KoboldCpp);
        assert_eq!(parse_backend_kind("turbo_quant").unwrap(), BackendKind::TurboQuant);
        assert_eq!(parse_backend_kind("TURBOQUANT").unwrap(), BackendKind::TurboQuant);
        assert_eq!(parse_backend_kind("LEMONADE").unwrap(), BackendKind::Lemonade);
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
    fn lookup_download_finds_turboquant_for_each_vendor() {
        let cuda13 = lookup_download(&BackendKind::TurboQuant, "nvidia").unwrap();
        assert!(cuda13.url.contains("cuda-13.3"));
        assert_eq!(cuda13.archive_format, "zip-flat");

        let cuda12 = lookup_download(&BackendKind::TurboQuant, "nvidia-cuda12").unwrap();
        assert!(cuda12.url.contains("cuda-12.4"));
        assert_eq!(cuda12.archive_format, "zip-flat");

        let cpu = lookup_download(&BackendKind::TurboQuant, "cpu").unwrap();
        assert!(cpu.url.contains("-cpu"));
        assert_eq!(cpu.archive_format, "zip-flat");

        let amd = lookup_download(&BackendKind::TurboQuant, "amd").unwrap();
        assert!(amd.url.contains("-vulkan"));
        assert_eq!(amd.archive_format, "zip-flat");
    }

    #[test]
    fn turboquant_url_is_pinned_not_latest() {
        // AtomicBot-AI publishes a different tag per variant; using
        // `/releases/latest/...` would only ever resolve to one of them.
        let entry = lookup_download(&BackendKind::TurboQuant, "nvidia").unwrap();
        assert!(
            !entry.url.contains("/releases/latest/"),
            "turboquant URL must pin a specific tag (got {})",
            entry.url
        );
        assert!(entry.url.contains("-d86eb0b"));
    }

    #[test]
    fn kind_dir_names_match_installer_layout() {
        assert_eq!(kind_dir_name(&BackendKind::LlamaCpp), "llama.cpp");
        assert_eq!(kind_dir_name(&BackendKind::Llamafile), "llamafile");
        assert_eq!(kind_dir_name(&BackendKind::KoboldCpp), "koboldcpp");
        assert_eq!(kind_dir_name(&BackendKind::TurboQuant), "turboquant");
        assert_eq!(kind_dir_name(&BackendKind::Lemonade), "lemonade");
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
    fn backend_kind_all_returns_five_kinds() {
        let kinds = BackendKind::all();
        assert_eq!(kinds.len(), 5);
        assert_eq!(kinds[0], BackendKind::LlamaCpp);
        assert_eq!(kinds[1], BackendKind::Llamafile);
        assert_eq!(kinds[2], BackendKind::KoboldCpp);
        assert_eq!(kinds[3], BackendKind::TurboQuant);
        assert_eq!(kinds[4], BackendKind::Lemonade);
    }

    #[test]
    fn backend_kind_serializes_to_canonical_strings() {
        let llama_cpp = serde_json::to_string(&BackendKind::LlamaCpp).unwrap();
        let llamafile = serde_json::to_string(&BackendKind::Llamafile).unwrap();
        let koboldcpp = serde_json::to_string(&BackendKind::KoboldCpp).unwrap();
        let turboquant = serde_json::to_string(&BackendKind::TurboQuant).unwrap();
        let lemonade = serde_json::to_string(&BackendKind::Lemonade).unwrap();
        assert_eq!(llama_cpp, "\"llama.cpp\"");
        assert_eq!(llamafile, "\"llamafile\"");
        assert_eq!(koboldcpp, "\"koboldcpp\"");
        assert_eq!(turboquant, "\"turboquant\"");
        assert_eq!(lemonade, "\"lemonade\"");
    }

    #[test]
    fn lemonade_default_port_is_13305() {
        // OpenAI-compatible API listens at 13305 upstream
        // (https://github.com/lemonade-sdk/lemonade). The frontend's
        // DEFAULT_PORTS in backend-manager.vue mirrors this constant — keep
        // both in lockstep.
        assert_eq!(BackendKind::Lemonade.default_port(), 13305);
    }

    #[test]
    fn lookup_download_finds_lemonade_for_any_vendor() {
        // Lemonade is a single-binary embeddable runtime that auto-detects
        // hardware on startup — therefore one DOWNLOAD_TABLE row covers all
        // vendors and `vendor_for_kind` maps every detected vendor to "all".
        for vendor in ["nvidia", "amd", "cpu"] {
            let entry = lookup_download(&BackendKind::Lemonade, vendor)
                .expect("Lemonade lookup should always resolve");
            assert_eq!(entry.vendor, "all");
            assert!(entry.url.contains("lemonade-embeddable-10.8.1-windows-x64.zip"));
            assert_eq!(entry.binary_windows, "lemonade-server.exe");
            assert_eq!(entry.archive_format, "zip");
        }
    }

    // ----- Fix 1: list_gguf_models scanner tests -----

    #[test]
    fn list_gguf_models_rejects_nonexistent_path() {
        let result = list_gguf_models(
            "Z:\\definitely\\does\\not\\exist\\anywhere".to_string(),
            Some(2),
        );
        assert!(result.is_err(), "missing path should error");
    }

    #[test]
    fn list_gguf_models_rejects_non_directory() {
        // Cargo.toml is a regular file in the package root; passing it to
        // the scanner should be rejected before any walk happens.
        let result = list_gguf_models("Cargo.toml".to_string(), Some(2));
        assert!(result.is_err(), "non-directory path should error");
    }

    #[test]
    fn list_gguf_models_returns_empty_for_empty_directory() {
        let dir = std::env::temp_dir().join("meridian-test-empty-gguf");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("setup: create temp dir");
        let result = list_gguf_models(dir.to_string_lossy().to_string(), Some(2))
            .expect("empty directory should scan cleanly");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            result.is_empty(),
            "empty directory must yield no entries, got {:?}",
            result
        );
    }

    #[test]
    fn list_gguf_models_finds_files_in_subdirectories() {
        // Mirror JC's typical layout: `models/<vendor>/<size>/<file>.gguf`.
        let root = std::env::temp_dir().join("meridian-test-nested-gguf");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("setup: create root");
        let nested = root.join("Qwen").join("7B");
        std::fs::create_dir_all(&nested).expect("setup: create nested");
        std::fs::write(nested.join("qwen2.5-7b-instruct-q4_k_m.gguf"), b"x")
            .expect("setup: write gguf placeholder");
        // A non-matching sibling must be ignored.
        std::fs::write(root.join("readme.txt"), b"x").expect("setup: write sibling");
        let result =
            list_gguf_models(root.to_string_lossy().to_string(), Some(6))
                .expect("scan should succeed");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(result.len(), 1, "expected one match, got {:?}", result);
        assert!(
            result[0].path.ends_with("qwen2.5-7b-instruct-q4_k_m.gguf"),
            "nested file must be found"
        );
        assert!(result[0].size_bytes > 0);
    }

    // ----- Fix #2: hf_resolve_model_files / quant scoring tests -----

    #[test]
    fn hf_quant_score_prefers_q4_over_fp16_over_fp32() {
        // Higher score = more preferred. q4 tokens outrank fp16 which
        // outranks fp32 which outranks anything with no quant token.
        let q4 = hf_quant_score("model-q4_k_m.gguf");
        let q5 = hf_quant_score("model-q5_k_m.gguf");
        let fp16 = hf_quant_score("model-fp16.gguf");
        let fp32 = hf_quant_score("model-fp32.safetensors");
        let plain = hf_quant_score("model.bin");
        assert!(q4 > q5, "q4 should outrank q5: q4={} q5={}", q4, q5);
        assert!(q5 > fp16, "q5 should outrank fp16: q5={} fp16={}", q5, fp16);
        assert!(fp16 > fp32, "fp16 should outrank fp32: fp16={} fp32={}", fp16, fp32);
        assert!(fp32 > plain, "fp32 should outrank plain: fp32={} plain={}", fp32, plain);
    }

    #[test]
    fn hf_quant_score_is_case_insensitive() {
        // Filenames vary in case across repos; the score must not.
        let upper = hf_quant_score("MODEL-Q4_K_M.GGUF");
        let lower = hf_quant_score("model-q4_k_m.gguf");
        assert_eq!(upper, lower);
    }

    #[test]
    fn hf_quant_score_matches_int4_and_int8() {
        // ONNX repos commonly use int4/int8 naming instead of q4/q8.
        let int4 = hf_quant_score("model_int4.onnx");
        let int8 = hf_quant_score("model_int8.onnx");
        let fp16 = hf_quant_score("model_fp16.onnx");
        assert!(int4 > int8, "int4 should outrank int8");
        assert!(int8 > fp16, "int8 should outrank fp16");
    }

    // ----- parse_hf_siblings — synthetic JSON tests (no network) -----

    /// Helper: build a minimal HF-API-shaped JSON body with a siblings list.
    fn hf_body(siblings: &[&str]) -> serde_json::Value {
        let entries: Vec<serde_json::Value> = siblings
            .iter()
            .map(|name| serde_json::json!({ "rfilename": name }))
            .collect();
        serde_json::json!({ "siblings": entries })
    }

    #[test]
    fn parse_hf_siblings_filters_to_model_extensions() {
        // README, config, tokenizer must all be dropped.
        let body = hf_body(&[
            "README.md",
            "config.json",
            "tokenizer.json",
            "tokenizer_config.json",
            "special_tokens_map.json",
            "model.safetensors",
            "generation_config.json",
        ]);
        let files = parse_hf_siblings(&body, "user/repo").expect("parse ok");
        assert_eq!(files.len(), 1, "expected one model file, got {:?}", files);
        assert_eq!(files[0].filename, "model.safetensors");
        assert_eq!(
            files[0].url,
            "https://huggingface.co/user/repo/resolve/main/model.safetensors"
        );
    }

    #[test]
    fn parse_hf_siblings_sorts_quantized_first() {
        let body = hf_body(&[
            "model.fp32.gguf",
            "model.fp16.gguf",
            "model-q8_0.gguf",
            "model-q4_k_m.gguf",
            "model-q5_k_m.gguf",
        ]);
        let files = parse_hf_siblings(&body, "user/repo").expect("parse ok");
        assert_eq!(files.len(), 5);
        assert_eq!(files[0].filename, "model-q4_k_m.gguf", "q4 first");
        assert_eq!(files[1].filename, "model-q5_k_m.gguf", "q5 second");
        assert_eq!(files[2].filename, "model-q8_0.gguf", "q8 third");
        assert_eq!(files[3].filename, "model.fp16.gguf", "fp16 fourth");
        assert_eq!(files[4].filename, "model.fp32.gguf", "fp32 last");
    }

    #[test]
    fn parse_hf_siblings_accepts_all_supported_extensions() {
        let body = hf_body(&[
            "weights.onnx",
            "weights.gguf",
            "weights.bin",
            "weights.safetensors",
            "weights.pt",
        ]);
        let files = parse_hf_siblings(&body, "user/repo").expect("parse ok");
        assert_eq!(files.len(), 5, "all five extensions should pass the filter");
        for f in &files {
            assert!(f.url.starts_with("https://huggingface.co/user/repo/resolve/main/"));
        }
    }

    #[test]
    fn parse_hf_siblings_carries_size_bytes_when_present() {
        // HF API includes `size` in bytes on modern responses. We surface it
        // so the UI can show "X GB" without a HEAD request.
        let body = serde_json::json!({
            "siblings": [
                { "rfilename": "model-q4_k_m.gguf", "size": 4_398_046_511_u64 },
                { "rfilename": "config.json" },
            ]
        });
        let files = parse_hf_siblings(&body, "user/repo").expect("parse ok");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].size_bytes, Some(4_398_046_511));
    }

    #[test]
    fn parse_hf_siblings_rejects_response_without_siblings() {
        let body = serde_json::json!({ "id": "user/repo", "tags": ["text-generation"] });
        let result = parse_hf_siblings(&body, "user/repo");
        assert!(result.is_err(), "missing siblings should error");
    }

    /// Empty repo IDs must error out before any HTTP work happens. The
    /// `is_empty()` short-circuit is the first line of the function, so
    /// this `#[tokio::test]` is deterministic even though the function is
    /// async — no network call ever fires. Real assertion: actually invoke
    /// the command, not the precondition of the input string.
    #[tokio::test]
    async fn hf_resolve_model_files_rejects_empty_repo() {
        let result = hf_resolve_model_files("".to_string()).await;
        assert!(result.is_err(), "empty repo id should error");
    }
}
