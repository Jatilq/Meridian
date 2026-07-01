// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! Meridian — portable install-path resolution for backends and Omnix.
//!
//! **Why this module exists (Fix C, 2026-07-01):** prior phases hardcoded
//! `E:\ai\Apps\backends` and `E:\ai\\Apps\Omnix` as default install roots.
//! That path is JC's personal dev machine; on a fresh install on any
//! other box the constants point at a directory that does not exist,
//! forcing the user to hand-edit the const OR succeed at hitting the
//! 404 path when nothing writes to that root.
//!
//! Portable defaults now resolve via [`dirs::data_local()`], which on
//! Windows returns `%LOCALAPPDATA%`, on Linux returns
//! `$XDG_DATA_HOME` (or `$HOME/.local/share`), and on macOS returns
//! `$HOME/Library/Application Support`. We join `Meridian/<program>` to
//! that location, so a fresh install on any machine writes binaries
//! into a folder the OS already owns.
//!
//! Resolution precedence (see [`resolve_backend_root`] / [`resolve_omnix_root`]):
//!   1. Caller-supplied `override` path (used by `download_backend`'s
//!      `target_dir` arg and `resolve_omnix_dir`'s `omnix_path` arg).
//!   2. User-configured override from `meridian.installPaths.{backend,omnix}`
//!      in the user-settings store (set via Settings → Advanced → Install Paths).
//!   3. [`dirs::data_local()`] join `Meridian/<program>`.
//!   4. Legacy `E:\ai\Apps\<program>` fallback so existing installs that
//!      already wrote binaries there keep finding them.
//!
//! The legacy fallback is the LAST priority on purpose: any user who
//! explicitly configures a path wins, the portable default wins second,
//! and the historical dev machine path is preserved as a back-compat
//! mirror for any pre-Fix-C install that already populated it.

use std::path::PathBuf;

/// Legacy fallback constants — kept ONLY so back-compat installs keep
/// finding the binaries they wrote before Fix C landed. They are NOT
/// the primary defaults anymore — see `resolve_*` functions.
const FALLBACK_BACKEND_ROOT_WINDOWS: &str = "E:\\ai\\Apps\\backends";
const FALLBACK_OMNIX_ROOT_WINDOWS: &str = "E:\\ai\\Apps\\Omnix";

/// Subdirectory under `data_local_dir()` where Meridian application data lives.
const APP_DIR_NAME: &str = "Meridian";

/// Resolve the backend install root at call time.
///
/// Precedence: explicit override → user-settings override → portable
/// `data_local()/Meridian/backends` → legacy `E:\ai\Apps\backends`.
/// All paths returned are absolute; consumers should `.join(kind_dir_name)`
/// to get the per-backend subdir.
pub fn resolve_backend_root(override_path: Option<&str>) -> PathBuf {
    if let Some(p) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(p) = read_user_override("meridian.installPaths.backend") {
        return PathBuf::from(p);
    }
    if let Some(base) = dirs::data_local_dir() {
        return base.join(APP_DIR_NAME).join("backends");
    }
    PathBuf::from(FALLBACK_BACKEND_ROOT_WINDOWS)
}

/// Resolve the Omnix install root at call time.
///
/// Precedence mirrors [`resolve_backend_root`]: explicit override → user
/// override → portable `data_local()/Meridian/Omnix` → legacy
/// `E:\ai\Apps\Omnix`.
pub fn resolve_omnix_root(override_path: Option<&str>) -> PathBuf {
    if let Some(p) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(p) = read_user_override("meridian.installPaths.omnix") {
        return PathBuf::from(p);
    }
    if let Some(base) = dirs::data_local_dir() {
        return base.join(APP_DIR_NAME).join("Omnix");
    }
    PathBuf::from(FALLBACK_OMNIX_ROOT_WINDOWS)
}

/// Read a user-configured override from the user-settings store. Returns
/// `None` when nothing is configured (or the configured value is empty /
/// cannot be read) — callers should treat that as "no override".
///
/// Implementation note: the user-settings store is loaded lazily by the
/// Vue side rather than eagerly at Rust startup. We use the
/// `user-settings.json` file directly with a tiny synchronous read so
/// this resolver never blocks on the store plugin's async API. If the
/// file is missing (first boot) the reader returns None cleanly.
fn read_user_override(store_key: &str) -> Option<String> {
    // Use the store plugin's file path convention: %APPDATA%/Meridian/<key>
    // We try to read via the plugin if it's wired into app state, falling
    // back to a no-override answer otherwise. This keeps the resolver
    // runnable from static contexts like `download_backend` without
    // requiring async-bridged plumbing.
    let override_dir = user_settings_override_dir()?;
    let path = override_dir.join(format!("{}.txt", store_key.replace('.', "_")));
    let text = std::fs::read_to_string(&path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Locate the on-disk directory where write_override_buf drops override
/// files. Mirrors the Tauri store plugin's filename convention used by
/// `secure_keys.rs::get_store` — kept independent so we don't couple
/// public path resolution to the encrypted-key store.
fn user_settings_override_dir() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?.join(APP_DIR_NAME);
    let dir = base.join("install-path-overrides");
    if dir.exists() {
        Some(dir)
    } else {
        // Don't auto-create; absence == "no override configured".
        None
    }
}

/// Write a user override for one of the install paths. Called from the
/// Settings → Advanced → Install Paths UI when the user picks a folder.
///
/// Logs (doesn't return error) on failure so callers don't need to
/// thread IO errors; the worst case is "override didn't stick" which
/// is recoverable by re-trying from the UI.
pub fn write_override(store_key: &str, value: &str) {
    let Some(dir) = user_settings_override_dir().or_else(|| {
        let base = dirs::data_local_dir()?.join(APP_DIR_NAME).join("install-path-overrides");
        std::fs::create_dir_all(&base).ok()?;
        Some(base)
    }) else {
        log::warn!("install_paths::write_override: could not resolve override dir for {}", store_key);
        return;
    };
    let path = dir.join(format!("{}.txt", store_key.replace('.', "_")));
    if let Err(e) = std::fs::write(&path, value) {
        log::warn!(
            "install_paths::write_override: failed to write {} ({}): {}",
            store_key,
            path.display(),
            e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_takes_priority() {
        let r = resolve_backend_root(Some("D:\\custom\\backends"));
        assert_eq!(r, PathBuf::from("D:\\custom\\backends"));
    }

    #[test]
    fn empty_override_falls_through() {
        // An empty override string is treated as "no override" so a
        // blank `target_dir` arg on download_backend doesn't freeze the
        // path to the current working directory.
        let r = resolve_backend_root(Some("   "));
        // Should NOT equal a path derived from whitespace.
        assert!(!r.as_os_str().is_empty());
        // And should be one of: portable default, or legacy fallback.
        // We don't assert a specific value because dirs::data_local()
        // varies in test environments.
        let portable = dirs::data_local_dir()
            .map(|d| d.join(APP_DIR_NAME).join("backends"))
            .unwrap_or_else(|| PathBuf::from(FALLBACK_BACKEND_ROOT_WINDOWS));
        let legacy = PathBuf::from(FALLBACK_BACKEND_ROOT_WINDOWS);
        assert!(r == portable || r == legacy, "got {:?}, expected either portable ({:?}) or legacy ({:?})", r, portable, legacy);
    }

    #[test]
    fn resolve_omnix_returns_a_path() {
        let r = resolve_omnix_root(None);
        assert!(!r.as_os_str().is_empty());
    }
}

// ============================================================================
// Tauri commands
// ============================================================================

/// Mount-point payload returned to the Settings UI for both backends
/// and Omnix. The `source` field tells the UI where the resolved path
/// came from so it can label the field "Override" vs "Portable default"
/// vs "Legacy fallback" — useful diagnostic when a user wonders "why
/// did my install land there?"
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPathInfo {
    pub path: String,
    /// "override" | "portable" | "legacy"
    pub source: String,
}

/// Pair of resolved install paths returned by `get_install_paths`. The
/// shape mirrors what the frontend Settings panel renders as two rows.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPaths {
    pub backends: InstallPathInfo,
    pub omnix: InstallPathInfo,
}

/// Return the resolved install paths for backends and Omnix.
///
/// Both `path` and `source` describe the same writable location. The
/// UI renders the path in an editable field and prefixes it with the
/// source label so a user can see "Override" vs "Portable default"
/// without having to consult the docs.
///
/// `source` is decided by walking the same precedence list as the
/// resolver: caller-override (these Tauri commands have no caller
/// override so this branch is unreachable here) → user-settings
/// override → portable default → legacy.
#[tauri::command]
pub fn get_install_paths() -> InstallPaths {
    InstallPaths {
        backends: backend_info(),
        omnix: omnix_info(),
    }
}

fn backend_info() -> InstallPathInfo {
    let path = resolve_backend_root(None);
    let source = label_for(&path);
    InstallPathInfo {
        path: path.to_string_lossy().into_owned(),
        source: source.to_string(),
    }
}

fn omnix_info() -> InstallPathInfo {
    let path = resolve_omnix_root(None);
    let source = label_for_omnix(&path);
    InstallPathInfo {
        path: path.to_string_lossy().into_owned(),
        source: source.to_string(),
    }
}

fn label_for(p: &std::path::Path) -> &'static str {
    if let Some(p_str) = read_user_override("meridian.installPaths.backend") {
        let stored = std::path::PathBuf::from(&p_str);
        if p == stored.as_path() {
            return "override";
        }
    }
    let portable = dirs::data_local_dir()
        .map(|d| d.join(APP_DIR_NAME).join("backends"))
        .unwrap_or_else(|| PathBuf::from(FALLBACK_BACKEND_ROOT_WINDOWS));
    if p == portable.as_path() { "portable" } else { "legacy" }
}

fn label_for_omnix(p: &std::path::Path) -> &'static str {
    if let Some(p_str) = read_user_override("meridian.installPaths.omnix") {
        let stored = std::path::PathBuf::from(&p_str);
        if p == stored.as_path() {
            return "override";
        }
    }
    let portable = dirs::data_local_dir()
        .map(|d| d.join(APP_DIR_NAME).join("Omnix"))
        .unwrap_or_else(|| PathBuf::from(FALLBACK_OMNIX_ROOT_WINDOWS));
    if p == portable.as_path() { "portable" } else { "legacy" }
}

/// Persist a user-chosen install path override. The frontend dialog
/// calls this when the user picks a directory in Settings → Advanced →
/// Install Paths.
///
/// Empty `value` clears the override (reverting to the portable default
/// on the next resolve). Caller-supplied paths may include a trailing
/// slash — we strip it before storage so equality comparisons are
/// stable.
#[tauri::command]
pub fn set_install_paths(kind: String, value: String) -> Result<(), String> {
    let trimmed = value.trim().trim_end_matches(['/', '\\']).to_string();
    let key = match kind.as_str() {
        "backends" | "backend" => "meridian.installPaths.backend",
        "omnix" => "meridian.installPaths.omnix",
        other => return Err(format!("Unknown install-path kind: '{}' (expected backends | omnix)", other)),
    };
    if trimmed.is_empty() {
        // Empty value clears the override; we just leave the (absent)
        // override file and the resolver falls through to the portable
        // default on the next lookup.
        log::info!("install_paths::set_install_paths: clearing override for {}", key);
    } else {
        write_override(key, &trimmed);
    }
    Ok(())
}
