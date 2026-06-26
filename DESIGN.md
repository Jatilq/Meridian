# Meridian — Design Document

## Core Philosophy

Meridian is Sigma File Manager with three additions. Do not redesign Sigma. Do not change Sigma's existing UI, layout, colors, or behavior unless directly required to integrate a new Meridian feature. Sigma's design is the design — match it exactly for all new components.

> Stack note: Meridian is **Tauri 2 + Vue 3 + Rust** (not Electron). New backend logic goes in Rust in `src-tauri/` and is exposed to the Vue frontend as Tauri commands (`#[tauri::command]`, registered in `lib.rs`, called with `invoke()`). Networking uses `reqwest`/`axum` (already in `Cargo.toml`); persistence uses `rusqlite` (`meridian.db`).

---

## Sigma's Existing Design (reference — do not change)

- Dark theme: near-black backgrounds, subtle borders, clean typography
- Left icon sidebar for navigation (home, files, bookmarks, settings, extensions)
- Home page: hero banner with background image, user directory shortcuts, drive cards with usage %
- File view: grouped by type (FOLDERS / VIDEOS / OTHER FILES), card thumbnails for media
- Right panel: file/folder metadata (size, path, items, dates)
- Tabs at top with folder names
- Breadcrumb navigation bar

All new Meridian UI must match this aesthetic — same colors, same font sizes, same border radius, same spacing rhythm.

---

## Addition 1: AI Panel

### Placement
Collapsible panel, right side of file view or bottom bar — whichever integrates more cleanly with Sigma's existing layout. Toggle with `Ctrl+Space`.

### Components
- **Endpoint field:** text input, default: `http://localhost:11434` (user will set to 9Router URL)
- **Model dropdown:** populated by `GET /v1/models` from configured endpoint — refreshes when endpoint changes
- **Omnix toggle:** switch to use Omnix at `http://localhost:7770/api` instead
- **Input field:** natural language, placeholder: `Ask about files or give an instruction...`
- **Result area:** scrollable, shows AI response or action result
- **Send:** Enter key or button

### System Prompt (injected automatically)
```
You are a file management assistant inside Meridian.
Current directory: {current_path}
Selected files: {selected_files}
Directory listing: {file_list}

Respond ONLY with JSON:
{
  "intent": "search|organize|analyze|rename|chat",
  "scope": "current|selected|all",
  "preview_only": true,
  "action": {},
  "message": "human readable explanation"
}
```

### Intent Routing
| Intent | Behavior |
|---|---|
| `search` | Highlight matching files in current pane |
| `organize` | Show proposed move/rename plan, await confirmation |
| `analyze` | Show summary in result area, no file action |
| `rename` | Show batch rename diff, await confirmation |
| `chat` | Show response in result area, no file action |

### Safety
- Never execute organize / rename / delete without confirmation dialog
- Dialog shows exactly what changes, which files, with visible Cancel
- Log all executed actions: timestamp, intent, files, outcome, confirmed/cancelled

---

## Addition 2: Enhanced Downloader

Sigma already has a downloader. Extend it — do not replace it.

### Format/Quality Selector
- Before any download, fetch formats via `yt-dlp --list-formats` (or `-J` for JSON)
- User picks: Video+Audio / Audio only / quality level
- Remember last choice per domain

### Parallel Chunk Downloading (IDM-style)
- For direct file URLs (not yt-dlp streams): check `Accept-Ranges: bytes` header
- If supported: split into N chunks (default 8), download in parallel, reassemble
- Implemented in Rust in `src-tauri/` using `reqwest` + `tokio` (already in `Cargo.toml`) — not Node
- Show individual chunk progress merged into one overall bar
- Verify file size after reassembly before deleting temp chunks

### Download Queue
- All downloads go through queue (max concurrent: configurable, default 3)
- Per-item controls: Pause / Resume / Cancel
- Queue persists across restarts via SQLite (`meridian.db` / `rusqlite`)
- Shows: filename, source domain, progress %, speed, ETA

### Auto-Save
- Configurable default download folder in settings
- Optional: auto-organize into subfolders (Videos/ Audio/ Files/)

### Browser Extension (Chrome/Edge)
- Manifest V3
- Detects video/audio on page (m3u8, mp4, webm, common CDN patterns)
- Overlay button on detected media elements
- Click sends URL to Meridian. The receiver reuses Sigma's existing `axum` HTTP server pattern (see `src-tauri/src/lan_share/`) to expose a `localhost` endpoint (e.g. `POST /download`), with CORS headers so the extension can POST. (Optionally a Tauri command reachable via `@tauri-apps/plugin-http`.) This is a Rust-side HTTP server, not a Node server.
- Meridian opens format selector, adds to queue
- Extension needs CORS headers on Meridian's receiver

---

## Addition 3: Omnix Integration

- Settings toggle: "Use Omnix for AI"
- When enabled: Meridian spawns `omnix --silent --dependent-pid <pid>` on startup (from Rust via `src-tauri/src/omnix.rs`, which exposes `spawn_omnix` / `kill_omnix` / `get_omnix_status` Tauri commands)
- Meridian shuts Omnix down on exit (via dependent-pid it is automatic)
- AI panel calls `/api/text` for text queries, `/api/vision` for image files
- Status dot in AI panel: green = Omnix online, grey = offline
- Falls back to configured endpoint if Omnix is offline

---

## Settings Panel (new Meridian section within Sigma's Settings)

Add a **Meridian** category in Sigma's existing Settings sidebar:

| Setting | Type | Default |
|---|---|---|
| AI endpoint URL | text | `http://localhost:11434` |
| Default model | text | (blank = use dropdown) |
| Use Omnix | toggle | off |
| Auto-start Omnix | toggle | off |
| Omnix path | text | auto-detect |
| Download folder | folder picker | Downloads |
| Max concurrent downloads | number | 3 |
| Chunk count | number | 8 |
| Auto-organize downloads | toggle | off |
| Browser extension status | read-only | shows connected/disconnected |

---

## What NOT to Change in Sigma

- Home page layout and hero banner
- Navigation sidebar icons and behavior
- File grouping and thumbnail display
- Existing downloader UI (extend only)
- Existing extensions system
- Existing search
- Color scheme, fonts, spacing, border radius