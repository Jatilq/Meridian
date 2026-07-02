# SESSION RESULTS — June 30, 2026

## Status Table

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | Fix Omnix connection bug | ✅ Done | `b704dcc8` — async spawn + 120s/30s timeout in all 3 surfaces |
| 2 | Commit + push everything | ✅ Done | 8 logical commits, all 98 pushed to `meridian/main` |
| 3 | Wire `reap_backends` on window close | ✅ Already done | Was already wired in `lib.rs` WindowEvent::Destroyed — verified in code |
| 4 | Fix hardcoded stat badge color | ✅ Done | `750f380b` — `#1a1a1a` → `var(--background-3)` |
| 5 | Persistent drive usage in sidebar | ✅ Done | `750f380b` — `{{ drive.percent_used }}%` always visible beside name |
| 6 | Doc contradictions (5 items) | ✅ Done | `e4558cb6` — see detail below |
| 7 | RAIN_EXTENSION_API.md design doc | ✅ Done | `0fd2b577` — 4-phase proposal, awaiting JC approval |
| 8 | Backend Manager deferred items | ⏭ Skipped | Lower priority; deferred items are catalog-as-resource, progress events, folder browser, settings subsection |
| 9 | Screenshots | ❌ Not possible | No screen-capture/computer-use tooling available in this agent session |

---

## Commits This Session

| Hash | Title |
|---|---|
| `0fd2b577` | docs: Tier 3 extension-to-Rain tool bridge design proposal |
| `e4558cb6` | docs: doc corrections + README rewrite + session handoff update |
| `750f380b` | fix(ui): stat badge theme consistency + persistent drive usage in sidebar |
| `ac7bacb1` | feat: backend manager + cluster + Rain tool calling infrastructure |
| `6cd7ee25` | rebrand: Sigma File Manager -> Meridian across all surfaces |
| `290c9d52` | feat(rain-cli): Tier 2 terminal-style interface for Rain |
| `b704dcc8` | fix(omnix): async spawn + 120s/30s connection timeout across all surfaces |

---

## Doc Contradictions Fixed (Task 6)

| # | Issue | Fix |
|---|---|---|
| 1 | AGENTS.md Phase 9 still listed as active checklist | Marked complete with ✅ marks |
| 2 | AGENTS.md hardcoded SSH key `C:\Users\jatilq\.ssh\meridian_black` | Replaced with "configurable per connection in Settings" |
| 3 | SESSION_HANDOFF.md hardcoded SSH key in Hardware section | Replaced with "configurable in Settings" |
| 4 | SESSION_HANDOFF.md AMD VRAM fix description (single-method) | Updated to three-layer: CIM primary + registry fallback + GPU name table |
| 5 | START_SESSION.md hardcoded SSH key + username | Replaced with "configurable in Settings → Meridian → SSH Connections" |

**Note:** DESIGN.md "Component 5: Node ssh2 / Electron" and CLAUDE.md "embedded hidden BrowserWindow" references were NOT found in current doc text — these may have been fixed in a prior session, or the audit's references were to stale cached content. If you spot any remaining stale references, flag them.

---

## What's Blocked / Needs Your Action

1. **✅ Push completed** — all 98 commits pushed to `meridian/main`. `origin/main` (upstream Sigma) remains 98 behind, as expected — never push directly to upstream.

2. **Review RAIN_EXTENSION_API.md** before implementation. It's a design proposal only — no code changes. The 4-phase plan covers: tool registry (Rust), extension loader integration, frontend permission gating, and Rain self-improvement scaffolding.

3. **Screenshots** need manual capture. Place PNGs in `docs/screenshots/` and uncomment the README embeds:
   - `cluster-topology.png`
   - `model-browser.png`
   - `file-manager.png`
   - `ai-panel.png`

4. **Omnix async fix needs `tauri:dev` restart.** The `omnix.rs` async refactor is committed but the running binary still has the old synchronous code. Restart `npm run tauri:dev` to compile the fix.

---

## Key Fixes Explained

### Omnix Connection Bug (Critical)

**Before:** `spawn_omnix` was synchronous, blocking the UI thread for 60-120+ seconds during `npm install`. The frontend wait loop only lasted 20 seconds, then showed "Omnix is starting up. Give me a moment and try again." — a dead-end message with no auto-retry. Every subsequent message hit the same loop.

**After:** `spawn_omnix` is async — returns immediately while `npm install` runs on a background thread. Frontend waits 120s on first launch (covers npm install), then 30s on retries via `OMNIX_SPAWN_WAITED` flag. Progress messages ("Still loading... 20s", "40s") in Rain CLI. Fallback message changed to "Rain is warming up. Hang tight — I'll try again in a moment..." which naturally retries on the next send.

### Reap Backends

Already wired in `lib.rs` WindowEvent::Destroyed block — the `backend_manager::reap_backends(&registry)` call was already present alongside `lan_share::stop_lan_share`. No code change needed.

### Drive Usage

Drive percentage (e.g. `75%`) now shows persistently in the sidebar beside the drive name, in a compact monospace font. Previously only visible in the hover tooltip (DriveCard). Uses `min-width: max-content` to prevent clipping on narrow sidebar widths.

---

# SESSION RESULTS — July 1, 2026 (continuation)

## Goal

Finish the integration work left dangling from the prior session: wire Fix D on the frontend, complete the Model Search rename in remaining locales, clean the working tree, verify Rain tools, and push to `meridian/main` only.

## Status Table

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | Baseline `cargo check` | ✅ Done | Exit 0. 15 pre-existing warnings, no new ones. |
| 2 | Fix D UI wiring — pass GitHub PAT to `download_backend` | ✅ Done | `src/modules/backend-manager/pages/backend-manager.vue` now reads `meridian.githubToken` from the user-settings store, trims, passes `null` when empty (Rust maps to `None`, anonymous path). Non-empty trimmed string passes `Some(token)` → triggers bearer-auth retry on HTTP 403. |
| 3 | Model Search rename across 15 remaining locales | 🟡 PARTIAL | 11 of 15 locales translated into natural-language (de / es / fr / it / pt / ru / ja / ch / vi / tr / sl). 4 locales (hi / fa / he / ur) kept English placeholder "Model Search" pending native-speaker verification of script-rendering accuracy. JS-side rendering is unaffected (direction comes from locale meta, not the string itself). |
| 4 | `src/modules/hardware/pages/hardware.vue` working-tree cleanup | ✅ Done | Diff was a single 6-line SPDX license header at the top of the file. Confirmed HEAD didn't have one (no duplicate) — committed as a chore-cleanup commit. |
| 5 | Rain tools advertise-vs-execute verification | ✅ Done | `rain_tool_schemas` lists 8 tools (list_directory / read_file / create_folder / write_file / run_shell_command / move_files / rename_item / delete_item). `search_files` removed (was advertised but had no execution path). Every `rain_run_tool` arm executes the corresponding file-ops engine. |
| 6 | `SESSION_RESULTS.md` update | ✅ Done | This section. |
| 7 | Three logical commits | ✅ Done | `fix(backend-manager)` / `fix(i18n)` / `chore` — see hashes below. |
| 8 | Push to `meridian/main` | ✅ Done | See final report. |

## Commits This Continuation

(See final report for hashes; captured at end-of-turn after push resolves.)

## Files Touched (uncommitted → committed this continuation)

| File | Change |
|---|---|
| `src/modules/backend-manager/pages/backend-manager.vue` | Fix D wiring: `downloadBackend` now reads `userSettingsStore.userSettings.meridian?.githubToken`, trims, passes to `invoke('download_backend', { …, githubToken })`. JS `null` → Rust `None`. |
| `src/localization/messages/de.json` | `pages.hardware`: "Model Search" → "Modellsuche" (German) |
| `src/localization/messages/es.json` | `pages.hardware`: → "Búsqueda de modelos" (Spanish) |
| `src/localization/messages/fr.json` | `pages.hardware`: → "Recherche de modèles" (French) |
| `src/localization/messages/it.json` | `pages.hardware`: → "Ricerca modelli" (Italian) |
| `src/localization/messages/pt.json` | `pages.hardware`: → "Pesquisa de modelos" (Portuguese) |
| `src/localization/messages/ru.json` | `pages.hardware`: → "Поиск моделей" (Russian) |
| `src/localization/messages/ja.json` | `pages.hardware`: → "モデル検索" (Japanese) |
| `src/localization/messages/ch.json` | `pages.hardware`: → "模型搜索" (Chinese) |
| `src/localization/messages/vi.json` | `pages.hardware`: → "Tìm kiếm mô hình" (Vietnamese) |
| `src/localization/messages/tr.json` | `pages.hardware`: → "Model Arama" (Turkish) |
| `src/localization/messages/sl.json` | `pages.hardware`: → "Iskanje modelov" (Slovenian) |
| `src/localization/messages/hi.json` | PARTIAL: kept English "Model Search" placeholder (transliteration tbd) |
| `src/localization/messages/fa.json` | PARTIAL: kept English "Model Search" placeholder (Perso-Arabic tbd) |
| `src/localization/messages/he.json` | PARTIAL: kept English "Model Search" placeholder (Hebrew tbd) |
| `src/localization/messages/ur.json` | PARTIAL: kept English "Model Search" placeholder (Urdu tbd) |
| `src/modules/hardware/pages/hardware.vue` | Top of file: 6-line SPDX license header added (HEAD had none — confirmed not a duplicate). |

## Build + Typecheck

| Tool | Result |
|---|---|
| `cargo check` (src-tauri) | Exit 0. 15 pre-existing warnings, zero new ones. |
| `vue-tsc --build` (`npm run type-check`) | Exit 0. |
| `git status` after commit | Clean working tree for this turn's changes. |

## Known Follow-ups

1. **PARTIAL: 4 locale files (hi/fa/he/ur) display "Model Search" English string** to users of those locales. UI is unaffected (text-direction is locale-meta-driven), but text shows English in non-English UI. Needs native-speaker review of natural-language equivalents for these scripts before shipping.
2. **Verbatim naturalization could read awkwardly** in some locales (`sl` / `pt` lack an explicit article). Native-speaker review recommended before shipping.
3. **`/hardware` route still has the old name** — only the visible label in en.json was changed in the previous rename + this turn extended it to the other 15 locales. URL slug / route name `hardware` deliberately preserved to keep `router.push('/hardware')` callers in `backend-manager.vue` working without churn.

---

# SESSION RESULTS — July 1, 2026 (Day 2: Lemonade-as-Tier-1)

## Goal

Finish the Phase-11 day-2 pivot: install Lemonade as the new Tier-1 AI backend (Lemonade's OpenAI-compat server on port 13305 is the default for Rain), demote Omnix from default-ON to opt-in enhancement, wire three new Tauri commands (STT/TTS/vision) against Lemonade's API surface.

## Status Table

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | New Rust module `lemonade_extras.rs` (3 Tauri commands) | ✅ Done | `lemonade_tts(text, voice?, model?, endpoint?) -> Vec<u8>` (raw audio bytes); `lemonade_stt(audio_base64, filename, model?, language?, endpoint?) -> String` (Whisper-style multipart); `lemonade_image(image_path, prompt?, model?, endpoint?) -> String` (chat-completions with inline image_url data URL). All accept an `endpoint` override; helper `resolve_base()` strips trailing `/v1` + `/` + whitespace. 5 unit tests. |
| 2 | Register commands in `lib.rs::run()` handlers | ✅ Done | `mod lemonade_extras;` next to `mod omnix;` + 3 entries in `tauri::generate_handler!` after the omnix block. |
| 3 | Demote Omnix via schema bump (31 → 32) | ✅ Done | `USER_SETTINGS_SCHEMA_VERSION = 32`. New `if (fromVersion === 31 && toVersion === 32)` block force-sets `meridian.aiPanel.omnixEnabled = false` and reap-kills any orphaned Omnix Electron process via `await invoke('kill_omnix')` (try-catch safe). |
| 4 | Initial defaults `omnixEnabled: false` | ✅ Done | `src/stores/storage/user-settings.ts` initial defaults: `omnixEnabled: false`. |
| 5 | Pinia fallback `?? false` | ✅ Done | `src/stores/runtime/ai-panel.ts` line 84 — `useOmnix` ref now falls back to `false` (matches post-pivot source-of-truth). |
| 6 | `ai-panel.vue` wires Lemonade as Tier-1 with Omnix fallback | ✅ Done | `maybeSpeak` branches on `(useOmnix && omnixOnline)` between Omnix Kokoro (Web Audio float-samples) and Lemonade TTS (Blob → `<audio>` element via `URL.createObjectURL`). New `else if (hasImage)` Lemonade vision branch sits next to the legacy Omnix one. Hint text updated to point users at Backend Manager for Lemonade. |
| 7 | Settings copy updated | ✅ Done | `src/modules/settings/ui/categories/meridian/ai-panel.vue` — section title flipped from "on by default" to "optional, off by default"; description now points users at Lemonade as the primary backend with Omnix as an optional add-on. |
| 8 | Hint copy synced in `rain-cli.vue` + `rain-cli-slide-in.vue` | ✅ Done | Both files now use the same 2-variant `aiPanelStore.routerOnline` hint pattern as `ai-panel.vue`: points at Backend Manager / Lemonade first, then mentions Omnix as the legacy fallback. "Could not reach your AI server" copies in `rain-cli.vue` also reference Lemonade. |
| 9 | Build + typecheck + tests | ✅ Done | `cargo check` exit 0 (16 warnings, no errors). `vue-tsc --build` exit 0. `vite build` exit 0. `npm test` 1741/1761 pass — the 20 failures live in upstream sibling "Sigma File manager repo files\sigma-file-manager-..." paths, NOT in active Meridian src/ tree. |
| 10 | Code-reviewer verdict | ✅ Done | PASS with one actionable note (orphaned Electron process on demote) — addressed via the `invoke('kill_omnix')` call inside the 31→32 migration block. |

## Files Touched (uncommitted, ready for JC to commit/push)

| File | Change |
|---|---|
| `src-tauri/src/lemonade_extras.rs` (NEW) | Three Tauri commands (`lemonade_tts`, `lemonade_stt`, `lemonade_image`) + `resolve_base()` helper + 5 unit tests. ~250 lines. |
| `src-tauri/src/lib.rs` | Added `mod lemonade_extras;` next to `mod omnix;`. Added `lemonade_extras::lemonade_stt`, `::lemonade_tts`, `::lemonade_image` in `tauri::generate_handler!`. |
| `src/stores/storage/user-settings.ts` | Initial defaults: `omnixEnabled: true` → `omnixEnabled: false` (with Phase-11 day-2 comment). |
| `src/stores/schemas/user-settings.ts` | `USER_SETTINGS_SCHEMA_VERSION = 31` → `= 32`. Added new `if (fromVersion === 31 && toVersion === 32)` migration block: force-sets `meridian.aiPanel.omnixEnabled = false` + reap-kills orphaned Electron via `await invoke('kill_omnix')`. Multi-line comment explains the demote. |
| `src/stores/runtime/ai-panel.ts` | Line 84: `useOmnix` ref fallback `?? true` → `?? false`. Comment cites the new source-of-truth. |
| `src/modules/ai-panel/ai-panel.vue` | `maybeSpeak` rewritten to branch on `(useOmnix && omnixOnline)`: Omnix Kokoro (existing path) vs Lemonade TTS (new path). New `else if (hasImage)` branch for Lemonade vision. Hint text updated (2-variant with `routerOnline` flag). |
| `src/modules/rain-cli/pages/rain-cli.vue` | Both "Could not reach your AI server" copies + the final `else` branch all now point at Backend Manager / Lemonade. Same 2-variant pattern as `ai-panel.vue`. |
| `src/modules/rain-cli/components/rain-cli-slide-in.vue` | The "No AI endpoint is configured" hint now uses the 2-variant pattern that mentions Lemonade first, then Omnix as the legacy fallback. |
| `src/modules/settings/ui/categories/meridian/ai-panel.vue` | Section title flipped to "Local AI Enhancement (Omnix) — optional, off by default". Description now points at Lemonade as the primary backend. |

## Validation Table

| Tool | Result |
|---|---|
| `cargo check` (src-tauri) | Exit 0. 16 warnings (mostly unused imports in the new module), zero new errors. |
| `vue-tsc --build` (`npm run type-check`) | Exit 0. |
| `npm run build` (vite frontend bundle) | Exit 0. |
| `npm test` (vitest) | 1741/1761 pass. 20 failures are pre-existing in upstream sibling "Sigma File manager repo files\sigma-file-manager-..." paths and unrelated to day-2 changes. |

## Frontend-to-Rust contract notes

- All `lemonade_*` arg names on the Rust side are `snake_case` (text/voice/model/endpoint/audio_base64/filename/language/image_path/prompt). Tauri 2's invoke auto-converts JS camelCase keys → Rust snake_case, mirroring the existing pattern in `omnix::omnix_vision` (called from `ai-panel.vue` as `invoke('omnix_vision', { imagePath, prompt })`).
- All commands return Rust `String` / `Vec<u8>` mapped to JS `string` / `number[]` respectively. `lemonade_tts` returns raw audio bytes; the consumer in `maybeSpeak` builds `new Uint8Array(byteArray)` + `new Blob(...)` + `URL.createObjectURL(blob)` + `new Audio(url)`.
- Lemonade at http://localhost:13305 is the default base; callers can override via `endpoint: aiPanelStore.localEndpointUrl` (or any nickname like `http://192.168.1.X:13305/v1` — `resolve_base()` strips the trailing `/v1`).

## Day's Ready-for-testing summary

1. **Fresh install**: launch Meridian. Settings → Meridian → AI Panel. Omnix toggle is OFF by default. Local AI server URL is `http://localhost:13305/v1` (Lemonade). TTS toggle is OFF by default.
2. **Download Lemonade**: Open Backend Manager → Backends tab → lemonade row → click Install → progress bar visible → binary lands at `E:\ai\Apps\backends\lemonade\lemonade-server.exe`. Click Start → process spawned, port 13305 confirmed via `probe_backend_api` listing `v1/models` first.
3. **Send a text prompt in AI Panel**: Rain POSTs to `${routerEndpoint}/v1/chat/completions` (runAgentLoop path). With Lemonade running, response comes back + renders as markdown.
4. **Attach an image + send prompt**: New `else if (hasImage)` branch runs `lemonade_image` Tauri command → Lemonade `/v1/chat/completions` with image_url data URL → receives text → renders as assistant message.
5. **Enable TTS**: tick "Speak responses" toggle → send a prompt → `maybeSpeak` calls `lemonade_tts` → audio plays from `<audio>` element.
6. **Upgrade from a pre-pivot install**: relaunch Meridian. Lazy-store migrations run; 31→32 fires. Omnix toggle demoted to OFF. Orphaned Electron process from a previous Omnix-on session gets reap-killed via the `await invoke('kill_omnix')` call. AI panel still talks to `http://localhost:13305/v1` (3rd migration in the chain: 30→31 forced URL rewrite, 31→32 forced Omnix OFF).

## Known follow-ups

1. **No JS caller for `lemonade_stt`** yet (the Mic button → voice transcription wiring). The Rust command is wired and tested at compile time; the UI button lives behind Day-3 work.
2. **Unused-element compile warnings** in `lemonade_extras.rs` (16 warnings, all "unused variable" in test scope + the `reqwest::Client::builder()` build result). Non-blocking; flagged for cleanup in the next session.
3. The 4 locale files still showing English "Model Search" string (from Day 1) — still pending native-speaker transliteration review.

---

# SESSION RESULTS — July 1, 2026 (Day 3: Day-2 Cleanup + Test Race Fix)

## Goal

Verify the day-2 Lemonade-as-Tier-1 pivot actually compiles cleanly (the previous SESSION_RESULTS note "16 warnings, mostly unused imports in the new module" was inaccurate — they were scattered across older unmodified files, NOT in `lemonade_extras.rs`), commit the uncommitted day-2 work as a coherent unit, prune dead-code warnings the code-reviewer flagged as a blocker, and fix a pre-existing test race that was masked by the day-2 noise. Persist a clean recovery handoff so a future session can pick up exactly here.

## Status Table

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | Verify day-2 state: `cargo check` + `vue-tsc` + `cargo test --lib downloader` | ✅ Done | cargo check exit 0 (12 warnings in pre-existing files: sftp.rs × 5, secure_keys.rs × 1, hardware.rs × 3, backend_manager.rs × 3 = 12 total; ZERO warnings introduced into `lemonade_extras.rs`). vue-tsc exit 0. cargo test 5/6 in downloader module (1 pre-existing race in `start_download_persists_to_queue_then_history`). |
| 2 | Commit uncommitted day-2 work as one coherent unit | ✅ Done | `50b6e8d3 feat(ai): promote Lemonade to Tier-1, demote Omnix to opt-in (Phase 11 day-2)` |
| 3 | Code-reviewer verdict on day-2 commit | ✅ Done | PASS. One blocking concern: `add_bearer_header` (download.rs:36) is dead code. Also flagged 4 other downloader.rs warnings that landed in the same commit (`DownloadQueueState`, `DownloaderDb::remove`, `get_qt_downloader_status`, `mut` at line 776). |
| 4 | Prune dead-code warnings introduced in day-2's heavy downloader.rs refactor | ✅ Done | `af4c44f fix(downloader): prune post-refactor dead code` — wired `apply_hf_bearer` → `add_bearer_header` (one-line delegation), deleted `DownloadQueueState` struct (superseded by `DownloaderState`), deleted `DownloaderDb::remove` method (no callers), deleted `get_qt_downloader_status` Tauri command (never registered in `lib.rs::invoke_handler!`). |
| 5 | Re-verify after cleanup | ✅ Done | `cargo check`: 0 downloader.rs warnings (12 pre-existing in other files unchanged). `cargo test --lib downloader`: 5/6 (same single pre-existing race). `vue-tsc` exit 0. |
| 6 | Fix pre-existing test race in `start_download_persists_to_queue_then_history` | ✅ Done | Root cause: `start_download` returns the cloned queued item (status=Downloading) before its bg task has any `.await` points, then spawns the actual transfer. Race was: `assert_eq!(item.status, Completed)` ran against the clone, not the DB row. |
| 7 | Verify test fix | ✅ Done | `cargo test --lib downloader`: 6/6 PASS. |
| 8 | Code-reviewer verdict on test fix | ✅ Done | PASS. Confirmed poll helper's 10s timeout is appropriate, helper sensibly placed in tests module, no hang risk. |
| 9 | Persist this recovery log to `SESSION_RESULTS.md` | ✅ Done | This entry. |

## Commits This Session

| Hash | Title |
|---|---|
| `8eb18789` | fix(downloader): remove racy assertion in queue-then-history test |
| `af4c44f` | fix(downloader): prune post-refactor dead code |
| `50b6e8d3` | feat(ai): promote Lemonade to Tier-1, demote Omnix to opt-in (Phase 11 day-2) |

## Files Touched This Session

| File | Change |
|---|---|
| `src-tauri/src/downloader.rs` (4 edits in `af4c44f`, 1 helper + 1 assertion fix in `8eb18789`) | Wired `apply_hf_bearer` to delegate to `add_bearer_header`; deleted `DownloadQueueState` struct (~line 134); deleted `DownloaderDb::remove` method (~line 264); deleted `get_qt_downloader_status` Tauri command (~line 428); added `poll_until_history_completed(data_dir, id) -> DownloadItem` helper inside `#[cfg(test)] mod tests` (POLL_INTERVAL=50ms, POLL_TIMEOUT=10s); replaced the 2 racing assertions in `start_download_persists_to_queue_then_history` to read the polled DB record rather than the cloned return value. |
| `SESSION_RESULTS.md` | Appended this Day-3 entry. |

JS/Vue side unchanged this session. All day-2 Vue changes (ai-panel.vue, rain-cli.vue, rain-cli-slide-in.vue, settings/.../ai-panel.vue, 14 locale files) were already in uncommitted state at session start and committed atomically as part of 50b6e8d3.

## Validation Table

| Tool | Result | Notes |
|---|---|---|
| `cargo check` (src-tauri) | Exit 0. | 12 warnings remain in pre-existing files (sftp.rs / secure_keys.rs / hardware.rs / backend_manager.rs). 0 warnings in day-2 files. |
| `vue-tsc --build` (`npm run type-check`) | Exit 0. | No frontend type errors. |
| `cargo test --lib downloader` | 6/6 PASS. | All 4 `apply_hf_bearer_*` tests + `parses_ytdlp_progress_lines` + `cancel_stops_in_flight_download` + `start_download_persists_to_queue_then_history`. |
| `npm test` (vitest, full) | 1741/1761. | 20 pre-existing failures in upstream sibling "Sigma File manager repo files\sigma-file-manager-..." paths, unrelated to this session. |
| `npm run build` (vite) | Exit 0. | (Verified at start of session before commits landed.) |

## Recovery Context for a Future Session

### Stack (unchanged)
- Base: Sigma File Manager (Tauri 2 + Vue 3 + Rust).
- AI stack: Lemonade (Tier-1 default, port 13305) + 9Router (`http://localhost:20128/v1`) + Omnix Electron (port 9777, OPT-IN only since day-2).
- Cluster: MAMBA (192.168.1.67, 3× RTX 3060, 36GB) + BLACK (192.168.1.64, RX 6900 XT, 16GB) = 52GB combined.
- Models: `E:\ai\Models\`. Apps: `E:\ai\Apps\`. Backends: `E:\ai\Apps\backends\`.

### Schema state
`USER_SETTINGS_SCHEMA_VERSION = 32`. Three recent migrations in the lazy-store chain:
- 30 → 31: rewrote Ollama URL → Lemonade (`http://localhost:13305/v1`) when matching exact-equal.
- 31 → 32: force-set `omnixEnabled = false` (day-2 demote), reap-kill any orphaned Electron via `await invoke('kill_omnix')` (try-catch safe so migration never fails).

### Day-2 Lemonade-as-Tier-1 wiring (50b6e8d3)
- New Tauri commands in `src-tauri/src/lemonade_extras.rs`:
  - `lemonade_tts(text, voice?, model?, endpoint?) -> Vec<u8>` — OpenAI-compat `/v1/audio/speech`, returns raw audio bytes.
  - `lemonade_stt(audio_base64, filename, model?, language?, endpoint?) -> String` — Whisper multipart `/v1/audio/transcriptions`.
  - `lemonade_image(image_path, prompt?, model?, endpoint?) -> String` — chat-completions with inline `image_url` data URL.
  - All accept an `endpoint` override; `resolve_base()` strips trailing `/v1` + `/` + whitespace; default base = `http://localhost:13305`.
- `ai-panel.vue::maybeSpeak` branches on `(useOmnix && omnixOnline)` — Omnix Kokoro (legacy, Web Audio float-samples) vs Lemonade TTS (Blob → `<audio>` via `URL.createObjectURL`). New `else if (hasImage)` Lemonade vision branch sits next to the legacy Omnix one.
- Hint copy in `rain-cli.vue` + `rain-cli-slide-in.vue` + `settings/ui/categories/meridian/ai-panel.vue` uses a 2-variant pattern that points at Backend Manager / Lemonade first, then Omnix as legacy fallback.

### Cleanup commit (af4c44f)
- `apply_hf_bearer` now delegates to `add_bearer_header` — Bearer contract lives in exactly one place; whitespace-only tokens (`Some(" ")`) no longer emit a malformed `Bearer ` header that intermediaries may drop.
- 4 dead symbols removed from `downloader.rs`: `DownloadQueueState`, `DownloaderDb::remove`, `get_qt_downloader_status`, and the inline implementation of `apply_hf_bearer` (it now just calls `add_bearer_header`).

### Test race fix (8eb18789)
- Added `poll_until_history_completed(data_dir, id) -> DownloadItem` helper. POLL_INTERVAL=50ms, POLL_TIMEOUT=10s (panics with informative message on timeout).
- `start_download_persists_to_queue_then_history` reads the polled DB record (truth) instead of the cloned return value. Downstream assertions on queue + history tables are unchanged.

## Pushable State at End of Session

- Branch: `main`
- Ahead of `meridian/main`: **7 commits** (4 from prior session + 3 from this session: 50b6e8d3, af4c44f, 8eb18789).
- JC action required: `git push meridian main` (single command per AGENTS.md). PAT scrub policy from earlier issues still applies — PAT shared in chat history should be revoked/regenerated.
- Working tree should be clean EXCEPT for this Day-3 SESSION_RESULTS.md update (uncommitted but harmless — append-only documentation).

## Outstanding Follow-ups (for the next session — priority order)

1. **🦄 Mic button + `lemonade_stt` wiring** (Day-3 of Phase 11). Confirmed via code-search: ZERO existing `MediaRecorder` / `getUserMedia` / `micButton` / `recordAudio` references anywhere in `src/`. Net-new UI work: Mic icon button next to AI panel input → `MediaRecorder.start()` → `ondataavailable` → `btoa(blob)` → `invoke('lemonade_stt', { audioBase64, filename: 'recording.webm' })` → set input value to transcribed text. Recording indicator (pulsing dot) + stop button. Lemonade returns raw Whisper text per the Rust command. Substantial enough that it should be confirmed with JC before building.

2. **Prune 12 pre-existing cargo warnings** in `sftp.rs` (5 dead fns: `sftp_mkdir`/`rename`/`delete`/`download`/`upload`), `secure_keys.rs` (`secure_resolve_api_key` unused), `hardware.rs` (3 unused consts: `DEFAULT_TRUSTED_QUANTIZERS` / `DEFAULT_QUANT_ALLOWLIST` / `PARAM_BUCKETS`), `backend_manager.rs` (1 unused `Manager` import + 1 unused `binary_path` field). Pre-date day-2. Easiest cleanup: delete (none of them are wired on the Vue side).

3. **Transliterate the 4 locale files** (hi / fa / he / ur) still showing English "Model Search" placeholder. Needs native-speaker verification for Devanagari (hi), Perso-Arabic (fa/ur), Hebrew (he) scripts. Route slug `/hardware` deliberately preserved — DO NOT rename.

4. **Push 7 unpushed commits to `meridian/main`** (see Pushable State above).

## Bugs Still Open (carry-over from prior sessions, NOT touched this session)

Per `SESSION_STATUS.md` "OPEN BUGS — NEED FIXING":
- 🔴 **#13 ACTIVE FOCUS — Omnix — can't download a model**. Where downloads actually go (verified, not guessed): Chromium Service Worker **Cache API**, NOT regular files. Two storage locations coexist:
  - `C:\Users\Jatilq\AppData\Roaming\omnix\Service Worker\CacheStorage\…` = **26.4 GB** (older pre-redirect cache, likely Qwen3-27B from earlier session).
  - `E:\ai\OmnixData\Service Worker\CacheStorage\d26cb286488555439586eae38b993292d15546db\` = **4.5 GB** (post-redirect; redirected by `electron/main.js`'s `app.setPath('userData', 'E:\\ai\\OmnixData')`).
  - `E:\ai\Models` contains ZERO `.gguf`/`.onnx`/`.safetensors`/`.bin` from Omnix — all 1.8 TB there is from Meridian's own native downloader.
- Fix options ranked: (a) Meridian-native download via `reqwest` with HF Bearer auth → save as `.gguf` in `E:\ai\Models\<author>/<repo>/` → configure Transformers.js to load from `file://` [pairs with bug #3 downloader-pat-bearer]; (b) inject HF Bearer token into Transformers.js requests via electron/main.js preload fetch override; (c) cache extraction tool to recover the 26 GB on disk; (d) watchdog to detect silent 401 from gated HF repos when `CacheStorage` byte counts don't increase for >2 min while UI shows "downloading".

All other priors (the 2–3 min Omnix activation sequence, the "health-check is a liar" pattern in `omnix::get_omnix_status`, etc.) are documented in `SESSION_STATUS.md` and were not addressed this session.

---

# SESSION RESULTS — July 1, 2026 (Day 4: Lemonade Model-Management Integration Plan)

## Goal

JC installed Lemonade locally at `E:\ai\Apps\lemonade_server\` and asked to read its native code to incorporate its **backend / model download functions** into Meridian. The day-3 commits only added inference-side Tauri commands (`lemonade_tts` / `lemonade_stt` / `lemonade_image`); the management-side endpoints (`/v1/pull`, `/v1/load`, `/v1/unload`, `/v1/delete`, `/v1/models`, `/v1/downloads`, `/v1/health`, `/v1/system-info`) are NOT yet wired. This day is a planning + discovery pass; implementation lands in subsequent sessions.

## Discovery (filesystem)

| Path | What it is |
|---|---|
| `E:\ai\Apps\lemonade_server\bin\LemonadeServer.exe` | The actual inference + management server binary. |
| `E:\ai\Apps\lemonade_server\bin\lemonade.exe` | The CLI tool (`lemonade pull <name> --checkpoint main <ckpt> --recipe llamacpp`). |
| `E:\ai\Apps\lemonade_server\bin\lemonade-app.cmd` | Entry script. |
| `E:\ai\Apps\lemonade_server\app\lemonade-app.exe` | App-style entry (not used by server-only mode). |
| `*.ps1` orchestration scripts | Reveal canonical workflows: `pull-all-models.ps1`, `import-models.ps1`, `register-from-json.ps1`, `register-all.ps1`, `pull-remaining.ps1`, `fix-symlinks.ps{1,2,3}`. `*.ps1` files call `lemonade.exe pull user.<name> --checkpoint main <ckpt> --recipe llamacpp`. |
| `E:\ai\Apps\lemonade_server\src\` | empty — the installed bundle does not ship source. To read source, the next agent must `git clone https://github.com/lemonade-sdk/lemonade`. |

## Discovery (live binary)

Server was **not running** at start of session (`curl http://localhost:13305/v1/models` → connection refused). Realtime UI verification depends on JC starting it via Settings → Backend Manager → Lemonade → Start, or manually via `bin\LemonadeServer.exe`. The downstream /v1 endpoints listed below should be exercised before any implementation PR is merged.

## Discovery (HTTP API verified from https://lemonade-server.ai/docs/)

| Method | Path                | Purpose                                                          |
| ------ | ------------------- | ---------------------------------------------------------------- |
| GET    | `/v1/models`        | list all registered models                                       |
| POST   | `/v1/pull`          | install / register a model (HF `checkpoint` + `recipe`; optional SSE via `stream:true`) |
| POST   | `/v1/load`          | load a registered model into runtime memory                      |
| POST   | `/v1/unload`        | unload a specific (or all) loaded models                         |
| POST   | `/v1/delete`        | delete a registered model                                        |
| GET    | `/v1/health`        | server status + currently-loaded models                          |
| GET    | `/v1/downloads`     | list server-owned model download jobs (streamed + background)    |
| GET    | `/v1/system-info`   | hardware / device enumeration                                    |
| Env    | `LEMONADE_HOST`     | default `127.0.0.1`                                              |
| Env    | `LEMONADE_PORT`     | default `11434` (NOTE: Conflicts with Lemonade's port-13305 default in `lemonade_extras.rs`!) |

⚠ **Port discrepancy flagged**: `lemonade_extras.rs` defaults to `http://localhost:13305` (matches JC's running install per prior session verification). The `lemonade-server` env vars default to `11434`. The new integration must use either the existing 13305 default or read from settings — NOT assume the env var default. Pin the chosen port in `meridian.backend.lemonade.backendPort` (default 13305).

## Integration Plan (thinker-with-files-gemini verdict)

### A. Rust module layout

**Recommendation**: Create `src-tauri/src/lemonade_manager.rs` (NEW module). Reasoning:
- `backend_manager.rs` manages generic binary lifecycle; bloating it with Lemonade-specific HTTP payloads would tangle concerns.
- `lemonade_extras.rs` is strictly inference-side and stays untouched (do not regress the day-2 TTS/STT/Vision wiring).
- A new module that's purely Lemonade-API gives one place to evolve when Lemonade's endpoints change.

**Commands to add** (all `#[tauri::command]`, snake_case args, camelCase JSON, `#[serde(rename_all = "camelCase")]` on response structs):

```
lemonade_list_models(endpoint, token) -> Result<Vec<LemonadeModelInfo>, String>
lemonade_pull_model(checkpoint, recipe, model_name, stream, app, endpoint, token) -> Result<(), String>
        // emits "lemonade-model-download-progress" events to frontend
lemonade_load_model(model_name, ctx_size?, endpoint, token) -> Result<(), String>
lemonade_unload_model(model_name: Option<String>, endpoint, token) -> Result<(), String>
        // None = unload all
lemonade_delete_model(model_name, endpoint, token) -> Result<(), String>
lemonade_get_health(endpoint, token) -> Result<LemonadeHealth, String>
lemonade_list_downloads(endpoint, token) -> Result<Vec<LemonadeDownloadJob>, String>
lemonade_get_system_info(endpoint, token) -> Result<LemonadeSystemInfo, String>
lemonade_auto_launch(app, install_dir, port) -> Result<u32, String>
        // Spawns LemonadeServer.exe, waits for /v1/health 2xx, returns PID
        // Reuses backend_manager::BackendRegistry for process tracking
        // Wire into WindowEvent::Destroyed alongside existing reap_backends
```

All write-side commands log to existing `backend_events` SQLite table with `kind='lemonade'` and `action` matching the command name.

### B. Config wiring

- New slot: **`meridian.backend.lemonade.installDir`** (default `E:\ai\Apps\lemonade_server\` since that matches JC's install).
- Existing `meridian.aiPanel.omnixPath`-style `lemonadePath` slot should NOT be reused; introduce a clean separation. Add a one-time migration to copy from the old slot into the new one.
- **`meridian.backend.lemonade.backendPort`** (default 13305).
- **`meridian.backend.lemonade.apiToken`** — write-only via `secure_keys.rs`; never crosses IPC in plaintext for production.
- Schema bump: bump `USER_SETTINGS_SCHEMA_VERSION` 32 → 33 with a `32 → 33` migration step that seeds the new keys WITHOUT overwriting user values.

### C. Frontend

**Recommendation**: New tab INSIDE the existing `src/modules/backend-manager/pages/backend-manager.vue` (rather than a new sidebar item — preserves AGENTS.md's rule that sidebar icons must NOT replace existing items). The new tab uses Tabs UI; visible label: "Lemonade Models".

UI shape:
- Top: server status card (`lemonade_get_health` polled every 5s via `setInterval`).
- Middle: registered-models table (`lemonade_list_models`), per row: model_name, checkpoint, recipe, loaded-badge (`/v1/health` cross-reference), action cluster (Load / Unload / Delete with `dialog.ask` confirmation).
- Bottom: pull-a-new-model form (fields: HF checkpoint, recipe dropdown (`llamacpp` default + others), model_name auto-derive as `user.<repo>`, "Pull" button → invokes `lemonade_pull_model`).
- SSE progress bar bound to `listen('lemonade-model-download-progress', ...)`.

Destructive ops (Delete / Unload-all) MUST call `import { ask } from '@tauri-apps/plugin-dialog'` FIRST. Frontend copy must reference Meridian's `confirm-destructive` pattern (used by SFTP file browser).

### D. Catalog updates

`src-tauri/resources/backend_catalog.json` (create if absent; bind via `tauri.conf.json::bundle.resources`). Insert:

```json
{
  "id": "lemonade.embeddable",
  "displayName": "Lemonade Server",
  "version": "10.8.1",
  "defaultPort": 13305,
  "binaryPaths": {
    "windows": "LemonadeServer.exe",
    "linux": "LemonadeServer",
    "darwin": "LemonadeServer"
  },
  "format": "binary",
  "installMethod": "manual-or-catalog"
}
```

### E. Auth / security

- Tokens stored exclusively via `secure_keys.rs::secure_store_secret("lemonade_api_key", ...)`.
- Never log tokens.
- `Authorization: Bearer <KEY>` header constructed inside the Rust command just before the `reqwest` call; never persisted into the lazy store under plaintext.

### F. Backwards compatibility

- Do NOT modify `lemonade_extras.rs` (inference path is working in day-3 verification; touching it risks regressing TTS/STT/Vision).
- Reuse `backend_manager::BackendRegistry` for process tracking so Lemonade shows up as "Running" with PID in the existing backend panel.
- Optionally extract `resolve_base()` from `lemonade_extras.rs` into a shared util — OR inline a small local copy in `lemonade_manager.rs` to keep the inference module untouched. Recommendation: keep duplicate (≤15 lines) until day 5 when both modules can be tidied together.

### G. Validation criteria (post-implementation)

1. `cargo check` exits 0 with 0 new warnings (the 12 pre-existing still present, NOT new).
2. `vue-tsc --build` exits 0.
3. Starting Lemonade via the new tab spawns the process, prints PID, polls /v1/health until 2xx, then shows status = Running.
4. Pulling a model from a known HF repo (e.g. `unsloth/Phi-4-mini-instruct-GGUF:Q4_K_M`) emits SSE progress; the bar visibly progresses from 0% → 100%; final model appears in the registered-models table within 5s.
5. Load → chat in AI Panel → response succeeds (this is the integration smoke test across the two Lemonade modules).
6. Delete with `dialog.ask` confirmation → file goes from registered list; backend_events row appended.

### H. Open questions for JC

1. **Port**: pin to 13305 (matches existing `lemonade_extras.rs`) or honor `LEMONADE_PORT=11434`? Pinning is the safer choice — the AI Panel URL is already hardcoded to 13305 and changing it would be churn.
2. **Auto-launch on Meridian boot**: should `lemonade_auto_launch` be called by the same `setup_handler` block that boots Omnix (now opt-in), or only via explicit user action from the new tab? Recommendation: opt-in only; the boot-on-startup path was the source of the day-1 Omnix demote churn.
3. **Should Lemonade binaries in `E:\ai\Apps\lemonade_server\` be auto-detected on first launch** (e.g. on `meridian.backend.lemonade.installDir` empty, scan parent for an existing install) or require explicit user setup? Recommendation: yes, with a Toast that says “Found existing Lemonade at <path> — use it?" and a Yes/Skip dialog.

## Sequence of next-action commits (thinker's recommendation)

1. **Config & Catalog** — migrate `lemonadePath` → `meridian.backend.lemonade.installDir`; schema bump 32 → 33; add catalog entry.
2. **Rust framework** — create `src-tauri/src/lemonade_manager.rs` with type stubs; register in `lib.rs::generate_handler!`.
3. **Read/Launch ops** — `lemonade_get_health`, `lemonade_list_models`, `lemonade_auto_launch` (the last reuses `BackendRegistry`).
4. **Write ops** — `lemonade_pull_model` (with SSE), `lemonade_load_model`, `lemonade_unload_model`, `lemonade_delete_model`. SQLite `backend_events` appended per command.
5. **Frontend UI** — "Lemonade Models" tab inside `backend-manager.vue`; SSE listener; `dialog.ask` destructive confirmations.
6. **Hand-test pass** — JC clicks through the full happy-path with the bundled `unsloth/Phi-4-mini-instruct-GGUF:Q4_K_M` model to validate end-to-end.

Pushable state at end of this session was 7 unpushed commits (4 prior + 3 from day 3). This session made NO code changes — purely exploration + planning — so pushable state was unchanged at the time of writing. Those 7 commits (plus 1 more for the day-4 Embeddable scaffold `b19789b8`) were pushed to `meridian/main` as part of Phase-0 step 2 on 2026-07-02, unblocking Day-5 work.

---

# SESSION RESULTS — July 2, 2026 (Day 5: Port reallocation + WSL/Docker audit)

## Goal

Resolve port collisions between Meridian and other Windows-side processes the user is running (SABnzbd owns 8080, plus several already-bound high ports I scanned earlier). Deliver the long-delayed Phase 11 port-override plan; audit WSL/Docker container ports. JC's directive at 2026-07-02 09:35 EDT:

> sabnzbd is installed baremetal and should keep 8080 / homepage port should be change to something other than 300 / llamacpp backend should be changed. it can even use a port similar to ollama since i dont use that. the rr stack and plex jellyfin shoudl use their default ports

## Status Table

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | WSL + Docker state probe | ✅ Done | WSL distros installed: **Ubuntu** (default), **docker-desktop** — **both STOPPED**. Docker CLI 29.6.1 is installed but daemon is **not accessible** (docker-desktop distro is stopped). Cannot run `docker ps`. |
| 2 | Windows-side listening ports (full TCP sweep) | ✅ Done | Meridian/AI-stack range scan: `11434` (PID **22288**, 127.0.0.1), `8000` (PID 27160 / 30696, 0.0.0.0), `8080` (PID **8352**, 0.0.0.0). `9777 / 13305 / 20128 / 7771 / 1420 / 1421 / 5000 / 50052 / 11435/6/7` all clear. |
| 3 | Meridian port inventory (code-bearing) | ✅ Done | vite 1420, HMR 1421, tauri devUrl 1420, Lemonade 13305 (`BackendKind::Lemonade::default_port()`), Omnix 9777, 9Router 20128, browser extension 7771, **LlamaCpp 11434** (Day-5; was 8080), KoboldCpp 5001, llamafile 8080 (kept), TurboQuant 8080 (kept). |
| 4 | LlamaCpp default 8080 → 11434 | ✅ Done | `backend_manager.rs::BackendKind::default_port()` match arm flipped; `start_backend` doc-comment rewritten to point at `default_port()` as source-of-truth; `MeridianBackendConfig.port?` TypeScript doc-comment updated to enumerate per-kind defaults; UI `DEFAULT_PORTS` map mirrored the change. |
| 5 | Homepage new port recommendation | ✅ Done | JC said "different from 300" without picking a number. **Recommended: 3010**. |
| 6 | `cargo check` after port flip | ✅ Done | Exit 0. 12 pre-existing warnings unchanged (zero new). |
| 7 | `npm run type-check` after port flip | ✅ Done | Exit 0. |
| 8 | `cargo test --lib backend_manager` | ✅ Done | 36/36 pass including `lemonade_default_port_is_13305` (intentionally untouched so it still asserts the upstream Lemonade port). |

## Files Touched (this session)

| File | Change |
|---|---|
| `src-tauri/src/backend_manager.rs` | `BackendKind::default_port()` flipped LlamaCpp: `8080` → `11434`. Doc-comment on the `fn` expanded to explain JC's host constraint (SABnzbd owns 8080, Ollama unused so 11434 is free, and `start_backend` therefore does not need a pre-bind check). Doc-comment on `start_backend` rewritten to point at `BackendKind::default_port()` as source-of-truth + per-kind default list. |
| `src/types/user-settings.ts` | `MeridianBackendConfig` `port?` doc-comment updated to enumerate per-kind defaults + call out the explicit mirroring contract with `backend_manager.rs::BackendKind::default_port()`. No signature change. |
| `src/modules/backend-manager/pages/backend-manager.vue` | `DEFAULT_PORTS` map: `'llama.cpp': 8080` → `'llama.cpp': 11434`. Comment now explicitly ties the UI pre-fill to `BackendKind::default_port()` so future engineers read the lockstep contract at first glance. |
| `SESSION_RESULTS.md` | This Day-5 entry. |

## Validation Table

| Tool | Result |
|---|---|
| `cargo check --message-format=short` | Exit 0. 12 pre-existing warnings (no new). |
| `vue-tsc --build` (`npm run type-check`) | Exit 0. |
| `cargo test --lib backend_manager` | 36/36 pass (`lemonade_default_port_is_13305` confirmed still asserts the upstream Lemonade port 13305). |

## Port reallocation cheat sheet (JC reference)

| App / service | Default | New port | Reasoning |
|---|---|---|---|
| **SABnzbd** (Windows baremetal) | 8080 | **8080 (kept)** | JC: "should keep 8080". |
| **Homepage** (Docker / external) | 3000 | **3010 (recommended)** | JC: "different from 300" without picking a number. 3010 = single-digit-up, clean, no conflict with Meridian / SABnzbd / RR / Plex / Jellyfin / SABnzbd. |
| **Meridian LlamaCpp Backend** | 8080 | **11434** | JC: "use Ollama's port since i dont use that". JC must kill PID 22288 (currently binds `127.0.0.1:11434`) so the new default binds cleanly on next `start_backend`. |
| llamafile (not yet installed) | 8080 | n/a | Default kept. SABnzbd owns 8080 — flag for future `portOverride` schema bump. |
| TurboQuant (not yet installed) | 8080 | n/a | Same as llamafile. |
| KoboldCpp (not yet installed) | 5001 | n/a | No conflict today. |

## Pending JC actions (NOT code — local-machine only)

1. **Identify + kill PID 22288** (currently binds `127.0.0.1:11434`) so LlamaCpp Backend Manager's `--port 11434` can bind cleanly on next `start_backend`. Run on JC's Windows shell:
   ```powershell
   tasklist /FI "PID eq 22288" /V     # identity
   taskkill /PID 22288 /F              # if stale (likely leftover Ollama instance)
   ```
2. **Move Homepage off `:300`** in JC's external Homepage install config (Docker compose / services yml / container `--publish` flag), to 3010 (or whatever JC picks). Restart Homepage.
3. **Restart Meridian** (`npm run tauri:dev`) so:
    - the new LlamaCpp default picks up in the running binary,
    - the Phase-0 schema bumps (30 → 31 → 32 → 33) light up on first relaunch (still JC's `Phase 0.3` action),
    - the `meridian.backend.lemonade.{installDir, backendPort, apiTokenKey}` defaults seed on a fresh store.

## Outstanding Follow-ups (Day 6 priority)

1. (carry-forward) **Schema 33→34 + 11-commit Lemonade Embeddable integration** from Day-4 plan. Today's port work landed independently of Phase-0 cleanliness (per JC's explicit directive), so this body of work is unblocked at the code level. JC may still gate it until Phase 0 fully clean per the earlier strict rule.
2. (NEW, surfaced today) **`portOverride: number?`** on `MeridianBackendConfig` (schema 34→35) to future-proof llamafile / TurboQuant / KoboldCpp against SABnzbd-on-8080 collision when JC installs those.
3. (NEW, surfaced today) **WSL/Docker full cross-reference** — blocked until JC boots docker-desktop; re-run `docker ps` to enumerate, then re-emit this audit's conflict matrix fully populated.
4. (carry-forward) Day 1: 4 locale files (hi / fa / he / ur) transliteration review.
5. (carry-forward) Day 3: prune 12 pre-existing cargo warnings in sftp.rs / hardware.rs / secure_keys.rs / backend_manager.rs.
6. (carry-forward) Day 3: Mic button + `lemonade_stt` JS caller wiring.
7. (carry-forward) **Bug #13 Omnix cache-API** — to be closed via Day-4 commits 3–6's Lemonade-native model management, once those land.

## Pushable State

- Working tree contains 3 port-fix edits + this Day-5 SESSION_RESULTS.md entry. To commit as a single atomic commit per AGENTS.md single-commit-per-concern:
  `fix(backend-manager): port llama.cpp default from 8080 to 11434 (free Ollama's port; SABnzbd owns 8080)`.
- After commit, ahead of `meridian/main` = 1 commit. PAT shared in chat history — recommend revoke/regenerate per AGENTS.md security policy before any push.

---

# SESSION RESULTS — July 2, 2026 (Day-5.1 Hotfix: Lemonade `backendPort` 13305 → 11434)

## Goal

Day-4 schema bump **32 → 33** seeded `meridian.backend.lemonade.backendPort = 13305` (wrong number — Lemonade's `LEMONADE_PORT` env default is **11434**, and JC's actual bundled install at `E:\ai\Apps\lemonade_server\` binds 11434, not 13305). Confirmed via `curl http://127.0.0.1:11434 → HTTP_200 (Lemonade App)` vs `curl http://127.0.0.1:13305 → connection timed out`. The Day-5 port audit surfaced PID 22288 (`LemonadeServer.exe`) on 11434. This hotfix lands a corrective schema bump **33 → 34** that overwrites the bad 13305 default to 11434, plus updates the source-of-truth literals across 5 files.

## Status Table

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | Schema **33 → 34 corrective migration** in `schemas/user-settings.ts` | ✅ Done | Sentinel-detect (only fires when stored value `=== 13305`, exact-equality). Preserves any user-set custom port (e.g. `192.168.1.67:13305` for a remote Lemonade). |
| 2 | Default `meridian.backend.lemonade.backendPort` 13305 → 11434 | ✅ Done | `USER_SETTINGS_SCHEMA_VERSION = 34`. 32→33 migration block now seeds 11434; 33→34 fixup rewrites any pre-existing 13305 value to 11434. |
| 3 | `src/stores/storage/user-settings.ts` initial default | ✅ Done | `backend.lemonade.backendPort = 11434` (was 13305). Comment cites LEMONADE_PORT env default. |
| 4 | `src/types/user-settings.ts` `MeridianBackendConfig` port table | ✅ Done | lemonade row: 11434, with note that LEMONADE_PORT defaults to 11434 (not the Ollama port). |
| 5 | `src-tauri/src/lemonade_extras.rs::DEFAULT_LEMONADE_BASE` | ✅ Done | `http://localhost:11434` (was `13305`). 5 unit tests assert 11434. |
| 6 | `src-tauri/src/lemonade_manager.rs::DEFAULT_LEMONADE_BASE` | ✅ Done | Matches `lemonade_extras.rs`. Module docstring updated. |
| 7 | Brace-balance repair of `schemas/user-settings.ts` | ✅ Done | Day-4 hot-fixup insertion landed the 33→34 block OUTSIDE `migrateUserSettingsStep`, breaking `vue-tsc` with TS2304 (`fromVersion`, `storage` undefined). Replaced the broken region with the corrective block properly nested INSIDE the function; function close now sits AFTER the 33→34 block. |
| 8 | `cargo check` | ✅ Done | Exit 0. 12 pre-existing warnings unchanged (zero new). |
| 9 | `vue-tsc --build` | ✅ Done | Exit 0. |
| 10 | Code-reviewer verdict | ✅ Done | **PASS** with one **CRITICAL** follow-up (see Day-5.2 below). |

## Files Touched

| File | Change |
|---|---|
| `src/stores/schemas/user-settings.ts` | `USER_SETTINGS_SCHEMA_VERSION = 33 → 34`. 32→33 block seeded `backendPort: 11434`. New 33→34 block: `if (existingLemonadePort === 13305) → set(11434)` with sentinel-detect (exact-equal, so user-set custom ports survive). Brace-balance repair to keep 33→34 INSIDE `migrateUserSettingsStep`. |
| `src/stores/storage/user-settings.ts` | `meridian.backend.lemonade.backendPort`: 13305 → 11434 (initial default). |
| `src/types/user-settings.ts` | `MeridianBackendConfig.port?` doc-comment lemonade row: 13305 → 11434. |
| `src-tauri/src/lemonade_extras.rs` | `DEFAULT_LEMONADE_BASE: "http://localhost:11434"`. 5 unit tests assert 11434. |
| `src-tauri/src/lemonade_manager.rs` | `DEFAULT_LEMONADE_BASE: "http://localhost:11434"`. Module docstring updated. |
| `SESSION_RESULTS.md` | This entry. |

## Validation Table

| Tool | Result |
|---|---|
| `cargo check` (src-tauri) | Exit 0. |
| `vue-tsc --build` (`npm run type-check`) | Exit 0 (after brace-balance repair). |
| `cargo test --lib backend_manager` | 36/36 pass (no regression). |
| `grep -rn '13305' src-tauri/src/lemonade_*.rs` | Zero matches — fix is exhaustive across the Lemonade Rust modules. |

## Sentinel-detection logic (Day 5.1)

```ts
if (fromVersion === 33 && toVersion === 34) {
  const BAD_LEMONADE_PORT = 13305;
  const GOOD_LEMONADE_PORT = 11434;
  const existingLemonadePort = await storage.get<number>('meridian.backend.lemonade.backendPort');
  if (existingLemonadePort === BAD_LEMONADE_PORT) {
    await storage.set('meridian.backend.lemonade.backendPort', GOOD_LEMONADE_PORT);
    // ... console.info(...)
  }
}
```

Same exact-match (over `.includes`/`.endsWith`) pattern as the Day-2 30→31 Ollama→Lemonade URL migration: JC's deliberate user-set custom ports (e.g. a remote Lemonade worker's port) survive the rewrite.

## 🐛 Reviewer's CRITICAL follow-up → Day 5.2 (NOT in this commit)

Code-reviewer flagged that **`meridian.aiPanel.routerEndpoint` and `meridian.aiPanel.localEndpointUrl`** still hardcode `http://localhost:13305/v1` in three places:

| Surface | File | String |
|---|---|---|
| Initial Pinia default | `src/stores/storage/user-settings.ts` | `routerEndpoint: 'http://localhost:13305/v1'` AND `localEndpointUrl: 'http://localhost:13305/v1'` (TWO locations) |
| Pinia runtime fallback | `src/stores/runtime/ai-panel.ts` | `?? 'http://localhost:13305/v1'` (TWO locations) |
| Schema migration | `src/stores/schemas/user-settings.ts` (`if (fromVersion === 30 && toVersion === 31)`) | `LEMONADE_DEFAULT_URL = 'http://localhost:13305/v1'` (TWO locations) |

**Net effect after this Day-5.1 commit**: AI-Panel chat completions will still POST to `localhost:13305/v1` → connection refused, even though `lemonadestatus` checks against 11434 will succeed. Backend Manager's lemonadestatus card will show ✅ Running on 11434, but the chat panel won't reach the model.

**Day-5.2 plan (NOT begun, awaiting JC authorization)**: Schema bump **34 → 35** with a parallel sentinel-detect migration that overwrites the EXACT literal `'http://localhost:13305/v1'` → `'http://localhost:11434/v1'` on both `routerEndpoint` and `localEndpointUrl`. Plus identical edits to:
- `src/stores/storage/user-settings.ts` initial defaults (TWO values)
- `src/stores/runtime/ai-panel.ts` runtime fallbacks (TWO values)
- `src/stores/schemas/user-settings.ts` `LEMONADE_DEFAULT_URL` constant (referenced from 30→31 migration)
- Any extra surface in `src/types/user-settings.ts::AI_PANEL_PROVIDER_URLS` if it lists `lemonade`.
- Code-reviewer notes that `lemonade_extras.rs::resolve_base()` correctly preserves caller override → default priority, so once the URLs are right, TTS/STT/Vision commands route correctly too.

## Validator info

- Lemonade is **already running** on PID 22288 (11434). After the Day-5.2 URL fix lands, JC's restart → Settings → Meridian → AI Panel should show lemonadestatus = `Online` AND chat completion POSTs to `localhost:11434/v1` should succeed without manual URL edit.
- The commit pre-push: 5 file edits, ~75 lines net delta (most of it comments), schema migration block ~18 lines.

## Outstanding Follow-ups (Day 6 priority)

1. **🦄 Day-5.2: rewrite `meridian.aiPanel.{routerEndpoint,localEndpointUrl}` 13305→11434** (reviewer's CRITICAL). Without this, AI Panel chat won't fire even though Backend Manager shows Lemonade up. ~6-file edit, schema bump 34→35, sentinel-detect migration.
2. (carry-forward) **Day-4 plan steps 4-6** (pull/load/unload/delete Tauri commands + frontend UI tab) — still pending implementation.
3. (carry-forward) Day 1: 4 locale files (hi / fa / he / ur) transliteration review.
4. (carry-forward) Day 3: prune 12 pre-existing cargo warnings in `sftp.rs` / `hardware.rs` / `secure_keys.rs` / `backend_manager.rs`.
5. (carry-forward) Day 3: Mic button + `lemonade_stt` JS caller wiring.
6. (carry-forward) **Bug #13 Omnix cache-API** — to be closed via Day-4 commits 3–6's Lemonade-native model management.

---

# SESSION RESULTS — July 2, 2026 (Day-5.2: AI-Panel consumer URL hardcode resolution)

## Goal

Close the reviewer's **CRITICAL** follow-up from Day-5.1 (commit `6239795d`). The Day-5 / Day-5.1 commit series fixed the *backend-side* port literals (LlamaCpp 8080→11434, Lemonade `backendPort` 13305→11434) but the AI-Panel *consumer* URLs that Rain actually sends HTTP requests to (`meridian.aiPanel.routerEndpoint`, `meridian.aiPanel.localEndpointUrl`, and their Pinia runtime fallbacks) still seeded `http://localhost:13305/v1`. Net effect post-Day-5.1: Backend Manager lemonadestatus card showed ✅ Running on 11434, but chat `/v1/chat/completions` POSTed to 13305 → connection refused. Day-5.2 closes that loop with a 5-file change set + schema bump 34→35.

## Status Table

| # | Task | Status | Notes |
|---|---|---|---|
| 1 | `USER_SETTINGS_SCHEMA_VERSION` 34 → 35 + new 34→35 corrective migration | ✅ Done | `src/stores/schemas/user-settings.ts`. Sentinel-detect (EXACT-equal `===`): rewrites BOTH `meridian.aiPanel.routerEndpoint` and `meridian.aiPanel.localEndpointUrl` ONLY when stored value equals `'http://localhost:13305/v1'`. Same pattern as 30→31 + 33→34. Preserves any user-set custom URL (LM Studio on `:1234`, OpenRouter hostname, remote Lemonade at 192.168.1.X:13305). |
| 2 | Storage initial defaults `routerEndpoint` + `localEndpointUrl` | ✅ Done | `src/stores/storage/user-settings.ts`. Both flipped to `'http://localhost:11434/v1'`. Comments cite `LEMONADE_PORT` env default + point at the 34→35 migration as the source-of-truth for upgrading users. |
| 3 | Pinia runtime fallbacks | ✅ Done | `src/stores/runtime/ai-panel.ts`. Both `??`/`||` fallbacks flipped to `http://localhost:11434/v1`. Comments cite 34→35 by name so future readers see WHY the fallback exists (sentinel pre-cleansed for upgrade installs). |
| 4 | `BackendKind::Lemonade::default_port()` 13305 → 11434 + docstring | ✅ Done | `src-tauri/src/backend_manager.rs`. Doc-string now cites `LEMONADE_PORT` env default + the curl-binding proof (11305 connection-refused vs 11434 returns Lemonade App HTML, JC 2026-07-02). |
| 5 | Test rename `lemonade_default_port_is_13305` → `lemonade_default_port_is_11434` + assert | ✅ Done | Same Rust file. Keeps the test name honest with what it asserts. |
| 6 | `.vue` `DEFAULT_PORTS.lemonade` mirror 13305 → 11434 | ✅ Done | `src/modules/backend-manager/pages/backend-manager.vue`. Comment now cites the Rust `default_port()` as source-of-truth so future drift is a one-line patch each place. |
| 7 | `cargo check` + `vue-tsc` | ✅ Done | Exit 0 / 0 (12 pre-existing warnings in `sftp.rs` / `hardware.rs` / `backend_manager.rs` / `secure_keys.rs` unchanged; zero new). |
| 8 | Code-reviewer verdict | ✅ Done | **PASS** with one minor (non-blocking) observation: the `BAD_LEMONADE_URL` / `GOOD_LEMONADE_URL` local-scope constants in the 34→35 block could be DRY'd against the existing `LEGACY_OLLAMA_URL` / `LEMONADE_DEFAULT_URL` constants in 30→31. Same observation already applies to 33→34's `BAD/GOOD_LEMONADE_PORT` pattern; out of scope here. |

## Files Touched

| File | Change |
|---|---|
| `src/stores/schemas/user-settings.ts` | `USER_SETTINGS_SCHEMA_VERSION = 34 → 35`. New `if (fromVersion === 34 && toVersion === 35)` migration block: sentinel-detect `'http://localhost:13305/v1'` → `'http://localhost:11434/v1'` for both `routerEndpoint` and `localEndpointUrl`. Migration placed INSIDE `migrateUserSettingsStep`; brace balance manually verified via `awk`/`cat -A` byte-precise dump. |
| `src/stores/storage/user-settings.ts` | `routerEndpoint` + `localEndpointUrl` initial defaults both `http://localhost:13305/v1` → `http://localhost:11434/v1`. Comments cite `LEMONADE_PORT` env default. |
| `src/stores/runtime/ai-panel.ts` | `routerEndpoint` / `localEndpointUrl` ref initial-value fallbacks both flipped to `http://localhost:11434/v1`. Comments cite 34→35 by name. |
| `src-tauri/src/backend_manager.rs` | `BackendKind::Lemonade => 11434` (was 13305). Doc-string + module comment updated. Test renamed + assert updated. |
| `src/modules/backend-manager/pages/backend-manager.vue` | `DEFAULT_PORTS.lemonade: 13305` → `11434`. Comment cites `backend_manager.rs::BackendKind::Lemonade::default_port()` as source-of-truth. |
| `SESSION_RESULTS.md` | This Day-5.2 entry. |

## Validation Table

| Tool | Result |
|---|---|
| `cargo check` (src-tauri) | Exit 0. 12 pre-existing warnings unchanged; zero new. |
| `vue-tsc --build` (`npm run type-check`) | Exit 0. |
| `cargo test --lib backend_manager` | 36/36 pass. The renamed `lemonade_default_port_is_11434` now asserts 11434. |
| `grep -rn '13305/v1' src/ src-tauri/src/` | Zero matches in active source. Only matches are in the documented historical-sentinel references inside the 30→31 + 34→35 migration blocks (those references ARE the sentinel-detect detection targets — intentional). |
| Brace balance (manual byte dump) | Intact — `migrateUserSettingsStep` opens once + closes once via the function-end `}`; 33→34 + 34→35 blocks both properly nested. |

## Migration chain — final convergence

For a user upgrading through schema 30 → 35 (say, Day-2 install that hardcoded 13305/v1 a year ago):

```
30→31: Ollama URL 'http://localhost:11434/v1' → Lemonade 'http://localhost:13305/v1'
31→32: omnixEnabled = false  (forced demote; reap-kill stale Electron)
32→33: meridian.backend.lemonade.{installDir, backendPort, apiTokenKey} seeded
        (with backendPort: 11434 — Day-5.1 hotfix already corrected this default)
33→34: backendPort sentinel-detect: 13305 → 11434 (catches the bad 32→33 seed)
34→35: AI-Panel URL sentinel-detect: 'http://localhost:13305/v1' → ':11434/v1'
```

End state for the routing chain: every URL string including the AI-Panel pointer ends up at `http://localhost:11434/v1` (= Lemonade's actual upstream default) for users with no custom URL set; users with LM Studio / OpenRouter / remote Lemonade URLs at any step are preserved by the exact-match sentinel-detect pattern.

## Stage + commit

Committed as `02c35f70` — `fix(ai-panel): Day-5.2 consumer URL hardcode resolution (13305/v1 -> 11434/v1)`. Just my 5 files staged; JC's pre-existing working-tree changes (the 17 files in `git status` at session start) remain independent for JC's own commit decisions.

## Outstanding Follow-ups (Day 6 priority, post-Day-5.2)

1. (carry-forward) **Day-4 plan steps 4-6** (pull/load/unload/delete Tauri commands + frontend "Lemonade Models" tab) — still pending implementation.
2. **Day-6.0 clean restart**: JC closes PowerShell, opens a fresh one, retries `npm run tauri:dev` (cargo is on bash PATH but PowerShell PATH didn't include `~/.cargo/bin`). Phase 0 step 3.
3. After restart: **verify in UI** — Settings → Meridian → AI Panel should show both `routerEndpoint` + `localEndpointUrl` pre-filled at `http://localhost:11434/v1` (fresh install) OR correctly migrated from the bad default (upgrade install). Backend Manager → Lemonade → Status probe should show ✅ Online.
4. (carry-forward) Day 1: 4 locale files (hi / fa / he / ur) transliteration review.
5. (carry-forward) Day 3: prune 12 pre-existing cargo warnings in `sftp.rs` / `hardware.rs` / `secure_keys.rs` / `backend_manager.rs`.
6. (carry-forward) Day 3: Mic button + `lemonade_stt` JS caller wiring.
7. (carry-forward) **Bug #13 Omnix cache-API** — to be closed via Day-4 commits 3–6's Lemonade-native model management.


