# Meridian — Agent Build Instructions

## Project Owner

James. Retired. Non-programmer. All development is agent-driven. He does not write code, edit files manually, or run multi-step command sequences. Agents do everything.

## Golden Rules

1. Never ask James to manually edit a file
2. Never ask James to run more than one command
3. Diagnose before acting — no guess-and-check
4. Never report completion without verifying
5. No destructive file operations without preview and confirmation
6. Do not modify Sigma's existing code unless required for integration

---

## Base Project

- Repo: `https://github.com/aleksey-hoffman/sigma-file-manager`
- Stack: **Tauri 2 + Vue 3 + Rust** (not Electron). Backend: Rust in `src-tauri/`, frontend: Vue 3 + TypeScript in `src/`
- License: GPL3
- Local path: `E:\ai\Projects\Meridian\`

---

## Phase 0 — Always Do This First

1. Read CLAUDE.md
2. Read DESIGN.md
3. Run file tree scan (exclude node_modules, .git, dist, src-tauri\target):
   `dir E:\ai\Projects\Meridian\ /s /b | findstr /v "node_modules" | findstr /v ".git" | findstr /v dist | findstr /v target`
4. Identify current phase based on what exists
5. Read only files relevant to current task — not the whole codebase

---

## Phase 1 — Get Sigma Running

1. Clone Sigma: `git clone https://github.com/aleksey-hoffman/sigma-file-manager E:\ai\Projects\Meridian\`
2. Check `package.json` (`scripts`) and `src-tauri/Cargo.toml` for correct install and dev commands
3. Run `npm install`
4. Run dev command: `npm run tauri:dev` (declared in `package.json`; equivalent to `tauri dev`, which runs the Vite frontend + Rust backend)
5. Confirm Sigma launches and UI renders
6. Do not proceed until confirmed working

**Completion check:** Sigma home page visible with drive cards and hero banner.

---

## Phase 2 — AI Panel

1. Read Sigma's existing Vue component structure — identify patterns for adding new panels
2. Read Sigma's state management (Pinia stores in `src/stores/runtime/`)
3. Read Sigma's Tauri command setup — commands are Rust `#[tauri::command]` in `src-tauri/src/*.rs`, registered in `src-tauri/src/lib.rs` via `generate_handler!`, and called from the frontend with `invoke()` from `@tauri-apps/api/core`
4. Create AI panel Vue component matching Sigma's style
5. Add toggle in Sigma's existing toolbar — do not add new sidebar nav items
6. Implement endpoint config + model selector (fetch `/v1/models`)
7. Implement natural language input with context injection (see DESIGN.md system prompt)
8. Implement intent routing — parse JSON response, route to handler
9. Add confirmation dialog for organize/rename/delete intents
10. The project already uses `rusqlite` with `meridian.db` in app data (see `src-tauri/src/downloader.rs`) — add tables there, do not create a separate database
11. Add Omnix toggle and vision mode (image file selected → `/api/vision`)

**Completion check:** AI panel opens with Ctrl+Space, model dropdown populates, query returns response.

---

## Phase 3 — Enhanced Downloader

1. Read Sigma's existing downloader code completely before touching it (`src-tauri/src/downloader.rs` and `src/modules/downloader/`)
2. Identify how Sigma calls yt-dlp (child process spawned from Rust via `std::process::Command` / `tokio::process`, surfaced through Tauri commands — not Node child_process)
3. Add format/quality selector — call `yt-dlp -J <url>` and parse JSON for formats
4. Build parallel chunk downloader for direct URLs (Rust in `src-tauri`, using `reqwest` + `tokio` already in `Cargo.toml`):
   - HEAD request → check `Accept-Ranges: bytes` and `Content-Length`
   - Split into N chunks, parallel fetch with Range headers
   - Write chunks to temp files, reassemble in order
   - Verify total size, delete temp files
5. Extend queue UI — add Pause/Resume/Cancel per item
6. Add SQLite tables: `download_queue`, `download_history` (in `meridian.db` via `rusqlite`)
7. Add configurable auto-save folder
8. Build browser extension:
   - `E:\ai\Projects\Meridian\browser-extension\`
   - Manifest V3, content script detects media URLs, background worker sends to Meridian
   - Meridian receiver: reuse the existing `axum` HTTP server pattern (see `src-tauri/src/lan_share/`) to expose a `localhost` endpoint — *not* a Node HTTP server. Optionally expose it as a Tauri command reachable via `@tauri-apps/plugin-http`. Either way, send CORS headers so the extension can POST.
   - Route: `POST /download` → opens format selector, adds to queue

**Completion check:** Paste a direct MP4 URL, Meridian downloads it in chunks with progress bar. Paste a YouTube URL, format selector appears.

---

## Phase 4 — Settings Integration

1. Read how Sigma stores and loads settings (`src/stores/runtime/settings.ts` + `src/stores/storage/user-settings.ts`, persisted via `tauri-plugin-store`)
2. Add Meridian section to Sigma's existing Settings UI (see DESIGN.md settings table)
3. Wire all values to their features
4. Persist via Sigma's existing config system (`tauri-plugin-store`)

**Completion check:** Settings panel shows Meridian section, endpoint URL saves and persists across restarts.

---

## Phase 5 — Omnix Integration

1. Detect Omnix binary (check common install locations, or use path from settings)
2. On AI panel enable: spawn `omnix --silent --dependent-pid <meridian-pid>` (spawn from Rust; `omnix.rs` already has `spawn_omnix` / `kill_omnix` / `get_omnix_status` commands registered in `lib.rs`)
3. Add status indicator in AI panel (ping `/api/text` with empty prompt to check)
4. Route text queries to `/api/text`, image queries to `/api/vision`
5. Fall back to configured endpoint if Omnix offline

**Completion check:** Toggle Omnix on, status dot turns green, send a query, response returns.

---

## Phase 6 — Package and Polish

1. Verify all features end to end
2. Build: `npm run tauri:build` (declared in `package.json`; equivalent to `tauri build`)
3. Create Start Menu shortcut via PowerShell
4. Update README with any Meridian-specific setup steps

**Completion check:** Built .exe launches, all features work, shortcut in Start Menu.

---

## Technical Rules

- Match Sigma's Vue patterns exactly (Vue 3 Composition API + `<script setup lang="ts">`)
- Use Sigma's existing Pinia stores (`src/stores/runtime/`) — add modules, don't replace
- Use Sigma's existing Tauri commands where possible (Rust `#[tauri::command]`, called from the frontend with `invoke()`)
- No new npm packages or Rust crates unless truly necessary — downloader uses Rust in `src-tauri` (reqwest/tokio/rusqlite already in `Cargo.toml`), not Node stdlib
- Browser extension: no external dependencies, single content script + background worker
- Never hardcode IPs, ports, or paths — always read from settings

---

## Hardware Reference

| Machine | IP | GPU | Role |
|---|---|---|---|
| MAMBA | 192.168.1.67 | 3× RTX 3060 (36GB) | Primary inference |
| BLACK | 192.168.1.64 | RX 6900 XT 16GB | Daily driver |
| 9Router | localhost | — | OpenAI proxy |
| Omnix | localhost:7770 | — | On-device AI |