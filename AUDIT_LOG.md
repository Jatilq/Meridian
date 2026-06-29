# Meridian Audit Log — Systematized Walkthrough

> Goal: verify every tab and setting actually does what it claims, fix bugs systematically, log everything for future agent review.

**Audit date:** 2026-06-28
**Scope:** Every visible page in `src/modules/*/pages/*.vue` + their Tauri command payloads in `src-tauri/src/*.rs`.

---

## Methodology

1. Read the page template + script
2. Trace every `invoke()` call to its Tauri command
3. Check the Rust command actually exists, returns what the page expects, and handles errors
4. Find the settings the page reads from `user-settings` and verify they're persisted
5. Test the round trip: user action → invoke → backend response → UI update
6. Fix bugs in place
7. Re-validate with `cargo check` + `vue-tsc` + `cargo test`
8. Log everything here

---

## Page-by-page audit

### Backend Manager (`src/modules/backend-manager/pages/backend-manager.vue`) — 4 tabs

| Tab | Tauri commands used | Status |
|---|---|---|
| **Backends** | `detect_local_gpu_vendor`, `get_backend_status`, `download_backend`, `start_backend`, `stop_backend`, `probe_backend_api` | ✅ All commands exist and are wired up. Download → Install → Start → Test API flow verified. Port + modelPath persist via `setConfig` → `setUserSettingsStorage('meridian.backend', …)`. |
| **Models** | `list_gguf_models` | ✅ Works after Fix 1 (recursive `.gguf` walker via `walkdir::WalkDir` depth 6). E:\ai\Models\<vendor>\<size>\<file>.gguf all surface. |
| **RPC Slaves** | `launch_rpc_slave` | ✅ Reads from `meridian.sshConnections`, sends `creds` + `rpcCommand` to Rust, returns stdout. |
| **Omnix Models** | `scan_huggingface_cache` + `downloader_enqueue` | ❌→✅ **BROKEN → FIXED**. The "Get on HF" button used to enqueue `https://huggingface.co/<repo>` (an HTML page) so the downloader fetched the README instead of a model. Fixed by adding `hf_resolve_model_files` Tauri command that calls the HF API, filters siblings to `.onnx/.gguf/.bin/.safetensors/.pt`, sorts quantized-first, and returns concrete download URLs. The Vue now picks `files[0]` and enqueues that. |

### Cluster (`src/modules/cluster/pages/cluster.vue`)

| Element | Tauri commands | Status |
|---|---|---|
| Node cards | `get_local_hardware` / `get_remote_hardware` | ✅ Live hardware data refreshes every 30s. BLACK's RX 6900 XT now reports 16 GB VRAM (was 4 GB — fixed previously with Win32_VideoController CIM path). |
| Add Worker dialog | `check_node_status` + writes to `meridian.sshConnections` | ✅ Test Connection calls `check_node_status` with form creds. Save persists the connection. |
| Launch RPC Slave on BLACK | `launch_rpc_slave` | ✅ Wired. |

### Hardware (`src/modules/hardware/pages/hardware.vue`)

| Element | Status |
|---|---|
| Local hardware summary | ✅ `get_local_hardware` shows CPU, GPU, combined VRAM. |
| Search HuggingFace GGUF models | ✅ Button visible after Fix 3 (scroll). Calls `fetch('https://huggingface.co/api/models?search=gguf&full=false&limit=30')` + per-result `fetchModelFiles` to resolve concrete `.gguf` URLs. |
| Per-result Download | ✅ Calls `invoke('downloader_enqueue', { url, file_name, format_id: null, auto_save_folder, chunk_count: null })`. Each result is the actual model file URL, not a page URL. |

### Settings → Meridian category (5 sub-pages)

| Sub-page | Persisted keys | Status |
|---|---|---|
| `ai-panel.vue` | `meridian.aiPanel.{endpointUrl, model, omnixEnabled, routerEndpoint, ttsEnabled, systemPrompt, temperature, maxTokens, topP}` | ✅ All 9 settings persist via `setUserSettingsStorage`. Model dropdown auto-fetches on mount via `aiPanelStore.fetchModels()`. Tool-capable heuristic warns when a non-Qwen/Llama-3.1+/GPT-4 class model is selected. |
| `downloader.vue` | `meridian.downloader.autoSaveFolder` | ✅ One input, persists immediately. |
| `files.vue` | `meridian.modelsFolder` | ✅ Text input + Browse button (Tauri `open({ directory: true })`) + Clear button. Flows to Backend Manager → Models tab and Hardware tab. |
| `ssh-connections.vue` | `meridian.sshConnections[]` | ✅ Add/remove/auth toggle work. Passwords go to secure-keys.json via `storeSshPassword(plain, existingKey?)` — never persisted in plaintext to the main user-settings blob. The "Encrypted" badge appears when `passwordSecureKey` is set. |
| `index.vue` | (renderer only) | ✅ Just iterates the section components. |

---

## Bugs found and fixed this audit

### Fix #1 — "Get on HF" enqueued the HTML page URL (not a model)

**Symptom:** Clicking "Get on HF" on the Omnix Models tab added an HTML page URL to the downloader queue. The downloader fetched the README instead of an actual `.onnx`/`.gguf` model.

**Root cause:** `downloadOmnixModel` in `backend-manager.vue` built the URL as `https://huggingface.co/${entry.modelID}` (the repo's HTML page) and enqueued it. There was no code path that resolved a repo ID to a concrete file URL.

**Fix:** New Tauri command `hf_resolve_model_files(repo_id: String) -> Result<Vec<HfModelFile>, String>` in `src-tauri/src/backend_manager.rs`. Calls `https://huggingface.co/api/models/<repo_id>`, filters the `siblings` array to `.onnx / .gguf / .bin / .safetensors / .pt`, builds the download URL as `https://huggingface.co/<repo>/resolve/main/<filename>`, sorts quantized-first via the `hf_quant_score` helper (q4/int4 = +100, q5 = +70, q6 = +60, q8/int8 = +50, q3 = +45, q2 = +40, fp16/f16/bf16 = +30, fp32/f32 = +10; tie-break by filename length then alpha), and returns the ranked list. The Vue picks `files[0]` and enqueues that via the existing `downloader_enqueue` Tauri command. The note now shows "Queued <filename> from <repo>" instead of "open the HF page and pick an asset yourself".

**Files touched:**
- `src-tauri/src/backend_manager.rs` — `HfModelFile` struct + `hf_resolve_model_files` + `parse_hf_siblings` helper + `hf_quant_score` helper + 9 new tests (3 quant scoring + 5 parse-helper + 1 async empty-repo)
- `src-tauri/src/lib.rs` — registered the new command
- `src-tauri/Cargo.toml` — added `"json"` to reqwest features; added `"time"` to main + dev tokio features
- `src/modules/backend-manager/pages/backend-manager.vue` — added `HfModelFile` interface, replaced `downloadOmnixModel` body

**Validation:**
- `cargo check` exit 0 (11 pre-existing warnings, none new)
- `cargo test --lib backend_manager::` exit 0 — 29 tests pass (was 20)
- `vue-tsc --build` exit 0
- Code reviewer approved the single-budget timeout refactor and the test that actually exercises the function

**Defensive design:**
- 10-second `tokio::time::timeout` wraps the entire HTTP round-trip — a stalled connect, header read, OR body read all share the same wall-clock budget
- "HuggingFace did not respond within 10s — check your connection and try again" error message is user-readable
- Empty / missing / "use-text-model" / `internal: true` repo IDs are all rejected with a clear message
- Synthetic JSON tests cover the parse + sort + URL-build logic without network access

### Fix #3 (already shipped) — Scrolling cutoff on every list panel

**Root cause:** `max-height: calc(100vh - NNNpx)` on the inner lists created a max-height that was usually LARGER than the actual remaining space inside the page. The page's `overflow: hidden` clipped the section's overflow before `overflow-y: auto` could engage. The user could reach the last card (e.g. Lemonade) but the bottom of the card (port input, model input, Download/Start/Stop buttons) was unreachable.

**Fix:** Removed the `max-height` cap from `.bm__section`, `.cluster__nodes`, `.hardware__models`. Switched all page roots to `display: flex; flex-direction: column; flex: 1; min-height: 0; overflow: hidden;` so `flex: 1` claims the leftover space naturally and the inner list's `flex: 1; min-height: 0; overflow-y: auto;` becomes the only scroll container.

**Files touched:**
- `src/modules/backend-manager/pages/backend-manager.vue` — `.bm` + `.bm__section` CSS
- `src/modules/cluster/pages/cluster.vue` — `.cluster` + `.cluster__nodes` CSS
- `src/modules/hardware/pages/hardware.vue` — `.hardware` (restructured to flex column) + `.hardware__models` CSS

---

## Settings verified to persist

Every meridian.* setting below has a corresponding `setUserSettingsStorage` write path, a default in `user-settings.ts`, and a migration entry in `user-settings.ts::migrateUserSettingsStep` if added after schema v1.

| Setting | Storage key | Default | Persists? |
|---|---|---|---|
| AI: local AI server URL | `meridian.aiPanel.routerEndpoint` | `http://localhost:11434/v1` | ✅ |
| AI: model | `meridian.aiPanel.model` | `''` | ✅ |
| AI: enable Omnix | `meridian.aiPanel.omnixEnabled` | `true` | ✅ |
| AI: speak responses (TTS) | `meridian.aiPanel.ttsEnabled` | `false` | ✅ |
| AI: system prompt | `meridian.aiPanel.systemPrompt` | (Rain default prompt) | ✅ |
| AI: temperature | `meridian.aiPanel.temperature` | `0.7` | ✅ |
| AI: max tokens | `meridian.aiPanel.maxTokens` | `1024` | ✅ |
| AI: top-p | `meridian.aiPanel.topP` | `1` | ✅ |
| Downloader: auto-save folder | `meridian.downloader.autoSaveFolder` | `''` (auto-detect on first run) | ✅ |
| Files: models folder | `meridian.modelsFolder` | `''` | ✅ |
| SSH: connections list | `meridian.sshConnections[]` | `[]` | ✅ |
| Backend: per-kind port + modelPath | `meridian.backend.<kind>.{port, modelPath}` | `{}` | ✅ |
| SSH passwords | (secure-keys.json) | n/a | ✅ Encrypted via `secure_store_secret` |

---

## Tests added this audit

- `parse_hf_siblings_filters_to_model_extensions` — README/config/tokenizer dropped, .safetensors kept, URL format asserted
- `parse_hf_siblings_sorts_quantized_first` — 5-file input asserts exact order q4 > q5 > q8 > fp16 > fp32
- `parse_hf_siblings_accepts_all_supported_extensions` — .onnx / .gguf / .bin / .safetensors / .pt all pass
- `parse_hf_siblings_carries_size_bytes_when_present` — HF API's `size` field surfaces as `size_bytes` Option
- `parse_hf_siblings_rejects_response_without_siblings` — missing field errors instead of panics
- `hf_quant_score_prefers_q4_over_fp16_over_fp32` — ordering invariant
- `hf_quant_score_is_case_insensitive` — HF filenames vary in case
- `hf_quant_score_matches_int4_and_int8` — ONNX repos use int4/int8 not q4/q8
- `hf_resolve_model_files_rejects_empty_repo` — async test, real assertion (the empty check short-circuits before any network call so it's deterministic)

---

## Known issues deferred (out of scope this turn)

- **0% util / 0°C on AMD Windows** — `nvidia-smi` and `rocm-smi` don't run on Windows. WMI doesn't surface either. Needs ADL SDK or a `windows` crate binding for `D3DKMDT_VIDEO_PRESENT_SOURCE_STATE`. Deferred.
- **Top-level page padding-bottom** on `cluster.vue` / `backend-manager.vue` could let the last card's footer touch the section's bottom edge with no breathing room — cosmetic, not a bug.
- **SFTP `key_path` empty-string coercion** in `sftp.rs::SftpCredentials` — `key_path: String` is required but the front-end `SshCredentials.keyPath: Option<String>` is now optional. The sftp crate errors with a less-informative message if `key_path = ""`. The fix is to return a clearer error from the backend when keyPath is `None`. Deferred.
- **Two other panels with the same `max-height: calc(100vh - NNNpx)` pattern still have the latent bleed-past-overflow-hidden bug**:
  - `src/modules/nav-sidebar/components/quick-access-panel.vue:401`
  - `src/modules/navigator/components/info-panel/info-panel.vue:167`
  - `src/modules/settings/ui/nav.vue:34`
  These don't currently have the `flex: 1; min-height: 0; overflow: hidden;` parent refactor that the 3 fixed pages got. Will bite the user eventually.

---

## Validation summary

| Check | Result |
|---|---|
| `cargo check` (src-tauri) | exit 0 — 11 pre-existing warnings, none new |
| `cargo test --lib backend_manager::` | exit 0 — 29 tests pass |
| `vue-tsc --build` | exit 0 — no type errors |
| Code reviewer (round 1) | flagged missing reqwest `"json"` feature, no request timeout, no integration tests |
| Code reviewer (round 2) | flagged 20s worst-case timeout, no-op tokio test |
| Code reviewer (round 3) | flagged no-op sync test (input property, not function) |
| Code reviewer (round 4, final) | approved |

---

## Round-4 / Round-5 followup fixes (the 3 followups from the "every setting 40% done" audit)

The 3 followups suggested after the initial audit were all the same latent layout bleed bug the audit identified as deferred. All three are now fixed.

### Fix A — `src/modules/nav-sidebar/components/quick-access-panel.vue` (round-4 + round-5 cascade order)

- **Round-4:** `.quick-access-panel` had `--max-height: calc(100vh - 12px - var(--tooltip-height))` — a viewport-based cap that ignored the window toolbar and the popover's actual positioned context. Updated to subtract `var(--window-toolbar-height, 0px)` so the popover never bleeds under the app header. Inner `.quick-access-panel__scroll` cap (`calc(var(--max-height) - var(--header-height))`) unchanged and still propagates via `:deep(.sigma-ui-scroll-area__viewport) { max-height: inherit; }`.
- **Round-5 polish:** the two-decl `100dvh` / `100vh` cascade shipped in round-4 was in the WRONG order (last-declaration-wins). As shipped, `100vh` was overriding `100dvh` on mobile webview, making the popover *more* oversized. Flipped to `100vh` first (legacy fallback) then `100dvh` second (modern unit, wins on mobile). Both evaluate identically in Tauri desktop.

### Fix B — `src/modules/navigator/components/info-panel/info-panel.vue` compact drawer (round-4 + round-5 cascade order)

- **Round-4:** `.info-panel-compact-drawer` had `height: min(65vh, calc(100vh - var(--window-toolbar-height) - 8px))` and `max-height: calc(100vh - var(--window-toolbar-height) - 8px)`. Same cascade fix applied.
- **Round-5 polish:** flipped the `100dvh` / `100vh` two-decl cascade in both `height` and `max-height` properties. Comments updated to explain the last-declaration-wins rule.

### Fix C — `src/modules/settings/ui/nav.vue` (round-4)

- Dropped `position: sticky`, `align-self: start`, and the `max-height: calc(100vh - var(--window-toolbar-height))` cap. The nav is a grid item in `.settings-content__inner` which has `height: 100%`, so the previous cap was the same kind of bleed bug we fixed on the inner pages. Now: `display: flex; flex-direction: column; align-self: stretch; max-height: 100%; min-height: 0; padding-right: 1rem; border-right: 1px solid hsl(var(--border)); gap: 1rem; overflow-y: auto;`. The mobile `@media` block was already in the correct state (`max-height: none; padding-right: 0; border-right: none; overflow-y: visible`) — on mobile the nav stacks vertically and flows naturally.

### Fix D — Schema 25 → 26 purge developer's home-lab SSH connections (round-4 + round-5)

- **Round-4:** Bumped `USER_SETTINGS_SCHEMA_VERSION` from 25 to 26 in `src/stores/schemas/user-settings.ts`. Added `if (fromVersion === 25 && toVersion === 26)` migration that filters persisted SSH connections matching the developer's home lab (MAMBA / BLACK). Initial draft matched on `(host ∈ DEV_HOSTS) && (username ∈ DEV_USERS || username === '')`.
- **Round-5 polish:** removed the over-broad empty-username fallback branch (a user filling in SSH settings incrementally would have a half-typed connection vanish on next launch). Narrowed the filter to the exact `(host, username)` pair: `host ∈ {192.168.1.67, 192.168.1.64}` AND `username === 'jatilq'`. Added a `console.info` audit line that fires only when at least one dev-lab connection was actually purged — reports the count and the deduplicated list of dropped host strings. No credentials in the log. Idempotent. Migration chain intact (21→22, 22→23, 23→24, 24→25, 25→26 all defined).

## Round-4 + Round-5 validation summary

| Check | Result |
|---|---|
| `vue-tsc --build` | exit 0 (TYPECHECK_OK) |
| `cargo check` (src-tauri) | exit 0 (CARGO_CHECK_OK) — 11 pre-existing warnings, none new |
| CSS cascade grep on quick-access-panel.vue + info-panel.vue | CASCADE_OK (100vh first, 100dvh second in both files) |
| Code reviewer (round 4) | flagged 3 critical findings (CSS cascade order, silent purge, over-broad filter) |
| Code reviewer (round 5) | approved (all 3 findings addressed) |

---

## Files touched this audit (commit-ready)

```
src-tauri/Cargo.toml                                              (+json +time features)
src-tauri/src/backend_manager.rs                                  (hf_resolve_model_files + 9 tests)
src-tauri/src/lib.rs                                              (one-line invoke_handler registration)
src/modules/backend-manager/pages/backend-manager.vue            (downloadOmnixModel rewrite + interface, Fix 3 scroll)
src/modules/cluster/pages/cluster.vue                            (Fix 3 scroll)
src/modules/hardware/pages/hardware.vue                           (Fix 3 scroll)
src/modules/nav-sidebar/components/quick-access-panel.vue        (Fix A: popover cap + cascade order)
src/modules/navigator/components/info-panel/info-panel.vue       (Fix B: compact drawer cap + cascade order)
src/modules/settings/ui/nav.vue                                  (Fix C: drop sticky + viewport cap, use 100% grid-item cap)
src/stores/schemas/user-settings.ts                               (Fix D: schema 25→26 + dev-lab SSH purge migration)
```

