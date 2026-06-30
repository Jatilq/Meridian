# SESSION HANDOFF — Meridian
## For any agent picking up this project

Last updated: June 28, 2026 (overnight autonomous session pass June 29)

---

## Current State

**Phases 1-8 complete. Phase 9 + 10 shipped. Phase 11 in progress.**

**What's confirmed working (June 29):**
- **Cluster Control decoupled from `meridian.sshConnections`.** Cluster nodes now live in `meridian.clusterWorkers`. The schema 25→26 dev-lab purge that wiped JC's MAMBA/BLACK entries no longer reaches the cluster — file-browser SSH and Cluster Control own separate arrays.
- **Global 24px bottom padding on every scrollable region.** `src/styles/index.css` carries a `:where(...)` zero-specificity rule covering `.cluster__nodes`, `.cluster-modal__body`, `.bm__section`, `.bm__models`, `.hardware__models`, `.settings-view__content`, `.settings-nav`, `.sigma-ui-dialog-scroll-content`. Per-component `padding-bottom` overrides still win on specificity.
- **HF search (Hardware Scanner) parallelised.** `Promise.all` over 12 candidates per query with per-repo try/catch (~8s → ~1-2s wall-clock). Round-17 server-side `&filter=` parameter was REVERSED in `1d2f7a43` after a live smoke-test proved HF treats `search + filter` as AND (combined request returned 0 rows; bare `search` returned 50). Diagnostic `console.log` on the success path reports raw count for future debugging.
- **Backend Manager Models tab — recursive `.gguf` scan.** `scan_models_recursive` in `backend_manager.rs:817` walks the tree with `walkdir::WalkDir` and `usize::MAX` depth. One-level recursion (in the previous `scan_models`) was replaced because users keep models in `E:\ai\Models\<vendor>\<family>\<variant>\`.
- **Backend Manager file resolution.** `hf_resolve_model_files` (in `hardware.rs`) resolves a HF repo to a single downloadable GGUF, then backend-manager.vue enqueues through `downloader_enqueue` with the resolved URL.
- **Cluster onboarding empty state.** When `nodeViews.length === 0`, the topology + cards + Launch button disappear; an inline `<h2>No workers yet</h2>` + Add Worker CTA appears instead.

**Today's commits (oldest → newest):**

| Hash | Subject | Why |
|---|---|---|
| `0f13389b` | fix(layout): global 24px scroll padding, real settings selectors, audit cluster modal body | Task 1 — index.css with explicit verified selectors, zero-specificity wrapper |
| `1d2f7a43` | fix(hardware): HF search — parallel siblings + success-path logging, drop broken &filter= param | HF latency fix, drop broken round-17 server-side filter |
| `9cab74b3` | fix(cluster): onboarding empty state, generic RPC target, scroll CSS cleanup | Bug-2 fix from JC's screenshot batch |
| `9ce7c802` | fix(backend-manager): resolve HF repo to real model file via hf_resolve_model_files | Task 3 — Omnix HF URL enqueue |
| `82b698c2` | fix(cluster): separate clusterWorkers from sshConnections, restore cluster nodes | Task 1 (Bug-1): cluster refactor |
| `57a8f861` | fix(layout+connectivity): scroll padding, HF search, Omnix count, CSS cleanup | ⚠ Bucket commit — see "Bucket commit" below |

Unpushed: count grows with each local commit — re-run `git rev-list --count origin/main..HEAD` and `git rev-list --count meridian/main..HEAD` before pushing.

---

## Architectural Decision: clusterWorkers vs sshConnections

**Problem.** Cluster Control's node list and the SSH file-browser's remote-pane list were both sourced from `meridian.sshConnections`. The schema 25→26 dev-lab purge (which strips `192.168.1.67` + `192.168.1.64` `jatilq` entries to satisfy "no hardcoded creds for new users") wiped JC's cluster nodes too — Cluster Control went blank.

**Solution.** Two independent arrays:

- `meridian.clusterWorkers: SshConnectionSetting[]` — Cluster Control hardware, Backend Manager's Workers/RPC Slaves tab, new **Settings → Meridian → Cluster Nodes** editor. Lives in `src/modules/cluster/pages/cluster.vue` (`nodeDefs`, `refreshNodeViews`, `launchRpcSlave`, `saveWorker`), `src/modules/backend-manager/pages/backend-manager.vue` (`mapClusterWorkerToSlave`), `src/modules/settings/ui/categories/meridian/cluster-nodes.vue` (new).
- `meridian.sshConnections: SshConnectionSetting[]` — File-browser remote-pane routing only. unchanged consumer list: `quick-access-panel.vue`, `ssh-connections.vue` settings editor.

**Schema migration.** `USER_SETTINGS_SCHEMA_VERSION` 26 → 27.

- **25 → 26** now writes an explicit marker `meridian.__purgedDevLab = droppedDevLabCount` ONLY when rows matching `(DEV_HOSTS × DEV_USERS)` were actually dropped. If `count > 0` the marker is set; if `count === 0` nothing is written.
- **26 → 27** reads the marker. If the marker exists and `> 0`, it copies any user-added entries that survived the purge from `sshConnections` → `clusterWorkers` AND re-seeds MAMBA + BLACK. If the marker is absent (or `0`), it does NOT seed phantom entries — only the user-added survivors are migrated.

Non-JC installs are safe. JC's install gets its two lab nodes back. Adding a worker in Settings → Meridian → Cluster Nodes now never touches the file-browser SSH list.

---

## Bucket commit `57a8f861` — known cleanup opportunity

`57a8f861` was JC's end-of-day bucket commit. It aggregates pre-existing residual changes from earlier in-flight rounds: Phase 2 Rain (`ai-panel.vue` + `ai-panel.ts` + meridian/ai-panel.vue), Phase 5 Omnix (`omnix.rs`), Phase 6 Cluster (`cluster.rs`), Phase 8 Rain (`rain_tools.rs`), Phase 11 Backend Manager (`backend_manager.rs` + `data/backends.json` + `main.ts` + `lib.rs`), and the `/nul` `.gitignore` guard.

The subject line is misleading (mentions scroll padding + HF search + Omnix count + CSS cleanup — none of which are the dominant changes in the diff). It does include a working `scan_models_recursive` (Phase 11, registered as a Tauri command, with a unit test), but that work is bundled with the unrelated phases.

**Recommended action.** `git reset --soft HEAD~1` then re-stage per area with accurate messages — keeps `git bisect` viable and makes future PR review possible. Wait for JC before doing this; it's a destructive rewrite of published history.

---

## User Configurable (any user) — unchanged

Recent commits removed JC-specific hardcoding so Meridian can configure itself for any user instead of being tied to one developer's filesystem:

- **No hardcoded credentials or paths.** `cluster.vue`, `utils/ssh-connections.ts`, `stores/storage/user-settings.ts`, `stores/schemas/user-settings.ts`, `backend-manager.vue`, `hardware.vue`, and `types/user-settings.ts` no longer seed `jatilq`, `C:\\Users\\jatilq\\.ssh\\meridian_black`, `192.168.1.67`, `192.168.1.64`, or `E:\\ai\\Models` as defaults. All of these now ship empty / blank and the user fills them in via the UI.
- **SSH supports both key and password auth.** New `SshAuthMethod = 'key' | 'password'` toggle in the SSH settings UI; `cluster.rs::ssh_exec()` branches on `key_path` → `authenticate_publickey`, password-only → `authenticate_password`, and rejects with `"No authentication method configured — provide a key path or password"` otherwise.
- **Isolated password storage.** SSH passwords live in the secure-keys.json Tauri store (matches the existing api-key isolation pattern) via new `secure_store_secret` / `secure_get_secret` / `secure_delete_secret` Tauri commands in `secure_keys.rs`. The frontend writes only through `storeSshPassword` on save (and clears the in-memory plaintext) so the main user-settings blob never holds plaintext.
- **Configurable Models folder.** New `meridian.modelsFolder` setting with **Settings → Meridian → Files** panel: a folder-path input + Browse button (Tauri dialog plugin). Hardware Scanner, AI panel, and Backend Manager read this path at runtime.
- **AMD VRAM cap fix.** `cluster.rs::get_remote_hardware` uses `Get-CimInstance Win32_VideoController` (CIM reports `AdapterRAM` as UInt64) instead of the legacy `Get-WmiObject` which capped the value to ~4 GB. RX 6900 XT now shows 16 GB.
- **Add Worker dialog.** `cluster.vue` Add Worker button opens an inline modal (Label / Host / Port / Username / Auth toggle / key OR password / Test Connection (calls `check_node_status`) / Save / Cancel).
- **Configurable download folder with auto-detect.** **Settings → Meridian → Downloader** has an `Auto-save folder` input; on first run the schema migration 21→22 prefers `E:\\Downloads` then `C:\\Users\\<user>\\Downloads`, then creates `E:\\Downloads`.

---

## Session Complete (cumulative)

All pre-Phase 9 + Phase 9 + Phase 10 tasks implemented:

- ✅ Rain first-run onboarding (onboardingComplete flag, greeting message, Skip button)
- ✅ Cluster topology map (SVG visualization of MAMBA + BLACK nodes)
- ✅ Fixed hardcoded SSH credentials in cluster.vue
- ✅ Serde camelCase audit (SshCredentials + SftpCredentials have `rename_all = "camelCase"`)
- ✅ Auto-detect default download folder (schema migration 21→22 with `E:\Downloads` priority)
- ✅ Omnix bundling (resources/omnix/ created, omnix.rs auto-extract logic)
- ✅ Phase 9: Installer package (`Meridian_2.1.1_x64-setup.exe`)
- ✅ Phase 10: Hardware Scanner + HuggingFace recommender
- ✅ clusterWorkers ↔ sshConnections separation (this session)
- ✅ Global 24px scroll padding (this session)
- ✅ Backend Manager recursive `.gguf` scan (this session, in `57a8f861`)
- ✅ HF repo → real model file resolution + downloader enqueue (this session)

---

## Phase 11 — Backend Manager (IN PROGRESS)

**What exists.**

- `src-tauri/src/backend_manager.rs`
  - `scan_models_recursive` at line 817 (registered in `lib.rs:426` as `backend_manager::scan_models_recursive`)
  - Walkdir-based, `usize::MAX` depth, one-level recursion replaced
  - Unit test `scan_models_recursive_walks_past_default_depth_cap` at line 1720
  - `hf_resolve_model_files` at line 851 (registered in `lib.rs:427` as `backend_manager::hf_resolve_model_files`)
  - Resolves a HuggingFace repo query to a single downloadable GGUF, then enqueues through `downloader_enqueue`
- `src/modules/hardware/pages/hardware.vue`
  - Parallel `Promise.all` over 12 sibling candidates per search click with per-repo try/catch (~8s → ~1-2s wall-clock)
  - Diagnostic `console.log` on the success path that reports the raw HF result count for future debugging
- `src/data/backends.json` — local backend catalog
- `src/data/omnix-catalog.json` — Omnix model catalog (233 lines; modelID/name/description/size/category/make/minRam/verifiedWorking)
- `src/modules/backend-manager/pages/backend-manager.vue`
  - Three tabs: Backends / Models / Workers
  - Maps `meridian.clusterWorkers` → RPC slave candidates via `mapClusterWorkerToSlave`
- `src/modules/cluster/pages/cluster.vue`
  - Add Worker dialog (inline modal with key/password auth toggle)
  - Test Connection via `check_node_status`
  - Encrypted SSH password via `secure-keys.json`
- Hardware vendor auto-detection: `cluster::get_local_hardware` runs `nvidia-smi` + WMI (`Get-CimInstance Win32_VideoController`) so NVIDIA + AMD mixed-vendor boxes route to the correct backend.

**Not yet.**

- Bundle catalog `resources/backend_catalog.json` (referenced in the Phase-11 spec)
- `reap_backends` cleanup on `WindowEvent::Destroyed` (referenced in the spec; `lan_share::stop_lan_share` is the existing call-site precedent)
- Per-backend install progress events (`app.emit("backend-install-progress", ...)`)
- Models tab folder browser / hardlink between Hardware Scanner and Backend Manager downloads

---

## Architecture Rules

1. Stack: Tauri 2 + Vue 3 + Rust. NOT Electron.
2. Omnix: separate hidden Electron process. Never embed as webview.
3. Rain: gender neutral, never breaks character, never says "I am an AI"
4. All frontend→Rust structs: `#[serde(rename_all = "camelCase")]`
5. Credentials: never hardcode, never plaintext, Tauri safeStorage or `secure-keys.json` for SSH passwords
6. Destructive operations: always confirmation dialog
7. JC async: never proceed on timeout for external actions
8. Performance: topology map must be pure SVG
9. **Cluster workers ≠ file-browser SSH**. Two independent arrays; never share storage between them again.

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
