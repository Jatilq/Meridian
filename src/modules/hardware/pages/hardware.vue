<script setup lang="ts">
/**
 * Hardware Scanner — HuggingFace GGUF model search + browse.
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
 *
 * Defaults (Phase 11 / 2026-06-30 spec):
 *   - query: empty (browse mode active) — Type to search, or browse the
 *     global trending feed.
 *   - sort: downloads desc (Trending) — auto-fired on mount.
 *   - selectedQuants: empty Set (chip group; "no selection = all quants")
 *   - onlyTrustedQuantizers: OFF (chip group hidden entirely). JC wanted
 *     no whitelist pre-applied so NVIDIA's own Nemotron series stays
 *     visible by default.
 *   - onlyFit: OFF. JC wanted full control — the previous auto-on
 *     watcher forced a narrowing that JC found frustrating. Off by
 *     default; the user opts in.
 *   - includeIq: OFF (IQ1/2/3 hidden unless toggled).
 */
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useRoute } from 'vue-router';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { useHardwarePool } from '@/composables/use-hardware-pool';

const userSettingsStore = useUserSettingsStore();
const modelsFolder = computed(() => userSettingsStore.userSettings.meridian?.modelsFolder ?? '');

// ============================================================================
// Deep-link entrypoint: the Backend Manager → Models tab (and quick-actions
// in the future) push to /hardware?searchHuggingface=<query>. Pre-fill the
// search box, then auto-run the search so the user lands on real results
// rather than an empty panel with a populated query.
// ============================================================================

const route = useRoute();
const incomingQuery = computed(() => {
  const raw = route.query.searchHuggingface;
  return typeof raw === 'string' && raw.trim() ? raw.trim() : '';
});

// ============================================================================
// IPC types (must mirror src-tauri/src/hardware.rs)
// ============================================================================

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
  /** Backend-stamped. `"browse"` for empty/None queries (global trending
   *  feed), `"wildcard"` for 1–4 char queries (HF prefix-match), or
   *  `"exact"` for ≥ 5 char queries (HF fuzzy substring match). UI
   *  surfaces a contextual hint line per kind. */
  kind: string;
}

// ============================================================================
// Hardware pane data
//
// Replaced the inline `loadHardware()` with the shared `useHardwarePool`
// composable so the "fit-only" VRAM budget includes remote RPC workers
// getting their GPU memory added into the pool (was previously local-only,
// so combined VRAM read as 36 GB even with BLACK online contributing 16).
// ============================================================================

const {
  entries: hardwareNodes,
  combinedVramMb,
  combinedVramGb,
  combinedGpuCount,
} = useHardwarePool();

const localNode = computed(() => hardwareNodes.value.find((n) => n.isLocal) ?? null);
const localCpuName = computed(() => localNode.value?.cpu?.name?.trim() || 'Unknown CPU');

// NOTE: the previous `watch(combinedVramGb, ...)` auto-fit watcher was
// removed in Phase 11. JC explicitly wanted the fit-toggle to stay OFF
// until opted in (so he can browse oversized models to download for a
// different machine). The 10%-buffer tooltip now documents what ON
// actually does; the user owns the toggle.

// ============================================================================
// Search filters — reactive, bound to sidebar controls
// ============================================================================

// Empty query = browse mode (backend's `"browse"` kind). The input's
// placeholder gives worked examples so users still get type hints.
// `llama` used to be the default; JC explicitly asked for empty.
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
// Default: empty = "include all quants" (no filter). Per JC's Phase 11
// spec, "all quant chips unchecked" means "show every GGUF" — i.e. the
// user opted INTO a restrictive allowlist only by explicitly clicking
// chips. The previous `new Set(['Q4_K_M'])` default was restrictive on
// purpose and acted as a quality floor, but JC explicitly asked for the
// inverse — and the Q4_K_M-only default ate every wildcard "B" search
// result because HF's broad prefix-match response rarely carries a Q4_K_M
// GGUF in the first repo slot.
const selectedQuants = ref<Set<string>>(new Set());

const trustedQuantizerOptions = [
  { key: 'bartowski', label: 'Bartowski' },
  { key: 'unsloth', label: 'Unsloth' },
  { key: 'maziyarpanahi', label: 'MaziyarPanahi' },
  { key: 'lonestriker', label: 'LoneStriker' },
  { key: 'mradermacher', label: 'mradermacher' },
] as const;
// Default OFF — chip group hidden entirely. JC explicitly asked for the
// whitelist to be opt-in; otherwise NVIDIA's own Nemotron series gets
// filtered out on first paint when NVIDIA is not on the curated list.
// When the user toggles ON, the 5 default names pre-populate so they can
// scope down (e.g. unsubscribe from `mradermacher`) right away.
const selectedTrustedQuantizers = ref<Set<string>>(new Set(trustedQuantizerOptions.map((o) => o.key)));
const onlyTrustedQuantizers = ref<boolean>(false);

// Default OFF. Phase 11 change: the previous auto-on watcher was
// removed so JC can browse oversized models without the UI fighting
// him. The tooltip on the toggle now documents the 10% safety buffer
// so a click-ON user knows why a 35GB model gets rejected on 36GB.
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
// Track the kind of the last completed search (browse / wildcard / exact /
// ''). Empty string means no search has run yet. Used by the template to
// render the right hint banner and by the result header for sort-aware
// copy ("Showing top trending GGUF models..." in browse mode).
const lastSearchKind = ref<string>('');
// Sequence counter: every searchModels() call increments this. Resolved
// responses whose captured seq doesn't match the current seq are stale
// (a newer search has superseded them) and are dropped — protects
// `models.value`, `lastSearchKind`, `searchError` from being clobbered
// by an out-of-order older HF response when JC types fast + hits enter.
const searchSeq = ref<number>(0);
// Pagination state: full result set stays in `models.value`; the v-for
// renders only the top `visibleCount` cards so a "Load More" button can
// reveal more without a fresh HF round-trip. PAGE_STEP is the increment
// granularity. `hasMoreModels` is true when the rendered slice is shorter
// than the loaded array — drives the visibility of the "Load More"
// button. Kept as a computed so it stays in sync with both
// `visibleCount.value` increments and `models.value` replacements
// without manual recompute glue in `loadMoreModels` / `clearFilters` /
// `searchModels`. `models.length` is the source of truth for total
// count — we deliberately do NOT keep a parallel `totalModelsRaw` ref
// because that would be a duplicate of state that can drift.
const visibleCount = ref<number>(30);
const PAGE_STEP = 30;
const hasMoreModels = computed(() => visibleCount.value < models.value.length);

// Sort-aware subtitle for the browse-mode banner. Trending/Recent/Liked.
// Kept as a computed so template renders one expression.
const browseSubtitle = computed(() => {
  switch (sortBy.value) {
    case 'lastModified': return 'recently updated';
    case 'likes': return 'most-liked';
    default: return 'trending';
  }
});

// Watch sortBy: when the search box is empty (true browse mode), changing
// the sort radio auto-refires the search so the banner text and the model
// list stay in sync. The watch-fires-when-query-is-empty criterion is
// intentional — using `lastSearchKind === 'browse'` is wrong because that
// state is sticky from the previous round-trip and would fire a SEARCH
// with the user's typed query the moment JC types and toggles the radio.
// Tracking `query.value.trim() === ''` covers both the normal "sort
// change in browse mode" path AND the (x)-cleared transition where the
// user just hit clear and now picks a different sort: in both, the
// intent is "I want the global feed, sorted by my pick". Search-mode
// (typed query, `wildcard` / `exact`) is excluded so JC's half-typed
// search isn't auto-submitted just because he clicked a radio.
watch(sortBy, () => {
  if (!loadingModels.value && query.value.trim() === '') {
    void searchModels();
  }
});

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
  // Empty query IS allowed now — it routes to browse mode (the backend
  // emits ?full=true&...&sort=... without a search param). The old
  // rejection ("Search query must not be empty.") was removed; this
  // branch handles the user-cleared-input edge case so the result
  // list isn't left in a stale state.
  // Capture the seq at function entry. After the await, compare against
  // the current `searchSeq.value` — if it advanced, a NEWER search has
  // superseded this call and its response must not be applied (this
  // prevents models/lastSearchKind/searchError from being clobbered by
  // out-of-order HF responses when JC types fast or click-spams Search).
  const mySeq = ++searchSeq.value;
  loadingModels.value = true;
  searchError.value = '';
  try {
    const params: HardwareSearchParams = {
      // Send null on empty so the backend's `Option::<String>` resolves
      // to `None` cleanly (without the frontend having to pre-trim).
      query: q === '' ? null : q,
      sortBy: sortBy.value,
      // Fetch the full top-100 page so a single round-trip covers most
      // users' first glance; the v-for renders the top `visibleCount`
      // and a "Load More" button reveals more locally. Loosening this to
      // the cap=100 ceiling costs the same HF quota as a 30-only page
      // would; the difference is purely client-side UX.
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
    // Stale response — drop without writing any state. The newer search
    // call will resolve and own the models/lastSearchKind updates.
    if (mySeq !== searchSeq.value) return;
    models.value = result;
    // Reset pagination to the first page on every fresh search — the
    // user just asked a new question, so showing the LATER half of the
    // previous result set would be misleading.
    visibleCount.value = PAGE_STEP;
    // Backend `kind` stamp on each row is the truth source for non-empty
    // results (so bare-stars inputs correctly downgrade to "exact" after
    // the Rust guard). For empty results the backend can't classify — we
    // re-predict locally from `q.length <= 4` to keep the hint visible.
    // Note: an empty `q` here means the user typed nothing OR cleared the
    // (x) button, both of which route to browse mode.
    if (q === '') {
      lastSearchKind.value = 'browse';
    } else {
      lastSearchKind.value = result[0]?.kind ?? (q.length <= 4 ? 'wildcard' : 'exact');
    }
    if (result.length === 0) {
      searchError.value = `No GGUF models matched the current filters. Try clearing a chip or broadening the search.`;
    }
  } catch (error) {
    // Drop errors from stale invocations too — letting them surface would
    // wipe the newer search's still-in-flight state to a misleading
    // error message.
    if (mySeq !== searchSeq.value) return;
    const message = error instanceof Error ? error.message : String(error);
    searchError.value = message;
    models.value = [];
    lastSearchKind.value = '';
  } finally {
    // Only the latest search owns the loading flag; an older finally
    // block must NOT flip loading=false while a newer search has already
    // re-set it to true.
    if (mySeq === searchSeq.value) loadingModels.value = false;
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
  // Clear quants: same default as the initial state — empty set means
  // "all quants" per the Phase 11 spec. Setting this back to
  // `new Set(['Q4_K_M'])` would re-impose the old restrictive default
  // and silence every wildcard search as soon as JC hits Clear.
  selectedQuants.value = new Set();
  selectedTrustedQuantizers.value = new Set(trustedQuantizerOptions.map((o) => o.key));
  onlyTrustedQuantizers.value = false;
  // Phase 11: explicit `false` regardless of hardware pool. The previous
  // version auto-set this based on `combinedVramGb.value > 0`; that
  // forced the fit-toggle back ON whenever the pool resolved with data,
  // which JC found surprising. User owns the toggle.
  onlyFit.value = false;
  includeIq.value = false;
  models.value = [];
  visibleCount.value = PAGE_STEP;
  searchError.value = '';
  lastSearchKind.value = '';
}

/**
 * Reveal the next batch of cards locally. No HF round-trip fires —
 * `models.value` holds up to 100 entries from the last search, so we
 * just expand the rendered window by PAGE_STEP. Cheap to call many
 * times; the `hasMoreModels` computed protects against running off the
 * end of the array. Re-clicking after the array is exhausted is a no-op
 * because the comparison is strict-less-than.
 */
function loadMoreModels() {
  if (!hasMoreModels.value) return;
  visibleCount.value = Math.min(visibleCount.value + PAGE_STEP, models.value.length);
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

// Pin the safety timer in module scope so React-key style "nav away before
// poll resolves" doesn't leak a still-running setTimeout. Vue's lifecycle
// doesn't auto-cancel setTimeout on unmount, so `onUnmounted` clears it
// explicitly. Without this guard, rapid navigation between Hardware
// Scanner and other panels leaks a closure-referenced timer per visit.
let deepLinkSafetyTimer: ReturnType<typeof setTimeout> | null = null;

onMounted(() => {
  // Two paths from mount:
  //   1. Deep-link: route.query.searchHuggingface is set → pre-fill the
  //      search box and gate the search on VRAM data arrival (else size
  //      fit-checks all read "TOO BIG" on initial load because the
  //      pool hasn't polled yet).
  //   2. Normal mount: auto-fire browse mode with current defaults —
  //      query='', sortBy='downloads' (Trending), all chips empty,
  //      fit OFF, no whitelist. First paint shows the top-100 trending
  //      GGUFs from HF. No VRAM wait needed because fit is OFF.
  void (async () => {
    if (incomingQuery.value) {
      // Deep-link flow: gate on VRAM before searching so the size
      // fit-checks have a real threshold. Same watch+timer safety net
      // as before.
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
          // Belt-and-suspenders: when the watch callback already cleared
          // the timer + resolved the Promise, the timer fires anyway. The
          // ref-null check short-circuits the no-op fallback path. Without
          // it, a future agent adding side effects to this branch gets a
          // spurious second invocation on slow systems where watch + timer
          // race on the same microtask.
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
    // Normal mount: auto-fire browse mode (Trending). One nextTick so
    // the empty-state UI paints first — gives the perceived "loading"
    // shimmer instead of looking like an instant populate that
    // masks the round-trip cost.
    await nextTick();
    if (!loadingModels.value) {
      await searchModels();
    }
  })();
});

onUnmounted(() => {
  // Invalidate any in-flight `searchModels()` (auto-fire on mount, deep-link,
  // or manual) by bumping `searchSeq`. The seq-stale guard inside
  // `searchModels` then drops the late resolution without writing to this
  // (now torn-down) component's refs. Without this bump the await would
  // resolve on a dead scope and Vue's silent-drop would emit a console
  // warning in dev builds; the Rust side would already have consumed
  // an HF round-trip for nothing.
  searchSeq.value++;
  // Cancel any in-flight deep-link wait so the closure references the
  // resolved promise's anchor (not the long-gone component scope).
  if (deepLinkSafetyTimer) {
    clearTimeout(deepLinkSafetyTimer);
    deepLinkSafetyTimer = null;
  }
});
</script>

<template>
  <div class="hardware">
    <!-- LEFT — filter sidebar. Right — results panel. -->
    <aside class="hardware__sidebar">
      <div class="hardware__sidebar-section hardware__sidebar-section--search">
        <div class="hardware__query-wrap">
          <input
            v-model="query"
            type="search"
            class="hardware__query"
            placeholder="Search e.g. 'llama-3.1', 'qwen2.5', 'nemotron'"
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
          <!-- Chip color tokens are SCOPED to `.hardware__chip--active` (see
               CSS). Removing the `quant--*` class from inactive chips is
               the fix for the "Q6/Q8 auto-selected" bug — the previous
               template painted the chip a vivid color regardless of
               active state. Now the color is the active-marker, only on
               when the user has clicked the chip. -->
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

      <div class="hardware__sidebar-section">
        <label
          class="hardware__toggle-row"
          :class="{ 'hardware__toggle-row--off': !onlyFit }"
          title="Narrow to models whose best GGUF fits your combined VRAM with a 10% safety buffer for KV cache + runtime overhead."
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

    <!-- RIGHT — results panel -->
    <main class="hardware__results">
      <div class="hardware__results-header">
        <h2 class="hardware__results-title">
          <!-- Pagination message: when fewer than `models.length` cards are
               rendered, show "Showing X of Y" so the user knows there's
               more to see (and the Load More button is the path to it).
               When everything fits, fall back to the plain count for
               parity with the previous render. -->
          <template v-if="hasMoreModels">
            Showing {{ Math.min(visibleCount, models.length) }} of {{ models.length }}
            {{ models.length === 1 ? 'model' : 'models' }}
          </template>
          <template v-else>
            {{ models.length }} {{ models.length === 1 ? 'model' : 'models' }}
          </template>
        </h2>
        <div class="hardware__results-sub">
          Local: <strong>{{ localCpuName }}</strong>
          · {{ combinedGpuCount }} GPU{{ combinedGpuCount === 1 ? '' : 's' }}
          · <strong>{{ combinedVramGb }} GB</strong> combined VRAM
        </div>
        <!-- Browse-mode banner: shown when the user hasn't typed anything
             (or after clicking the (x) clear button) so they know they're
             seeing the global HF feed and how it's sorted. Distinct from
             the wildcard hint so the two never overlap. -->
        <div
          v-if="models.length > 0 && lastSearchKind === 'browse'"
          class="hardware__results-banner"
          aria-live="polite"
        >
          <span class="hardware__results-banner-dot" aria-hidden="true"></span>
          Showing the top <strong>{{ browseSubtitle }}</strong> GGUF models from
          HuggingFace. Type to search, or change the sort above to switch the feed.
        </div>
        <div
          v-if="models.length > 0 && lastSearchKind === 'wildcard'"
          class="hardware__results-banner hardware__results-wildcard-hint"
          aria-live="polite"
        >
          <span class="hardware__results-banner-dot" aria-hidden="true"></span>
          Showing prefix matches for
          <code>{{ query }}*</code>
          — add another character for more specific results.
        </div>
      </div>

      <div v-if="searchError" class="hardware__error">{{ searchError }}</div>

      <div v-if="loadingModels" class="hardware__skeleton-list" aria-hidden="true">
        <div v-for="i in 4" :key="i" class="hardware__skeleton"></div>
      </div>

      <div v-else-if="models.length > 0" class="hardware__results-list">
        <article
          v-for="model in models.slice(0, visibleCount)"
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

      <!-- Local pagination footer. Sits OUTSIDE `hardware__results-list`
           (the vertical scroll container) so the button stays anchored
           below the scrollable cards. Hoisting matters: when JC scrolls
           to card 30 with the button inside the scroll div, the button
           scrolls off with the last card and the user has to scroll back
           up to find it again. The original todo for Fix 4 explicitly
           called out "scroll-cutoff fix" — moving the button below the
           scroll container IS the cutoff fix. Re-clicking after the
           array is exhausted is a no-op because `v-if` hides the whole
           wrap when `hasMoreModels` is false. -->
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

/* Query row: input on the left, (x) clear-button absolutely positioned
   inside the wrap so it overlays the input's right edge. Input gets
   right-padding so typed text doesn't run under the (x) button. */
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
/* Hide the native search-cancel decoration since we provide our own
   cross-browser (×) button. */
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
/* The legacy `.hardware__chip--quant` marker was REMOVED from the chip
   class binding in Phase 11 — it had no CSS rules targeting it in the
   new scope-split design and was dead code. Color tokens now live under
   `.quant--*` rules scoped to `.hardware__chip--active` below. */

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
/* Browse-mode / wildcard-hint banner. Two-tone: the dot on the left is
   a status pip; the message sits next to it. The wildcard hint reuses
   `.hardware__results-wildcard-hint` for its tighter <code> styling. */
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
.hardware__results-wildcard-hint code {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  background: hsl(var(--background-3, var(--background-2)));
  padding: 0.05rem 0.3rem;
  border-radius: 3px;
  color: hsl(var(--foreground));
  margin: 0 0.15rem;
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

/* ─── Quant color tokens ─────────────────────────────────────────────────
 *
 * IMPORTANT — split scopes to fix the "Q6/Q8 auto-selected" bug:
 *
 *   - Chips: color tokens ONLY apply when the chip is .hardware__chip--active.
 *     Without this would paint an inactive Q6 chip orange, looking toggle-on.
 *   - Result badges (.hardware__result-quant): always colored (informational).
 *
 * The compound selector keeps the color-as-active-marker pattern: a
 * chip's color glow tells you the toggle state at a glance. */
.hardware__chip--active.quant--q4,
.hardware__chip--active.quant--q4-km,
.hardware__chip--active.quant--q4-ks,
.hardware__chip--active.quant--q4-0 {
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
.hardware__chip--active.quant--q5-ks {
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
.hardware__chip--active.quant--q6-k {
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
.hardware__chip--active.quant--q8-0 {
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
.hardware__chip--active.quant--bf16 {
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
.hardware__chip--active.quant--f32 {
  background: rgba(244, 63, 94, 0.18);
  color: rgb(251, 113, 133);
  border-color: rgba(244, 63, 94, 0.6);
}
.hardware__result-quant.quant--f32 {
  background: rgba(244, 63, 94, 0.15);
  color: rgb(251, 113, 133);
  border: 1px solid rgba(244, 63, 94, 0.4);
}
.hardware__result-quant.quant--unknown {
  background: hsl(var(--background-2));
  color: hsl(var(--muted-foreground));
  border: 1px solid hsl(var(--border));
}
</style>
