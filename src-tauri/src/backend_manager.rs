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

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use walkdir::WalkDir;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

/// Legacy fallback install root used when neither `dirs::data_local()`
/// nor a user override resolve. Preserved so installs that wrote
/// binaries to this path before Fix C keep finding them.
const FALLBACK_BACKEND_ROOT: &str = "E:\\ai\\Apps\\backends";

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
    /// Static URL for downloads where the asset path is stable across
    /// releases (TurboQuant pinned to d86eb0b, Lemonade pinned to v10.8.1).
    /// When set, this URL is used directly and `gh_repo` is ignored.
    /// None means the URL is resolved at download time via GitHub API —
    /// see `gh_repo` / `gh_repo_match` / `gh_repo_match_alt`.
    url: Option<&'static str>,
    /// GitHub repo slug for runtime asset discovery ("ggml-org/llama.cpp").
    /// When `Some`, the actual asset URL is resolved at download time via
    /// `GET https://api.github.com/repos/<gh_repo>/releases/latest` and
    /// asset-name substring matching. Eliminates the version-drift bug
    /// where pinning `/releases/b<NNN>/...` becomes orphaned when
    /// upstream bumps version. None means `url` is used directly.
    gh_repo: Option<&'static str>,
    /// Substring matched (case-insensitive) against asset names in the
    /// GitHub release `assets[]` array. First matching asset's
    /// `browser_download_url` is used as the download URL.
    gh_repo_match: Option<&'static str>,
    /// Fallback substring if `gh_repo_match` doesn't appear in any
    /// asset name (e.g. upstream changes its CUDA suffix convention).
    gh_repo_match_alt: Option<&'static str>,
    /// Required prefix on the matched asset name (case-insensitive).
    /// When the GitHub release ships MULTIPLE assets that all match
    /// `gh_repo_match` substring (e.g. llama.cpp publishes both
    /// `cudart-llama-bin-win-cuda-12.4-x64.zip` AND `llama-b9842-bin-win-cuda-12.4-x64.zip`
    /// — cudart is the CUDA runtime bundle, NOT the llama-server),
    /// the asset-preference filter rejects any name that does NOT start
    /// with this prefix. Set to `Some("llama-")` for llama.cpp variants;
    /// None falls back to "first substring match wins" which is wrong
    /// the moment upstream publishes multiple sibling assets.
    gh_repo_pref_prefix: Option<&'static str>,
    binary_windows: &'static str,
    binary_linux: &'static str,
    /// "zip" | "zip-flat" | "tar.gz" | "binary"
    archive_format: &'static str,
}

/// Static catalog for Step 1. Step 2 will read this from a bundled JSON file
/// (`resources/backend_catalog.json`) loaded by Tauri at startup.
///
/// Resolves the chronic 404 problem on llama.cpp / llamafile / koboldcpp by
/// using the GitHub Releases API for assets whose filenames include the
/// release tag (e.g. `llama-b9842-bin-win-cuda-12.4-x64.zip`). Static URLs
/// would 404 the moment upstream bumps the version — the API lookup is
/// version-drift-proof. TurboQuant (each variant has its own tag in
/// AtomicBot-AI's repo, so `/releases/latest/` only ever resolves one of
/// them) and Lemonade (pinned to v10.8.1) stay on static URLs by design.
const DOWNLOAD_TABLE: &[DownloadEntry] = &[
    // === llama.cpp — asset name includes the version tag (e.g. b9842), so
    // === we resolve via GitHub API at download time to track upstream.
    // CPU build — small zip, plain CPU runtime, no GPU required.
    DownloadEntry {
        kind: BackendKind::LlamaCpp,
        id: "llama.cpp.cpu",
        vendor: "cpu",
        url: None,
        gh_repo: Some("ggml-org/llama.cpp"),
        gh_repo_match: Some("bin-win-cpu-x64"),
        gh_repo_match_alt: Some("bin-win-cpu"),
        gh_repo_pref_prefix: Some("llama-"),
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip",
    },
    // NVIDIA build — CUDA-12.4 preferred over CUDA-13.3 because broad driver
    // compatibility wins over bleeding-edge features for most installs.
    // Falls back to the older `bin-win-cuda-x64` suffix convention if needed.
    DownloadEntry {
        kind: BackendKind::LlamaCpp,
        id: "llama.cpp.nvidia",
        vendor: "nvidia",
        url: None,
        gh_repo: Some("ggml-org/llama.cpp"),
        gh_repo_match: Some("bin-win-cuda-12.4-x64"),
        gh_repo_match_alt: Some("bin-win-cuda-x64"),
        gh_repo_pref_prefix: Some("llama-"),
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip",
    },
    // AMD build — Windows uses HIP/Radeon naming, NOT ROCm (no separate
    // ROCm-for-Windows archive ships upstream). Fallback matches the older
    // `bin-win-rocm-x64` suffix used before the HIP rename if a
    // pre-rename release is going through the API at request time.
    DownloadEntry {
        kind: BackendKind::LlamaCpp,
        id: "llama.cpp.amd",
        vendor: "amd",
        url: None,
        gh_repo: Some("ggml-org/llama.cpp"),
        gh_repo_match: Some("bin-win-hip-radeon-x64"),
        gh_repo_match_alt: Some("bin-win-rocm-x64"),
        gh_repo_pref_prefix: Some("llama-"),
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip",
    },
    // === llamafile — versioned binary, we want the bare `llamafile-<ver>`
    // === (no .zip wrapper) so the 'binary' archive_format can rename it to
    // === `llamafile.exe` on install. Mozilla-Ocho redirects to mozilla-ai;
    // === use the live org slug.
    DownloadEntry {
        kind: BackendKind::Llamafile,
        id: "llamafile.universal",
        vendor: "all",
        url: None,
        gh_repo: Some("mozilla-ai/llamafile"),
        gh_repo_match: Some("llamafile-"),
        // Bare `llamafile` (no -<ver> suffix) on older releases — fallback
        // path before llamafile adopted versioning around 0.9.x.
        gh_repo_match_alt: Some(".zip"),
        // Required prefix on the matched asset name. With the bare
        // `llamafile-` matcher we could otherwise pick `.sha256`
        // checksum files first if upstream starts shipping those;
        // require the name to actually start with `llamafile`.
        gh_repo_pref_prefix: Some("llamafile"),
        binary_windows: "llamafile.exe",
        binary_linux: "llamafile",
        archive_format: "binary",
    },
    // === koboldcpp — single-exe releases, no zip wrapper since v1.85 or so.
    // === CPU fallback (`nocuda`) also supports Vulkan for AMD GPUs per
    // === upstream README; `koboldcpp` (with CUDA) is the NVIDIA path.
    DownloadEntry {
        kind: BackendKind::KoboldCpp,
        id: "koboldcpp.cpu",
        vendor: "cpu",
        url: None,
        gh_repo: Some("LostRuins/koboldcpp"),
        gh_repo_match: Some("koboldcpp-nocuda"),
        // Older releases shipped `koboldcpp-nocuda-X.Y.Z.exe` versioned.
        gh_repo_match_alt: Some("nocuda"),
        // Required prefix: `koboldcpp-nocuda.exe` AND
        // `koboldcpp-nocuda.exe.sha256` would both match the substring
        // above; the prefix picker rejects the .sha256 sibling.
        gh_repo_pref_prefix: Some("koboldcpp"),
        binary_windows: "koboldcpp-nocuda.exe",
        // Linux release ships as `koboldcpp-linux-x64-nocuda` per the
        // upstream README — matches the resolve-path naming convention.
        binary_linux: "koboldcpp-linux-x64-nocuda",
        archive_format: "binary",
    },
    DownloadEntry {
        kind: BackendKind::KoboldCpp,
        id: "koboldcpp.nvidia",
        vendor: "nvidia",
        url: None,
        gh_repo: Some("LostRuins/koboldcpp"),
        gh_repo_match: Some("koboldcpp.exe"),
        // Stable backup if upstream renames the bulk-CUDA binary; older
        // names include `koboldcpp_cuda.exe` and `koboldcpp-rocm.exe`.
        gh_repo_match_alt: Some("koboldcpp_cuda"),
        // See the sibling note above — required to disambiguate from
        // `.sha256` checksum files upstream may ship in the future.
        gh_repo_pref_prefix: Some("koboldcpp"),
        binary_windows: "koboldcpp.exe",
        binary_linux: "koboldcpp-linux-x64",
        archive_format: "binary",
    },
    // === TurboQuant — AtomicBot-AI publishes a separate tag per variant
    // === (`turboquant-windows-x64-{cuda-13.3,cuda-12.4,cpu,vulkan}-<sha>`),
    // === so /releases/latest/ resolves to ONE variant and the rest 404.
    // === Pin each variant to its specific tag with a static URL.
    // turboquant CUDA 13.3 — primary NVIDIA path.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.cuda-13.3",
        vendor: "nvidia",
        url: Some("https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-cuda-13.3-d86eb0b/llama-turboquant-windows-x64-cuda-13.3.zip"),
        gh_repo: None,
        gh_repo_match: None,
        gh_repo_match_alt: None,
        gh_repo_pref_prefix: None,
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip-flat",
    },
    // turboquant CUDA 12.4 — older NVIDIA driver compatibility.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.cuda-12.4",
        vendor: "nvidia-cuda12",
        url: Some("https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-cuda-12.4-d86eb0b/llama-turboquant-windows-x64-cuda-12.4.zip"),
        gh_repo: None,
        gh_repo_match: None,
        gh_repo_match_alt: None,
        gh_repo_pref_prefix: None,
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip-flat",
    },
    // turboquant CPU — no GPU required.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.cpu",
        vendor: "cpu",
        url: Some("https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-cpu-d86eb0b/llama-turboquant-windows-x64-cpu.zip"),
        gh_repo: None,
        gh_repo_match: None,
        gh_repo_match_alt: None,
        gh_repo_pref_prefix: None,
        binary_windows: "llama-server.exe",
        binary_linux: "llama-server",
        archive_format: "zip-flat",
    },
    // turboquant Vulkan — AMD / fallback path.
    DownloadEntry {
        kind: BackendKind::TurboQuant,
        id: "turboquant.vulkan",
        vendor: "amd",
        url: Some("https://github.com/AtomicBot-ai/atomic-llama-cpp-turboquant/releases/download/turboquant-windows-x64-vulkan-d86eb0b/llama-turboquant-windows-x64-vulkan.zip"),
        gh_repo: None,
        gh_repo_match: None,
        gh_repo_match_alt: None,
        gh_repo_pref_prefix: None,
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
        url: Some("https://github.com/lemonade-sdk/lemonade/releases/download/v10.8.1/lemonade-embeddable-10.8.1-windows-x64.zip"),
        gh_repo: None,
        gh_repo_match: None,
        gh_repo_match_alt: None,
        gh_repo_pref_prefix: None,
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
// Progress event payload
// ============================================================================

/// Emitted during `download_backend` so the frontend can show a live
/// progress bar. Mirrors the `binary-download-progress` pattern from
/// `app_updater.rs`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendDownloadProgress {
    pub kind: String,
    pub downloaded: u64,
    pub total: u64,
    pub percent: f64,
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
///
/// `github_token` is OPTIONAL. The GitHub Releases resolver attempts an
/// anonymous request first; on 403 (typically rate-limit), it retries
/// once with `Authorization: Bearer <github_token>`. A token is NOT
/// required for normal operation — it lifts only the anonymous
/// 60-req/hr ceiling.
#[tauri::command]
pub async fn download_backend(
    app: tauri::AppHandle,
    backend_kind: String,
    variant_id: Option<String>,
    target_dir: Option<String>,
    github_token: Option<String>,
) -> Result<String, String> {
    let kind = parse_backend_kind(&backend_kind)?;
    let github_token = github_token
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

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
        .map(|p| p.to_string())
        .unwrap_or_else(|| crate::install_paths::resolve_backend_root(None).to_string_lossy().into_owned());
    let install_root = PathBuf::from(&root).join(kind_dir_name(&kind));
    std::fs::create_dir_all(&install_root).map_err(|e| {
        format!("Failed to create install dir {}: {}", install_root.display(), e)
    })?;

    // Resolve the actual download URL. Pinning `/releases/b<NNN>/...` to
    // today's release tag works at install time but becomes orphaned the
    // moment upstream bumps the version (we hit this bug with llama.cpp
    // b9835 -> b9842 and `Mozilla-Ocho/llamafile` -> `mozilla-ai/llamafile`).
    // When `gh_repo` is set, query the GitHub Releases API and locate the
    // asset whose name matches `gh_repo_match` — version-drift-proof.
    let resolved_url = match entry.url {
        Some(url) => url.to_string(),
        None => {
            let repo = entry.gh_repo.unwrap_or("");
            let primary = entry.gh_repo_match.unwrap_or("");
            if repo.is_empty() || primary.is_empty() {
                return Err(format!(
                    "Variant {} has neither a static URL nor a complete GitHub lookup config",
                    entry.id
                ));
            }
            log::info!(
                "Resolving variant {} via GitHub Releases API: repo={} match={}",
                entry.id, repo, primary
            );
            resolve_github_release_url(repo, primary, entry.gh_repo_match_alt, entry.gh_repo_pref_prefix, github_token.as_deref()).await?
        }
    };

    // Use a proper client with a 5-minute timeout instead of the
    // default reqwest::get which has no timeout and buffers the entire
    // response into memory before yielding.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let response = client
        .get(&resolved_url)
        .send()
        .await
        .map_err(|e| format!("HTTP GET failed for {}: {}", resolved_url, e))?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {} for {}",
            response.status(),
            resolved_url
        ));
    }

    let total_size = response.content_length().unwrap_or(0);
    let emit_kind = backend_kind.clone();

    // Spool to disk so peak RAM stays in the tens-of-MB range regardless of
    // download size — a multi-GB GGUF would have OOMed under the previous
    // `bytes.extend_from_slice(&chunk)` accumulator. The archive is then
    // extracted (or renamed) from this spool below.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let spool_path = install_root.join(format!(".downloading-{}-{}", std::process::id(), nanos));
    let mut file = tokio::fs::File::create(&spool_path)
        .await
        .map_err(|e| format!("Failed to create spool file {}: {}", spool_path.display(), e))?;

    let mut downloaded: u64 = 0;
    let mut last_emitted_percent: f64 = -1.0;
    let mut last_emitted_bytes: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write chunk to spool file: {}", e))?;
        downloaded += chunk.len() as u64;

        let percent: f64 = if total_size > 0 {
            (downloaded as f64 / total_size as f64) * 100.0
        } else {
            0.0
        };
        // Throttle to ~0.5% delta / ~1MB delta so a multi-GB file does NOT
        // flood the event bus (a 64KB chunk x thousands of iterations would
        // otherwise pin a CPU on the UI side parsing JSON). Both rules are
        // OR'd so a slow server (small chunks, no percent change) still emits
        // at least once per MB downloaded.
        let percent_delta = (percent - last_emitted_percent).abs();
        let bytes_delta = downloaded.saturating_sub(last_emitted_bytes);
        if percent_delta >= 0.5 || bytes_delta >= 1_048_576 {
            let _ = app.emit(
                "backend-download-progress",
                BackendDownloadProgress {
                    kind: emit_kind.clone(),
                    downloaded,
                    total: total_size,
                    percent,
                },
            );
            last_emitted_percent = percent;
            last_emitted_bytes = downloaded;
        }
    }
    // Flush so the spool file is fully readable from a fresh handle before
    // we drop it — critical before extraction opens it on Windows.
    file.flush().await.map_err(|e| format!("Failed to flush spool file: {}", e))?;

    // Force-emit a final 100% event so the UI's progress bar saturates even
    // when the last chunk didn't cross the throttle delta. When total_size
    // is unknown (no Content-Length header), progress stays indeterminate
    // (0%) but downstream UI marks the bar as "downloading" without %
    // rather than stuck.
    let _ = app.emit(
        "backend-download-progress",
        BackendDownloadProgress {
            kind: emit_kind.clone(),
            downloaded,
            total: total_size,
            percent: if total_size > 0 { 100.0 } else { 0.0 },
        },
    );

    // Drop the Tokio file handle BEFORE re-opening for extraction. Windows
    // forbids a second File on a path while another handle is open.
    drop(file);

    if let Err(e) = write_archive_from_file(&install_root, &spool_path, entry.archive_format, &kind) {
    // Cleanup the spool on failure so the install_root doesn't accumulate
    // orphaned `.downloading-<pid>-<nanos>` files across retries. Logged at
    // warn rather than swallowed so disk-fill bugs surface in DIAG logs.
    if let Err(cleanup_err) = std::fs::remove_file(&spool_path) {
        log::warn!(
            "Failed to remove spool {} after extraction error {}: {}",
            spool_path.display(),
            cleanup_err,
            e
        );
    }
    return Err(e);
    }

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

/// Recursively walks a directory tree, returning every `.gguf` file found.
///
/// Sibling to `list_gguf_models` but with NO depth cap (`usize::MAX`).
/// Users with deeply-nested layouts — e.g. the typical
/// `E:\ai\Models\bartowski\<repo>\<size>\<file>.gguf` going 4-5 levels
/// deep, or any custom organisation that nests more than 6 levels —
/// can hit "No .gguf files found" with the depth-capped walker. The
/// Models tab wires this command through so the scan can never miss a
/// file for any reasonable on-disk arrangement.
#[tauri::command]
pub fn scan_models_recursive(
    path: String,
) -> Result<Vec<GgufModelEntry>, String> {
    list_gguf_models(path, Some(usize::MAX))
}

// ============================================================================
// HuggingFace repo -> concrete file resolution
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
    // UX is "Resolving..." for HF_API_TIMEOUT seconds — short enough to fail
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
/// wall-clock wait is `HF_API_TIMEOUT` seconds, not 2x.
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
    // IQ-quants ("IQ1_XSS", "IQ2_XS", "IQ3_S", "IQ4_NL", etc.) ship
    // MUCH smaller files at the cost of significantly lower quality
    // than Q4_K_M. The HF resolution must not default to them.
    //
    // IQ1 / IQ2 / IQ3: heavy penalty so they fall BELOW plain
    // `model.bin` (score < 0). The hashrate / quality tradeoff is
    // severe enough that we want the user to explicitly opt-in.
    //
    // IQ4: lighter penalty — borderline acceptable when a repo ships
    // only IQ4 + larger quants; still below Q4_K_M but doesn't bleed
    // past fp32 in pathological sorting cases.
    if lower.contains("iq1_") || lower.contains("iq1-")
        || lower.contains("iq2_") || lower.contains("iq2-")
        || lower.contains("iq3_") || lower.contains("iq3-")
    {
        score -= 200;
    }
    else if lower.contains("iq4_") || lower.contains("iq4-") {
        score -= 50;
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
// GitHub Releases API asset resolver
// ============================================================================
//
// llama.cpp / llamafile / koboldcpp publish assets whose filenames embed
// the version tag (e.g. `llama-b9842-bin-win-cuda-12.4-x64.zip`,
// `llamafile-0.10.3`, `koboldcpp-nocuda.exe`). Pinning any URL with a
// specific tag in a `const` is a maintenance footgun: the tag floats
// forward every release cycle and a pinned URL 404s forever the moment
// upstream moves on.
//
// `resolve_github_release_url` queries `GET /repos/<repo>/releases/latest`
// and walks the `assets[]` array, picking the first asset whose
// `name` contains the requested substring (case-insensitive). A second
// optional `alt_match` lets a caller fall back to a different naming
// convention if upstream changes its asset suffix between releases
// (we hit exactly this with llama.cpp renaming ROCm -> HIP-Radeon).
//
// On success returns the asset's `browser_download_url` so a follow-up
// `reqwest::get` works without further discovery round-trips. Mirrors
// the asset-picking pattern in `app_updater.rs::pick_release_installer_asset`
// (the upstream installer updater).
async fn resolve_github_release_url(
    repo: &str,
    primary_match: &str,
    alt_match: Option<&str>,
    preferred_prefix: Option<&str>,
    github_token: Option<&str>,
) -> Result<String, String> {
    let api_url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to build HTTP client for GitHub lookup: {}", e))?;

    // First attempt: anonymous. Builds the request fresh so a second
    // attempt with auth headers can use a different `RequestBuilder`
    // without mutating the first.
    let anonymous = build_github_request(&client, &api_url, None)
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed for {}: {}", repo, e))?;

    let response = if anonymous.status().as_u16() == 403 {
        if let Some(token) = github_token {
            log::info!(
                "GitHub Releases API for {} returned HTTP 403 — retrying with bearer token",
                repo
            );
            let authed = build_github_request(&client, &api_url, Some(token))
                .send()
                .await
                .map_err(|e| format!("GitHub API retry failed for {}: {}", repo, e))?;
            if !authed.status().is_success() {
                return Err(format!(
                    "GitHub Releases API for {} returned HTTP {} even with bearer token",
                    repo,
                    authed.status()
                ));
            }
            authed
        } else {
            // No token configured — surface the actionable advice so the
            // user knows where to add one.
            return Err(format!(
                "GitHub Releases API for {} returned HTTP 403 (anonymous rate limit). \
                 Configure a GitHub Personal Access Token in Settings > Advanced > Install \
                 Paths (githubToken) for elevated rate limits and retry.",
                repo
            ));
        }
    } else if !anonymous.status().is_success() {
        return Err(format!(
            "GitHub Releases API for {} returned HTTP {}",
            repo,
            anonymous.status()
        ));
    } else {
        anonymous
    };

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub Releases JSON for {}: {}", repo, e))?;
    let assets = body
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| {
            format!(
                "GitHub Releases response for {} missing `assets` array — release may be empty",
                repo
            )
        })?;

    let primary = pick_release_asset(&assets, primary_match, preferred_prefix);
    if let Some(url) = primary {
        return Ok(url);
    }
    if let Some(alt) = alt_match {
        if let Some(url) = pick_release_asset(&assets, alt, preferred_prefix) {
            return Ok(url);
        }
    }
    Err(format!(
        "GitHub release for {} has no asset matching '{}'{} (with preferred prefix {:?})",
        repo,
        primary_match,
        alt_match
            .map(|a| format!(" or '{}'", a))
            .unwrap_or_default(),
        preferred_prefix
    ))
}

/// Build a `GET api.github.com/.../releases/latest` request with the
/// common headers. The optional `token` is sent as a Bearer token —
/// nothing else is changed about the request. Hoisted out of
/// `resolve_github_release_url` so the anonymous and auth-retry paths
/// share the same accept / user-agent surface.
fn build_github_request(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Meridian-BackendManager");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {}", t));
    }
    req
}

/// Pick the first asset whose `name` matches `needle` (case-insensitive
/// substring) AND whose lowercased name starts with `preferred_prefix` when
/// that prefix is `Some`.
///
/// Extracted from `resolve_github_release_url` so the picker is unit-testable
/// without a real HTTP round-trip — synthetic JSON asset lists can pin down
/// edge cases (the cudart-vs-llama-server disambiguation) without relying on
/// the live GitHub release keeping that exact ordering.
fn pick_release_asset(
    assets: &[serde_json::Value],
    needle: &str,
    preferred_prefix: Option<&str>,
) -> Option<String> {
    let needle_lower = needle.to_lowercase();
    let prefix_lower = preferred_prefix.map(|p| p.to_lowercase());
    assets.iter().find_map(|a| {
        let name = a.get("name").and_then(|n| n.as_str())?;
        let lower = name.to_lowercase();
        if !lower.contains(&needle_lower) {
            return None;
        }
        if let Some(ref p) = prefix_lower {
            // WITHOUT this guard, llama.cpp's GitHub release returns
            // `cudart-llama-bin-win-cuda-12.4-x64.zip` (the CUDA runtime
            // bundle) BEFORE `llama-b<ver>-bin-win-cuda-12.4-x64.zip`,
            // and `find_map` picks the cudart sibling. Extracting that
            // archive yields NO llama-server.exe — install "succeeds"
            // structurally but `start_backend` then errors with "binary
            // not found". Set `gh_repo_pref_prefix = Some("llama-")` to
            // disambiguate.
            if !lower.starts_with(p) {
                return None;
            }
        }
        let url = a.get("browser_download_url").and_then(|u| u.as_str())?;
        Some(url.to_string())
    })
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

fn platform_binary_name(kind: &BackendKind) -> String {
    // Multiple DOWNLOAD_TABLE rows can share a kind (koboldcpp.cpu +
    // koboldcpp.nvidia, llama.cpp.cpu + llama.cpp.nvidia + llama.cpp.amd).
    // Each carries a different binary_name target so the actual install
    // reveals which variant the user picked. Walk candidate binaries in
    // declaration order and return the first one that EXISTS on disk;
    // fall back to the first candidate when none are installed yet so a
    // pre-install status query (e.g. `disk_status_for_kind` from the
    // `get_backend_status` command) still surfaces a sensible default.
    let candidates: Vec<&'static str> = DOWNLOAD_TABLE
        .iter()
        .filter(|e| &e.kind == kind)
        .map(|e| if cfg!(windows) { e.binary_windows } else { e.binary_linux })
        .collect();
    if candidates.is_empty() {
        return "unknown".to_string();
    }
    if let Ok(install_root) = backend_install_root(kind, None) {
        for c in &candidates {
            if install_root.join(c).exists() {
                return (*c).to_string();
            }
        }
    }
    candidates[0].to_string()
}

fn backend_install_root(
    kind: &BackendKind,
    override_dir: Option<String>,
) -> Result<PathBuf, String> {
    let root = override_dir
        .filter(|p| !p.trim().is_empty())
        .map(|p| p.to_string())
        .unwrap_or_else(|| crate::install_paths::resolve_backend_root(None).to_string_lossy().into_owned());
    // Fall-back safety: if even crate::install_paths failed and returned
    // an empty path, use the legacy constant. Should not happen in
    // practice (resolve_backend_root always returns something).
    let root = if root.trim().is_empty() {
        FALLBACK_BACKEND_ROOT.to_string()
    } else {
        root
    };
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

/// Extracts a zip / finalises a binary download from the spool file at
/// `spool_path` into `install_root`. The spool is the file `download_backend`
/// streamed to disk during the network transfer — keeps peak RAM small.
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
///
/// `zip::ZipArchive::new` accepts any `Read + Seek`; we hand it the spool
/// `std::fs::File` directly, eliminating the previous "buffer the whole zip
/// into RAM" bottleneck.
fn write_archive_from_file(
    install_root: &Path,
    spool_path: &Path,
    archive_format: &str,
    kind: &BackendKind,
) -> Result<(), String> {
    use std::io::Read;

    match archive_format {
        "binary" => {
            // Atomic single-syscall rename — replaces any prior install of
            // this backend with the new file. No temp files left behind.
            let target_name = platform_binary_name(kind);
            let target = install_root.join(target_name);
            std::fs::rename(spool_path, &target).map_err(|e| {
                format!(
                    "Failed to rename {} to {}: {}",
                    spool_path.display(),
                    target.display(),
                    e
                )
            })?;
            Ok(())
        }
        "zip" | "zip-flat" => {
            let strip = matches!(archive_format, "zip");
            let file = std::fs::File::open(spool_path).map_err(|e| {
                format!("Failed to reopen spool {}: {}", spool_path.display(), e)
            })?;
            let mut archive = zip::ZipArchive::new(file)
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
            // Drop the archive (closes the inner File handle) BEFORE removing
            // the spool — Windows locks prevent remove_file on an open file.
            drop(archive);
            std::fs::remove_file(spool_path).map_err(|e| {
                format!("Failed to remove spool {}: {}", spool_path.display(), e)
            })?;
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
        let matcher = entry.gh_repo_match.unwrap_or("");
        let prefix = entry.gh_repo_pref_prefix.unwrap_or("");
        assert!(
            matcher.contains("cuda"),
            "expected CUDA substring in gh_repo_match, got '{}'",
            matcher
        );
        assert_eq!(
            prefix, "llama-",
            "expected 'llama-' preferred prefix to reject the cudart bundle"
        );
        assert_eq!(entry.archive_format, "zip");
    }

    #[test]
    fn lookup_download_finds_hip_or_rocm_for_amd() {
        let entry = lookup_download(&BackendKind::LlamaCpp, "amd").unwrap();
        let primary = entry.gh_repo_match.unwrap_or("");
        let alt = entry.gh_repo_match_alt.unwrap_or("");
        let combined = format!("{} {}", primary, alt);
        assert!(
            combined.contains("hip") || combined.contains("rocm"),
            "expected HIP/Radeon hint in lookup matchers, got primary='{}' alt='{}'",
            primary, alt
        );
        assert_eq!(entry.archive_format, "zip");
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
        let matcher = entry.gh_repo_match.unwrap_or("");
        assert!(
            matcher.contains("nocuda"),
            "expected 'nocuda' substring in gh_repo_match, got '{}'",
            matcher
        );
        assert_eq!(entry.archive_format, "binary");
    }

    #[test]
    fn lookup_download_finds_turboquant_for_each_vendor() {
        let cuda13 = lookup_download(&BackendKind::TurboQuant, "nvidia").unwrap();
        assert!(cuda13.url.unwrap_or_default().contains("cuda-13.3"));
        assert_eq!(cuda13.archive_format, "zip-flat");

        let cuda12 = lookup_download(&BackendKind::TurboQuant, "nvidia-cuda12").unwrap();
        assert!(cuda12.url.unwrap_or_default().contains("cuda-12.4"));
        assert_eq!(cuda12.archive_format, "zip-flat");

        let cpu = lookup_download(&BackendKind::TurboQuant, "cpu").unwrap();
        assert!(cpu.url.unwrap_or_default().contains("-cpu"));
        assert_eq!(cpu.archive_format, "zip-flat");

        let amd = lookup_download(&BackendKind::TurboQuant, "amd").unwrap();
        assert!(amd.url.unwrap_or_default().contains("-vulkan"));
        assert_eq!(amd.archive_format, "zip-flat");
    }

    #[test]
    fn turboquant_url_is_pinned_not_latest() {
        let entry = lookup_download(&BackendKind::TurboQuant, "nvidia").unwrap();
        let url = entry.url.unwrap_or_default();
        assert!(
            !url.contains("/releases/latest/"),
            "turboquant URL must pin a specific tag (got {})",
            url
        );
        assert!(url.contains("-d86eb0b"));
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
        assert_eq!(BackendKind::Lemonade.default_port(), 13305);
    }

    #[test]
    fn lookup_download_finds_lemonade_for_any_vendor() {
        for vendor in ["nvidia", "amd", "cpu"] {
            let entry = lookup_download(&BackendKind::Lemonade, vendor)
                .expect("Lemonade lookup should always resolve");
            assert_eq!(entry.vendor, "all");
            assert!(entry
                .url
                .unwrap_or_default()
                .contains("lemonade-embeddable-10.8.1-windows-x64.zip"));
            assert_eq!(entry.binary_windows, "lemonade-server.exe");
            assert_eq!(entry.archive_format, "zip");
        }
    }

    #[test]
    fn pick_release_asset_rejects_cudart_bundle_for_llama_cpp() {
        let assets = vec![
            serde_json::json!({
                "name": "cudart-llama-bin-win-cuda-12.4-x64.zip",
                "browser_download_url": "https://github.com/ggml-org/llama.cpp/releases/download/b9842/cudart-llama-bin-win-cuda-12.4-x64.zip",
            }),
            serde_json::json!({
                "name": "llama-b9842-bin-win-cuda-12.4-x64.zip",
                "browser_download_url": "https://github.com/ggml-org/llama.cpp/releases/download/b9842/llama-b9842-bin-win-cuda-12.4-x64.zip",
            }),
            serde_json::json!({
                "name": "SHA256SUMS",
                "browser_download_url": "https://github.com/ggml-org/llama.cpp/releases/download/b9842/SHA256SUMS",
            }),
        ];
        let url = pick_release_asset(
            &assets,
            "bin-win-cuda-12.4-x64",
            Some("llama-"),
        )
        .expect("llama-server build must be selected over cudart bundle");
        assert!(
            url.contains("llama-b9842-bin-win-cuda-12.4-x64.zip"),
            "selected asset must be the llama-server zip, got: {}",
            url
        );
        assert!(
            !url.contains("cudart-"),
            "cudart bundle must NEVER be selected — got {}",
            url
        );
    }

    #[test]
    fn pick_release_asset_rejects_cudart_even_when_listed_first() {
        let assets = vec![
            serde_json::json!({
                "name": "cudart-llama-bin-win-cuda-12.4-x64.zip",
                "browser_download_url": "https://cdn.example/cudart.zip",
            }),
            serde_json::json!({
                "name": "llama-b9842-bin-win-cuda-12.4-x64.zip",
                "browser_download_url": "https://cdn.example/llama-server.zip",
            }),
        ];
        assert_eq!(
            assets[0]["name"].as_str(),
            Some("cudart-llama-bin-win-cuda-12.4-x64.zip"),
            "precondition: cudart sibling must be at index 0 for this test"
        );
        let url = pick_release_asset(
            &assets,
            "bin-win-cuda-12.4-x64",
            Some("llama-"),
        )
        .expect("must skip cudart at index 0 and pick llama-server at index 1");
        assert!(url.ends_with("llama-server.zip"));
    }

    #[test]
    fn pick_release_asset_accepts_first_match_when_no_prefix_required() {
        let assets = vec![
            serde_json::json!({
                "name": "koboldcpp-nocuda.exe",
                "browser_download_url": "https://cdn.example/koboldcpp-nocuda.exe",
            }),
        ];
        let url = pick_release_asset(&assets, "koboldcpp-nocuda", None)
            .expect("no prefix constraint, first match wins");
        assert!(url.ends_with("koboldcpp-nocuda.exe"));
    }

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
        let root = std::env::temp_dir().join("meridian-test-nested-gguf");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("setup: create root");
        let nested = root.join("Qwen").join("7B");
        std::fs::create_dir_all(&nested).expect("setup: create nested");
        std::fs::write(nested.join("qwen2.5-7b-instruct-q4_k_m.gguf"), b"x")
            .expect("setup: write gguf placeholder");
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

    #[test]
    fn hf_quant_score_prefers_q4_over_fp16_over_fp32() {
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
        let upper = hf_quant_score("MODEL-Q4_K_M.GGUF");
        let lower = hf_quant_score("model-q4_k_m.gguf");
        assert_eq!(upper, lower);
    }

    #[test]
    fn hf_quant_score_matches_int4_and_int8() {
        let int4 = hf_quant_score("model_int4.onnx");
        let int8 = hf_quant_score("model_int8.onnx");
        let fp16 = hf_quant_score("model_fp16.onnx");
        assert!(int4 > int8, "int4 should outrank int8");
        assert!(int8 > fp16, "int8 should outrank fp16");
    }

    fn hf_body(siblings: &[&str]) -> serde_json::Value {
        let entries: Vec<serde_json::Value> = siblings
            .iter()
            .map(|name| serde_json::json!({ "rfilename": name }))
            .collect();
        serde_json::json!({ "siblings": entries })
    }

    #[test]
    fn parse_hf_siblings_filters_to_model_extensions() {
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

    #[tokio::test]
    async fn hf_resolve_model_files_rejects_empty_repo() {
        let result = hf_resolve_model_files("".to_string()).await;
        assert!(result.is_err(), "empty repo id should error");
    }

    #[test]
    fn hf_quant_score_penalizes_iq_quants_below_q4_k_m() {
        let q4 = hf_quant_score("model-Q4_K_M.gguf");
        let plain = hf_quant_score("model.bin");
        assert!(q4 > 0, "precondition: Q4_K_M must score > 0 (got {})", q4);
        for name in [
            "model-IQ1_S.gguf",
            "model-IQ1_M.gguf",
            "model-IQ2_XXS.gguf",
            "model-IQ2_XSS.gguf",
            "model-IQ2_S.gguf",
            "model-IQ2_M.gguf",
            "model-IQ3_XXS.gguf",
            "model-IQ3_XS.gguf",
            "model-IQ3_S.gguf",
            "model-IQ3_M.gguf",
            "model-IQ3_NL.gguf",
        ] {
            let s = hf_quant_score(name);
            assert!(
                s < q4,
                "{} (score={}) must score below Q4_K_M (score={})",
                name, s, q4
            );
            assert!(
                s < plain,
                "{} (score={}) must score below plain (score={})",
                name, s, plain
            );
        }
    }

    #[test]
    fn hf_quant_score_penalizes_iq4_mildly_but_below_q4() {
        let iq4 = hf_quant_score("model-IQ4_XS.gguf");
        let q4 = hf_quant_score("model-Q4_K_M.gguf");
        let plain = hf_quant_score("model.bin");
        assert!(
            iq4 < q4,
            "IQ4 (score={}) must score below Q4_K_M (score={})",
            iq4, q4
        );
        assert!(
            iq4 > plain,
            "IQ4 (score={}) must score above plain (score={})",
            iq4, plain
        );
    }

    #[test]
    fn scan_models_recursive_walks_past_default_depth_cap() {
        let root = std::env::temp_dir().join("meridian-test-deep-gguf");
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for level in 0..8 {
            deep = deep.join(format!("level-{}", level));
            std::fs::create_dir_all(&deep).expect("create deep dir");
        }
        std::fs::write(deep.join("deep-q4_k_m.gguf"), b"x")
            .expect("write leaf");

        let capped = list_gguf_models(
            root.to_string_lossy().to_string(),
            Some(6),
        )
        .expect("capped scan should succeed");
        assert!(
            capped.is_empty(),
            "list_gguf_models(max_depth=6) must NOT find the leaf at depth 8, got {:?}",
            capped
        );

        let result =
            scan_models_recursive(root.to_string_lossy().to_string())
                .expect("recursive scan should succeed");
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            result.len(),
            1,
            "deep recursive scan must find the leaf, got {:?}",
            result
        );
        assert!(
            result[0].path.contains("deep-q4_k_m.gguf"),
            "found path was {:?}",
            result[0].path
        );
    }
}
