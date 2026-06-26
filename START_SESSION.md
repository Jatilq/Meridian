# Meridian — Session Starter

## Paste this at the start of every Claude Code session:

---

Read CLAUDE.md, then AGENTS.md, then DESIGN.md.

Then run this command to get the project structure without reading every file:
`dir E:\ai\Projects\Meridian\ /s /b | findstr /v "node_modules" | findstr /v ".git"`

Based on the file tree, identify what phase we are in by checking which phases from AGENTS.md have already been completed. Read only the source files directly relevant to today's task — do not read the entire codebase at once.

Then give me a one-paragraph status summary: what phase we are on, what is done, and what the next step is. Wait for my confirmation before doing anything.

---

## If Something Is Broken, Paste This:

Read CLAUDE.md. Do not attempt any fixes yet.

Run the app and capture the full error output. Read the files mentioned in the error. Identify the root cause. Tell me what you found and propose one fix. Wait for my go-ahead before applying it.

---

## If You Want a Status Report, Paste This:

Read CLAUDE.md, AGENTS.md, and DESIGN.md. Then scan the project file tree (exclude node_modules and .git). Based on what exists, give me a full status report: what phases are complete, what is in progress, what is missing, and what the next action should be.

---

## Model Tips (Qwen3.6 35B locally)

- Keep sessions focused on one phase at a time
- If context feels like it is getting stale, start a new session with the starter prompt above
- For large file reads (Sigma source), ask it to summarize rather than dump full contents
- Nemotron at 1M context is better for full codebase audits — use it for planning, Qwen3.6 for writing
