# Meridian Configuration

How to configure Meridian's AI and services. All settings live under
**Settings → Meridian**.

## AI Panel

Meridian's AI uses two backends with a clear split:

### Primary AI — 9Router (required for text)

9Router is an OpenAI-compatible proxy that handles **all text inference**.

- **Endpoint URL:** `http://localhost:20128/v1` (default)
- **Model:** `openrouter/openrouter/free` (default — OpenRouter's free auto-router,
  picks a working free model automatically, $0 cost)
- The Model dropdown fetches the live list from `<endpoint>/models`.

The status indicator shows **Connected** when `GET <endpoint>/models` returns 200.

Notes on models:
- Many 9Router providers are billing-gated (ollama subscription, credit-based
  providers, expired OpenAI keys). If a model returns 401/403/400, it needs
  credentials/credits on the 9Router side.
- Free, no-key options confirmed working: `openrouter/openrouter/free`,
  Cloudflare models (`cf/@cf/...`).

### Local AI Enhancement — Omnix (optional)

Omnix is an embedded local AI engine (Electron + transformers.js, WebGPU).
It is **OFF by default**. The app works fully via 9Router without it.

When **Enabled**, Omnix adds:
- **Vision** — analyze image files (FastVLM 0.5B)
- **TTS** — speak responses aloud (Kokoro 82M, voice `af_heart`)
- **Director** — intent classification (small Qwen models)

Omnix runs as a separate hidden Electron process spawned by Meridian on startup
when the toggle is on. It listens internally on `http://localhost:9777` — the
user never needs to configure this; it is fixed.

Sub-option:
- **Speak responses (TTS)** — only shown when Omnix is enabled.

### Capability matrix (verified)

| Capability | Backend | Status |
|---|---|---|
| Text inference | 9Router | Working (free models) |
| Vision (images) | Omnix FastVLM | Working |
| Text-to-speech | Omnix Kokoro | Working |
| Director intent | Omnix / small Qwen | Working |
| Speech-to-text | Omnix Whisper | NOT supported (ONNX incompatibility) |

## Omnix install (one-time, for the optional local AI)

Omnix lives at `E:\ai\Apps\Omnix`. One-time setup:

```
cd E:\ai\Apps\Omnix
npm install
```

Model cache is redirected to `E:\ai\OmnixData` (off the C: drive). Models
download on first use into the Electron cache (transformers.js,
`allowLocalModels=false` — they cannot be pre-placed as loose files).

Confirmed-working Omnix models (others fail on the engine):
- `LemOneLabs/Qwen3-0.6B-ONNX` (text/Director)
- `LemOneLabs/Qwen2.5-0.5B-Instruct-abliterated-ONNX` (text/Director)
- `onnx-community/FastVLM-0.5B-ONNX` (vision)
- `onnx-community/Kokoro-82M-v1.0-ONNX` (TTS)

Models ≥1B and non-Qwen architectures (Llama 8B, Mistral 12B, etc.) fail on
Omnix's transformers.js (unsupported architecture or WebGPU memory limits).

## Downloader

- **Auto-save folder:** destination for downloads (blank = prompt each time)
- **Chunk count:** parallel download segments (default 8)

## Cluster Control

SSH-based control of the inference cluster, accessible via the Cluster Control sidebar icon.

### Features (all built)

- **Add Worker dialog** — label, host, port, username, auth method (SSH key or password)
- **Key/password toggle** — switch between key-file and password authentication
- **Test Connection** — verifies SSH connectivity before saving
- **Live hardware polling** — GPU utilization, temperature, VRAM usage, CPU, RAM for every connected node
- **SVG topology map** — workstation/server tower icons for MAMBA (primary), desktop gaming tower for workers, connection lines with RPC status
- **Combined VRAM** — auto-summed across all connected nodes (e.g. 36 GB local + 16 GB remote = 52 GB)
- **Launch RPC Slave** — one-click SSH launch of llama.cpp RPC server on any worker node
- **Per-node detail cards** — CPU name/cores, RAM used/total, per-GPU breakdown with util%/VRAM/temp
- **Empty-state onboarding** — guides users to add their first worker when no connections exist

### Hardware reference

| Machine | IP | CPU | GPU | VRAM | RAM |
|---|---|---|---|---|---|
| MAMBA | 192.168.1.67 | Xeon E5-2697v4 36c | 3× RTX 3060 | 36GB | 256GB |
| BLACK | 192.168.1.64 | Ryzen 9 5950X 16c | RX 6900 XT | 16GB | 64GB |
| Combined | — | — | — | 52GB | 320GB |

Passwords are encrypted via Tauri's secure storage before persisting. Key files use the path on disk directly.
