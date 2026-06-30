// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the project root for the full license text.
// Copyright © 2026 Meridian Agent. All rights reserved.

//! Meridian — Hardware Scanner (HF GGUF model search backend).
//!
//! Single-stage `hardware_search_gguf_models` Tauri command. Replaces the
//! previous 62-round-trip-per-click pattern (1 list + 50 sibling fetches
//! done with `for await`) with one `full=true&limit=N` list call that comes
//! back with siblings + their sizes baked in. The Vue side then renders a
//! pre-ranked Vec<RankedGgufModel> with no client-side HF calls.
//!
//! Default filter values (sent from the Vue side on first paint):
//!   - sort: downloads desc
//!   - quant allowlist: Q4_K_M, Q5_K_M, Q6_K, Q8_0 (IQ1/2/3 always excluded
//!     unless include_iq is true)
//!   - trusted quantizers: empty (UI tier toggles it ON with default list)
//!   - only_fit: false (UI tier toggles it ON when combined VRAM > 0)
//!
//! `combined_vram_mb` is the total VRAM across local + RPC workers (per
//! AGENTS.md Phase 10: a single inference is joint across the pool). The
//! fit check applies a 10% safety buffer for KV cache / runtime overhead.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Trust whitelist the UI seeds by default (Bartowski, Unsloth, MaziyarPanahi,
/// LoneStriker, mradermacher). Empty `trusted_quantizers` param means the
/// trust filter is in "any" mode (no author is excluded) so the user sees
/// results across all authors. Sending a non-empty list switches it to
/// "only-allow" filter mode.
pub const DEFAULT_TRUSTED_QUANTIZERS: &[&str] = &[
    "bartowski",
    "unsloth",
    "maziyarpanahi",
    "lonestriker",
    "mradermacher",
];

/// Default quantization allowlist when the UI doesn't override. Q4_K_M is
/// the speed/quality sweet spot for most 7-13B models on 36-52GB; Q8_0 is
/// for users with VRAM to spare; Q5_K_M + Q6_K are middle ground. IQ
/// variants are always excluded unless `include_iq=true`.
pub const DEFAULT_QUANT_ALLOWLIST: &[&str] = &["Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"];

/// Tokens that mark a model as "IQ-quantized" — extreme size reduction at
/// significant quality loss. Excluded by default even when on the allowlist.
pub const IQ_TOKENS: &[&str] = &["IQ1", "IQ2", "IQ3"];

/// 10% safety buffer on top of current combined VRAM for fit checks.
/// llama.cpp KV cache + scratch + runtime overhead typically consumes
/// ~5-15% of nominal VRAM at inference start; 10% is the conservative
/// midpoint so a model "fits" it would actually load.
pub const VRAM_FIT_SAFETY_RATIO: f64 = 0.90;

/// Param-count buckets the UI exposes as filter chips. Filename pattern
/// `(N)\.?(N)?B` is parsed; models whose param count doesn't match a
/// filename token fall back to "Unknown" (the bucket UI chip labelled as
/// "Show all" instead).
pub const PARAM_BUCKETS: &[&str] = &[
    "1-3B", "4-8B", "9-15B", "16-30B", "30-60B", "60B+",
];

// ============================================================================
// IPC types (frontend-facing)
// ============================================================================

/// Search-knob payload the Vue sidebar sends to the backend on every
/// search click. Built up from `<script setup>` reactive refs in
/// `hardware.vue` and emitted via `invoke('hardware_search_gguf_models', { params })`.
///
/// `combined_vram_mb` is filled from the existing
/// `cluster::get_local_hardware` snapshot + any active RPC workers. Sending
/// it from the client (instead of re-fetching from the backend) keeps the
/// search hot path free of probe calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareSearchParams {
    /// Free-text search query, e.g. "qwen2.5", "llama-3.1".
    pub query: String,

    /// One of "downloads" (default), "lastModified", "likes".
    #[serde(default)]
    pub sort_by: Option<String>,

    /// HF list-page limit. 30 default; cap at 100 to stay polite to HF.
    #[serde(default)]
    pub limit: Option<u32>,

    /// Architecture filter — empty list = all. Tokens are lowercase.
    #[serde(default)]
    pub architectures: Vec<String>,

    /// Param-size filter — empty list = all. Tokens are bucket labels,
    /// see PARAM_BUCKETS.
    #[serde(default)]
    pub size_buckets: Vec<String>,

    /// Quant allowlist — empty list = DEFAULT_QUANT_ALLOWLIST.
    #[serde(default)]
    pub quant_allowlist: Vec<String>,

    /// Quantizer trust — empty list = "any mode" (no author excluded).
    /// Non-empty switches to "only-allow" filter mode and DOES exclude
    /// untrusted authors entirely.
    #[serde(default)]
    pub trusted_quantizers: Vec<String>,

    /// When true, IQ1/IQ2/IQ3 tokens are NOT excluded. Off by default
    /// because the quality hit is severe. Even when on, IQ4 stays
    /// excluded — IQ4 at -50 score is borderline and we keep the perma-exclude.
    #[serde(default)]
    pub include_iq: Option<bool>,

    /// When true, drop models whose best GGUF exceeds the combined VRAM
    /// (with safety buffer). Recommended when combined VRAM > 0.
    #[serde(default)]
    pub only_fit: Option<bool>,

    /// Sum of all GPUs' memoryTotal across local + RPC workers, in MiB.
    /// 0 means "no hardware data yet" — fit checks become "always true".
    pub combined_vram_mb: u64,
}

/// Single ranked result the Vue side renders as a card.
///
/// `fitsHardware` reflects the combined VRAM test (with 10% buffer). The
/// Vue side surfaces this as a green/red badge per card so JC can spot
/// what loads on his actual setup without doing arithmetic.
///
/// `primaryQuant` is the token we picked (e.g. "Q4_K_M") from the best
/// GGUF sibling — drives the quant badge color in the UI.
///
/// `ggufUrl` and `ggufFilename` are the direct-download pair passed to
/// `downloader_enqueue` from the existing downloader flow.
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
}

// ============================================================================
// HuggingFace list-response shapes (subset of fields we use)
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
    /// Populated when the list call sets `full=true`. Each sibling is one
    /// file in the repo; we filter to `.gguf` ext and pick the best by
    /// quant priority.
    #[serde(default)]
    siblings: Vec<HfSibling>,
}

#[derive(Debug, Deserialize)]
struct HfSibling {
    #[serde(rename = "rfilename")]
    rfilename: String,
    /// File size in bytes — present on `full=true` responses, absent for
    /// the cheap `full=false` payloads. We rely on `full=true` so this
    /// is populated for every GGUF sibling we touch.
    #[serde(default)]
    size: Option<u64>,
}

// ============================================================================
// Tauri command
// ============================================================================

/// Search HuggingFace for GGUF models matching the given filter set.
/// Returns a ranked, pre-resolved Vec<RankedGgufModel> ready for the Vue
/// result cards. No further HF round-trips are needed from the client.
///
/// Pipeline:
///   1. Build list URL with `full=true&limit=N` (siblings + size bundled).
///   2. Single HF list GET.
///   3. For each repo: filter by arch / param bucket / quantizer trust /
///      quant allowlist / IQ exclusion.
///   4. Pick the best GGUF per repo by quant priority score
///      (Q4_K_M > Q5_K_M > Q4_K_S > ...).
///   5. Compute `fitsHardware` against `combined_vram_mb` with 10% buffer.
///   6. Sort by user-selected field (downloads / lastModified / likes),
///      tiebreak: fits → trusted → best quant → alphabetical.
///
/// Errors are user-visible (returned via the invoke promise):
///   - empty query (no HF round-trip wasted)
///   - HTTP non-2xx (named explicitly)
///   - JSON schema drift ("Missing siblings array")
///   - HF rate limit (HTTP 429) surfaced verbatim — response carries the
///     `Retry-After` header so the UI can render it
#[tauri::command]
pub async fn hardware_search_gguf_models(
    params: HardwareSearchParams,
) -> Result<Vec<RankedGgufModel>, String> {
    let query = params.query.trim();
    if query.is_empty() {
        return Err("Search query must not be empty.".to_string());
    }

    // 1. Build URL. `full=true` is the linchpin: one round-trip returns
    // siblings with size, so we never need a `GET /api/models/<id>/tree/main`
    // follow-up. HF's free-tier rate limit (~100 req / 5 min) is generous
    // enough that 1-2 searches / minute from Meridian never trips it.
    let limit = params.limit.unwrap_or(30).clamp(1, 100);
    let mut url = format!(
        "https://huggingface.co/api/models?search={}&full=true&limit={}",
        percent_encode(query),
        limit
    );
    match params.sort_by.as_deref().unwrap_or("downloads") {
        "lastModified" => url.push_str("&sort=lastModified&direction=-1"),
        "likes" => url.push_str("&sort=likes&direction=-1"),
        // "downloads" — HF default sort is already downloads-desc, but
        // pass the param explicitly so the URL is self-documenting when
        // a future engineer reads it without this comment.
        _ => url.push_str("&sort=downloads&direction=-1"),
    };

    // 2. Fetch.
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

    // 3-5. Filter + score per repo.
    let quant_allow = if params.quant_allowlist.is_empty() {
        DEFAULT_QUANT_ALLOWLIST.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    } else {
        params.quant_allowlist.clone()
    };
    let quant_allow_lower: Vec<String> = quant_allow.iter().map(|q| q.to_lowercase()).collect();
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
        // Architecture filter — early reject.
        let architecture = infer_architecture(repo);
        if arch_filter_active
            && !arch_filter_lower.iter().any(|a| a == &architecture.to_lowercase())
        {
            continue;
        }
        // Param-bucket filter — early reject.
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

        // Quantizer trust — only-allow mode when the user provided a list.
        let author_lower = repo.author.as_deref().unwrap_or("").to_lowercase();
        let is_trusted = author_lower.is_empty()
            || trusted_lower.is_empty()
            || trusted_lower.iter().any(|t| t == &author_lower);
        if trust_filter_active && !is_trusted {
            continue;
        }

        // Pick the best GGUF that passes the quant + IQ filter.
        let mut best_pick: Option<&HfSibling> = None;
        let mut best_score = i32::MIN;
        for sib in repo.siblings.iter() {
            let fname = sib.rfilename.as_str();
            if !fname.to_lowercase().ends_with(".gguf") {
                continue;
            }
            if !include_iq && carries_iq_token(fname) {
                continue;
            }
            // IQ4 is always excluded — borderline-perma-exclude per
            // AGENTS.md Phase 10 quant-recommendation rules. Surfaces
            // only on explicit future "Include ALL IQs" flag (not
            // exposed in current UI; readded if JC asks).
            if fname.to_uppercase().contains("IQ4") {
                continue;
            }
            // Canonicalise both sides to lowercase so a future edit that
            // changes one side (allowlist casing, filename casing, dash-vs-
            // underscore) doesn't silently break the comparison.
            let fname_lower = fname.to_lowercase();
            if !quant_allow_lower.iter().any(|qa| fname_lower.contains(qa)) {
                continue;
            }
            let score = quant_priority_score(fname);
            if score > best_score {
                best_score = score;
                best_pick = Some(sib);
            }
        }
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
        });
    }

    // 6. Sort. Primary sort = user-selected; tiebreak = fit > trusted >
    // best-quant > alphabetical for stable, predictable ordering.
    match params.sort_by.as_deref().unwrap_or("downloads") {
        "lastModified" => ranked.sort_by(|a, b| b.last_modified.cmp(&a.last_modified)),
        "likes" => ranked.sort_by(|a, b| b.likes.cmp(&a.likes)),
        _ => ranked.sort_by(|a, b| b.downloads.cmp(&a.downloads)),
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
        "[hardware_search_gguf_models] query='{}' returned {} ranked results (raw repos: {})",
        query,
        ranked.len(),
        body.len()
    );
    Ok(ranked)
}

// ============================================================================
// Heuristic helpers (pure, testable)
// ============================================================================

/// Returns the priority score for a quant token in a GGUF filename.
/// Higher = more preferred. Order: Q4_K_M > Q5_K_M > Q4_K_S > Q5_K_S >
/// Q6_K > Q8_0 > Q4_0 > BF16/F16 > F32 > unknown.
fn quant_priority_score(name: &str) -> i32 {
    let upper = name.to_uppercase();
    if upper.contains("Q4_K_M") {
        return 100;
    }
    if upper.contains("Q5_K_M") {
        return 90;
    }
    if upper.contains("Q4_K_S") {
        return 85;
    }
    if upper.contains("Q5_K_S") {
        return 80;
    }
    if upper.contains("Q6_K") {
        return 70;
    }
    if upper.contains("Q8_0") {
        return 60;
    }
    if upper.contains("Q4_0") {
        return 55;
    }
    if upper.contains("BF16") || upper.contains("F16") {
        return 40;
    }
    if upper.contains("F32") {
        return 20;
    }
    if upper.contains("IQ4") {
        return 30;
    }
    if upper.contains("IQ3") {
        return 5;
    }
    if upper.contains("IQ2") || upper.contains("IQ1") {
        return 1;
    }
    10 // any other GGUF — last-resort catch-all
}

/// Returns true when the filename carries an IQ1 / IQ2 / IQ3 token. Both
/// `IQ1_XSS` style (lowercase or uppercase, dash or underscore) match.
fn carries_iq_token(name: &str) -> bool {
    let upper = name.to_uppercase().replace('-', "_");
    let normalised = upper.as_str();
    IQ_TOKENS.iter().any(|t| normalised.contains(t))
}

/// Pulls the leading `<quant>` token out of a filename like
/// `Llama-3-8B-Instruct-Q4_K_M.gguf`. Returns `Q4_K_M` (uppercase, dash
/// normalised). Falls back to `"GGUF"` when no recognisable token matches.
fn extract_quant_token(filename: &str) -> String {
    let upper = filename.to_uppercase().replace('-', "_");
    // Order matters — Q4_K_M must precede Q4_0 and Q4_K_S.
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

/// Derives the canonical architecture label for a repo. Two-phase:
///
/// 1. **Tag-priority match.** HF tags are curated; only exact token hits
///    after splitting by `-`/`_`/space count. A "phi" tag pulls a model
///    into the Phi bucket without ambiguity.
/// 2. **Id fallback with word-boundary check.** Id substrings can
///    over-include ("philosophy" contains "phi"). Only match when the
///    token is preceded by a path/delimiter boundary OR extends to the
///    end of the string. Catches the common case where HF lists a Lambda
///    or Mistral fork without the proper tag.
///
/// Returns "unknown" when no token matches. UI surfaces that as "Other"
/// so users can spot-check taxonomy gaps.
fn infer_architecture(repo: &HfRepo) -> String {
    // Subfamily tokens MUST come before their bare parents — the loop
    // returns on the first match. A repo tagged `["phi3"]` returns
    // `"phi"` via `("phi3", "phi")`, NOT `"unknown"` via a missed
    // bare-`"phi"` check.
    const PATTERNS: &[(&str, &str)] = &[
        ("llama-3", "llama"),
        ("llama", "llama"),
        ("qwen3", "qwen"),
        ("qwen2", "qwen"),
        ("qwen", "qwen"),
        ("mixtral", "mistral"),
        ("mistral", "mistral"),
        ("gemma2", "gemma"),
        ("gemma", "gemma"),
        ("phi4", "phi"),
        ("phi3", "phi"),
        ("phi", "phi"),
        ("deepseek", "deepseek"),
    ];
    // 1. Tags. Match either (a) exact token, OR (b) token-prefix where the
    // next char is a digit / dot / dash `â€• picks up `qwen2.5`, `llama-3.1`,
    // `phi3-mini-128k`, `gemma-3-27b-it`. Bare dash-prefix (`starts_with("qwen-")`)
    // misses the dot convention HF ships most modern Qwen / Gemma / Llama
    // variants under.
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
        if matches {
            return (*arch).to_string();
        }
    }
    // 2. Repo-id with word-boundary check: token must be at path start
    // or follow `/`, `-`, `_`. Trailing chars must be a delimiter, digit,
    // or string-end — never a letter (which would indicate an embed like
    // "philosophy" or "lollms-llama-shimmer").
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

/// Walks the filename list looking for a `<digits>B` token. Returns the
/// first match (e.g. "7B"). Empty string when no token matches.
///
/// MoE notation (`<total>x<active>B` such as `Mixtral-8x7B-Instruct`)
/// returns the ACTIVE param count — the digit run AFTER the `x` —
/// which is the convention HF + llama.cpp use for VRAM budgeting on
/// sparse models.
fn infer_param_count(filenames: &[&str]) -> String {
    for fname in filenames.iter() {
        let upper = fname.to_uppercase();
        let chars: Vec<char> = upper.chars().collect();
        let n = chars.len();
        let mut i = 0;
        while i < n {
            if !chars[i].is_ascii_digit() {
                i += 1;
                continue;
            }
            // Collect digit run (with optional decimal).
            let start = i;
            while i < n && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let digits: String = chars[start..i].iter().collect();
            // Optional MoE "X" marker: re-scan for the ACTIVE digit run
            // after the X.
            if i < n && chars[i] == 'X' {
                let after_x = i + 1;
                let mut j = after_x;
                while j < n && (chars[j].is_ascii_digit() || chars[j] == '.') {
                    j += 1;
                }
                if j > after_x && j < n && chars[j] == 'B' {
                    return format!("{}B", chars[after_x..j].iter().collect::<String>());
                }
                i = j;
                continue;
            }
            // Plain `<digits>B` — the canonical case.
            if i < n && chars[i] == 'B' && !digits.is_empty() {
                return format!("{}B", digits);
            }
        }
    }
    String::new()
}

/// Maps a `<digits>B` label like "7B" or "70B" into a UI bucket label
/// from PARAM_BUCKETS. Returns `"Unknown"` when the label is empty or
/// falls outside any defined bucket (caller filters at the bucket
/// check, but we never want to crash on odd patterns).
fn derive_param_bucket(label: &str) -> String {
    // Strip the trailing B if present.
    let body = label.trim_end_matches('B').trim_end_matches('b');
    let n: f64 = match body.parse() {
        Ok(v) => v,
        Err(_) => return "Unknown".to_string(),
    };
    match n {
        x if x <= 3.0 => "1-3B".to_string(),
        x if x <= 8.0 => "4-8B".to_string(),
        x if x <= 15.0 => "9-15B".to_string(),
        x if x <= 30.0 => "16-30B".to_string(),
        x if x <= 60.0 => "30-60B".to_string(),
        _ => "60B+".to_string(),
    }
}

/// Rough estimate of a GGUF's file size when HF didn't include `size` in
/// the response (older API behaviour, or non-`full=true` clients). Built
/// off the parameter count + quant class — accurate to ~10-20% which is
/// good enough for "fits my hardware" but not for download UI sizing
/// (the actual download will report the real bytes).
fn estimate_size_bytes(param_count: &str, quant: &str) -> u64 {
    let params: f64 = param_count
        .trim_end_matches('B')
        .trim_end_matches('b')
        .parse()
        .unwrap_or(0.0);
    if params == 0.0 {
        return 0;
    }
    // Bytes per param by quant (industry-standard heuristics):
    //   Q4_K_M ~ 0.55 B/param, Q5_K_M ~ 0.65, Q6_K ~ 0.75, Q8_0 ~ 0.95,
    //   F16 ~ 2.0, F32 ~ 4.0
    let bp = match quant {
        q if q.contains("Q4_K_M") => 0.55,
        q if q.contains("Q5_K_M") => 0.65,
        q if q.contains("Q6_K") => 0.75,
        q if q.contains("Q8_0") => 0.95,
        q if q.contains("BF16") || q.contains("F16") => 2.0,
        q if q.contains("F32") => 4.0,
        _ => 0.70, // conservative default
    };
    (params * 1_000_000_000.0 * bp) as u64
}

/// Rounds bytes → GB with 1-decimal precision (matches the UI's std
/// formatter, avoids the `4.700000000000001` float-rounding display bug).
fn round_gb(bytes: u64) -> f64 {
    let gb = bytes as f64 / 1_073_741_824.0;
    (gb * 10.0).round() / 10.0
}

/// Trivial percent-encoder. Only the search query goes through this and
/// it only ever contains a-z, 0-9, '.', '-', and sometimes ' '. Keeping
/// it local saves a dep on the `urlencoding` crate.
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
        let q4km = quant_priority_score("llama-3-8B-Q4_K_M.gguf");
        let q5km = quant_priority_score("llama-3-8B-Q5_K_M.gguf");
        let q8 = quant_priority_score("llama-3-8B-Q8_0.gguf");
        let f16 = quant_priority_score("llama-3-8B-F16.gguf");
        let f32 = quant_priority_score("llama-3-8B-F32.gguf");
        assert!(q4km > q5km, "Q4_K_M={} should be > Q5_K_M={}", q4km, q5km);
        assert!(q5km > q8, "Q5_K_M={} should be > Q8_0={}", q5km, q8);
        assert!(q8 > f16, "Q8_0={} should be > F16={}", q8, f16);
        assert!(f16 > f32, "F16={} should be > F32={}", f16, f32);
    }

    #[test]
    fn quant_priority_treats_q4km_preferred_over_q4_0() {
        // The bug-prone case: a filename with both `Q4_K_M` and `Q4_0`
        // substrings (e.g. "Q4_K_M-Q4_0-fallback"). Order in source
        // matters — Q4_K_M must win.
        let score = quant_priority_score("Q4_K_M-v1-Q4_0-fallback.gguf");
        assert_eq!(score, 100, "filename with both tokens should pick Q4_K_M");
    }

    #[test]
    fn quant_priority_penalizes_iq1_iq2_below_default() {
        let default = quant_priority_score("random-model.gguf");
        let iq1 = quant_priority_score("model-IQ1_S.gguf");
        let iq2 = quant_priority_score("model-IQ2_M.gguf");
        let iq3 = quant_priority_score("model-IQ3_S.gguf");
        assert!(iq1 < default, "IQ1 should score below default-fallback");
        assert!(iq2 < default, "IQ2 should score below default-fallback");
        assert!(iq3 < default, "IQ3 should score below default-fallback");
    }

    #[test]
    fn extract_quant_token_matches_q4km() {
        assert_eq!(extract_quant_token("Llama-3-8B-Instruct-Q4_K_M.gguf"), "Q4_K_M");
        assert_eq!(
            extract_quant_token("qwen2.5-7b-instruct-q5_k_m.gguf"),
            "Q5_K_M"
        );
        assert_eq!(extract_quant_token("phi-3-q6_K.gguf"), "Q6_K");
        assert_eq!(extract_quant_token("mistral-7B-q8_0.gguf"), "Q8_0");
        assert_eq!(extract_quant_token("totally-not-a-model.gguf"), "GGUF");
    }

    #[test]
    fn infer_param_count_finds_standard_patterns() {
        assert_eq!(infer_param_count(&["Llama-3-8B-Instruct-Q4_K_M.gguf"]), "8B");
        assert_eq!(infer_param_count(&["qwen2.5-7B-Instruct-Q5_K_M.gguf"]), "7B");
        assert_eq!(infer_param_count(&["deepseek-coder-33b.gguf"]), "33B");
        // No digit-then-B pattern: phi-3 has no `B` suffix at all.
        assert_eq!(infer_param_count(&["phi-3-mini-4k-instruct.gguf"]), "");
        // Truly no match at all.
        assert_eq!(infer_param_count(&["totally-not-a-model.gguf"]), "");
    }

    #[test]
    fn infer_param_count_handles_moe_active_param_convention() {
        // Mixtral-8x7B active params (the run AFTER the X) = 7B, NOT 8B.
        // The naive implementation that returns the first digit run would
        // classify Mixtral 8x7B as 8B — over-budgeting by ~5x on a 13B-tier
        // fit check.
        assert_eq!(infer_param_count(&["Mixtral-8x7B-Instruct-v0.1.gguf"]), "7B");
        assert_eq!(infer_param_count(&["mixtral-8x22b-v0.1.gguf"]), "22B");
        // DeepSeek-V3 sparse 256x21B active
        assert_eq!(infer_param_count(&["DeepSeek-V3-256x21B-base.gguf"]), "21B");
        // Lowercase variants work too.
        assert_eq!(infer_param_count(&["mistral-small-24b-base.gguf"]), "24B");
    }

    #[test]
    fn infer_architecture_picks_suffix_via_bare_parent_fallback() {
        // Suffix variants like "qwen2.5", "gemma-3-27b-it", "llama-3.1"
        // don't match the bare PATTERNS tokens directly, but each has a
        // dash-prefix match against the bare parent ("qwen-", "gemma-",
        // "llama-") `\u2014 the `t.starts_with(format!("{}-", token))` branch
        // in the tag check picks them up. This test pins down the intent
        // so a future engineer who reads the test set cannot accidentally
        // regress the suffix-coverage behavior.
        let mut repo = HfRepo {
            id: "user/generic-fork-q4km".to_string(),
            author: Some("user".to_string()),
            downloads: 0,
            likes: 0,
            last_modified: None,
            tags: vec![],
            siblings: vec![],
        };
        repo.tags = vec!["qwen2.5".to_string()];
        assert_eq!(infer_architecture(&repo), "qwen");
        repo.tags = vec!["gemma-3-27b-it".to_string()];
        assert_eq!(infer_architecture(&repo), "gemma");
        repo.tags = vec!["llama-3.1".to_string()];
        assert_eq!(infer_architecture(&repo), "llama");
        // Dotted-versions fire via the digit/dot/dash suffix rule.
        repo.tags = vec!["phi3.5".to_string()];
        assert_eq!(infer_architecture(&repo), "phi");
        repo.tags = vec!["deepseek-v3".to_string()];
        assert_eq!(infer_architecture(&repo), "deepseek");
        repo.tags = vec!["mistral-7b".to_string()];
        assert_eq!(infer_architecture(&repo), "mistral");
        // Underscore variants (rare but legal in HF tags).
        repo.tags = vec!["qwen2_5".to_string()];
        assert_eq!(infer_architecture(&repo), "qwen");
        // Bare (no suffix) tags still match.
        repo.tags = vec!["llama".to_string()];
        assert_eq!(infer_architecture(&repo), "llama");
    }

    #[test]
    fn infer_architecture_picks_subfamily_tags_via_patterns_ordering() {
        // Subfamily tokens (phi3, phi4, qwen2, qwen3) MUST come before
        // bare tokens in PATTERNS so a tag of "phi3" matches before the
        // bare "phi" fallback. Regression net for the one-line reorder
        // fix.
        let mut repo = HfRepo {
            id: "user/quantized-model-q4km".to_string(),
            author: Some("user".to_string()),
            downloads: 0,
            likes: 0,
            last_modified: None,
            tags: vec!["phi3".to_string()],
            siblings: vec![],
        };
        assert_eq!(infer_architecture(&repo), "phi");
        repo.tags = vec!["phi4".to_string()];
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
        // "philosophy" contains "phi" as a substring — naive `contains`
        // would falsely tag this as Phi-family. Word-boundary check
        // should reject because "phi" is FOLLOWED BY "l" (an embedded
        // letter, not a delimiter/digit/path-sep).
        let mut repo = HfRepo {
            id: "scholar/philosophy-101-gguf".to_string(),
            author: Some("scholar".to_string()),
            downloads: 0,
            likes: 0,
            last_modified: None,
            tags: vec![],
            siblings: vec![],
        };
        assert_eq!(infer_architecture(&repo), "unknown");
        // A clean Phi-3 repo IS picked up via word-boundary.
        repo.id = "microsoft/Phi-3-mini-4k-instruct-gguf".to_string();
        assert_eq!(infer_architecture(&repo), "phi");
        // Clean tag wins even when the id is generic.
        repo.id = "user/quantized-llm-7b-instruct".to_string();
        repo.tags = vec!["llama".to_string(), "text-generation".to_string()];
        assert_eq!(infer_architecture(&repo), "llama");
        // Tag-priority beats id-substring fallthrough for tied signals.
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
        // 7B params * 0.55 B/param = 3.85 GB. Allow ±15% margin.
        let bytes = estimate_size_bytes("7B", "Q4_K_M");
        let gb = bytes as f64 / 1_073_741_824.0;
        assert!(
            gb > 3.3 && gb < 4.5,
            "7B Q4_K_M should be ~3.85GB, got {} GB",
            gb
        );
    }

    #[test]
    fn estimate_size_bytes_q8_70b_is_about_70gb() {
        let bytes = estimate_size_bytes("70B", "Q8_0");
        let gb = bytes as f64 / 1_073_741_824.0;
        assert!(
            gb > 60.0 && gb < 80.0,
            "70B Q8_0 should be ~66GB, got {} GB",
            gb
        );
    }

    #[test]
    fn carries_iq_token_detects_iq1_iq2_iq3_only() {
        assert!(carries_iq_token("model-IQ1_S.gguf"), "IQ1 must trip the guard");
        assert!(carries_iq_token("model-iq2_m.gguf"), "lowercase IQ2 must trip the guard");
        assert!(carries_iq_token("model-IQ3-XXS.gguf"), "dash variant of IQ3 must trip the guard");
        assert!(
            !carries_iq_token("model-IQ4_XS.gguf"),
            "IQ4 should NOT trip the IQ1-3 guard"
        );
        assert!(!carries_iq_token("model-Q4_K_M.gguf"), "Q4_K_M is not IQ");
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
        assert_eq!(round_gb(8_589_934_592), 8.0); // exactly 8 GiB
        assert_eq!(round_gb(17_179_869_184), 16.0); // exactly 16 GiB
    }

    #[test]
    fn empty_params_query_rejected() {
        // Pure test of the precondition (must not await HTTP). We can't
        // construct a full HfRepo body without a server, so this checks
        // only the empty-query branch.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(hardware_search_gguf_models(HardwareSearchParams {
            query: "".to_string(),
            sort_by: None,
            limit: None,
            architectures: vec![],
            size_buckets: vec![],
            quant_allowlist: vec![],
            trusted_quantizers: vec![],
            include_iq: None,
            only_fit: None,
            combined_vram_mb: 0,
        }));
        assert!(result.is_err(), "empty query must reject without HTTP");
        assert!(
            result.unwrap_err().contains("empty"),
            "error must mention empty query"
        );
    }
}
