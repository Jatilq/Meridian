// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

//! SSH-based cluster control (Phase 6) using russh (pure-Rust SSH).
//!
//! Commands: check_node_status, get_gpu_stats, launch_rpc_slave.
//! Credentials are passed in from the frontend (stored encrypted via the Tauri
//! store on the JS side) — never hardcoded here. SSH connection testing against
//! live MAMBA/BLACK is PENDING user confirmation; this module is written to
//! compile and is wired but unverified against real hosts.

use std::sync::Arc;
use async_trait::async_trait;
use russh::client;
use russh::keys::key;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::secure_keys;

/// GPU stats for one device, parsed from nvidia-smi / rocm-smi / WMI output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuStat {
    pub index: u32,
    pub name: String,
    pub utilization: u32,    // percent (0 if unavailable)
    #[serde(rename = "memoryUsed")]
    pub memory_used: u64,    // MB
    #[serde(rename = "memoryTotal")]
    pub memory_total: u64,   // MB
    pub temperature: u32,    // celsius (0 if unavailable)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatusResult {
    pub online: bool,
    pub gpus: Vec<GpuStat>,
}

/// CPU info for a machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub name: String,
    pub cores: u32,
    pub utilization: f32, // percent
}

/// RAM info for a machine (all values in MB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamInfo {
    #[serde(rename = "totalMb")]
    pub total_mb: u64,
    #[serde(rename = "usedMb")]
    pub used_mb: u64,
    #[serde(rename = "freeMb")]
    pub free_mb: u64,
    pub utilization: f32, // percent
}

/// Full hardware snapshot for one machine (CPU + RAM + GPUs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSnapshot {
    pub online: bool,
    pub cpu: Option<CpuInfo>,
    pub ram: Option<RamInfo>,
    pub gpus: Vec<GpuStat>,
    pub error: Option<String>,
}

/// SSH connection parameters supplied by the frontend (from encrypted store).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshCredentials {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    /// Absolute path to a private key file (key-based auth). Takes precedence
    /// over `password` if both are provided.
    pub key_path: Option<String>,
    /// Optional plaintext password for password-based SSH auth (used when
    /// `key_path` is None or empty). Used as a fallback for in-flight
    /// operations (e.g. Test Connection modal) where no encrypted key has
    /// been issued yet.
    pub password: Option<String>,
    /// Optional reference to a secret stored in the secure-keys store via
    /// `secure_store_secret`. When set and `password` is empty, the plaintext
    /// is fetched on the Rust side and used at auth time. The frontend never
    /// needs to send plaintext over IPC for stored credentials.
    pub password_secure_key: Option<String>,
}

fn default_port() -> u16 {
    22
}

/// Minimal russh client handler. We accept the server key (homelab LAN hosts);
/// production hardening could pin host keys here.
struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Connect over SSH and run a single command, returning its stdout.
/// Auth method resolution:
/// 1. `key_path` if non-empty -> key-based auth.
/// 2. `password_secure_key` if set -> look up plaintext via
///    `secure_keys::secure_get_secret` on the Rust side.
/// 3. `password` (plaintext) as final fallback (Test Connection / in-flight).
/// Returns Err if no auth method is usable.
async fn ssh_exec(
    app_handle: &AppHandle,
    creds: &SshCredentials,
    command: &str,
) -> Result<String, String> {
    let key_path = creds
        .key_path
        .clone()
        .filter(|p| !p.trim().is_empty());

    // Decrypt from secure store if a key was issued and no plaintext was sent.
    let resolved_password: Option<String> = if let Some(plain) = creds
        .password
        .clone()
        .filter(|p| !p.is_empty())
    {
        Some(plain)
    } else if let Some(secure_key) = creds
        .password_secure_key
        .clone()
        .filter(|k| !k.is_empty())
    {
        // The store I/O is sync and small; off-load to a blocking thread so
        // the runtime isn't stalled if the disk is slow.
        let key_for_lookup = secure_key.clone();
        let app_for_lookup = app_handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            secure_keys::secure_get_secret(app_for_lookup, key_for_lookup)
        })
        .await
        .map_err(|e| format!("Failed to join SSH password lookup: {e}"))?
        .ok()
        .flatten()
        .filter(|p| !p.is_empty())
    } else {
        None
    };

    if key_path.is_none() && resolved_password.is_none() {
        return Err("No authentication method configured \u{2014} provide a key path or password".to_string());
    }

    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, (creds.host.as_str(), creds.port), ClientHandler)
        .await
        .map_err(|e| format!("SSH connect to {}:{} failed: {}", creds.host, creds.port, e))?;

    let authed = match (key_path, resolved_password) {
        (Some(key_path), _) => {
            let key_pair = russh::keys::load_secret_key(&key_path, None)
                .map_err(|e| format!("Failed to load SSH key {}: {}", key_path, e))?;
            session
                .authenticate_publickey(&creds.username, Arc::new(key_pair))
                .await
                .map_err(|e| format!("SSH key auth failed: {}", e))?
        }
        (None, Some(password)) => {
            session
                .authenticate_password(&creds.username, password)
                .await
                .map_err(|e| format!("SSH password auth failed: {}", e))?
        }
        (None, None) => unreachable!("checked above"),
    };
    if !authed {
        return Err(format!("SSH auth rejected for {}@{}", creds.username, creds.host));
    }

    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("SSH channel open failed: {}", e))?;
    // want_pty=false prevents spawning visible terminal windows on Windows
    channel
        .exec(false, command)
        .await
        .map_err(|e| format!("SSH exec failed: {}", e))?;

    let mut output = Vec::new();
    while let Some(msg) = channel.wait().await {
        if let russh::ChannelMsg::Data { ref data } = msg {
            output.extend_from_slice(data);
        }
    }
    Ok(String::from_utf8_lossy(&output).to_string())
}

/// Parse `nvidia-smi --query-gpu=... --format=csv,noheader,nounits` output.
/// Expected columns: index, name, utilization.gpu, memory.used, memory.total, temperature.gpu
fn parse_nvidia_smi(out: &str) -> Vec<GpuStat> {
    out.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            if cols.len() < 6 {
                return None;
            }
            Some(GpuStat {
                index: cols[0].parse().unwrap_or(0),
                name: cols[1].to_string(),
                utilization: cols[2].parse().unwrap_or(0),
                memory_used: cols[3].parse().unwrap_or(0),
                memory_total: cols[4].parse().unwrap_or(0),
                temperature: cols[5].parse().unwrap_or(0),
            })
        })
        .collect()
}

const NVIDIA_SMI_CMD: &str = "nvidia-smi --query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu --format=csv,noheader,nounits";

/// PowerShell snippet that returns the actual hardware VRAM (in bytes) for
/// every GPU on a Windows box by reading the WDDM 2.0 driver-published
/// QWORD `HardwareInformation.qwMemorySize` from each adapter's registry
/// tree. Output is a JSON array of uint64 values, sorted descending and
/// deduplicated via `Sort-Object -Unique`. Paired with
/// `parse_registry_vram_bytes` on the Rust side.
///
/// Requires PowerShell 5.1+ (default on Windows 10/11 + Server 2016+); the
/// SSH user must be able to read
/// `HKLM:\SYSTEM\CurrentControlSet\Control\Video` (default for the
/// Authenticated Users group, so JC's standard remote PowerShell sessions
/// work without an `Run as administrator` elevation step).
const REGISTRY_VRAM_PS: &str = r#"powershell -Command "$enumPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Video'; $sizes = @(); if (Test-Path $enumPath) { foreach ($adapterDir in Get-ChildItem $enumPath -ErrorAction SilentlyContinue) { $videoDir = Join-Path $adapterDir.PSPath 'Video'; if (-not (Test-Path $videoDir)) { continue }; foreach ($videoSubKey in Get-ChildItem -Path $videoDir -ErrorAction SilentlyContinue) { $qw = Get-ItemProperty -Path $videoSubKey.PSPath -Name 'HardwareInformation.qwMemorySize' -ErrorAction SilentlyContinue; if ($qw -and $qw.'HardwareInformation.qwMemorySize' -gt 0) { $sizes += [uint64]$qw.'HardwareInformation.qwMemorySize' } } } }; $sizes | Sort-Object -Descending -Unique | ConvertTo-Json -Compress""#;

/// Check whether a node is reachable over SSH. Returns online=true if the SSH
/// session connects + authenticates (runs `true` as a no-op probe).
#[tauri::command]
pub async fn check_node_status(
    app_handle: AppHandle,
    creds: SshCredentials,
) -> Result<NodeStatusResult, String> {
    // Probe with a trivial command; success => online. GPU stats fetched
    // separately via get_gpu_stats so a slow smi doesn't block the dot.
    match ssh_exec(&app_handle, &creds, "true").await {
        Ok(_) => Ok(NodeStatusResult { online: true, gpus: vec![] }),
        Err(e) => Err(e),
    }
}

/// Fetch GPU stats. `vendor` selects the tool: "nvidia" -> nvidia-smi,
/// "amd" -> rocm-smi. Parsed into a uniform GpuStat list.
#[tauri::command]
pub async fn get_gpu_stats(
    app_handle: AppHandle,
    creds: SshCredentials,
    vendor: String,
) -> Result<Vec<GpuStat>, String> {
    match vendor.as_str() {
        "amd" => {
            // rocm-smi JSON output; parsing kept minimal/best-effort.
            let out = ssh_exec(&app_handle, &creds, "rocm-smi --showuse --showmemuse --showtemp --json").await?;
            Ok(parse_rocm_smi(&out))
        }
        _ => {
            let out = ssh_exec(&app_handle, &creds, NVIDIA_SMI_CMD).await?;
            Ok(parse_nvidia_smi(&out))
        }
    }
}

/// Launch the llama.cpp RPC slave on a node. `rpc_command` is configurable in
/// settings (defaults supplied by the frontend). Returns combined stdout.
#[tauri::command]
pub async fn launch_rpc_slave(
    app_handle: AppHandle,
    creds: SshCredentials,
    rpc_command: String,
) -> Result<String, String> {
    if rpc_command.trim().is_empty() {
        return Err("No RPC slave command configured".to_string());
    }
    ssh_exec(&app_handle, &creds, &rpc_command).await
}

/// Run nvidia-smi locally (MAMBA runs Meridian) and parse GPU stats.
fn local_nvidia_gpus() -> Vec<GpuStat> {
    use std::process::Command;
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            parse_nvidia_smi(&String::from_utf8_lossy(&out.stdout))
        }
        _ => vec![],
    }
}

/// Local hardware snapshot (CPU + RAM via sysinfo, GPUs via local nvidia-smi).
/// Used for MAMBA, the machine Meridian runs on — no SSH needed.
#[tauri::command]
pub async fn get_local_hardware() -> Result<HardwareSnapshot, String> {
    use sysinfo::System;
    let mut sys = System::new();
    // Two refreshes spaced briefly so CPU utilization is meaningful.
    sys.refresh_cpu_all();
    std::thread::sleep(std::time::Duration::from_millis(200));
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());
    let cpu_util = sys.global_cpu_usage();
    let cores = sys.cpus().len() as u32;

    let total_kb = sys.total_memory();
    let used_kb = sys.used_memory();
    let total_mb = total_kb / 1024 / 1024;
    let used_mb = used_kb / 1024 / 1024;
    let free_mb = total_mb.saturating_sub(used_mb);
    let ram_util = if total_kb > 0 {
        (used_kb as f32 / total_kb as f32) * 100.0
    } else {
        0.0
    };

    Ok(HardwareSnapshot {
        online: true,
        cpu: Some(CpuInfo { name: cpu_name, cores, utilization: cpu_util }),
        ram: Some(RamInfo { total_mb, used_mb, free_mb, utilization: ram_util }),
        gpus: local_nvidia_gpus(),
        error: None,
    })
}

/// Parse a `KEY=value` line from PowerShell Write-Output blocks.
fn parse_kv(out: &str, key: &str) -> Option<String> {
    out.lines()
        .find(|l| l.trim_start().starts_with(&format!("{}=", key)))
        .and_then(|l| l.splitn(2, '=').nth(1))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Remote hardware snapshot over SSH (used for BLACK). Gathers CPU/RAM via
/// PowerShell CIM on Windows, GPUs via WMI. Auto-detects GPU vendor.
#[tauri::command]
pub async fn get_remote_hardware(
    app_handle: AppHandle,
    creds: SshCredentials,
) -> Result<HardwareSnapshot, String> {
    // Probe connectivity first.
    if let Err(e) = ssh_exec(&app_handle, &creds, "true").await {
        return Ok(HardwareSnapshot { online: false, cpu: None, ram: None, gpus: vec![], error: Some(e) });
    }

    // CPU via CIM (BLACK is Windows; wmic is deprecated, use PowerShell CIM).
    let cpu_out = ssh_exec(
        &app_handle,
        &creds,
        "powershell -Command \"$c=Get-CimInstance Win32_Processor; Write-Output ('NAME='+$c.Name); Write-Output ('CORES='+$c.NumberOfCores); Write-Output ('LOAD='+$c.LoadPercentage)\"",
    )
    .await
    .unwrap_or_default();
    let cpu_name = parse_kv(&cpu_out, "NAME").unwrap_or_else(|| "Unknown CPU".to_string());
    let cores = parse_kv(&cpu_out, "CORES").and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
    let cpu_util = parse_kv(&cpu_out, "LOAD").and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(0.0);

    // RAM via CIM (values in KB).
    let ram_out = ssh_exec(
        &app_handle,
        &creds,
        "powershell -Command \"$o=Get-CimInstance Win32_OperatingSystem; Write-Output ('TOTAL_KB='+$o.TotalVisibleMemorySize); Write-Output ('FREE_KB='+$o.FreePhysicalMemory)\"",
    )
    .await
    .unwrap_or_default();
    let total_mb = parse_kv(&ram_out, "TOTAL_KB").and_then(|s| s.trim().parse::<u64>().ok()).map(|kb| kb / 1024).unwrap_or(0);
    let free_mb = parse_kv(&ram_out, "FREE_KB").and_then(|s| s.trim().parse::<u64>().ok()).map(|kb| kb / 1024).unwrap_or(0);
    let used_mb = total_mb.saturating_sub(free_mb);
    let ram_util = if total_mb > 0 { (used_mb as f32 / total_mb as f32) * 100.0 } else { 0.0 };

    // Auto-detect GPU vendor via WMI (get all non-virtual GPUs with VRAM)
    let gpu_vendor_out = ssh_exec(
        &app_handle,
        &creds,
        "powershell -Command \"Get-CimInstance Win32_VideoController | Where-Object { $_.Name -notmatch 'Parsec|Virtual|Basic|Microsoft' -and $_.AdapterRAM -gt 0 } | Select-Object Name,AdapterRAM | ConvertTo-Json\"",
    )
    .await
    .unwrap_or_default();

    // Fix for Windows GPUs reporting inaccurate VRAM: WMI AdapterRAM is a
    // uint32 capped at ~4 GB by the WMI provider; AMD drivers in
    // particular never expose full VRAM via this path — the user's RX 6900
    // XT (16 GB) shows up as 4 GB here. The authoritative 64-bit value
    // lives in the video-adapter registry at
    //   HKLM\SYSTEM\CurrentControlSet\Control\Video\<adapter-guid>\Video\<mode>\
    //     HardwareInformation.qwMemorySize
    // (REG_BINARY, 8 bytes). Cross-vendor accurate for AMD / NVIDIA / Intel
    // on WDDM 2.0+ hosts — Windows 10 1709 (Sept 2017) onwards. Drivers
    // below WDDM 2.0 fall back to the (broken) WMI value; a future DXGI
    // fallback can close that gap. Driver registration order tracks WMI
    // enumeration order on Windows, so positional pairing assigns the i-th
    // registry size to the i-th WMI adapter. Single-GPU hosts (like JC's
    // BLACK) only have one entry on each side; the loop is trivially
    // correct for that case.
    let mut gpus = parse_wmi_gpu_json(&gpu_vendor_out);
    let gpu_reg_out = ssh_exec(&app_handle, &creds, REGISTRY_VRAM_PS)
        .await
        .unwrap_or_default();
    let reg_vram = parse_registry_vram_bytes(&gpu_reg_out);
    merge_gpus_with_registry(&mut gpus, &reg_vram);

    Ok(HardwareSnapshot {
        online: true,
        cpu: Some(CpuInfo { name: cpu_name, cores, utilization: cpu_util }),
        ram: Some(RamInfo { total_mb, used_mb, free_mb, utilization: ram_util }),
        gpus,
        error: None,
    })
}

/// Parse WMI GPU JSON output (Windows Get-WmiObject for Win32_VideoController).
/// AdapterRAM is in bytes, convert to MB. Returns GPU stats with name and VRAM.
fn parse_wmi_gpu_json(out: &str) -> Vec<GpuStat> {
    let parsed: serde_json::Value = match serde_json::from_str(out) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut gpus = Vec::new();

    // Handle both single object and array
    let items = if let Some(arr) = parsed.as_array() {
        arr.clone()
    } else {
        vec![parsed]
    };

    for (i, item) in items.iter().enumerate() {
        let name = item.get("Name").and_then(|v| v.as_str()).unwrap_or("Unknown GPU").to_string();
        let adapter_ram = item.get("AdapterRAM").and_then(|v| v.as_u64()).unwrap_or(0);
        let memory_total_mb = adapter_ram / 1024 / 1024;

        gpus.push(GpuStat {
            index: i as u32,
            name,
            utilization: 0,
            memory_used: 0,
            memory_total: memory_total_mb,
            temperature: 0,
        });
    }
    gpus
}

/// Replace each GPU's WMI-derived `memory_total` with the matching registry
/// byte count when one is available at the same index. Driver registration
/// order tracks WMI enumeration order on Windows, so positional pairing
/// assigns the i-th registry size to the i-th WMI adapter. Indexes past
/// `reg_vram.len()` keep their WMI values (relevant for virtual / legacy
/// display adapters that don't expose the QWORD). Single-GPU hosts (like
/// JC's BLACK) only have one entry on each side; the loop is trivially
/// correct for that case. Multi-GPU mixed-vendor hosts fall back to
/// undefined-but-deterministic pairing — a future change can add a
/// name-substring match against the registry's `DeviceDesc` to lock down
/// the contract for that case.
fn merge_gpus_with_registry(gpus: &mut [GpuStat], reg_vram: &[u64]) {
    for (i, gpu) in gpus.iter_mut().enumerate() {
        if let Some(&bytes) = reg_vram.get(i) {
            if bytes > 0 {
                gpu.memory_total = bytes / (1024 * 1024);
            }
        }
    }
}

/// Parse the JSON array emitted by the registry-reading PowerShell command
/// in `get_remote_hardware`. Each element is an unsigned 64-bit byte count
/// for one physical GPU's VRAM (read from the driver-published QWORD value
/// `HardwareInformation.qwMemorySize`). Returns sizes sorted descending so
/// the caller can pair them with WMI adapters in driver-registration
/// order. Empty Vec on bad JSON / empty array; nothing else is invalid.
fn parse_registry_vram_bytes(out: &str) -> Vec<u64> {
    let parsed: serde_json::Value = match serde_json::from_str(out) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut sizes: Vec<u64> = arr.iter().filter_map(|v| v.as_u64()).collect();
    // Sort descending so duplicate values are adjacent (Vec::dedup removes
    // adjacent duplicates; O(n) after the sort). The PowerShell script
    // already uses `-Unique` but we defensively re-dedupe here in case a
    // future caller passes raw registry output with sub-key repeats.
    sizes.sort_by(|a, b| b.cmp(a));
    sizes.dedup();
    sizes
}

/// Parse `top -bn1` %Cpu line → utilization percent (100 - idle).
#[allow(dead_code)]
fn parse_top_cpu_util(out: &str) -> Option<f32> {
    // e.g. "%Cpu(s):  3.2 us,  1.1 sy, ...,  94.5 id, ..."
    let idle = out
        .split(',')
        .find(|seg| seg.contains("id"))
        .and_then(|seg| seg.trim().split_whitespace().next())
        .and_then(|n| n.parse::<f32>().ok())?;
    Some((100.0 - idle).clamp(0.0, 100.0))
}

/// Parse `free -m` → (total, used, free) in MB from the Mem: line.
#[allow(dead_code)]
fn parse_free_mb(out: &str) -> Option<(u64, u64, u64)> {
    let line = out.lines().find(|l| l.starts_with("Mem:"))?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return None;
    }
    let total = cols[1].parse::<u64>().ok()?;
    let used = cols[2].parse::<u64>().ok()?;
    let free = cols[3].parse::<u64>().ok()?;
    Some((total, used, free))
}

/// Best-effort rocm-smi JSON parse. AMD's schema varies by version, so this is
/// intentionally lenient and returns an empty list if the shape is unexpected.
/// Refined once tested against the real BLACK node (pending).
fn parse_rocm_smi(out: &str) -> Vec<GpuStat> {
    let parsed: serde_json::Value = match serde_json::from_str(out) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut gpus = Vec::new();
    if let Some(obj) = parsed.as_object() {
        for (key, card) in obj {
            if !key.starts_with("card") {
                continue;
            }
            let idx = key.trim_start_matches("card").parse().unwrap_or(0);
            let util = card.get("GPU use (%)").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0);
            let temp = card.get("Temperature (Sensor edge) (C)").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()).map(|f| f as u32).unwrap_or(0);
            gpus.push(GpuStat {
                index: idx,
                name: "AMD GPU".to_string(),
                utilization: util,
                memory_used: 0,
                memory_total: 0,
                temperature: temp,
            });
        }
    }
    gpus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_smi_rows() {
        let sample = "0, NVIDIA GeForce RTX 3060, 45, 4096, 12288, 62\n1, NVIDIA GeForce RTX 3060, 0, 100, 12288, 40";
        let gpus = parse_nvidia_smi(sample);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[0].utilization, 45);
        assert_eq!(gpus[0].memory_total, 12288);
        assert_eq!(gpus[1].temperature, 40);
    }

    #[test]
    fn nvidia_parser_ignores_malformed() {
        assert!(parse_nvidia_smi("garbage line").is_empty());
    }

    // ----- AMD VRAM registry fix tests -----

    #[test]
    fn parse_registry_vram_returns_sizes_sorted_descending() {
        // AMD RX 6900 XT (16 GB = 17179869184 bytes) on top; legacy 4 GB
        // residual below. Order matters — the largest value always pairs
        // with the largest-card WMI adapter.
        let sample = r#"[17179869184,4294967296]"#;
        let sizes = parse_registry_vram_bytes(sample);
        assert_eq!(sizes, vec![17179869184, 4294967296]);
    }

    #[test]
    fn parse_registry_vram_dedupes_repeated_subkeys() {
        // HardwareInformation.qwMemorySize is the same value across all
        // sub-keys (display modes) of one adapter; the PowerShell
        // collector pushes duplicates that we dedupe via `-Unique`. For
        // safety the Rust parser also handles a hand-crafted duplicate
        // string input should still emit one entry.
        let sample = r#"[17179869184,17179869184,8589934592]"#;
        let sizes = parse_registry_vram_bytes(sample);
        assert_eq!(
            sizes,
            vec![17179869184, 8589934592],
            "duplicates must be removed and the result sorted desc"
        );
    }

    #[test]
    fn parse_registry_vram_returns_empty_for_invalid_json() {
        assert!(parse_registry_vram_bytes("totally not JSON").is_empty());
        assert!(parse_registry_vram_bytes("").is_empty());
        assert!(parse_registry_vram_bytes("{}").is_empty());
    }

    #[test]
    fn wmi_parser_still_works_with_lone_object_no_array_brackets() {
        // PowerShell ConvertTo-Json in single-result mode emits a bare
        // object without array brackets. The parser must accept both
        // shapes so an RDP/headless node with one virtual GPU doesn't
        // drop the adapter.
        let sample = r#"{"Name":"Basic Display Adapter","AdapterRAM":0}"#;
        let gpus = parse_wmi_gpu_json(sample);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "Basic Display Adapter");
    }

    // ----- merge_gpus_with_registry tests -----

    fn make_gpu(idx: u32, name: &str, memory_total_mb: u64) -> GpuStat {
        GpuStat {
            index: idx,
            name: name.to_string(),
            utilization: 0,
            memory_used: 0,
            memory_total: memory_total_mb,
            temperature: 0,
        }
    }

    #[test]
    fn merge_gpus_with_registry_overrides_wmi_vram_for_single_gpu() {
        // JC's BLACK: WMI under-reports a 16384 MB RX 6900 XT as 4096 MB
        // because the WMI AdapterRAM field is uint32-capped. The
        // authoritative 16384 MB value pairs in via positional merge.
        let mut gpus = vec![make_gpu(0, "AMD Radeon RX 6900 XT", 4096)];
        let reg = vec![17179869184u64]; // 16384 MB
        merge_gpus_with_registry(&mut gpus, &reg);
        assert_eq!(
            gpus[0].memory_total, 16384,
            "registry value must override the broken WMI value"
        );
        // Other fields unchanged.
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[0].name, "AMD Radeon RX 6900 XT");
    }

    #[test]
    fn merge_gpus_with_registry_keeps_wmi_when_registry_empty() {
        // Pre-WDDM-2.0 hosts: registry returns an empty Vec. The WMI
        // fallback value (wrong on AMD but the best we have) must be
        // preserved so the UI doesn't render a zero card.
        let mut gpus = vec![make_gpu(0, "AMD Radeon RX 6900 XT", 4096)];
        let empty: Vec<u64> = vec![];
        merge_gpus_with_registry(&mut gpus, &empty);
        assert_eq!(
            gpus[0].memory_total, 4096,
            "empty registry must leave WMI value untouched"
        );
    }

    #[test]
    fn merge_gpus_with_registry_keeps_wmi_for_unmatched_indexes() {
        // Two-GPU host where the registry only reports one adapter (a
        // virtual display adapter that doesn't expose qwMemorySize).
        // Index 0 must pick up the registry value; index 1 must keep WMI.
        let mut gpus = vec![
            make_gpu(0, "AMD Radeon RX 6900 XT", 4096),
            make_gpu(1, "Hyper-V Virtual Display", 0),
        ];
        let reg = vec![17179869184u64]; // only one registry entry
        merge_gpus_with_registry(&mut gpus, &reg);
        assert_eq!(gpus[0].memory_total, 16384, "matched index must override");
        assert_eq!(
            gpus[1].memory_total, 0,
            "unmatched index must keep its WMI value"
        );
    }

    #[test]
    fn merge_gpus_with_registry_ignores_zero_byte_entries() {
        // Defensive: a registry value of 0 must not overwrite a working WMI
        // value (could happen on a virtual adapter).
        let mut gpus = vec![make_gpu(0, "AMD Radeon RX 6900 XT", 4096)];
        let reg = vec![0u64];
        merge_gpus_with_registry(&mut gpus, &reg);
        assert_eq!(
            gpus[0].memory_total, 4096,
            "zero-byte registry entry must not overwrite WMI"
        );
    }
}
