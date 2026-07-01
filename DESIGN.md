# Meridian — Design Document

> **PHASES 1-10 COMPLETE. Current work is Phase 11 Backend Manager. Do not re-implement completed phases.**

## Core Philosophy

Meridian is a local-first AI workstation built on Sigma File Manager. Do not redesign Sigma. Add to it. Every new component must match Sigma's existing dark aesthetic exactly — same colors, fonts, spacing, border radius.

The file manager is the shell. Everything else — AI, cluster control, SSH, coding agent — lives inside it.

---

## Sigma's Existing Design (do not change)

- Dark theme: near-black backgrounds (#1a1a1a range), subtle borders, clean typography
- Left icon sidebar: home, files, bookmarks, settings, extensions
- Home page: hero banner, user directory shortcuts, drive cards with usage %
- File view: grouped by type, card thumbnails for media
- Right panel: file/folder metadata
- Tabs at top, breadcrumb navigation
- Extensions system

All new Meridian UI matches this aesthetic exactly.

---

## Component 1: Omnix Engine

### Architecture
Omnix runs as a **separate Electron process** spawned by Meridian. This is the final, intentional architecture:
- `omnix.rs` spawns Omnix's own Electron runtime from `resources/omnix/`
- Omnix's Express server listens on port 9777
- The AI panel polls `/api/health` for status; vision/TTS/director calls go over HTTP
- No hidden BrowserWindow, no embedded webview — process isolation is required for Omnix's WebGPU compute worker

### Settings
- Enable/disable Omnix toggle (default: on)
- Model selector (populated from Omnix's available models)
- Path config optional — Omnix is bundled in `resources/omnix/`

---

## Component 2: AI Panel

### Placement
Collapsible right-side panel. Toggle with `Ctrl+Space`. Independent of info panel.

### Modes
- **Omnix mode** — calls `localhost:9777` (separate Electron process) — lightweight, always available
- **9Router mode** — calls configured endpoint — full model pool, requires MAMBA

### Components
- Mode toggle: Omnix / 9Router
- Model dropdown (fetches from active endpoint's `/v1/models`)
- Three-state status dot: grey = offline, yellow = running/no worker, green = inference ready
- Natural language input field
- Result/response area (scrollable)
- Send button + Enter

### System Prompt (Omnix mode)
```
You are a file management assistant embedded in Meridian.
Current directory: {current_path}
Selected files: {selected_files}
Directory contents: {file_list}

Respond ONLY with JSON:
{
  "intent": "search|organize|analyze|rename|chat",
  "scope": "current|selected|all",
  "preview_only": true,
  "action": {},
  "message": "human readable explanation"
}
```

### Intent Routing (via Omnix /api/director)
Before sending to the model, route the query through `/api/director` to classify intent. Director returns the intent type, then the appropriate handler runs.

### Voice Input (STT)
- Microphone button in AI panel input field
- Records audio → sends to `/api/stt` → populates input field with transcript
- User reviews and hits Send

### Voice Output (TTS)
- Speaker button on any AI response
- Sends response text to `/api/tts` → plays audio

### Vision
- When an image file is selected in the active pane and a query is submitted → automatically sends to `/api/vision` as multipart
- No manual mode switching — Meridian detects the selected file type

### Safety
- Never execute organize/rename/delete without confirmation dialog
- Log all AI actions to SQLite: timestamp, intent, files, outcome, confirmed/cancelled

---

## Component 3: Enhanced Downloader

### Already built — do not rebuild
- Parallel chunk downloader (HTTP Range requests)
- yt-dlp integration with format/quality selector
- Persistent queue (SQLite) with pause/resume/cancel
- Browser extension receiver on port 7771

### Remaining work
- Browser extension icons (placeholder PNG files)
- End-to-end test with real YouTube URL

---

## Component 4: Cluster Control Panel

### Access
- Left sidebar icon (new icon below extensions)
- Or via View menu → Cluster Control

### Layout
```
┌─────────────────────────────────────┐
│ CLUSTER CONTROL                     │
├─────────────────────────────────────┤
│ MAMBA (<MAMBA_IP>)               │
│ ● Online  |  3× RTX 3060  |  36GB  │
│ Models: [loaded model name]         │
│ GPU: [utilization bar]              │
├─────────────────────────────────────┤
│ BLACK (<BLACK_IP>)               │
│ ● Online  |  RX 6900 XT  |  16GB  │
│ RPC Slave: [OFF]    [LAUNCH SLAVE] │
├─────────────────────────────────────┤
│ Combined Pool                       │
│ ○ 36GB (MAMBA only)                │
│ ● 52GB (MAMBA + BLACK)  [ACTIVE]   │
├─────────────────────────────────────┤
│ 9Router Status: ● Connected        │
│ Endpoint: http://<MAMBA_IP>:PORT │
└─────────────────────────────────────┘
```

### Launch Slave Button
1. SSH into BLACK (<BLACK_IP>) using stored credentials
2. Run llama.cpp RPC slave command on BLACK
3. 9Router detects expanded pool
4. Status updates to 52GB combined
5. AI panel model dropdown refreshes

### Node Monitoring
- Poll MAMBA and BLACK status every 30 seconds
- GPU utilization via SSH command (nvidia-smi for MAMBA, rocm-smi for BLACK)
- Loaded model name via 9Router API

### SSH Credentials
- Stored in Meridian settings (encrypted)
- Username, host, port, key file path or password

---

## Component 5: SSH / SFTP File Browser

### Integration
- Remote machines appear as bookmarks in the bookmarks sidebar
- Click a remote bookmark → one pane navigates to the remote filesystem
- Remote pane looks identical to local pane (same columns, same operations)
- Breadcrumb shows: `ssh://mamba/home/jatilq/` style paths

### Operations
- Browse directories, open files (downloads to temp, opens locally)
- Copy files between local and remote panes (drag and drop)
- Rename, delete, new folder on remote
- Upload: drag local files to remote pane
- Download: drag remote files to local pane

### Connections
Pre-configured for MAMBA and BLACK. User can add more in settings.

### Tech
- Node `ssh2` library in Electron main process
- SFTP subsystem for file operations
- IPC between renderer (file pane) and main (ssh2 client)

---

## Component 6: Rain Agent (tools + memory)

### Built on the existing AI Panel
- Rain is the AI panel from Component 2 — UPGRADED from chat assistant to an agent
  with tool calling and persistent memory. Not a separate extension.
- Same right-side panel, same Ctrl+Space toggle, same 9Router/Omnix routing.

### Memory files (user app data dir, alongside meridian.db)
- **SOUL.md** — fixed personality/identity. User-editable; Rain never auto-modifies it.
- **MEMORY.md** — mutable. Rain appends autonomously when it learns something; never
  deletes/rewrites without confirmation.
- **FAVORITES.md** — paths/models/preferences Rain notices repeated; auto-updated.
- All three injected into the system prompt at request time; seeded from bundled
  defaults if missing.

### Tools (OpenAI-style tool_calls via 9Router)
- `list_directory`, `read_file`, `search_files` — read-only, immediate
- `create_folder` — non-destructive, immediate
- `move_files`, `rename_item`, `delete_item` — confirmation card in panel first
  (delete defaults to recycle bin; warns on non-empty folders)
- Tools work on local AND ssh:// remote paths (reuse Phase 7 SFTP commands)

### Agent loop
- User message → model may emit tool_calls → execute (or confirm) → feed results back
  → loop until final answer. Hard cap 10 tool iterations/turn.
- Requires a tool-call-capable model (Qwen3.6+); settings flags when the selected
  model lacks tool support.

### Panel additions
```
┌─────────────────────────────────────┐
│ Rain                       [model▼] │
├─────────────────────────────────────┤
│  [conversation + tool activity]     │
│  ┌─ confirm: move 3 files ────────┐ │
│  │ src → dest    [Cancel][Confirm]│ │
│  └────────────────────────────────┘ │
├─────────────────────────────────────┤
│ Search: [scope▼]                    │
│ [input field]              [Send]   │
└─────────────────────────────────────┘
```

### Safety
- Destructive tools always confirm in-panel before executing.
- All tool calls logged to SQLite (timestamp, tool, args, outcome, confirmed/cancelled).

### Model Recommendation
- Agent/tool tasks → 9Router → Qwen3.6+ (tool-capable, on the 52GB pool)
- Quick chat/vision/TTS → Omnix → Qwen 0.6B

---

## Settings Panel (Meridian section)

| Category | Setting | Default |
|---|---|---|
| AI — Omnix | Enable Omnix | on |
| AI — Omnix | Default model | qwen-3-0.6b |
| AI — 9Router | Endpoint URL | http://<MAMBA_IP>:PORT |
| AI — 9Router | Default model | (from dropdown) |
| Cluster | MAMBA IP | <MAMBA_IP> |
| Cluster | BLACK IP | <BLACK_IP> |
| Cluster | SSH username | jatilq |
| Cluster | SSH key path | (configurable) |
| Cluster | RPC slave command | (configurable) |
| Cluster | Poll interval | 30s |
| SSH/SFTP | Saved connections | MAMBA, BLACK |
| Downloader | Save folder | E:\Downloads |
| Downloader | Max concurrent | 3 |
| Downloader | Chunk count | 8 |

---

## Build Phases (updated)

- ✅ Phase 1 — Sigma running
- ✅ Phase 2 — AI Panel
- ✅ Phase 3 — Enhanced Downloader
- ✅ Phase 4 — Settings
- ✅ Phase 5 — Omnix engine (separate Electron process, bundled)
- ✅ Phase 6 — Cluster Control Panel (live hardware, SSH, topology map)
- ✅ Phase 7 — SSH/SFTP File Browser
- ✅ Phase 8 — Rain Agent (tools + memory + onboarding)
- ✅ Phase 9 — Package & installer (Omnix bundled, Windows installer built)
- ✅ Phase 10 — Hardware Scanner + HuggingFace Recommender
- 🔄 Phase 11 — Backend Manager

---

## Phase 11 — Backend Manager

### Status: Substantially built (see SESSION_HANDOFF.md for deferred items)

Backend Manager is a three-tab panel accessible via the sidebar. It manages the entire local inference stack without touching a command line.

### What's built

- **Backends tab** — lists available backends (llama.cpp CUDA/ROCm/CPU, llamafile, koboldcpp, TurboQuant, Lemonade), auto-filtered by detected GPU vendor. Download, launch, stop, remove operations for each backend. Inline progress during downloads.
- **Models tab** — scans `E:\ai\Models\` for GGUF files with size and estimated quant type. Delete with confirmation (Recycle Bin). Launch model into active backend.
- **RPC Slaves tab** — pick an SSH connection from cluster settings, upload a backend binary via SFTP, launch RPC slave. Status reflects live node check.
- **Rust backend** (`backend_manager.rs`): 9+ Tauri commands — `list_available_backends`, `detect_local_gpu_vendor`, `install_backend`, `list_installed_backends`, `remove_backend`, `launch_backend`, `stop_backend`, `copy_backend_to_worker`, `launch_rpc_slave_remote`, `scan_models`, `delete_model`.
- **GPU vendor detection** — parses GPU name strings (NVIDIA vs AMD/Radeon) with WMI fallback via `rocm-smi --json`. Auto-selects CUDA vs ROCm builds.
- **5 supported backends**: llama.cpp (3 GPU variants), llamafile, koboldcpp, TurboQuant (4 variants), Lemonade.
- **Hardware Scanner** (Phase 10) — nvidia-smi/rocm-smi probes, HuggingFace model search with VRAM-fit filtering, trusted quantizer recommender.
- **Backend catalog** in `src/data/backends.json` (Vite-bundled, not Tauri resource).

### What's deferred (per SESSION_HANDOFF.md)

- `resources/backend_catalog.json` — catalog is still in source code, not a bundled JSON resource
- Install progress events — not all backends emit progress to the frontend yet
- Models tab folder browser — needs `tauri-plugin-dialog` file picker integration
- `reap_backends` wired into `main.rs` — backend cleanup on window destroy not yet connected
- Backend Manager settings subsection in Settings panel

---

## What NOT to Change in Sigma

- Home page, hero banner, drive cards
- Navigation sidebar icons (add new ones, don't replace)
- File grouping and thumbnail display
- Existing extensions system (extend, don't replace)
- Color scheme, fonts, spacing
