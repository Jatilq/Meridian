# CLAUDE.md — Meridian Agent Behavior

## Who You Are Working For

James. Retired Verizon FiOS employee. Homelab AI enthusiast. Non-programmer — does not write code, does not want to. Two-machine inference setup: MAMBA (192.168.1.67, 3× RTX 3060) and BLACK (192.168.1.64, RX 6900 XT). Uses 9Router as OpenAI-compatible proxy. All development is agent-driven.

## The Project

Meridian is a fork of Sigma File Manager (**Tauri 2 + Vue 3 + Rust**, GPL3) — *not Electron*. The backend is Rust in `src-tauri/` (Tauri commands via `#[tauri::command]`, registered in `src-tauri/src/lib.rs` with `generate_handler!`); the frontend is Vue 3 + TypeScript in `src/` and calls the backend with `invoke()` from `@tauri-apps/api/core`. SQLite is `rusqlite` (`meridian.db` in app data); HTTP/networking is `reqwest` + `axum`; async runtime is `tokio`. Sigma is excellent. Your job is to add three things on top without breaking or redesigning what Sigma already does:
1. AI panel connected to 9Router/local models
2. IDM-style parallel chunk downloader + browser extension
3. Omnix local AI integration

Read DESIGN.md. Read AGENTS.md. Then read Sigma's existing code. Then act.

---

## Hard Rules

### Never:
- Ask James to edit a file manually
- Ask James to run more than one command
- Report a task complete without verifying
- Guess at a fix — always diagnose first
- Redesign or replace Sigma's existing UI
- Hardcode endpoint URLs, IPs, or file paths
- Execute destructive file operations without a confirmation dialog
- Add new JS/TS frameworks or Rust crates not already in the project (check `package.json` and `src-tauri/Cargo.toml` first)

### Always:
- Read CLAUDE.md before starting
- Read Sigma's source before modifying it
- Match Sigma's coding patterns exactly (Vue 3 Composition API + `<script setup lang="ts">`; Rust `#[tauri::command]` registered in `lib.rs`)
- Verify each phase builds and runs before moving to the next
- Show previews before any rename/move/delete
- Write and run scripts for multi-step operations yourself

---

## When Something Is Broken

1. Read the full error — do not truncate
2. Identify root cause
3. State what you believe the cause is
4. Apply one fix
5. Verify it worked
6. Report: what was wrong, what you changed, confirmed working

No random fixes. No asking James to try things.

---

## When You Are Unsure

State the uncertainty clearly. Propose two options maximum. Ask James to choose. Never proceed on assumptions for anything that affects files or project structure.

---

## AI Panel — Special Care

The AI panel executes file operations from model output. Most sensitive part of the project.

- Never execute organize/rename/delete without a confirmation dialog
- Dialog must show: what changes, which files, visible Cancel button
- If model response is not valid JSON → display as plain chat, do not execute
- Log every AI action to SQLite: timestamp, intent, files affected, outcome, confirmed/cancelled

---

## Downloader — Special Care

- Never overwrite existing files without asking
- Chunk reassembly: verify final file size matches Content-Length before deleting temp chunks
- Browser extension: only intercept URLs the user explicitly clicks — no silent background capture
- Queue must resume on restart, not restart from zero

---

## Reporting to James

When a phase is done:
- What was built (one sentence per feature)
- How to test it (one action James can take)
- Known limitations if any
- Nothing else — concise only

---

## Context Window Management (for local models)

If running on Qwen3.6 35B or similar via 9Router:
- Do not read the entire Sigma codebase at once
- Read only files relevant to the current task
- Use file tree scan first to orient, then targeted reads
- If context feels stale, say so — James will start a new session with START_SESSION.md