# Rain Extension API — Tier 3 Tool Bridge

**Status:** Design proposal (not implemented). Awaiting JC approval before coding.

**Date:** June 30, 2026

---

## Problem

Rain CLI currently has 9 hardcoded tools in `rain_tools.rs`:

| Tool | Category | Risk Level |
|---|---|---|
| `list_directory` | File ops | Safe (read-only) |
| `read_file` | File ops | Safe (read-only) |
| `search_files` | File ops | Safe (read-only) |
| `create_folder` | File ops | Low |
| `move_files` | File ops | Destructive (confirmation gated) |
| `rename_item` | File ops | Destructive (confirmation gated) |
| `delete_item` | File ops | Destructive (confirmation gated) |
| `write_file` | File ops | Destructive (confirmation gated) |
| `run_shell_command` | Shell | Destructive (confirmation gated) |

These are **Tier 2 baseline** — safe-by-default, no extension required. Every user gets them.

But there's no mechanism for an installed extension to **add a new tool** that Rain can call. Rain's capability set is frozen at install time. The extension marketplace can add sidebar pages, context menu items, and toolbar buttons — but not Rain tools.

---

## Vision

Rain's three-tier capability model:

| Tier | Surface | Capability | Requires |
|---|---|---|---|
| **1** | AI Panel | Lightweight file assistant, zero-config | Built-in (Omnix) |
| **2** | Rain CLI | Full terminal, 9 baseline tools, markdown rendering | Built-in |
| **3** | Rain CLI + extensions | Extended tools via installed extensions | Extension install |

The extension marketplace is the mechanism that grants Rain CLI additional capabilities. Rain herself can be used to **build** Tier 3 extensions (scaffolding code with her baseline file + shell tools), but the user must explicitly install/enable the extension before it activates.

**Guardrail (non-negotiable):** Rain writing extension code ≠ Rain installing or activating it. Any extension Rain drafts must go through the normal user-facing install/enable flow. Rain can build the tool, but cannot silently grant herself the capability.

---

## Proposed API

### Extension Manifest Addition

Extensions declare Rain tools in their `manifest.json` (or `package.json` for Sigma extensions):

```json
{
  "name": "my-extension",
  "meridian": {
    "rainTools": [
      {
        "name": "web_search",
        "description": "Search the web and return results. Uses DuckDuckGo API.",
        "parameters": {
          "type": "object",
          "properties": {
            "query": {
              "type": "string",
              "description": "The search query"
            },
            "maxResults": {
              "type": "integer",
              "description": "Maximum results to return (default 5)"
            }
          },
          "required": ["query"]
        },
        "permissionLevel": "read-only",
        "handler": "./tools/web-search.js"
      }
    ]
  }
}
```

### Permission Levels

Every Rain tool declares a `permissionLevel` that maps to Meridian's existing confirmation-gate system:

| Level | Behavior | Examples |
|---|---|---|
| `read-only` | Executes immediately, no confirmation | web_search, get_weather, read_config |
| `write` | Confirmation card before execution | create_file, update_database |
| `destructive` | Confirmation card + warning before execution | delete_remote, reset_config |
| `shell` | Confirmation card + command preview + warning | run_command, install_package |

The UI reuses the existing confirmation card pattern from `ai-panel.vue` / `rain-cli.vue` — the `DESTRUCTIVE` set check already gates `move_files`, `rename_item`, `delete_item`, `write_file`, `run_shell_command`. Extension tools with `permissionLevel` >= `write` join this set automatically.

### Tool Handler Contract

Each tool handler is a JavaScript module that exports an async function:

```javascript
// tools/web-search.js
export default async function webSearch(params, context) {
  const { query, maxResults = 5 } = params;
  const response = await fetch(`https://api.duckduckgo.com/?q=${encodeURIComponent(query)}&format=json`);
  const data = await response.json();
  return {
    results: data.AbstractText ? [{ title: 'Result', snippet: data.AbstractText }] : [],
    source: 'DuckDuckGo'
  };
}
```

The `context` object provides:
- `context.appPath` — the app's data directory
- `context.currentPath` — the user's current navigator path (if in file manager)
- `context.log(message)` — writes to the rain_tool_log SQLite table
- `context.emit(event, data)` — sends events to the frontend (progress updates, etc.)

**No filesystem or shell access by default.** The handler runs in a sandboxed VM (Sigma's existing `run_extension_command` infrastructure). Filesystem/shell access requires `permissionLevel: "shell"` and the extension to declare the `filesystem` or `shell` capability in its manifest.

### Registration Flow

1. User installs an extension via the marketplace (existing Sigma flow).
2. Extension loader reads `manifest.json`, finds `meridian.rainTools`.
3. For each tool entry, the loader registers it in a **global tool registry** (a new `RainToolRegistry` state managed by Tauri).
4. When Rain CLI's agent loop calls `rain_tool_schemas`, the response includes both the 9 baseline tools AND any registered extension tools.
5. When Rain calls a registered extension tool, `rain_run_tool` dispatches to the extension's handler via the existing `run_extension_command` infrastructure.

### Unregistration Flow

1. User uninstalls or disables an extension.
2. Extension loader removes the extension's tools from the global registry.
3. Next `rain_tool_schemas` call no longer includes those tools.

---

## Implementation Plan

### Phase A — Tool Registry (Rust side)

**New file:** `src-tauri/src/rain_tool_registry.rs`

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredRainTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub permission_level: String, // "read-only" | "write" | "destructive" | "shell"
    pub extension_id: String,
    pub handler_path: String,
}

pub type RainToolRegistry = Arc<Mutex<HashMap<String, RegisteredRainTool>>>;
```

**New Tauri commands:**
- `register_rain_tool(extension_id: String, tool: RegisteredRainTool)` — adds to registry
- `unregister_rain_tools(extension_id: String)` — removes all tools for an extension
- `list_rain_extension_tools()` — returns all registered extension tools (for debugging/settings)

**Modify `rain_tool_schemas`:** After returning the 9 baseline tools, append any registered extension tools from the registry.

**Modify `rain_run_tool`:** If the tool name matches a registered extension tool, dispatch to the extension's handler via `run_extension_command` instead of the hardcoded match arms.

### Phase B — Extension Loader Integration (Rust side)

**Modify `src-tauri/src/extensions.rs`:** In the existing extension install/load flow, after reading the manifest, check for `meridian.rainTools`. For each tool entry:
1. Validate the handler file exists in the extension directory.
2. Call `register_rain_tool` to add it to the registry.

On extension uninstall/disable, call `unregister_rain_tools`.

### Phase C — Frontend Integration (Vue side)

**Modify `ai-panel.vue` and `rain-cli.vue`:**
- The `DESTRUCTIVE` set is no longer hardcoded — it's built from the baseline set PLUS any extension tools with `permissionLevel >= "write"`.
- Extension tool confirmation cards show the extension name (so the user knows which extension provides the tool).

**Modify Settings → Meridian → Rain:**
- New section: "Extension Tools" — lists all registered extension tools with their permission level and extension name.
- Toggle to disable individual extension tools without uninstalling the extension.

### Phase D — Rain Self-Improvement (Optional, later)

Rain CLI can scaffold a new extension with her baseline tools:
1. User asks: "Build me a web search tool for Rain"
2. Rain generates the extension manifest + handler code using her file-writing tools.
3. Rain writes the files to a staging directory.
4. User reviews and installs via the marketplace (explicit action).

**No auto-install path.** The staging directory is separate from the installed extensions directory. Rain cannot write directly to the extensions directory.

---

## Security Considerations

1. **Sandboxed execution.** Extension tool handlers run in the same sandboxed VM as existing extension commands (`run_extension_command`). No direct filesystem or network access unless explicitly declared and user-approved.

2. **Permission escalation prevention.** An extension cannot upgrade its own permission level after installation. The manifest is read once at install time; changes require re-installation (which triggers the user approval flow).

3. **Tool name collision.** If two extensions register a tool with the same name, the second registration fails with an error. Extensions must namespace their tools (e.g., `myext_web_search` not `web_search`).

4. **Rate limiting.** Extension tool calls go through the same max-10-iterations agent loop as baseline tools. An extension cannot create an infinite loop.

5. **Audit logging.** All extension tool calls are logged to the `rain_tool_log` SQLite table with the extension ID, just like baseline tool calls.

---

## What This Does NOT Change

- The AI Panel (Tier 1) is unaffected — it uses its own tool set, not the Rain CLI tool registry.
- The 9 baseline tools remain hardcoded and available to all users.
- Rain's personality, memory, and identity rules are unchanged.
- The extension marketplace UI/UX is unchanged — extensions just gain a new `meridian.rainTools` manifest field.
- Existing extension capabilities (sidebar pages, context menus, toolbar buttons) are unchanged.

---

## Open Questions

1. **Should extension tools be available in the AI Panel (Tier 1) too, or only Rain CLI (Tier 2+)?** Proposal: Rain CLI only. The AI Panel is the lightweight surface; extension tools are for power users.

2. **Should the handler run in-process (Node VM) or out-of-process (separate Node worker)?** Sigma's existing `run_extension_command` uses in-process. For security, out-of-process would be better but adds complexity. Proposal: start with in-process (matching existing Sigma patterns), migrate to out-of-process if security issues arise.

3. **Should Rain be able to suggest extensions?** e.g., "I don't have a web search tool, but there's one in the marketplace. Want me to show you?" This would be a nice Tier 2→3 discovery path. Proposal: yes, but only suggest, never auto-install.
