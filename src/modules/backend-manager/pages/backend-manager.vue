<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
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

const DEFAULT_PORTS: Record<MeridianBackendKind, number> = {
  'llama.cpp': 8080,
  'koboldcpp': 5001,
  'llamafile': 8080,
  'turboquant': 8080,
  'lemonade': 13305,
};

// ============================================================================
// Tab state
// ============================================================================
type TabId = 'backends' | 'models' | 'slaves' | 'omnix-models';

const tabs: { id: TabId; label: string }[] = [
  { id: 'backends', label: 'Backends' },
  { id: 'models', label: 'Models' },
  { id: 'slaves', label: 'RPC Slaves' },
  { id: 'omnix-models', label: 'Omnix Models' },
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
    const installDir = await invoke<string>('download_backend', {
      backendKind: entry.id,
      variantId: variant.id,
      targetDir: null,
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
      </button>
    </nav>

    <!-- ============================ Backends tab ============================ -->
    <section v-show="activeTab === 'backends'" class="bm__section" role="tabpanel">
      <article
        v-for="entry in catalog.backends"
        :key="entry.id"
        class="bm__backend"
      >
        <header class="bm__backend-head">
          <div>
            <h2 class="bm__backend-name">{{ entry.name }}</h2>
            <span class="bm__backend-homepage">{{ entry.homepage }}</span>
          </div>
          <div class="bm__status-row">
            <span v-if="apiProbes[entry.id]" :class="['bm__api-badge', apiProbes[entry.id]?.ok ? 'bm__api-badge--ok' : 'bm__api-badge--bad']">
              {{ apiProbes[entry.id]?.ok ? `API live · ${apiProbes[entry.id]?.elapsedMs}ms` : 'API down' }}
            </span>
            <span :class="['bm__status', `bm__status--${statuses[entry.id]?.status ?? 'notInstalled'}`]">
              {{ statuses[entry.id]?.status ?? 'notInstalled' }}
            </span>
          </div>
        </header>

        <p class="bm__backend-desc">{{ entry.description }}</p>

        <div class="bm__variants-label">Choose a runtime:</div>
        <ul class="bm__variants">
          <li
            v-for="variant in entry.variants"
            :key="variant.id"
            :class="[
              'bm__variant',
              {
                'bm__variant--selected': getActiveVariant(entry).id === variant.id,
              },
            ]"
          >
            <button
              type="button"
              class="bm__variant-btn"
              :disabled="busy[entry.id]"
              :title="variant.notes"
              @click="selectVariant(entry, variant.id)"
            >
              <span class="bm__variant-radio" />
              <span class="bm__variant-label">{{ variant.label }}</span>
              <span class="bm__variant-hw">{{ variant.hardware.toUpperCase() }}</span>
              <span class="bm__variant-size">{{ formatBytes(variant.sizeBytes) }}</span>
            </button>
          </li>
        </ul>

        <div class="bm__config">
          <label class="bm__config-row">
            <span class="bm__config-label">Port</span>
            <input
              type="number"
              min="1"
              max="65535"
              class="bm__input bm__input--port"
              :value="getPort(entry.id)"
              :disabled="busy[entry.id]"
              @change="setConfig(entry.id, { port: Number(($event.target as HTMLInputElement).value) || DEFAULT_PORTS[entry.id] })"
            />
          </label>
          <label class="bm__config-row">
            <span class="bm__config-label">Model</span>
            <div class="bm__model-row">
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
                class="bm__btn bm__btn--ghost"
                :disabled="busy[entry.id] || statuses[entry.id]?.status === 'running'"
                @click="loadModelInto(entry)"
              >
                Pick…
              </button>
            </div>
          </label>
        </div>

        <div class="bm__backend-meta">
          <span v-if="statuses[entry.id]?.port">
            <span class="bm__meta-label">Listening on:</span>
            <code class="bm__meta-value">http://localhost:{{ statuses[entry.id]?.port }}/v1</code>
          </span>
          <span v-if="statuses[entry.id]?.installPath">
            <span class="bm__meta-label">Installed at:</span>
            <code class="bm__meta-value">{{ statuses[entry.id]?.installPath }}</code>
          </span>
          <span v-if="statuses[entry.id]?.pid">
            <span class="bm__meta-label">PID:</span>
            <span>{{ statuses[entry.id]?.pid }}</span>
          </span>
          <span v-if="apiProbes[entry.id]?.urlTested">
            <span class="bm__meta-label">Last probe:</span>
            <code class="bm__meta-value">{{ apiProbes[entry.id]?.urlTested }}</code>
          </span>
        </div>

        <footer class="bm__backend-footer">
          <button
            class="bm__btn bm__btn--download"
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
              {{ statuses[entry.id]?.status === 'notInstalled' ? 'Download selected runtime' : 'Re-Download selected runtime' }}
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
          <span v-if="note[entry.id]" class="bm__note">{{ note[entry.id] }}</span>
        </footer>
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

.bm__backend-homepage {
  display: block;
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
}

.bm__backend-desc {
  margin: 0;
  font-size: 0.85rem;
  color: hsl(var(--muted-foreground));
  line-height: 1.4;
}

.bm__status-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-left: auto;
}

.bm__status {
  padding: 0.15rem 0.6rem;
  border-radius: 999px;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-3));
  font-size: 0.7rem;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: hsl(var(--muted-foreground));
  white-space: nowrap;
}

.bm__status--installed {
  border-color: hsl(var(--primary) / 50%);
  color: hsl(var(--primary));
}

.bm__status--running {
  border-color: hsl(150 60% 50% / 50%);
  color: hsl(150 60% 55%);
  background: hsl(150 60% 50% / 8%);
}

.bm__api-badge {
  padding: 0.15rem 0.5rem;
  border-radius: 999px;
  font-size: 0.7rem;
  font-weight: 600;
  white-space: nowrap;
}

.bm__api-badge--ok {
  background: hsl(150 60% 50% / 15%);
  color: hsl(150 60% 55%);
  border: 1px solid hsl(150 60% 50% / 40%);
}

.bm__api-badge--bad {
  background: hsl(0 70% 60% / 15%);
  color: hsl(0 70% 60%);
  border: 1px solid hsl(0 70% 60% / 40%);
}

.bm__variants-label {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  text-transform: uppercase;
  letter-spacing: 0.04em;
  font-weight: 600;
}

.bm__variants {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  list-style: none;
  margin: 0;
  padding: 0;
}

.bm__variant {
  border-radius: var(--radius-sm);
  background: hsl(var(--background-3));
  border: 1px solid transparent;
}

.bm__variant--selected {
  border-color: hsl(var(--primary));
}

.bm__variant-btn {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  width: 100%;
  padding: 0.5rem 0.75rem;
  background: transparent;
  border: 0;
  cursor: pointer;
  text-align: left;
  font-family: inherit;
  font-size: 0.85rem;
  color: inherit;
}

.bm__variant-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.bm__variant-btn:hover:not(:disabled) {
  background: hsl(var(--foreground) / 4%);
}

.bm__variant-radio {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  border: 1.5px solid hsl(var(--muted-foreground));
  flex-shrink: 0;
  position: relative;
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

.bm__variant-label {
  font-weight: 500;
  flex: 1;
}

.bm__variant-hw {
  font-family: var(--font-mono, monospace);
  font-size: 0.65rem;
  letter-spacing: 0.5px;
  opacity: 0.7;
}

.bm__variant-size {
  font-variant-numeric: tabular-nums;
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}

.bm__config {
  display: grid;
  grid-template-columns: 140px 1fr;
  gap: 0.5rem 0.75rem;
  align-items: center;
  background: hsl(var(--background-3));
  border-radius: var(--radius-sm);
  padding: 0.6rem 0.75rem;
  border: 1px solid hsl(var(--border));
}

.bm__config-row {
  display: contents;
}

.bm__config-label {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  text-transform: uppercase;
  letter-spacing: 0.04em;
  font-weight: 600;
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

.bm__input--port {
  max-width: 110px;
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

.bm__backend-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}

.bm__meta-label {
  font-weight: 600;
  margin-right: 0.25rem;
}

.bm__meta-value {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
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
