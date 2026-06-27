# Meridian — Design Document

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

## Component 1: Omnix Embedded Engine

### Architecture Change (critical)
Omnix does NOT run as a separate spawned process. It runs INSIDE Meridian's Electron main process:
- Omnix's `server.ts` Express API starts when Meridian starts
- A hidden `BrowserWindow` loads Omnix's compute worker renderer (provides WebGPU for model inference)
- All models including large ones have full WebGPU access through Meridian's own renderer
- No separate Omnix install, no spawn/kill complexity, no process management

### Startup Sequence
1. Meridian Electron main process starts
2. Main window loads (Sigma UI)
3. Hidden BrowserWindow created → loads Omnix compute worker
4. Omnix Express server starts on port 9777
5. AI panel polls `/api/health` → status dot updates
6. Ready

### Settings (simplified from previous design)
- Enable/disable Omnix toggle (default: on)
- Model selector (populated from Omnix's available models)
- No path config needed — Omnix is bundled

---

## Component 2: AI Panel

### Placement
Collapsible right-side panel. Toggle with `Ctrl+Space`. Independent of info panel.

### Modes
- **Omnix mode** — calls `localhost:9777` — lightweight, always available, embedded
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
│ MAMBA (192.168.1.67)               │
│ ● Online  |  3× RTX 3060  |  36GB  │
│ Models: [loaded model name]         │
│ GPU: [utilization bar]              │
├─────────────────────────────────────┤
│ BLACK (192.168.1.64)               │
│ ● Online  |  RX 6900 XT  |  16GB  │
│ RPC Slave: [OFF]    [LAUNCH SLAVE] │
├─────────────────────────────────────┤
│ Combined Pool                       │
│ ○ 36GB (MAMBA only)                │
│ ● 52GB (MAMBA + BLACK)  [ACTIVE]   │
├─────────────────────────────────────┤
│ 9Router Status: ● Connected        │
│ Endpoint: http://192.168.1.67:PORT │
└─────────────────────────────────────┘
```

### Launch Slave Button
1. SSH into BLACK (192.168.1.64) using stored credentials
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

## Component 6: Agent Coding Extension

### Built on Sigma's Extension System
- First-party extension, ships with Meridian
- Appears in Extensions sidebar

### Panel Layout
```
┌─────────────────────────────────────┐
│ AGENT CODER                [model▼] │
├─────────────────────────────────────┤
│ Working on: [file path]             │
│ [local] [remote: mamba] [remote: black] │
├─────────────────────────────────────┤
│                                     │
│  [conversation / code output area]  │
│                                     │
├─────────────────────────────────────┤
│ [input field]              [Send]   │
└─────────────────────────────────────┘
```

### Behavior
- Works on file currently selected in active pane (local or remote)
- Remote files: read/write via SSH/SFTP
- Calls AI panel API (9Router mode for coding tasks — needs Qwen3.6 or equivalent)
- Can read, edit, create files directly
- Shows diff before applying changes
- Confirmation required before writes

### Model Recommendation
- Coding tasks → 9Router → Qwen3.6 35B (or whatever is loaded on the full 52GB pool)
- Quick questions → Omnix → Qwen 0.6B

---

## Settings Panel (Meridian section)

| Category | Setting | Default |
|---|---|---|
| AI — Omnix | Enable Omnix | on |
| AI — Omnix | Default model | qwen-3-0.6b |
| AI — 9Router | Endpoint URL | http://192.168.1.67:PORT |
| AI — 9Router | Default model | (from dropdown) |
| Cluster | MAMBA IP | 192.168.1.67 |
| Cluster | BLACK IP | 192.168.1.64 |
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
- 🔄 Phase 5 — Omnix embedded (architectural change: hidden BrowserWindow, not spawn)
- ⬜ Phase 6 — Cluster Control Panel
- ⬜ Phase 7 — SSH/SFTP File Browser
- ⬜ Phase 8 — Agent Coding Extension
- ⬜ Phase 9 — Package & installer (bundle Omnix, sign, Start Menu)

---

## What NOT to Change in Sigma

- Home page, hero banner, drive cards
- Navigation sidebar icons (add new ones, don't replace)
- File grouping and thumbnail display
- Existing extensions system (extend, don't replace)
- Color scheme, fonts, spacing
