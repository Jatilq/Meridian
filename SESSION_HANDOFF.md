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

## User Configurable (any user)

Recent commits removed JC-specific hardcoding so Meridian can configure itself for any user instead of being tied to one developer's filesystem:

- **No hardcoded credentials or paths.** `cluster.vue`, `utils/ssh-connections.ts`, `stores/storage/user-settings.ts`, `stores/schemas/user-settings.ts`, `backend-manager.vue`, `hardware.vue`, and `types/user-settings.ts` no longer seed `jatilq`, `C:\\Users\\jatilq\\.ssh\\meridian_black`, `192.168.1.67`, `192.168.1.64`, or `E:\\ai\\Models` as defaults. All of these now ship empty / blank and the user fills them in via the UI.
- **SSH supports both key and password auth.** New `SshAuthMethod = 'key' | 'password'` toggle in the SSH settings UI; `cluster.rs::ssh_exec()` branches on `key_path` → `authenticate_publickey`, password-only → `authenticate_password`, and rejects with `"No authentication method configured — provide a key path or password"` otherwise.
- **Isolated password storage.** SSH passwords live in the secure-keys.json Tauri store (matches the existing api-key isolation pattern) via new `secure_store_secret` / `secure_get_secret` / `secure_delete_secret` Tauri commands in `secure_keys.rs`. The frontend writes only through `storeSshPassword` on save (and clears the in-memory plaintext) so the main user-settings blob never holds plaintext. The Rust side reads via `secure_get_secret` on demand inside `ssh_exec`, gated by a `passwordSecureKey` reference on the connection. Notes: this is isolation, not strong cryptographic encryption — a determined attacker with code execution on the machine can still read `secure-keys.json`. Swapping in Windows Credential Manager / macOS Keychain / libsecret behind the same `secure_*` interface is a local upgrade.
- **Configurable Models folder.** New `meridian.modelsFolder` setting with `Settings -> Meridian -> Files` panel: a folder-path input + Browse button (Tauri dialog plugin). Hardware Scanner, AI panel, and Backend Manager read this path at runtime.
- **AMD VRAM cap fix.** `cluster.rs::get_remote_hardware` now uses `Get-CimInstance Win32_VideoController` (CIM reports `AdapterRAM` as UInt64) instead of the legacy `Get-WmiObject` which capped the value to ~4 GB. RX 6900 XT now shows 16 GB.
- **Add Worker dialog.** `cluster.vue` Add Worker button now opens an inline modal (Label / Host / Port / Username / Auth toggle / key OR password / Test Connection (calls `check_node_status`) / Save / Cancel) instead of dispatching a CustomEvent to global SSH settings.
- **Configurable download folder with auto-detect.** `Settings -> Meridian -> Downloader` has an `Auto-save folder` input; on first run the schema migration 21→22 prefers `E:\\Downloads` then `C:\\Users\\<user>\\Downloads`, then creates `E:\\Downloads`.

The Hardware Reference table lower in this file still lists JC's MAMBA / BLACK setup because that is documentation of the development environment, not defaults that ship to users.

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