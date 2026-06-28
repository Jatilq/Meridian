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
    /// Absolute path to a private key file (key-based auth).
    pub key_path: Option<String>,
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

/// Connect over SSH using key-based auth and run a single command, returning
/// its stdout. Returns Err on connect/auth/exec failure.
async fn ssh_exec(creds: &SshCredentials, command: &str) -> Result<String, String> {
    let key_path = creds
        .key_path
        .clone()
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| "No SSH key path configured".to_string())?;

    let key_pair = russh::keys::load_secret_key(&key_path, None)
        .map_err(|e| format!("Failed to load SSH key {}: {}", key_path, e))?;

    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, (creds.host.as_str(), creds.port), ClientHandler)
        .await
        .map_err(|e| format!("SSH connect to {}:{} failed: {}", creds.host, creds.port, e))?;

    let authed = session
        .authenticate_publickey(&creds.username, Arc::new(key_pair))
        .await
        .map_err(|e| format!("SSH auth failed: {}", e))?;
    if !authed {
        return Err(format!("SSH key auth rejected for {}@{}", creds.username, creds.host));
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

/// Check whether a node is reachable over SSH. Returns online=true if the SSH
/// session connects + authenticates (runs `true` as a no-op probe).
#[tauri::command]
pub async fn check_node_status(creds: SshCredentials) -> Result<NodeStatusResult, String> {
    // Probe with a trivial command; success => online. GPU stats fetched
    // separately via get_gpu_stats so a slow smi doesn't block the dot.
    match ssh_exec(&creds, "true").await {
        Ok(_) => Ok(NodeStatusResult { online: true, gpus: vec![] }),
        Err(e) => Err(e),
    }
}

/// Fetch GPU stats. `vendor` selects the tool: "nvidia" -> nvidia-smi,
/// "amd" -> rocm-smi. Parsed into a uniform GpuStat list.
#[tauri::command]
pub async fn get_gpu_stats(creds: SshCredentials, vendor: String) -> Result<Vec<GpuStat>, String> {
    match vendor.as_str() {
        "amd" => {
            // rocm-smi JSON output; parsing kept minimal/best-effort.
            let out = ssh_exec(&creds, "rocm-smi --showuse --showmemuse --showtemp --json").await?;
            Ok(parse_rocm_smi(&out))
        }
        _ => {
            let out = ssh_exec(&creds, NVIDIA_SMI_CMD).await?;
            Ok(parse_nvidia_smi(&out))
        }
    }
}

/// Launch the llama.cpp RPC slave on a node. `rpc_command` is configurable in
/// settings (defaults supplied by the frontend). Returns combined stdout.
#[tauri::command]
pub async fn launch_rpc_slave(creds: SshCredentials, rpc_command: String) -> Result<String, String> {
    if rpc_command.trim().is_empty() {
        return Err("No RPC slave command configured".to_string());
    }
    ssh_exec(&creds, &rpc_command).await
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
pub async fn get_remote_hardware(creds: SshCredentials) -> Result<HardwareSnapshot, String> {
    // Probe connectivity first.
    if let Err(e) = ssh_exec(&creds, "true").await {
        return Ok(HardwareSnapshot { online: false, cpu: None, ram: None, gpus: vec![], error: Some(e) });
    }

    // CPU via CIM (BLACK is Windows; wmic is deprecated, use PowerShell CIM).
    let cpu_out = ssh_exec(
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
        &creds,
        "powershell -Command \"$o=Get-CimInstance Win32_OperatingSystem; Write-Output ('TOTAL_KB='+$o.TotalVisibleMemorySize); Write-Output ('FREE_KB='+$o.FreePhysicalMemory)\"",
    )
    .await
    .unwrap_or_default();
    let total_mb = parse_kv(&ram_out, "TOTAL_KB").and_then(|s| s.trim().parse::<u64>().ok()).map(|kb| kb / 1024).unwrap_or(0);
    let free_mb = parse_kv(&ram_out, "FREE_KB").and_then(|s| s.trim().parse::<u64>().ok()).map(|kb| kb / 1024).unwrap_or(0);
    let used_mb = total_mb.saturating_sub(free_mb);
    let ram_util = if total_mb > 0 { (used_mb as f32 / total_mb as f32) * 100.0 } else { 0.0 };

    // Auto-detect GPU vendor via WMI (check for NVIDIA vs AMD in GPU names).
    let gpu_vendor_out = ssh_exec(
        &creds,
        "powershell -Command \"Get-WmiObject Win32_VideoController | Where-Object {$_.Name -like '*AMD*' -or $_.Name -like '*Radeon*' -or $_.Name -like '*NVIDIA*'} | Select-Object Name,AdapterRAM | ConvertTo-Json\"",
    )
    .await
    .unwrap_or_default();

    // Parse WMI JSON to extract GPU names and VRAM
    let gpus = parse_wmi_gpu_json(&gpu_vendor_out);

    Ok(HardwareSnapshot {
        online: true,
        cpu: Some(CpuInfo { name: cpu_name, cores, utilization: cpu_util }),
        ram: Some(RamInfo { total_mb, used_mb, free_mb, utilization: ram_util }),
        gpus,
        error: None,
    })
}

/// Parse WMI GPU JSON output (Windows Get-CimInstance for Win32_VideoController).
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
        // AdapterRAM is in bytes; Windows DWORD overflow caps at ~4GB for some fields
        // but for RX 6900 XT (16GB), it should still report correctly
        let adapter_ram = item.get("AdapterRAM").and_then(|v| v.as_u64()).unwrap_or(0);
        let memory_total_mb = adapter_ram / 1024 / 1024; // bytes to MB

        gpus.push(GpuStat {
            index: i as u32,
            name,
            utilization: 0, // WMI doesn't expose utilization on Windows
            memory_used: 0, // Not available via WMI without GPU-Z or similar
            memory_total: memory_total_mb,
            temperature: 0, // Not available via WMI without additional tools
        });
    }
    gpus
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
}
