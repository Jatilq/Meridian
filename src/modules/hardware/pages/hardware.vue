<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
/**
 * Hardware Scanner — HuggingFace GGUF model search + browse.
 *
 * Phase 11 LM-Studio parity (2026-06-30):
 *   * Search-all: single-letter queries (`b`, `g`) now substring-match
 *     the HF catalog instead of prefix-matching a tiny slice. The
 *     previous wildcard mode (`*` at 1-4 chars) was removed in
 *     hardware.rs — HF's fuzzy matcher is already loose and the literal
 *     star over-narrowed.
 *   * Machine-selector dropdown: VRAM target per search (Local / each
 *     worker / Combined). Drives both the top-level FITS badge and
 *     every per-sibling row in the expanded LM-Studio-style table.
 *   * Expandable card details: chevron toggles a per-quant breakdown
 *     (filename / quant / size / FITS) and a real-fetch button for
 *     `max_position_embeddings` from `config.json` (heuristic first;
 *     truth on demand — the latter goes through a new Tauri command
 *     `hardware_fetch_model_detail`).
 *   * HF model-card link: the `model.id` text is now a real
 *     `<a href>` to `https://huggingface.co/{id}` with an external-link
 *     icon, target=_blank.
 *
 * Defaults (carried over from prior phases):
 *   - query: empty (browse mode)
 *   - sort: downloads desc (Trending) — auto-fired on mount
 *   - selectedQuants: empty Set ("all quants")
 *   - onlyTrustedQuantizers: OFF
 *   - onlyFit: OFF (the user opts in)
 *   - includeIq: OFF
 *   - targetMachine: 'combined' (sum of all GPUs across local + workers)
 */
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useRoute } from 'vue-router';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { useHardwarePool } from '@/composables/use-hardware-pool';

const userSettingsStore = useUserSettingsStore();
const modelsFolder = computed(() => userSettingsStore.userSettings.meridian?.modelsFolder ?? '');

const route = useRoute();
const incomingQuery = computed(() => {
  const raw = route.query.searchHuggingface;
  return typeof raw === 'string' && raw.trim() ? raw.trim() : '';
});

// 10% safety buffer — must stay in lockstep with
// src-tauri/src/hardware.rs::VRAM_FIT_SAFETY_RATIO.
const VRAM_FIT_SAFETY_RATIO = 0.90;

// ============================================================================
// IPC types (must mirror src-tauri/src/hardware.rs)
// ============================================================================

// contextLengthSource contract: the Rust backend ALWAYS writes one of
// `"estimate"` (heuristic matched, value > 0) or `"none"` (heuristic
// returned None, value null). The Vue side promotes `"none"` to
// `"config_json"` after `hardware_fetch_model_detail` resolves with a
// real `max_position_embeddings`. A future maintainer who reads the
// type union and looks for a wire-format `"config_json"` path: there
// isn't one — it's a post-fetch affordance only.
interface HardwareSearchParams {
  query: string | null;
  sortBy?: string;
  limit?: number;
  architectures: string[];
  sizeBuckets: string[];
  quantAllowlist: string[];
  trustedQuantizers: string[];
  includeIq?: boolean;
  onlyFit?: boolean;
  combinedVramMb: number;
}

interface RankedGgufSibling {
  filename: string;
  quant: string;
  sizeBytes: number;
  sizeGb: number;
  fitsHardware: boolean;
  score: number;
}

interface RankedGgufModel {
  id: string;
  author: string;
  name: string;
  downloads: number;
  likes: number;
  lastModified?: string;
  primaryQuant: string;
  sizeBytes: number;
  sizeGb: number;
  fitsHardware: boolean;
  isTrustedQuantizer: boolean;
  quantizerLabel: string;
  architecture: string;
  paramCountLabel: string;
  ggufUrl: string;
  ggufFilename: string;
  tags: string[];
  kind: string;
  contextLength?: number;
  contextLengthSource?: 'estimate' | 'config_json' | 'none';
  siblings: RankedGgufSibling[];
}

interface ModelDetail {
  repoId: string;
  maxPositionEmbeddings: number | null;
  source: 'config_json' | 'none';
}

// ============================================================================
// Hardware pool
// ============================================================================

const {
  entries: hardwareNodes,
  combinedVramMb,
  combinedVramGb,
  combinedGpuCount,
} = useHardwarePool();

const localNode = computed(() => hardwareNodes.value.find((n) => n.isLocal) ?? null);
const localCpuName = computed(() => localNode.value?.cpu?.name?.trim() || 'Unknown CPU');

// ============================================================================
// Machine-selector dropdown
//
// Each option targets one node's GPU VRAM; 'combined' sums across all
// nodes. `targetVramMb` is the reactive truth for every fit check in
// this component — both the top-level card badge AND every sibling
// row in the expanded table bind to it via `fitsForSizeGb`. No HF
// round-trip fires on dropdown change (Phase 11 LM-Studio parity:
// switching target re-renders locally because sibling sizes are
// pre-baked).
// ============================================================================

// String discriminator for the machine-selector v-model. The previous
// tagged-union shape (`'combined' | 'local' | { kind: 'worker'; host }`)
// silently deselected after any poll cycle because Vue's built-in `<select>`
// v-model uses `===` to compare the bound ref against the option `:value`,
// and the parent `computed` re-created fresh `{ kind, host }` objects on
// every recompute. A string discriminator survives reference churn and
// stays stable across renders. Worker entries stringify to `worker:<host>`.
type MachineTargetId = 'combined' | 'local' | `worker:${string}`;

const workerOptionId = (host: string): MachineTargetId => `worker:${host}`;

const selectedMachineId = ref<MachineTargetId>('combined');
const workerOptions = computed(() => hardwareNodes.value.filter((n) => !n.isLocal && n.online));

interface MachineOption {
  id: MachineTargetId;
  label: string;
  vramMb: number;
}

const machineOptions = computed<MachineOption[]>(() => {
  const opts: MachineOption[] = [
    { id: 'combined', label: 'Combined (all GPUs)', vramMb: combinedVramMb.value },
  ];
  opts.push({
    id: 'local',
    label: 'Local',
    vramMb: (localNode.value?.gpus ?? []).reduce((s, g) => s + (g.memoryTotal || 0), 0),
  });
  for (const w of workerOptions.value) {
    const vr = w.gpus.reduce((s, g) => s + (g.memoryTotal || 0), 0);
    if (vr <= 0) continue;
    opts.push({ id: workerOptionId(w.host), label: `${w.name} (${w.host})`, vramMb: vr });
  }
  return opts;
});

const targetVramMb = computed(() => {
  const id = selectedMachineId.value;
  if (id === 'combined') return combinedVramMb.value;
  if (id === 'local') {
    return (localNode.value?.gpus ?? []).reduce((s, g) => s + (g.memoryTotal || 0), 0);
  }
  // `worker:<host>` branch — strip the prefix, look up the pool node.
  const WORKER_PREFIX = 'worker:';
  if (id.startsWith(WORKER_PREFIX)) {
    const host = id.slice(WORKER_PREFIX.length);
    const node = hardwareNodes.value.find((n) => !n.isLocal && n.host === host);
    if (!node) return 0;
    return node.gpus.reduce((s, g) => s + (g.memoryTotal || 0), 0);
  }
  return 0;
});

const targetVramGb = computed(() => targetVramMb.value > 0 ? Math.floor(targetVramMb.value / 1024) : 0);

function fitsForSizeGb(sizeGb: number, targetMb: number): boolean {
  if (targetMb === 0) return true; // No hardware data — show all.
  const targetGb = targetMb / 1024;
  return sizeGb <= targetGb * VRAM_FIT_SAFETY_RATIO;
}

// ============================================================================
// Search filters — reactive, bound to sidebar controls
// ============================================================================

// Empty query = browse mode. (No more `llama` placeholder default.)
const query = ref<string>('');
const sortBy = ref<'downloads' | 'lastModified' | 'likes'>('downloads');

const architectureOptions = [
  { key: 'llama', label: 'Llama' },
  { key: 'qwen', label: 'Qwen' },
  { key: 'mistral', label: 'Mistral' },
  { key: 'gemma', label: 'Gemma' },
  { key: 'phi', label: 'Phi' },
  { key: 'deepseek', label: 'DeepSeek' },
] as const;
const selectedArchitectures = ref<Set<string>>(new Set());

const sizeOptions = ['1-3B', '4-8B', '9-15B', '16-30B', '30-60B', '60B+'] as const;
const selectedSizes = ref<Set<string>>(new Set());

const quantOptions = ['Q4_K_M', 'Q5_K_M', 'Q6_K', 'Q8_0'] as const;
// Empty set = "include all quants". Same as Phase 11 default.
const selectedQuants = ref<Set<string>>(new Set());

const trustedQuantizerOptions = [
  { key: 'bartowski', label: 'Bartowski' },
  { key: 'unsloth', label: 'Unsloth' },
  { key: 'maziyarpanahi', label: 'MaziyarPanahi' },
  { key: 'lonestriker', label: 'LoneStriker' },
  { key: 'mradermacher', label: 'mradermacher' },
] as const;
const selectedTrustedQuantizers = ref<Set<string>>(new Set(trustedQuantizerOptions.map((o) => o.key)));
const onlyTrustedQuantizers = ref<boolean>(false);

const onlyFit = ref<boolean>(false);
const includeIq = ref<boolean>(false);

// ============================================================================
// Result list state
// ============================================================================

const models = ref<RankedGgufModel[]>([]);
const loadingModels = ref(false);
const searchError = ref<string>('');
const lastSearchKind = ref<string>('');
// Sequence counter — every searchModels() call increments. Stale
// resolutions whose captured seq != current are dropped. The same
// pattern protects `onUnmounted` mid-flight resolutions (see blow).
const searchSeq = ref<number>(0);

const visibleCount = ref<number>(30);
const PAGE_STEP = 30;
const hasMoreModels = computed(() => visibleCount.value < models.value.length);

// ============================================================================
// Card-expand state — Phase 11 LM-Studio parity
//
// `expandedCards` holds repo ids currently expanded. `modelDetails`
// holds the per-repo ModelDetail payload fetched on demand (real
// `max_position_embeddings` from `config.json`). The user can
// expand multiple cards at once for LM-Studio-style cross-comparison.
// ============================================================================

const expandedCards = ref<Set<string>>(new Set());
const modelDetails = ref<Map<string, ModelDetail>>(new Map());
const loadingDetail = ref<Set<string>>(new Set());

function isExpanded(modelId: string): boolean {
  return expandedCards.value.has(modelId);
}

function toggleExpand(model: RankedGgufModel) {
  if (expandedCards.value.has(model.id)) {
    expandedCards.value.delete(model.id);
  } else {
    expandedCards.value.add(model.id);
    // Lazy-fetch real context on first expand IF the heuristic was
    // unable to classify. (User explicitly asked for "real fetch
    // only on detail-view expand" — so when the heuristic figured it
    // out, we honour the estimate and don't fire a config.json call.)
    if (
      model.contextLength == null ||
      model.contextLengthSource !== 'estimate'
    ) {
      if (!modelDetails.value.has(model.id) && !loadingDetail.value.has(model.id)) {
        void fetchContextLength(model.id);
      }
    }
  }
}

async function fetchContextLength(repoId: string) {
  // Snapshot the search-generation at fetch start. A new search calls
  // `searchModels()` which bumps `searchSeq.value` and replaces
  // `models.value`. If our fetch resolves AFTER that replacement,
  // `models.value.find(m => m.id === repoId)` might find a *different*
  // row carrying the same id (filter narrowed, repo dropped, etc.) and
  // overwrite the new row's context with the stale-fetch result. Drop
  // the mutation when the apply-time seq != our snapshot. Same
  // invariant the search command itself uses.
  const fetchSeq = searchSeq.value;
  loadingDetail.value.add(repoId);
  try {
    const detail = await invoke<ModelDetail>('hardware_fetch_model_detail', { repoId });
    if (fetchSeq !== searchSeq.value) return;
    modelDetails.value.set(repoId, detail);
    const target = models.value.find((m) => m.id === repoId);
    if (target) {
      target.contextLength = detail.maxPositionEmbeddings ?? undefined;
      target.contextLengthSource = detail.source;
    }
    // Clear any stale error notice for this row now that the apply
    // succeeded. Without this, a user who clicks Retry after a
    // transient failure sees the success path land AND the error
    // message still showing — two contradictory affordances in the
    // same expanded row.
    clearContextFetchNotice(repoId);
  } catch (err) {
    if (fetchSeq !== searchSeq.value) return; // Stale-resolved: drop on the new fetch's behalf.
    // Keep the heuristic source as-is (likely 'none' / 'estimate') so
    // the badge doesn't pretend we know more than we do. Surface the
    // error inline in the expanded row keyed by `repoId` — a 401 on
    // one card shouldn't shout "search failed" at the whole panel.
    const msg = `Context fetch failed: ${err instanceof Error ? err.message : String(err)}`;
    setContextFetchNotice(repoId, msg);
  } finally {
    loadingDetail.value.delete(repoId);
  }
}

// Scoped per-repo notices for context-fetch errors. Each repo gets
// its own slot (Map<repoId, message>) so a transient 401 on one card
// doesn't blow away other cards' state. Replaces the previous global
// `contextFetchNotice` ref which conflated source-failure with
// destination-missing. Cleared on every fresh searchSeq bump so stale
// errors don't linger on rows that already re-fetched successfully.
type NoticeMap = Record<string, string>;
const contextFetchNoticeByRepo = ref<NoticeMap>({});
function setContextFetchNotice(repoId: string, msg: string) {
  contextFetchNoticeByRepo.value = { ...contextFetchNoticeByRepo.value, [repoId]: msg };
}
function clearContextFetchNotice(repoId: string) {
  if (!(repoId in contextFetchNoticeByRepo.value)) return;
  const next = { ...contextFetchNoticeByRepo.value };
  delete next[repoId];
  contextFetchNoticeByRepo.value = next;
}

// ============================================================================
// Helpers
// ============================================================================

const browseSubtitle = computed(() => {
  switch (sortBy.value) {
    case 'lastModified': return 'recently updated';
    case 'likes': return 'most-liked';
    default: return 'trending';
  }
});

// Watch sortBy: when the search box is empty (true browse mode),
// changing the sort radio auto-refires the search.
watch(sortBy, () => {
  if (!loadingModels.value && query.value.trim() === '') {
    void searchModels();
  }
});

// Reset transient context-fetch notices AND prune the per-repo detail
// cache on every fresh search so a past failure (or a no-longer-in-view
// repo) doesn't linger indefinitely. Without the prune, users who
// expand dozens of cards across many searches would balloon the Map;
// across 100 searches × 50 cards expanded that's ~5000 entries /
// several MB of cached config.json payloads sitting in memory even
// though Vue's reactive Map.set triggers a new identity each time.
// `modelDetails.value.clear()` is cheaper than filtering by id and the
// UX intent is clear: every search is a new exploration; old detail
// fetches don't carry forward.
watch(searchSeq, () => {
  contextFetchNoticeByRepo.value = {};
  modelDetails.value.clear();
});

function toggleSetMember(set: Set<string>, key: string) {
  if (set.has(key)) {
    set.delete(key);
  } else {
    set.add(key);
  }
}

function isQuantIq(quant: string): boolean {
  return /IQ[1-3]/i.test(quant);
}

function formatContextLength(tokens: number | null | undefined): string {
  if (tokens == null) return '—';
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(tokens >= 10_000_000 ? 0 : 1)}M`;
  if (tokens >= 1024) return `${Math.round(tokens / 1024)}k`;
  return `${tokens}`;
}

async function searchModels() {
  const q = query.value.trim();
  const mySeq = ++searchSeq.value;
  loadingModels.value = true;
  searchError.value = '';
  try {
    const params: HardwareSearchParams = {
      query: q === '' ? null : q,
      sortBy: sortBy.value,
      limit: 100,
      architectures: Array.from(selectedArchitectures.value),
      sizeBuckets: Array.from(selectedSizes.value),
      quantAllowlist: Array.from(selectedQuants.value),
      trustedQuantizers: onlyTrustedQuantizers.value
        ? Array.from(selectedTrustedQuantizers.value)
        : [],
      includeIq: includeIq.value,
      onlyFit: onlyFit.value,
      combinedVramMb: combinedVramMb.value,
    };
    const result = await invoke<RankedGgufModel[]>('hardware_search_gguf_models', { params });
    if (mySeq !== searchSeq.value) return;
    models.value = result;
    visibleCount.value = PAGE_STEP;
    if (q === '') {
      lastSearchKind.value = 'browse';
    } else {
      lastSearchKind.value = result[0]?.kind ?? 'exact';
    }
    if (result.length === 0) {
      searchError.value = `No GGUF models matched the current filters. Try clearing a chip or broadening the search.`;
    }
  } catch (error) {
    if (mySeq !== searchSeq.value) return;
    const message = error instanceof Error ? error.message : String(error);
    searchError.value = message;
    models.value = [];
    lastSearchKind.value = '';
  } finally {
    if (mySeq === searchSeq.value) loadingModels.value = false;
  }
}

async function downloadModel(model: RankedGgufModel, sibling?: RankedGgufSibling) {
  // When the user clicks a sibling row's Download button, the
  // `sibling` arg is set and we download the specific quant; the
  // top-level button falls back to the best sibling (`model.gguf*`).
  const url = sibling
    ? `https://huggingface.co/${model.id}/resolve/main/${sibling.filename}`
    : model.ggufUrl;
  const fileName = sibling ? sibling.filename : model.ggufFilename;
  try {
    await invoke('downloader_enqueue', {
      url,
      file_name: fileName,
      format_id: null,
      auto_save_folder: modelsFolder.value,
      chunk_count: null,
    });
  } catch (error) {
    searchError.value = `Enqueue failed: ${error instanceof Error ? error.message : String(error)}`;
  }
}

function clearFilters() {
  selectedArchitectures.value = new Set();
  selectedSizes.value = new Set();
  selectedQuants.value = new Set();
  selectedTrustedQuantizers.value = new Set(trustedQuantizerOptions.map((o) => o.key));
  onlyTrustedQuantizers.value = false;
  onlyFit.value = false;
  includeIq.value = false;
  models.value = [];
  visibleCount.value = PAGE_STEP;
  searchError.value = '';
  lastSearchKind.value = '';
  expandedCards.value = new Set();
  selectedMachineId.value = 'combined';
}

function loadMoreModels() {
  if (!hasMoreModels.value) return;
  visibleCount.value = Math.min(visibleCount.value + PAGE_STEP, models.value.length);
}

function quantColorClass(quant: string): string {
  if (quant.includes('Q4')) return 'quant--q4';
  if (quant.includes('Q5')) return 'quant--q5';
  if (quant.includes('Q6')) return 'quant--q6';
  if (quant.includes('Q8')) return 'quant--q8';
  if (quant.includes('F16') || quant.includes('BF16')) return 'quant--f16';
  if (quant.includes('F32')) return 'quant--f32';
  return 'quant--unknown';
}

function quantLabel(quant: string): string {
  return quant.replace(/_/g, '-');
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return `${n}`;
}

function relativeTime(iso: string | undefined): string {
  if (!iso) return '';
  const ms = Date.now() - new Date(iso).getTime();
  if (Number.isNaN(ms)) return '';
  const days = Math.floor(ms / (1000 * 60 * 60 * 24));
  if (days < 1) return 'today';
  if (days < 7) return `${days}d ago`;
  if (days < 30) return `${Math.floor(days / 7)}w ago`;
  if (days < 365) return `${Math.floor(days / 30)}mo ago`;
  return `${Math.floor(days / 365)}y ago`;
}

// Pin the safety timer in module scope so "nav away before poll
// resolves" doesn't leak a still-running setTimeout.
let deepLinkSafetyTimer: ReturnType<typeof setTimeout> | null = null;

onMounted(() => {
  void (async () => {
    if (incomingQuery.value) {
      await new Promise<void>((resolve) => {
        const stop = watch(combinedVramGb, (v) => {
          if (v <= 0) return;
          if (deepLinkSafetyTimer) {
            clearTimeout(deepLinkSafetyTimer);
            deepLinkSafetyTimer = null;
          }
          stop();
          resolve();
        });
        deepLinkSafetyTimer = setTimeout(() => {
          if (!deepLinkSafetyTimer) return;
          deepLinkSafetyTimer = null;
          stop();
          resolve();
        }, 1500);
      });
      query.value = incomingQuery.value;
      await searchModels();
      return;
    }
    await nextTick();
    if (!loadingModels.value) {
      await searchModels();
    }
  })();
});

onUnmounted(() => {
  searchSeq.value++;
  if (deepLinkSafetyTimer) {
    clearTimeout(deepLinkSafetyTimer);
    deepLinkSafetyTimer = null;
  }
});

// Per-quant filter applied client-side to the sibling list (so chip
// toggles affect both the visual top-level pick AND the expand
// table). Returns siblings whose quant token matches the chip
// selection, AND excludes IQ1/2/3 unless the toggle is on.
function visibleSiblingsFor(model: RankedGgufModel): RankedGgufSibling[] {
  return model.siblings.filter((s) => {
    if (!includeIq.value && isQuantIq(s.quant)) return false;
    if (selectedQuants.value.size > 0) {
      return selectedQuants.value.has(s.quant);
    }
    return true;
  });
}

// Whether the model.id can be linked to (HF uses these as page keys).
function hfModelUrl(repoId: string): string {
  return `https://huggingface.co/${repoId}`;
}
</script>

<template>
  <div class="hardware">
    <aside class="hardware__sidebar">
      <div class="hardware__sidebar-section hardware__sidebar-section--search">
        <div class="hardware__query-wrap">
          <input
            v-model="query"
            type="search"
            class="hardware__query"
            placeholder="Search e.g. 'llama-3.1', 'qwen2.5', 'nemotron' or leave empty to browse"
            aria-label="Search HuggingFace GGUF models"
            @keydown.enter="searchModels"
          >
          <button
            v-if="query.length > 0"
            class="hardware__query-clear"
            type="button"
            aria-label="Clear search (revert to browse mode)"
            title="Clear — keeps you in browse mode but lets you pick a different sort"
            @click="query = ''"
          >×</button>
        </div>
        <button
          class="hardware__search-btn"
          :disabled="loadingModels"
          @click="searchModels"
        >
          {{ loadingModels ? 'Searching…' : 'Search' }}
        </button>
      </div>

      <div class="hardware__sidebar-section">
        <div class="hardware__section-label">Sort by</div>
        <div class="hardware__radio-group">
          <button
            v-for="opt in (['downloads','lastModified','likes'] as const)"
            :key="opt"
            class="hardware__radio"
            :class="{ 'hardware__radio--active': sortBy === opt }"
            @click="sortBy = opt"
          >
            {{ opt === 'downloads' ? 'Trending' : opt === 'lastModified' ? 'Recent updates' : 'Most liked' }}
          </button>
        </div>
      </div>

      <div class="hardware__sidebar-section">
        <div class="hardware__section-label">
          Architecture
          <span class="hardware__section-help">No selection = all</span>
        </div>
        <div class="hardware__chip-group">
          <button
            v-for="opt in architectureOptions"
            :key="opt.key"
            class="hardware__chip"
            :class="{ 'hardware__chip--active': selectedArchitectures.has(opt.key) }"
            @click="toggleSetMember(selectedArchitectures, opt.key)"
          >{{ opt.label }}</button>
        </div>
      </div>

      <div class="hardware__sidebar-section">
        <div class="hardware__section-label">
          Parameter size
          <span class="hardware__section-help">No selection = all</span>
        </div>
        <div class="hardware__chip-group">
          <button
            v-for="sz in sizeOptions"
            :key="sz"
            class="hardware__chip hardware__chip--small"
            :class="{ 'hardware__chip--active': selectedSizes.has(sz) }"
            @click="toggleSetMember(selectedSizes, sz)"
          >{{ sz }}</button>
        </div>
      </div>

      <div class="hardware__sidebar-section">
        <div class="hardware__section-label">
          Quantization
          <span class="hardware__section-help">No selection = all quants · IQ1/2/3 hidden by default</span>
        </div>
        <div class="hardware__chip-group">
          <button
            v-for="q in quantOptions"
            :key="q"
            class="hardware__chip"
            :class="[`quant--${q.toLowerCase().replace('_','-')}`, { 'hardware__chip--active': selectedQuants.has(q) }]"
            @click="toggleSetMember(selectedQuants, q)"
          >{{ q.replace('_','-') }}</button>
          <button
            class="hardware__chip hardware__chip--toggle"
            :class="{ 'hardware__chip--active': includeIq }"
            @click="includeIq = !includeIq"
            title="IQ1/IQ2/IQ3 — much smaller files, severe quality loss. Off by default."
          >Include IQ</button>
        </div>
      </div>

      <div class="hardware__sidebar-section">
        <div class="hardware__section-label">
          Quantizer trust
          <span class="hardware__section-help">Off by default — opt in to filter to trusted authors</span>
        </div>
        <label class="hardware__toggle-row">
          <input v-model="onlyTrustedQuantizers" type="checkbox">
          <span>Only whitelist</span>
        </label>
        <div v-if="onlyTrustedQuantizers" class="hardware__chip-group">
          <button
            v-for="tq in trustedQuantizerOptions"
            :key="tq.key"
            class="hardware__chip hardware__chip--small"
            :class="{ 'hardware__chip--active': selectedTrustedQuantizers.has(tq.key) }"
            @click="toggleSetMember(selectedTrustedQuantizers, tq.key)"
          >{{ tq.label }}</button>
        </div>
      </div>

      <!-- Hardware fit target — Phase 11 LM-Studio parity. Single global
           dropdown drives every FITS/TOO BIG badge (top-level AND each
           per-quant row in the expanded table). Switching the dropdown
           is purely client-side — sibling sizes are pre-baked in the
           search response so no HF round-trip fires. -->
      <div class="hardware__sidebar-section">
        <div class="hardware__section-label">
          Fit against
          <span class="hardware__section-help">10% buffer for KV cache + overhead</span>
        </div>
        <select
          v-model="selectedMachineId"
          class="hardware__machine-select"
          aria-label="Fit-against hardware target"
        >
          <option
            v-for="opt in machineOptions"
            :key="opt.id"
            :value="opt.id"
          >
            {{ opt.label }} ({{ opt.vramMb > 0 ? `${Math.floor(opt.vramMb / 1024)} GB` : 'no GPU data' }})
          </option>
        </select>
      </div>

      <div class="hardware__sidebar-section">
        <label
          class="hardware__toggle-row"
          :class="{ 'hardware__toggle-row--off': !onlyFit }"
          title="Narrow to models whose best GGUF fits your combined VRAM with a 10% safety buffer。"
        >
          <input
            v-model="onlyFit"
            type="checkbox"
            :disabled="combinedVramGb === 0"
          >
          <span>
            Only models that fit my hardware
            <span v-if="combinedVramGb > 0" class="hardware__vram-inline">({{ combinedVramGb }} GB)</span>
          </span>
        </label>
      </div>

      <div class="hardware__sidebar-section hardware__sidebar-section--actions">
        <button class="hardware__clear-btn" @click="clearFilters">Clear filters</button>
      </div>
    </aside>

    <main class="hardware__results">
      <div class="hardware__results-header">
        <h2 class="hardware__results-title">
          <template v-if="hasMoreModels">
            Showing {{ Math.min(visibleCount, models.length) }} of {{ models.length }}
            {{ models.length === 1 ? 'model' : 'models' }}
          </template>
          <template v-else>
            {{ models.length }} {{ models.length === 1 ? 'model' : 'models' }}
          </template>
          <span v-if="targetVramGb > 0" class="hardware__results-target"> · fits against {{ targetVramGb }} GB</span>
        </h2>
        <div class="hardware__results-sub">
          Local: <strong>{{ localCpuName }}</strong>
          · {{ combinedGpuCount }} GPU{{ combinedGpuCount === 1 ? '' : 's' }}
          · <strong>{{ combinedVramGb }} GB</strong> combined VRAM
        </div>
        <div
          v-if="models.length > 0 && lastSearchKind === 'browse'"
          class="hardware__results-banner"
          aria-live="polite"
        >
          <span class="hardware__results-banner-dot" aria-hidden="true"></span>
          Showing the top <strong>{{ browseSubtitle }}</strong> GGUF models from
          HuggingFace. Type to search, or change the sort above to switch the feed.
        </div>
      </div>

      <div v-if="searchError" class="hardware__error">{{ searchError }}</div>

      <div v-if="loadingModels" class="hardware__skeleton-list" aria-hidden="true">
        <div v-for="i in 4" :key="i" class="hardware__skeleton"></div>
      </div>

      <div v-else-if="models.length > 0" class="hardware__results-list">
        <article
          v-for="model in models.slice(0, visibleCount)"
          :key="model.id"
          class="exo-card exo-card--amber hardware__result"
          :class="['hardware__result--expanded-' + isExpanded(model.id), { 'hardware__result--no-fit': !fitsForSizeGb(model.sizeGb, targetVramMb) }]"
        >
          <!-- Top row: HF tile + name + primary quant + fit + trusted +
               chevron. Click the chevron (or this top-row zone) to toggle
               the LM-Studio-style per-quant breakdown below. The tile is
               a small amber gradient block that reads as the HuggingFace
               avatar without needing a custom SVG icon (lucide doesn't
               ship a HuggingFace logo). -->
          <div class="hardware__result-tile" aria-hidden="true">
            <span class="hardware__result-tile-mark">HF</span>
          </div>
          <div class="hardware__result-info">
            <div class="hardware__result-row1">
              <span class="hardware__result-name">{{ model.name }}</span>
              <span
                class="hardware__result-quant"
                :class="quantColorClass(model.primaryQuant)"
              >{{ quantLabel(model.primaryQuant) }}</span>
              <span
                class="hardware__result-fit"
                :class="{ 'hardware__result-fit--yes': fitsForSizeGb(model.sizeGb, targetVramMb), 'hardware__result-fit--no': !fitsForSizeGb(model.sizeGb, targetVramMb) }"
              >
                {{ fitsForSizeGb(model.sizeGb, targetVramMb) ? 'FITS' : 'TOO BIG' }}
              </span>
              <span v-if="model.isTrustedQuantizer" class="hardware__result-trust">✓ trusted</span>
              <!-- Context-length badge: heuristic estimate (≈), real
                   value from config.json (✓), or unknown (—). The label
                   tells the user the provenance; the value updates on
                   expand + fetch. -->
              <span
                class="hardware__ctx-badge"
                :class="{
                  'hardware__ctx-badge--estimate': model.contextLengthSource === 'estimate',
                  'hardware__ctx-badge--real': model.contextLengthSource === 'config_json',
                  'hardware__ctx-badge--unknown': model.contextLength == null,
                }"
                :title="model.contextLengthSource === 'config_json'
                  ? 'Context window from this repo’s config.json'
                  : model.contextLengthSource === 'estimate'
                  ? 'Context window estimated from family + id (click Details to verify)'
                  : 'Context window unknown — click Details to fetch from config.json'"
              >
                <span class="hardware__ctx-label">ctx</span>
                <span class="hardware__ctx-value">{{ formatContextLength(model.contextLength) }}</span>
                <span class="hardware__ctx-mark">
                  {{ model.contextLengthSource === 'config_json' ? '✓'
                    : model.contextLengthSource === 'estimate' ? '≈'
                    : '—' }}
                </span>
              </span>
            </div>
            <!-- HF repo id — wraps the existing `model.id` text in a
                 real <a> to huggingface.co/{id}, with a small external-
                 link icon beside it. Clicking opens in a new tab. -->
            <a
              :href="hfModelUrl(model.id)"
              target="_blank"
              rel="noopener noreferrer"
              class="hardware__result-repo-link"
              :title="`Open ${model.id} on HuggingFace`"
            >
              <span class="hardware__result-repo">{{ model.id }}</span>
              <span class="hardware__external-icon" aria-hidden="true">↗</span>
            </a>
            <div class="hardware__result-meta">
              {{ model.ggufFilename }} ·
              <span class="hardware__result-size">{{ model.sizeGb.toFixed(1) }} GB</span> ·
              ↓ {{ formatNumber(model.downloads) }} ·
              ❤ {{ formatNumber(model.likes) }} ·
              {{ model.paramCountLabel || '?' }} ·
              {{ model.architecture }}
              <span v-if="model.lastModified"> · {{ relativeTime(model.lastModified) }}</span>
            </div>
            <div class="hardware__result-quantizer">
              Quantizer: <strong>{{ model.quantizerLabel }}</strong>
            </div>
          </div>
          <!-- Expand chevron + top-level Download. Wrapped in exo-actions
               so the two controls stack vertically and align with the
               exo-card geometry (the tile | info | ... | actions rail).
               The Download button keeps its existing primary look; the
               expand chevron keeps its existing geometric icon. -->
          <div class="hardware__result-actions exo-actions">
            <button
              class="hardware__download-btn exo-actions__btn exo-actions__btn--primary"
              :disabled="!fitsForSizeGb(model.sizeGb, targetVramMb)"
              :title="fitsForSizeGb(model.sizeGb, targetVramMb)
                ? `Download ${model.ggufFilename}`
                : `Model exceeds ${targetVramGb > 0 ? targetVramGb + 'GB' : ''} target`"
              @click="downloadModel(model)"
            >Download</button>
            <button
              class="hardware__expand-btn exo-actions__btn"
              :aria-expanded="isExpanded(model.id)"
              :aria-label="isExpanded(model.id) ? `Collapse ${model.name} details` : `Expand ${model.name} details`"
              :title="isExpanded(model.id) ? 'Collapse details' : `Expand — ${model.siblings.length} quants + real context`"
              @click="toggleExpand(model)"
            >{{ isExpanded(model.id) ? '▴' : '▾' }}</button>
          </div>

          <!-- LM-Studio-style per-quant breakdown. Renders ONLY when
               expanded. Filters siblings client-side via the same
               quant + IQ toggles as the sidebar, so the visible table
               always matches the user's stated intent. Each row's
               FITS/TOO BIG reflects the currently-selected machine
               target. -->
          <div v-if="isExpanded(model.id)" class="hardware__expanded">
            <div class="hardware__expanded-header">
              <strong>{{ model.siblings.length }} GGUF variants</strong>
              <span class="hardware__expanded-help">vs. {{ targetVramGb > 0 ? `${targetVramGb} GB target` : 'no GPU data' }} (10% buffer)</span>
            </div>
            <div class="hardware__sibling-table">
              <div
                v-for="sib in visibleSiblingsFor(model)"
                :key="sib.filename"
                class="hardware__sibling-row"
                :class="{ 'hardware__sibling-row--no-fit': !fitsForSizeGb(sib.sizeGb, targetVramMb) }"
              >
                <span
                  class="hardware__sibling-quant"
                  :class="quantColorClass(sib.quant)"
                >{{ quantLabel(sib.quant) }}</span>
                <span class="hardware__sibling-size">{{ sib.sizeGb.toFixed(1) }} GB</span>
                <span
                  class="hardware__sibling-fit"
                  :class="{ 'hardware__sibling-fit--yes': fitsForSizeGb(sib.sizeGb, targetVramMb), 'hardware__sibling-fit--no': !fitsForSizeGb(sib.sizeGb, targetVramMb) }"
                >{{ fitsForSizeGb(sib.sizeGb, targetVramMb) ? 'FITS' : 'TOO BIG' }}</span>
                <button
                  class="hardware__sibling-download"
                  :disabled="!fitsForSizeGb(sib.sizeGb, targetVramMb)"
                  :title="`Download ${sib.filename}`"
                  @click="downloadModel(model, sib)"
                >↓</button>
              </div>
            </div>
            <!-- Real-fetch context affordance. Loading skeleton pulses
                 while in flight. After: (a) real value, (b) heuristic,
                 (c) unknown — distinct template branches so the user
                 knows the provenance. (d) is the explicit fetch-error
                 case from `contextFetchNotice`, rendered inline here
                 so a network/HTTP failure is visible WITHOUT confusing
                 it with the generic "no config.json" branch. -->
            <div class="hardware__ctx-row">
              <template v-if="contextFetchNoticeByRepo[model.id]">
                <span class="hardware__ctx-error">
                  {{ contextFetchNoticeByRepo[model.id] }}
                  <button class="hardware__ctx-fetch-btn" @click="fetchContextLength(model.id)">Retry</button>
                </span>
              </template>
              <template v-else-if="loadingDetail.has(model.id)">
                <span class="hardware__ctx-fetching">Fetching max_position_embeddings from config.json…</span>
              </template>
              <template v-else-if="model.contextLengthSource === 'config_json' && model.contextLength != null">
                <span class="hardware__ctx-real">
                  Context window: <strong>{{ formatContextLength(model.contextLength) }}</strong>
                  <span class="hardware__ctx-source">(from config.json)</span>
                </span>
              </template>
              <template v-else-if="model.contextLengthSource === 'none' || (model.contextLength == null && !loadingDetail.has(model.id))">
                <span class="hardware__ctx-unknown">
                  Context window unknown for this repo — config.json doesn't ship a max_position_embeddings value.
                  <button class="hardware__ctx-fetch-btn" @click="fetchContextLength(model.id)">Retry fetch</button>
                </span>
              </template>
              <template v-else>
                <span class="hardware__ctx-verified-hint">
                  Context is currently a heuristic estimate. <button class="hardware__ctx-fetch-btn" @click="fetchContextLength(model.id)">Verify against config.json</button>
                </span>
              </template>
            </div>
          </div>
        </article>
      </div>

      <div v-if="hasMoreModels" class="hardware__load-more-wrap">
        <button
          class="hardware__load-more-btn"
          @click="loadMoreModels"
        >
          Load next {{ Math.min(PAGE_STEP, models.length - visibleCount) }}
          ({{ models.length - visibleCount }} more total)
        </button>
      </div>

      <div v-else-if="!searchError" class="hardware__empty">
        <p class="hardware__empty-text">
          Pick a filter or type a query.
          Default view: top {{ browseSubtitle }} GGUF models.
          Hardware fit considers {{ targetVramGb > 0 ? `your ${targetVramGb} GB target (${targetVramMb.toLocaleString()} MiB · 10% buffer)` : 'no GPU data yet' }}.
        </p>
      </div>
    </main>
  </div>
</template>

<style scoped>
.hardware {
  display: flex;
  flex-direction: row;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  gap: 1.5rem;
  padding: 1.5rem;
}

/* ─── Sidebar ──────────────────────────────────────────────────────────── */

.hardware__sidebar {
  flex: 0 0 280px;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  overflow-y: auto;
  padding-right: 0.5rem;
  scrollbar-gutter: stable;
}
.hardware__sidebar-section {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.hardware__sidebar-section--actions {
  margin-top: auto;
}
.hardware__section-label {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: hsl(var(--muted-foreground));
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
}
.hardware__section-help {
  font-weight: 400;
  text-transform: none;
  letter-spacing: 0;
  font-size: 0.7rem;
  font-style: italic;
  opacity: 0.8;
}

.hardware__query-wrap {
  position: relative;
  width: 100%;
}
.hardware__query {
  width: 100%;
  padding: 0.55rem 2rem 0.55rem 0.75rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-2));
  color: hsl(var(--foreground));
  font: inherit;
}
.hardware__query:focus {
  outline: 2px solid hsl(var(--primary) / 0.5);
  outline-offset: 1px;
}
.hardware__query::-webkit-search-cancel-button {
  -webkit-appearance: none;
  appearance: none;
}
.hardware__query-clear {
  position: absolute;
  top: 50%;
  right: 0.4rem;
  transform: translateY(-50%);
  width: 1.5rem;
  height: 1.5rem;
  border-radius: 50%;
  background: hsl(var(--background-3, var(--background-2)));
  border: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  font-size: 1rem;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  transition: all 120ms ease;
}
.hardware__query-clear:hover {
  color: hsl(var(--foreground));
  border-color: hsl(var(--primary) / 0.6);
  background: hsl(var(--primary) / 0.1);
}
.hardware__search-btn {
  padding: 0.6rem 0.75rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  border: none;
  cursor: pointer;
  font-weight: 500;
}
.hardware__search-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.hardware__radio-group {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.hardware__radio {
  text-align: left;
  padding: 0.35rem 0.6rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  font-size: 0.85rem;
}
.hardware__radio--active {
  background: hsl(var(--primary) / 0.15);
  border-color: hsl(var(--primary));
  color: hsl(var(--foreground));
  font-weight: 500;
}

.hardware__chip-group {
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
}
.hardware__chip {
  padding: 0.3rem 0.6rem;
  border-radius: 999px;
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  font-size: 0.8rem;
  transition: all 120ms ease;
}
.hardware__chip:hover {
  border-color: hsl(var(--primary) / 0.6);
  color: hsl(var(--foreground));
}
.hardware__chip--active {
  background: hsl(var(--primary) / 0.18);
  border-color: hsl(var(--primary));
  color: hsl(var(--foreground));
  font-weight: 500;
}
.hardware__chip--small {
  padding: 0.2rem 0.5rem;
  font-size: 0.75rem;
}
.hardware__chip--toggle {
  font-style: italic;
  border-style: dashed;
}

.hardware__toggle-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.85rem;
  cursor: pointer;
  user-select: none;
}
.hardware__toggle-row input {
  cursor: pointer;
}
.hardware__toggle-row--off {
  opacity: 0.6;
}
.hardware__vram-inline {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
}

/* Phase 11 LM-Studio parity — machine-selector dropdown. The
   selected target drives every FITS/TOO BIG badge; changing it
   re-renders locally without a fresh HF round-trip (sibling sizes
   are pre-baked in the search response). */
.hardware__machine-select {
  width: 100%;
  padding: 0.45rem 0.6rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  color: hsl(var(--foreground));
  font: inherit;
  font-size: 0.85rem;
  cursor: pointer;
}
.hardware__machine-select:focus {
  outline: 2px solid hsl(var(--primary) / 0.5);
  outline-offset: 1px;
  border-color: hsl(var(--primary));
}

.hardware__clear-btn {
  padding: 0.45rem 0.75rem;
  border-radius: var(--radius-sm);
  background: transparent;
  border: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  font-size: 0.85rem;
}
.hardware__clear-btn:hover {
  border-color: hsl(var(--primary));
  color: hsl(var(--foreground));
}

/* ─── Results ──────────────────────────────────────────────────────────── */

.hardware__results {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  min-height: 0;
  overflow: hidden;
}
.hardware__results-header {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
}
.hardware__results-title {
  font-size: 1rem;
  font-weight: 600;
  margin: 0;
}
.hardware__results-target {
  font-weight: 400;
  color: hsl(var(--muted-foreground));
  font-size: 0.85rem;
}
.hardware__results-sub {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}
.hardware__results-banner {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  font-style: italic;
  padding: 0.4rem 0.6rem 0.4rem 0.9rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  position: relative;
  line-height: 1.4;
  margin-top: 0.25rem;
}
.hardware__results-banner-dot {
  position: absolute;
  top: 0.65rem;
  left: 0.4rem;
  width: 0.4rem;
  height: 0.4rem;
  border-radius: 50%;
  background: hsl(var(--primary));
  box-shadow: 0 0 0 0.2rem hsl(var(--primary) / 0.18);
}
.hardware__results-list {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  scrollbar-gutter: stable;
  padding-right: 0.5rem;
}
/* ==========================================================================
   Per-result card — exo-style row layout (.exo-card--amber supplies the
   gradient bg + amber border + hover glow). The grid here is a 3-column
   variant (tile | info | actions) since per-model rows don't need a
   fluid specs column — all chip / meta lines live inside the info column
   stacked vertically. The grid-template-columns customises the exo-card
   base 4-column template by overriding it for this card type. The 4-col
   layout still kicks in below 1024px via the stack media-query at the
   bottom of exo.css. */
.hardware__result {
  display: grid;
  grid-template-columns: 56px 1fr auto;
  align-items: flex-start;
  gap: 1.1rem;
  /* Override exo-card's 4-col template so hardware.vue fits 3 cols
     without extra unused grid slots. Visual padding inherited from
     exo-card. */
  padding: 0.85rem 1.05rem 0.85rem 0.85rem;
}
/* Stack to single column on tablet/mobile — restores the 1024px
   media-query behaviour defined in exo.css for the base .exo-card.
   Without this override the 3-col rule we just declared would
   shadow the media-query and cards stay tiled on small viewports. */
@media (max-width: 1024px) {
  .hardware__result {
    grid-template-columns: 1fr;
  }
  .hardware__result-tile {
    margin-bottom: 0.4rem;
  }
}

/* ----- HF tile (replaces a missing lucide icon) ----- */
.hardware__result-tile {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: 76px;
  background: linear-gradient(180deg,
    hsl(40 95% 60% / 22%) 0%,
    hsl(var(--background-3)) 60%,
    rgba(0, 0, 0, 0.4) 100%);
  border-radius: var(--radius-sm);
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.4));
  flex-shrink: 0;
  align-self: center;
  position: relative;
  overflow: hidden;
}
.hardware__result-tile::before {
  content: '';
  position: absolute;
  inset: 0;
  background: radial-gradient(ellipse at 50% 0%,
    hsl(40 95% 60% / 28%) 0%, transparent 70%);
  pointer-events: none;
}
.hardware__result-tile-mark {
  position: relative;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  font-weight: 800;
  font-size: 0.85rem;
  color: hsl(40 95% 60%);
  text-shadow: 0 0 4px hsl(40 95% 60% / 55%);
  letter-spacing: 0.02em;
}
.hardware__result-info {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
  /* Exo-style "specs" column named to mirror cluster.vue + backend-manager.vue */
}
.hardware__result-row1 {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}
.hardware__result-name {
  font-weight: 600;
  background: linear-gradient(120deg, hsl(var(--foreground)) 0%, hsl(40 95% 60%) 100%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  font-size: 1.05rem;
}
.hardware__result-quant {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  padding: 0.1rem 0.4rem;
  border-radius: 999px;
  font-weight: 600;
}
.hardware__result-fit {
  font-family: var(--font-mono, monospace);
  font-size: 0.65rem;
  padding: 0.1rem 0.4rem;
  border-radius: 4px;
  font-weight: 700;
}
.hardware__result-fit--yes {
  background: rgba(34, 197, 94, 0.15);
  color: rgb(74, 222, 128);
  border: 1px solid rgba(34, 197, 94, 0.4);
}
.hardware__result-fit--no {
  background: rgba(220, 38, 38, 0.12);
  color: rgb(248, 113, 113);
  border: 1px solid rgba(220, 38, 38, 0.4);
}
.hardware__result-trust {
  font-size: 0.7rem;
  color: rgb(74, 222, 128);
  font-weight: 500;
}

/* ─── HF repo id link (Phase 11 LM-Studio parity) ──────────────────────── */
.hardware__result-repo-link {
  display: inline-flex;
  align-items: center;
  gap: 0.3rem;
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  text-decoration: none;
  word-break: break-all;
}
.hardware__result-repo-link:hover {
  color: hsl(var(--primary));
  text-decoration: underline;
}
.hardware__result-repo {
  font-family: inherit;
  font-size: inherit;
}
.hardware__external-icon {
  font-size: 0.85rem;
  color: hsl(var(--muted-foreground));
  flex-shrink: 0;
}
.hardware__result-repo-link:hover .hardware__external-icon {
  color: hsl(var(--primary));
}

.hardware__result-meta {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  word-break: break-word;
}
.hardware__result-size {
  font-weight: 600;
  color: hsl(var(--foreground));
}
.hardware__result-quantizer {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
}

/* ─── Context-length badge (Phase 11) ────────────────────────────────── */
.hardware__ctx-badge {
  display: inline-flex;
  align-items: center;
  gap: 0.2rem;
  font-size: 0.65rem;
  padding: 0.1rem 0.4rem;
  border-radius: 4px;
  font-family: var(--font-mono, monospace);
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-2));
  color: hsl(var(--muted-foreground));
}
.hardware__ctx-badge--estimate {
  background: rgba(234, 179, 8, 0.12);
  border-color: rgba(234, 179, 8, 0.4);
  color: rgb(250, 204, 21);
}
.hardware__ctx-badge--real {
  background: rgba(34, 197, 94, 0.15);
  border-color: rgba(34, 197, 94, 0.4);
  color: rgb(74, 222, 128);
}
.hardware__ctx-badge--unknown {
  background: hsl(var(--background-2));
  border-color: hsl(var(--border));
  color: hsl(var(--muted-foreground));
  font-style: italic;
}
.hardware__ctx-label {
  opacity: 0.75;
  font-size: 0.6rem;
}
.hardware__ctx-mark {
  font-weight: 700;
  font-size: 0.7rem;
}

/* ─── Per-row actions + expand chevron ───────────────────────────────── */
.hardware__result-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.4rem;
  flex-shrink: 0;
  min-width: 0;
}
.hardware__download-btn {
  /* Exo-style primary button — .exo-actions__btn--primary adds the
     amber border + tint via flex with our existing palette. We keep
     the existing hsl(var(--primary)) background so the button reads
     "primary" instead of "amber accent" — primary-contrast against
     is higher on Download than on a per-kind accent. */
  justify-content: center;
}
.hardware__expand-btn {
  width: auto;
  height: auto;
  padding: 0.4rem 0.7rem;
}
.hardware__expand-btn:hover {
  border-color: hsl(40 95% 60%);
  color: hsl(40 95% 60%);
}

/* ─── Expanded details: per-quant table + context fetch ───────────────── */
.hardware__result--expanded-true .hardware__expanded {
  display: block;
}
.hardware__result--expanded-false .hardware__expanded {
  display: none;
}
.hardware__expanded {
  grid-column: 1 / -1;
  margin-top: 0.6rem;
  padding-top: 0.6rem;
  border-top: 1px dashed hsl(var(--border));
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.hardware__expanded-header {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
  font-size: 0.8rem;
  color: hsl(var(--foreground));
}
.hardware__expanded-help {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  font-style: italic;
}
.hardware__sibling-table {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}
.hardware__sibling-row {
  display: grid;
  grid-template-columns: auto 1fr auto auto;
  align-items: center;
  gap: 0.6rem;
  padding: 0.35rem 0.6rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-1, var(--background-2)));
  border: 1px solid hsl(var(--border));
  font-size: 0.75rem;
}
.hardware__sibling-row--no-fit {
  opacity: 0.55;
  border-style: dashed;
}
.hardware__sibling-quant {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  padding: 0.1rem 0.4rem;
  border-radius: 999px;
  font-weight: 600;
}
.hardware__sibling-size {
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
}
.hardware__sibling-fit {
  font-family: var(--font-mono, monospace);
  font-size: 0.65rem;
  padding: 0.1rem 0.4rem;
  border-radius: 4px;
  font-weight: 700;
}
.hardware__sibling-fit--yes {
  background: rgba(34, 197, 94, 0.15);
  color: rgb(74, 222, 128);
  border: 1px solid rgba(34, 197, 94, 0.4);
}
.hardware__sibling-fit--no {
  background: rgba(220, 38, 38, 0.12);
  color: rgb(248, 113, 113);
  border: 1px solid rgba(220, 38, 38, 0.4);
}
.hardware__sibling-download {
  width: 1.8rem;
  height: 1.6rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  border: none;
  cursor: pointer;
  font-size: 0.9rem;
  font-weight: 700;
  padding: 0;
  line-height: 1;
}
.hardware__sibling-download:hover:not(:disabled) {
  filter: brightness(1.1);
}
.hardware__sibling-download:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.hardware__ctx-row {
  padding: 0.4rem 0.6rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}
.hardware__ctx-fetching {
  font-style: italic;
}
.hardware__ctx-real {
  color: hsl(var(--foreground));
}
.hardware__ctx-source {
  margin-left: 0.4rem;
  font-style: italic;
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
}
.hardware__ctx-unknown {
  color: hsl(var(--muted-foreground));
}
/* Phase 11 LM-Studio parity — error-state styling for the per-repo
   context-fetch failure branch (5th template in the expanded row).
   Without this rule the error text inherits muted-foreground and
   reads identically to the success-shaped "config.json missing"
   branch above; user can't visually distinguish a transient network
   failure from the repo genuinely not shipping a config.json. Red
   foreground + matching button border so the visual cue is obvious. */
.hardware__ctx-error {
  color: rgb(248, 113, 113);
}
.hardware__ctx-error .hardware__ctx-fetch-btn {
  border-color: rgb(248, 113, 113);
  color: rgb(248, 113, 113);
}
.hardware__ctx-verified-hint {
  color: hsl(var(--muted-foreground));
  font-style: italic;
}
.hardware__ctx-fetch-btn {
  margin-left: 0.4rem;
  padding: 0.15rem 0.5rem;
  border-radius: var(--radius-sm);
  background: transparent;
  border: 1px solid hsl(var(--primary));
  color: hsl(var(--primary));
  cursor: pointer;
  font-size: 0.7rem;
  font-weight: 500;
  font-style: normal;
}
.hardware__ctx-fetch-btn:hover {
  background: hsl(var(--primary) / 0.12);
}

.hardware__error {
  font-size: 0.8rem;
  color: hsl(var(--destructive, 0 70% 60%));
  padding: 0.6rem 0.8rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--destructive, 0 70% 60%) / 0.4);
  background: hsl(var(--background-2));
}
.hardware__empty {
  flex: 1;
  display: flex;
  align-items: flex-start;
  justify-content: flex-start;
  padding-top: 0.5rem;
}
.hardware__empty-text {
  color: hsl(var(--muted-foreground));
  font-size: 0.85rem;
  font-style: italic;
}
.hardware__load-more-wrap {
  display: flex;
  justify-content: center;
  padding: 0.75rem 0 0.25rem;
}
.hardware__load-more-btn {
  padding: 0.55rem 1.25rem;
  border-radius: var(--radius-sm);
  background: transparent;
  border: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  font-size: 0.85rem;
  font-weight: 500;
  transition: all 120ms ease;
}
.hardware__load-more-btn:hover:not(:disabled) {
  border-color: hsl(var(--primary));
  color: hsl(var(--foreground));
  background: hsl(var(--primary) / 0.08);
}
.hardware__load-more-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.hardware__skeleton-list {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.hardware__skeleton {
  height: 84px;
  border-radius: var(--radius-sm);
  background: linear-gradient(
    90deg,
    hsl(var(--background-2)) 0%,
    hsl(var(--background-3, var(--background-2))) 50%,
    hsl(var(--background-2)) 100%
  );
  background-size: 200% 100%;
  animation: skeleton-shimmer 1.4s ease-in-out infinite;
  border: 1px solid hsl(var(--border));
}
@keyframes skeleton-shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}

/* ─── Quant color tokens ───────────────────────────────────────────────── */
.hardware__chip--active.quant--q4,
.hardware__chip--active.quant--q4-km,
.hardware__chip--active.quant--q4-ks,
.hardware__chip--active.quant--q4-0,
.hardware__sibling-quant.quant--q4,
.hardware__sibling-quant.quant--q4-km,
.hardware__sibling-quant.quant--q4-ks,
.hardware__sibling-quant.quant--q4-0 {
  background: rgba(34, 197, 94, 0.18);
  color: rgb(74, 222, 128);
  border-color: rgba(34, 197, 94, 0.6);
}
.hardware__result-quant.quant--q4,
.hardware__result-quant.quant--q4-km,
.hardware__result-quant.quant--q4-ks,
.hardware__result-quant.quant--q4-0 {
  background: rgba(34, 197, 94, 0.15);
  color: rgb(74, 222, 128);
  border: 1px solid rgba(34, 197, 94, 0.4);
}
.hardware__chip--active.quant--q5,
.hardware__chip--active.quant--q5-km,
.hardware__chip--active.quant--q5-ks,
.hardware__sibling-quant.quant--q5,
.hardware__sibling-quant.quant--q5-km,
.hardware__sibling-quant.quant--q5-ks {
  background: rgba(234, 179, 8, 0.18);
  color: rgb(250, 204, 21);
  border-color: rgba(234, 179, 8, 0.6);
}
.hardware__result-quant.quant--q5,
.hardware__result-quant.quant--q5-km,
.hardware__result-quant.quant--q5-ks {
  background: rgba(234, 179, 8, 0.15);
  color: rgb(250, 204, 21);
  border: 1px solid rgba(234, 179, 8, 0.4);
}
.hardware__chip--active.quant--q6,
.hardware__chip--active.quant--q6-k,
.hardware__sibling-quant.quant--q6,
.hardware__sibling-quant.quant--q6-k {
  background: rgba(249, 115, 22, 0.18);
  color: rgb(251, 146, 60);
  border-color: rgba(249, 115, 22, 0.6);
}
.hardware__result-quant.quant--q6,
.hardware__result-quant.quant--q6-k {
  background: rgba(249, 115, 22, 0.15);
  color: rgb(251, 146, 60);
  border: 1px solid rgba(249, 115, 22, 0.4);
}
.hardware__chip--active.quant--q8,
.hardware__chip--active.quant--q8-0,
.hardware__sibling-quant.quant--q8,
.hardware__sibling-quant.quant--q8-0 {
  background: rgba(59, 130, 246, 0.18);
  color: rgb(96, 165, 250);
  border-color: rgba(59, 130, 246, 0.6);
}
.hardware__result-quant.quant--q8,
.hardware__result-quant.quant--q8-0 {
  background: rgba(59, 130, 246, 0.15);
  color: rgb(96, 165, 250);
  border: 1px solid rgba(59, 130, 246, 0.4);
}
.hardware__chip--active.quant--f16,
.hardware__chip--active.quant--bf16,
.hardware__sibling-quant.quant--f16,
.hardware__sibling-quant.quant--bf16 {
  background: rgba(168, 85, 247, 0.18);
  color: rgb(192, 132, 252);
  border-color: rgba(168, 85, 247, 0.6);
}
.hardware__result-quant.quant--f16,
.hardware__result-quant.quant--bf16 {
  background: rgba(168, 85, 247, 0.15);
  color: rgb(192, 132, 252);
  border: 1px solid rgba(168, 85, 247, 0.4);
}
.hardware__chip--active.quant--f32,
.hardware__sibling-quant.quant--f32 {
  background: rgba(244, 63, 94, 0.18);
  color: rgb(251, 113, 133);
  border-color: rgba(244, 63, 94, 0.6);
}
.hardware__result-quant.quant--f32 {
  background: rgba(244, 63, 94, 0.15);
  color: rgb(251, 113, 133);
  border: 1px solid rgba(244, 63, 94, 0.4);
}
.hardware__sibling-quant.quant--unknown,
.hardware__result-quant.quant--unknown {
  background: hsl(var(--background-2));
  color: hsl(var(--muted-foreground));
  border: 1px solid hsl(var(--border));
}
</style>
