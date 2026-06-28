<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import catalogData from '@/data/backends.json';
import type { DirContents } from '@/types/dir-entry';

// ============================================================================
// Catalog types — mirror src/data/backends.json. Kept inline (not exported) so
// the Vue panel compiles against the bundled catalog string-with-asset-paths
// without needing a separate schema-validation step. If the catalog grows a
// richer schema, move these to src/types/backend-catalog.ts.
// ============================================================================
type Hardware = 'cpu' | 'nvidia' | 'amd';

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
  id: string;
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

type RuntimeStatusKind = 'llama.cpp' | 'llamafile' | 'koboldcpp';
type RuntimeStatusString = 'notInstalled' | 'installed' | 'running';

interface BackendRuntimeStatus {
  kind: RuntimeStatusKind;
  status: RuntimeStatusString;
  installPath?: string;
  sizeBytes?: number;
  pid?: number;
  startedAt?: number;
  modelPath?: string;
}

// ============================================================================
// Tab state
// ============================================================================
type TabId = 'backends' | 'models' | 'slaves';

const tabs: { id: TabId; label: string }[] = [
  { id: 'backends', label: 'Backends' },
  { id: 'models', label: 'Models' },
  { id: 'slaves', label: 'RPC Slaves' },
];

const activeTab = ref<TabId>('backends');

// ============================================================================
// Backends tab state
// ============================================================================
const detected = ref<GpuVendorInfo | null>(null);
const statuses = ref<Partial<Record<string, BackendRuntimeStatus>>>({});
const busy = ref<Partial<Record<string, boolean>>>({});
const note = ref<Partial<Record<string, string>>>({});

function matchVariant(entry: BackendEntry): BackendVariant {
  const desired = detected.value?.vendor ?? 'cpu';
  return (
    entry.variants.find((variant) => variant.hardware === desired) ??
    entry.variants.find((variant) => variant.hardware === 'cpu') ??
    entry.variants[0]
  );
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
const modelsDir = computed(() => userSettingsStore.userSettings.meridian?.modelsFolder ?? '');

interface ModelRow {
  filename: string;
  path: string;
  sizeBytes: number;
  quant: string;
}

const models = ref<ModelRow[]>([]);
const modelsBusy = ref(false);
const modelsNote = ref('');

// Detects common GGUF quant tokens from a filename. Patterns we recognize:
//
//   Major-quant-mode  : Q4_K_M / Q5_K_S / Q8_K / Q4_0 / Q4_1 / Q5_0 / Q5_1 / Q8_0
//   Important-Quant   : IQ1_S / IQ2_XXS / IQ2_XS / IQ2_S / IQ2_M
//                        IQ3_XXS / IQ3_XS / IQ3_S / IQ3_M
//                        IQ4_XS / IQ4_NL
//   Float             : F16 / F32 / BF16
//
// Anchored on `_`, `-`, or `.` so we don't accidentally match whole-word
// substrings inside unrelated parts of the filename.
const QUANT_RE = /(?:^|[._-])(IQ[1-4]_(?:XXS|XS|S|M|NL)|Q[0-8]_(?:K_S|K_M|Q4_0|Q4_1|Q5_0|Q5_1|Q8_0)|F16|F32|BF16)(?:[._-]|$)/i;

function parseQuant(filename: string): string {
  const match = filename.match(QUANT_RE);
  return match ? match[1].toUpperCase().replace('_', '-') : 'unknown';
}

// ============================================================================
// RPC Slaves tab state
//
// TODO Phase 11 Step 4: read these from user-settings.sshConnections
// filtered by tag 'cluster-worker'. For Step 3 we mirror cluster.vue's
// hardcoded BLACK so the Launch button is wired end-to-end.
// ============================================================================
interface SlaveRow {
  name: string;
  host: string;
  port: number;
  username: string;
  keyPath: string;
  role: string;
}

// No JC-specific default: slaves are populated from userSettingsStore.meridian
// connections in the next pass; we keep the ref empty here so a fresh install
// shows the empty state instead of a fake BLACK row.
const slaves = ref<SlaveRow[]>([]);

// ============================================================================
// Tauri command wrappers
// ============================================================================
async function refreshBackends(): Promise<void> {
  try {
    detected.value = await invoke<GpuVendorInfo>('detect_local_gpu_vendor');
  }
  catch {
    detected.value = null;
  }
  try {
    const arr = await invoke<BackendRuntimeStatus[]>('get_backend_status', {
      backendKind: null,
    });
    const next: Partial<Record<string, BackendRuntimeStatus>> = {};
    for (const entry of arr) {
      // `entry.kind` from Rust is the literal "llama.cpp" | "llamafile" | "koboldcpp";
      // index into string-keyed map so it lines up with `BackendEntry.id` from the catalog.
      next[entry.kind] = entry;
    }
    statuses.value = next;
  }
  catch {
    statuses.value = {};
  }
}

async function downloadBackend(entry: BackendEntry): Promise<void> {
  busy.value[entry.id] = true;
  note.value[entry.id] = 'Downloading...';
  try {
    const installDir = await invoke<string>('download_backend', {
      backendKind: entry.id,
      targetDir: null,
    });
    note.value[entry.id] = `Installed to ${installDir}`;
    await refreshBackends();
  }
  catch (error) {
    note.value[entry.id] = `Download failed: ${error}`;
  }
  finally {
    busy.value[entry.id] = false;
  }
}

async function startStopBackend(entry: BackendEntry): Promise<void> {
  const status = statuses.value[entry.id as RuntimeStatusKind];
  busy.value[entry.id] = true;
  try {
    if (status?.status === 'running' && typeof status.pid === 'number') {
      await invoke('stop_backend', { pid: status.pid });
      note.value[entry.id] = 'Stopped';
    }
    else {
      const pid = await invoke<number>('start_backend', {
        backendKind: entry.id,
        modelPath: null,
        extraArgs: null,
      });
      note.value[entry.id] = `Started pid=${pid}`;
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

async function refreshModels(): Promise<void> {
  modelsBusy.value = true;
  modelsNote.value = '';
  try {
    const dir = await invoke<DirContents>('list_directory', { path: modelsDir.value });
    models.value = dir.entries
      .filter((entry) => entry.is_file && /\.gguf$/i.test(entry.name))
      .map((entry) => ({
        filename: entry.name,
        path: entry.path,
        sizeBytes: entry.size,
        quant: parseQuant(entry.name),
      }))
      .sort((a, b) => b.sizeBytes - a.sizeBytes);
    if (models.value.length === 0) {
      modelsNote.value = `No .gguf files found in ${modelsDir.value}`;
    }
  }
  catch (error) {
    modelsNote.value = `Could not read ${modelsDir.value}: ${error}`;
    models.value = [];
  }
  finally {
    modelsBusy.value = false;
  }
}

async function launchSlave(slave: SlaveRow): Promise<void> {
  busy.value[slave.name] = true;
  note.value[slave.name] = 'Launching...';
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
    note.value[slave.name] = out || 'RPC slave launch sent';
  }
  catch (error) {
    note.value[slave.name] = `Launch failed: ${error}`;
  }
  finally {
    busy.value[slave.name] = false;
  }
}

onMounted(() => {
  void refreshBackends();
  void refreshModels();
});
</script>

<template>
  <div class="bm">
    <header class="bm__header">
      <h1 class="bm__title">Backend Manager</h1>
      <div class="bm__detected">
        Detected GPU:
        <strong v-if="detected">{{ detected.vendor }}</strong>
        <strong v-else>unknown</strong>
        <span v-if="detected?.gpuName"> · {{ detected.gpuName }}</span>
      </div>
    </header>

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
          <span :class="['bm__status', `bm__status--${statuses[entry.id]?.status ?? 'notInstalled'}`]">
            {{ statuses[entry.id]?.status ?? 'notInstalled' }}
          </span>
        </header>

        <p class="bm__backend-desc">{{ entry.description }}</p>

        <ul class="bm__variants">
          <li
            v-for="variant in entry.variants"
            :key="variant.id"
            :class="['bm__variant', { 'bm__variant--recommended': matchVariant(entry).id === variant.id }]"
          >
            <span class="bm__variant-label">{{ variant.label }}</span>
            <span class="bm__variant-hw">{{ variant.hardware.toUpperCase() }}</span>
            <span class="bm__variant-size">{{ formatBytes(variant.sizeBytes) }}</span>
          </li>
        </ul>

        <div class="bm__backend-meta">
          <span v-if="statuses[entry.id]?.installPath">
            <span class="bm__meta-label">Installed at:</span>
            <code class="bm__meta-value">{{ statuses[entry.id]?.installPath }}</code>
          </span>
          <span v-if="statuses[entry.id]?.sizeBytes">
            <span class="bm__meta-label">Binary size:</span>
            <span>{{ formatBytes(statuses[entry.id]?.sizeBytes) }}</span>
          </span>
          <span v-if="statuses[entry.id]?.pid">
            <span class="bm__meta-label">PID:</span>
            <span>{{ statuses[entry.id]?.pid }}</span>
          </span>
        </div>

        <footer class="bm__backend-footer">
          <button
            class="bm__btn"
            :disabled="
              busy[entry.id]
                || statuses[entry.id]?.status === 'running'
                || statuses[entry.id]?.status === 'installed'
            "
            @click="downloadBackend(entry)"
          >
            {{ statuses[entry.id]?.status === 'notInstalled' ? 'Download' : 'Re-Download' }}
          </button>
          <button
            class="bm__btn bm__btn--primary"
            :disabled="
              busy[entry.id]
                || statuses[entry.id]?.status !== 'installed'
            "
            @click="startStopBackend(entry)"
          >
            {{ statuses[entry.id]?.status === 'running' ? 'Stop' : 'Start' }}
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
          <p class="bm__section-sub">{{ modelsDir }}</p>
        </div>
        <button class="bm__btn" :disabled="modelsBusy" @click="refreshModels">
          {{ modelsBusy ? 'Scanning...' : 'Refresh' }}
        </button>
      </header>

      <p v-if="modelsNote" class="bm__note">{{ modelsNote }}</p>

      <ul v-if="models.length" class="bm__models">
        <li v-for="model in models" :key="model.path" class="bm__model">
          <div class="bm__model-info">
            <div class="bm__model-name">{{ model.filename }}</div>
            <div class="bm__model-meta">{{ formatBytes(model.sizeBytes) }} · quant: {{ model.quant }}</div>
          </div>
          <button
            class="bm__btn"
            disabled
            title="Loading a model is wired in Phase 11 Step 4 (Rain agent tool call)."
          >
            Load
          </button>
        </li>
      </ul>
    </section>

    <!-- ============================ RPC Slaves tab ========================== -->
    <section v-show="activeTab === 'slaves'" class="bm__section" role="tabpanel">
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
  </div>
</template>

<style scoped>
.bm {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 1.5rem;
  height: 100%;
  overflow-y: auto;
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

.bm__status {
  margin-left: auto;
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

.bm__variants {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
  list-style: none;
  margin: 0;
  padding: 0;
}

.bm__variant {
  display: flex;
  gap: 0.4rem;
  align-items: baseline;
  padding: 0.25rem 0.6rem;
  border-radius: var(--radius-sm);
  background: hsl(var(--background-3));
  border: 1px solid transparent;
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}

.bm__variant--recommended {
  border-color: hsl(var(--primary));
  color: hsl(var(--foreground));
}

.bm__variant-label {
  font-weight: 500;
}

.bm__variant-hw {
  font-family: var(--font-mono, monospace);
  font-size: 0.65rem;
  letter-spacing: 0.5px;
  opacity: 0.7;
}

.bm__variant-size {
  font-variant-numeric: tabular-nums;
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

.bm__note {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}

.bm__section-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
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
}

.bm__model-name {
  font-weight: 500;
  font-family: var(--font-mono, monospace);
  font-size: 0.85rem;
}

.bm__model-meta {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
  margin-top: 0.15rem;
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
</style>
