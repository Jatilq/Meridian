<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const userSettingsStore = useUserSettingsStore();
const modelsFolder = computed(() => userSettingsStore.userSettings.meridian?.modelsFolder ?? '');

interface ModelInfo {
  id: string;
  name: string;
  downloads: number;
  likes: number;
  url: string;
  filename: string;
  sizeGb: number;
  quant: string;
}

interface ModelSearchResult {
  id: string;
  downloads: number;
  likes: number;
  tags: string[];
}

interface HfModelDetails {
  id: string;
  siblings: Array<{ rfilename: string }>;
}

interface HardwareSnapshot {
  online: boolean;
  cpu: { name: string; cores: number } | null;
  ram: { totalMb: number } | null;
  gpus: Array<{ name: string; memoryTotal: number }>;
}

const nodes = ref<HardwareSnapshot[]>([]);
const models = ref<ModelInfo[]>([]);
const loadingModels = ref(false);
const searchError = ref('');

async function loadHardware() {
  try {
    const local = await invoke<HardwareSnapshot>('get_local_hardware');
    nodes.value = [local];
  } catch {
    nodes.value = [];
  }
}

const combinedVramGb = computed(() => {
  const totalMb = nodes.value
    .flatMap(n => n.gpus)
    .reduce((sum, g) => sum + (g.memoryTotal || 0), 0);
  return totalMb > 0 ? Math.floor(totalMb / 1024) : 0;
});

// Quant tokens in priority order: Q4_K_M preferred, then larger-but-still-fast.
const QUANT_PRIORITY = [
  /^.*Q4_K_M\.gguf$/i,
  /^.*Q5_K_M\.gguf$/i,
  /^.*Q4_K_S\.gguf$/i,
  /^.*Q5_K_S\.gguf$/i,
  /^.*Q8_0\.gguf$/i,
  /^.*Q4_0\.gguf$/i,
  /^.*\.gguf$/i, // last-resort: any .gguf
];

function pickBestGguf(filenames: string[]): { filename: string; quant: string } | null {
  for (const pattern of QUANT_PRIORITY) {
    const hit = filenames.find((name) => pattern.test(name));
    if (hit) {
      const quantMatch = hit.match(/(Q\d_K_[MS]|Q\d_0|Q\d_1|F16|F32|BF16|IQ\d_\w+|IQ\d_\w+)/i);
      return {
        filename: hit,
        quant: quantMatch ? quantMatch[1].toUpperCase().replace('_', '-') : 'GGUF',
      };
    }
  }
  return null;
}

async function fetchModelFiles(modelId: string): Promise<{ filename: string; quant: string } | null> {
  try {
    const response = await fetch(`https://huggingface.co/api/models/${encodeURIComponent(modelId)}`);
    if (!response.ok) return null;
    const data = (await response.json()) as HfModelDetails;
    const ggufNames = (data.siblings || [])
      .map((s) => s.rfilename)
      .filter((name) => /\.gguf$/i.test(name));
    return pickBestGguf(ggufNames);
  } catch {
    return null;
  }
}

async function searchModels() {
  loadingModels.value = true;
  searchError.value = '';
  const query = 'gguf';
  try {
    // Round-18 fix for the "Search button does nothing" symptom. Round-17
    // tried to add a server-side `&filter=gguf|text-generation|...`
    // parameter (assumed pipe-OR semantics), but a live smoke-test
    // confirmed HF treats `search` + `filter` as AND, not OR:
    //
    //     search=gguf alone           → 50 valid results
    //     search=gguf&filter=gguf|a|b → 0 results  (filter excludes all)
    //
    // …and HF's `filter=` parser doesn't recognise `|` OR `,` as OR
    // separators on this endpoint — both forms returned 0. So the
    // round-17 server-side lever was wrong on BOTH combinator AND
    // separator. Dropping it; the surviving client-side levers are:
    //
    //   1. PARALLEL SIBLING RESOLUTION. Sequential `for await` over 8
    //      repos took ~8s wall-clock — felt like the button "did nothing"
    //      until results landed mid-click. Promise.all over 12 candidates
    //      cuts to ~1-2s. HF API tolerates 10-20 simultaneous fetches
    //      from a single IP for short bursts (free tier is ~100 req / 5 min).
    //
    //   2. DIAGNOSTIC LOGGING. Both success and failure paths now log to
    //      `console.log` / `console.error`. Round-13 swallowed errors
    //      into `searchError.value` (user-visible) but left the bug
    //      invisible to anyone diagnosing a button that "stopped working".
    //      The success-path log lets a future round see "received N raw,
    //      M resolved" without an interactive DevTools probe.
    //
    //   3. CLIENT-SIDE SAFETY NET (loose check below): `idBlob.includes
    //      ('gguf') || tagBlob.includes('gguf')` — drop false-positives
    //      that matched `search=gguf` only via description text but ship
    //      no GGUF cargo. Sort by downloads (proxy for "useful repo on
    //      this hardware tier") over likes — round-13 removed the likes > 5
    //      cutoff because many fine-tunes are useful and have <5 likes.
    const url = `https://huggingface.co/api/models?search=${encodeURIComponent(query)}&full=false&limit=50`;
    const response = await fetch(url, {
      method: 'GET',
      headers: { Accept: 'application/json' },
    });
    if (!response.ok) {
      console.error(`[hardware] HF search HTTP ${response.status} (${response.statusText}) for ${url}`);
      searchError.value = `HuggingFace returned HTTP ${response.status}`;
      models.value = [];
      return;
    }
    const data = (await response.json()) as ModelSearchResult[];
    const all = Array.isArray(data) ? data : [];
    if (all.length === 0) {
      console.error(`[hardware] HF search returned 0 repos for "${query}" despite 200 OK`);
      searchError.value = `HuggingFace returned zero search results for "${query}". Try again, or pick from the Models tab.`;
      models.value = [];
      return;
    }
    // Surface the raw count on the SUCCESS path too — without this,
    // a "Search returned 50 raw repos but 0 resolved" diagnostic was
    // completely invisible (round-13 only logged errors). Now webview
    // DevTools will show the raw vs resolved counts every click.
    console.log(`[hardware] HF search received ${all.length} raw repos for "${query}" (post-search, pre-client-filter)`);
    // Client-side safety net. HF's `search=gguf` text filter already
    // restricted to repos matching "gguf" in id, tag, or description,
    // so the surviving rows almost always have GGUF cargo. The check
    // below is `id.includes('gguf') || 'gguf' in tags` — a single
    // loose pass to drop false-positives that mentioned GGUF only in
    // the description but ship no GGUF filesystem entries. Sort by
    // downloads (proxy for "useful repo on this hardware tier") over
    // likes — round-13 removed the `likes > 5` cutoff because many
    // fine-tunes are useful and have <5 likes.
    const candidates = all
      .filter((m) => {
        const idBlob = (m.id || '').toLowerCase();
        const tagBlob = (m.tags || []).join(' ').toLowerCase();
        return idBlob.includes('gguf') || tagBlob.includes('gguf');
      })
      .sort((a, b) => (b.downloads || 0) - (a.downloads || 0))
      .slice(0, 12);
    if (candidates.length === 0) {
      console.error(`[hardware] HF returned ${all.length} repos but filter dropped all of them (none had gguf in id or tags)`);
      searchError.value = `HuggingFace returned ${all.length} repos for "${query}" but none matched the GGUF filter. Try the Models tab instead.`;
      models.value = [];
      return;
    }

    // Resolve up to 12 candidates in PARALLEL. Per-repo try/catch
    // isolates failures so a single 404'd repo doesn't kill the
    // entire list. The narrow `pickBestGguf` filter rejects non-gguf
    // siblings (e.g. safetensors weights only) so a repo with no
    // gguf sibling also logs cleanly.
    const resolvedPicks = await Promise.all(
      candidates.map(async (c) => {
        try {
          const picked = await fetchModelFiles(c.id);
          if (!picked) {
            console.error(`[hardware] HF resolution: ${c.id} returned no .gguf sibling`);
            return null;
          }
          return { candidate: c, picked };
        }
        catch (err) {
          console.error(`[hardware] HF resolution failed for ${c.id}: ${err instanceof Error ? err.message : String(err)}`);
          return null;
        }
      }),
    );
    const resolved: ModelInfo[] = resolvedPicks
      .filter((entry): entry is { candidate: ModelSearchResult; picked: { filename: string; quant: string } } => entry !== null)
      .map(({ candidate: c, picked }) => {
        const modelName = c.id.split('/').pop() || c.id;
        return {
          id: c.id,
          name: modelName,
          downloads: c.downloads,
          likes: c.likes,
          url: `https://huggingface.co/${c.id}/resolve/main/${picked.filename}`,
          filename: picked.filename,
          sizeGb: Math.round(estimatedSizeGb(picked.filename) * 10) / 10,
          quant: picked.quant,
        };
      });
    models.value = resolved;
    if (resolved.length === 0) {
      searchError.value = `${candidates.length} repos passed the LLM filter, but none exposed a concrete .gguf sibling. The HF API can list LLM repos that no longer ship GGUF weights — try the catalog tab instead.`;
    }
  }
  catch (error) {
    console.error(`[hardware] HF search failed: ${error instanceof Error ? error.stack : String(error)}`);
    searchError.value = `Search failed: ${error instanceof Error ? error.message : String(error)}`;
    models.value = [];
  }
  finally {
    loadingModels.value = false;
  }
}

// Rough size estimate from a typical Q4_K_M filename pattern (e.g. "Qwen2.5-7B-Instruct-Q4_K_M.gguf").
// Returns 0 if we can't infer it; the real download will size it correctly.
function estimatedSizeGb(filename: string): number {
  const match = filename.match(/(\d+\.?\d*)[BM]/i);
  if (!match) return 0;
  const value = parseFloat(match[1]);
  if (/GB?$/i.test(match[0])) return value;
  if (/MB?$/i.test(match[0])) return value / 1024;
  return 0;
}

async function downloadModel(model: ModelInfo) {
  try {
    await invoke('downloader_enqueue', {
      url: model.url,
      file_name: model.filename,
      format_id: null,
      auto_save_folder: modelsFolder.value,
      chunk_count: null,
    });
  } catch (error) {
    searchError.value = `Enqueue failed: ${error instanceof Error ? error.message : String(error)}`;
  }
}

onMounted(() => {
  void loadHardware();
});
</script>

<template>
  <div class="hardware">
    <div class="hardware__header">
      <h1 class="hardware__title">Hardware Scanner</h1>
      <div class="hardware__vram">
        Combined VRAM: <strong>{{ combinedVramGb }}GB</strong>
      </div>
    </div>

    <div class="hardware__nodes">
      <div
        v-for="(node, idx) in nodes"
        :key="idx"
        class="hardware__node"
      >
        <span class="hardware__node-name">Local Machine</span>
        <span v-if="node.cpu" class="hardware__node-cpu">{{ node.cpu.name }} ({{ node.cpu.cores }} cores)</span>
        <span v-if="node.gpus.length" class="hardware__node-gpu">
          {{ node.gpus.length }} GPU{{ node.gpus.length > 1 ? 's' : '' }}:
          {{ node.gpus.map(g => g.name).join(', ') }}
        </span>
      </div>
    </div>

    <div class="hardware__actions">
      <button @click="searchModels" :disabled="loadingModels">
        {{ loadingModels ? 'Searching...' : 'Search HuggingFace GGUF models' }}
      </button>
    </div>

    <p v-if="searchError" class="hardware__error">{{ searchError }}</p>

    <div v-if="models.length" class="hardware__models">
      <div
        v-for="model in models"
        :key="model.id"
        class="hardware__model"
      >
        <div class="hardware__model-info">
          <div class="hardware__model-name">{{ model.name }}</div>
          <div class="hardware__model-repo">{{ model.id }}</div>
          <div class="hardware__model-meta">
            {{ model.filename }} ·
            quant: {{ model.quant }} ·
            {{ model.sizeGb > 0 ? `${model.sizeGb} GB · ` : '' }}❤ {{ model.likes }} · ⬇ {{ model.downloads }}
          </div>
        </div>
        <button @click="downloadModel(model)">Download</button>
      </div>
    </div>
    <p v-else-if="!loadingModels && !searchError" class="hardware__hint">
      Click "Search" to enumerate GGUF models available on HuggingFace.
      Each result pre-resolves the actual .gguf file URL before download.
    </p>
  </div>
</template>

<style scoped>
.hardware {
  display: flex;
  flex-direction: column;
  padding: 1.5rem;
  /* Page-level no-scroll container. The active tab's inner list is the
     only scroll region. Previous `height: 100%; overflow-y: auto` made
     .hardware AND .hardware__models BOTH scroll targets — wheel events
     split between them and the inner list's `max-height: calc(100vh - ...)`
     cap was usually larger than the actual remaining space, so its
     bottom got clipped under .hardware's `overflow: auto` boundary
     instead of being reachable via scroll. `flex: 1; min-height: 0`
     claims the full router-view-wrapper height without bleeding past it. */
  flex: 1;
  min-height: 0;
  overflow: hidden;
  gap: 1rem;
}
.hardware__header {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  /* Spacing between header and the next sibling is handled by `.hardware`'s
     `gap: 1rem` — drop the explicit margin-bottom to avoid doubling it. */
}
.hardware__title {
  font-size: 1.25rem;
  font-weight: 600;
}
.hardware__vram {
  color: hsl(var(--muted-foreground));
}
.hardware__nodes {
  margin-bottom: 1rem;
}
.hardware__node {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.75rem;
  background: hsl(var(--background-2));
  border-radius: var(--radius-sm);
}
.hardware__node-name {
  font-weight: 600;
}
.hardware__error {
  font-size: 0.85rem;
  color: hsl(var(--destructive, 0 70% 60%));
  background: hsl(var(--background-2));
  padding: 0.6rem 0.8rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
}
.hardware__hint {
  font-size: 0.85rem;
  color: hsl(var(--muted-foreground));
  font-style: italic;
  padding: 0.5rem 0.8rem;
}
.hardware__models {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  /* The ONLY scroll container in this page. `flex: 1; min-height: 0`
     claims exactly the leftover vertical space inside .hardware
     (header + nodes + actions reserve the rest). The previous
     `max-height: calc(100vh - 340px)` was the actual bug: `100vh`
     measures the full WebView2 viewport and ignores the window
     title bar, app toolbar, .hardware padding, and other UI above this
     list. When the calculated cap exceeded the space .hardware actually
     offered, the list's overflow got clipped under .hardware's
     `overflow: auto` boundary instead of scrolling. Drop the cap;
     let flex do the sizing. */
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  scrollbar-gutter: stable;
}
.hardware__model {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.75rem;
  background: hsl(var(--background-2));
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
}
.hardware__model-info {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
  flex: 1;
}
.hardware__model-name {
  font-weight: 500;
}
.hardware__model-repo {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
}
.hardware__model-meta {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  word-break: break-all;
}
</style>
