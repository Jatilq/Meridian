# SESSION HANDOFF — Meridian
## For any agent picking up this project (Hermes, Kilo Code, OpenCode, Claude Code)

Last updated: June 28, 2026 morning session

---

## What Just Happened

Long productive session June 27-28. Here's what got built and confirmed working:

**Confirmed working via Parsec screenshots:**
- Cluster Control showing MAMBA (3× RTX 3060, real temps) + BLACK (Ryzen 9 5950X, RX 6900 XT) live
- Rain greeting: "Hey, it's Rain. Where do you want to start?" ✅
- Rain responding naturally to file questions ✅
- Settings panel: Temperature (0.7), Top-p (1), Max tokens, SSH Connections all showing correctly ✅
- GitHub repo live: https://github.com/Jatilq/Meridian ✅

**Fixed this session:**
- Right-click paste in input fields
- Downloader pending-forever (status serialization bug)
- BLACK cluster SSH key-path (serde camelCase bug)
- Rain personality and identity rules
- Phase 8 Rain agent: tool calling, memory files, confirmation cards, SQLite logging

---

## Immediate Next Tasks (do these in order)

### Task 1: Serde audit (small, do first)
Check ALL frontend→Rust structs for camelCase/snake_case mismatch.
The BLACK cluster bug was: frontend sent `keyPath`, Rust expected `key_path`, field arrived as None.
Fix: add `#[serde(rename_all = "camelCase")]` to any struct that receives data from Vue.
Check specifically: SftpCredentials in sftp.rs, any other IPC structs.
Commit: `fix: serde camelCase audit - ensure all IPC structs handle frontend field names`

### Task 2: Default download folder (small)
Auto-detect on first run: check if `E:\Downloads` exists, else use `C:\Users\jatilq\Downloads`.
If neither exists, create `E:\Downloads`.
Set as default in settings on first run only (don't override user's saved preference).
Commit: `feat: auto-detect default download folder on first run`

### Task 3: Rain onboarding (medium)
First-run welcome flow — triggers when no settings have been saved yet.
Rain says: "Hey, I'm Rain. Looks like this is your first time here — want me to walk you through a few basics?"
Offers 4 skippable steps:
1. Set download folder (pre-filled with auto-detected path)
2. Configure 9Router endpoint (default: http://localhost:20128/v1)
3. Add SSH connections (MAMBA/BLACK pre-filled)
4. Done — "You're all set. Ask me anything."
Skip button always visible. Never blocks the user from using the app.
Commit: `feat: Rain first-run onboarding flow`

### Task 4: Push to GitHub
After tasks 1-3 are committed, push everything.
JC will provide a fresh PAT (classic, repo scope) directly in the terminal.
Push command: `git remote set-url meridian https://<TOKEN>@github.com/Jatilq/Meridian.git && git push meridian main && git remote set-url meridian https://github.com/Jatilq/Meridian.git`
Never echo the token. Scrub immediately after push.

---

## Next Major Phase: Phase 9 — Package & Installer

After tasks 1-4 are done:
- Bundle Omnix files into Tauri resources/
- Bundle yt-dlp binary (already in src-tauri/binaries/)
- Build Windows .msi installer via Tauri bundler
- Start Menu shortcut
- Test installer on clean path
- Final README with setup steps

---

## After Phase 9: Phase 10 — Hardware Scanner + HuggingFace Recommender

- Scan local hardware (already partially built in Cluster Control)
- Add BLACK's specs via SSH
- Query HuggingFace API for compatible models
- Filter by: fits VRAM / fits RAM / trusted quantizers only
- Trusted quantizers: Bartowski, Unsloth, MaziyarPanahi, LoneStriker
- Min quant: Q4_K_M — never recommend IQ1-IQ3

---

## After Phase 10: Phase 11 — Backend Manager (BIG FEATURE)

This is the feature that makes Meridian completely self-contained.

### What it does:
Users can download, configure, and launch inference backends directly from Meridian.
No command line. No manual setup. No external tools.

### Backends to support:
- llama.cpp (CPU / CUDA / ROCm-Vulkan variants)
- llamafile (single executable, simplest)
- koboldcpp

### How it works:
1. Backend Manager panel (new sidebar icon)
2. Hardware auto-detected (GPU vendor from Cluster Control data)
3. Download button → fetches correct binary for hardware → saves to E:\ai\Apps\backends\
4. Start/Stop controls per backend
5. Model list from E:\ai\Models\ → load with one click

### RPC Slave auto-setup (the killer feature):
When "Launch RPC Slave on BLACK" is clicked:
1. Meridian copies llama.cpp RPC server binary to BLACK via SFTP
2. SSH to BLACK → starts RPC server on port 50052
3. MAMBA's llama-server connects to BLACK
4. Combined 52GB VRAM pool available to 9Router
5. Update/upgrade: Meridian downloads new binary, SFTPs to BLACK, restarts

### rpc_manager integration:
- https://github.com/arseniy0924/rpc_manager is the orchestration tool
- Meridian should be able to launch and control rpc_manager
- rpc_manager chooses the backend for the slave automatically

### Why this matters:
Install Meridian → Backend Manager → download llama.cpp → load model → 52GB inference.
Nobody else has this. File manager + full inference stack management in one app.

---

## Architecture Rules (never change these)

1. Stack: Tauri 2 + Vue 3 + Rust. NOT Electron.
2. Omnix: separate hidden Electron process. Do NOT embed as webview.
3. Rain: gender neutral, never breaks character, never says "I am an AI"
4. Credentials: never hardcode, never store plaintext, use Tauri safeStorage
5. Destructive operations: always confirmation dialog
6. JC's workflow: async, never proceed on timeout for external actions

---

## If Multiple Agents Are Running

Kilo Code, OpenCode, and Hermes may all be active. Coordinate:
- Never edit the same file simultaneously
- Check git status before starting work
- Commit frequently with clear messages
- Each agent should focus on different tasks

Suggested split if multiple agents active:
- Agent 1: Tasks 1-2 (serde audit + download folder) — small Rust fixes
- Agent 2: Task 3 (Rain onboarding) — Vue frontend work
- Agent 3: Phase 9 installer prep — Tauri config work
