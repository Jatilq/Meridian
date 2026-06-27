# CLAUDE.md — Meridian Agent Behavior

## Who You Are Working For

JC. Hobbyist. Non-programmer — does not write code, does not want to. Runs a two-machine AI inference homelab: MAMBA (<MAMBA_IP>, 3× RTX 3060, 36GB VRAM) and BLACK (<BLACK_IP>, RX 6900 XT, 16GB VRAM). Combined via llama.cpp RPC: 52GB effective VRAM. Uses 9Router as OpenAI-compatible proxy. All development is agent-driven.

## The Project

Meridian is a local-first AI workstation built on Sigma File Manager (Electron + Vue, GPL3). It combines:
- File management (Sigma)
- Embedded local AI engine (Omnix — hidden BrowserWindow inside Meridian's Electron process)
- Natural language file operations (AI panel → Omnix or 9Router)
- Cluster control (SSH slave launcher, MAMBA + BLACK = 52GB)
- Remote file access (SSH/SFTP browser)
- Agent coding (extension using full model pool)

Everything runs locally. Nothing cloud-dependent except by user choice.

Read DESIGN.md. Read AGENTS.md. Read existing source. Then act.

---

## Hard Rules

### Never:
- Ask JC to manually edit a file
- Ask JC to run more than one command
- Report complete without verifying
- Guess — always diagnose first
- Redesign Sigma's existing UI
- Hardcode IPs, ports, credentials, or paths
- Execute destructive operations without confirmation dialog
- Add frameworks not already in the project
- Store credentials in plaintext

### Always:
- Read CLAUDE.md before starting
- Read source files before modifying them
- Match Sigma's existing patterns (Vue components, IPC, store)
- Verify each phase before moving to next
- Show previews/diffs before file changes
- Run scripts yourself, don't ask JC to run them

---

## When Something Is Broken

1. Read full error output — never truncate
2. Identify root cause
3. State the cause clearly
4. Apply one targeted fix
5. Verify it worked
6. Report: what was wrong, what changed, confirmed working

No random fixes. No asking JC to try things.

---

## When Unsure

State the uncertainty. Propose two options max. Ask JC to choose. Never proceed on assumptions for anything affecting files, credentials, or project structure.

---

## JC's Workflow — Read This Carefully

JC manages this project asynchronously. He is often on another machine doing other things and checks in when he thinks the agent is ready for input. He is NOT sitting watching the terminal.

This means:
- If JC does not respond, he is busy — WAIT. Do not start a timer and proceed when it expires.
- Never make unilateral decisions because a timeout was reached. Hold your position and wait indefinitely.
- If you need a decision before proceeding, state clearly what you need and stop. Do not proceed, do not guess, do not pick the "safe" option on your own.
- The only exception: if the current operation is clearly safe and reversible (like running a read-only diagnostic), you may complete it and report. Never take external or destructive actions without explicit confirmation.
- Push to GitHub, SSH commands, file deletions, and anything with external side effects always require explicit confirmation — no timeouts apply.

---

## AI Panel — Special Care

- Never execute organize/rename/delete without confirmation dialog
- Confirmation shows: what changes, which files, visible Cancel
- If model response not valid JSON → show as plain chat, don't execute
- Log every AI action: timestamp, intent, files, outcome, confirmed/cancelled

---

## SSH/SFTP — Special Care

- Never store credentials in plaintext — use Electron safeStorage
- Never log SSH passwords or key contents
- All destructive remote operations (delete, overwrite) require confirmation
- If SSH connection drops mid-operation, report clearly, do not retry silently

---

## Omnix Embedding — Special Care

- Hidden BrowserWindow must NOT be offscreen — WebGPU requires real GPU context
- If Omnix server fails to start, Meridian still launches — AI panel shows Omnix offline
- Never block main window render on Omnix startup
- One restart attempt if compute worker crashes, then mark offline

---

## Cluster Control — Special Care

- SSH credentials for cluster control same rules as SSH/SFTP
- RPC slave command is configurable — never hardcode it
- Confirm before launching slave (it consumes BLACK's full GPU)
- Show clear status when combined pool is active vs MAMBA only

---

## Reporting to JC

When a phase is done:
- What was built (one sentence per feature)
- How to test it (one action JC can take)
- Known limitations if any
- Commit hash
- Nothing else — concise only

---

## Context Management (local models)

If running on Qwen3.6 35B or similar via 9Router:
- Do not read entire Sigma codebase at once
- Scan file tree first, then targeted reads
- One phase at a time
- If context stale, say so — JC will start fresh session with START_SESSION.md

---

## The Vision (never lose sight of this)

Meridian is the tool that doesn't exist yet: a local-first AI workstation where the file manager is the shell for everything. File ops, embedded AI, cluster management, remote access, agent coding — one Electron app, one installer, runs entirely on JC's own hardware. No subscriptions, no cloud, no data leaving the machine.

Every decision should serve this vision. If a shortcut compromises it, take the longer road.
