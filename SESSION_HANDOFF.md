# SESSION HANDOFF — Meridian
## For any agent picking up this project

Last updated: June 28, 2026

---

## Current State

**Phases 1-8 complete.** Working on pre-Phase 9 tasks.

**What's confirmed working:**
- Rain AI assistant with personality, tool calling, memory files, onboarding
- Cluster Control showing MAMBA + BLACK live hardware with SVG topology map
- SSH/SFTP remote file browser
- Downloader with yt-dlp
- Markdown rendering in Rain panel
- GitHub repo live: https://github.com/Jatilq/Meridian

**Recent commits:**
- Rain first-run onboarding + cluster topology map
- Serde camelCase audit (SftpCredentials fixed)
- Auto-detect default download folder
- Omnix build + bundling research (Kilo)
- Rebrand to Meridian (in progress)

---

## Immediate Tasks (in priority order)

### Task 3 — Rain First-Run Onboarding ✅ DONE
- Implemented onboardingComplete flag
- Rain shows first-time message with current settings summary
- Skip button integrated into ai-panel.vue

### Omnix Bundling (Kilo)
Bundle pre-built Omnix into Tauri resources:
- Built dist/server.cjs + UI assets in src-tauri/resources/omnix/
- Update tauri.conf.json bundle.resources with omnix directory
- Update omnix.rs: auto-extract to E:\ai\Apps\Omnix\, npm install on first run
- First-run flag to prevent repeated npm install
Commit: `feat: bundle Omnix with installer`
- tauri.conf.json: productName → 'Meridian', identifier → 'com.meridian.app'
- package.json: name → 'meridian'
- About screen: 'Meridian — Built on Sigma File Manager by Aleksey Hoffman'
- Window title: 'Meridian'
- Keep all Sigma credits in CREDITS.md and README.md
Commit: `feat: rebrand to Meridian, maintain Sigma attribution`

### What's New Popup (Hermes)
Shows on version change. Dark theme modal, gold accents.
Content for v1.0:
- 🤖 Rain — AI assistant built in
- 🖥️ Cluster Control — Multi-node GPU pooling
- 📁 SSH/SFTP Browser — Remote machines as local drives
- ⬇️ Smart Downloader — yt-dlp powered
- 👁️ Vision & TTS — Omnix powered
- ⚡ Agent Mode — Rain can actually move and organize files
Commit: `feat: What's New popup`

### Omnix Bundling (Kilo)
Bundle pre-built Omnix into Tauri resources:
- Copy dist/, electron/, server.ts, package.json to src-tauri/resources/omnix/
- Add to tauri.conf.json bundle.resources
- Update omnix.rs: check E:\ai\Apps\Omnix\ exists, if not extract from resources + npm install
- First-run flag so npm install only runs once
Commit: `feat: bundle Omnix with installer`

### Push all commits
After above tasks done, JC provides fresh PAT (classic, repo scope).
Command: `git remote set-url meridian https://<TOKEN>@github.com/Jatilq/Meridian.git && git push meridian main && git remote set-url meridian https://github.com/Jatilq/Meridian.git`
Never echo token. Scrub immediately after push.

---

## Phase 9 — Package & Installer
After all above tasks complete:
- Omnix already bundling (Kilo)
- yt-dlp already in src-tauri/binaries/
- Run: `npm run tauri build`
- Test installer
- Verify Start Menu shortcut
- Update README

---

## THE BIG VISION — Phase 11 Multi-Node Cluster (READ THIS)

This is the feature that makes Meridian a game changer for average users.

### The Problem It Solves
Most people have multiple machines with GPUs sitting around — an old gaming PC, a laptop, a friend's machine. Each one alone can only run small models. Combined, they could run much larger models. But combining them requires command line setup, llama.cpp compilation, manual configuration. Nobody does it.

### What Meridian Makes Possible
- Add ANY machine as a worker node via SSH (same settings panel already built)
- Meridian copies the RPC slave binary to the worker via SFTP — worker needs nothing installed
- Worker machine is completely passive — just needs SSH access
- Pool all VRAM across all nodes automatically
- Run models that fit the combined pool

### Example
- User has: 3060 desktop (12GB) + old laptop with 1660 (6GB)
- Separately: can run 7B models
- Together via Meridian: 18GB VRAM, runs 13B comfortably
- No command line. No manual setup. Just add in Meridian settings and click Launch.

### Topology Map (visual centerpiece)
SVG-based node graph in Cluster Control / Backend Manager:
- Each worker = a node card showing: hostname, IP, GPU name, VRAM, CPU, status dot
- Connected by lines showing network link + latency
- Combined VRAM total shown prominently
- Color coded: green = active, yellow = connecting, grey = offline
- Click a node = expand details
- "Add Worker" button opens SSH connection dialog (reuses existing SSH settings UI)
- Workers auto-appear on map when added

### Key Design Rules for Topology Map
- Pure SVG — no D3, no heavy libraries, zero performance impact
- Reuses existing 30s polling data from Cluster Control — no new network calls
- Scales to 2, 3, 4, 5+ nodes with same code
- Dark theme matching Meridian (#1e1e1e background, #c9a84c gold accents for active nodes)

### How Workers Are Added
1. User clicks "Add Worker" in topology map
2. SSH connection dialog (hostname, username, key path) — same as existing SSH settings
3. Meridian SSHs in, detects GPU via nvidia-smi or rocm-smi
4. Node appears on topology map with detected specs
5. "Launch RPC Slave" button copies binary via SFTP and starts it
6. Combined VRAM updates automatically
7. 9Router gains access to expanded pool

### Worker Requirements (intentionally minimal)
- SSH access (any OS — Windows, Linux, Mac)
- Nothing else — Meridian handles everything else via SSH/SFTP

### GitHub Repo Description (already updated or update now)
`Local-first AI workstation. File manager + embedded AI (Rain) + multi-node GPU cluster — add any machine as a worker, pool VRAM across devices, run larger models than any single GPU allows. Built on Sigma File Manager.`

---

## Phase 10 — Hardware Scanner + HuggingFace Recommender
- Scan all nodes' hardware (already have data from Cluster Control)
- Query HuggingFace for models fitting combined VRAM pool
- Trusted quantizers: Bartowski, Unsloth, MaziyarPanahi, LoneStriker
- Min quant: Q4_K_M only — never IQ1-IQ3
- Download goes through Meridian's downloader queue

---

## Session Complete

**Today's work:**
- ✅ Rain first-run onboarding (onboardingComplete flag, greeting message, Skip button)
- ✅ Cluster topology map (SVG visualization of MAMBA + BLACK nodes)
- ✅ Fixed hardcoded SSH credentials in cluster.vue
- ✅ Serde camelCase audit (SftpCredentials already fixed)
- ✅ Auto-detect default download folder (schema migration 21→22)

**Remaining:**
- Omnix bundling (resources/omnix/ created, tauri.conf.json needs update)
- Rebrand to Meridian (tauri.conf.json, package.json)
- Phase 9 installer build

1. Stack: Tauri 2 + Vue 3 + Rust. NOT Electron.
2. Omnix: separate hidden Electron process. Never embed as webview.
3. Rain: gender neutral, never breaks character, never says "I am an AI"
4. All frontend→Rust structs: `#[serde(rename_all = "camelCase")]`
5. Credentials: never hardcode, never plaintext, Tauri safeStorage
6. Destructive operations: always confirmation dialog
7. JC async: never proceed on timeout for external actions
8. Performance: topology map must be pure SVG, no heavy libraries

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
