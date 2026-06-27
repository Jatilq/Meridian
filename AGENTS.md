# Meridian — Agent Build Instructions

## Project Owner

JC. Hobbyist. Non-programmer. All development is agent-driven. Does not write code, edit files manually, or run multi-step command sequences. Agents do everything.

## Golden Rules

1. Never ask JC to manually edit a file
2. Never ask JC to run more than one command
3. Diagnose before acting — no guess-and-check
4. Never report completion without verifying
5. No destructive file operations without preview and confirmation
6. Do not modify Sigma's existing code unless required for integration
7. Read before writing — always read relevant source files first

---

## Stack

- **Base:** Sigma File Manager (Electron + Vue 3)
- **AI Engine:** Omnix (embedded as hidden BrowserWindow in Meridian's Electron process)
- **Heavy AI:** 9Router OpenAI-compatible proxy → MAMBA/BLACK
- **SSH/SFTP:** Node `ssh2` library
- **Cluster:** llama.cpp RPC slave via SSH
- **Database:** SQLite (meridian.db — AI logs, download queue, SSH connections)
- **Extensions:** Sigma's existing extension system

---

## Phase 0 — Always Do This First

1. Read CLAUDE.md
2. Read DESIGN.md
3. Scan project tree (exclude node_modules, .git, dist, target):
   `dir E:\ai\Projects\Meridian\ /s /b | findstr /v "node_modules" | findstr /v ".git" | findstr /v "\dist\" | findstr /v "\target\"`
4. Identify current phase
5. Read only files relevant to current task

---

## Phase 5 — Omnix Embedded (PRIORITY — replaces previous spawn approach)

### Architecture
Omnix runs INSIDE Meridian's Electron process. Not spawned separately.

### Steps
1. Read Meridian's Electron main process entry point (check package.json main field)
2. Read `E:\ai\Apps\Omnix\electron\main.js` completely
3. Read `E:\ai\Apps\Omnix\server.ts` completely
4. Read `E:\ai\Apps\Omnix\src\` or equivalent compute worker entry point
5. In Meridian's main process:
   - Import and start Omnix's Express server (port 9777)
   - Create hidden BrowserWindow that loads Omnix's compute worker HTML
   - BrowserWindow: `show: false`, `webPreferences: { offscreen: false }` (must be non-offscreen for WebGPU)
6. Remove or disable old omnix.rs spawn/kill commands (keep get_omnix_status — still useful)
7. Add startup health check: poll `/api/health` until green before marking Omnix ready
8. Test: launch Meridian, confirm `/api/health` → 200, send test prompt to `/api/text`, confirm inference response
9. Test vision: select an image file in Meridian, send query, confirm `/api/vision` response

**Completion check:** Meridian launches, Omnix health shows green, text and vision queries return real responses.

---

## Phase 6 — Cluster Control Panel

### Steps
1. Add cluster control icon to Sigma's left sidebar (below extensions icon)
2. Build ClusterControl Vue component (see DESIGN.md layout)
3. Add SSH client in Electron main process using `ssh2` npm package
4. Tauri → Electron IPC: commands for `check_node_status`, `launch_rpc_slave`, `get_gpu_stats`
5. `check_node_status`: SSH to MAMBA/BLACK, run `nvidia-smi` or `rocm-smi`, parse output
6. `launch_rpc_slave`: SSH to BLACK, run llama.cpp RPC slave command (configurable in settings)
7. `get_gpu_stats`: parse GPU utilization and VRAM usage from smi output
8. Poll node status every 30 seconds, update Vue store
9. 9Router endpoint status: GET configured endpoint `/v1/models`, show model list
10. Add Cluster section to Meridian settings (IPs, SSH credentials, RPC command)

**Completion check:** Panel shows MAMBA and BLACK status, Launch Slave button SSHs to BLACK and starts RPC, combined VRAM shows 52GB.

---

## Phase 7 — SSH / SFTP File Browser

### Steps
1. Read how Sigma's file pane loads directory listings (the list_directory Rust command or equivalent)
2. Add SSH connection manager in Electron main (ssh2 SFTP subsystem)
3. Add SSH bookmarks to bookmarks sidebar: MAMBA and BLACK pre-configured
4. When SSH bookmark clicked: open SFTP session, list remote directory, populate pane
5. Remote pane: same Vue component as local pane, different data source
6. File operations over SFTP: copy, move, rename, delete, new folder
7. Drag between local and remote panes: upload/download via SFTP
8. Breadcrumb shows `ssh://hostname/path` for remote panes
9. Add SSH connections section to Meridian settings

**Completion check:** Click MAMBA bookmark, remote filesystem loads in pane, can copy file from MAMBA to local.

---

## Phase 8 — Agent Coding Extension

### Steps
1. Read Sigma's extension system — understand how extensions are structured and loaded
2. Create `extensions/agent-coder/` following Sigma's extension format
3. Build AgentCoder panel component (see DESIGN.md layout)
4. Wire to active pane's selected file (local or remote)
5. For remote files: read/write via Phase 7 SSH/SFTP
6. AI calls: route to 9Router (not Omnix) for coding tasks — needs Qwen3.6+
7. Show diff before applying any file changes
8. Confirmation required before writes
9. Register extension in Meridian's extension loader

**Completion check:** Select a local file, open Agent Coder, ask it to add a comment, see diff, confirm, file is updated.

---

## Phase 9 — Package & Installer

### Steps
1. Verify all phases complete and working
2. Bundle Omnix files into Electron's `resources/` directory
3. Bundle yt-dlp binary into resources
4. Configure electron-builder (check existing build config)
5. Build Windows installer: `npm run build` or `npm run electron:build` (check package.json)
6. Test installer on clean path
7. Create Start Menu shortcut (included in installer)
8. Write final README with any user setup steps

**Completion check:** Installer runs, Meridian installs to Program Files, launches with all features working, Omnix embedded and functional.

---

## Technical Rules

### Electron Patterns
- Match Sigma's existing IPC patterns exactly
- Use existing Vuex/Pinia store — add modules, don't replace
- New npm packages: check if `ssh2` already present; if not, install it
- Hidden BrowserWindow for Omnix: must NOT be offscreen (WebGPU requires real GPU context)

### SSH/SFTP
- Use `ssh2` npm package in main process only — not renderer
- All SFTP operations via IPC
- Store SSH credentials encrypted (use Electron's safeStorage API)
- Never log credentials

### Omnix Integration
- Omnix Express server must start before main window is shown OR start concurrently and handle "not ready" state gracefully
- Hidden BrowserWindow must stay alive as long as Meridian is open
- If Omnix compute worker crashes, attempt restart once before marking offline

### Agent Coding Extension
- Coding tasks always use 9Router, never Omnix
- Always show diff before writing files
- Never write to files without confirmation

---

## Hardware Reference

| Machine | IP | GPU | VRAM | Role |
|---|---|---|---|---|
| MAMBA | 192.168.1.67 | 3× RTX 3060 | 36GB | Primary inference, headless |
| BLACK | 192.168.1.64 | RX 6900 XT | 16GB | Daily driver, RPC slave |
| Combined | — | — | 52GB | Large model inference |
| 9Router | MAMBA:PORT | — | — | OpenAI proxy |
| Omnix | embedded | WebGPU | — | Lightweight local AI |

## SSH Access

- MAMBA username: `jatilq`
- BLACK username: `jatilq`
- Credentials stored in Meridian settings (encrypted)
