# Session Status — Bug Tracker (JC Testing Round)

**Date:** 2026-07-01
**State:** Selective wipe of `$APPDATA/com.meridian.app/` completed (meridian.db, secure-keys.json, user-data/, etc.). Fresh-install state tested.

---

## INCORPORATION SCOPING — Omnix → Meridian (2026-07-01)

**Goal JC asked us to scope:** Make Omnix a native tab inside Meridian rather than a separately-installed process. Process-sync pain points today (Squirrel vs git clone, port mismatch, worker-not-connected races) all root back to Omnix being out-of-process.

### License finding (RED FLAG — re-shaped every option)

- E:\ai\Apps\Omnix has **NO LICENSE file** in repo root.
- **NO `license` field** in `package.json`.
- Fetching `https://raw.githubusercontent.com/LoanLemon/Omnix/{main,master}/LICENSE` returns **404**.
- Default copyright = **“all rights reserved”** under US/Berlin Convention. The author (LoanLemon) retains all rights.
- MIT-licensed npm deps: `@huggingface/transformers@4.2.0`, `kokoro-js@1.2.1`, `onnxruntime-web`, `onnxruntime-node`, `sharp` — all can be absorbed into Meridian with attribution.
- The .tsx/.ts **source code of Omnix itself** is NOT covered by any open-source grant. Copying substantial portions into Meridian would be a derivative work.
- For JC’s personal/hobbyist use per `AGENTS.md`: reading the code, running the unmodified binary, and shipping it bundled **inside Meridian’s installer** as a private aggregation is defensible. **Translating React components into Vue for incorporation is not.** This narrows Option A below.

### Three options (Option A is spawned as Option C due to license)

| | Option A (“copy + rewrite”) | Option C (“clean-room reimplement”) | Option B (“embedded pane sidecar”) |
|---|---|---|---|
| Legality | **NO** — unlicensed copy. | YES — new code, MIT libs reused, model IDs are facts not copyright. | YES — ship unmodified binary, embed its UI. |
| Effort | ~5000 lines transliterated. | 3–5 weeks of fresh Vue + Web Worker code. | **1–2 days.** |
| Files impacted in Meridian | New `src/modules/omnix-tab/` ~70 components. | Same but every byte is original. | New `src/modules/omnix-tab/` 2 wrapper files + `src-tauri/omnix.rs` change. |
| Stuff reused (MIT/facts) | Same as C | MIT libs + model IDs from `modelList.ts` + DESIGN-style UX patterns. | None in Meridian source — binary ships intact. |
| Process model | One process (Meridian). | One process (Meridian). | Two: Meridian + **sidecar child Electron**. |
| GPU reliability | Single WebGPU context in Meridian Tauri WebView2. Stable on Win 11/Edge ≥110. | Same. | Each sidecar runs its own Chromium — Electron’s bundle is the **known-good** shell that LoanLemon already tested. |
| Crash isolation | **None.** A transformers.js OOM or WebGPU fault kills Meridian’s renderer thread. | Same. | **Maximum.** Sidecar dies silently. Meridian UI keeps working. |
| Honors DESIGN.md intent (“process isolation is required”) | **Violates.** | **Violates.** | **Honors.** |
| Replaces today’s spawn-omnix flow | Yes. | Yes. | Yes — `omnix.rs` spawns via `Command::new_sidecar()` instead of `fork()`. |
| Bundle footprint impact | 0 | 0 | +0 (Omnix binary is already bundled as `resources/omnix/`); what changes is the **spawn mechanism** — Tauri’s sidecar pattern replaces Electron-as-Node. |

### Recommended path: **Option B (forever sidecar)**

JC’s framing (hobbyist, retired, frustrated by today’s Omnix bugs) eliminates the 4-week rewrite option. Architecture is already committed in `DESIGN.md` line 30–34:

> *“Omnix runs as a **separate Electron process** spawned by Meridian. This is the final, intentional architecture… No hidden BrowserWindow, no embedded webview — **process isolation is required** for Omnix’s WebGPU compute worker.”*

The only concession B asks of DESIGN.md: drop the bare `fork()` from `omnix.rs` in favour of `Command::new_sidecar()`. Plus the new tab is a tiny wrapper, not a rewrite.

### First 3 actions (for Option B, in dependency order)

1. **Register Omnix binary as a Taurisidecar.** Edit `src-tauri/tauri.conf.json`: add to `bundle.externalBin` array, reference the already-extracted `resources/omnix/Omnix.exe` (or for dev, the `E:\ai\Apps\Omnix\dist\Omnix.Setup.0.7.0.exe` if we ship the installer flow).
2. **Migrate `omnix.rs::spawn_omnix` from `Command::new(...).spawn()` to `tauri::api::process::Command::new_sidecar("omnix")`.** Add `tauri-plugin-sidecar` to `Cargo.toml`. Return `pid + listened-port` so the Vue tab can wait for `GET /api/health` to return 200 before showing the iframe. Add `kill_omnix` hook into `WindowEvent::Destroyed` alongside the existing `lan_share::stop_lan_share` call.
3. **Build the Vue Omnix tab.** New `src/modules/omnix-tab/` directory with: 1) a Vue component that renders a sidebar icon + tab route registered in `src/router/routes.ts` between `cluster` and `backend-manager`, 2) a `<WebviewWindow>` child pointing at `http://localhost:9777` once `:9777` is alive, 3) a fallback toast when sidecar fails to bind `:9777` within 15s.

### What is **not** part of this scope (kept out of the way)

- The 8 open bugs in `OPEN BUGS — NEED FIXING` below remain untouched.
- Lite-engines port (Director/STT/TTS) from the earlier analysis is a **follow-up after** B is shipped — depends on Meridian’s own downloader (bug #3) being fixed first.
- Models-bug-13 (Omnix download broken) does **not** fix itself by switching to sidecar. B changes **how** Omnix starts, not **what** it does on the inside. Bug #13 still requires Meridian-side model acquisition + the cache-extraction tool if we want to unlock the 26 GB junk in `Service Worker/CacheStorage`.
- The Process-isolation-isolation TRACE in the olden plan can stay in `DESIGN.md` since B preserves it.

---

## PRIORITY 0 — Omnix Deep-Dive (META)

**JC's comment:** "Revisit Omnix and understand the issues and how it's connected to Meridian. It's the core program we need."

### Architecture (from source analysis)

Omnix is a **relay-based AI inference server** with this architecture:

```
Meridian (Tauri) ──HTTP──→ Omnix Express server (:9777) ──WebSocket──→ Compute Worker (headless Chromium)
                                                                                          ↓
                                                                              @huggingface/transformers
                                                                              WebGPU / ONNX Runtime
                                                                              kokoro-js (TTS)
```

**Layer 1: Electron main.js** (`electron/main.js`)
- Creates a visible BrowserWindow (the GUI window JC saw)
- Auto-starts the Express API server after 1200ms delay
- Can spawn headless **compute worker** BrowserWindows (hidden, `show: false`)
- Worker windows load the Omnix frontend with `?mode=worker` param
- Model cache redirected to `E:\ai\OmnixData` (not C: drive)
- Forces high-performance GPU via Chromium switches
- Monitors parent PID (`--dependent-pid`) and exits when Meridian dies

**Layer 2: Express server** (`server.ts`, port 9777)
- Pure relay — does NO inference itself
- WebSocket relay to connected compute worker windows
- **Endpoints exposed:**
  - `GET /api/health` — returns `{ status: "ok", pid }` if Express is running
  - `POST /api/text` — text generation: `{ prompt, systemPrompt, modelId, temperature, top_p, maxTokens }` → `{ response, think? }`
  - `POST /api/vision` — image analysis (multipart)
  - `POST /api/stt` — speech-to-text (multipart)
  - `POST /api/tts` — text-to-speech: `{ text, voiceId }` → audio
  - `POST /api/director` — intent classification
  - `POST /api/image` — image generation
  - `POST /api/music` — music generation
  - `POST /api/server/relay` — relay control
  - `GET /api/server/status` — server status
- All task endpoints call `dispatchTask(type, input, options)` from `src/engine/socketHandler.ts`
- `dispatchTask` sends work over WebSocket to the connected compute worker
- **If no worker is connected → throws error**

**Layer 3: Compute Worker** (headless Chromium BrowserWindow)
- Loads the Omnix React frontend with `?mode=worker`
- Connects to server via WebSocket
- Runs `@huggingface/transformers` with WebGPU acceleration
- Handles actual AI inference (text, vision, TTS, etc.)
- Server can send `SPAWN_WORKER` IPC message to main.js to spawn a worker

### The "Engine Activation Sequence" (2-3 min)

1. Meridian calls `spawn_omnix` → Electron process starts
2. Electron main.js loads → creates main window (visible = bug #1, now fixed)
3. After 1200ms, `startBackgroundServer()` forks the Express server
4. Express server starts → `/api/health` returns OK immediately
5. Server sends `SPAWN_WORKER` to Electron main → headless worker window created
6. Worker window loads React frontend → connects WebSocket
7. Worker loads `@huggingface/transformers` → downloads/initializes model
8. **Steps 6-7 take 2-3 minutes** → this is the activation sequence

### Critical Bug Pattern Discovered

**The health check is a LIAR.** `GET /api/health` returns OK as soon as Express starts (step 4), but the compute worker isn't ready until step 8. This means:
- `get_omnix_status` returns `true` too early
- `omnix_text` fails with "No compute worker connected" because the worker hasn't finished loading
- The AI panel works because it uses `fetch()` to the router endpoint (9Router / llama.cpp), NOT Omnix

**THIS IS THE ROOT CAUSE OF BUG #2.** The `omnix_text` Rust command POSTs to Omnix, but Omnix's worker isn't ready. The health check lies about readiness.

### Fix Plan Adjustment

Based on this deep-dive, bug #2 needs more than just the `omnix_text` Rust command:
1. The `omnix_text` command exists ✅ (already in working tree)
2. But it will fail until the compute worker is actually ready
3. **Fix needed:** The Rain CLI `handleSend()` already has a 120s wait loop for Omnix. But it checks `get_omnix_status` (health endpoint) which returns true before the worker is ready. We need either:
   - A new `/api/ready` endpoint in Omnix that checks worker connectivity (best)
   - Or retry logic in `omnix_text` that catches "No compute worker" and retries after a delay (workaround)
4. The AI panel works because it uses `runAgentLoop()` → `fetch()` to router endpoint, NOT Omnix. This is a DIFFERENT inference path.

### Relationship to 9Router

9Router (`http://localhost:20128/v1`) is a SEPARATE OpenAI-compatible proxy. It's NOT Omnix. The AI panel's `routerEndpoint` defaults to `http://localhost:11434/v1` (llama.cpp default). The Rain CLI uses `invoke('omnix_text')` which goes to Omnix (port 9777). These are two completely different inference backends:
- **9Router/llama.cpp** = local GGUF model inference via Backend Manager
- **Omnix** = browser-based inference via transformers.js + WebGPU

### Bundled Resources Status

Only 5 files bundled: `server.ts`, `package.json`, `electron/main.js`, `electron/preload.cjs`, `electron/icon.png`
The `src/engine/socketHandler.ts` that `server.ts` imports is NOT bundled. The compiled `dist/server.cjs` is also not bundled. This means `npm install` + `tsx server.ts` must resolve the missing imports at runtime. If the full Omnix source was never copied to `E:\ai\Apps\Omnix\`, the server will crash on import.

**Files studied:** `src-tauri/resources/omnix/server.ts`, `src-tauri/resources/omnix/electron/main.js`, `src-tauri/resources/omnix/electron/preload.cjs`, `src-tauri/resources/omnix/package.json`, `src-tauri/src/omnix.rs`, `src/stores/runtime/ai-panel.ts`

---

## FIXES ALREADY IN WORKING TREE (need `tauri:dev` restart to test)

### 1. Omnix window flashes visibly on spawn
- **Root cause:** `spawn_omnix` in `omnix.rs` used bare `.spawn()` without `CREATE_NO_WINDOW` flag
- **Fix:** Added `#[cfg(windows)] { const CREATE_NO_WINDOW: u32 = 0x08000000; command.creation_flags(CREATE_NO_WINDOW); }` to the Electron spawn
- **File:** `src-tauri/src/omnix.rs`
- **Status:** ✅ Fixed, cargo check passes

### 2. Rain CLI "Unknown error" on message send
- **Root cause:** `invoke('omnix_text', ...)` is called from 3 Vue files (`rain-cli.vue:414`, `rain-cli.vue:431`, `ai-panel.vue:545`, `ai-panel.vue:573`, `rain-cli-slide-in.vue:96`) but NO `#[tauri::command] pub async fn omnix_text(...)` existed in Rust. The AI panel works because it uses a DIFFERENT code path — `runAgentLoop()` which POSTs directly via `fetch()` to the router endpoint, bypassing Tauri entirely.
- **Fix:** Added `omnix_text` command to `src-tauri/src/omnix.rs` — POSTs to `http://localhost:9777/api/text` with prompt/systemPrompt/temperature/maxTokens/topP. Response parsing has fallback chain: `response > text > content > message > choices[0].message.content > raw`. Registered in `src-tauri/src/lib.rs` invoke_handler.
- **Files:** `src-tauri/src/omnix.rs`, `src-tauri/src/lib.rs`
- **Status:** ✅ Fixed, cargo check passes. **MUST restart `npm run tauri:dev` to pick up new Rust command.**
- **JC's test result:** AI panel (slide-in) confirmed working — green light, Rain responded. CLI still shows "Unknown error" because the running binary doesn't have the new command yet.

---

## OPEN BUGS — NEED FIXING

### 🔴 13. ACTIVE FOCUS — Omnix — can't download a model
- **JC's comment (2026-07-01 ~20:25 EDT):** *"good to know. so this is what we need to focus on."* — *Investigated on demand.* LC follow-up: *"You need to thoroughly investigate where the downloads of the models go. Don't guess."*
- **JC's earlier comment:** *"First red flag. I can't download a model with Omnix."*
- **Where the downloads ACTUALLY go (verified by code + filesystem inspection, NOT a guess):**
  - **Destination mechanism:** Chromium Service Worker **Cache API**, not regular files. Models are stored as opaque blob chunks split by SHA-1 hash. Original filenames live in an adjacent LevelDB index. JC CANNOT see them via `*.gguf` searches in Explorer.
  - **Two storage locations coexist on JC's machine:**
    - **`C:\Users\Jatilq\AppData\Roaming\omnix\Service Worker\CacheStorage\...` = 26.4 GB (older, pre-redirect cache).** Most likely contains JC's previously-downloaded Qwen3-27B models. Likely from a version of Omnix before the userData redirect was added.
    - **`E:\ai\OmnixData\Service Worker\CacheStorage\d26cb286488555439586eae38b993292d15546db\` = 4.5 GB (newer, post-redirect cache).** Created by `electron/main.js`'s hardcoded `app.setPath('userData', 'E:\\ai\\OmnixData')`.
  - **Mapped to actual blobs** (E:\ai\OmnixData side): largest = 919 MB, 769 MB, 570 MB, 543 MB, 505 MB, 325 MB, 317 MB — consistent with split shards of an in-progress large model fetch.
  - **NOT in `E:\ai\Models`:** zero `.gguf`/`.onnx`/`.safetensors`/`.bin` files. The 1.8 TB there are from Meridian's own native downloader.
  - **NOT in IndexedDB:** only 16 KB total (`file__0.indexeddb.leveldb` + `http_localhost_9777.indexeddb.leveldb`). Transformers.js v3 uses Cache API, not IndexedDB.
  - **NOT in any visible Roaming folder:** `blob_storage/` empty, `Local Storage/` empty, `DawnWebGPUCache/` is just shader bytecode.
- **Verified evidence (live polling, 20:16–20:30 EDT):**
  - `/api/health` `{ok, 38648}`, `/api/server/status` `{relayActive:true, isElectron:true}` after JC clicks — Electron flipped from `false`→`true`. PID changed 4652→38648, suggesting Electron did spin up a child process during the test.
  - Disk writes to `E:\ai\Models`: **0 files in last 30 min.** 0 files in last 60 sec.
  - `E:\ai\OmnixData\*` modified in last 30 min: **0 files.**
  - C:\Users\Jatilq\AppData\Roaming\omnix\Service Worker/ modified in last 30 min: **needs verification**; last known activity was 2026-06-30 at the E:\ai\OmnixData side.
  - 3 stale `.part` files from 12:32 today (1–3.4 GB). These are from Meridian's native downloader, NOT Omnix.
  - Last successful model write to `E:\ai\Models` was 14:14 today (Qwen3.5-27B.Q4_K_M.gguf, 16.5 GB) — 6 hours BEFORE JC started this test.
- **Why JC's clicks aren't producing new cache bytes:**
  1. The Express relay server (PID 4652 initially) was the only thing listening on `:9777`. During polling, an Electron-forked tsx process spawned (PID 38648), reported `isElectron:true`.
  2. The compute worker is created by `electron/main.js::createWorkerWindow()` which opens `file://${path.join(__dirname, '../dist/index.html')}?mode=worker`. Same React UI as the GUI but headless.
  3. Models are downloaded by `@huggingface/transformers` running inside that headless worker, fetched via standard `fetch()` to HF Hub URLs.
  4. **`@huggingface/transformers` does NOT auto-attach a Bearer token** to its downloads. JC's likely test cases: gated HF repos like `mradermacher/...Claude-Opus...abliterated` and `unsloth/Qwen3.6-27B-MTP` — any with `gated: true` on HF will return 401 silently, write 0 bytes to cache, and Transformers.js treats failures identically to "still loading" in the UI.
- **Fix options in priority order (kill ONE root cause at a time):**
  1. **Bypass the cache (best long-term):** write a Meridian-native download path that fetches via `reqwest` with HF Bearer auth (bug #3), saves as a normal `.gguf` in `E:\ai\Models\<author>\<repo>/`, then configures Transformers.js to load from `file://` instead of HF Hub. Removes the broken-down Omnix download path entirely. **Pairs with #3.**
  2. **Inject HF Bearer token into Transformers.js requests** — patch `electron/main.js` to expose `process.env.HF_TOKEN` to the renderer; or open a global `fetch` override in the preload that adds `Authorization: Bearer <hf_token>` automatically when origin is `huggingface.co`.
  3. **Cache extraction tool (recovery for already-downloaded 26 GB):** parse the LevelDB index in `Roaming\omnix\Service Worker` and `E:\ai\OmnixData\Service Worker`, identify blob chunks by cache key, stitch them back into real `.gguf` files in `E:\ai\Models\<matching-id>/`. Use a library like `node-chromium-cache-parser` or write a custom LevelDB reader. **This unlocks the 26 GB that's already on disk.**
  4. **Watchdog:** if Omnix says "downloading" but `E:\ai\OmnixData\Service Worker` + `Roaming\omnix\Service Worker` byte counts don't increase for >2 min, surface a toast + log to SQLite.
- **Files (concrete):**
  - `E:\ai\Apps\Omnix\electron\main.js` — has userData redirect, spawns worker
  - `E:\ai\Apps\Omnix\electron\preload.cjs` — no download-Model API exposed
  - `E:\ai\Apps\Omnix\package.json`, `dist/server.cjs` — relay only (no model download code)
  - `E:\ai\Apps\Omnix\download_all_models.py` and 3 other test_*.py scripts — JC's prior probes. NOT yet read.
  - `candidate_results.md` (594 bytes) — JC's notes from prior probe. NOT yet read.
- **Open questions (defer until JC says go):**
  - Why `/api/server/status` reports `isElectron:true` even when an Electron process isn't in tasklist (resource accounting mismatch?)
  - What does JC's `download_all_models.py` actually do? What did `candidate_results.md` capture?
  - Did OmniX-data's most recent modification timestamp mean the downloads that ARE in E:\ai\OmnixData are stale (1+ day old with no new activity during today's session)?
- **Status:** 🔴 **ACTIVE FOCUS** — diagnosis now CONFIRMED. NOT a feature request — broken symmetry between Omnix's "downloading" UI and the actual cache writes (HF auth silent no-op + invisible Service Worker blob destination that JC can't see in Explorer).

---

### 3. Model Search download buttons do nothing (no feedback, no HF auth)
- **JC's comment:** "I chose a model to download and nothing happens when I click any of the download buttons"
- **Root cause (diagnosed):** Two issues:
  1. **No HF auth headers:** The `downloader_enqueue` Rust command has zero HTTP header support. HuggingFace model downloads require `Authorization: Bearer hf_xxx` headers. Without them, downloads silently 401/403.
  2. **No visible feedback:** Downloads enqueue silently to the app's downloads dir (not the models folder, which is empty post-wipe). No toast, no progress indicator, no notification. The downloader popover has a progress bar but it's hidden unless manually opened.
- **Fix needed:**
  - Add `headers: Option<HashMap<String, String>>` to `downloader_enqueue`, `start_download`, `download_direct`, `download_chunked`, `fetch_head`, `fetch_chunk` in `src-tauri/src/downloader.rs`
  - Thread HF Bearer token from `userSettingsStore.userSettings.meridian.githubToken` (or add a dedicated `huggingFaceToken` field) through the download pipeline
  - Add toast notification on download start
  - Auto-open downloader popover when a download is enqueued from Hardware Scanner
- **Files:** `src-tauri/src/downloader.rs`, `src/modules/hardware/pages/hardware.vue`, `src/modules/downloader/downloader-toolbar-button.vue`
- **JC's classification:** BUG — the download buttons exist and are clickable, they just don't do anything visible. Not a feature request.
- **Status:** 🔧 Designed, not implemented

### 4. Backend Manager → Local Models search/filter bar doesn't work
- **JC's comment:** "The filter search bar in backend manager does not work. I'm trying to filter only 4B models and it's not doing it. I type in gemma and it does not filter. This is the local models."
- **Root cause:** Not yet diagnosed — need to read the filter logic in `backend-manager.vue`
- **Fix needed:** Diagnose and fix the Models tab search/filter
- **Files:** `src/modules/backend-manager/pages/backend-manager.vue`
- **JC's classification:** BUG — the filter bar exists in the UI and is interactive, it just doesn't filter. Not a feature request.
- **Status:** 🆕 Not yet diagnosed

### 5. Settings — no Save/Apply button (values look like placeholders)
- **JC's comment:** "There needs to be a save and or apply button so you know that changes have been made to settings. You need to be able to know something has been configured because you have what looks like placeholder data."
- **Root cause:** Settings auto-save on change but there's ZERO visual feedback. No dirty state indicator, no save confirmation, no toast.
- **Fix needed:** Add a "Save" button OR a toast notification ("Settings saved") OR a dirty-state indicator (unsaved dot/asterisk) to all Meridian settings panels
- **Files:** `src/modules/settings/ui/categories/meridian/*.vue`
- **JC's classification:** BUG — settings panel exists but gives zero feedback. User can't tell if anything was configured. Broken, not a new feature.
- **Status:** 📝 UX bug

### 6. Downloads — no progress feedback
- **JC's comment:** "There has to be many elements that communicate to the user something is happening. I just tried to download an Omnix model, it says it's queued but I have no clue if it is or if it's even downloading. We need more status bars. Better communicating with user."
- **Root cause:** The polling infrastructure exists (`downloader_get_state` + 500ms poll timer in `downloader-toolbar-button.vue`) but visual feedback is minimal. The popover has a progress bar but it's hidden unless you manually open it. No toast on start/complete, no speed, no ETA.
- **Fix needed:**
  - Inline progress bar visible without opening the popover (on the toolbar button badge)
  - Toast notification on download start ("Downloading: filename.gguf")
  - Live % + speed + ETA in the downloader popover
  - Toast notification on download complete
- **Files:** `src/modules/downloader/downloader-toolbar-button.vue`
- **JC's classification:** BUG — the download system exists and claims to queue items, but the user gets zero confirmation it's working. Broken feedback loop, not a new feature.
- **Status:** 📝 UX bug

### 7. Backend Manager → Models tab needs filtering/sort
- **JC's comment:** "The models tab should have options to filter models that I already have. Sort by quantization, size and author — Unsloth for instance."
- **Why it's a BUG not a feature request:** The Models tab EXISTS in the UI. It's a panel the user navigates to. But it lacks the basic filtering/sorting controls that make it functional. A tab that exists but can't do what it's supposed to do is broken, not a missing feature.
- **Fix needed:** Add sort/filter controls to the Models tab: sort by size, sort by quantization type, filter by author/quantizer, show only models that fit hardware
- **Files:** `src/modules/backend-manager/pages/backend-manager.vue`
- **Status:** 🆕 Bug

### 8. Backend Manager — RPC Slaves tab duplicated
- **JC's comment:** "Don't fully understand why the RPC Slaves tab is in the Backend Manager if it's already in the Topology tab."
- **Why it's a BUG:** The tab EXISTS in two places doing the same thing. That's confusing and broken navigation — the user doesn't know where to go.
- **Fix needed:** Remove the Workers tab from Backend Manager. Keep RPC slave management in Topology/Cluster Control where the hardware topology already lives. Backend Manager should focus on: install/manage backends, manage models.
- **Files:** `src/modules/backend-manager/pages/backend-manager.vue`
- **Status:** 🆕 Bug

### 9. Backend Manager → Backends tab end-to-end unverified
- **JC's comment:** "The backends tab is amazing if it works. And I have yet to see it work."
- **Why it's a BUG:** The tab EXISTS and LOOKS polished but has never actually been confirmed working. A feature that exists in the UI but doesn't function is broken.
- **Fix needed:** Verify the full pipeline: download backend binary → start it → load a GGUF model → verify inference responds. This is the core value prop of Phase 11.
- **Files:** `src-tauri/src/backend_manager.rs`, `src/modules/backend-manager/pages/backend-manager.vue`
- **Status:** 🔴 Critical bug — needs e2e test pass

### 10. Settings — GitHub PAT field confusing
- **JC's comment:** "Why is the app asking for the GitHub Personal Access Token?"
- **Why it's a BUG:** The field EXISTS and prompts the user for a token they may not need. It looks mandatory but is actually optional. Broken UX that confuses and intimidates users.
- **Root cause:** The PAT is optional — it's only used by Backend Manager for elevated GitHub API rate limits when downloading backend binaries. But the field looks required and there's no explanation.
- **Fix needed:** Add "(Optional)" label, inline help text explaining when/why it's needed, or hide it until Backend Manager actually needs it
- **Files:** `src/modules/settings/ui/categories/meridian/install-paths.vue`
- **Status:** 🆕 Bug

### 13. Omnix — can't download a model
- **JC's comment:** "First red flag. I can't download a model with Omnix."
- **Why it's a BUG:** Omnix's model download flow is part of its core feature set (per the Omnix GUI it ships with). If the user can't download a model from Omnix — whether via the GUI or via Meridian's UI routing to Omnix — the program can't do its primary job.
- **Root cause:** Not yet diagnosed. Need JC to confirm:
  1. Which surface triggered the failure: Omnix GUI directly (`C:\Users\Jatilq\AppData\Local\Programs\omnix\Omnix.exe`) vs Meridian → AI Panel → model selector vs Meridian → Hardware Scanner → Download button vs Meridian → Backend Manager → Models → Download.
  2. What the failure looked like: button greyed out / click does nothing / error message text / partial download / hangs forever / authentication fail.
  3. Which model: which HF model id, which quant, which size — so we can reproduce.
  4. Whether it has anything to do with the Omnix compute worker status (WebGPU worker might not be ready, so download side panel reuses same bridge — see P0 above).
- **Likely relation to other bugs:**
  - Possibly linked to #3 (HF auth missing in downloader — but that path is Meridian's native downloader, not Omnix's).
  - Possibly linked to P0 (compute worker not ready → derived page also broken).
  - Could be a fresh bug in Omnix itself (Omnix is third-party, version-controlled at LoanLemon/Omnix). Easiest fix would be: route model downloads through Meridian's own downloader (bug #3 fix) instead of through Omnix's internal download.
- **Files (suspected):** `E:\ai\Apps\Omnix\dist\server.cjs` (Omnix 0.7.0 server bundle), `E:\ai\Apps\Omnix\download_all_models.py`, plus any Tauri command that proxies model downloads to Omnix if there is one.
- **Status:** 🆕 Bug — needs clarification from JC before diagnosis can start

---

## FIXES ALREADY COMMITTED

### 11. Dev server EACCES on port 5173
- **Commit:** `b1f07bce` — `fix(dev-server): retarget Vite to port 1420 -- Windows reserved 5134-5233`
- **Root cause:** Windows Hyper-V/WSL2 reserves TCP 5134-5233 on this host. 5173 + 5174 both in that range.
- **Status:** ✅ Fixed + committed

### 12. Omnix not spawning on boot
- **Commit:** `65bfcd54` — `fix(omnix): spawn engine on app boot + drop stale localStorage fallback`
- **Root cause:** `spawn_omnix` was only called from `setUseOmnix(true)` (user click handler). Fresh install had `useOmnix=true` from default but no click ever fired.
- **JC's test result:** Confirmed working — "engine activation sequence in progress" observed on boot
- **Status:** ✅ Fixed + committed, verified by test

---

## SUMMARY

| Category | Count |
|---|---|
| Fixed in working tree (needs tauri:dev restart) | 2 |
| Fixed + committed | 2 |
| Open bugs needing fix | 8 |
| **Total issues discovered** | **12** |

**Recommended fix order:**
1. P0: Omnix deep-dive (understand the architecture before fixing symptoms)
2. #2: Verify omnix_text works after tauri:dev restart
3. #4: Backend Manager filter bar (broken)
4. #3: Download buttons + HF auth (broken)
5. #9: Backend Manager e2e verification (critical)
6. #5 + #6: Settings save feedback + download progress (UX)
7. #7 + #8: Models tab filters + remove duplicate Workers tab
8. #10: GitHub PAT UX clarification
