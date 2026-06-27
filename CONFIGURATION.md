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

## Cluster (Phase 6 — in progress)

SSH-based control of the inference cluster:
- MAMBA `<MAMBA_IP>` (3× RTX 3060, 36GB) — primary inference, headless
- BLACK `<BLACK_IP>` (RX 6900 XT, 16GB) — daily driver, RPC slave
- Combined via llama.cpp RPC: ~52GB effective VRAM

(Configuration UI documented when Phase 6 lands.)
