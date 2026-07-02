<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { ask } from '@tauri-apps/plugin-dialog';
import { useRouter } from 'vue-router';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import type { MeridianBackendKind, MeridianBackendConfig, SshConnectionSetting } from '@/types/user-settings';
import catalogData from '@/data/backends.json';
import omnixCatalogData from '@/data/omnix-catalog.json';

// ============================================================================
// Catalog types — mirror src/data/backends.json shape.
// ============================================================================
type Hardware = 'cpu' | 'nvidia' | 'amd';

// ============================================================================
// Omnix catalog — bundled from src/data/omnix-catalog.json (built from
// E:/ai/Apps/Omnix/src/shared/modelList.ts). Static so we don't have to
// runtime-parse TypeScript upstream.
// ============================================================================
type OmnixCategory =
  | 'text' | 'vision' | 'tts' | 'image-gen' | 'stt' | 'music-gen'
  | 'director' | 'coder' | 'embedding';

interface OmnixCatalogEntry {
  id: string;
  modelID: string;
  name: string;
  description: string;
  size?: string;
  category: OmnixCategory;
  make?: string;
  minRam?: number;
  internal?: boolean;
  verifiedWorking?: boolean;
}

// Raw array carries everything including the synthetic router entry
// ('use-text-model'); the `omnixCatalog` computed below hides `internal` ones.
const omnixCatalogRaw = omnixCatalogData as OmnixCatalogEntry[];

interface BackendVariant {
  id: string;
  label: string;
  hardware: Hardware;
  downloadUrl: string;
  binaryName: string;
  sizeBytes: number;
  sha256: string;
  archiveFormat: 'zip' | 'binary' | 'tar.gz';
  notes: string;
}

interface BackendEntry {
  id: MeridianBackendKind;
  name: string;
  description: string;
  homepage: string;
  binaryNameHint: string;
  variants: BackendVariant[];
}

interface BackendCatalog {
  schemaVersion: string;
  generatedAt: string;
  note: string;
  backends: BackendEntry[];
}

const catalog = catalogData as BackendCatalog;
const userSettingsStore = useUserSettingsStore();

// ============================================================================
// Tauri command payloads — mirror backend_manager.rs camelCase DTOs.
// ============================================================================
interface GpuVendorInfo {
  vendor: Hardware;
  gpuName?: string;
  source: string;
}

type RuntimeStatusKind = MeridianBackendKind;
type RuntimeStatusString = 'notInstalled' | 'installed' | 'running';

interface BackendRuntimeStatus {
  kind: RuntimeStatusKind;
  status: RuntimeStatusString;
  installPath?: string;
  sizeBytes?: number;
  pid?: number;
  startedAt?: number;
  modelPath?: string;
  port?: number;
}

interface BackendApiStatus {
  ok: boolean;
  kind: RuntimeStatusKind;
  port: number;
  urlTested: string;
  elapsedMs: number;
  httpStatus?: number;
  error?: string;
}

// One concrete model file inside a HuggingFace repo — the payload shape
// from `hf_resolve_model_files` (see src-tauri/src/backend_manager.rs).
// Sorted quantized-first by the Rust side so files[0] is the best on-device
// inference asset.
interface HfModelFile {
  filename: string;
  url: string;
  sizeBytes?: number;
}

// Mirror of `backend_manager.rs::BackendKind::default_port()` so the UI's
// pre-fill matches the Rust side out-of-the-box. Keep in lockstep — if
// Rust's `default_port()` changes, this table changes the same day. The
// llama.cpp entry is 11434 because SABnzbd holds 8080 on JC's host and
// JC does not use Ollama (per Day-5 / SESSION_RESULTS conversation).
const DEFAULT_PORTS: Record<MeridianBackendKind, number> = {
  'llama.cpp': 11434,
  'koboldcpp': 5001,
  'llamafile': 8080,
  'turboquant': 8080,
  // Lemonade's upstream `LEMONADE_PORT` env default. Previously 13305;
  // 11434 matches the real LemonadeServer.exe binding on JC's host
  // (verified 2026-07-02 via curl). Keep in lockstep with
  // backend_manager.rs::BackendKind::Lemonade::default_port().
  'lemonade': 11434,
};

// ============================================================================
// Tab state
// ============================================================================
type TabId = 'backends' | 'models' | 'slaves' | 'omnix-models' | 'lemonade-models';

const tabs: { id: TabId; label: string }[] = [
  { id: 'backends', label: 'Backends' },
  { id: 'models', label: 'Models' },
  { id: 'slaves', label: 'RPC Slaves' },
  { id: 'omnix-models', label: 'Omnix Models' },
  { id: 'lemonade-models', label: 'Lemonade Models' },
];

// Live download progress per backend kind, driven by `backend-download-progress`
// Tauri events emitted from backend_manager::download_backend.
interface BackendDownloadProgressPayload {
  kind: string;
  downloaded: number;
  total: number;
  percent: number;
}
const downloadProgress = ref<Record<string, number>>({});
let unlistenProgress: UnlistenFn | undefined;

const activeTab = ref<TabId>('backends');
const router = useRouter();
// The rich HuggingFace search panel lives at /hardware (see
// src/modules/hardware/pages/hardware.vue) — the only place with real
// filters, sort, VRAM-fit, and a Download affordance that lands in the
// existing downloader queue. The Models tab is local-model management,
// so it deep-links into Hardware Scanner for any "find me a model on HF"
// intent rather than duplicating search UX here.
function openHuggingFaceSearch() {
  router.push('/hardware');
}

// ============================================================================
// Per-backend config (read from user-settings, written on change).
// ============================================================================
const backendConfig = computed(() => {
  return userSettingsStore.userSettings.meridian?.backend ?? {
    'llama.cpp': {},
    'llamafile': {},
    'koboldcpp': {},
    'turboquant': {},
    'lemonade': {},
  };
});

// ============================================================================
// Hardware-tier recommendation banner — derived from `detected.vendor`.
// Mirrors the three tiers the user agreed on (Omnix / Lemonade / llama.cpp).
// ============================================================================
interface TierRecommendation {
  tier: string;
  copy: string;
  detail: string;
}

const tierRecommendation = computed<TierRecommendation | null>(() => {
  if (detectedLoading.value) return null;
  const vendor = detected.value?.vendor ?? 'cpu';
  switch (vendor) {
    case 'cpu':
      return {
        tier: 'Tier 1',
        copy: 'Your hardware is best suited for Omnix built-in models.',
        detail: 'Zero-config, runs on any GPU including integrated graphics. Open the Omnix Models tab above.',
      };
    case 'nvidia':
      return {
        tier: 'Tier 3',
        copy: 'llama.cpp CUDA is recommended for your NVIDIA GPU.',
        detail: 'For the largest models, use the llama.cpp CUDA variant. Lemonade is also supported and includes AMD/Intel NPU passthrough for mixed hardware.',
      };
    case 'amd':
      return {
        tier: 'Tier 2 / Tier 3',
        copy: 'Lemonade is recommended for your AMD hardware.',
        detail: 'Supports AMD Radeon GPUs and (separately) Intel NPUs at runtime. llama.cpp ROCm is a strong alternative for big Radeon-only setups.',
      };
    default:
      return null;
  }
});

function getPort(kind: MeridianBackendKind): number {
  const cfg = backendConfig.value[kind];
  return cfg?.port ?? DEFAULT_PORTS[kind];
}

function getModelPath(kind: MeridianBackendKind): string {
  return backendConfig.value[kind]?.modelPath ?? '';
}

function setConfig(kind: MeridianBackendKind, patch: Partial<MeridianBackendConfig>) {
  const current = userSettingsStore.userSettings.meridian?.backend ?? {};
  const merged = { ...(current[kind] ?? {}), ...patch };
  const nextBackend = { ...current, [kind]: merged };
  userSettingsStore.userSettings.meridian.backend = nextBackend;
  // Write the WHOLE backend object to a single storage key. Writing
  // `meridian.backend.${kind}` would let the dot in 'llama.cpp' be parsed
  // as a path separator by `setNestedValue` and corrupt the nested object.
  userSettingsStore.setUserSettingsStorage('meridian.backend', nextBackend);
}

// ============================================================================
// Backends tab state
// ============================================================================
const detected = ref<GpuVendorInfo | null>(null);
const detectedLoading = ref(true);
const statuses = ref<Partial<Record<string, BackendRuntimeStatus>>>({});
const busy = ref<Partial<Record<string, boolean>>>({});
const note = ref<Partial<Record<string, string>>>({});
const apiProbes = ref<Partial<Record<string, BackendApiStatus>>>({});

// User-selected variant per backend; falls back to detected-vendor match.
const selectedVariantId = ref<Partial<Record<string, string>>>({});

function autoPickVariantId(entry: BackendEntry): string {
  const desired = detected.value?.vendor ?? 'cpu';
  const match =
    entry.variants.find((v) => v.hardware === desired) ??
    entry.variants.find((v) => v.hardware === 'cpu') ??
    entry.variants[0];
  return match.id;
}

function getActiveVariant(entry: BackendEntry): BackendVariant {
  const selected = selectedVariantId.value[entry.id];
  if (selected) {
    const found = entry.variants.find((v) => v.id === selected);
    if (found) return found;
  }
  const id = autoPickVariantId(entry);
  return entry.variants.find((v) => v.id === id) ?? entry.variants[0];
}

function selectVariant(entry: BackendEntry, variantId: string) {
  selectedVariantId.value[entry.id] = variantId;
  note.value[entry.id] = `Selected: ${entry.variants.find((v) => v.id === variantId)?.label ?? variantId}`;
}

function formatBytes(bytes: number | undefined | null): string {
  if (!bytes || bytes <= 0) return '—';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

// ============================================================================
// Models tab state
// ============================================================================
const modelsDir = ref(userSettingsStore.userSettings.meridian?.modelsFolder ?? '');
watch(modelsDir, (val) => {
  if (userSettingsStore.userSettings.meridian && val !== userSettingsStore.userSettings.meridian.modelsFolder) {
    userSettingsStore.userSettings.meridian.modelsFolder = val;
    userSettingsStore.setUserSettingsStorage('meridian.modelsFolder', val);
  }
});

interface ModelRow {
  filename: string;
  path: string;
  sizeBytes: number;
  quant: string;
  modifiedAt: number;
}

interface RawModelEntry {
  name: string;
  path: string;
  sizeBytes: number;
  modifiedAt: number;
}

const models = ref<ModelRow[]>([]);
const modelsBusy = ref(false);
const modelsNote = ref('');
// Free-text filter applied to the Models tab list. Matches against
// substring of the filename OR the quant token so typing "llama" or
// "Q4_K_M" both narrow the list sensibly. Case-insensitive.
const modelQuery = ref('');
// User-selected target backend per model (keyed by model.path) so each row
// in the Models tab keeps an independent choice instead of falling back to
// DOM sibling-groping at click time.
const loadTargetFor = ref<Partial<Record<string, MeridianBackendKind>>>({});

const QUANT_RE = /(?:^|[._-])(IQ[1-4]_(?:XXS|XS|S|M|NL)|Q[0-8]_(?:K_S|K_M|Q4_0|Q4_1|Q5_0|Q5_1|Q8_0)|F16|F32|BF16)(?:[._-]|$)/i;

function parseQuant(filename: string): string {
  const match = filename.match(QUANT_RE);
  return match ? match[1].toUpperCase().replace('_', '-') : 'unknown';
}

function formatBytes2(bytes: number | undefined | null): string {
  return formatBytes(bytes);
}

const filteredModels = computed<ModelRow[]>(() => {
  const q = modelQuery.value.trim().toLowerCase();
  if (!q) return models.value;
  return models.value.filter((m) => {
    // Filename (without extension noise) OR quant substring. The user-facing
    // discovery signal here is "I just downloaded llama-3.1-8b-q4 — can I
    // find it?" — a substring match on either the filename or the parsed
    // quant token covers that case while staying simple.
    if (m.filename.toLowerCase().includes(q)) return true;
    if (m.quant.toLowerCase().includes(q)) return true;
    // Size match: typing e.g. "8gb" matches models of that approximate
    // size, which is the second-most-common filtering intent. When the
    // user types a bare number, infer the unit from magnitude: <= 32 is
    // almost certainly GB (anything smaller is a tiny embedding), > 32
    // is almost certainly MB (single GB models don't ask for a filter at
    // "8gb" by leaving the unit off). This avoids the footgun where
    // typing "700" alone silently defaults to GB and misses a 700MB file.
    const sizeMatch = q.match(/^(\d+(?:\.\d+)?)\s*(gb|mb)?$/i);
    if (sizeMatch) {
      const target = parseFloat(sizeMatch[1]);
      const explicitUnit = sizeMatch[2]?.toLowerCase();
      const unit = explicitUnit ?? (target <= 32 ? 'gb' : 'mb');
      const actualGb = m.sizeBytes / 1024 / 1024 / 1024;
      const tolerance = Math.max(0.5, actualGb * 0.15);
      if (unit === 'mb' && Math.abs(actualGb * 1024 - target) <= 0.5) return true;
      if (unit === 'gb' && Math.abs(actualGb - target) <= tolerance) return true;
    }
    return false;
  });
});

// ============================================================================
// RPC Slaves tab state — populated from user-settings.clusterWorkers.
// Round-26 reset: previously this read `sshConnections` (same array the
// file-browser remote panes used). Cluster infrastructure now has its own
// dedicated `meridian.clusterWorkers` array; the file-browser-owned
// `sshConnections` is no longer surfaced here.
// ============================================================================
interface SlaveRow {
  name: string;
  host: string;
  port: number;
  username: string;
  keyPath: string;
  role: string;
}

function mapClusterWorkerToSlave(conn: SshConnectionSetting, _index: number): SlaveRow {
  return {
    name: conn.label || `${conn.username}@${conn.host}`,
    host: conn.host,
    port: conn.port,
    username: conn.username,
    keyPath: conn.keyPath || '',
    role: 'llama.cpp RPC slave (worker)',
  };
}

const slaves = computed<SlaveRow[]>(() => {
  const list = userSettingsStore.userSettings.meridian?.clusterWorkers;
  return Array.isArray(list) ? list.map(mapClusterWorkerToSlave) : [];
});

// ============================================================================
// Omnix Models tab state — bundled catalog + HF cache scan.
// Mirrors src-tauri/src/omnix.rs::InstalledHfModel.
// ============================================================================
interface InstalledHfModel {
  repoId: string;
  path: string;
}

// Hide internal routing helpers (`use-text-model`) from the user-facing list
// while still allowing them in the bundled JSON so the catalog stays in
// lockstep with upstream modelList.ts.
const omnixCatalog = computed<OmnixCatalogEntry[]>(() =>
  omnixCatalogRaw.filter((m) => !m.internal)
);

const omnixInstalled = ref<InstalledHfModel[]>([]);
const omnixInstalledSet = computed(() => new Set(omnixInstalled.value.map((m) => m.repoId)));
// The original badge code reported the wrong number: it computed
// `omnixInstalledSet.size / omnixCatalog.length` directly, which can
// produce bogus ratios like "32 of 20" because the two numbers were
// surveying different universes — the HF cache scan returns repoIds
// for EVERYTHING the user has downloaded across all projects (other
// apps, manual downloads, etc.), while `omnixCatalog.length` counts
// only the bundled model entries. The badge now reports the
// intersection: how many catalog entries have a matching cached
// download. UX-wise this is "X of Y installed" where X ≤ Y by
// construction. The full HF cache count is reported separately as
// "+N other in cache" so the user still sees their raw cache footprint.
// All three derived values share one `catalogModelIds` Set under the
// hood — the alternative (each computed rebuilding its own Set on every
// reactive read) duplicated work for no gain at N≈30 entries.
const omnixCatalogModelIds = computed(() => new Set(omnixCatalog.value.map((m) => m.modelID)));
const omnixInstalledInCatalogSet = computed(() => {
  const ids = omnixCatalogModelIds.value;
  return new Set(
    omnixInstalled.value
      .map((m) => m.repoId)
      .filter((id) => ids.has(id)),
  );
});
const omnixOtherCacheCount = computed(() => {
  const ids = omnixCatalogModelIds.value;
  return omnixInstalled.value.filter((m) => !ids.has(m.repoId)).length;
});
const omnixBusy = ref(false);
const omnixNote = ref('');

// ============================================================================
// Lemonade Models tab state (Day-7 Phase 8). Mirrors the omnix tab pattern
// but drives Lemonade's native lifecycle endpoints via 5 Tauri commands in
// src-tauri/src/lemonade_manager.rs. Destructive ops (delete) gate behind
// a Tauri `ask()` confirmation dialog per AGENTS.md.
// ============================================================================
interface LemonadeModelInfo {
  id: string;
  recipe?: string | null;
  labels: string[];
  sizeBytes?: number | null;
  downloaded: boolean;
  loaded: boolean;
  checkpoint?: string | null;
}
const lemonadeModels = ref<LemonadeModelInfo[]>([]);
const lemonadeBusy = ref(false);
const lemonadeNote = ref('');
const lemonadeError = ref('');

async function refreshLemonade(): Promise<void> {
  lemonadeBusy.value = true;
  lemonadeNote.value = '';
  lemonadeError.value = '';
  try {
    lemonadeModels.value = await invoke<LemonadeModelInfo[]>('lemonade_list_models', {
      endpoint: null,
    });
  }
  catch (error) {
    lemonadeModels.value = [];
    lemonadeError.value = `Lemonade unreachable on :11434 — ${error}`;
  }
  finally {
    lemonadeBusy.value = false;
  }
}

async function lemonadeAction(action: 'load' | 'unload' | 'pull', modelId: string): Promise<void> {
  lemonadeBusy.value = true;
  lemonadeNote.value = `${action} ${modelId}…`;
  try {
    const command = action === 'load' ? 'lemonade_load'
      : action === 'unload' ? 'lemonade_unload'
        : 'lemonade_pull';
    await invoke<string>(command, { modelName: modelId, endpoint: null });
    lemonadeNote.value = `${modelId}: ${action} ok`;
    await refreshLemonade();
  }
  catch (error) {
    lemonadeNote.value = `${action} failed: ${error}`;
  }
  finally {
    lemonadeBusy.value = false;
  }
}

async function lemonadeDelete(model: LemonadeModelInfo): Promise<void> {
  const sizeLabel = formatBytes2(model.sizeBytes);
  const confirmed = await ask(
    `Delete ${model.id} (${sizeLabel}) from Lemonade's on-disk cache? This cannot be undone.`,
    { title: 'Delete Lemonade model', kind: 'warning', okLabel: 'Delete', cancelLabel: 'Keep' },
  );
  if (!confirmed) {
    lemonadeNote.value = `Delete cancelled for ${model.id}`;
    return;
  }
  lemonadeBusy.value = true;
  lemonadeNote.value = `delete ${model.id}…`;
  try {
    await invoke<string>('lemonade_delete', { modelName: model.id, endpoint: null });
    lemonadeNote.value = `${model.id}: deleted`;
    await refreshLemonade();
  }
  catch (error) {
    lemonadeNote.value = `delete failed: ${error}`;
  }
  finally {
    lemonadeBusy.value = false;
  }
}

async function refreshOmnix(): Promise<void> {
  omnixBusy.value = true;
  omnixNote.value = '';
  try {
    omnixInstalled.value = await invoke<InstalledHfModel[]>('scan_huggingface_cache');
  }
  catch (error) {
    omnixInstalled.value = [];
    omnixNote.value = `Could not scan HuggingFace cache: ${error}`;
  }
  finally {
    omnixBusy.value = false;
  }
}

// RAM gate only — Omnix runs on WebGPU/WASM so every GPU vendor (NVIDIA /
// AMD / Intel / integrated) works. This function intentionally ignores GPU
// vendor; the heavier 8B/12B entries still warn so users on small machines
// know what they are getting into. Rename later if a real GPU gate lands.
function omnixRamGate(entry: OmnixCatalogEntry): 'fits' | 'heavy' {
  const ramRequired = entry.minRam ?? 0;
  return ramRequired > 8 ? 'heavy' : 'fits';
}

async function downloadOmnixModel(entry: OmnixCatalogEntry): Promise<void> {
  omnixBusy.value = true;
  omnixNote.value = `Resolving ${entry.name} (${entry.size ?? 'unknown size'})…`;
  try {
    // Synthetic router entry — the catalog marks these with `internal: true`
    // and the UI filter strips them, but guard here too in case a future
    // catalog leaves one in. Nothing to download; the local router handles it.
    if (entry.modelID === 'use-text-model' || entry.internal) {
      omnixNote.value = `${entry.name} is a routing helper, not a downloadable model.`;
      return;
    }
    // Ask the Rust side to enumerate the repo's actual model files
    // (.onnx / .gguf / .bin / .safetensors / .pt), ranked quantized-first.
    // The pre-fix code queued `https://huggingface.co/<repo>` which is an
    // HTML page — the downloader fetched the README instead of a model.
    const files = await invoke<HfModelFile[]>('hf_resolve_model_files', {
      repoId: entry.modelID,
    });
    if (files.length === 0) {
      omnixNote.value = `No downloadable model files found in ${entry.modelID}. Use the HF page link to pick one manually.`;
      return;
    }
    const file = files[0];
    // Hand the file URL to the existing downloader queue so it lands in
    // the configured auto-save folder (typically E:\ai\Models\).
    await invoke('downloader_enqueue', {
      url: file.url,
      file_name: file.filename,
      format_id: null,
      auto_save_folder: userSettingsStore.userSettings.meridian?.modelsFolder ?? '',
      chunk_count: null,
    });
    const total = files.length;
    omnixNote.value = total > 1
      ? `Queued ${file.filename} from ${entry.modelID} (1 of ${total} files — others available on the HF page).`
      : `Queued ${file.filename} from ${entry.modelID}.`;
  }
  catch (error) {
    omnixNote.value = `Failed to queue ${entry.name}: ${error}`;
  }
  finally {
    omnixBusy.value = false;
  }
}

// ============================================================================
// Tauri command wrappers
// ============================================================================
async function refreshBackends(): Promise<void> {
  try {
    detected.value = await invoke<GpuVendorInfo>('detect_local_gpu_vendor');
  }
  catch (error) {
    detected.value = null;
    note.value.__global__ = `Could not detect GPU vendor: ${error}`;
  }
  finally {
    detectedLoading.value = false;
  }
  try {
    const arr = await invoke<BackendRuntimeStatus[]>('get_backend_status', {
      backendKind: null,
    });
    const next: Partial<Record<string, BackendRuntimeStatus>> = {};
    for (const entry of arr) {
      next[entry.kind] = entry;
    }
    statuses.value = next;
  }
  catch (error) {
    statuses.value = {};
  }
}

async function downloadBackend(entry: BackendEntry): Promise<void> {
  busy.value[entry.id] = true;
  downloadProgress.value[entry.id] = 0;
  const variant = getActiveVariant(entry);
  note.value[entry.id] = `Downloading ${variant.label} (${formatBytes(variant.sizeBytes)})...`;
  try {
    // Read the user-configured GitHub PAT from the user-settings store. The
    // Rust side (backend_manager::download_backend / resolve_github_release_url)
    // lets `Option<String>` resolve `null` AND an empty/whitespace string to
    // None, so passing `null` here is the explicit no-token signal — anonymous
    // resolution is taken on the Rust side. When the user has configured a
    // token in Settings → Meridian → Backend Manager, the same anonymous call
    // retries with a bearer Authorization header on HTTP 403 (Fix D).
    const rawToken = userSettingsStore.userSettings.meridian?.githubToken;
    const githubToken = typeof rawToken === 'string' && rawToken.trim().length > 0
      ? rawToken.trim()
      : null;
    const installDir = await invoke<string>('download_backend', {
      backendKind: entry.id,
      variantId: variant.id,
      targetDir: null,
      githubToken,
    });
    downloadProgress.value[entry.id] = 100;
    note.value[entry.id] = `Installed → ${installDir}`;
    await refreshBackends();
  }
  catch (error) {
    note.value[entry.id] = `Download failed: ${error}`;
    if (typeof error === 'string' && error.includes('No download entry')) {
      note.value[entry.id] +=
        ' — Try clicking a different variant (top of card) to override the auto-detected GPU.';
    }
  }
  finally {
    busy.value[entry.id] = false;
    delete downloadProgress.value[entry.id];
  }
}

async function startStopBackend(entry: BackendEntry, opts?: { modelPathOverride?: string }): Promise<void> {
  const status = statuses.value[entry.id];
  busy.value[entry.id] = true;
  try {
    if (status?.status === 'running' && typeof status.pid === 'number') {
      await invoke('stop_backend', { pid: status.pid });
      note.value[entry.id] = 'Stopped';
    }
    else {
      const port = getPort(entry.id);
      const modelPath = opts?.modelPathOverride?.trim() || getModelPath(entry.id) || null;
      const pid = await invoke<number>('start_backend', {
        backendKind: entry.id,
        modelPath,
        extraArgs: null,
        port,
      });
      const modelNote = modelPath ? `with ${modelPath.split(/[\\/]/).pop()}` : '(no model loaded)';
      note.value[entry.id] = `Started pid=${pid} on port ${port} ${modelNote}`;
    }
    await refreshBackends();
  }
  catch (error) {
    note.value[entry.id] = `Start/Stop failed: ${error}`;
  }
  finally {
    busy.value[entry.id] = false;
  }
}

async function loadModel(entry: BackendEntry, modelPath: string) {
  if (!modelPath) {
    note.value[entry.id] = 'Pick a model from the Models tab first.';
    return;
  }
  setConfig(entry.id, { modelPath });
  // Loading a model means (re)starting the backend with --model <path>.
  await startStopBackend(entry, { modelPathOverride: modelPath });
}

async function probeBackend(entry: BackendEntry) {
  busy.value[entry.id] = true;
  note.value[entry.id] = `Probing http://localhost:${getPort(entry.id)}/health ...`;
  try {
    const result = await invoke<BackendApiStatus>('probe_backend_api', {
      backendKind: entry.id,
    });
    apiProbes.value[entry.id] = result;
    if (result.ok) {
      note.value[entry.id] = `API OK — ${result.urlTested} (HTTP ${result.httpStatus}, ${result.elapsedMs} ms)`;
    }
    else {
      note.value[entry.id] = `API not responding: ${result.error ?? 'no 2xx response'} (probed ${result.urlTested})`;
    }
    // Persist last-check timestamp so the UI can show a "checked at HH:MM" hint.
    setConfig(entry.id, {
      lastApiCheckAt: Date.now(),
      lastApiCheckOk: result.ok,
    });
  }
  catch (error) {
    note.value[entry.id] = `Probe failed: ${error}`;
  }
  finally {
    busy.value[entry.id] = false;
  }
}

async function refreshModels(): Promise<void> {
  modelsBusy.value = true;
  modelsNote.value = '';
  try {
    if (!modelsDir.value) {
      modelsNote.value = 'Models folder is not configured. Set it in Settings → Meridian → Files.';
      models.value = [];
      return;
    }
    // `scan_models_recursive` walks the entire tree under modelsDir without
    // any depth cap. The previous `list_gguf_models(maxDepth=6)` walker
    // missed files on hosts with deeply-nested layouts
    // (e.g. `E:\ai\Models\<vendor>\<size1>\<size2>\<file>.gguf`). Calling
    // the recursive command means "No .gguf files found" is a genuine
    // empty-result signal, not a depth-cap artefact. See
    // `backend_manager::scan_models_recursive` for the walker details.
    const entries = await invoke<RawModelEntry[]>('scan_models_recursive', {
      path: modelsDir.value,
    });
    models.value = entries
      .map((entry) => ({
        filename: entry.name,
        path: entry.path,
        sizeBytes: entry.sizeBytes,
        quant: parseQuant(entry.name),
        modifiedAt: entry.modifiedAt,
      }))
      .sort((a, b) => b.sizeBytes - a.sizeBytes);
    if (models.value.length === 0) {
      modelsNote.value = `No .gguf files found under ${modelsDir.value} (recursive walk). Try a shallower or different model root — e.g. E:\\ai\\Models\\.`;
    }
  }
  catch (error) {
    modelsNote.value = `Could not scan ${modelsDir.value}: ${error}`;
    models.value = [];
  }
  finally {
    modelsBusy.value = false;
  }
}

async function launchSlave(slave: SlaveRow): Promise<void> {
  busy.value[slave.name] = true;
  note.value[slave.name] = 'Launching RPC slave...';
  try {
    const out = await invoke<string>('launch_rpc_slave', {
      creds: {
        host: slave.host,
        port: slave.port,
        username: slave.username,
        keyPath: slave.keyPath,
      },
      rpcCommand: 'llama-server --rpc 0.0.0.0:50052',
    });
    note.value[slave.name] = out || 'RPC slave launch sent.';
  }
  catch (error) {
    note.value[slave.name] = `Launch failed: ${error}`;
  }
  finally {
    busy.value[slave.name] = false;
  }
}

function loadModelInto(entry: BackendEntry) {
  activeTab.value = 'models';
  modelsNote.value = `Pick a .gguf file below, then click "Load into ${entry.name}".`;
}

async function loadIntoBackend(entry: BackendEntry, model: ModelRow) {
  activeTab.value = 'backends';
  await loadModel(entry, model.path);
}

// Resolve a backend by MeridianBackendKind with a fallback to the catalog's
// first entry. Strict indexing (`Partial<Record>` + `noUncheckedIndexedAccess`)
// makes `catalog.backends[0]` `BackendEntry | undefined`, so we use a
// non-null assertion here — the catalog ships at least three entries and
// any reachable code path has the JS object in memory already.
function pickBackend(id: MeridianBackendKind | undefined): BackendEntry {
  const target: MeridianBackendKind = id ?? 'llama.cpp';
  return catalog.backends.find((b) => b.id === target) ?? catalog.backends[0]!;
}

// Per-kind theme key — drives the per-per-backend accent gradient, the LED
// color, the variant-chip selected state, and the kind-label tint. Mirrors
// cluster.vue's `exoTheme(nodeName)` shape. Each MeridianBackendKind maps to
// one of five palette entries defined in the .bm-runtime__* CSS tokens.
type RuntimeTheme = 'llama' | 'lemonade' | 'kobold' | 'llamafile' | 'turboquant';

function themeKeyFor(kind: MeridianBackendKind): RuntimeTheme {
  switch (kind) {
    case 'llama.cpp':   return 'llama';
    case 'lemonade':    return 'lemonade';
    case 'koboldcpp':   return 'kobold';
    case 'llamafile':   return 'llamafile';
    case 'turboquant':  return 'turboquant';
  }
}

// Short identifier rendered inside the CSS-drawn tile (kept under 5 chars
// so the monospace glyphs don't wrap inside the 56×80px tile). The dots in
// `llama.cpp` would visually overflow, so we use a stylistic abbreviation.
function themeInitials(kind: MeridianBackendKind): string {
  switch (kind) {
    case 'llama.cpp':   return 'll.cpp';
    case 'lemonade':    return 'LMND';
    case 'koboldcpp':   return 'KCpp';
    case 'llamafile':   return 'LF';
    case 'turboquant':  return 'TQ';
  }
}

watch(detected, (val) => {
  if (val) {
    note.value.__global__ = `Detected GPU: ${val.vendor} (${val.gpuName ?? 'unknown'}, source=${val.source}). Override the variant per backend below if needed.`;
  }
});

onMounted(async () => {
  unlistenProgress = await listen<BackendDownloadProgressPayload>(
    'backend-download-progress',
    (event) => {
      downloadProgress.value[event.payload.kind] = event.payload.percent;
    },
  );
  void refreshBackends();
  void refreshModels();
  void refreshOmnix();
});

onUnmounted(() => {
  unlistenProgress?.();
});
</script>

<template>
  <div class="bm">
    <header class="bm__header">
      <h1 class="bm__title">Backend Manager</h1>
      <div class="bm__detected">
        Detected GPU:
        <strong v-if="detectedLoading">detecting…</strong>
        <strong v-else-if="detected">{{ detected.vendor }}</strong>
        <strong v-else>unknown</strong>
        <span v-if="detected?.gpuName"> · {{ detected.gpuName }}</span>
      </div>
    </header>

    <p v-if="note.__global__" class="bm__global-note">{{ note.__global__ }}</p>

    <div v-if="tierRecommendation" class="bm__tier-banner" role="note">
      <span class="bm__tier-badge">{{ tierRecommendation.tier }}</span>
      <strong class="bm__tier-copy">{{ tierRecommendation.copy }}</strong>
      <span class="bm__tier-detail">{{ tierRecommendation.detail }}</span>
    </div>

    <nav class="bm__tabs" role="tablist">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        role="tab"
        :aria-selected="activeTab === tab.id"
        :class="['bm__tab', { 'bm__tab--active': activeTab === tab.id }]"
        @click="activeTab = tab.id"
      >
        {{ tab.label }}
        <span v-if="tab.id === 'models' && models.length" class="bm__tab-count">
          ({{ models.length }})
        </span>
        <span v-else-if="tab.id === 'slaves' && slaves.length" class="bm__tab-count">
          ({{ slaves.length }})
        </span>
        <span v-else-if="tab.id === 'omnix-models' && omnixInstalledInCatalogSet.size" class="bm__tab-count">
          ({{ omnixInstalledInCatalogSet.size }}/{{ omnixCatalog.length }})
        </span>
        <span v-else-if="tab.id === 'lemonade-models' && lemonadeModels.length" class="bm__tab-count">
          ({{ lemonadeModels.length }})
        </span>
      </button>
    </nav>

    <!-- ============================ Backends tab — exo-style cards ========== -->
    <!-- Mirrors cluster.vue's row layout: per-kind CSS-drawn tile LEFT, -->
    <!-- identity column (kind + name + homepage + status pill), fluid         -->
    <!-- specs column (description + variant chips + port/model + meta),       -->
    <!-- actions column (Download/Start/Stop/Test API). Per-kind accent      -->
    <!-- gradient + ambient grid backdrop give the same visual language as     -->
    <!-- the Cluster Control panel.                                            -->
    <section v-show="activeTab === 'backends'" class="bm__section" role="tabpanel">
      <div class="bm-runtime__ambient-grid" aria-hidden="true" />

      <article
        v-for="entry in catalog.backends"
        :key="entry.id"
        class="bm-runtime"
        :class="[
          `bm-runtime--${themeKeyFor(entry.id)}`,
          {
            'bm-runtime--running': statuses[entry.id]?.status === 'running',
            'bm-runtime--installed': statuses[entry.id]?.status === 'installed',
          },
        ]"
      >
        <!-- Per-kind CSS-drawn tile. Top/bottom color bands are per-kind; -->
        <!-- body shows initials + 3 thin lines + status LED.                -->
        <div class="bm-runtime__tile" aria-hidden="true">
          <div class="bm-runtime__band" />
          <div class="bm-runtime__body">
            <span class="bm-runtime__initials">{{ themeInitials(entry.id) }}</span>
            <div class="bm-runtime__lines">
              <span /><span /><span />
            </div>
            <div
              class="bm-runtime__led"
              :class="{
                'bm-runtime__led--installed': statuses[entry.id]?.status === 'installed',
                'bm-runtime__led--running':  statuses[entry.id]?.status === 'running',
              }"
            />
          </div>
          <div class="bm-runtime__band bm-runtime__band--bottom" />
        </div>

        <!-- Identity column: kind tag + name + homepage + status pill -->
        <div class="bm-runtime__identity">
          <span class="bm-runtime__kind">{{ entry.id }}</span>
          <span class="bm-runtime__name">{{ entry.name }}</span>
          <span class="bm-runtime__homepage">{{ entry.homepage }}</span>

          <div class="bm-runtime__status-row">
            <span
              :class="[
                'bm-runtime__status',
                `bm-runtime__status--${statuses[entry.id]?.status ?? 'notInstalled'}`,
              ]"
            >
              <span class="bm-runtime__status-dot" />
              <span class="bm-runtime__status-text">{{ statuses[entry.id]?.status ?? 'notInstalled' }}</span>
            </span>
            <span
              v-if="apiProbes[entry.id]"
              :class="[
                'bm-runtime__api-badge',
                apiProbes[entry.id]?.ok ? 'bm-runtime__api-badge--ok' : 'bm-runtime__api-badge--bad',
              ]"
            >
              API · {{ apiProbes[entry.id]?.ok ? `${apiProbes[entry.id]?.elapsedMs}ms` : 'down' }}
            </span>
          </div>
        </div>

        <!-- Specs column: description + variant chips + port/model + meta -->
        <div class="bm-runtime__specs">
          <p class="bm-runtime__desc">{{ entry.description }}</p>

          <div class="bm-runtime__variants">
            <span class="bm-runtime__variants-label">Builds</span>
            <button
              v-for="variant in entry.variants"
              :key="variant.id"
              type="button"
              class="bm-runtime__variant-chip"
              :class="{ 'bm-runtime__variant-chip--selected': getActiveVariant(entry).id === variant.id }"
              :disabled="busy[entry.id]"
              :title="variant.notes"
              @click="selectVariant(entry, variant.id)"
            >
              <span class="bm-runtime__variant-hw">{{ variant.hardware.toUpperCase() }}</span>
              <span class="bm-runtime__variant-label">{{ variant.label }}</span>
              <span class="bm-runtime__variant-size">{{ formatBytes(variant.sizeBytes) }}</span>
            </button>
          </div>

          <div class="bm-runtime__config">
            <label class="bm-runtime__config-row">
              <span class="bm-runtime__config-label">Port</span>
              <input
                type="number"
                min="1"
                max="65535"
                class="bm__input bm-runtime__input--port"
                :value="getPort(entry.id)"
                :disabled="busy[entry.id]"
                @change="setConfig(entry.id, { port: Number(($event.target as HTMLInputElement).value) || DEFAULT_PORTS[entry.id] })"
              />
            </label>
            <label class="bm-runtime__config-row bm-runtime__config-row--model">
              <span class="bm-runtime__config-label">Model</span>
              <div class="bm-runtime__model-row">
                <input
                  type="text"
                  class="bm__input"
                  placeholder="Path to .gguf or pick from Models tab"
                  :value="getModelPath(entry.id)"
                  :disabled="busy[entry.id] || statuses[entry.id]?.status === 'running'"
                  @change="setConfig(entry.id, { modelPath: ($event.target as HTMLInputElement).value })"
                />
                <button
                  type="button"
                  class="bm__btn bm__btn--ghost bm-runtime__model-pick"
                  :disabled="busy[entry.id] || statuses[entry.id]?.status === 'running'"
                  @click="loadModelInto(entry)"
                >
                  Pick…
                </button>
              </div>
            </label>
          </div>

          <div class="bm-runtime__meta">
            <span v-if="statuses[entry.id]?.port" class="bm-runtime__meta-chip">
              <span class="bm-runtime__meta-label">port</span>
              <code class="bm-runtime__meta-value">http://localhost:{{ statuses[entry.id]?.port }}/v1</code>
            </span>
            <span v-if="statuses[entry.id]?.installPath" class="bm-runtime__meta-chip">
              <span class="bm-runtime__meta-label">path</span>
              <code class="bm-runtime__meta-value">{{ statuses[entry.id]?.installPath }}</code>
            </span>
            <span v-if="statuses[entry.id]?.pid" class="bm-runtime__meta-chip">
              <span class="bm-runtime__meta-label">pid</span>
              <span class="bm-runtime__meta-value">{{ statuses[entry.id]?.pid }}</span>
            </span>
            <span v-if="apiProbes[entry.id]?.urlTested" class="bm-runtime__meta-chip">
              <span class="bm-runtime__meta-label">probe</span>
              <code class="bm-runtime__meta-value">{{ apiProbes[entry.id]?.urlTested }}</code>
            </span>
          </div>
        </div>

        <!-- Actions column: Download (with progress) / Start or Stop / Test API -->
        <div class="bm-runtime__actions">
          <button
            class="bm__btn bm__btn--download bm-runtime__download"
            :disabled="
              busy[entry.id]
                || statuses[entry.id]?.status === 'running'
                || statuses[entry.id]?.status === 'installed'
            "
            @click="downloadBackend(entry)"
          >
            <template v-if="downloadProgress[entry.id] !== undefined">
              <span class="bm__dl-progress">
                <span class="bm__dl-bar" :style="{ width: Math.round(downloadProgress[entry.id]) + '%' }" />
                <span class="bm__dl-text">{{ Math.round(downloadProgress[entry.id]) }}%</span>
              </span>
            </template>
            <template v-else>
              {{ statuses[entry.id]?.status === 'notInstalled' ? 'Download' : 'Re-Download' }}
            </template>
          </button>
          <button
            v-if="statuses[entry.id]?.status === 'running'"
            class="bm__btn bm__btn--danger"
            :disabled="busy[entry.id]"
            @click="startStopBackend(entry)"
          >
            Stop
          </button>
          <button
            v-else
            class="bm__btn bm__btn--primary"
            :disabled="
              busy[entry.id]
                || statuses[entry.id]?.status !== 'installed'
            "
            @click="startStopBackend(entry)"
          >
            Start
          </button>
          <button
            class="bm__btn"
            :disabled="busy[entry.id] || statuses[entry.id]?.status !== 'running'"
            @click="probeBackend(entry)"
          >
            Test API
          </button>
          <span v-if="note[entry.id]" class="bm-runtime__note">{{ note[entry.id] }}</span>
        </div>
      </article>
    </section>

    <!-- ============================ Models tab ============================== -->
    <section v-show="activeTab === 'models'" class="bm__section" role="tabpanel">
      <header class="bm__section-head">
        <div>
          <h2 class="bm__section-title">Local models</h2>
          <p class="bm__section-sub">{{ modelsDir || '(not configured)' }}</p>
        </div>
        <div class="bm__section-head-actions">
          <button
            class="bm__btn"
            :disabled="modelsBusy"
            @click="refreshModels"
          >
            {{ modelsBusy ? 'Scanning...' : 'Refresh' }}
          </button>
          <!-- Deep-link into the Hardware Scanner so users have ONE canonical
               place to search HuggingFace with real filters + download. -->
          <button
            class="bm__btn bm__btn--primary"
            title="Open the HuggingFace GGUF search panel"
            @click="openHuggingFaceSearch"
          >
            Search HuggingFace
          </button>
        </div>
      </header>

      <div class="bm__models-hf-hint" role="note">
        <span>Need a specific model from HuggingFace?</span>
        <button
          class="bm__link-btn"
          @click="openHuggingFaceSearch"
        >
          Open the search panel →
        </button>
      </div>

      <p v-if="modelsNote" class="bm__note">{{ modelsNote }}</p>

      <div v-if="models.length" class="bm__models-filter" role="search">
        <input
          v-model="modelQuery"
          type="search"
          class="bm__input"
          placeholder="Filter — filename, quant (Q4_K_M), or size (7 = GB, 700 = MB, '8gb' explicit)"
          aria-label="Filter local models"
          :disabled="modelsBusy"
        >
        <span class="bm__models-count">
          {{ filteredModels.length }}
          <template v-if="modelQuery.trim()">/ {{ models.length }}</template>
          shown
        </span>
        <button
          v-if="modelQuery.trim()"
          class="bm__btn bm__btn--ghost"
          type="button"
          @click="modelQuery = ''"
        >Clear</button>
      </div>

      <p v-if="models.length && filteredModels.length === 0" class="bm__note bm__note--empty">
        No model matched "{{ modelQuery }}". Try a shorter substring or clear the filter.
      </p>

      <ul v-if="filteredModels.length" class="bm__models">
        <li v-for="model in models" :key="model.path" class="bm__model">
          <div class="bm__model-info">
            <div class="bm__model-name">{{ model.filename }}</div>
            <div class="bm__model-meta">{{ formatBytes2(model.sizeBytes) }} · quant: {{ model.quant }}</div>
          </div>
          <div class="bm__model-actions">
            <select
              class="bm__select"
              :disabled="busy[model.path]"
              :value="loadTargetFor[model.path] ?? 'llama.cpp'"
              @change="loadTargetFor[model.path] = ($event.target as HTMLSelectElement).value as MeridianBackendKind"
            >
              <option value="llama.cpp">Load into llama.cpp</option>
              <option value="llamafile">Load into llamafile</option>
              <option value="koboldcpp">Load into koboldcpp</option>
              <option value="turboquant">Load into TurboQuant</option>
            </select>
            <button
              class="bm__btn bm__btn--primary"
              :disabled="busy[model.path]"
              @click="loadIntoBackend(pickBackend(loadTargetFor[model.path]), model)"
            >
              Load into selected
            </button>
          </div>
        </li>
      </ul>
    </section>

    <!-- ============================ RPC Slaves tab ========================== -->
    <section v-show="activeTab === 'slaves'" class="bm__section" role="tabpanel">
      <p v-if="!slaves.length" class="bm__note bm__note--empty">
        No SSH connections configured.
        Add one in <strong>Settings → Meridian → SSH</strong>.
      </p>

      <article v-for="slave in slaves" :key="slave.name" class="bm__slave">
        <header class="bm__slave-head">
          <span class="bm__dot bm__dot--off" />
          <div>
            <h2 class="bm__slave-name">{{ slave.name }}</h2>
            <p class="bm__slave-role">{{ slave.role }}</p>
          </div>
          <span class="bm__slave-host">{{ slave.username }}@{{ slave.host }}:{{ slave.port }}</span>
        </header>

        <footer class="bm__backend-footer">
          <button
            class="bm__btn bm__btn--primary"
            :disabled="busy[slave.name]"
            @click="launchSlave(slave)"
          >
            {{ busy[slave.name] ? 'Launching...' : 'Launch RPC Slave' }}
          </button>
          <span v-if="note[slave.name]" class="bm__note">{{ note[slave.name] }}</span>
        </footer>
      </article>
    </section>

    <!-- ============================ Omnix Models tab ======================= -->
    <section v-show="activeTab === 'omnix-models'" class="bm__section" role="tabpanel">
      <header class="bm__section-head">
        <div>
          <h2 class="bm__section-title">Omnix Models</h2>
          <p class="bm__section-sub">
            {{ omnixInstalledInCatalogSet.size }} of {{ omnixCatalog.length }} installed ·
            Tier 1 — zero-config, runs on any GPU<span v-if="omnixOtherCacheCount"> · +{{ omnixOtherCacheCount }} other in cache</span>
          </p>
        </div>
        <button class="bm__btn" :disabled="omnixBusy" @click="refreshOmnix">
          {{ omnixBusy ? 'Scanning…' : 'Refresh' }}
        </button>
      </header>

      <p v-if="omnixNote" class="bm__note">{{ omnixNote }}</p>

      <ul class="bm__models">
        <li v-for="entry in omnixCatalog" :key="entry.id" class="bm__model">
          <div class="bm__model-info">
            <div class="bm__model-name-row">
              <span class="bm__model-name">{{ entry.name }}</span>
              <span class="bm__badge bm__badge--tier">Omnix</span>
              <span :class="['bm__badge', 'bm__badge--cat', `bm__badge--cat-${entry.category}`]">{{ entry.category }}</span>
              <span v-if="entry.verifiedWorking" class="bm__badge bm__badge--verified" title="Verified working in our test setup">✓ tested</span>
              <span v-if="omnixInstalledSet.has(entry.modelID)" class="bm__badge bm__badge--installed">installed</span>
              <span :class="['bm__badge', omnixRamGate(entry) === 'fits' ? 'bm__badge--compat' : 'bm__badge--heavy']">
                {{ omnixRamGate(entry) === 'fits' ? 'tier-1 ready' : 'needs 8GB+ RAM' }}
              </span>
            </div>
            <div class="bm__model-meta">
              {{ entry.size ?? '~unknown' }} · min RAM {{ entry.minRam ?? 0 }} GB · {{ entry.make ?? 'Community' }}
            </div>
            <div class="bm__model-desc">{{ entry.description }}</div>
          </div>
          <div class="bm__model-actions">
            <a
              class="bm__btn"
              :href="`https://huggingface.co/${entry.modelID}`"
              target="_blank"
              rel="noreferrer noopener"
              :title="`Open ${entry.modelID} on HuggingFace`"
            >
              HF page
            </a>
            <button
              class="bm__btn bm__btn--primary"
              :disabled="omnixBusy || omnixInstalledSet.has(entry.modelID)"
              @click="downloadOmnixModel(entry)"
            >
              {{ omnixInstalledSet.has(entry.modelID) ? 'Installed' : 'Get on HF' }}
            </button>
          </div>
        </li>
      </ul>
    </section>

    <!-- ============================ Lemonade Models tab ====================== -->
    <section v-show="activeTab === 'lemonade-models'" class="bm__section" role="tabpanel">
      <header class="bm__section-head">
        <div>
          <h2 class="bm__section-title">Lemonade Models</h2>
          <p class="bm__section-sub">
            Live catalog from <code>localhost:11434</code> ({{ lemonadeModels.length }} entries)
          </p>
        </div>
        <div class="bm__section-head-actions">
          <button class="bm__btn" :disabled="lemonadeBusy" @click="refreshLemonade">
            {{ lemonadeBusy ? 'Scanning…' : 'Refresh' }}
          </button>
        </div>
      </header>
      <p v-if="lemonadeError" class="bm__note">{{ lemonadeError }}</p>
      <p v-if="lemonadeNote" class="bm__note bm__note--empty">{{ lemonadeNote }}</p>
      <ul v-if="lemonadeModels.length" class="bm__models">
        <li v-for="model in lemonadeModels" :key="model.id" class="bm__model">
          <div class="bm__model-info">
            <div class="bm__model-name-row">
              <span class="bm__model-name">{{ model.id }}</span>
              <span v-if="model.loaded" class="bm__badge bm__badge--installed">loaded</span>
              <span v-else-if="model.downloaded" class="bm__badge bm__badge--verified">downloaded</span>
              <span v-else class="bm__badge">available</span>
              <span v-if="model.recipe" class="bm__badge bm__badge--cat">{{ model.recipe }}</span>
            </div>
            <div class="bm__model-meta">
              {{ formatBytes2(model.sizeBytes) }}<template v-if="model.labels.length"> · {{ model.labels.join(' · ') }}</template>
            </div>
          </div>
          <div class="bm__model-actions">
            <button v-if="model.downloaded && !model.loaded" class="bm__btn" :disabled="lemonadeBusy" @click="lemonadeAction('load', model.id)">Load</button>
            <button v-if="model.loaded" class="bm__btn bm__btn--danger" :disabled="lemonadeBusy" @click="lemonadeAction('unload', model.id)">Unload</button>
            <button v-if="!model.downloaded" class="bm__btn bm__btn--primary" :disabled="lemonadeBusy" @click="lemonadeAction('pull', model.id)">Pull</button>
            <button v-if="model.downloaded" class="bm__btn" :disabled="lemonadeBusy" @click="lemonadeDelete(model)">Delete</button>
          </div>
        </li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
.bm {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 1.5rem;
  /* `flex: 1` (replacing the previous `height: 100%`) lets .bm claim
     every pixel of the router-view-wrapper without bleeding past it.
     `min-height: 0` is the canonical companion to `flex: 1` in a
     column flex parent — without it, .bm refuses to shrink below its
     intrinsic content size and the page overflows the viewport. */
  flex: 1;
  min-height: 0;
  /* No scroll on .bm itself. The page is the viewport clip; the active
     .bm__section is the only scroll region. */
  overflow: hidden;
  color: hsl(var(--foreground));
}

.bm__header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}

.bm__title {
  font-size: 1.25rem;
  font-weight: 600;
  margin: 0;
}

.bm__detected {
  font-size: 0.85rem;
  color: hsl(var(--muted-foreground));
}

.bm__global-note {
  margin: 0;
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  background: hsl(var(--background-2));
  padding: 0.5rem 0.7rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
}

/* ---- Hardware-tier recommendation banner ---- */
.bm__tier-banner {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.6rem;
  padding: 0.65rem 0.85rem;
  border-radius: var(--radius-sm);
  background: linear-gradient(
    90deg,
    hsl(var(--primary) / 12%) 0%,
    hsl(var(--background-3)) 100%
  );
  border: 1px solid hsl(var(--primary) / 30%);
  font-size: 0.85rem;
  color: hsl(var(--foreground));
}

.bm__tier-badge {
  display: inline-flex;
  align-items: center;
  padding: 0.15rem 0.55rem;
  border-radius: 999px;
  background: hsl(var(--primary));
  color: hsl(0 0% 100%);
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  white-space: nowrap;
}

.bm__tier-copy {
  font-weight: 600;
}

.bm__tier-detail {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  flex: 1 1 240px;
  min-width: 0;
}

/* Fix 3 scroll rules live on the original .bm__section and .bm__models
   declarations above to keep CSS discoverable. Do not redeclare here. */

.bm__tabs {
  display: flex;
  gap: 0.25rem;
  padding: 0.25rem;
  background: hsl(var(--background-2));
  border-radius: var(--radius-sm);
  width: fit-content;
}

.bm__tab {
  padding: 0.4rem 0.9rem;
  border-radius: var(--radius-sm);
  border: 1px solid transparent;
  background: transparent;
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  font-size: 0.85rem;
}

.bm__tab:hover {
  color: hsl(var(--foreground));
}

.bm__tab--active {
  border-color: hsl(var(--border));
  background: hsl(var(--background-3));
  color: hsl(var(--foreground));
}

.bm__tab-count {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  margin-left: 0.15rem;
}

.bm__section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  /* The ONLY scroll container for the active tab.
     `flex: 1; min-height: 0` claims exactly the leftover vertical space
     inside .bm (the page wrapper). No `max-height` cap here — a previous
     `max-height: calc(100vh - 220px)` was the actual bug: `100vh`
     measures the full WebView2 viewport (ignoring the window title bar,
     app toolbar, .bm padding, header, tier banner, and tabs), so the
     calculated cap was usually LARGER than the space .bm actually
     offered. .bm's `overflow: hidden` then clipped the section's
     overflow before `overflow-y: auto` could engage, leaving users
     staring at the top of Lemonade with no way to reach its settings.
     Drop the cap; let flex do the sizing. */
  flex: 1;
  min-height: 0;
  /* `max-height` caps each tab section so the active tab scrolls when
     its content overflows, even when the upstream sizing chain is
     broken (router-view-wrapper → sidebar → .bm collapsing mid-render
     is the historical bug pattern). The 100vh / 100dvh cascade is the
     standard "modern wins last-decl" pattern — mobile webviews: dvh
     actually constrains; Tauri desktop: both units evaluate
     identically so the cascade is effectively a no-op there (safety
     net for window resize).
     220 cap is empirically set between the two banner extremes so the
     section scrolls WITHOUT over-clipping in either state:
       - No banners render (3 children of .bm: header, tabs, section):
         chrome above ≈ window-toolbar 32 + padding-top 24 + header 40
         + gap 16 + tabs 40 + gap 16 = 168 → available = 100vh - 168.
         At 100vh=1080, available=912. Cap=860. Cap is 52px TIGHTER
         than available → section caps at 860 and scrolls on overflow.
       - Both tier + note render (5 children): chrome ≈ 168 + tier ~40
         + note ~28 + 2 extra gaps 32 = 268 → available = 100vh - 268.
         At 100vh=1080, available=812. Cap=860. Cap is 48px LOOSER
         than available → section fills available parent space; cap is
         a no-op and the scrollbar appears ONLY because of parent
         overflow:hidden (which is the bug pattern this CSS prevents). */
  max-height: calc(100vh - var(--window-toolbar-height, 48px) - 220px);
  max-height: calc(100dvh - var(--window-toolbar-height, 48px) - 220px);
  overflow-y: auto;
  scrollbar-gutter: stable;
}

.bm__backend,
.bm__slave {
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  padding: 1rem;
  background: hsl(var(--background-2));
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}

.bm__backend-head,
.bm__slave-head {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.bm__backend-name,
.bm__slave-name {
  font-size: 1rem;
  font-weight: 600;
  margin: 0;
}

.bm__variant-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.bm__variant-btn:hover:not(:disabled) {
  background: hsl(var(--foreground) / 4%);
}

.bm__variant--selected .bm__variant-radio {
  border-color: hsl(var(--primary));
}

.bm__variant--selected .bm__variant-radio::after {
  content: '';
  position: absolute;
  inset: 2.5px;
  border-radius: 50%;
  background: hsl(var(--primary));
}

.bm__input {
  width: 100%;
  padding: 0.35rem 0.5rem;
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  font-family: var(--font-mono, monospace);
  font-size: 0.8rem;
  outline: none;
}

.bm__input:focus {
  border-color: hsl(var(--ring));
}

.bm__input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.bm__models-filter {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  padding: 0.55rem 0.75rem;
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
}

.bm__models-filter > .bm__input {
  flex: 1;
  min-width: 0;
}

.bm__models-count {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.bm__model-row {
  display: flex;
  gap: 0.4rem;
  align-items: center;
}

.bm__select {
  padding: 0.35rem 0.5rem;
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  font-size: 0.8rem;
  outline: none;
}

.bm__backend-footer {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  flex-wrap: wrap;
}

.bm__btn {
  padding: 0.4rem 0.85rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-3));
  color: hsl(var(--foreground));
  font-size: 0.8rem;
  cursor: pointer;
  font-family: inherit;
}

.bm__btn:hover:not(:disabled) {
  background: hsl(var(--foreground) / 5%);
}

.bm__btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.bm__btn--primary {
  border-color: hsl(var(--primary));
  background: hsl(var(--primary) / 10%);
}

.bm__btn--primary:hover:not(:disabled) {
  background: hsl(var(--primary) / 20%);
}

.bm__btn--ghost {
  background: transparent;
}

.bm__btn--download {
  position: relative;
  overflow: hidden;
  min-width: 160px;
}

.bm__dl-progress {
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 1.4em;
}

.bm__dl-bar {
  position: absolute;
  inset: -4px -8px;
  background: hsl(var(--primary) / 20%);
  border-radius: var(--radius-sm);
  transition: width 0.3s ease;
  z-index: 0;
}

.bm__dl-text {
  position: relative;
  z-index: 1;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.bm__btn--danger {
  border-color: hsl(0 70% 55%);
  background: hsl(0 70% 60% / 10%);
}

.bm__btn--danger:hover:not(:disabled) {
  background: hsl(0 70% 60% / 20%);
}

/* ========================================================================== */
/* Backends tab — exo-style runtime cards (mirrors cluster.vue exo-node)    */
/*                                                                              */
/* Per-kind accent palette (llama.cpp teal / lemonade coral / koboldcpp      */
/* violet / llamafile amber / turboquant magenta) drives the tile bands,     */
/* status pill, name gradient, variant-chip selected state, and meta-chip    */
/* label colors. Animation classes for running LED pulse + amber-band         */
/* glow. Ambient dot-grid backdrop mirrors .cluster__ambient-grid.            */
/* ========================================================================== */
.bm__section {
  position: relative;
}

.bm-runtime__ambient-grid {
  position: absolute;
  inset: 0;
  background-image: radial-gradient(circle at 2px 2px, hsl(var(--primary) / 18%) 1.2px, transparent 1.2px);
  background-size: 24px 24px;
  opacity: 0.6;
  pointer-events: none;
  z-index: 0;
  border-radius: var(--radius-md);
}

/* Per-kind accent palette — five backends get five distinct gradient/border
   glow values. The kind column (label + LED + bands + meta-label + ripple
   glow) all read from the same tokens so a re-theming is one CSS edit away.
   Same shape as cluster.vue's --cluster-node-* tokens for visual parity.    */
.bm-runtime {
  --bm-runtime-llama-accent:      174 80% 45%;
  --bm-runtime-llama-bg-from:     hsl(174 80% 45% / 0.20);
  --bm-runtime-llama-bg-to:       hsl(174 80% 45% / 0.04);
  --bm-runtime-llama-border:      hsl(174 80% 45% / 0.55);
  --bm-runtime-llama-glow:        hsl(174 80% 45% / 0.40);

  --bm-runtime-lemonade-accent:   348 83% 58%;
  --bm-runtime-lemonade-bg-from:  hsl(348 83% 58% / 0.20);
  --bm-runtime-lemonade-bg-to:    hsl(348 83% 58% / 0.04);
  --bm-runtime-lemonade-border:   hsl(348 83% 58% / 0.55);
  --bm-runtime-lemonade-glow:     hsl(348 83% 58% / 0.40);

  --bm-runtime-kobold-accent:     280 80% 65%;
  --bm-runtime-kobold-bg-from:    hsl(280 80% 65% / 0.20);
  --bm-runtime-kobold-bg-to:      hsl(280 80% 65% / 0.04);
  --bm-runtime-kobold-border:     hsl(280 80% 65% / 0.55);
  --bm-runtime-kobold-glow:       hsl(280 80% 65% / 0.40);

  --bm-runtime-llamafile-accent:   40 90% 55%;
  --bm-runtime-llamafile-bg-from:  hsl(40 90% 55% / 0.20);
  --bm-runtime-llamafile-bg-to:    hsl(40 90% 55% / 0.04);
  --bm-runtime-llamafile-border:   hsl(40 90% 55% / 0.55);
  --bm-runtime-llamafile-glow:     hsl(40 90% 55% / 0.40);

  --bm-runtime-turboquant-accent:  318 80% 65%;
  --bm-runtime-turboquant-bg-from: hsl(318 80% 65% / 0.20);
  --bm-runtime-turboquant-bg-to:   hsl(318 80% 65% / 0.04);
  --bm-runtime-turboquant-border:  hsl(318 80% 65% / 0.55);
  --bm-runtime-turboquant-glow:    hsl(318 80% 65% / 0.40);
}

.bm-runtime {
  position: relative;
  z-index: 1;
  display: grid;
  grid-template-columns: 64px minmax(160px, 200px) 1fr minmax(180px, 200px);
  align-items: center;
  gap: 1.1rem;
  padding: 1rem 1.15rem;
  margin-bottom: 0.75rem;
  border: 1px solid var(--rt-border, hsl(var(--border)));
  border-radius: var(--radius-md);
  background:
    linear-gradient(135deg, var(--rt-bg-from, hsl(var(--background-3))) 0%, var(--rt-bg-to, transparent) 100%),
    hsl(var(--background-2));
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.22);
  transition: box-shadow 0.2s ease, transform 0.2s ease, border-color 0.2s ease;
  min-height: 124px;
}
.bm-runtime:hover {
  transform: translateY(-1px);
  box-shadow: 0 8px 22px rgba(0, 0, 0, 0.28), 0 0 0 1px var(--rt-glow, hsl(var(--primary) / 22%));
}
.bm-runtime--llama {
  --rt-accent: var(--bm-runtime-llama-accent);
  --rt-bg-from: var(--bm-runtime-llama-bg-from);
  --rt-bg-to: var(--bm-runtime-llama-bg-to);
  --rt-border: var(--bm-runtime-llama-border);
  --rt-glow: var(--bm-runtime-llama-glow);
}
.bm-runtime--lemonade {
  --rt-accent: var(--bm-runtime-lemonade-accent);
  --rt-bg-from: var(--bm-runtime-lemonade-bg-from);
  --rt-bg-to: var(--bm-runtime-lemonade-bg-to);
  --rt-border: var(--bm-runtime-lemonade-border);
  --rt-glow: var(--bm-runtime-lemonade-glow);
}
.bm-runtime--kobold {
  --rt-accent: var(--bm-runtime-kobold-accent);
  --rt-bg-from: var(--bm-runtime-kobold-bg-from);
  --rt-bg-to: var(--bm-runtime-kobold-bg-to);
  --rt-border: var(--bm-runtime-kobold-border);
  --rt-glow: var(--bm-runtime-kobold-glow);
}
.bm-runtime--llamafile {
  --rt-accent: var(--bm-runtime-llamafile-accent);
  --rt-bg-from: var(--bm-runtime-llamafile-bg-from);
  --rt-bg-to: var(--bm-runtime-llamafile-bg-to);
  --rt-border: var(--bm-runtime-llamafile-border);
  --rt-glow: var(--bm-runtime-llamafile-glow);
}
.bm-runtime--turboquant {
  --rt-accent: var(--bm-runtime-turboquant-accent);
  --rt-bg-from: var(--bm-runtime-turboquant-bg-from);
  --rt-bg-to: var(--bm-runtime-turboquant-bg-to);
  --rt-border: var(--bm-runtime-turboquant-border);
  --rt-glow: var(--bm-runtime-turboquant-glow);
}
.bm-runtime--running {
  border-color: hsl(var(--success) / 55%);
}

/* ===== CSS-drawn tile (mirrors .exo-node__tile in cluster.vue) ===== */
.bm-runtime__tile {
  position: relative;
  width: 56px;
  height: 84px;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.4));
}
.bm-runtime__band {
  height: 6px;
  background: linear-gradient(180deg, hsl(var(--rt-accent)) 0%, hsl(var(--rt-accent) / 80%) 100%);
  box-shadow: 0 0 6px hsl(var(--rt-accent) / 65%);
}
.bm-runtime__band--bottom {
  background: linear-gradient(0deg, hsl(var(--rt-accent)) 0%, hsl(var(--rt-accent) / 80%) 100%);
}
.bm-runtime__body {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 4px;
  background:
    linear-gradient(180deg, hsl(var(--rt-accent) / 22%) 0%, hsl(var(--background-3)) 60%, rgba(0,0,0,0.4) 100%);
  border-left: 1px solid hsl(var(--rt-accent) / 35%);
  border-right: 1px solid hsl(var(--rt-accent) / 35%);
  position: relative;
  overflow: hidden;
}
.bm-runtime__body::before {
  content: '';
  position: absolute;
  inset: 0;
  background: radial-gradient(ellipse at 50% 0%, hsl(var(--rt-accent) / 28%) 0%, transparent 70%);
  pointer-events: none;
}
.bm-runtime__initials {
  position: relative;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  font-weight: 700;
  font-size: 0.72rem;
  color: hsl(var(--rt-accent));
  text-shadow: 0 0 4px hsl(var(--rt-accent) / 50%);
  letter-spacing: 0.02em;
  text-align: center;
  z-index: 1;
}
.bm-runtime__lines {
  position: relative;
  width: 75%;
  display: flex;
  flex-direction: column;
  gap: 2px;
  z-index: 1;
}
.bm-runtime__lines span {
  height: 2px;
  background: rgba(0,0,0,0.45);
  border-radius: 1px;
}
.bm-runtime__led {
  position: relative;
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #6b7280;
  transition: background 0.2s ease, box-shadow 0.2s ease;
  z-index: 1;
}
.bm-runtime__led--installed {
  background: hsl(var(--primary));
  box-shadow: 0 0 5px hsl(var(--primary) / 80%);
}
.bm-runtime__led--running {
  background: hsl(var(--success));
  box-shadow: 0 0 7px hsl(var(--success) / 90%);
  animation: bm-runtime-led-pulse 1.4s ease-in-out infinite;
}
@keyframes bm-runtime-led-pulse {
  0%, 100% { opacity: 1; filter: brightness(1); }
  50%      { opacity: 0.65; filter: brightness(0.85); }
}

/* ===== Identity column ===== */
.bm-runtime__identity {
  display: flex;
  flex-direction: column;
  gap: 0.18rem;
  min-width: 0;
}
.bm-runtime__kind {
  font-size: 0.62rem;
  font-weight: 700;
  letter-spacing: 1.2px;
  text-transform: uppercase;
  color: hsl(var(--rt-accent));
  font-family: var(--font-mono, 'Consolas', monospace);
}
.bm-runtime__name {
  font-size: 1.2rem;
  font-weight: 800;
  font-family: var(--font-mono, 'Consolas', monospace);
  background: linear-gradient(120deg, hsl(var(--foreground)) 0%, hsl(var(--rt-accent)) 100%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  line-height: 1.15;
}
.bm-runtime__homepage {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
  word-break: break-all;
}
.bm-runtime__status-row {
  margin-top: 0.35rem;
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
}

/* ===== Status pill (animated when running) ===== */
.bm-runtime__status {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.25rem 0.6rem;
  border-radius: 100px;
  font-size: 0.7rem;
  font-weight: 600;
  font-family: var(--font-mono, monospace);
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-3));
  color: hsl(var(--muted-foreground));
  text-transform: uppercase;
  letter-spacing: 0.04em;
  white-space: nowrap;
}
.bm-runtime__status--installed {
  border-color: hsl(var(--rt-accent) / 55%);
  color: hsl(var(--rt-accent));
  background: hsl(var(--rt-accent) / 10%);
}
.bm-runtime__status--running {
  border-color: hsl(var(--success) / 55%);
  color: hsl(var(--success));
  background: hsl(var(--success) / 12%);
  box-shadow: 0 0 9px hsl(var(--success) / 30%);
}
.bm-runtime__status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 5px currentColor;
}

.bm-runtime__api-badge {
  display: inline-flex;
  align-items: center;
  padding: 0.25rem 0.55rem;
  border-radius: 100px;
  font-size: 0.65rem;
  font-weight: 600;
  font-family: var(--font-mono, monospace);
  white-space: nowrap;
}
.bm-runtime__api-badge--ok {
  background: hsl(var(--success) / 12%);
  color: hsl(var(--success));
  border: 1px solid hsl(var(--success) / 40%);
}
.bm-runtime__api-badge--bad {
  background: hsl(0 70% 60% / 12%);
  color: hsl(0 70% 60%);
  border: 1px solid hsl(0 70% 60% / 40%);
}

/* ===== Specs column (description + chip variants + port/model + meta) ===== */
.bm-runtime__specs {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  min-width: 0;
}
.bm-runtime__desc {
  margin: 0;
  font-size: 0.78rem;
  color: hsl(var(--muted-foreground));
  line-height: 1.4;
}
.bm-runtime__variants {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
  align-items: center;
}
.bm-runtime__variants-label {
  font-size: 0.62rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: hsl(var(--muted-foreground));
  margin-right: 0.2rem;
}
.bm-runtime__variant-chip {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.3rem 0.55rem;
  border-radius: 999px;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-3));
  cursor: pointer;
  font-family: inherit;
  font-size: 0.72rem;
  transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease;
}
.bm-runtime__variant-chip:hover:not(:disabled) {
  border-color: hsl(var(--rt-accent) / 50%);
  background: hsl(var(--rt-accent) / 6%);
  color: hsl(var(--foreground));
}
.bm-runtime__variant-chip:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.bm-runtime__variant-chip--selected {
  background: hsl(var(--rt-accent) / 18%);
  border-color: hsl(var(--rt-accent));
  color: hsl(var(--rt-accent));
  font-weight: 600;
}
.bm-runtime__variant-hw {
  font-family: var(--font-mono, monospace);
  font-size: 0.62rem;
  font-weight: 700;
  letter-spacing: 0.5px;
  opacity: 0.85;
}
.bm-runtime__variant-label {
  font-weight: 500;
}
.bm-runtime__variant-size {
  font-variant-numeric: tabular-nums;
  font-size: 0.65rem;
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
}
.bm-runtime__variant-chip--selected .bm-runtime__variant-size {
  color: hsl(var(--rt-accent) / 80%);
}

.bm-runtime__config {
  display: flex;
  gap: 0.6rem;
  align-items: stretch;
  flex-wrap: wrap;
}
.bm-runtime__config-row {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
  flex: 1;
  min-width: 0;
}
.bm-runtime__config-row--model {
  flex: 2;
}
.bm-runtime__config-label {
  font-size: 0.62rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: hsl(var(--muted-foreground));
}
.bm-runtime__input--port {
  max-width: 110px;
}
.bm-runtime__model-row {
  display: flex;
  gap: 0.3rem;
}
.bm-runtime__model-row > .bm__input {
  flex: 1;
  min-width: 0;
}
.bm-runtime__model-pick {
  padding: 0.3rem 0.6rem;
  font-size: 0.72rem;
}

.bm-runtime__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
}
.bm-runtime__meta-chip {
  display: inline-flex;
  align-items: baseline;
  gap: 0.35rem;
  padding: 0.25rem 0.55rem;
  background: hsl(var(--background-3));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  word-break: break-word;
  max-width: 100%;
}
.bm-runtime__meta-label {
  font-weight: 700;
  color: hsl(var(--rt-accent));
  text-transform: uppercase;
  font-size: 0.6rem;
  letter-spacing: 0.05em;
}
.bm-runtime__meta-value {
  font-size: 0.7rem;
  color: hsl(var(--foreground));
}

/* ===== Actions column ===== */
.bm-runtime__actions {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  align-items: stretch;
  justify-content: center;
}
.bm-runtime__actions > .bm__btn {
  font-family: inherit;
  font-size: 0.74rem;
  padding: 0.4rem 0.7rem;
}
.bm-runtime__download {
  min-width: 0;
}
.bm-runtime__note {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
  word-break: break-word;
  padding-top: 0.2rem;
}

/* ===== Narrow viewport: stack specs/actions below identity ===== */
@media (max-width: 1024px) {
  .bm-runtime {
    grid-template-columns: 60px 1fr;
    grid-template-areas:
      'tile      identity'
      'specs     specs'
      'actions   actions';
    row-gap: 0.85rem;
  }
  .bm-runtime__tile     { grid-area: tile; }
  .bm-runtime__identity { grid-area: identity; }
  .bm-runtime__specs    { grid-area: specs; }
  .bm-runtime__actions  {
    grid-area: actions;
    flex-direction: row;
    flex-wrap: wrap;
  }
}

.bm__note {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}

.bm__note--empty {
  font-style: italic;
  padding: 0.6rem 0.8rem;
  background: hsl(var(--background-2));
  border-radius: var(--radius-sm);
  border: 1px dashed hsl(var(--border));
}

.bm__section-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
}

.bm__section-head-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.bm__models-hf-hint {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  background: hsl(var(--background-2));
  border: 1px dashed hsl(var(--border));
  border-radius: var(--radius-sm);
  font-size: 0.8rem;
  color: hsl(var(--muted-foreground));
}

.bm__link-btn {
  background: transparent;
  border: none;
  color: hsl(var(--primary));
  cursor: pointer;
  font: inherit;
  padding: 0;
  text-decoration: underline;
}

.bm__link-btn:hover {
  color: hsl(var(--primary) / 0.85);
}

.bm__section-title {
  font-size: 1rem;
  margin: 0;
}

.bm__section-sub {
  margin: 0;
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
}

.bm__models {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  list-style: none;
  padding: 0;
  margin: 0;
  /* Lists inside a tab section. Scrolling is now handled by the parent
     .bm__section (it's the only scroll container in this component);
     the list grows naturally to its content size and the section's
     overflow takes over when the total exceeds the section's height. */
}

.bm__model {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.6rem 0.9rem;
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
}

.bm__model-info {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
}

.bm__model-name {
  font-weight: 500;
  font-family: var(--font-mono, monospace);
  font-size: 0.85rem;
  word-break: break-all;
}

.bm__model-meta {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  margin-top: 0.15rem;
}

.bm__model-actions {
  display: flex;
  gap: 0.4rem;
  align-items: center;
  flex-shrink: 0;
}

.bm__dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}

.bm__dot--off {
  background: hsl(var(--muted-foreground));
}

.bm__slave-role {
  margin: 0;
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  font-style: italic;
}

.bm__slave-host {
  margin-left: auto;
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
  white-space: nowrap;
}

/* ---- Omnix Models tab ---- */
.bm__model-name-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.4rem;
}

.bm__model-desc {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  margin-top: 0.25rem;
  font-style: italic;
  line-height: 1.35;
  max-width: 60ch;
}

.bm__badge {
  display: inline-flex;
  align-items: center;
  padding: 0.1rem 0.45rem;
  border-radius: 999px;
  font-size: 0.65rem;
  font-weight: 600;
  letter-spacing: 0.3px;
  border: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  background: hsl(var(--background-3));
  white-space: nowrap;
  text-transform: uppercase;
}

.bm__badge--tier {
  border-color: hsl(var(--primary));
  color: hsl(var(--primary));
  background: hsl(var(--primary) / 10%);
}

.bm__badge--verified {
  border-color: hsl(150 60% 50% / 50%);
  color: hsl(150 60% 60%);
  background: hsl(150 60% 50% / 12%);
}

.bm__badge--installed {
  border-color: hsl(210 80% 60% / 50%);
  color: hsl(210 80% 70%);
  background: hsl(210 80% 60% / 12%);
}

.bm__badge--compat {
  border-color: hsl(140 50% 50% / 40%);
  color: hsl(140 50% 60%);
  background: hsl(140 50% 50% / 10%);
}

.bm__badge--heavy {
  border-color: hsl(40 90% 55% / 40%);
  color: hsl(40 90% 60%);
  background: hsl(40 90% 50% / 10%);
}

.bm__badge--cat {
  font-family: var(--font-mono, monospace);
  text-transform: lowercase;
}

.bm__badge--cat-text { color: hsl(220 70% 70%); border-color: hsl(220 70% 60% / 40%); }
.bm__badge--cat-vision { color: hsl(280 70% 75%); border-color: hsl(280 70% 60% / 40%); }
.bm__badge--cat-tts { color: hsl(330 70% 75%); border-color: hsl(330 70% 60% / 40%); }
.bm__badge--cat-stt { color: hsl(180 60% 65%); border-color: hsl(180 60% 50% / 40%); }
.bm__badge--cat-image-gen { color: hsl(20 80% 70%); border-color: hsl(20 80% 60% / 40%); }
.bm__badge--cat-music-gen { color: hsl(50 80% 70%); border-color: hsl(50 80% 60% / 40%); }
.bm__badge--cat-coder { color: hsl(120 60% 65%); border-color: hsl(120 60% 50% / 40%); }
.bm__badge--cat-embedding { color: hsl(60 60% 65%); border-color: hsl(60 60% 50% / 40%); }
.bm__badge--cat-director { color: hsl(0 60% 70%); border-color: hsl(0 60% 50% / 40%); }
</style>
