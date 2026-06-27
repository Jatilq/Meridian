# Meridian — Session Starter

## Paste this at the start of every Claude Code session:

---

Read CLAUDE.md, then AGENTS.md, then DESIGN.md.

Then scan the project tree:
`dir E:\ai\Projects\Meridian\ /s /b | findstr /v "node_modules" | findstr /v ".git" | findstr /v "\dist\" | findstr /v "\target\"`

Based on the file tree, identify what phase we are in. The phases are:
- Phase 5: Omnix embedded as hidden BrowserWindow (CURRENT PRIORITY)
- Phase 6: Cluster Control Panel (SSH slave launcher, node monitoring)
- Phase 7: SSH/SFTP File Browser (remote panes)
- Phase 8: Agent Coding Extension
- Phase 9: Package and installer

Read only the source files relevant to the current phase. Do not read the entire codebase.

Give me a one-paragraph status summary: what phase we are on, what is done, what the next step is. Wait for my confirmation before doing anything.

---

## The Vision (remind yourself every session)

Meridian is a local-first AI workstation. File manager + embedded Omnix AI engine + cluster control (MAMBA + BLACK = 52GB VRAM) + SSH/SFTP remote access + agent coding extension. One Electron app. One installer. Everything local. No cloud.

---

## If Something Is Broken:

Read CLAUDE.md. Do not attempt fixes yet.
Run the app and capture the full error. Read files mentioned in the error. Identify root cause. Tell me what you found and propose one fix. Wait for go-ahead.

---

## If You Want a Status Report:

Read CLAUDE.md, AGENTS.md, DESIGN.md. Scan the project tree. Give a full status: phases complete, in progress, missing, next action.

---

## Model Tips

- Use Nemotron 1M context for full codebase audits and planning
- Use Qwen3.6 35B for writing code
- Keep sessions focused on one phase
- Start new session if context feels stale — paste this prompt again
