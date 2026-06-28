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
11. Model launch configuration (Option 2 from AI panel work): the Cluster Control
    panel is where llama.cpp model-LOAD parameters belong, because they take
    effect when the model is launched/reloaded — NOT per-request (Meridian is an
    OpenAI API client and cannot set these per-query). When launching/reloading a
    model from Cluster Control, pass these to llama.cpp via the RPC/launch command:
    - **GPU split** across MAMBA's 3 GPUs → `--tensor-split` / `--n-gpu-layers`
    - **Context size** → `--ctx-size`
    - **KV cache type** → `--cache-type-k` / `--cache-type-v`
    Expose them as fields in the Cluster model-launch config, substituted into the
    configurable launch command string. (Per-request params — system prompt,
    temperature, max tokens, top-p — already live in AI panel settings.)

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

## Phase 8 — Rain Agent Upgrade (tools + memory)

### Goal
Upgrade the EXISTING Rain AI panel (Phase 5: `src/modules/ai-panel/ai-panel.vue` +
`src/stores/runtime/ai-panel.ts`) from a chat assistant into an **agent with tool
calling and persistent memory**. This is NOT a new extension — it extends the Rain
that already exists. Build on the current handleSend pipeline, system prompt, and
9Router/Omnix routing.

### Personality + memory files
Three markdown files in the user's app data directory (same dir as `meridian.db`;
resolve via Tauri `appDataDir()`):
- **SOUL.md** — fixed personality/identity. User may edit; Rain NEVER auto-modifies it.
- **MEMORY.md** — mutable long-term memory. Rain APPENDS autonomously (no confirmation)
  when it learns something useful. Rain must NEVER delete/rewrite entries without
  explicit user confirmation.
- **FAVORITES.md** — paths/models/preferences Rain notices used repeatedly. Auto-updated.

On startup: seed any missing file from a bundled default (SOUL.md = Rain's base
personality; MEMORY.md / FAVORITES.md start with a header only). All three are injected
into Rain's system prompt context at request time.

### Tools (OpenAI-style function calling via 9Router)
Transport: OpenAI `tools`/`tool_calls` in the 9Router chat-completion request. Agent
mode REQUIRES a tool-call-capable model (e.g. Qwen3.6+). Settings must flag clearly
when the selected model does not support tool calls. Each tool is a Tauri command
(reuse dir_reader/sftp/selection commands); tools work on local AND ssh:// paths.
1. `list_directory(path)` — read directory listing (read-only, immediate)
2. `read_file(path)` — read file contents (read-only, immediate)
3. `search_files(query, scope)` — search across scope (current/all/specific drive) (immediate)
4. `create_folder(path)` — create directory (non-destructive, immediate, NO confirmation)
5. `move_files(src, dest)` — move (CONFIRMATION REQUIRED)
6. `rename_item(old, new)` — rename (CONFIRMATION REQUIRED)
7. `delete_item(path)` — delete (CONFIRMATION REQUIRED; default to RECYCLE BIN not
   permanent; show exactly what will be deleted, warn if folder has contents)

### Confirmation flow
- Read-only + create_folder execute immediately.
- move/rename/delete render a confirmation card in the Rain panel showing the exact
  operation (src→dest, old→new, or delete target + contents warning) with Confirm/Cancel.
- Every tool call is logged to SQLite (timestamp, tool, args, outcome, confirmed/cancelled).

### Agent loop (in handleSend)
1. User message → Rain (system prompt includes SOUL.md + MEMORY.md + FAVORITES.md +
   current_path/selected_files/scope).
2. Model may return tool_calls → execute read-only immediately, or show confirmation
   for destructive ones.
3. Tool results feed back into the model; loop until a final text answer.
4. Hard cap: MAX 10 tool iterations per turn (prevent runaway loops).
5. After the turn, Rain may append to MEMORY.md / FAVORITES.md if it learned something.

### Model routing
Agent/tool tasks → 9Router (tool-capable model). Omnix stays for lightweight chat/
vision/TTS. Default `openrouter/openrouter/free` may not support function calling —
surface a tool-capable model requirement in settings.

**Completion check:** Ask Rain "what's in my Downloads folder?" → it calls
`list_directory` and answers from real data. Ask "rename report.txt to final.txt" →
confirmation card → confirm → file renamed. Rain appends a note to MEMORY.md when it
learns a preference.

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
| MAMBA | <MAMBA_IP> | 3× RTX 3060 | 36GB | Primary inference, headless |
| BLACK | <BLACK_IP> | RX 6900 XT | 16GB | Daily driver, RPC slave |
| Combined | — | — | 52GB | Large model inference |
| 9Router | MAMBA:PORT | — | — | OpenAI proxy |
| Omnix | embedded | WebGPU | — | Lightweight local AI |

## SSH Access

- MAMBA username: `jatilq`
- BLACK username: `jatilq`
- Credentials stored in Meridian settings (encrypted)
