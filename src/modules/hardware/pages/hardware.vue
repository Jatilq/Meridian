<script setup lang="ts">
/**
 * Hardware Scanner — HuggingFace GGUF model search.
 *
 * Replaces the previous single hardcoded "Search HuggingFace GGUF models"
 * button with a real search experience: query input, filter sidebar
 * (architecture / parameter size / quant allowlist / quantizer trust /
 * fit-toggle / include-IQ), pre-filtered result cards ranked by downloads
 * with VRAM-fit badges per JC's combined hardware pool.
 *
 * The new Tauri command `hardware_search_gguf_models` (in
 * `src-tauri/src/hardware.rs`) returns a fully-resolved Vec<RankedGgufModel>
 * from one HF `full=true` round-trip — no client-side HF calls anymore,
 * so chip toggles no longer feel like a no-op until results land.
 */
import { ref, computed, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const userSettingsStore = useUserSettingsStore();
const modelsFolder = computed(() => userSettingsStore.userSettings.meridian?.modelsFolder ?? '');

// ============================================================================
// IPC types (must mirror src-tauri/src/hardware.rs)
// ============================================================================

interface HardwareSearchParams {
  query: string;
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
}

interface HardwareSnapshot {
  cpu?: { name: string; cores: number };
  ram?: { totalMb: number };
  gpus: Array<{ name: string; memoryTotal: number }>;
}

// ============================================================================
// Hardware pane data
// ============================================================================

const nodes = ref<HardwareSnapshot[]>([]);

async function loadHardware() {
  try {
    const local = await invoke<HardwareSnapshot>('get_local_hardware');
    nodes.value = [local];
    // Hardware loaded — flip the fit-filter to ON by default. The ref
    // starts OFF so the brief window before this resolves doesn't hit
    // the 0-byte threshold deadlock.
    if (combinedVramMb.value > 0) {
      onlyFit.value = true;
    }
  } catch {
    nodes.value = [];
  }
}

// Sum every GPU's memoryTotal across local + RPC workers (per
// AGENTS.md Phase 10: a single inference is joint across the pool).
const combinedVramMb = computed(() =>
  nodes.value
    .flatMap((n) => n.gpus)
    .reduce((sum, g) => sum + (g.memoryTotal || 0), 0)
);
const combinedVramGb = computed(() =>
  combinedVramMb.value > 0 ? Math.floor(combinedVramMb.value / 1024) : 0
);

// ============================================================================
// Search filters — reactive, bound to sidebar controls
// ============================================================================

const query = ref<string>('llama');
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
// Default: Q4_K_M only (the speed/quality sweet spot). User toggles the rest.
const selectedQuants = ref<Set<string>>(new Set(['Q4_K_M']));

const trustedQuantizerOptions = [
  { key: 'bartowski', label: 'Bartowski' },
  { key: 'unsloth', label: 'Unsloth' },
  { key: 'maziyarpanahi', label: 'MaziyarPanahi' },
  { key: 'lonestriker', label: 'LoneStriker' },
  { key: 'mradermacher', label: 'mradermacher' },
] as const;
const selectedTrustedQuantizers = ref<Set<string>>(new Set(trustedQuantizerOptions.map((o) => o.key)));
const onlyTrustedQuantizers = ref<boolean>(true);

// Default OFF until hardware data confirms positive VRAM. Defaulting ON
// with `combinedVramMb === 0` caused an empty-results deadlock: the
// fit-threshold becomes 0 and every model fails the check. The toggle
// flips to ON inside `loadHardware` once real VRAM is available.
const onlyFit = ref<boolean>(false);
// IQ1/IQ2/IQ3 are excluded unless this is on; keep OFF by default
// (severe quality hit per AGENTS.md Phase 10).
const includeIq = ref<boolean>(false);

// ============================================================================
// Result list state
// ============================================================================

const models = ref<RankedGgufModel[]>([]);
const loadingModels = ref(false);
const searchError = ref<string>('');

// ============================================================================
// Filter <-> Set helpers (drop-in for chip click handlers)
// ============================================================================

function toggleSetMember(set: Set<string>, key: string) {
  // Vue 3 reactivity tracks Set.add / Set.delete on `ref(new Set())`
  // automatically — no manual reassign needed.
  if (set.has(key)) {
    set.delete(key);
  } else {
    set.add(key);
  }
}

async function searchModels() {
  const q = query.value.trim();
  if (!q) {
    searchError.value = 'Enter a search query (e.g. "llama-3", "qwen2.5 7b").';
    return;
  }
  loadingModels.value = true;
  searchError.value = '';
  try {
    const params: HardwareSearchParams = {
      query: q,
      sortBy: sortBy.value,
      limit: 30,
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
    models.value = result;
    if (result.length === 0) {
      searchError.value = `No GGUF models matched the current filters. Try clearing a chip or broadening the search.`;
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    searchError.value = message;
    models.value = [];
  } finally {
    loadingModels.value = false;
  }
}

async function downloadModel(model: RankedGgufModel) {
  try {
    await invoke('downloader_enqueue', {
      url: model.ggufUrl,
      file_name: model.ggufFilename,
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
  selectedQuants.value = new Set(['Q4_K_M']);
  selectedTrustedQuantizers.value = new Set(trustedQuantizerOptions.map((o) => o.key));
  onlyTrustedQuantizers.value = true;
  onlyFit.value = combinedVramGb.value > 0;
  includeIq.value = false;
  models.value = [];
  searchError.value = '';
}

function quantColorClass(quant: string): string {
  // Q4 = green (best speed/quality). Q5 = yellow. Q6 = orange. Q8+ = blue.
  // Returns a class name mapped in the <style> block.
  if (quant.includes('Q4')) return 'quant--q4';
  if (quant.includes('Q5')) return 'quant--q5';
  if (quant.includes('Q6')) return 'quant--q6';
  if (quant.includes('Q8')) return 'quant--q8';
  if (quant.includes('F16') || quant.includes('BF16')) return 'quant--f16';
  if (quant.includes('F32')) return 'quant--f32';
  return 'quant--unknown';
}

function quantLabel(quant: string): string {
  // Normalise `_` to `-` for display: "Q4_K_M" -> "Q4-K-M".
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

onMounted(() => {
  void loadHardware();
});
</script>

<template>
  <div class="hardware">
    <!-- LEFT — filter sidebar. Right — results panel. -->
    <aside class="hardware__sidebar">
      <div class="hardware__sidebar-section">
        <input
          v-model="query"
          type="search"
          class="hardware__query"
          placeholder="Search e.g. 'llama-3.1', 'qwen2.5'"
          @keydown.enter="searchModels"
        >
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
            {{ opt === 'downloads' ? 'Trending' : opt === 'lastModified' ? 'Recent' : 'Most liked' }}
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
          <span class="hardware__section-help">IQ1/2/3 hidden by default</span>
        </div>
        <div class="hardware__chip-group">
          <button
            v-for="q in quantOptions"
            :key="q"
            class="hardware__chip"
            :class="['hardware__chip--quant', `quant--${q.toLowerCase().replace('_','-')}`, { 'hardware__chip--active': selectedQuants.has(q) }]"
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
        <div class="hardware__section-label">Quantizer trust</div>
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

      <div class="hardware__sidebar-section">
        <label class="hardware__toggle-row" :class="{ 'hardware__toggle-row--off': !onlyFit }">
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

    <!-- RIGHT — results panel -->
    <main class="hardware__results">
      <div class="hardware__results-header">
        <h2 class="hardware__results-title">
          {{ models.length }} {{ models.length === 1 ? 'model' : 'models' }}
        </h2>
        <div class="hardware__results-sub">
          Local: <strong>{{ nodes[0]?.cpu?.name ?? 'Unknown CPU' }}</strong>
          · {{ nodes.flatMap(n => n.gpus).length }} GPU{{ nodes.flatMap(n => n.gpus).length === 1 ? '' : 's' }}
          · <strong>{{ combinedVramGb }} GB</strong> combined VRAM
        </div>
      </div>

      <div v-if="searchError" class="hardware__error">{{ searchError }}</div>

      <div v-if="loadingModels" class="hardware__skeleton-list" aria-hidden="true">
        <div v-for="i in 4" :key="i" class="hardware__skeleton"></div>
      </div>

      <div v-else-if="models.length > 0" class="hardware__results-list">
        <article
          v-for="model in models"
          :key="model.id + ':' + model.ggufFilename"
          class="hardware__result"
          :class="{ 'hardware__result--no-fit': !model.fitsHardware }"
        >
          <div class="hardware__result-info">
            <div class="hardware__result-row1">
              <span class="hardware__result-name">{{ model.name }}</span>
              <span
                class="hardware__result-quant"
                :class="quantColorClass(model.primaryQuant)"
              >{{ quantLabel(model.primaryQuant) }}</span>
              <span
                class="hardware__result-fit"
                :class="{ 'hardware__result-fit--yes': model.fitsHardware, 'hardware__result-fit--no': !model.fitsHardware }"
              >
                {{ model.fitsHardware ? 'FITS' : 'TOO BIG' }}
              </span>
              <span v-if="model.isTrustedQuantizer" class="hardware__result-trust">✓ trusted</span>
            </div>
            <div class="hardware__result-repo">{{ model.id }}</div>
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
          <button
            class="hardware__download-btn"
            :disabled="!model.fitsHardware"
            :title="model.fitsHardware ? `Download ${model.ggufFilename}` : 'Model exceeds combined VRAM'"
            @click="downloadModel(model)"
          >Download</button>
        </article>
      </div>

      <div v-else-if="!searchError" class="hardware__empty">
        <p class="hardware__empty-text">
          Enter a query above and pick filters.
          Hardware fit considers {{ combinedVramGb > 0 ? `your ${combinedVramGb}GB combined VRAM (local + RPC workers)` : 'no GPU data yet' }}.
        </p>
      </div>
    </main>
  </div>
</template>

<style scoped>
.hardware {
  display: flex;
  flex-direction: row;
  /* Page-level no-scroll container. claim the full router-view-wrapper
     height — inner panels scroll independently. */
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
.hardware__query {
  width: 100%;
  padding: 0.55rem 0.75rem;
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
.hardware__results-sub {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
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
.hardware__result {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 1rem;
  padding: 0.75rem 1rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-2));
}
.hardware__result--no-fit {
  opacity: 0.75;
  border-style: dashed;
}
.hardware__result-info {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
  flex: 1;
}
.hardware__result-row1 {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.5rem;
}
.hardware__result-name {
  font-weight: 600;
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
.hardware__result-repo {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  word-break: break-all;
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
.hardware__download-btn {
  flex-shrink: 0;
  padding: 0.4rem 0.9rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  border: none;
  cursor: pointer;
  font-weight: 500;
  font-size: 0.85rem;
  transition: filter 120ms ease;
}
.hardware__download-btn:hover:not(:disabled) {
  filter: brightness(1.1);
}
.hardware__download-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
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

/* Quant color tokens. Mind the prefix `quant--q4-km` etc matches what
 * `quantColorClass()` returns. We use em-dash variants so the chip
 * matches the badge in the result card. */
.quant--q4,
.quant--q4-km,
.quant--q4-ks,
.quant--q4-0 {
  background: rgba(34, 197, 94, 0.15);
  color: rgb(74, 222, 128);
  border: 1px solid rgba(34, 197, 94, 0.4);
}
.quant--q5,
.quant--q5-km,
.quant--q5-ks {
  background: rgba(234, 179, 8, 0.15);
  color: rgb(250, 204, 21);
  border: 1px solid rgba(234, 179, 8, 0.4);
}
.quant--q6,
.quant--q6-k {
  background: rgba(249, 115, 22, 0.15);
  color: rgb(251, 146, 60);
  border: 1px solid rgba(249, 115, 22, 0.4);
}
.quant--q8,
.quant--q8-0 {
  background: rgba(59, 130, 246, 0.15);
  color: rgb(96, 165, 250);
  border: 1px solid rgba(59, 130, 246, 0.4);
}
.quant--f16,
.quant--bf16 {
  background: rgba(168, 85, 247, 0.15);
  color: rgb(192, 132, 252);
  border: 1px solid rgba(168, 85, 247, 0.4);
}
.quant--f32 {
  background: rgba(244, 63, 94, 0.15);
  color: rgb(251, 113, 133);
  border: 1px solid rgba(244, 63, 94, 0.4);
}
.quant--unknown {
  background: hsl(var(--background-2));
  color: hsl(var(--muted-foreground));
  border: 1px solid hsl(var(--border));
}
</style>
