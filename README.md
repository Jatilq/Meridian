# Meridian

**A fork of Sigma File Manager with local AI integration and IDM-style downloading.**

Meridian is built on top of [Sigma File Manager](https://github.com/aleksey-hoffman/sigma-file-manager) (Electron + Vue, GPL3) — one of the best open-source file managers available. Rather than rebuild what Sigma already does beautifully, Meridian adds the features Sigma is missing:

1. **Local AI panel** — natural language file operations routed through 9Router to any local model
2. **IDM-style downloader** — multi-segment parallel downloading, browser interception, queue management
3. **Omnix integration** — lightweight on-device AI for file analysis without hitting MAMBA

## What Sigma Already Provides (don't rebuild these)

- Beautiful dark UI with hero banner and custom backgrounds
- Home page with drive cards showing usage %
- Dual-pane layout with tabs
- File grouping by type with video thumbnails
- Built-in yt-dlp video downloader
- Extensions system
- WSL drive integration
- Breadcrumb navigation, bookmarks, search

## What Meridian Adds

### 1. AI Panel
- Collapsible panel matching Sigma's existing UI style
- OpenAI-compatible endpoint (default: 9Router)
- Model selector fetching from `/v1/models`
- Natural language input with current directory context injected
- Intent routing: search / organize / analyze / rename / chat
- Model returns JSON action — confirmation required before destructive ops
- All AI actions logged to SQLite

### 2. IDM-Style Downloader (extending Sigma's existing yt-dlp downloader)
- Multi-segment parallel downloading via HTTP range requests
- Download queue with pause / resume / cancel per item
- Format and quality selector before download starts
- Auto-save to configurable default folder
- Chrome/Edge browser extension to intercept video URLs and send to Meridian automatically

### 3. Omnix Integration
- Optional mode toggle in AI panel settings
- When enabled, calls Omnix local API at `http://localhost:7770/api`
- Zero overhead AI path when MAMBA models are busy
- Vision endpoint: auto-sends selected image with query to `/api/vision`

## Stack

- **Base:** Sigma File Manager (Electron + Vue)
- **AI:** HTTP fetch to OpenAI-compatible endpoint (9Router → MAMBA/BLACK)
- **Downloader:** yt-dlp (existing) + custom parallel chunk downloader
- **Database:** SQLite (AI action log, download history)
- **Browser Extension:** Chrome/Edge Manifest V3

## Owner

James. Non-programmer. All development is agent-driven. See CLAUDE.md.

## Project Location

`E:\ai\Projects\Meridian\`

## Quick Start for Agents

1. Read CLAUDE.md first — always
2. Read AGENTS.md for build order and technical rules
3. Read DESIGN.md for UI/UX decisions
4. Clone Sigma, get it building, then add Meridian features on top
