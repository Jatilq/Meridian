# Meridian — Agent Build Instructions

## Project Owner

JC. Hobbyist. Retired. Non-programmer. All development is agent-driven. Does not write code, edit files manually, or run multi-step command sequences.

## Golden Rules

1. Never ask JC to manually edit a file
2. Never ask JC to run more than one command
3. Diagnose before acting — no guess-and-check
4. Never report completion without verifying
5. No destructive file operations without preview and confirmation
6. Do not modify Sigma's existing code unless required for integration
7. Read before writing — always read relevant source files first
8. Never proceed on timeout — wait indefinitely for JC on external actions

---

## Stack

- **Base:** Sigma File Manager (Tauri 2 + Vue 3 + Rust) — NOT Electron
- **AI Engine:** Omnix (separate hidden Electron process — DO NOT CHANGE)
- **Heavy AI:** 9Router OpenAI-compatible proxy → MAMBA/BLACK
- **SSH/SFTP:** russh crate in Rust backend
- **Cluster:** llama.cpp RPC via SSH
- **Database:** SQLite (meridian.db — AI logs, download queue, SSH connections, tool call log)
- **Extensions:** Sigma's existing extension system

---

## Phase 0 — Always Do This First

1. Read CLAUDE.md
2. Read SESSION_HANDOFF.md
3. Read DESIGN.md
4. Scan project tree (exclude node_modules, .git, dist, target)
5. Check `git log origin/main..HEAD` for unpushed commits
6. Identify current task
7. Read only files relevant to current task

---

## COMPLETED PHASES (do not re-implement)

### ✅ Phase 1 — Sigma Foundation
### ✅ Phase 2 — Rain AI Panel (with personality, greeting, scope selector)
### ✅ Phase 3 — Enhanced Downloader (yt-dlp, parallel chunks, browser extension)
### ✅ Phase 4 — Settings Panel
### ✅ Phase 5 — Omnix Integration (Vision, TTS, Director)
### ✅ Phase 6 — Cluster Control Panel (live hardware, SSH, Launch RPC Slave)
### ✅ Phase 7 — SSH/SFTP File Browser (remote panes, drag transfer)
### ✅ Phase 8 — Rain Agent Upgrade (tool calling, memory files, confirmation cards)

---

## CURRENT TASKS (pre-Phase 9)

### Task 1 — Serde Audit
Check all frontend→Rust IPC structs for camelCase/snake_case mismatch.
Pattern: frontend Vue sends camelCase, Rust expects snake_case unless `#[serde(rename_all = "camelCase")]` is present.
Known fixed: SshCredentials in cluster.rs. Check: SftpCredentials, any other structs receiving Vue data.
Fix: add `#[serde(rename_all = "camelCase")]` where missing.
Verify: cargo check clean.
Commit: `fix: serde camelCase audit`

### Task 2 — Default Download Folder
Auto-detect on first run only (don't override saved settings).
Check order: E:\Downloads → C:\Users\jatilq\Downloads → create E:\Downloads if neither exists.
Set as default in downloader settings.
Commit: `feat: auto-detect default download folder`

### Task 3 — Rain First-Run Onboarding
Trigger: no settings saved yet (first launch detection).
Rain message: "Hey, I'm Rain. Looks like this is your first time here — want me to walk you through a few basics?"
4 steps (all skippable):
1. Set download folder (pre-filled with auto-detected)
2. Configure 9Router endpoint (pre-filled: http://localhost:20128/v1)
3. Add SSH connections (MAMBA/BLACK pre-filled with standard values)
4. Done message from Rain
Always visible Skip button. Never blocks the app.
Commit: `feat: Rain first-run onboarding`

---

## Phase 9 — Package & Installer

1. Read Tauri's bundler documentation (check tauri.conf.json existing bundle config)
2. Add Omnix to bundle resources (the full E:\ai\Apps\Omnix directory or built output)
3. Verify yt-dlp is in bundle (src-tauri/binaries/ — already there)
4. Configure Windows installer in tauri.conf.json (productName, identifier, version)
5. Build: `npm run tauri build`
6. Test installer on a clean path
7. Verify Start Menu shortcut created
8. Update README with user setup instructions

**Completion check:** Double-click installer, Meridian installs, launches, Rain greets, 9Router panel shows.

---

## Phase 10 — Hardware Scanner + HuggingFace Model Recommender

1. Build hardware scan command in Rust using sysinfo + nvidia-smi/rocm-smi (reuse Cluster Control code)
2. Add BLACK's hardware via SSH (already wired in cluster.rs)
3. Build HuggingFace API client — search models by:
   - Max VRAM available (local + RPC slave if active)
   - Max RAM available
   - Trusted quantizers: Bartowski, Unsloth, MaziyarPanahi, LoneStriker
   - Min quant: Q4_K_M — filter out IQ1/IQ2/IQ3 and non-standard quants
4. Build Hardware Scanner Vue panel (new sidebar icon or in Cluster Control)
5. Show: detected hardware → recommended models → download button (via yt-dlp or HF API)
6. Download goes through Meridian's existing downloader queue

**Completion check:** Open Hardware Scanner → sees 3× RTX 3060 + RX 6900 XT → recommends models that fit 52GB → click download → appears in downloader queue.

---

## Phase 11 — Backend Manager (NEW — IMPORTANT)

### Vision
Meridian manages the entire local AI inference stack. No command line needed. Install Meridian → download backend → load model → inference ready.

### New Panel: Backend Manager
New sidebar icon. Shows available backends:

| Backend | Variants | Notes |
|---|---|---|
| llama.cpp | CPU, CUDA, ROCm/Vulkan | Primary target |
| llamafile | Universal | Simplest, single exe |
| koboldcpp | CUDA, ROCm | Popular alternative |

### Per-backend UI
- Status: Not installed / Installed / Running / Stopped
- Download button (auto-selects correct variant for detected GPU)
- Version info + Update button
- Start/Stop controls
- Port configuration
- Active model display

### Download logic
1. Read GPU vendor from Cluster Control hardware data (already available)
2. NVIDIA → download CUDA build
3. AMD → download ROCm/Vulkan build  
4. No GPU detected → download CPU build
5. Save to `E:\ai\Apps\backends\<backend-name>\`
6. Verify binary after download (checksum if available)

### RPC Slave Auto-Setup (killer feature)
When user clicks "Setup BLACK as RPC Slave":
1. Check if llama.cpp RPC server binary exists locally
2. If not, download it first
3. SFTP copy the rpc-server binary to BLACK (use existing SFTP infrastructure)
4. SSH to BLACK: start rpc-server: `./rpc-server --host 0.0.0.0 --port 50052`
5. Update Cluster Control to show RPC slave active
6. Combined VRAM updates to 52GB
7. 9Router gains access to expanded pool

### Update/Upgrade Slave
- Download new binary locally
- SFTP to BLACK (replaces old binary)
- SSH restart: kill old rpc-server, start new one
- Show version before/after

### rpc_manager integration
- https://github.com/arseniy0924/rpc_manager is the orchestration tool
- Meridian should be able to launch rpc_manager as a managed process
- rpc_manager handles backend selection for the slave automatically
- Clone rpc_manager to `E:\ai\Apps\rpc_manager\` if not present
- Launch/monitor/stop rpc_manager from Backend Manager panel

### Model Management (basic)
- Scan `E:\ai\Models\` for GGUF files
- List models with size and estimated quant type
- Load model into active backend with one click
- Show currently loaded model per backend
- Unload / swap model

### Integration Points
- Cluster Control: "Launch RPC Slave" button calls Backend Manager's RPC setup
- Hardware Scanner: recommended models link to Backend Manager download
- 9Router: Backend Manager informs which local backends are running + their ports

**Completion check:** Open Backend Manager → Download llama.cpp CUDA → Start it → Load Qwen3.6 35B → Cluster Control shows model loaded → 9Router can route to it.

---

## Technical Rules

### Tauri/Vue Patterns
- All Tauri commands: `#[tauri::command]` in Rust, `invoke()` in Vue
- ALL frontend→Rust structs must have `#[serde(rename_all = "camelCase")]`
- Use existing Vuex/Pinia store patterns
- Match Sigma's existing Vue component structure

### SSH/SFTP
- Use russh crate (already in Cargo.toml)
- Key: `C:\Users\jatilq\.ssh\meridian_black`
- Never store credentials in plaintext
- All destructive remote operations need confirmation

### Rain Agent
- Tool calls via OpenAI function calling spec
- Max 10 iterations per turn
- Read-only tools: immediate execution
- Destructive tools: confirmation card first
- All tool calls logged to rain_tool_log in meridian.db

### Security
- No hardcoded IPs, ports, credentials
- No plaintext credential storage
- GitHub PAT: inline in remote URL only, scrub immediately after push, never echo
- Rain cannot access system folders or Windows directory

---

## Hardware Reference

| Machine | IP | CPU | GPU | VRAM | RAM |
|---|---|---|---|---|---|
| MAMBA | 192.168.1.67 | Xeon E5-2697v4 36c | 3× RTX 3060 | 36GB | 256GB |
| BLACK | 192.168.1.64 | Ryzen 9 5950X 16c | RX 6900 XT | 16GB | 64GB |
| Combined | — | — | — | 52GB | 320GB |

9Router: `http://localhost:20128/v1`
Omnix: `http://localhost:9777`
Models: `E:\ai\Models\`
Apps: `E:\ai\Apps\`
Backends: `E:\ai\Apps\backends\` (Phase 11)
