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

## Phase 11 — Backend Manager

### Goal
Make Meridian self-contained: a single panel where users can **download, install, configure, and launch inference backends** without using the command line. Supported: **llama.cpp (CUDA build for NVIDIA, ROCm build for AMD), llamafile, koboldcpp**. Hardware auto-detection picks the right set. The panel also handles **RPC slave setup** — copying the chosen backend binary to a worker machine over **SFTP** and launching it over **SSH** — and manages models stored under `E:\ai\Models\`. Source of truth for this phase: JC's session summary on 2026-06-28 (the spec was provided in chat because DESIGN.md / SESSION_HANDOFF.md marked Phase 11 as "details TBD"). This document is now updated so future agents can pick up the work without re-asking.

### Steps

1. **Route + sidebar entry.** Add `/backend-manager` to `src/router/routes.ts` with a Lucide icon (suggest `BrainCircuitIcon` from `@lucide/vue`, but pick one that fits Sigma's dark aesthetic) — place it between `cluster` and `settings`. The icon must NOT replace any existing sidebar item.

2. **New Tauri module `src-tauri/src/backend_manager.rs`.** Commands (all `#[tauri::command]`, all frontend-facing structs `#[serde(rename_all = "camelCase")]`):
   - `list_available_backends()` — returns the supported backends annotated by the detected vendor on the local machine (see auto-detection logic below).
   - `detect_local_gpu_vendor()` — returns `"nvidia" | "amd" | "cpu"` per the algorithm in **Auto-detection logic** below (name-string parsing + local `rocm-smi` fallback; does NOT delegate to `cluster::get_local_hardware` because that function only runs `nvidia-smi` locally and would miss AMD).
   - `install_backend(kind: String)` — downloads the release artifact (zip / tarball / single binary) to `resources/backends/<kind>/` using `reqwest` (already in `Cargo.toml`) plus `flate2` / `xz2` / `zip` (already in `Cargo.toml`) for extraction. Stream progress via `app.emit("backend-install-progress", ...)`. Idempotent: re-running on an installed backend updates in place; running on a different version replaces.
   - `list_installed_backends()` — scans `resources/backends/` and returns `[{ kind, version, path, sizeBytes, installedAt }]`.
   - `remove_backend(kind: String)` — deletes the install dir. **CONFIRMATION REQUIRED** in UI (use the Tauri `dialog` plugin). Logged to SQLite (`backend_events` table, action = `"remove"`).
   - `launch_backend(kind: String, model_path: String, params: BackendParams)` — spawns the binary via `process_runner.rs`; returns `{ pid, startedAt }` so the panel can show "running".
   - `stop_backend(pid: u32)` — kills the running process; checks PID matches a tracked child, never kill -9 a foreign PID.
   - `copy_backend_to_worker(ssh_creds: SshCredentials, kind: String, remote_path: String)` — uploads the local binary (or a tarball of it) to `remote_path` via `sftp.rs`. **SFTP credentials mapping:** `sftp.rs::SftpCredentials` requires `key_path: String`, but frontend-facing `cluster.rs::SshCredentials` has `key_path: Option<String>`. Build the `SftpCredentials` from `ssh_creds` by mapping `ssh_creds.key_path.clone().unwrap_or_default()` into `SftpCredentials.key_path`. **Prefer** to return `Err("SFTP requires a key path — configure one in the SSH connection settings")` early when `ssh_creds.key_path` is `None` rather than silently coercing to `""` (an empty key path fails at the SSH layer with a less-informative error). Returns the remote path written. Reuse `SshCredentials` from `cluster.rs` (`#[serde(rename_all = "camelCase")]` already in place).
   - `launch_rpc_slave_remote(ssh_creds: SshCredentials, kind: String, model_path: String, params: BackendParams)` — composes and runs the llama.cpp RPC slave launch command over SSH (`russh` from `cluster.rs::ssh_exec`). Returns combined stdout. This is the **same RPC launch flow that Cluster Control uses**, but driven from Backend Manager — both call sites must produce compatible commands so 9Router sees the expanded pool identically.
   - `scan_models(root: String)` — walks `root` (default `E:\ai\Models\`) with `walkdir`, returns one entry per file: `{ name, path, sizeBytes, modifiedAt, kind: "gguf" | "mlx" | "safetensors" | "other" }` (inferred from extension). One level of recursion only to avoid scanning runaway trees.
   - `delete_model(path: String)` — **CONFIRMATION REQUIRED**; logged to SQLite. Default to Recycle Bin via `trash` crate (already in `Cargo.toml`).

3. **Register commands.** Add every new command to `src-tauri/src/lib.rs::invoke_handler!`. Use `mod backend_manager;` at the top alongside the other `mod` declarations.

4. **New Vue module `src/modules/backend-manager/`.** Mirror the layout of `src/modules/hardware/` (index.ts + pages/) so future agents can find it. Three pages/tabs inside one panel:
   - **Backends** — list of available (filtered by detected vendor) + installed, with Install / Launch / Stop / Remove buttons. Inline progress bar during install (driven by the emitted progress event).
   - **Workers** — pick an SSH connection from the cluster settings (read encrypted creds from the same `user-settings` / Tauri `store` that Cluster Control reads), push a backend binary to it, launch the RPC slave. Status reflects `cluster.rs::check_node_status`.
   - **Models** — browse `E:\ai\Models\`, search, delete, "Send to Backend Manager" (`launch_backend` with this model pre-filled), or "Send to Downloader" (reuse `downloader::downloader_enqueue`).

5. **Reuse Phase 10 Hardware Scanner** for the GPU/vendor detection card so Backend Manager doesn't re-implement detection logic. The Hardware Scanner panel already calls `cluster::get_local_hardware` — Backend Manager should read the same data via a Tauri `get_local_hardware` wrapper that returns the vendor string only.

6. **SSH credentials.** Reuse `SshCredentials` from `cluster.rs` (already camelCase + already accepts `host`/`port`/`username`/`keyPath`). NEVER hardcode IPs / usernames / keys. NEVER log credentials. Encrypt via Tauri `safeStorage` (or the existing Tauri `store` plugin — match whichever Cluster Control uses today).

7. **Settings.** Add a **Backend Manager** subsection to `src/modules/settings/pages/settings.vue` (alongside AI / Cluster / SSH) listing installed backends, install paths, model root, and a bootstrap toggle (default ON). Provider list does not include hardcoded download URLs — fetch from a single `BackendCatalog` (a JSON shipped under `resources/backend_catalog.json` and bundled at build time via `tauri.conf.json::bundle.resources`). Update the catalog by editing the JSON, no RPM.

8. **Safety.** Every backend install + every backend launch + every copy-to-worker + every model delete must:
   - Use the Tauri `dialog` confirm plugin with the exact operation shown (which backend, which version, where it goes, what will be deleted).
   - Show a destructive-operation warning if the target is non-empty (mirror the file-browser conflict dialog).
   - Append to a SQLite `backend_events` table (timestamp, kind, action, args, outcome).

9. **Process tracking + cleanup.** Maintain a `BACKEND_CHILDREN: Mutex<HashMap<u32, Child>>` parallel to `OMNIX_CHILD` in `omnix.rs`. `launch_backend` inserts, `stop_backend` removes.

   **Reap function shape — mirror `omnix::kill_omnix`.** The existing `omnix::kill_omnix` locks the global Mutex, calls `.take()` on the inner option (or, for backend manager, `.drain()` over the HashMap), then `child.kill()` + `child.wait()` on each child. Use the same structure for `backend_manager::reap_backends`:

   ```rust
   pub fn reap_backends() -> Result<(), String> {
       let mut guard = BACKEND_CHILDREN.lock().map_err(|e| format!("Mutex error: {}", e))?;
       for (pid, mut child) in guard.drain() {
           let _ = child.kill();
           let _ = child.wait();
           log::info!("Reaped backend pid={}", pid);
       }
       Ok(())
   }
   ```

   **Wire into `main.rs`** — extend the existing `WindowEvent::Destroyed` block (the block that already calls `lan_share::stop_lan_share`) to also call `backend_manager::reap_backends().ok()` next to it. **Important note:** `main.rs` does NOT currently call `omnix::kill_omnix` on shutdown — that call-site pattern does not exist yet — so the existing precedent for cleanup-on-main-window-destroy is `lan_share::stop_lan_share`. The reap function SHAPE mirrors `omnix::kill_omnix`, but the call-site WIRING mirrors `lan_share::stop_lan_share`. Add `mod backend_manager;` (if not already present) and the `reap_backends` helper inside `backend_manager.rs`.

### Auto-detection logic
- **Primary path — parse vendor from GPU name strings.** Call `cluster::get_local_hardware` (or re-run its nvidia-smi + WMI probes directly inside the wrapper) to collect GPU names. For each GPU, tag the vendor: name contains `"NVIDIA"` → `"nvidia"`; name contains `"AMD"` or `"Radeon"` → `"amd"`. Aggregate across all reported GPUs: any NVIDIA → overall `"nvidia"`; else any AMD/Radeon → overall `"amd"`. This works on Windows where WMI lists both vendors on the same machine, and on Linux where the GPU name string is the only reliable signal (mixed-vendor boxes, AMD cards leaking into nvidia-smi name lists, etc.).
- **Fallback — local `rocm-smi --json`.** If name-based parsing did not return `"amd"`, execute `rocm-smi --json` locally; if it parses non-empty → `"amd"`. **Important:** `cluster::get_local_hardware` only runs `nvidia-smi` locally — its `rocm-smi` branch is SSH-only for remote nodes. Run the local `rocm-smi` probe directly inside `backend_manager::detect_local_gpu_vendor`.
- If neither path finds any GPU → `"cpu"`.
- NVIDIA → offer `llama.cpp` CUDA only.
- AMD → offer `llama.cpp` ROCm only.
- CPU → offer `llamafile` + `koboldcpp`.
- Always offer a "manual override" dropdown so a user with an unusual GPU can still try a backend.

### Model management
- Default model root: `E:\ai\Models\`. Configurable in Backend Manager settings.
- Scan root + one level of subdirs (no runaway).
- Each entry: name, size, modified time, guessed kind from extension, action buttons (open folder, delete-with-confirm, "Launch in backend", "Copy to worker via SFTP").
- "Add model" → either opens a URL input that enqueues into the existing downloader (`downloader::downloader_enqueue`) or, for HF model pages, parses a HF download URL and enqueues the GGUF file directly.

### Bundle catalog (new file)
- `resources/backend_catalog.json` shipped via `tauri.conf.json::bundle.resources`. Shape:
  ```json
  [
    { "kind": "llama.cpp-cuda",  "displayName": "llama.cpp (CUDA)", "version": "b1234", "url": "https://…", "format": "zip", "binary": "llama-server.exe" },
    { "kind": "llama.cpp-rocm",  "displayName": "llama.cpp (ROCm)", "version": "b1234", "url": "https://…", "format": "tar.gz", "binary": "llama-server" },
    { "kind": "llamafile",       "displayName": "llamafile",        "version": "0.9.x", "url": "https://…", "format": "binary", "binary": "llamafile" },
    { "kind": "koboldcpp",       "displayName": "koboldcpp",        "version": "1.x",   "url": "https://…", "format": "zip", "binary": "koboldcpp.exe" }
  ]
  ```
- Backend Manager reads this file at startup and shows the user what's available. The catalog is bundled with the installer (`tauri:build`); editing it = new release.

### Completion check
1. Open Backend Manager with no GPU detected → llamafile + koboldcpp available; NVIDIA-only entries hidden.
2. Click Install on llamafile → progress emitted and visible → binary present in `resources/backends/llamafile/`.
3. Pick a GGUF model from the Models tab → click Launch → process starts, PID returned, panel shows "running".
4. Click Stop → process exits; BACKEND_CHILDREN entry removed.
5. Workers tab → select MAMBA → Push llama.cpp + Launch RPC → SFTP copy succeeds, SSH exec starts the slave, Cluster Control's combined VRAM goes to 52GB.
6. Models tab → delete a non-essential model → confirmation dialog → file goes to Recycle Bin → backend_events row appended.
7. Restart Meridian → Backend Manager remembers installed backends (state persisted via Tauri `store`).
8. Disable backend manager in settings → panel route still loads but the Tauri commands return early `disabled` or empty lists; no auto-spawn.

### Hard rules recap (don't violate)
- NEVER hardcode IPs / paths / model names (catalog files + settings are the only sources).
- NEVER log SSH credentials or backend binary contents.
- NEVER execute `kill -9` / `TerminateProcess` on a PID the app didn't fork.
- Destructive ops ALWAYS show a confirmation dialog with the exact target.
- Reuse existing Tauri modules (`cluster.rs`, `omnix.rs`, `downloader.rs`, `sftp.rs`, `process_runner.rs`, `trash` crate) instead of reinventing.

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
