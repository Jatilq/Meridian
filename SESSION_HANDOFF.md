# SESSION HANDOFF — Meridian
## For any agent picking up this project

Last updated: June 28, 2026

---

## Current State

**Phases 1-8 complete.** Pre-Phase 9 tasks complete.

**What's confirmed working:**
- Rain AI assistant with personality, tool calling, memory files, onboarding
- Cluster Control showing MAMBA + BLACK live hardware with SVG topology map
- SSH/SFTP remote file browser
- Downloader with yt-dlp
- Markdown rendering in Rain panel
- GitHub repo live: https://github.com/Jatilq/Meridian

**Recent commits:**
- d9d1462a fix(cluster): auto-detect GPU vendor via WMI, Windows AMD VRAM, hide SSH terminal popup
- 77df0ca6 feat: Universal onboarding flow (intro → local/API/basic → download folder → done)
- 680cc38e feat: Phase 10 Hardware Scanner panel + HuggingFace recommender
- 0a5abd93 feat: What's New popup + onboarding refinements
- df7d8738 feat: Rain first-run onboarding + cluster topology map

---

## Session Complete

All pre-Phase 9 tasks implemented:
- ✅ Rain first-run onboarding (onboardingComplete flag, greeting message, Skip button)
- ✅ Cluster topology map (SVG visualization of MAMBA + BLACK nodes)
- ✅ Fixed hardcoded SSH credentials in cluster.vue
- ✅ Serde camelCase audit (SshCredentials already has rename_all = "camelCase")
- ✅ Auto-detect default download folder (schema migration 21→22 with E:\Downloads priority)
- ✅ Omnix bundling (resources/omnix/ created, omnix.rs auto-extract logic)

---

## Phase 9 — Package & Installer ✅ DONE

Completed:
- Omnix bundled in resources/omnix/
- Installer built: `Meridian_2.1.1_x64-setup.exe` (40.7MB)
- Installer tested and working
- Pending: Update README with user setup instructions

---

## Phase 10 — Hardware Scanner + HuggingFace Recommender ✅ DONE

- Hardware Scanner in sidebar (`/hardware` route)
- Combined VRAM display from local machine
- HuggingFace API search for GGUF models
- Download integration via Meridian downloader queue

---

## Phase 11 — Backend Manager (ready to implement)

---

## Architecture Rules

1. Stack: Tauri 2 + Vue 3 + Rust. NOT Electron.
2. Omnix: separate hidden Electron process. Never embed as webview.
3. Rain: gender neutral, never breaks character, never says "I am an AI"
4. All frontend→Rust structs: `#[serde(rename_all = "camelCase")]`
5. Credentials: never hardcode, never plaintext, Tauri safeStorage
6. Destructive operations: always confirmation dialog
7. JC async: never proceed on timeout for external actions
8. Performance: topology map must be pure SVG

---

## Hardware

| Machine | IP | GPU | VRAM | RAM | Role |
|---|---|---|---|---|---|
| MAMBA | 192.168.1.67 | 3× RTX 3060 | 36GB | 256GB | Primary inference |
| BLACK | 192.168.1.64 | RX 6900 XT | 16GB | 64GB | RPC slave |
| Combined | — | — | 52GB | 320GB | Large models |

9Router: http://localhost:20128/v1
Omnix: http://localhost:9777
SSH key: C:\Users\jatilq\.ssh\meridian_black
Projects: E:\ai\Projects\Meridian\
Models: E:\ai\Models\