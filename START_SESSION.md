# Meridian — Session Starter
## Paste this at the start of EVERY session with ANY agent (Hermes, Kilo, OpenCode, Claude Code)

---

Read these files in order before doing anything:
1. `E:\ai\Projects\Meridian\CLAUDE.md`
2. `E:\ai\Projects\Meridian\AGENTS.md`
3. `E:\ai\Projects\Meridian\DESIGN.md`
4. `E:\ai\Projects\Meridian\SESSION_HANDOFF.md`

Then scan the project tree:
`dir E:\ai\Projects\Meridian\ /s /b | findstr /v "node_modules" | findstr /v ".git" | findstr /v "\dist\" | findstr /v "\target\"`

Give me a one paragraph status: what phase we are on, what is done, what the next step is. Wait for my confirmation before doing anything.

---

## CRITICAL FACTS — Never Forget These

**Stack:** Tauri 2 + Vue 3 + Rust. NOT Electron. NOT plain Node.
**DO NOT change the Omnix architecture.** Separate Electron process is intentional.
**Phases 1-8 are COMPLETE.** Do not re-implement anything from these phases.
**JC manages async** — he checks in when ready, not watching terminal. Never proceed on timeout for external actions.
**Rain is the AI assistant** — gender neutral, never refers to itself as AI, warm personality.
**9Router endpoint:** `http://localhost:20128/v1`
**SSH key for BLACK:** `C:\Users\jatilq\.ssh\meridian_black`
**Project path:** `E:\ai\Projects\Meridian\`
**GitHub:** `https://github.com/Jatilq/Meridian` (may have unpushed commits — check)

---

## Completed Phases

### ✅ Phase 1 — Sigma Foundation
Sigma File Manager forked, running on Tauri + Vue. All drives, WSL, thumbnails, tabs working.

### ✅ Phase 2 — Rain AI Panel
- Rain: gender neutral AI assistant, warm personality, never breaks character
- Greeting: "Hey, it's Rain. Where do you want to start?"
- 9Router connected at localhost:20128/v1
- Default model: openrouter/openrouter/free
- Scope selector: Current folder / All drives / per-drive
- System prompt injects current_path, selected_files, file_list, SOUL.md, MEMORY.md, FAVORITES.md

### ✅ Phase 3 — Enhanced Downloader
- yt-dlp auto-routing (probes URL, routes correctly)
- Parallel chunk downloading for direct URLs
- Background spawn, 500ms polling for progress
- Queue with pause/resume/cancel, SQLite persistence
- Browser extension receiver on port 7771
- yt-dlp bundled in src-tauri/binaries/
- Right-click paste fixed in input fields
- Default download folder: E:\Downloads (auto-detected)

### ✅ Phase 4 — Settings
- Meridian section: Primary AI (9Router), Local AI (Omnix), Downloader, SSH Connections
- Temperature, max tokens, top-p, system prompt, context window (read-only)
- Tool-capable model warning when model doesn't support function calling

### ✅ Phase 5 — Omnix Integration
- Omnix spawns as hidden Electron process on startup
- Port: 9777. Working: Vision (FastVLM), TTS (Kokoro), Director (Qwen 0.6B)
- Three-state status dot: grey/yellow/green
- Text inference routes to 9Router NOT Omnix

### ✅ Phase 6 — Cluster Control Panel
- Live hardware dashboard: MAMBA (3× RTX 3060) + BLACK (RX 6900 XT)
- MAMBA: Intel Xeon E5-2697v4, 36 cores, 255.9GB RAM
- BLACK: AMD Ryzen 9 5950X, 16 cores, 63.9GB RAM
- SSH key fixed (serde camelCase bug resolved)
- Launch RPC Slave button (needs llama.cpp RPC binary on BLACK — see Phase 11)
- Combined VRAM display, 30s polling

### ✅ Phase 7 — SSH/SFTP File Browser
- MAMBA and BLACK as bookmarks in sidebar
- Remote panes alongside local panes
- SFTP ops: list, mkdir, rename, delete, drag transfer
- ssh:// breadcrumb navigation
- SSH connections settings UI
- Note: SFTP routing still reads hardcoded list in ssh-connections.ts (minor follow-up)

### ✅ Phase 8 — Rain Agent Upgrade
- Tool calling via OpenAI function calling spec (9Router)
- 7 tools: list_directory, read_file, search_files, create_folder, move_files, rename_item, delete_item
- Confirmation cards for destructive tools (move, rename, delete)
- SOUL.md / MEMORY.md / FAVORITES.md in app data — injected into every prompt
- Rain appends to MEMORY.md autonomously after each turn
- SQLite rain_tool_log table for all tool calls
- Max 10 iterations per agent loop
- Rain onboarding first-run flow (in progress — see current task)

---

## Current Phase: Working on pre-Phase 9 tasks

### Tasks in progress (do these before Phase 9):
1. **Serde audit** — check all frontend→Rust structs for camelCase/snake_case mismatch (same bug as BLACK cluster SSH key). Check SftpCredentials and any other IPC structs.
2. **Default download folder** — auto-detect E:\Downloads or C:\Users\jatilq\Downloads on first run
3. **Rain onboarding** — first-run welcome flow. Rain says "Hey, I'm Rain. Looks like this is your first time here — want me to walk you through a few basics?" Offers: set download folder, configure 9Router, add SSH connections. Skippable.

### After those 3 tasks:

## Phase 9 — Package & Installer
- Bundle Omnix files into resources/
- Bundle yt-dlp binary (already in src-tauri/binaries/)
- Tauri bundler for Windows .msi installer
- Start Menu shortcut
- Final README with setup steps

## Phase 10 — Hardware Scanner + HuggingFace Model Recommender
- Detect all local GPUs, VRAM, CPU, RAM (already partially built in Cluster Control)
- Via SSH: add BLACK's specs
- Query HuggingFace API for models fitting the hardware
- Filter: fits VRAM / fits RAM / trusted quantizers (Bartowski, Unsloth, MaziyarPanahi, LoneStriker)
- Min quant: Q4_K_M — never recommend IQ1-IQ3
- Min quant policy baked in: Q4_K_M, Q5_K_M, or Q8_0 only

## Phase 11 — Backend Manager (NEW — READ CAREFULLY)

### Vision
Meridian manages the entire local AI inference stack — not just the file manager. Users can download, configure, and launch inference backends directly from Meridian. No manual setup required.

### What to build:

**Backend Manager Panel (new sidebar icon):**
- Lists available backends with status (installed/not installed/running/stopped):
  - llama.cpp (variants: CPU, CUDA, ROCm/Vulkan)
  - llamafile (single executable, simplest option)
  - koboldcpp
- Download button per backend — fetches correct binary for detected hardware:
  - NVIDIA GPU detected → download CUDA build
  - AMD GPU detected → download ROCm/Vulkan build
  - No GPU → download CPU build
- Store binaries in `E:\ai\Apps\backends\`
- Start/Stop controls per backend
- Port configuration per backend

**RPC Manager integration:**
- rpc_manager (github.com/arseniy0924/rpc_manager) is the orchestration tool
- Meridian should be able to launch and control rpc_manager
- When "Launch RPC Slave on BLACK" is clicked:
  1. Meridian copies the correct llama.cpp RPC server binary to BLACK via SFTP (Meridian already has SFTP!)
  2. SSH to BLACK and starts the RPC server: `rpc-server --host 0.0.0.0 --port 50052`
  3. MAMBA's llama-server connects to BLACK's RPC server
  4. Combined 52GB pool becomes available to 9Router
- Update/upgrade slave runtime: Meridian downloads new binary, SFTPs it to BLACK, restarts service

**Model loading:**
- List models from E:\ai\Models\ (existing model folder)
- Load a model into the active backend with one click
- Show which model is currently loaded per backend
- Unload / swap model controls

**Integration with Cluster Control:**
- When RPC slave is active and model is loaded, Combined VRAM updates to 52GB
- 9Router endpoint auto-detects the expanded pool

### Why this matters:
This makes Meridian completely self-contained. No manual llama.cpp setup, no command line, no external tools. Install Meridian → download a backend → load a model → 52GB inference pool ready. Nobody else has built this.

---

## Hardware Reference

| Machine | IP | GPU | VRAM | Role |
|---|---|---|---|---|
| MAMBA | 192.168.1.67 | 3× RTX 3060 | 36GB | Primary inference, headless |
| BLACK | 192.168.1.64 | RX 6900 XT | 16GB | Daily driver, RPC slave |
| Combined | — | — | 52GB | Large model inference |
| 9Router | localhost:20128 | — | — | OpenAI proxy |
| Omnix | localhost:9777 | WebGPU | — | Vision/TTS/Director |

SSH key: `C:\Users\jatilq\.ssh\meridian_black`
Username: `jatilq` on both machines
Models: `E:\ai\Models\`
Apps: `E:\ai\Apps\`
Backends: `E:\ai\Apps\backends\` (to be created by Backend Manager)

---

## Known Issues / Follow-ups
- SFTP routing reads hardcoded ssh-connections.ts, not settings
- openrouter/openrouter/free may not support tool calling
- STT (Whisper via Omnix) shelved
- BLACK RX 6900 XT shows 0.0/0.0GB VRAM in cluster panel when RPC slave not active — expected behavior
- Downloader test button added for debugging

---

## Git Status
Remote: https://github.com/Jatilq/Meridian
May have unpushed commits — check with `git status` and `git log origin/main..HEAD`
To push: need fresh PAT from https://github.com/settings/tokens (repo scope, classic)
Push command: `git remote set-url meridian https://<TOKEN>@github.com/Jatilq/Meridian.git && git push meridian main && git remote set-url meridian https://github.com/Jatilq/Meridian.git`
Never echo the token. Scrub remote URL immediately after push.
