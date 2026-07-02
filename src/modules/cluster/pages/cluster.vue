<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, reactive } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import { PlusIcon, XIcon, KeyRoundIcon, LockIcon, PlugZapIcon, RefreshCwIcon } from '@lucide/vue';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { storeSshPassword } from '@/utils/ssh-connections';
import type { SshConnectionSetting, SshAuthMethod } from '@/types/user-settings';
import { useHardwarePool, type HardwarePoolEntry } from '@/composables/use-hardware-pool';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();
// Cluster Control owns its own worker list separate from the file-browser SSH
// connections. Backend Manager's RPC Slaves tab reads the same array. The
// legacy `meridian.sshConnections` list is reserved for file-browser remote
// panes only.
const clusterWorkers = computed(() => userSettingsStore.userSettings.meridian?.clusterWorkers ?? []);

interface NodeView {
  name: string;
  host: string;
  role: string;
  online: boolean;
  local: boolean;
  cpu: { name: string; cores: number; utilization: number } | null;
  ram: { totalMb: number; usedMb: number; freeMb: number; utilization: number } | null;
  gpus: Array<{
    index: number;
    name: string;
    utilization: number;
    memoryUsed: number;
    memoryTotal: number;
    temperature: number;
  }>;
  error: string | null;
}

// per-source `HardwareSnapshot` sub-shapes (GpuStat, CpuInfo, RamInfo) live on
// the shared `useHardwarePool` composable. We just consume their shape here.
interface NodeDef {
  id: string;
  name: string;
  host: string;
  role: string;
  local: boolean;
}

// MAMBA is the local Meridian node; everything else is a worker.
const nodeDefs = computed<NodeDef[]>(() => {
  const conns = clusterWorkers.value || [];
  const nodes: NodeDef[] = [
    {
      id: 'local',
      name: 'MAMBA',
      host: 'local',
      role: 'Primary inference',
      local: true,
    },
  ];

  // Filter out MAMBA — it's already the local entry above; adding it again
  // from workers would double-count its 3× RTX 3060 in combinedVram.
  conns
    .filter(c => c.label !== 'MAMBA')
    .forEach(c => {
    nodes.push({
      id: c.host,
      name: c.label || c.host,
      host: c.host,
      role: 'Worker node',
      local: false,
    });
  });

  return nodes;
});

// Shared hardware pool — single source of truth for the local Meridian box
// plus every clusterWorker. The polling cadence and React-key stability are
// owned by the composable so hardware.vue and cluster.vue never disagree
// about "what's the latest VRAM/cache snapshot?".
const { entries: hardwareEntries, refresh } = useHardwarePool();

const nodeViews = computed<NodeView[]>(() => {
  const byHost = new Map<string, HardwarePoolEntry>();
  for (const e of hardwareEntries.value) byHost.set(e.host, e);
  return nodeDefs.value.map<NodeView>((def) => {
    const snap = byHost.get(def.host);
    return {
      name: def.name,
      host: def.host,
      role: def.role,
      online: snap?.online ?? false,
      local: def.local,
      cpu: snap?.cpu ?? null,
      ram: snap?.ram ?? null,
      gpus: snap?.gpus ?? [],
      error: snap?.error ?? null,
    };
  });
});

const rpcLaunching = ref(false);
const rpcActive = ref(false);
const rpcMessage = ref('');

// Global manual refresh — wired to the new top-bar Refresh button. Tracks a
// transient `refreshing` flag so the button can show a disabled + spinner
// state. We don't restart the polling timer here; `useHardwarePool`'s refresh
// snapshot is enough to force one immediate poll. Errors are surfaced to the
// same line that launchRpcSlave writes to so JC sees a single status feed
// below the board rather than a separate toast.
const refreshing = ref(false);
const lastError = ref<string | null>(null);
async function refreshAll() {
  if (refreshing.value) return;
  refreshing.value = true;
  lastError.value = null;
  try {
    await refresh();
  } catch (e) {
    console.error('[cluster] refreshAll failed:', e);
    lastError.value = `Refresh failed: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    refreshing.value = false;
  }
}

/** Build an SshCredentials-shaped object for one stored connection. References
 *  the password by secure key only — the backend fetches plaintext from the
 *  secure-keys store at auth time. Plaintext never rides IPC for stored creds. */
function credsFromConn(conn: SshConnectionSetting | undefined) {
  return {
    host: conn?.host ?? '',
    port: conn?.port ?? 22,
    username: conn?.username ?? '',
    keyPath: conn?.keyPath ?? '',
    passwordSecureKey: conn?.passwordSecureKey ?? '',
    authMethod: conn?.authMethod ?? 'key',
  };
}

/** Find the first non-local worker node, or null if none exist. */
function firstWorkerNode(): NodeView | null {
  return nodeViews.value.find(n => !n.local) ?? null;
}

/** Maximum GPU utilization across all GPUs on a node (0 if none). */
function maxUtil(gpus: NodeView['gpus']): number {
  return gpus.length > 0 ? Math.max(...gpus.map(g => g.utilization || 0)) : 0;
}

/** Maximum GPU temperature across all GPUs on a node, formatted string. */
function maxTemp(gpus: NodeView['gpus']): string {
  return gpus.length > 0 ? `${Math.max(...gpus.map(g => g.temperature || 0))}°C` : '—';
}

/** Display-friendly host — turn the local placeholder into 127.0.0.1. */
function displayHost(host: string): string {
  return host === 'local' ? '127.0.0.1' : host;
}

/** Per-node theme key — drives the accent gradient / glow tokens. */
function exoTheme(nodeName: string): 'mamba' | 'black' | 'default' {
  if (nodeName === 'MAMBA') return 'mamba';
  if (nodeName === 'BLACK') return 'black';
  return 'default';
}

// Generic RPC slave launcher. Targets the first non-local worker (BLACK),
// not MAMBA. A brand-new install has zero SSH workers, so the function
// guards on `firstWorkerNode()` and exits early if none found.
async function launchRpcSlave() {
  const target = firstWorkerNode();
  if (!target) {
    rpcMessage.value = 'No worker nodes to launch on. Add one above first.';
    return;
  }
  rpcLaunching.value = true;
  rpcMessage.value = '';
  const conn = clusterWorkers.value?.find(c => c.host === target.host);
  if (!conn) {
    rpcMessage.value = `No SSH connection found for ${target.name}. Re-add the worker.`;
    rpcLaunching.value = false;
    return;
  }
  try {
    const out = await invoke<string>('launch_rpc_slave', {
      creds: credsFromConn(conn),
      rpcCommand: 'llama-server --rpc 0.0.0.0:50052',
    });
    rpcMessage.value = out || `RPC slave launched on ${target.name}.`;
    rpcActive.value = true;
  } catch (error) {
    rpcMessage.value = `Failed: ${error}`;
    rpcActive.value = false;
  } finally {
    rpcLaunching.value = false;
  }
}

// ----- Add Worker dialog -----
interface WorkerForm {
  label: string;
  host: string;
  port: number;
  username: string;
  authMethod: SshAuthMethod;
  keyPath: string;
  password: string;
}

function blankWorker(): WorkerForm {
  return { label: '', host: '', port: 22, username: '', authMethod: 'key', keyPath: '', password: '' };
}

const showAddWorker = ref(false);
const newWorker = reactive<WorkerForm>(blankWorker());
const testing = ref(false);
const testResult = ref<{ ok: boolean; message: string } | null>(null);
const saving = ref(false);

function openAddWorker() {
  Object.assign(newWorker, blankWorker());
  testResult.value = null;
  showAddWorker.value = true;
}

function closeAddWorker() {
  showAddWorker.value = false;
}

const canTest = computed(() =>
  newWorker.host.trim() !== ''
  && newWorker.username.trim() !== ''
  && (newWorker.authMethod === 'key'
    ? newWorker.keyPath.trim() !== ''
    : newWorker.password !== ''),
);

const canSave = computed(() =>
  newWorker.label.trim() !== ''
  && newWorker.host.trim() !== ''
  && newWorker.username.trim() !== ''
  && (newWorker.authMethod === 'key'
    ? newWorker.keyPath.trim() !== ''
    : newWorker.password !== '')
  && newWorker.port > 0 && newWorker.port < 65536,
);

async function testWorkerConnection() {
  if (!canTest.value) return;
  testing.value = true;
  testResult.value = null;
  try {
    await invoke('check_node_status', { creds: credsFromForm(newWorker) });
    testResult.value = { ok: true, message: 'Connected' };
  } catch (error) {
    testResult.value = { ok: false, message: `Failed: ${String(error)}` };
  } finally {
    testing.value = false;
  }
}

function credsFromForm(form: WorkerForm) {
  return {
    host: form.host.trim(),
    port: form.port,
    username: form.username.trim(),
    keyPath: form.authMethod === 'key' ? form.keyPath.trim() : '',
    password: form.authMethod === 'password' ? form.password : '',
    authMethod: form.authMethod,
  };
}

async function saveWorker() {
  if (!canSave.value) return;
  saving.value = true;
  try {
    // Encrypt plaintext password into the secure-keys store before pushing
    // the connection to user-settings. Plaintext lives only in form state.
    let passwordSecureKey: string | undefined;
    if (newWorker.authMethod === 'password' && newWorker.password) {
      passwordSecureKey = await storeSshPassword(newWorker.password);
    }
    const conn: SshConnectionSetting = {
      label: newWorker.label.trim(),
      host: newWorker.host.trim(),
      port: newWorker.port,
      username: newWorker.username.trim(),
      authMethod: newWorker.authMethod,
      keyPath: newWorker.authMethod === 'key' ? newWorker.keyPath.trim() : '',
      passwordSecureKey,
    };
    userSettingsStore.userSettings.meridian.clusterWorkers.push(conn);
    await userSettingsStore.setUserSettingsStorage(
      'meridian.clusterWorkers',
      userSettingsStore.userSettings.meridian.clusterWorkers,
    );
    showAddWorker.value = false;
    await refresh();
  } catch (error) {
    console.error('Failed to save worker connection:', error);
  } finally {
    saving.value = false;
  }
}

// ESC closes the modal
function onDialogKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && showAddWorker.value) {
    closeAddWorker();
  }
}

const combinedVram = computed(() => {
  const totalMb = nodeViews.value
    .flatMap(n => n.gpus)
    .reduce((sum, g) => sum + (g.memoryTotal || 0), 0);
  return totalMb > 0 ? `${(totalMb / 1024).toFixed(0)}GB` : '—';
});

// Nuance line under the VRAM number — tells the user how the value was built
// (e.g. "2 nodes · 4 GPUs"). Replaces the bland "Combined VRAM: 52GB" alone.
const combinedVramDetail = computed(() => {
  const nodes = nodeViews.value.length;
  const gpus = nodeViews.value.flatMap(n => n.gpus).length;
  return `${nodes} node${nodes === 1 ? '' : 's'} · ${gpus} GPU${gpus === 1 ? '' : 's'}`;
});

function gb(mb: number): string {
  return (mb / 1024).toFixed(1);
}

/** Average VRAM utilization percentage across all GPUs. */
function vramUtil(gpus: NodeView['gpus']): number {
  const totalMb = gpus.reduce((s, g) => s + (g.memoryTotal || 0), 0);
  const usedMb = gpus.reduce((s, g) => s + (g.memoryUsed || 0), 0);
  return totalMb > 0 ? Math.round((usedMb / totalMb) * 100) : 0;
}

onMounted(() => {
  window.addEventListener('keydown', onDialogKeydown);
});

onUnmounted(() => {
  window.removeEventListener('keydown', onDialogKeydown);
});
</script>

<template>
  <div class="cluster">
    <!-- Enhanced header: title + section label on the left,
         VRAM tile (animated sheen) + Add Worker pill on the right. -->
    <div class="cluster__header">
      <div class="cluster__header-left">
        <h1 class="cluster__title">Topology</h1>
        <div class="cluster__section-header">NETWORK TOPOLOGY</div>
      </div>
      <div class="cluster__header-right">
        <div class="cluster__summary">
          <div class="cluster__summary-label">COMBINED VRAM</div>
          <div class="cluster__summary-value">
            <strong class="cluster__vram-value">{{ combinedVram }}</strong>
            <span class="cluster__vram-detail">{{ combinedVramDetail }}</span>
          </div>
        </div>
        <div class="cluster__header-actions">
          <button class="cluster__refresh-header" :disabled="refreshing" @click="refreshAll">
            <RefreshCwIcon :size="14" />
            {{ refreshing ? 'Refreshing…' : 'Refresh' }}
          </button>
          <button class="cluster__add-header" @click="openAddWorker">
            <PlusIcon :size="14" />
            Add Worker
          </button>
        </div>
      </div>
    </div>

    <template v-if="nodeViews.length">
      <!-- ================================================================ -->
      <!-- Exo-style board — row-per-node. Icon LEFT, identity+specs RIGHT.   -->
      <!-- Ambient dot-grid backdrop + horizontal RPC line between nodes.     -->
      <!-- No scroll needed: 1-4 nodes fit comfortably on a 1080p viewport.  -->
      <!-- ================================================================ -->
      <div class="cluster__board">
        <div class="cluster__ambient-grid" aria-hidden="true" />

        <!-- RPC connection line — horizontal, sits between the first two
             cards (above them). Glows + flips accent color when RPC active. -->
        <div
          v-if="nodeViews.length > 1"
          class="cluster__rpc-line"
          :class="{ 'cluster__rpc-line--active': rpcActive }"
          aria-hidden="true"
        >
          <span class="cluster__rpc-line-stem" />
          <span class="cluster__rpc-line-badge">RPC</span>
          <span class="cluster__rpc-line-arrows">▶</span>
        </div>

        <div
          v-for="node in nodeViews"
          :key="node.host"
          class="exo-node"
          :class="[
            `exo-node--${exoTheme(node.name)}`,
            { 'exo-node--offline': !node.online, 'exo-node--local': node.local },
          ]"
        >
          <!-- Server tile — CSS-drawn 56x80px rack with animated RGB strip.
               Color is derived from `exoTheme(node.name)` via the
               `--node-accent` token the row sets. -->
          <div class="exo-node__tile" aria-hidden="true">
            <div class="exo-node__rack-ear exo-node__rack-ear--left" />
            <div class="exo-node__body">
              <div class="exo-node__rgb-strip" />
              <div class="exo-node__slot" />
              <div class="exo-node__slot" />
              <div class="exo-node__drive" />
              <div class="exo-node__led" :class="{ 'exo-node__led--on': node.online }" />
            </div>
            <div class="exo-node__rack-ear exo-node__rack-ear--right" />
          </div>

          <!-- Identity column (right of icon) -->
          <div class="exo-node__identity">
            <div class="exo-node__role">{{ node.role }}</div>
            <div class="exo-node__name">{{ node.name }}</div>
            <div class="exo-node__host">{{ displayHost(node.host) }}</div>
          </div>

          <!-- Specs — name/host/specs RIGHT column with rich monospace grid -->
          <div class="exo-node__specs">
            <div class="exo-node__spec-row">
              <span class="exo-node__spec-label">CPU</span>
              <span v-if="node.cpu" class="exo-node__spec-value">
                {{ node.cpu.name }} · {{ node.cpu.cores }}c · {{ node.cpu.utilization.toFixed(0) }}%
              </span>
              <span v-else class="exo-node__spec-value exo-node__spec-value--muted">—</span>
            </div>
            <div class="exo-node__spec-row">
              <span class="exo-node__spec-label">RAM</span>
              <span v-if="node.ram" class="exo-node__spec-value">
                {{ gb(node.ram.usedMb) }}/{{ gb(node.ram.totalMb) }} GB · {{ node.ram.utilization.toFixed(0) }}%
              </span>
              <span v-else class="exo-node__spec-value exo-node__spec-value--muted">—</span>
            </div>
            <div v-if="node.gpus.length" class="exo-node__spec-row exo-node__spec-row--multi">
              <span class="exo-node__spec-label">GPU</span>
              <div class="exo-node__gpu-list">
                <div v-for="gpu in node.gpus" :key="gpu.index" class="exo-node__gpu">
                  <span class="exo-node__gpu-name">#{{ gpu.index }} {{ gpu.name }}</span>
                  <span class="exo-node__gpu-stat">
                    {{ gpu.utilization }}% · {{ gb(gpu.memoryUsed) }}/{{ gb(gpu.memoryTotal) }} GB · {{ gpu.temperature }}°C
                  </span>
                </div>
              </div>
            </div>
            <div v-else class="exo-node__spec-row">
              <span class="exo-node__spec-label">GPU</span>
              <span class="exo-node__spec-value exo-node__spec-value--muted">No GPU data</span>
            </div>
          </div>

          <!-- Status pill + per-node actions -->
          <div class="exo-node__actions">
            <div
              class="exo-node__status"
              :class="{
                'exo-node__status--online': node.online,
                'exo-node__status--offline': !node.online,
              }"
            >
              <span class="exo-node__status-dot" />
              <span class="exo-node__status-text">
                {{ node.online ? `${maxUtil(node.gpus)}% · ${maxTemp(node.gpus)}` : 'Offline' }}
              </span>
            </div>
            <button
              v-if="!node.local && node.online"
              class="exo-node__launch"
              :disabled="rpcLaunching"
              @click="launchRpcSlave"
            >
              {{ rpcLaunching ? 'Launching…' : `Launch RPC on ${node.name}` }}
            </button>
            <!-- Offline diagnostic — surfaces WHY a worker is unreachable.
                 Available via `node.error` from the hardware pool. Hides online
                 nodes and offline-without-error nodes (which just show "Offline"
                 in the status pill). The 0.7rem muted text fits under the launch
                 button without crowding the actions column. -->
            <div v-if="!node.online && node.error" class="exo-node__error">
              Offline — {{ node.error }}
            </div>
          </div>
        </div>
      </div>

      <p
        v-if="rpcMessage"
        class="cluster__rpc-message"
        :class="{ 'cluster__rpc-message--err': rpcMessage.startsWith('Failed') || rpcMessage.startsWith('No ') }"
      >
        {{ rpcMessage }}
      </p>
      <p v-if="lastError" class="cluster__rpc-message cluster__rpc-message--err">
        {{ lastError }}
      </p>
    </template>

    <!-- ===================== Empty state (no SSH workers) ===================== -->
    <template v-else>
      <div class="cluster__empty" role="region" aria-labelledby="cluster-empty-title">
        <div class="cluster__empty-icon" aria-hidden="true">
          <PlusIcon :size="48" />
        </div>
        <h2 id="cluster-empty-title" class="cluster__empty-title">No workers yet</h2>
        <p class="cluster__empty-text">
          Add your first worker to coordinate llama.cpp inference across machines.
          Cluster Control shows the topology, hardware profile, and lets you launch
          an RPC slave on any connected box.
        </p>
        <button
          type="button"
          class="cluster__empty-cta"
          @click="openAddWorker"
        >
          <PlusIcon :size="14" />
          Add your first worker
        </button>
        <p class="cluster__empty-hint">
          Each worker needs an SSH connection (key file or password). Launch RPC
          Slave then runs <code>llama-server --rpc 0.0.0.0:50052</code> over SSH on
          the chosen target — make sure llama.cpp is installed there.
        </p>
      </div>
    </template>

    <!-- Add Worker dialog -->
    <Teleport to="body">
      <Transition name="cluster-modal">
        <div
          v-if="showAddWorker"
          class="cluster-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="cluster-modal-title"
        >
          <div class="cluster-modal__panel">
            <div class="cluster-modal__header">
              <h2 id="cluster-modal-title" class="cluster-modal__title">
                <PlusIcon :size="18" />
                Add Worker
              </h2>
              <button
                type="button"
                class="cluster-modal__close"
                aria-label="Close"
                @click="closeAddWorker"
              >
                <XIcon :size="16" />
              </button>
            </div>

            <div class="cluster-modal__body">
              <div class="cluster-form">
                <div class="cluster-form__row">
                  <div class="cluster-form__field">
                    <label class="cluster-form__label">Label</label>
                    <input
                      v-model="newWorker.label"
                      class="cluster-form__input"
                      type="text"
                      placeholder="MAMBA"
                      autocomplete="off"
                    />
                  </div>
                  <div class="cluster-form__field">
                    <label class="cluster-form__label">Host</label>
                    <input
                      v-model="newWorker.host"
                      class="cluster-form__input"
                      type="text"
                      placeholder="192.168.1.67"
                      autocomplete="off"
                    />
                  </div>
                  <div class="cluster-form__field cluster-form__field--port">
                    <label class="cluster-form__label">Port</label>
                    <input
                      v-model.number="newWorker.port"
                      class="cluster-form__input"
                      type="number"
                      min="1"
                      max="65535"
                    />
                  </div>
                </div>

                <div class="cluster-form__row">
                  <div class="cluster-form__field">
                    <label class="cluster-form__label">Username</label>
                    <input
                      v-model="newWorker.username"
                      class="cluster-form__input"
                      type="text"
                      placeholder="username"
                      autocomplete="off"
                    />
                  </div>
                </div>

                <div class="cluster-form__row">
                  <div class="cluster-form__field">
                    <label class="cluster-form__label">Auth method</label>
                    <div class="cluster-form__toggle">
                      <button
                        type="button"
                        class="cluster-form__toggle-btn"
                        :class="{ 'cluster-form__toggle-btn--active': newWorker.authMethod === 'key' }"
                        @click="newWorker.authMethod = 'key'"
                      >
                        <KeyRoundIcon :size="14" />
                        Key file
                      </button>
                      <button
                        type="button"
                        class="cluster-form__toggle-btn"
                        :class="{ 'cluster-form__toggle-btn--active': newWorker.authMethod === 'password' }"
                        @click="newWorker.authMethod = 'password'"
                      >
                        <LockIcon :size="14" />
                        Password
                      </button>
                    </div>
                  </div>
                </div>

                <div class="cluster-form__row">
                  <div v-if="newWorker.authMethod === 'key'" class="cluster-form__field">
                    <label class="cluster-form__label">Key file path</label>
                    <input
                      v-model="newWorker.keyPath"
                      class="cluster-form__input"
                      type="text"
                      placeholder="C:\Users\name\.ssh\id_ed25519"
                      autocomplete="off"
                    />
                  </div>
                  <div v-else class="cluster-form__field">
                    <label class="cluster-form__label">Password</label>
                    <input
                      v-model="newWorker.password"
                      class="cluster-form__input"
                      type="password"
                      placeholder="••••••••"
                      autocomplete="off"
                    />
                  </div>
                </div>

                <div class="cluster-form__test">
                  <button
                    type="button"
                    class="cluster-form__test-btn"
                    :disabled="!canTest || testing"
                    @click="testWorkerConnection"
                  >
                    <PlugZapIcon :size="14" />
                    {{ testing ? 'Testing…' : 'Test Connection' }}
                  </button>
                  <div v-if="testResult" class="cluster-form__test-result" :class="{
                    'cluster-form__test-result--ok': testResult.ok,
                    'cluster-form__test-result--err': !testResult.ok,
                  }">
                    {{ testResult.ok ? '✅' : '❌' }} {{ testResult.message }}
                  </div>
                </div>
              </div>
            </div>

            <div class="cluster-modal__footer">
              <button
                type="button"
                class="cluster-modal__btn cluster-modal__btn--ghost"
                @click="closeAddWorker"
              >
                Cancel
              </button>
              <button
                type="button"
                class="cluster-modal__btn cluster-modal__btn--primary"
                :disabled="!canSave || saving"
                @click="saveWorker"
              >
                {{ saving ? 'Saving…' : 'Save' }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<style scoped>
/* ── Cluster / Topology — exo-style board layout ──────────────────────────── */
/* Per-row layout: 96px CSS-drawn rack tile | identity column | fluid specs |
   actions column. Per-node accent palette via `--node-accent` so the whole
   card recolors without per-node overrides. Ambient dot-grid behind the
   board keeps the visual depth that the SVG dot pattern used to provide.
   ────────────────────────────────────────────────────────────────────────── */
.cluster {
  /* ── Per-node accent tokens ─────────────────────────────────────────── */
  /* Hex coordinates so consumers can compose them via
     `hsl(var(--cluster-node-X-accent) / N%)`. */
  --cluster-node-mamba-accent: 174 80% 45%;      /* Teal / cyan */
  --cluster-node-mamba-bg-from: hsl(174 80% 45% / 0.20);
  --cluster-node-mamba-bg-to:   hsl(174 80% 45% / 0.04);
  --cluster-node-mamba-border:  hsl(174 80% 45% / 0.55);
  --cluster-node-mamba-glow:    hsl(174 80% 45% / 0.40);

  --cluster-node-black-accent: 348 83% 58%;      /* Coral / red */
  --cluster-node-black-bg-from: hsl(348 83% 58% / 0.20);
  --cluster-node-black-bg-to:   hsl(348 83% 58% / 0.04);
  --cluster-node-black-border:  hsl(348 83% 58% / 0.55);
  --cluster-node-black-glow:    hsl(348 83% 58% / 0.40);

  --cluster-node-default-accent: 280 80% 65%;   /* Magenta / violet */
  --cluster-node-default-bg-from: hsl(280 80% 65% / 0.20);
  --cluster-node-default-bg-to:   hsl(280 80% 65% / 0.04);
  --cluster-node-default-border:  hsl(280 80% 65% / 0.55);
  --cluster-node-default-glow:    hsl(280 80% 65% / 0.40);

  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 1.5rem;
  background: var(--background);
  color: var(--foreground);
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

/* ========================================================================== */
/* Header                                                                    */
/* ========================================================================== */
.cluster__header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 1rem;
}

.cluster__header-left {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.cluster__title {
  font-size: 1.75rem;
  font-weight: 800;
  letter-spacing: -0.01em;
  margin: 0;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  /* Gradient title — picks up --primary, gives the page instant personality
     vs. the previous flat white. */
  background: linear-gradient(120deg, var(--foreground) 0%, hsl(var(--primary)) 100%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
}

.cluster__section-header {
  font-size: 0.7rem;
  font-weight: 700;
  color: var(--muted-foreground);
  text-transform: uppercase;
  letter-spacing: 1.5px;
}

.cluster__header-right {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.cluster__summary {
  position: relative;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  padding: 0.5rem 1rem;
  border: 1px solid hsl(var(--primary) / 40%);
  border-radius: var(--radius-md);
  background: linear-gradient(135deg, hsl(var(--primary) / 14%) 0%, hsl(var(--primary) / 2%) 100%);
}
.cluster__summary::before {
  /* Animated diagonal sheen — gives the VRAM card subtle motion even on idle
     so it reads as a "live" status backdrop rather than a static label. */
  content: '';
  position: absolute;
  inset: 0;
  background: linear-gradient(
    90deg,
    transparent 0%,
    hsl(var(--primary) / 28%) 50%,
    transparent 100%
  );
  transform: translateX(-100%);
  animation: cluster-summary-sheen 4s ease-in-out infinite;
  pointer-events: none;
}
@keyframes cluster-summary-sheen {
  0%, 100% { transform: translateX(-100%); }
  50%      { transform: translateX(100%); }
}
.cluster__summary-label {
  font-size: 0.6rem;
  font-weight: 700;
  letter-spacing: 1.2px;
  color: var(--muted-foreground);
  text-transform: uppercase;
}
.cluster__summary-value {
  display: flex;
  align-items: baseline;
  gap: 0.6rem;
  margin-top: 0.1rem;
}
.cluster__vram-value {
  font-size: 1.5rem;
  font-weight: 800;
  color: var(--primary);
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  letter-spacing: -0.02em;
  text-shadow: 0 0 12px hsl(var(--primary) / 35%);
}
.cluster__vram-detail {
  font-size: 0.7rem;
  color: var(--muted-foreground);
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
}

.cluster__header-actions {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
}

.cluster__refresh-header,
.cluster__add-header {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  font-size: 0.8rem;
  font-weight: 600;
  padding: 0.55rem 1rem;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--foreground);
  cursor: pointer;
  transition: background 0.15s ease, border-color 0.15s ease, transform 0.4s ease;
  font-family: inherit;
}
.cluster__refresh-header:hover:not(:disabled) {
  background: hsl(var(--primary) / 10%);
  border-color: var(--primary);
}
.cluster__refresh-header:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
/* Spin the icon while a manual refresh is in flight. */
.cluster__refresh-header:disabled svg {
  animation: cluster-refresh-spin 1s linear infinite;
}
@keyframes cluster-refresh-spin {
  to { transform: rotate(360deg); }
}

.cluster__add-header:hover {
  background: hsl(var(--primary) / 10%);
  border-color: var(--primary);
}

/* ========================================================================== */
/* Board container                                                            */
/* ========================================================================== */
.cluster__board {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 1.5rem;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background:
    radial-gradient(ellipse at top, hsl(var(--primary) / 10%) 0%, var(--background-2) 60%);
  box-shadow:
    inset 0 0 0 1px hsl(var(--primary) / 5%),
    0 6px 24px rgba(0, 0, 0, 0.22);
  overflow: hidden;
}

.cluster__ambient-grid {
  position: absolute;
  inset: 0;
  background-image: radial-gradient(circle at 2px 2px, hsl(var(--primary) / 22%) 1.2px, transparent 1.2px);
  background-size: 24px 24px;
  opacity: 0.6;
  pointer-events: none;
  z-index: 0;
}

/* ========================================================================== */
/* RPC connection line — sits between the two nodes.                          */
/* ========================================================================== */
.cluster__rpc-line {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: 0.5rem;
  height: 22px;
  margin: 0 192px 0 auto;
  width: 220px;
  transition: opacity 0.3s ease;
  opacity: 0.5;
}
.cluster__rpc-line-stem {
  flex: 1;
  height: 2px;
  background: linear-gradient(
    90deg,
    var(--muted-foreground) 0%,
    hsl(var(--muted-foreground) / 60%) 50%,
    var(--muted-foreground) 100%
  );
  border-radius: 2px;
  position: relative;
  overflow: hidden;
}
.cluster__rpc-line-stem::before {
  /* Dashed flow overlay — reads as "ambient traffic" on the line. */
  content: '';
  position: absolute;
  inset: 0;
  background-image: linear-gradient(
    90deg,
    transparent 0%,
    transparent 40%,
    var(--muted-foreground) 50%,
    transparent 60%,
    transparent 100%
  );
  background-size: 14px 100%;
  background-repeat: repeat-x;
  animation: cluster-rpc-flow 1.6s linear infinite;
  opacity: 0.6;
}
@keyframes cluster-rpc-flow {
  from { background-position: 0 0; }
  to   { background-position: 14px 0; }
}
.cluster__rpc-line-badge {
  font-size: 0.65rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  padding: 0.15rem 0.5rem;
  border-radius: 4px;
  background: var(--background);
  border: 1px solid var(--muted-foreground);
  color: var(--muted-foreground);
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
}
.cluster__rpc-line-arrows {
  font-size: 0.7rem;
  color: var(--muted-foreground);
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
}

.cluster__rpc-line--active {
  opacity: 1;
}
.cluster__rpc-line--active .cluster__rpc-line-stem {
  background: linear-gradient(
    90deg,
    hsl(var(--primary) / 0%) 0%,
    hsl(var(--primary) / 100%) 50%,
    hsl(var(--primary) / 0%) 100%
  );
  box-shadow: 0 0 14px hsl(var(--primary) / 55%);
  animation: cluster-rpc-pulse 1s ease-in-out infinite;
}
.cluster__rpc-line--active .cluster__rpc-line-stem::before {
  background-image: linear-gradient(
    90deg,
    transparent 0%,
    transparent 30%,
    hsl(var(--primary)) 50%,
    transparent 70%,
    transparent 100%
  );
  animation-duration: 0.7s;
  opacity: 1;
}
.cluster__rpc-line--active .cluster__rpc-line-badge {
  background: hsl(var(--primary) / 22%);
  border-color: var(--primary);
  color: var(--primary);
  box-shadow: 0 0 10px hsl(var(--primary) / 60%);
}
.cluster__rpc-line--active .cluster__rpc-line-arrows {
  color: var(--primary);
}
@keyframes cluster-rpc-pulse {
  0%, 100% { opacity: 0.85; }
  50%      { opacity: 1; filter: brightness(1.25); }
}

/* ========================================================================== */
/* Per-node cards (exo-style)                                                  */
/* ========================================================================== */
.exo-node {
  position: relative;
  z-index: 2;
  display: grid;
  grid-template-columns: 96px minmax(180px, 200px) 1fr minmax(180px, 220px);
  align-items: center;
  gap: 1.25rem;
  padding: 1rem 1.25rem;
  border: 1px solid var(--node-border, var(--border));
  border-radius: var(--radius-md);
  background:
    linear-gradient(135deg, var(--node-bg-from, var(--background-3)) 0%, var(--node-bg-to, transparent) 100%),
    var(--background-2);
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.22);
  transition: box-shadow 0.2s ease, transform 0.2s ease, border-color 0.2s ease;
  min-height: 116px;
}
.exo-node:hover {
  transform: translateY(-1px);
  box-shadow: 0 8px 22px rgba(0, 0, 0, 0.28), 0 0 0 1px var(--node-glow, hsl(var(--primary) / 22%));
}
.exo-node--mamba {
  --node-accent: var(--cluster-node-mamba-accent);
  --node-bg-from: var(--cluster-node-mamba-bg-from);
  --node-bg-to:   var(--cluster-node-mamba-bg-to);
  --node-border:  var(--cluster-node-mamba-border);
  --node-glow:    var(--cluster-node-mamba-glow);
}
.exo-node--black {
  --node-accent: var(--cluster-node-black-accent);
  --node-bg-from: var(--cluster-node-black-bg-from);
  --node-bg-to:   var(--cluster-node-black-bg-to);
  --node-border:  var(--cluster-node-black-border);
  --node-glow:    var(--cluster-node-black-glow);
}
.exo-node--default {
  --node-accent: var(--cluster-node-default-accent);
  --node-bg-from: var(--cluster-node-default-bg-from);
  --node-bg-to:   var(--cluster-node-default-bg-to);
  --node-border:  var(--cluster-node-default-border);
  --node-glow:    var(--cluster-node-default-glow);
}

.exo-node--offline {
  opacity: 0.6;
}

/* ── Server tile (CSS-drawn 56×80px rack) ── */
.exo-node__tile {
  position: relative;
  width: 56px;
  height: 80px;
  display: flex;
  align-items: stretch;
  justify-content: center;
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.4));
}
.exo-node__rack-ear {
  width: 8px;
  background: var(--background-3);
  border: 1px solid hsl(var(--node-accent) / 30%);
  border-radius: 2px;
}
.exo-node__rack-ear--left  { transform: skewX(-8deg); border-right: 0; }
.exo-node__rack-ear--right { transform: skewX(8deg);  border-left: 0;  }

.exo-node__body {
  flex: 1;
  position: relative;
  background:
    linear-gradient(180deg, hsl(var(--node-accent) / 22%) 0%, var(--background-3) 60%, rgba(0,0,0,0.45) 100%);
  border-top: 1px solid hsl(var(--node-accent) / 55%);
  border-bottom: 1px solid hsl(var(--node-accent) / 55%);
  display: flex;
  flex-direction: column;
  padding: 4px 4px 6px;
  gap: 3px;
  overflow: hidden;
}
.exo-node__body::before {
  /* Top glow inside the body — sits behind the slots/drive. */
  content: '';
  position: absolute;
  inset: 0;
  background:
    radial-gradient(ellipse at 50% 0%, hsl(var(--node-accent) / 32%) 0%, transparent 70%);
  pointer-events: none;
}
.exo-node__rgb-strip {
  height: 2px;
  background: linear-gradient(
    90deg,
    hsl(var(--node-accent) / 60%),
    hsl(var(--node-accent)),
    hsl(var(--node-accent) / 60%)
  );
  border-radius: 1px;
  box-shadow: 0 0 6px hsl(var(--node-accent) / 65%);
  animation: cluster-rgb-pulse 3s ease-in-out infinite;
}
@keyframes cluster-rgb-pulse {
  0%, 100% { filter: brightness(1); }
  50%      { filter: brightness(1.4); }
}
.exo-node__slot {
  height: 3px;
  background: rgba(0,0,0,0.5);
  border-radius: 1px;
  border-top: 1px solid rgba(255,255,255,0.04);
}
.exo-node__drive {
  flex: 1;
  background: linear-gradient(180deg, rgba(0,0,0,0.4) 0%, rgba(0,0,0,0.6) 100%);
  border-radius: 1px;
  border-top: 1px solid hsl(var(--node-accent) / 30%);
  position: relative;
}
.exo-node__drive::after {
  /* Tiny drive-light dot in the corner. */
  content: '';
  position: absolute;
  bottom: 2px;
  right: 3px;
  width: 4px;
  height: 2px;
  background: hsl(var(--node-accent) / 55%);
  border-radius: 1px;
}
.exo-node__led {
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: #6b7280;
  align-self: flex-end;
  margin-top: 2px;
  transition: background 0.2s ease, box-shadow 0.2s ease;
}
.exo-node__led--on {
  background: hsl(var(--success));
  box-shadow: 0 0 6px hsl(var(--success) / 80%);
  animation: cluster-led-blink 2.4s ease-in-out infinite;
}
@keyframes cluster-led-blink {
  0%, 100% { opacity: 1; }
  50%      { opacity: 0.55; }
}

/* ── Identity column ── */
.exo-node__identity {
  display: flex;
  flex-direction: column;
  gap: 0.1rem;
  min-width: 0;
}
.exo-node__role {
  font-size: 0.65rem;
  font-weight: 700;
  letter-spacing: 1.1px;
  color: var(--muted-foreground);
  text-transform: uppercase;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
}
.exo-node__name {
  font-size: 1.5rem;
  font-weight: 800;
  letter-spacing: -0.01em;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  /* Gradient: white → node accent — gives each card instant identity. */
  background: linear-gradient(120deg, var(--foreground) 0%, hsl(var(--node-accent)) 100%);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  line-height: 1.15;
}
.exo-node__host {
  font-size: 0.75rem;
  color: hsl(var(--node-accent) / 80%);
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  word-break: break-all;
}

/* ── Specs column ── */
.exo-node__specs {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  font-size: 0.8rem;
  min-width: 0;
}
.exo-node__spec-row {
  display: flex;
  gap: 0.5rem;
  align-items: baseline;
}
.exo-node__spec-row--multi {
  align-items: flex-start;
}
.exo-node__spec-label {
  flex-shrink: 0;
  width: 38px;
  font-size: 0.65rem;
  font-weight: 700;
  letter-spacing: 1pt;
  color: hsl(var(--node-accent));
  text-transform: uppercase;
}
.exo-node__spec-value {
  color: var(--foreground);
  font-weight: 500;
  word-break: break-word;
}
.exo-node__spec-value--muted {
  color: var(--muted-foreground);
}
.exo-node__gpu-list {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  flex: 1;
  min-width: 0;
}
.exo-node__gpu {
  display: flex;
  gap: 0.75rem;
  padding: 0.25rem 0.55rem;
  background: rgba(0,0,0,0.25);
  border-left: 2px solid hsl(var(--node-accent) / 55%);
  border-radius: 3px;
  flex-wrap: wrap;
  font-size: 0.75rem;
}
.exo-node__gpu-name {
  color: var(--foreground);
  font-weight: 600;
}
.exo-node__gpu-stat {
  color: var(--muted-foreground);
}

/* ── Actions column ── */
.exo-node__actions {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  justify-content: center;
  gap: 0.5rem;
}
.exo-node__status {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.35rem 0.7rem;
  border-radius: 100px;
  font-size: 0.75rem;
  font-weight: 600;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  border: 1px solid var(--border);
  background: var(--background-3);
  color: var(--muted-foreground);
  align-self: flex-start;
}
.exo-node__status--online {
  background: hsl(var(--success) / 14%);
  border-color: hsl(var(--success) / 45%);
  color: hsl(var(--success));
  box-shadow: 0 0 8px hsl(var(--success) / 30%);
}
.exo-node__status--offline {
  background: rgba(107, 114, 128, 0.12);
  border-color: rgba(107, 114, 128, 0.4);
  color: var(--muted-foreground);
}
.exo-node__status-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 6px currentColor;
}

.exo-node__launch {
  padding: 0.45rem 0.85rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--node-accent) / 65%);
  background: hsl(var(--node-accent) / 14%);
  color: hsl(var(--node-accent));
  cursor: pointer;
  font-size: 0.72rem;
  font-weight: 700;
  letter-spacing: 0.04em;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  transition: background 0.15s ease, color 0.15s ease, box-shadow 0.15s ease;
  text-transform: uppercase;
}
.exo-node__launch:hover:not(:disabled) {
  background: hsl(var(--node-accent));
  color: #ffffff;
  box-shadow: 0 0 12px hsl(var(--node-accent) / 60%);
}
.exo-node__launch:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* Offline error line — red, monospace, surface the cause of a node failure.
   Sits inside .exo-node__actions so it stacks below the status pill + launch
   button. Wraps freely. */
.exo-node__error {
  font-size: 0.7rem;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  color: hsl(var(--destructive) / 85%);
  line-height: 1.3;
  word-break: break-word;
}

/* ========================================================================== */
/* RPC message line                                                           */
/* ========================================================================== */
.cluster__rpc-message {
  font-size: 0.8rem;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  color: hsl(var(--success));
  margin: 0;
}
.cluster__rpc-message--err {
  color: hsl(var(--destructive));
}

/* ========================================================================== */
/* Empty state                                                                */
/* ========================================================================== */
.cluster__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.75rem;
  padding: 2.5rem 1.5rem;
  margin: 1.5rem 0;
  text-align: center;
  background:
    radial-gradient(ellipse at top, hsl(var(--primary) / 10%) 0%, var(--background-2) 70%);
  border: 1px dashed hsl(var(--primary) / 35%);
  border-radius: var(--radius-md);
  flex: 1;
  min-height: 280px;
}
.cluster__empty-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 64px;
  height: 64px;
  color: var(--primary);
  background: hsl(var(--primary) / 14%);
  border: 2px dashed hsl(var(--primary) / 40%);
  border-radius: 50%;
  box-shadow: 0 0 18px hsl(var(--primary) / 22%);
}
.cluster__empty-title {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 700;
  color: var(--foreground);
}
.cluster__empty-text {
  margin: 0;
  font-size: 0.9rem;
  color: var(--muted-foreground);
  max-width: 480px;
  line-height: 1.5;
}
.cluster__empty-cta {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.6rem 1.2rem;
  background: var(--primary);
  color: #ffffff;
  border: 0;
  border-radius: var(--radius-sm);
  font-size: 0.95rem;
  font-weight: 600;
  cursor: pointer;
  margin-top: 0.5rem;
  font-family: inherit;
  box-shadow: 0 0 18px hsl(var(--primary) / 40%);
}
.cluster__empty-cta:hover:not(:disabled) { opacity: 0.85; }
.cluster__empty-cta:disabled { opacity: 0.4; cursor: not-allowed; }
.cluster__empty-hint {
  margin: 0.5rem 0 0;
  font-size: 0.75rem;
  color: var(--muted-foreground);
  max-width: 540px;
  line-height: 1.4;
}
.cluster__empty-hint code {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  background: var(--background-3);
  padding: 0.1rem 0.35rem;
  border-radius: var(--radius-sm);
}

/* ========================================================================== */
/* Add Worker modal — unchanged                                                */
/* ========================================================================== */
.cluster-modal {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.55);
  backdrop-filter: var(--sigma-dialog-overlay-backdrop-blur, blur(8px));
  z-index: 10000;
}
.cluster-modal__panel {
  width: min(540px, 92vw);
  max-height: 90vh;
  display: flex;
  flex-direction: column;
  background: var(--background-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-md, 8px);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
  color: var(--foreground);
  overflow: hidden;
}
.cluster-modal__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.875rem 1rem;
  border-bottom: 1px solid var(--border);
  background: var(--background);
}
.cluster-modal__title {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 1rem;
  font-weight: 700;
  margin: 0;
  color: var(--foreground);
}
.cluster-modal__close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 0;
  padding: 0.25rem;
  color: var(--muted-foreground);
  cursor: pointer;
  border-radius: var(--radius-sm);
}
.cluster-modal__close:hover { background: var(--background); color: var(--foreground); }
.cluster-modal__body { padding: 1rem; overflow-y: auto; }
.cluster-modal__footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.5rem;
  padding: 0.75rem 1rem;
  border-top: 1px solid var(--border);
  background: var(--background);
}
.cluster-modal__btn {
  padding: 0.4rem 0.85rem;
  font-size: 0.85rem;
  border-radius: var(--radius-sm);
  cursor: pointer;
  border: 1px solid transparent;
  transition: background 0.15s ease, border-color 0.15s ease, opacity 0.15s ease;
  font-family: inherit;
}
.cluster-modal__btn:disabled { opacity: 0.4; cursor: not-allowed; }
.cluster-modal__btn--ghost {
  background: transparent;
  border-color: var(--border);
  color: var(--foreground);
}
.cluster-modal__btn--ghost:hover:not(:disabled) { background: var(--background); }
.cluster-modal__btn--primary {
  background: var(--primary);
  color: #ffffff;
  border-color: var(--primary);
}
.cluster-modal__btn--primary:hover:not(:disabled) { opacity: 0.85; }

.cluster-form { display: flex; flex-direction: column; gap: 0.75rem; }
.cluster-form__row { display: flex; align-items: flex-end; gap: 0.5rem; }
.cluster-form__field {
  display: flex; flex: 1; flex-direction: column; gap: 0.25rem; min-width: 0;
}
.cluster-form__field--port { flex: 0 0 88px; }
.cluster-form__label { color: var(--muted-foreground); font-size: 0.75rem; }
.cluster-form__input {
  width: 100%; padding: 0.35rem 0.55rem; font-size: 0.85rem;
  background: var(--background); border: 1px solid var(--border);
  border-radius: var(--radius-sm); color: var(--foreground);
  outline: none; transition: border-color 0.15s ease, box-shadow 0.15s ease;
  font-family: inherit;
}
.cluster-form__input:focus { border-color: var(--primary); box-shadow: 0 0 0 2px var(--primary); }
.cluster-form__input::placeholder { color: var(--muted-foreground); opacity: 0.5; }
.cluster-form__toggle {
  display: inline-flex; border-radius: var(--radius-sm);
  overflow: hidden; border: 1px solid var(--border);
  background: var(--background); align-self: flex-start;
}
.cluster-form__toggle-btn {
  display: inline-flex; align-items: center; gap: 0.25rem;
  padding: 0.35rem 0.6rem; font-size: 0.8rem;
  background: transparent; border: 0;
  color: var(--muted-foreground); cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
  font-family: inherit;
}
.cluster-form__toggle-btn + .cluster-form__toggle-btn { border-left: 1px solid var(--border); }
.cluster-form__toggle-btn:hover { background: var(--background-2); color: var(--foreground); }
.cluster-form__toggle-btn--active {
  background: var(--background-3);
  border-bottom: 2px solid var(--primary);
  color: var(--foreground); font-weight: 600;
}
.cluster-form__test {
  display: flex; align-items: center; gap: 0.5rem;
  padding-top: 0.25rem; flex-wrap: wrap;
}
.cluster-form__test-btn {
  display: inline-flex; align-items: center; gap: 0.25rem;
  padding: 0.35rem 0.65rem; font-size: 0.8rem;
  background: var(--background); border: 1px solid var(--border);
  border-radius: var(--radius-sm); color: var(--foreground); cursor: pointer;
  font-family: inherit;
}
.cluster-form__test-btn:hover:not(:disabled) {
  background: var(--background-2); border-color: var(--primary);
}
.cluster-form__test-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.cluster-form__test-result { font-size: 0.8rem; color: var(--muted-foreground); }
.cluster-form__test-result--ok { color: hsl(var(--success)); }
.cluster-form__test-result--err { color: hsl(var(--destructive)); }

.cluster-modal-enter-from, .cluster-modal-leave-to { opacity: 0; }
.cluster-modal-enter-from .cluster-modal__panel,
.cluster-modal-leave-to .cluster-modal__panel { transform: translateY(8px) scale(0.98); }
.cluster-modal-enter-active, .cluster-modal-leave-active { transition: opacity 0.18s ease; }
.cluster-modal-enter-active .cluster-modal__panel,
.cluster-modal-leave-active .cluster-modal__panel { transition: transform 0.18s ease; }

/* ========================================================================== */
/* Compact layout for narrow viewports — stack specs/actions below identity, */
/* shrink the VRAM tile, fold header right-side actions into one row.         */
/* ========================================================================== */
@media (max-width: 1024px) {
  .exo-node {
    grid-template-columns: 80px 1fr;
    grid-template-areas:
      'tile      identity'
      'specs     specs'
      'actions   actions';
    row-gap: 0.85rem;
  }
  .exo-node__tile     { grid-area: tile; }
  .exo-node__identity { grid-area: identity; }
  .exo-node__specs    { grid-area: specs; }
  .exo-node__actions  {
    grid-area: actions;
    flex-direction: row;
    flex-wrap: wrap;
  }
}

@media (max-width: 768px) {
  .cluster__header {
    flex-wrap: wrap;
    align-items: flex-start;
  }
  /* Squeeze the VRAM pill so Refresh + Add Worker still fit on one line.
     Drop the secondary "nodes · GPUs" detail text under 768. */
  .cluster__vram-detail { display: none; }
  .cluster__vram-value  { font-size: 1.1rem; }
  .cluster__refresh-header,
  .cluster__add-header {
    padding: 0.45rem 0.7rem;
    font-size: 0.72rem;
  }
}
</style>
