# Meridian

**The local-first AI workstation. File manager, AI engine, cluster control, remote access — one package.**

Meridian is built on [Sigma File Manager](https://github.com/aleksey-hoffman/sigma-file-manager) (Electron + Vue, GPL3) and extends it into something that doesn't exist anywhere else: a complete local AI workstation where everything runs on your own hardware, nothing touches the cloud unless you want it to.

---

## The Vision

Most AI tools are cloud-dependent. Cursor, GitHub Copilot, ChatGPT — they all phone home. Meridian is the opposite: a single Electron app that combines everything a serious AI hobbyist or developer needs, running entirely on local hardware.

---

## What Meridian Is

### 1. File Manager (Sigma foundation)
- Dual-pane layout with tabs
- File grouping with media thumbnails
- Bookmarks, search, preview panel
- Copy/move/rename with undo
- Archive browsing, WSL integration
- Extensions system

### 2. Embedded Local AI (Omnix — integrated, not separate)
Omnix runs **inside** Meridian's own Electron process as a hidden compute window. No separate app, no separate install, no process management.
- **Text** — natural language file queries via `/api/text`
- **Vision** — analyze selected images via `/api/vision`
- **STT** — speak file commands instead of typing
- **TTS** — AI panel reads responses out loud
- **Director** — Omnix's intent router classifies queries before sending to models
- **Image generation** — local Flux nodes via `/api/image`
- **Music** — local audio synthesis via `/api/music`
- Models: Qwen 3 0.6B confirmed working; larger models via WebGPU in the embedded renderer

### 3. AI Panel (Natural Language File Operations)
- Collapsible panel, connects to any OpenAI-compatible endpoint
- Model selector fetches live from `/v1/models`
- Toggle between Omnix (lightweight, embedded) and 9Router (full model pool)
- Intent routing: search / organize / rename / analyze / chat
- Confirmation required before any destructive action
- SQLite action log

### 4. Enhanced Downloader (IDM-style)
- Extends Sigma's existing yt-dlp downloader
- Parallel chunk downloading via HTTP Range requests
- Format/quality selector, persistent queue, pause/resume/cancel
- Chrome/Edge browser extension intercepts video URLs automatically

### 5. Cluster Control Panel
- Shows MAMBA and BLACK online/offline status
- One-click SSH slave launch: fires up llama.cpp RPC on BLACK via SSH
- 36GB VRAM (MAMBA alone) → 52GB VRAM (MAMBA + BLACK combined)
- 9Router automatically gains access to the expanded pool
- Monitor active nodes, GPU utilization, loaded models

### 6. SSH / SFTP File Browser
- Browse and manage files on remote machines directly in Meridian's file panes
- MAMBA and BLACK appear as bookmarked SSH connections
- Drag files between local and remote panes
- Full file operations (copy, move, rename, delete) over SSH/SFTP
- Foundation for agent coding on remote files

### 7. Agent Coding Extension
- Built on Sigma's existing extension system
- Opens a coding agent panel connected to the full model pool (via 9Router)
- Works on local files OR remote files via SSH
- Powered by the AI panel API — extensions call the same endpoints the UI uses
- The AI panel becomes a first-class API for all extensions to build on

---

## Architecture

```
Meridian (Electron + Vue)
├── Sigma File Manager (base — file ops, UI, extensions)
├── Omnix Engine (embedded hidden BrowserWindow — WebGPU compute worker)
│   ├── Express API server (localhost:9777)
│   ├── /api/text, /api/vision, /api/director, /api/stt, /api/tts
│   └── Models: Qwen 0.6B (fast) + larger via WebGPU
├── AI Panel (Vue component)
│   ├── Omnix mode → localhost:9777
│   └── 9Router mode → MAMBA:PORT (heavy models, full VRAM pool)
├── Cluster Control Panel
│   ├── SSH to BLACK → launch llama.cpp RPC slave
│   ├── 9Router gains 52GB combined VRAM
│   └── Node status monitoring
├── SSH/SFTP Browser
│   ├── Remote panes alongside local panes
│   └── Foundation for remote agent coding
├── Enhanced Downloader
│   ├── yt-dlp + parallel chunk downloader
│   └── Browser extension receiver
└── Extension System (Sigma's existing)
    └── Agent Coding Extension (first-party)
```

## Hardware

| Machine | IP | GPU | Role |
|---|---|---|---|
| MAMBA | <MAMBA_IP> | 3× RTX 3060 (36GB) | Primary inference, headless server |
| BLACK | <BLACK_IP> | RX 6900 XT (16GB) | Daily driver, RPC slave |
| Combined | — | 52GB effective | Large model inference |
| 9Router | localhost on MAMBA | — | OpenAI-compatible proxy |
| Omnix | embedded in Meridian | WebGPU | Lightweight on-device AI |

## Why This Matters

Every component is Electron + Node:
- Sigma — Electron + Vue
- Omnix — Electron + WebGPU
- llama-cluster-launcher — Electron
- SSH/SFTP — Node `ssh2` library
- Agent coding — extension calling AI panel API

One stack. One package. One installer. Everything local.

No Cursor. No Copilot. No cloud. Your hardware, your models, your data.

## Owner

JC. Hobbyist. All development is agent-driven. See CLAUDE.md.

## Project Location

`E:\ai\Projects\Meridian\`
