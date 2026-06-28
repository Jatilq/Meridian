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
