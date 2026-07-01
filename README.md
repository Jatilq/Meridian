# Meridian

> **Beta** — Phase 11 (Backend Manager) is substantially built with deferred items; core AI, cluster, and downloader features are production-ready. See [What's New](#whats-new) and [Backend Manager](#backend-manager) for current status.

**A local-first AI workstation built on [Sigma File Manager](https://github.com/aleksey-hoffman/sigma-file-manager).**

Meridian combines a powerful file manager with an embedded AI assistant, multi-node GPU cluster management, and a three-tier inference engine — letting anyone, on any hardware, run real local AI without touching a command line.

No subscriptions. No cloud. No data leaving your machine — unless you choose to connect one yourself.

---

## What's New

### Redesigned Cluster Topology View
The Cluster Control panel now has an SVG network diagram with distinct device-shaped icons: a **workstation/server tower** silhouette for the primary MAMBA node and a **desktop gaming tower** silhouette for worker nodes. Each icon contains a vertical VRAM fill-bar gauge, floating stat badge (GPU util%, temperature), and monospace memory text (e.g. `36.0GB/36.0GB (100%)`). A dashed connection line with bi-directional arrowheads connects nodes, glowing bright when an RPC session is active. The "Launch RPC Slave" button appears only on worker nodes.

### AMD VRAM Detection Fix
The registry-walk probe (`WMI Win32_VideoController` → `AdapterRAM`) now correctly reads VRAM on AMD cards. RX 6900 XT was previously reported as 4 GB due to nvidia-smi-only probing; it now shows the full 16 GB, bringing combined cluster VRAM from 20 GB to the correct **52 GB**.

### HuggingFace Model Browser
The Hardware Scanner panel lets you search HuggingFace for GGUF models with:
- Search by name (1-3 character queries auto-append `+GGUF` to target quantized repos)
- Filters by architecture (Llama, Qwen, Mistral, DeepSeek, etc.) with exclude-IQ toggle
- Sort by downloads, trending, or recency
- VRAM-fit badge showing whether each model fits your combined cluster pool
- Trusted quantizer indicators (Bartowski, Unsloth, MaziyarPanahi, LoneStriker)

### New Inference Backends
Beyond the original llama.cpp, llamafile, and koboldcpp, Backend Manager now supports:
- **TurboQuant** — llama.cpp fork with `--triattention-*` flags; CUDA 12.4 / 13.3 / CPU / Vulkan variants
- **Lemonade** — single-binary server that auto-detects NVIDIA, AMD, Intel NPU, or CPU; OpenAI-compatible on port 13305

### Cluster Workers / SSH Connections Split
The architecture now cleanly separates **clusterWorkers** (used by Cluster Control and Backend Manager for inference orchestration) from **sshConnections** (used by the file-browser remote pane for SFTP file operations). This prevents the earlier bug where removing a file-browser SSH connection would also remove a cluster worker, or vice versa.

---

## The Idea

Most local AI tools assume you already know what GGUF is, already have CUDA installed, already know how to compile llama.cpp. Meridian doesn't assume any of that.

You install Meridian. **Rain** — the AI built into the file manager — greets you and works immediately, no setup, on basically any hardware. From there, you can grow:

- Want smarter AI? Download a model from the built-in catalog.
- Have a gaming PC and an old laptop? Add the laptop as a worker — Meridian pools both GPUs' VRAM automatically.
- Already run Ollama or have an OpenAI key? Point Rain at it.

Meridian meets you wherever you are on the local-AI journey.

---

## Three-Tier AI Engine

Most users have hardware that doesn't fit a single inference stack. Meridian solves this with three tiers, auto-recommended based on your detected hardware:

| Tier | Engine | Hardware | Setup |
|---|---|---|---|
| **1** | Omnix (built-in) | Any GPU, even integrated | Zero config — works immediately |
| **2** | Lemonade | AMD/Intel NPU, mixed hardware | One-click download from Backend Manager |
| **3** | llama.cpp / koboldcpp | Dedicated NVIDIA/AMD GPU | One-click download, auto-matched to your GPU |

Meridian's Hardware Scanner detects your GPU and recommends the right tier — no guessing.

---

## Multi-Node GPU Cluster

This is the feature that doesn't exist anywhere else at this level of simplicity.

**Add any machine as a worker.** An old gaming PC, a laptop, a friend's spare computer — anything with SSH access. The worker needs nothing installed. Meridian copies the inference binary over SFTP and starts it remotely.

- One desktop with a 12 GB GPU + one laptop with a 6 GB GPU = an 18 GB combined pool
- Live topology map shows every node, its hardware, and connection status
- One click to launch an RPC slave on any worker
- Combined VRAM updates automatically as workers connect

No Docker. No Kubernetes. No command line on the worker machine.

---

## Rain — Your AI Assistant

Rain lives inside Meridian. Direct, a little dry when it matters — built to feel like a knowledgeable colleague, not a help desk.

- **Tool calling** — Rain can actually search, organize, rename, and move your files, not just describe how to
- **Persistent memory** — SOUL.md (personality), MEMORY.md (what Rain learns about you), FAVORITES.md (your habits) carry across sessions
- **Vision & voice** — analyze images and speak responses via Omnix, fully local
- **Confirmation gates** — every destructive action shows a preview before it happens
- **Model agnostic** — works with the built-in engine, a local server you run, or your own API key (OpenAI, Anthropic, OpenRouter, Groq)

---

## Core File Manager

Built on Sigma File Manager — dual-pane layout, tabs, drive cards, media thumbnails, bookmarks, extensions system, and everything else that makes Sigma one of the best open-source file managers available.

---

## SSH/SFTP Remote Browser

Remote machines appear as panes right alongside local drives. Browse, copy, move, rename, delete — across local and remote — without switching tools.

---

## Smart Downloader

- yt-dlp powered — YouTube, Twitch, hundreds of sites
- Parallel chunk downloading for direct file URLs
- Persistent queue with pause/resume/cancel
- Browser extension (Chrome/Edge) for one-click capture

---

## Backend Manager

One panel to manage your entire local inference stack:

- **Backends tab** — download and run llama.cpp, llamafile, koboldcpp, TurboQuant, Lemonade — auto-matched to your GPU
- **Models tab** — scan and manage your local GGUF model library
- **RPC Slaves tab** — manage your cluster workers

### Current status

| Feature | Status |
|---|---|
| Download backends | ✅ Working (auto-selects CUDA/ROCm/CPU) |
| Launch / stop backends | ✅ Working |
| GPU vendor detection | ✅ Working (name parsing + WMI + rocm-smi) |
| 5 backend types supported | ✅ Working |
| Models scan + delete | ✅ Working |
| Copy backend to worker via SFTP | ✅ Working |
| Launch RPC slave via SSH | ✅ Working |
| Backend catalog in `backends.json` | ✅ Working (Vite-bundled) |
| **Deferred (planned)** | |
| `backend_catalog.json` as bundled resource | ⏳ Catalog still in source code |
| Install progress emission (all backends) | ⏳ Partial |
| Models tab folder browser | ⏳ Needs file picker integration |
| `reap_backends` on window destroy | ⏳ Not wired |
| Backend Manager settings subsection | ⏳ Not in Settings panel yet |

---

## Screenshots

> Manual capture still needed. Once you place PNG files in `docs/screenshots/`, the markdown below will render automatically.

<!-- Screenshots: replace comment with actual images when captured -->
<!-- ![Cluster Topology](docs/screenshots/cluster-topology.png) -->
<!-- ![Model Browser](docs/screenshots/model-browser.png) -->
<!-- ![File Manager](docs/screenshots/file-manager.png) -->
<!-- ![AI Panel](docs/screenshots/ai-panel.png) -->

---

## Getting Started

### Requirements
- Windows 10/11
- For cluster features: SSH access to any worker machines (nothing else required on them)

### Installation
1. Download the installer from [Releases](https://github.com/Jatilq/Meridian/releases)
2. Run it
3. Launch Meridian — Rain handles the rest

### First Run
Rain's onboarding walks you through:
- Picking a download folder
- Choosing how you want AI to work (built-in / your own server / API key / basic features)
- You're set in under a minute

---

## Building from Source

```bash
git clone https://github.com/Jatilq/Meridian.git
cd Meridian
npm install
npm run tauri:dev
```

Requirements: Node.js 18+, Rust, Tauri CLI 2.x

---

## Architecture

```
Meridian (Tauri 2 + Vue 3 + Rust)
├── Sigma File Manager (base — file ops, UI, extensions)
├── Rain (AI assistant — tool calling, memory, personality)
├── Three-Tier AI Engine
│   ├── Tier 1: Omnix (embedded, zero-config)
│   ├── Tier 2: Lemonade (downloadable, AMD/Intel NPU)
│   └── Tier 3: llama.cpp / koboldcpp (downloadable, dedicated GPU)
├── Cluster Control (multi-node SSH workers, live topology map)
├── Backend Manager (download/manage inference engines + models)
├── SSH/SFTP Browser (remote file panes)
└── Smart Downloader (yt-dlp + parallel chunks + browser extension)
```

---

## Roadmap

- Agent coding extension — Rain reads/writes code with diff preview, works on local or remote files
- Extension marketplace additions — web search, more Rain capabilities
- More worker discovery options

---

## Credits

Meridian is built on **[Sigma File Manager](https://github.com/aleksey-hoffman/sigma-file-manager)** by Aleksey Hoffman. Licensed under GPL3.

Additional integrations:
- **[Omnix](https://github.com/LoanLemon/Omnix)** — embedded local AI engine
- **[Lemonade](https://github.com/lemonade-sdk/lemonade)** — multi-hardware inference server
- **[TurboQuant](https://github.com/AtomicBot-AI/TurboQuant)** — llama.cpp fork with tri-attention
- **[yt-dlp](https://github.com/yt-dlp/yt-dlp)** — video downloading
- **[llama.cpp](https://github.com/ggml-org/llama.cpp)** — reference inference engine

See [CREDITS.md](CREDITS.md) for full attribution.

---

## License

GPL3 — see [LICENSE](LICENSE).

---

## Contributing

Personal hobby project, built almost entirely through AI-agent-driven development. Issues and pull requests welcome.
