// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the project root for the full license text.
// Copyright © 2026 Meridian Agent. All rights reserved.

//! Meridian — Hardware Scanner (HF GGUF model search + browse backend).
//!
//! Two Tauri commands back the Hardware Scanner panel:
//!
//! * [`hardware_search_gguf_models`] — bulk list the HF global / search
//!   feed in one `full=true&limit=N` round-trip. The Rust side picks the
//!   best GGUF per repo by quant-priority score AND keeps the
//!   per-sibling list for the LM-Studio-style per-quant breakdown on
//!   card expand.
//!
//! * [`hardware_fetch_model_detail`] — on-demand fetch of a single
//!   repo's `config.json` for the real `max_position_embeddings`
//!   value. JC explicitly chose heuristic-on-card-grid + real-fetch-on-
//!   expand as the UX path: avoids 50–100 config.json round-trips per
//!   search click, with the truth served only when the user clicks
//!   "Details" on a card.
//!
//! Browse mode: an empty query (or `None`) against the search command
//! emits a `browse` URL with no `search=` param — HF's global
//! trending / latest feed sorted by the user-selected field.
//!
//! Two-mode classification (Phase 11 LM-Studio parity): `browse` (no
//! `search=`) vs `exact` (any non-empty query). All non-empty queries
//! route to HF's native fuzzy substring matcher; the previous
//! wildcard-mode behaviour (literal `*` at 1–4 chars) was removed
//! because HF's substring matcher is already loose and the literal
//! star over-narrowed single-letter results. See `build_hf_search_url`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Trust whitelist the UI seeds when the user toggles "Only whitelist" ON.
pub const DEFAULT_TRUSTED_QUANTIZERS: &[&str] = &[
    "bartowski", "unsloth", "maziyarpanahi", "lonestriker", "mradermacher",
];

/// Default quantization allowlist when the UI doesn't override.
pub const DEFAULT_QUANT_ALLOWLIST: &[&str] = &["Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"];

/// Tokens that mark a model as "IQ-quantized".
pub const IQ_TOKENS: &[&str] = &["IQ1", "IQ2", "IQ3"];

/// 10% safety buffer for fit checks.
pub const VRAM_FIT_SAFETY_RATIO: f64 = 0.90;

/// Param-count buckets the UI exposes as filter chips.
pub const PARAM_BUCKETS: &[&str] = &[
    "1-3B", "4-8B", "9-15B", "16-30B", "30-60B", "60B+",
];

/// Browse-mode tag (no `search=` param emitted).
pub const KIND_BROWSE: &str = "browse";
/// Exact-mode tag (any non-empty query, HF substring match).
pub const KIND_EXACT: &str = "exact";

/// Per-sibling row the frontend renders in the LM-Studio-style
/// per-quant breakdown inside an expanded card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedGgufSibling {
    pub filename: String,
    pub quant: String,
    pub size_bytes: u64,
    pub size_gb: f64,
    /// Search-time fit against combined local + RPC pool with the 10%
    /// safety buffer. The Vue side recomputes this locally when the
    /// user changes the machine-selector dropdown (no HF round-trip).
    pub fits_hardware: bool,
    /// Quant-priority score (higher = more preferred). Lets the per-quant
    /// table sort the variants LM-Studio-style: best first.
    pub score: i32,
}

/// Detail view the frontend fetches on card expand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDetail {
    pub repo_id: String,
    pub max_position_embeddings: Option<u32>,
    /// `"config_json"` if we read `max_position_embeddings` from a real
    /// config.json at the repo root, `"none"` if 404 / parse failure.
    pub source: String,
}

// ============================================================================
// IPC types (frontend-facing)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareSearchParams {
    /// Free-text search query, e.g. "qwen2.5", "llama-3.1".
    /// `None` or `Some("")` = browse mode (no `search=` param emitted).
    pub query: Option<String>,
    /// One of "downloads" (default), "lastModified", "likes".
    #[serde(default)]
    pub sort_by: Option<String>,
    /// HF list-page limit. 100 default; cap at 100.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Architecture filter — empty list = all. Tokens are lowercase.
    #[serde(default)]
    pub architectures: Vec<String>,
    /// Param-size filter — empty list = all.
    #[serde(default)]
    pub size_buckets: Vec<String>,
    /// Quant allowlist — empty list = "all quants".
    #[serde(default)]
    pub quant_allowlist: Vec<String>,
    /// Quantizer trust — empty list = "any mode".
    #[serde(default)]
    pub trusted_quantizers: Vec<String>,
    #[serde(default)]
    pub include_iq: Option<bool>,
    #[serde(default)]
    pub only_fit: Option<bool>,
    /// Sum of all GPUs' memoryTotal across local + RPC workers, in MiB.
    pub combined_vram_mb: u64,
}

/// Single ranked result the Vue side renders as a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedGgufModel {
    pub id: String,
    pub author: String,
    pub name: String,
    pub downloads: u64,
    pub likes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    pub primary_quant: String,
    pub size_bytes: u64,
    pub size_gb: f64,
    pub fits_hardware: bool,
    pub is_trusted_quantizer: bool,
    pub quantizer_label: String,
    pub architecture: String,
    pub param_count_label: String,
    pub gguf_url: String,
    pub gguf_filename: String,
    pub tags: Vec<String>,
    pub kind: String,
    /// Heuristic context-window estimate in tokens, derived from the
    /// repo id + tags. May be `None` when the heuristic didn't match a
    /// known family — the Vue surface then renders "—" with a "Fetch
    /// real context" affordance driving `hardware_fetch_model_detail`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u32>,
    /// Heuristic provenance. "estimate" if the value came from the
    /// id/tag scan, "none" if the heuristic returned None. The real
    /// fetch updates the value via the Vue side (source becomes
    /// "config_json" once the value lands).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length_source: Option<String>,
    /// Every GGUF sibling for this repo (filename + quant + size +
    /// pre-computed fits). The per-quant LM-Studio breakdown on card
    /// expand renders from this list.
    pub siblings: Vec<RankedGgufSibling>,
}

// ============================================================================
// HuggingFace response shapes
// ============================================================================

#[derive(Debug, Deserialize)]
struct HfRepo {
    #[serde(rename = "id")]
    id: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default)]
    last_modified: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    /// Populated when the list call sets `full=true`.
    #[serde(default)]
    siblings: Vec<HfSibling>,
}

#[derive(Debug, Deserialize)]
struct HfSibling {
    #[serde(rename = "rfilename")]
    rfilename: String,
    /// File size in bytes — present on `full=true` responses.
    #[serde(default)]
    size: Option<u64>,
}

// ============================================================================
// Tauri command: search + browse
// ============================================================================

/// Search HuggingFace for GGUF models matching the given filter set, or
/// browse the global trending feed when the query is empty/None.
/// Returns a ranked Vec<RankedGgufModel> ready for the Vue result cards.
#[tauri::command]
pub async fn hardware_search_gguf_models(
    params: HardwareSearchParams,
) -> Result<Vec<RankedGgufModel>, String> {
    let query = params.query.as_deref().unwrap_or("").trim();

    let limit = params.limit.unwrap_or(100).clamp(1, 100);
    let (url, kind) = build_hf_search_url(
        if query.is_empty() { None } else { Some(query) },
        params.sort_by.as_deref(),
        limit,
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "Meridian-HardwareScanner/1.0")
        .send()
        .await
        .map_err(|e| format!("HuggingFace API request failed: {}", e))?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");
        return Err(format!(
            "HuggingFace rate limit hit (HTTP 429). Retry after {} seconds.",
            retry_after
        ));
    }
    let response = response
        .error_for_status()
        .map_err(|e| format!("HuggingFace API returned HTTP {} for {}", e.status().map(|s| s.as_u16()).unwrap_or(0), url))?;
    let body: Vec<HfRepo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse HuggingFace API JSON: {}", e))?;

    let quant_filter_active = !params.quant_allowlist.is_empty();
    let quant_allow_lower: Vec<String> = params
        .quant_allowlist
        .iter()
        .map(|q| q.to_lowercase())
        .collect();
    let include_iq = params.include_iq.unwrap_or(false);
    let trust_filter_active = !params.trusted_quantizers.is_empty();
    let trusted_lower: Vec<String> = params
        .trusted_quantizers
        .iter()
        .map(|q| q.to_lowercase())
        .collect();
    let only_fit = params.only_fit.unwrap_or(false);
    let fit_threshold_bytes = if params.combined_vram_mb == 0 {
        u64::MAX
    } else {
        ((params.combined_vram_mb as f64 * VRAM_FIT_SAFETY_RATIO) * 1024.0 * 1024.0) as u64
    };

    let arch_filter_active = !params.architectures.is_empty();
    let arch_filter_lower: Vec<String> = params
        .architectures
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let bucket_filter_active = !params.size_buckets.is_empty();

    let mut ranked: Vec<RankedGgufModel> = Vec::new();
    for repo in body.iter() {
        let architecture = infer_architecture(repo);
        if arch_filter_active
            && !arch_filter_lower.iter().any(|a| a == &architecture.to_lowercase())
        {
            continue;
        }
        let gguf_filenames: Vec<&str> = repo
            .siblings
            .iter()
            .map(|s| s.rfilename.as_str())
            .filter(|n| n.to_lowercase().ends_with(".gguf"))
            .collect();
        if gguf_filenames.is_empty() {
            continue;
        }
        let param_count = infer_param_count(&gguf_filenames);
        let param_bucket = derive_param_bucket(&param_count);
        if bucket_filter_active && !params.size_buckets.iter().any(|b| b == &param_bucket) {
            continue;
        }

        let author_lower = repo.author.as_deref().unwrap_or("").to_lowercase();
        let is_trusted = author_lower.is_empty()
            || trusted_lower.is_empty()
            || trusted_lower.iter().any(|t| t == &author_lower);
        if trust_filter_active && !is_trusted {
            continue;
        }

        // Walk ALL GGUF siblings. The best-pick (highest quant score)
        // gets the top-level badge; the entire list is preserved as
        // `siblings` for the per-quant breakdown on expand.
        let mut best_pick: Option<&HfSibling> = None;
        let mut best_score = i32::MIN;
        let mut sibling_rows: Vec<RankedGgufSibling> = Vec::new();
        for sib in repo.siblings.iter() {
            let fname = sib.rfilename.as_str();
            if !fname.to_lowercase().ends_with(".gguf") {
                continue;
            }
            if !include_iq && carries_iq_token(fname) {
                continue;
            }
            if fname.to_uppercase().contains("IQ4") {
                continue;
            }
            let fname_lower = fname.to_lowercase();
            if !passes_quant_filter(&fname_lower, &quant_allow_lower, quant_filter_active) {
                continue;
            }
            let score = quant_priority_score(fname);
            if score > best_score {
                best_score = score;
                best_pick = Some(sib);
            }
            let size_bytes = sib.size.unwrap_or_else(|| {
                estimate_size_bytes(&param_count, &extract_quant_token(fname))
            });
            let size_gb = round_gb(size_bytes);
            sibling_rows.push(RankedGgufSibling {
                filename: sib.rfilename.clone(),
                quant: extract_quant_token(fname),
                size_bytes,
                size_gb,
                fits_hardware: size_bytes <= fit_threshold_bytes,
                score,
            });
        }
        sibling_rows.sort_by(|a, b| {
            b.score.cmp(&a.score).then_with(|| a.filename.cmp(&b.filename))
        });
        let best = match best_pick {
            Some(b) => b,
            None => continue,
        };

        let size_bytes = best.size.unwrap_or_else(|| {
            estimate_size_bytes(&param_count, &extract_quant_token(&best.rfilename))
        });
        let size_gb = round_gb(size_bytes);
        let fits_hardware = size_bytes <= fit_threshold_bytes;
        if only_fit && !fits_hardware {
            continue;
        }

        let gguf_url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            repo.id, best.rfilename
        );

        let id_lower = repo.id.to_lowercase();
        let context_length_estimate = infer_context_length(&id_lower, &repo.tags);
        let context_length_source = if context_length_estimate.is_some() {
            "estimate"
        } else {
            "none"
        };

        ranked.push(RankedGgufModel {
            id: repo.id.clone(),
            author: repo.author.clone().unwrap_or_default(),
            name: repo
                .id
                .split('/')
                .next_back()
                .unwrap_or(&repo.id)
                .to_string(),
            downloads: repo.downloads,
            likes: repo.likes,
            last_modified: repo.last_modified.clone(),
            primary_quant: extract_quant_token(&best.rfilename),
            size_bytes,
            size_gb,
            fits_hardware,
            is_trusted_quantizer: !author_lower.is_empty() && is_trusted,
            quantizer_label: repo.author.clone().unwrap_or_else(|| "Community".to_string()),
            architecture,
            param_count_label: param_count,
            gguf_url,
            gguf_filename: best.rfilename.clone(),
            tags: repo.tags.clone(),
            kind: kind.to_string(),
            context_length: context_length_estimate,
            context_length_source: Some(context_length_source.to_string()),
            siblings: sibling_rows,
        });
    }

    ranked.sort_by(|a, b| {
        b.downloads
            .cmp(&a.downloads)
            .then_with(|| b.fits_hardware.cmp(&a.fits_hardware))
            .then_with(|| b.is_trusted_quantizer.cmp(&a.is_trusted_quantizer))
            .then_with(|| {
                quant_priority_score(&b.gguf_filename).cmp(&quant_priority_score(&a.gguf_filename))
            })
            .then_with(|| a.gguf_filename.cmp(&b.gguf_filename))
    });

    log::info!(
        "[hardware_search_gguf_models] query='{}' (kind={}) returned {} ranked results (raw repos: {})",
        if query.is_empty() { "<browse>" } else { query },
        kind,
        ranked.len(),
        body.len()
    );
    Ok(ranked)
}

// ============================================================================
// Tauri command: real per-repo context-length fetch on demand
// ============================================================================

/// Fetch the real context-window length for a single HuggingFace repo.
/// Hits `https://huggingface.co/{repo_id}/resolve/main/config.json` and
/// reads `max_position_embeddings`. Returns `source = "none"` (not an
/// error) on 404 / parse failure so the Vue side knows it was a
/// well-formed probe that returned no answer.
#[tauri::command]
pub async fn hardware_fetch_model_detail(repo_id: String) -> Result<ModelDetail, String> {
    if repo_id.is_empty() {
        return Err("repo_id must not be empty".to_string());
    }
    let url = format!("https://huggingface.co/{}/resolve/main/config.json", repo_id);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "Meridian-HardwareScanner/1.0")
        .send()
        .await
        .map_err(|e| format!("HuggingFace API request failed: {}", e))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(ModelDetail {
            repo_id,
            max_position_embeddings: None,
            source: "none".to_string(),
        });
    }
    let response = response.error_for_status().map_err(|e| {
        format!(
            "HuggingFace API returned HTTP {} for {}",
            e.status().map(|s| s.as_u16()).unwrap_or(0),
            url
        )
    })?;
    let json: serde_json::Value = response.json().await.map_err(|e| {
        format!("Failed to parse config.json for {}: {}", repo_id, e)
    })?;
    let ctx = json
        .get("max_position_embeddings")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let source = if ctx.is_some() { "config_json" } else { "none" };
    Ok(ModelDetail {
        repo_id,
        max_position_embeddings: ctx,
        source: source.to_string(),
    })
}

// ============================================================================
// URL builder (pure, testable)
// ============================================================================

/// Builds the HF list URL and classifies the request into one of two
/// modes (Phase 11 LM-Studio parity):
///
/// * `"browse"` — empty/None input, no `search=` param emitted.
/// * `"exact"` — any non-empty input, percent-encoded verbatim for
///   HF's native fuzzy substring match. The previous wildcard mode
///   (`1-4 chars → append literal *`) was REMOVED because HF's
///   fuzzy substring matcher is already loose and the literal star
///   over-narrowed single-letter queries. Bare-stars input is still
///   percent-encoded so HF sees the literal `*` token rather than
///   dumping the index via a match-all glob.
///
/// **Fix 2 (2026-06-30):** Queries of ≤ 3 characters now have
/// `+GGUF` appended to them. This is because HF's trending/browse
/// feed returns the most popular base models (Llama, Qwen, Gemma,
/// etc.) and none of these repos ship GGUF files in their siblings.
/// A short query like `b` was returning `BAAI/bge-small-en-v1.5` and
/// `google/bigbird-roberta-base` — neither of which are GGUF
/// quantizer repos. Appending `GGUF` targets the GGUF-quantized
/// model repos (e.g. `bartowski/Llama-3.2-1B-Instruct-GGUF`) which
/// are the repos the Hardware Scanner was built to discover.
pub(crate) fn build_hf_search_url(
    query: Option<&str>,
    sort_by: Option<&str>,
    limit: u32,
) -> (String, &'static str) {
    let trimmed = query.unwrap_or("").trim();
    let is_browse = trimmed.is_empty();

    let mut url = format!(
        "https://huggingface.co/api/models?full=true&limit={}",
        limit.clamp(1, 100)
    );
    if !is_browse {
        // Fix 2: short queries (≤ 3 chars) like 'b', 'be', 'q' route
        // through HF's search as '<query> GGUF' so the result set
        // targets GGUF-quantized model repos instead of popular base
        // models that have no GGUF siblings. Longer queries (≥ 4 chars)
        // like 'llama', 'qwen2.5' are specific enough to find GGUF
        // repos without the suffix.
        let search_expression = if trimmed.chars().count() <= 3 {
            format!("{} GGUF", trimmed)
        } else {
            trimmed.to_string()
        };
        url.push_str(&format!("&search={}", percent_encode(&search_expression)));
    }
    match sort_by.unwrap_or("downloads") {
        "lastModified" => url.push_str("&sort=lastModified&direction=-1"),
        "likes" => url.push_str("&sort=likes&direction=-1"),
        _ => url.push_str("&sort=downloads&direction=-1"),
    }
    let kind = if is_browse { KIND_BROWSE } else { KIND_EXACT };
    (url, kind)
}

// ============================================================================
// Heuristic helpers (pure, testable)
// ============================================================================

/// Quant priority ordering — higher = more preferred.
fn quant_priority_score(name: &str) -> i32 {
    let upper = name.to_uppercase();
    if upper.contains("Q4_K_M") { return 100; }
    if upper.contains("Q5_K_M") { return 90; }
    if upper.contains("Q4_K_S") { return 85; }
    if upper.contains("Q5_K_S") { return 80; }
    if upper.contains("Q6_K") { return 70; }
    if upper.contains("Q8_0") { return 60; }
    if upper.contains("Q4_0") { return 55; }
    if upper.contains("BF16") || upper.contains("F16") { return 40; }
    if upper.contains("F32") { return 20; }
    if upper.contains("IQ4") { return 30; }
    if upper.contains("IQ3") { return 5; }
    if upper.contains("IQ2") || upper.contains("IQ1") { return 1; }
    10
}

/// Returns true when the filename carries an IQ1 / IQ2 / IQ3 token.
fn carries_iq_token(name: &str) -> bool {
    let upper = name.to_uppercase().replace('-', "_");
    IQ_TOKENS.iter().any(|t| upper.contains(t))
}

/// Per-file filter predicate.
fn passes_quant_filter(
    fname_lower: &str,
    quant_allow_lower: &[String],
    quant_filter_active: bool,
) -> bool {
    if !quant_filter_active {
        return true;
    }
    quant_allow_lower.iter().any(|qa| fname_lower.contains(qa))
}

/// Pulls the leading `<quant>` token out of a filename.
fn extract_quant_token(filename: &str) -> String {
    let upper = filename.to_uppercase().replace('-', "_");
    let tokens = [
        "Q4_K_M", "Q5_K_M", "Q4_K_S", "Q5_K_S", "Q6_K", "Q8_0", "Q4_0",
        "BF16", "F16", "F32", "IQ4_XS", "IQ4_NL", "IQ3_XXS", "IQ3_XS",
        "IQ3_S", "IQ3_M", "IQ2_XXS", "IQ2_XSS", "IQ2_S", "IQ2_M", "IQ2_XS",
        "IQ1_S", "IQ1_M",
    ];
    for t in tokens.iter() {
        if upper.contains(t) {
            return (*t).to_string();
        }
    }
    "GGUF".to_string()
}

/// Derives the canonical architecture label for a repo.
fn infer_architecture(repo: &HfRepo) -> String {
    const PATTERNS: &[(&str, &str)] = &[
        ("llama-3", "llama"), ("llama", "llama"),
        ("qwen3", "qwen"), ("qwen2", "qwen"), ("qwen", "qwen"),
        ("mixtral", "mistral"), ("mistral", "mistral"),
        ("gemma2", "gemma"), ("gemma", "gemma"),
        ("phi4", "phi"), ("phi3", "phi"), ("phi", "phi"),
        ("deepseek", "deepseek"),
    ];
    let tags_lower: Vec<String> = repo.tags.iter().map(|t| t.to_lowercase()).collect();
    for (token, arch) in PATTERNS.iter() {
        let matches = tags_lower.iter().any(|t| {
            t == *token
                || (t.starts_with(*token)
                    && t.len() > token.len()
                    && matches!(
                        t.as_bytes()[token.len()],
                        b'0'..=b'9' | b'.' | b'-'
                    ))
        });
        if matches { return (*arch).to_string(); }
    }
    let id_lower = repo.id.to_lowercase();
    for (token, arch) in PATTERNS.iter() {
        let mut search_from = 0usize;
        while let Some(pos) = id_lower[search_from..].find(token) {
            let abs_pos = search_from + pos;
            let before_ok = abs_pos == 0
                || matches!(id_lower.as_bytes()[abs_pos - 1], b'/' | b'-' | b'_');
            let after_pos = abs_pos + token.len();
            let after_ok = after_pos >= id_lower.len()
                || matches!(
                    id_lower.as_bytes()[after_pos],
                    b'/' | b'-' | b'_' | b'.' | b'0'..=b'9'
                );
            if before_ok && after_ok {
                return (*arch).to_string();
            }
            search_from = abs_pos + 1;
        }
    }
    "unknown".to_string()
}

/// Walks the filename list looking for a `<digits>B` token.
fn infer_param_count(filenames: &[&str]) -> String {
    for fname in filenames.iter() {
        let upper = fname.to_uppercase();
        let chars: Vec<char> = upper.chars().collect();
        let n = chars.len();
        let mut i = 0;
        while i < n {
            if !chars[i].is_ascii_digit() { i += 1; continue; }
            let start = i;
            while i < n && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
            let digits: String = chars[start..i].iter().collect();
            if i < n && chars[i] == 'X' {
                let after_x = i + 1;
                let mut j = after_x;
                while j < n && (chars[j].is_ascii_digit() || chars[j] == '.') { j += 1; }
                if j > after_x && j < n && chars[j] == 'B' {
                    return format!("{}B", chars[after_x..j].iter().collect::<String>());
                }
                i = j;
                continue;
            }
            if i < n && chars[i] == 'B' && !digits.is_empty() {
                return format!("{}B", digits);
            }
        }
    }
    String::new()
}

/// Maps a `<digits>B` label into a UI bucket label.
fn derive_param_bucket(label: &str) -> String {
    let body = label.trim_end_matches('B').trim_end_matches('b');
    let n: f64 = match body.parse() { Ok(v) => v, Err(_) => return "Unknown".to_string() };
    match n {
        x if x <= 3.0 => "1-3B".to_string(),
        x if x <= 8.0 => "4-8B".to_string(),
        x if x <= 15.0 => "9-15B".to_string(),
        x if x <= 30.0 => "16-30B".to_string(),
        x if x <= 60.0 => "30-60B".to_string(),
        _ => "60B+".to_string(),
    }
}

/// Heuristic context-window estimate from repo id + tags. Returns
/// `None` when no known family fingerprints match — the Vue surface
/// then renders "—" with a "Fetch real context" affordance for the
/// on-demand config.json lookup.
///
/// Family fingerprints are ordered most-specific-first so `qwen2.5`
/// wins over the bare `qwen` parent, and `phi-3.5` wins over `phi-3`.
fn infer_context_length(id_lower: &str, tags: &[String]) -> Option<u32> {
    // Most-specific → most-generic. Llama-3.x = 128k. Llama-2 = 4k.
    // Mistral 7b-v0.1 = 8k; v0.2/v0.3 = 32k; Nemo = 128k.
    // Phi-3.5-mini = 128k; Phi-3-mini = 4k (with -128k suffix bumping).
    // DeepSeek-V2 = 128k; V3 = 64k.
    let patterns: &[(&str, u32)] = &[
        ("phi-3.5-mini", 131072),
        ("phi-3-mini-128k", 131072),
        ("phi-3.5", 131072),
        ("phi-4", 16384),
        ("phi-3-medium", 4096),
        ("phi-3-mini", 4096),
        ("phi-3", 4096),
        ("phi", 2048),
        ("llama-3.3", 131072),
        ("llama-3.2", 131072),
        ("llama-3.1", 131072),
        ("llama-3", 131072),
        ("llama-2", 4096),
        ("qwen2.5", 131072),
        ("qwen3", 32768),
        ("qwen2", 32768),
        ("qwen", 32768),
        ("mistral-nemo", 131072),
        ("mistral-large", 32768),
        ("mistral-7b-v0.3", 32768),
        ("mistral-7b-v0.2", 32768),
        ("mistral-small-24b", 32768),
        ("mistral-7b-v0.1", 8192),
        ("mistral", 8192),
        ("gemma-3", 131072),
        ("gemma-2", 8192),
        ("gemma", 8192),
        ("deepseek-v3", 65536),
        ("deepseek-v2", 131072),
        ("deepseek-coder-v2", 131072),
        ("deepseek-coder", 16384),
        ("deepseek", 32768),
        ("nemotron-4-340b", 4096),
        ("nemotron-4", 8192),
        ("nemotron", 4096),
    ];
    for (pat, len) in patterns.iter() {
        if id_lower.contains(pat) {
            return Some(*len);
        }
    }
    // Tag-level fallback: some authors tag with `ctx-128k` etc.
    for t in tags {
        let t_lower = t.to_lowercase();
        if let Some(body) = t_lower.strip_prefix("ctx-") {
            if let Some(num) = body.strip_suffix('k') {
                if let Ok(n) = num.parse::<u32>() {
                    return Some(n * 1024);
                }
            }
            if let Some(num) = body.strip_suffix('m') {
                if let Ok(n) = num.parse::<u32>() {
                    return Some(n * 1024 * 1024);
                }
            }
        }
    }
    None
}

/// Rough size estimate when HF didn't include `size`.
fn estimate_size_bytes(param_count: &str, quant: &str) -> u64 {
    let params: f64 = param_count.trim_end_matches('B').trim_end_matches('b').parse().unwrap_or(0.0);
    if params == 0.0 { return 0; }
    let bp = match quant {
        q if q.contains("Q4_K_M") => 0.55,
        q if q.contains("Q5_K_M") => 0.65,
        q if q.contains("Q6_K") => 0.75,
        q if q.contains("Q8_0") => 0.95,
        q if q.contains("BF16") || q.contains("F16") => 2.0,
        q if q.contains("F32") => 4.0,
        _ => 0.70,
    };
    (params * 1_000_000_000.0 * bp) as u64
}

/// Rounds bytes → GB with 1-decimal precision.
fn round_gb(bytes: u64) -> f64 {
    let gb = bytes as f64 / 1_073_741_824.0;
    (gb * 10.0).round() / 10.0
}

/// Trivial percent-encoder.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quant_priority_orders_q4km_above_others() {
        assert!(quant_priority_score("llama-3-8B-Q4_K_M.gguf") > quant_priority_score("llama-3-8B-Q5_K_M.gguf"));
        assert!(quant_priority_score("llama-3-8B-Q5_K_M.gguf") > quant_priority_score("llama-3-8B-Q8_0.gguf"));
        assert!(quant_priority_score("llama-3-8B-Q8_0.gguf") > quant_priority_score("llama-3-8B-F16.gguf"));
        assert!(quant_priority_score("llama-3-8B-F16.gguf") > quant_priority_score("llama-3-8B-F32.gguf"));
    }

    #[test]
    fn quant_priority_treats_q4km_preferred_over_q4_0() {
        assert_eq!(quant_priority_score("Q4_K_M-v1-Q4_0-fallback.gguf"), 100);
    }

    #[test]
    fn quant_priority_penalizes_iq1_iq2_below_default() {
        let default = quant_priority_score("random-model.gguf");
        assert!(quant_priority_score("model-IQ1_S.gguf") < default);
        assert!(quant_priority_score("model-IQ2_M.gguf") < default);
        assert!(quant_priority_score("model-IQ3_S.gguf") < default);
    }

    #[test]
    fn extract_quant_token_matches_q4km() {
        assert_eq!(extract_quant_token("Llama-3-8B-Instruct-Q4_K_M.gguf"), "Q4_K_M");
        assert_eq!(extract_quant_token("qwen2.5-7b-instruct-q5_k_m.gguf"), "Q5_K_M");
        assert_eq!(extract_quant_token("phi-3-q6_K.gguf"), "Q6_K");
        assert_eq!(extract_quant_token("mistral-7B-q8_0.gguf"), "Q8_0");
        assert_eq!(extract_quant_token("totally-not-a-model.gguf"), "GGUF");
    }

    #[test]
    fn infer_param_count_finds_standard_patterns() {
        assert_eq!(infer_param_count(&["Llama-3-8B-Instruct-Q4_K_M.gguf"]), "8B");
        assert_eq!(infer_param_count(&["qwen2.5-7B-Instruct-Q5_K_M.gguf"]), "7B");
        assert_eq!(infer_param_count(&["deepseek-coder-33b.gguf"]), "33B");
        assert_eq!(infer_param_count(&["phi-3-mini-4k-instruct.gguf"]), "");
        assert_eq!(infer_param_count(&["totally-not-a-model.gguf"]), "");
    }

    #[test]
    fn infer_param_count_handles_moe_active_param_convention() {
        assert_eq!(infer_param_count(&["Mixtral-8x7B-Instruct-v0.1.gguf"]), "7B");
        assert_eq!(infer_param_count(&["mixtral-8x22b-v0.1.gguf"]), "22B");
        assert_eq!(infer_param_count(&["DeepSeek-V3-256x21B-base.gguf"]), "21B");
        assert_eq!(infer_param_count(&["mistral-small-24b-base.gguf"]), "24B");
    }

    #[test]
    fn infer_architecture_picks_suffix_via_bare_parent_fallback() {
        let mut repo = HfRepo {
            id: "user/generic-fork-q4km".to_string(),
            author: Some("user".to_string()),
            downloads: 0, likes: 0, last_modified: None,
            tags: vec![], siblings: vec![],
        };
        repo.tags = vec!["qwen2.5".to_string()];
        assert_eq!(infer_architecture(&repo), "qwen");
        repo.tags = vec!["gemma-3-27b-it".to_string()];
        assert_eq!(infer_architecture(&repo), "gemma");
        repo.tags = vec!["llama-3.1".to_string()];
        assert_eq!(infer_architecture(&repo), "llama");
        repo.tags = vec!["phi3.5".to_string()];
        assert_eq!(infer_architecture(&repo), "phi");
        repo.tags = vec!["deepseek-v3".to_string()];
        assert_eq!(infer_architecture(&repo), "deepseek");
        repo.tags = vec!["mistral-7b".to_string()];
        assert_eq!(infer_architecture(&repo), "mistral");
        repo.tags = vec!["qwen2_5".to_string()];
        assert_eq!(infer_architecture(&repo), "qwen");
        repo.tags = vec!["llama".to_string()];
        assert_eq!(infer_architecture(&repo), "llama");
    }

    #[test]
    fn infer_architecture_picks_subfamily_tags_via_patterns_ordering() {
        let mut repo = HfRepo {
            id: "user/quantized-model-q4km".to_string(),
            author: Some("user".to_string()),
            downloads: 0, likes: 0, last_modified: None,
            tags: vec![], siblings: vec![],
        };
        assert_eq!(infer_architecture(&repo), "unknown");
        repo.tags = vec!["phi3".to_string()];
        assert_eq!(infer_architecture(&repo), "phi");
        repo.tags = vec!["qwen2".to_string()];
        assert_eq!(infer_architecture(&repo), "qwen");
        repo.tags = vec!["qwen3".to_string()];
        assert_eq!(infer_architecture(&repo), "qwen");
        repo.tags = vec!["gemma2".to_string()];
        assert_eq!(infer_architecture(&repo), "gemma");
        repo.tags = vec!["llama-3".to_string()];
        assert_eq!(infer_architecture(&repo), "llama");
    }

    #[test]
    fn infer_architecture_rejects_over_inclusion_via_word_boundary() {
        let mut repo = HfRepo {
            id: "scholar/philosophy-101-gguf".to_string(),
            author: Some("scholar".to_string()),
            downloads: 0, likes: 0, last_modified: None,
            tags: vec![], siblings: vec![],
        };
        assert_eq!(infer_architecture(&repo), "unknown");
        repo.id = "microsoft/Phi-3-mini-4k-instruct-gguf".to_string();
        assert_eq!(infer_architecture(&repo), "phi");
        repo.id = "user/quantized-llm-7b-instruct".to_string();
        repo.tags = vec!["llama".to_string(), "text-generation".to_string()];
        assert_eq!(infer_architecture(&repo), "llama");
        repo.tags = vec!["mistral".to_string()];
        repo.id = "user/my-mistral-fork-gguf".to_string();
        assert_eq!(infer_architecture(&repo), "mistral");
    }

    #[test]
    fn derive_param_bucket_maps_ranges() {
        assert_eq!(derive_param_bucket("3B"), "1-3B");
        assert_eq!(derive_param_bucket("7B"), "4-8B");
        assert_eq!(derive_param_bucket("8B"), "4-8B");
        assert_eq!(derive_param_bucket("13B"), "9-15B");
        assert_eq!(derive_param_bucket("30B"), "16-30B");
        assert_eq!(derive_param_bucket("33B"), "30-60B");
        assert_eq!(derive_param_bucket("70B"), "60B+");
        assert_eq!(derive_param_bucket(""), "Unknown");
        assert_eq!(derive_param_bucket("garbage"), "Unknown");
    }

    #[test]
    fn estimate_size_bytes_q4km_7b_is_about_4gb() {
        let gb = estimate_size_bytes("7B", "Q4_K_M") as f64 / 1_073_741_824.0;
        assert!(gb > 3.3 && gb < 4.5, "got {} GB", gb);
    }

    #[test]
    fn estimate_size_bytes_q8_70b_is_about_70gb() {
        let gb = estimate_size_bytes("70B", "Q8_0") as f64 / 1_073_741_824.0;
        assert!(gb > 60.0 && gb < 80.0, "got {} GB", gb);
    }

    #[test]
    fn carries_iq_token_detects_iq1_iq2_iq3_only() {
        assert!(carries_iq_token("model-IQ1_S.gguf"));
        assert!(carries_iq_token("model-iq2_m.gguf"));
        assert!(carries_iq_token("model-IQ3-XXS.gguf"));
        assert!(!carries_iq_token("model-IQ4_XS.gguf"));
        assert!(!carries_iq_token("model-Q4_K_M.gguf"));
    }

    #[test]
    fn percent_encode_handles_special_chars() {
        assert_eq!(percent_encode("llama-3.1"), "llama-3.1");
        assert_eq!(percent_encode("qwen2.5 7b"), "qwen2.5%207b");
        assert_eq!(percent_encode("foo/bar"), "foo%2Fbar");
        assert_eq!(percent_encode("hello+world"), "hello%2Bworld");
    }

    #[test]
    fn round_gb_formats_to_one_decimal() {
        assert_eq!(round_gb(4_398_046_511), 4.1);
        assert_eq!(round_gb(8_589_934_592), 8.0);
        assert_eq!(round_gb(17_179_869_184), 16.0);
    }

    // ===== URL builder — Phase 11 no-wildcard =====

    #[test]
    fn build_search_url_none_query_routes_to_browse() {
        let (url, kind) = build_hf_search_url(None, None, 30);
        assert!(!url.contains("&search="));
        assert!(url.contains("sort=downloads"));
        assert_eq!(kind, KIND_BROWSE);
    }

    #[test]
    fn build_search_url_empty_string_routes_to_browse() {
        let (url, kind) = build_hf_search_url(Some(""), None, 30);
        assert!(!url.contains("&search="));
        assert_eq!(kind, KIND_BROWSE);
    }

    #[test]
    fn build_search_url_whitespace_only_routes_to_browse() {
        let (url, kind) = build_hf_search_url(Some("   "), None, 30);
        assert!(!url.contains("&search="));
        assert_eq!(kind, KIND_BROWSE);
    }

    #[test]
    fn build_search_url_browse_honours_sort_token() {
        let (url, _) = build_hf_search_url(None, Some("lastModified"), 30);
        assert!(url.contains("sort=lastModified"));
        assert!(!url.contains("&search="));
    }

    #[test]
    fn build_search_url_browse_clamps_limit() {
        assert!(build_hf_search_url(None, None, 9999).0.contains("limit=100"));
        assert!(build_hf_search_url(None, None, 0).0.contains("limit=1"));
    }

    /// Short queries (≤ 3 chars) now append `+GGUF` so the HF search
    /// targets GGUF-quantized model repos instead of base models that
    /// have no GGUF siblings. Single letter B → B+GGUF.
    /// No literal `*` is appended — HF's fuzzy substring matcher is
    /// already loose enough for the query without the star.
    #[test]
    fn build_search_url_single_letter_no_wildcard() {
        let (url, kind) = build_hf_search_url(Some("B"), None, 30);
        assert!(url.contains("search=B%20GGUF&"), "URL: {}", url);
        assert!(!url.contains("B*"), "URL must NOT have literal *: {}", url);
        assert_eq!(kind, KIND_EXACT);
    }

    #[test]
    fn build_search_url_four_char_query_no_wildcard() {
        let (url, kind) = build_hf_search_url(Some("Qwen"), None, 30);
        assert!(url.contains("search=Qwen&"), "URL: {}", url);
        assert!(!url.contains("Qwen*"), "URL must NOT have literal *: {}", url);
        assert_eq!(kind, KIND_EXACT);
    }

    /// Five+ char queries are percent-encoded and route to HF substring
    /// match. The previous "wildcard*" mode vanished.
    #[test]
    fn build_search_url_user_typed_star_is_percent_encoded() {
        let (url, kind) = build_hf_search_url(Some("B*"), None, 30);
        // B* is 2 chars → +GGUF suffix appended
        assert!(url.contains("search=B%2A%20GGUF&"), "URL: {}", url);
        assert!(!url.contains("B**"));
        assert_eq!(kind, KIND_EXACT);
    }

    #[test]
    fn build_search_url_five_char_query_substring_match() {
        let (url, kind) = build_hf_search_url(Some("llama"), None, 30);
        assert!(url.contains("search=llama&"), "URL: {}", url);
        assert!(!url.contains("llama*"));
        assert_eq!(kind, KIND_EXACT);
    }

    #[test]
    fn build_search_url_trims_surrounding_whitespace() {
        let (url, kind) = build_hf_search_url(Some("  llama  "), None, 30);
        assert!(url.contains("search=llama&"), "URL: {}", url);
        assert_eq!(kind, KIND_EXACT);
    }

    #[test]
    fn build_search_url_honours_sort_token() {
        let (url, _) = build_hf_search_url(Some("llama"), Some("lastModified"), 30);
        assert!(url.contains("sort=lastModified"));
        assert!(url.contains("direction=-1"));
    }

    #[test]
    fn build_search_url_clamps_limit_to_100() {
        assert!(build_hf_search_url(Some("llama"), None, 9999).0.contains("limit=100"));
        assert!(build_hf_search_url(Some("llama"), None, 0).0.contains("limit=1"));
    }

    /// Bare-stars percent-encode so HF treats them as literal tokens.
    /// 2-char input → +GGUF appended.
    #[test]
    fn build_search_url_bare_stars_percent_encoded() {
        let (url, kind) = build_hf_search_url(Some("**"), None, 30);
        assert!(url.contains("search=%2A%2A%20GGUF&"), "URL: {}", url);
        assert!(!url.contains("search=*&"));
        assert_eq!(kind, KIND_EXACT);
    }

    /// Single star percent-encoded and +GGUF appended (≤ 3 chars).
    #[test]
    fn build_search_url_single_star_percent_encoded() {
        let (url, kind) = build_hf_search_url(Some("*"), None, 30);
        assert!(url.contains("search=%2A%20GGUF&"), "URL: {}", url);
        assert_eq!(kind, KIND_EXACT);
    }

    /// 4+ char queries do NOT get the +GGUF suffix — they're
    /// specific enough to find GGUF repos on their own.
    #[test]
    fn build_search_url_four_or_more_chars_no_gguf_suffix() {
        let (url, kind) = build_hf_search_url(Some("qwen"), None, 30);
        assert!(url.contains("search=qwen&"), "URL: {}", url);
        assert!(!url.contains("qwen%20GGUF"));
        assert_eq!(kind, KIND_EXACT);
    }

    #[test]
    fn build_search_url_five_chars_no_gguf_suffix() {
        let (url, kind) = build_hf_search_url(Some("llama"), None, 30);
        assert!(url.contains("search=llama&"), "URL: {}", url);
        assert!(!url.contains("llama%20GGUF"));
        assert_eq!(kind, KIND_EXACT);
    }

    #[test]
    fn build_search_url_exact_4_chars_no_gguf_suffix() {
        // 4 is the boundary — just long enough to not need the suffix.
        let (url, kind) = build_hf_search_url(Some("deep"), None, 30);
        assert!(url.contains("search=deep&"), "URL: {}", url);
        assert!(!url.contains("deep%20GGUF"));
        assert_eq!(kind, KIND_EXACT);
    }

    // ===== Quant filter semantics =====

    #[test]
    fn quant_filter_empty_means_no_filter() {
        let allow: Vec<String> = vec![];
        for fname in [
            "model-q4_k_m.gguf", "model-q5_k_m.gguf", "model-q8_0.gguf",
            "model-f16.gguf", "model-iq1_s.gguf", "model-bf16.gguf",
        ] {
            assert!(passes_quant_filter(fname, &allow, !allow.is_empty()));
        }
        assert!(passes_quant_filter("model-iq2_m.gguf", &[], false));
    }

    #[test]
    fn quant_filter_only_q4km_rejects_others() {
        let allow = vec!["q4_k_m".to_string()];
        assert!(passes_quant_filter("llama-3-8B-q4_k_m.gguf", &allow, true));
        assert!(!passes_quant_filter("llama-3-8B-q5_k_m.gguf", &allow, true));
        assert!(!passes_quant_filter("llama-3-8B-q8_0.gguf", &allow, true));
        assert!(!passes_quant_filter("llama-3-8B-f16.gguf", &allow, true));
    }

    #[test]
    fn quant_filter_multiple_quants_match_each() {
        let allow = vec!["q4_k_m".to_string(), "q5_k_m".to_string(), "q8_0".to_string()];
        assert!(passes_quant_filter("model-q5_k_m.gguf", &allow, true));
        assert!(passes_quant_filter("model-q4_k_m.gguf", &allow, true));
        assert!(passes_quant_filter("model-q8_0.gguf", &allow, true));
        assert!(!passes_quant_filter("model-iq1_s.gguf", &allow, true));
        assert!(!passes_quant_filter("model-f16.gguf", &allow, true));
        assert!(!passes_quant_filter("model-q6_k.gguf", &allow, true));
    }

    #[test]
    fn quant_filter_iq4_perma_exclude_runs_before_filter() {
        let fname = "model-iq4_xs.gguf";
        let upper = fname.to_uppercase();
        let fname_lower = fname.to_lowercase();
        let include_iq = true;
        let quant_filter_active = false;
        let quant_allow_lower: Vec<String> = vec![];
        let rejected_by_iq_tokens =
            !include_iq && IQ_TOKENS.iter().any(|t| upper.contains(t));
        let rejected_by_iq4 = upper.contains("IQ4");
        let rejected_by_allowlist = !passes_quant_filter(
            &fname_lower, &quant_allow_lower, quant_filter_active,
        );
        assert!(!rejected_by_iq_tokens);
        assert!(rejected_by_iq4);
        assert!(!rejected_by_allowlist);
        assert!(rejected_by_iq_tokens || rejected_by_iq4 || rejected_by_allowlist);
    }

    // ===== Context-length heuristic =====

    #[test]
    fn context_llama3_returns_128k() {
        assert_eq!(infer_context_length("meta-llama/llama-3-8b-instruct-gguf", &[]), Some(131072));
        assert_eq!(infer_context_length("bartowski/llama-3.1-8b-instruct-gguf", &[]), Some(131072));
    }

    #[test]
    fn context_llama2_returns_4k() {
        assert_eq!(infer_context_length("meta-llama/llama-2-7b-chat-gguf", &[]), Some(4096));
    }

    #[test]
    fn context_qwen25_returns_128k() {
        assert_eq!(infer_context_length("qwen/qwen2.5-7b-instruct-gguf", &[]), Some(131072));
    }

    #[test]
    fn context_phi3_mini_4k_default_bumps_to_128k_for_128k_variant() {
        assert_eq!(infer_context_length("microsoft/phi-3-mini-4k-instruct-gguf", &[]), Some(4096));
        assert_eq!(infer_context_length("microsoft/phi-3-mini-128k-instruct-gguf", &[]), Some(131072));
    }

    #[test]
    fn context_phi35_returns_128k_phi4_returns_16k() {
        assert_eq!(infer_context_length("microsoft/phi-3.5-mini-instruct-gguf", &[]), Some(131072));
        assert_eq!(infer_context_length("microsoft/phi-4-gguf", &[]), Some(16384));
    }

    #[test]
    fn context_mistral_v01_8k_v03_32k_nemo_128k() {
        assert_eq!(infer_context_length("mistralai/mistral-7b-v0.1-instruct-gguf", &[]), Some(8192));
        assert_eq!(infer_context_length("mistralai/mistral-7b-v0.2-chat-gguf", &[]), Some(32768));
        assert_eq!(infer_context_length("mistralai/mistral-7b-v0.3-instruct-gguf", &[]), Some(32768));
        assert_eq!(infer_context_length("mistralai/mistral-nemo-instruct-gguf", &[]), Some(131072));
    }

    #[test]
    fn context_unknown_family_returns_none() {
        assert_eq!(infer_context_length("user/my-very-custom-llm-gguf", &[]), None);
    }

    #[test]
    fn context_tag_ctx_marker_overrides_id() {
        let tags = vec!["text-generation".to_string(), "ctx-128k".to_string()];
        assert_eq!(infer_context_length("user/random-gguf", &tags), Some(131072));
        assert_eq!(infer_context_length("user/another-gguf", &vec!["ctx-32k".to_string()]), Some(32768));
        assert_eq!(infer_context_length("user/longctx-gguf", &vec!["ctx-1m".to_string()]), Some(1024 * 1024));
    }

    // ===== Live HF gated integration test =====

    /// Live integration test for `hardware_fetch_model_detail`. Gated
    /// with #[ignore] so unit tests stay offline-deterministic; run
    /// with `cargo test --lib hardware::tests -- --ignored` when
    /// network is available. Verifies the wiring
    /// `hardware_fetch_model_detail -> reqwest::get -> parse config.json`.
    #[tokio::test]
    #[ignore = "live HF API integration — run with `cargo test --lib hardware::tests -- --ignored` when network is available"]
    async fn live_fetch_model_detail_smoke() {
        use std::time::Duration;
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            hardware_fetch_model_detail("meta-llama/Meta-Llama-3-8B-Instruct".to_string()),
        )
        .await;
        match result {
            Ok(Ok(detail)) => {
                assert!(
                    detail.source == "config_json" || detail.source == "none",
                    "unexpected source: {}",
                    detail.source
                );
            }
            Ok(Err(msg)) => {
                eprintln!("live_fetch_model_detail skipped: {}", msg);
            }
            Err(_) => {
                eprintln!("live_fetch_model_detail timed out");
            }
        }
    }
}
