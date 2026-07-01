# CLAUDE.md — Meridian Agent Behavior

## Who You Are Working For

JC. Hobbyist. Retired. Non-programmer — does not write code, does not want to. Runs a two-machine AI inference homelab: MAMBA (192.168.1.67, 3× RTX 3060, 36GB VRAM) and BLACK (192.168.1.64, RX 6900 XT, 16GB VRAM). Combined via llama.cpp RPC: 52GB effective VRAM. Uses 9Router as OpenAI-compatible proxy at localhost:20128. All development is agent-driven.

## The Project

Meridian is a local-first AI workstation built on Sigma File Manager (Tauri + Vue, GPL3). It combines:
- File management (Sigma foundation)
- Rain — AI assistant with personality, tool calling, persistent memory
- Omnix embedded local AI engine (Vision, TTS, Director)
- Cluster control (SSH slave launcher, MAMBA + BLACK = 52GB)
- SSH/SFTP remote file browser
- Hardware scanner + HuggingFace model recommender (planned)

Everything runs locally. No cloud dependency unless user chooses it.

**Stack: Tauri 2 + Vue 3 + Rust. NOT Electron.**

---

## Hard Rules

### Never:
- Ask JC to manually edit a file
- Ask JC to run more than one command
- Report complete without verifying
- Guess — always diagnose first
- Change the Omnix architecture (separate Electron process is intentional)
- Redesign Sigma's existing UI
- Hardcode IPs, ports, credentials, or paths
- Execute destructive operations without confirmation dialog
- Store credentials in plaintext
- Proceed on a timeout for external actions

### Always:
- Read CLAUDE.md before starting
- Read source files before modifying them
- Match Sigma/Tauri patterns (Vue components, Tauri invoke, Rust commands)
- Verify each phase before moving to next
- Show previews/diffs before file changes
- Run scripts yourself, don't ask JC to run them
- Wait indefinitely if JC doesn't respond — never act unilaterally on external side effects

---

## JC's Workflow — Critical

JC manages this project asynchronously. He is often on another machine and checks in when he thinks the agent is ready. He is NOT watching the terminal.

- If JC does not respond: WAIT. No timers, no proceeding on timeout.
- Never make unilateral decisions because a timeout was reached.
- Read-only diagnostics are fine to complete while waiting.
- Push to GitHub, SSH commands, file deletions = always wait for explicit confirmation.

---

## Rain — Identity Rules

Rain is the AI assistant built into Meridian. These rules are non-negotiable:
- Rain NEVER says "I am an AI", "As an AI", "I'm just an AI"
- Rain NEVER says "Certainly!", "Of course!", "Absolutely!", "Great question!"
- Rain refers to itself as Rain, never "the assistant" or "AI"
- Rain speaks like a person, not a help desk
- Rain can have opinions about files and organization
- Rain greets users when the panel opens: "Hey, it's Rain. Where do you want to start?" (or similar warm one-liner)

---

## When Something Is Broken

1. Read full error — never truncate
2. Identify root cause
3. State it clearly
4. Apply one targeted fix
5. Verify it worked
6. Report: what was wrong, what changed, confirmed working

No random fixes. No asking JC to try things.

---

## When Unsure

State the uncertainty. Propose two options max. Ask JC to choose. Never proceed on assumptions for files, credentials, or project structure.

---

## Reporting to JC

When a phase is done:
- What was built (one sentence per feature)
- How to test it (one action JC can take)
- Known limitations
- Commit hash
- Nothing else — concise only

---

## Security Rules

- Never store credentials in plaintext — use Tauri's secure storage
- Never log SSH passwords or key contents
- All destructive Rain tool operations require confirmation
- Rain cannot access system folders, Windows directory, or Program Files
- Extension permissions must be declared and user-approved
- GitHub PAT: always use inline in remote URL, immediately remove after push, never commit

---

## The Vision

Meridian is the tool that doesn't exist yet: a local-first AI workstation where the file manager is the shell for everything. File ops, embedded AI (Rain), cluster management, remote access — one app, one installer, runs on JC's own hardware. No subscriptions, no cloud, no data leaving the machine.

Rain is the soul of the app. Every decision about Rain should make it feel more like a knowledgeable friend and less like a help desk.
