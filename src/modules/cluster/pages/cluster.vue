<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, reactive } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import { PlusIcon, XIcon, KeyRoundIcon, LockIcon, PlugZapIcon } from '@lucide/vue';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { storeSshPassword, clearSshPassword } from '@/utils/ssh-connections';
import type { SshConnectionSetting, SshAuthMethod } from '@/types/user-settings';
import { useHardwarePool, type HardwarePoolEntry } from '@/composables/use-hardware-pool';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();
// Round-26 reset: Cluster Control owns its own worker list separate from
// the file-browser SSH connections. Backend Manager's RPC Slaves tab
// reads the same array. The legacy `meridian.sshConnections` list is
// reserved for the file-browser remote-pane routing only.
const clusterWorkers = computed(() => userSettingsStore.userSettings.meridian?.clusterWorkers ?? []);

// View type is kept: the template still iterates rows of `NodeView` shape.
// The per-source `HardwareSnapshot` sub-shapes (GpuStat, CpuInfo, RamInfo)
// were removed along with the now-deleted `refreshNodeViews()` — those
// types are owned by the shared `useHardwarePool` composable.
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

// Node definition for the topology map — drives role/label metadata that
// the composable doesn't carry (it only knows isLocal + author/host).
// MAMBA is the local Meridian node; everything else is a worker.
interface NodeDef {
  id: string;
  name: string;
  host: string;
  role: string;
  local: boolean;
}

// Build node list from the hardware pool's local-machine entry plus
// any configured cluster workers. The local machine (MAMBA) uses
// host='local' to match the useHardwarePool composable's built-in
// local source — without this the join in nodeViews drops MAMBA's
// GPUs and combinedVram never includes them. (Fix 1: the old code
// only added local when a clusterWorker had label === 'MAMBA', which
// meant a fresh install with no workers configured saw 0 local GPUs.)
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

  // All cluster workers are remote. Filter out MAMBA — it's already
  // the local-machine entry above; adding it again from workers would
  // double-count its 3× RTX 3060 in combinedVram (36 GB × 2 = 72 GB)
  // and show two nodes for the same physical machine.
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

// Cluster View = nodeDef (without polling) × hardwareEntry-by-host join.
// The composable's `local` placeholder (host === 'local') won't match any
// clusterWorkers hostname — that's intentional; Cluster Control routes MAMBA
// through `nodeDefs.local = true` and the composable fetches it via its own
// non-MAMBA-spec entry. Joining instead of refetching removes the poller
// dedup and the stale-while-refetch race.
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

/** Memory text line: 36.0GB/36.0GB (100%), monospace. Returns '—' when offline. */
function memText(node: NodeView): string {
  if (!node.online || node.gpus.length === 0) return '—';
  const total = node.gpus.reduce((s, g) => s + (g.memoryTotal || 0), 0);
  const used = node.gpus.reduce((s, g) => s + (g.memoryUsed || 0), 0);
  const pct = total > 0 ? ((used / total) * 100).toFixed(0) : '0';
  return `${(used / 1024).toFixed(1)}GB/${(total / 1024).toFixed(1)}GB (${pct}%)`;
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

const firstWorkerName = computed(() => firstWorkerNode()?.name ?? '');


// ----- Add Worker dialog (Fix 4) -----
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
    <div class="cluster__header">
      <h1 class="cluster__title">Topology</h1>
      <div class="cluster__summary">
        Combined VRAM: <strong class="cluster__vram-value">{{ combinedVram }}</strong>
      </div>
    </div>

    <div class="cluster__section-header">NETWORK TOPOLOGY</div>

    <template v-if="nodeViews.length">
      <!-- Topology Map — scaled 1.65× for visual dominance.
           Vertical 2-node layout. Name labels ABOVE each icon.
           Stat badge BESIDE icon (same row, vertically centered).
           Horizontal VRAM fill-bar near bottom of each icon.
           Connection line with chevron arrowheads, bright when RPC active.
           Monospace throughout, accent color from theme.
           viewBox 400×580 accommodates 2 nodes at 260px spacing. -->
      <div class="cluster__topology">
      <svg
        viewBox="0 0 400 580"
        class="cluster__topology-svg"
        xmlns="http://www.w3.org/2000/svg"
      >
        <defs>
          <marker id="arrow-down" markerWidth="10" markerHeight="10" refX="10" refY="0" orient="auto">
            <polygon points="0,-5 10,0 0,5" fill="currentColor" />
          </marker>
          <marker id="arrow-up" markerWidth="10" markerHeight="10" refX="0" refY="0" orient="auto">
            <polygon points="10,-5 0,0 10,5" fill="currentColor" />
          </marker>
        </defs>

        <!-- ================================================================ -->
        <!-- Connection line: MAMBA (top) ↔ BLACK (bottom)                   -->
        <!-- ================================================================ -->
        <g v-if="nodeViews.length > 1">
          <line
            x1="120" y1="197"
            x2="120" y2="288"
            class="cluster__conn-line"
            :class="{ 'cluster__conn-line--active': rpcActive }"
            marker-start="url(#arrow-up)"
            marker-end="url(#arrow-down)"
          />
          <rect x="102" y="227" width="36" height="16" rx="4" class="cluster__conn-badge-bg" />
          <text x="120" y="238" text-anchor="middle" class="cluster__conn-badge">RPC</text>
        </g>

        <!-- ================================================================ -->
        <!-- Per-node groups. Each node at y = idx * 260.                    -->
        <!-- Icon group scaled 1.65× around its center (120, 96) via nested  -->
        <!-- transform, so all icon-element coordinates stay original.        -->
        <!-- ================================================================ -->
        <g
          v-for="(node, idx) in nodeViews"
          :key="node.host"
          :transform="`translate(0, ${idx * 260})`"
        >
          <title>{{ node.name }} ({{ node.host }})
{{ node.online ? 'Online' : 'Offline' }}
CPU: {{ node.cpu?.name ?? 'N/A' }} · {{ node.cpu?.cores ?? '?' }} cores
RAM: {{ node.ram ? (node.ram.usedMb/1024).toFixed(1) + '/' + (node.ram.totalMb/1024).toFixed(1) + 'GB' : 'N/A' }}
GPU{{ node.gpus.length > 1 ? 's: ' : ': ' }}{{ node.gpus.length > 0 ? node.gpus.map(g => g.name + ' (' + (g.memoryUsed/1024).toFixed(1) + '/' + (g.memoryTotal/1024).toFixed(1) + 'GB)').join(', ') : 'None' }}</title>

          <!-- === Name label ABOVE icon === -->
          <text x="120" y="0" text-anchor="middle" class="cluster__node-label-name">{{ node.name }}</text>
          <text x="120" y="16" text-anchor="middle" class="cluster__node-label-host">{{ node.host }}</text>

          <!-- === Device icon — scaled 1.65× around original center (100, 58)
               then positioned at viewBox center (120, 96). Pattern:
               translate(destX, destY) scale(s) translate(-origX, -origY) -->
          <g
            transform="translate(120, 96) scale(1.65) translate(-100, -58)"
            class="cluster__tower-icon"
            :class="{
              'cluster__tower-icon--online': node.online,
              'cluster__tower-icon--offline': !node.online,
            }"
          >
            <!-- Icon elements at original coordinates; 1.65× scaling
                 applied uniformly by the parent transform. -->
            <g v-if="idx === 0">
              <rect x="64" y="4" width="10" height="18" rx="2" class="cluster__rack-ear" />
              <rect x="126" y="4" width="10" height="18" rx="2" class="cluster__rack-ear" />
              <rect x="74" y="8" width="52" height="100" rx="3" class="cluster__tower-body" />
              <rect x="82" y="16" width="36" height="3" rx="1" class="cluster__icon-slot" />
              <rect x="82" y="24" width="36" height="3" rx="1" class="cluster__icon-slot" />
              <rect x="82" y="32" width="36" height="3" rx="1" class="cluster__icon-slot" />
              <rect x="82" y="44" width="36" height="8" rx="1" class="cluster__icon-drive" />
              <rect x="82" y="58" width="36" height="8" rx="1" class="cluster__icon-drive" />
              <rect x="82" y="74" width="36" height="3" rx="1" class="cluster__icon-slot" />
              <rect x="82" y="82" width="36" height="3" rx="1" class="cluster__icon-slot" />
              <rect x="82" y="90" width="36" height="3" rx="1" class="cluster__icon-slot" />
              <circle cx="100" cy="97" r="2" class="cluster__icon-led" />
            </g>

            <g v-else>
              <polygon points="76,14 80,8 120,8 124,14 124,104 76,104" class="cluster__tower-body" />
              <rect x="76" y="14" width="3" height="90" class="cluster__rgb-strip" />
              <polygon points="86,20 114,20 118,26 86,26" class="cluster__icon-slot" />
              <polygon points="86,32 114,32 118,38 86,38" class="cluster__icon-slot" />
              <polygon points="86,44 114,44 118,50 86,50" class="cluster__icon-slot" />
              <rect x="84" y="60" width="32" height="8" rx="1" class="cluster__icon-drive" />
              <rect x="84" y="78" width="32" height="14" rx="2" class="cluster__icon-psu" />
              <circle cx="118" cy="97" r="3" class="cluster__icon-led" />
            </g>

            <!-- VRAM fill-bar (also inside scaled group, keeps position) -->
            <g v-if="node.online && node.gpus.length > 0">
              <rect x="80" y="95" width="40" height="6" rx="2" class="cluster__vram-fill-bg" />
              <rect
                x="80" y="95"
                :width="40 * vramUtil(node.gpus) / 100"
                height="6"
                rx="2"
                class="cluster__vram-fill"
              />
            </g>
          </g>

          <!-- === Indicator dot (left of icon, vertically centered) === -->
          <circle
            cx="46"
            cy="120"
            r="6"
            class="cluster__dot-indicator"
            :class="node.online ? 'cluster__dot-indicator--on' : 'cluster__dot-indicator--off'"
          />

          <!-- === Stat badge (right of icon, vertically centered) === -->
          <g v-if="node.online && node.gpus.length > 0" transform="translate(175, 101)">
            <rect x="0" y="0" width="72" height="38" rx="5" class="cluster__stat-bg" />
            <text x="36" y="15" text-anchor="middle" class="cluster__stat-text">{{ maxUtil(node.gpus) }}%</text>
            <text x="36" y="29" text-anchor="middle" class="cluster__stat-text">{{ maxTemp(node.gpus) }}</text>
          </g>
          <g v-else transform="translate(175, 109)">
            <rect x="0" y="0" width="72" height="22" rx="5" class="cluster__stat-bg" />
            <text x="36" y="15" text-anchor="middle" class="cluster__stat-text">Offline</text>
          </g>

          <!-- === Memory text below icon === -->
          <text x="120" y="185" text-anchor="middle" class="cluster__mem-text">{{ memText(node) }}</text>
        </g>
      </svg>

      <button
        class="cluster__add-worker"
        @click="openAddWorker"
      >
        <PlusIcon :size="14" />
        Add Worker
      </button>
    </div>

    <!-- Detailed Hardware Cards -->
    <div class="cluster__nodes">
      <div
        v-for="(node, idx) in nodeViews"
        :key="node.host"
        class="cluster__node"
      >
        <div class="cluster__node-head">
          <span
            class="cluster__dot"
            :class="node.online ? 'cluster__dot--on' : 'cluster__dot--off'"
          />
          <span class="cluster__node-name">{{ node.name }}</span>
          <span class="cluster__node-host">{{ node.host }}</span>
          <span class="cluster__node-role">{{ node.role }}</span>
          <button class="cluster__refresh" @click="refresh()">Refresh</button>
        </div>

        <template v-if="node.online">
          <!-- CPU -->
          <div v-if="node.cpu" class="cluster__hw-row">
            <span class="cluster__hw-label">CPU</span>
            <span class="cluster__hw-value">
              {{ node.cpu.name }} · {{ node.cpu.cores }} cores · {{ node.cpu.utilization.toFixed(0) }}%
            </span>
          </div>

          <!-- RAM -->
          <div v-if="node.ram" class="cluster__hw-row">
            <span class="cluster__hw-label">RAM</span>
            <span class="cluster__hw-value">
              {{ gb(node.ram.usedMb) }} / {{ gb(node.ram.totalMb) }}GB used
              ({{ gb(node.ram.freeMb) }}GB free · {{ node.ram.utilization.toFixed(0) }}%)
            </span>
          </div>

          <!-- GPUs -->
          <div v-if="node.gpus.length" class="cluster__gpus">
            <div
              v-for="gpu in node.gpus"
              :key="gpu.index"
              class="cluster__gpu"
            >
              <div class="cluster__gpu-name">GPU {{ gpu.index }}: {{ gpu.name }}</div>
              <div class="cluster__gpu-stats">
                <span>{{ gpu.utilization }}% util</span>
                <span>{{ gb(gpu.memoryUsed) }}/{{ gb(gpu.memoryTotal) }}GB</span>
                <span>{{ gpu.temperature }}°C</span>
              </div>
            </div>
          </div>
          <div v-else class="cluster__hw-row">
            <span class="cluster__hw-label">GPU</span>
            <span class="cluster__hw-value">No GPU data</span>
          </div>
          <!-- Per-node Launch RPC Slave button — only on worker nodes (non-local), not on MAMBA -->
          <div v-if="!node.local" class="cluster__node-actions">
            <button
              class="cluster__launch"
              :disabled="rpcLaunching"
              @click="launchRpcSlave"
            >
              {{ rpcLaunching ? 'Launching…' : `Launch RPC Slave on ${node.name}` }}
            </button>
          </div>
        </template>
        <div v-else class="cluster__offline">
          {{ node.error ? `Offline — ${node.error}` : 'Offline (no connection)' }}
        </div>
      </div>
    </div>
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
        <!-- Add Worker dialog. Backdrop click does NOT close the dialog:
             JC reported accidental data loss when clicking outside. The
             previous `@click.self="closeAddWorker"` paired with
             openAddWorker()'s on-every-open reset silently wiped typed-but-
             not-saved input. Only the Cancel button (footer), the X button
             (header), and the Escape key close the dialog. The form state
             is reset every time the user opens the dialog (see
             openAddWorker), so save+reopen / cancel+reopen both start
             blank — but a mid-typing click-outside keeps the dialog open
             AND preserves the partial input. -->
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
/* ── Topology page — uses global Meridian design system from vars.css
     (near-black bg, teal accent, white text, green status, thin borders).
     Only topology-specific values are kept as local variables.
   ──────────────────────────────────────────────────────────────────────── */

.cluster {
  --clr-accent-dim: hsl(8, 50%, 15%); /* dark coral fill for tower body */

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

.cluster__header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}

.cluster__title {
  font-size: 1.25rem;
  font-weight: 700;
  color: var(--foreground);
}

.cluster__summary {
  color: var(--muted-foreground);
  font-size: 0.875rem;
}

.cluster__vram-value {
  color: var(--primary);
  font-weight: 700;
}

.cluster__section-header {
  font-size: 0.7rem;
  font-weight: 700;
  color: var(--muted-foreground);
  text-transform: uppercase;
  letter-spacing: 1.5px;
  padding: 0.5rem 0 0.25rem;
  border-bottom: 1px solid var(--border);
  margin-bottom: 0.75rem;
}

.cluster__topology {
  position: relative;
  background: var(--background-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 1rem 1rem 0.5rem;
  margin-bottom: 1rem;
}

.cluster__topology-svg {
  width: 100%;
  height: min(55vh, 480px);
  display: block;
}

.cluster__conn-line {
  stroke: var(--border);
  color: var(--border);
  stroke-width: 2;
  stroke-dasharray: 5 4;
  transition: stroke 0.3s ease, opacity 0.3s ease, color 0.3s ease;
  opacity: 0.3;
}
.cluster__conn-line--active {
  stroke: var(--primary);
  color: var(--primary);
  stroke-dasharray: none;
  opacity: 1;
}

.cluster__conn-badge-bg {
  fill: var(--background-3);
  stroke: var(--primary);
  stroke-width: 1;
}

.cluster__conn-badge {
  fill: var(--muted-foreground);
  font-size: 10px;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  font-weight: 600;
  text-anchor: middle;
  dominant-baseline: central;
}

/* Arrowhead markers use fill="currentColor" in the SVG definition —
   they inherit the color of the referencing <line> element. The line's
   `color` CSS property is set via .cluster__conn-line / --active so
   the marker fill automatically changes when rpcActive toggles. */

/* Tower icon shared */
.cluster__tower-icon {
  transition: opacity 0.2s ease;
}
.cluster__tower-icon--online {
  opacity: 1;
}
.cluster__tower-icon--offline {
  opacity: 0.25;
}

.cluster__tower-body {
  fill: var(--background-3);
  stroke: var(--border);
  stroke-width: 1;
}
.cluster__tower-icon--online .cluster__tower-body {
  fill: var(--clr-accent-dim);
  stroke: var(--primary);
  stroke-width: 2;
}

/* MAMBA rack ears */
.cluster__rack-ear {
  fill: var(--background-3);
  stroke: var(--border);
  stroke-width: 0.8;
}

/* Gaming tower RGB strip — uses accent color */
.cluster__rgb-strip {
  fill: var(--clr-accent-dim);
}
.cluster__tower-icon--online .cluster__rgb-strip {
  fill: var(--primary);
}

/* Slot / drive / PSU filler — dark against near-black bg */
.cluster__icon-slot {
  fill: rgba(0, 0, 0, 0.35);
  stroke: none;
}
.cluster__icon-drive {
  fill: rgba(0, 0, 0, 0.3);
  stroke: var(--border);
  stroke-width: 0.5;
}
.cluster__icon-psu {
  fill: rgba(0, 0, 0, 0.25);
  stroke: var(--border);
  stroke-width: 0.5;
}
.cluster__icon-led {
  fill: #22c55e;
}

/* Online/offline indicator dot — fully opaque green/gray */
.cluster__dot-indicator {
  transition: fill 0.2s ease;
}
.cluster__dot-indicator--on {
  fill: #22c55e;
  stroke: none;
}
.cluster__dot-indicator--off {
  fill: #6b7280;
  stroke: none;
}

/* Node name ABOVE icon — large, bold, white (section-heading weight) */
.cluster__node-label-name {
  fill: var(--foreground);
  font-size: 20px;
  font-weight: 800;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  letter-spacing: 0.02em;
}
.cluster__node-label-host {
  fill: var(--muted-foreground);
  font-size: 12px;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
}

/* Memory text: monospace, accent color, large enough to read at a glance */
.cluster__mem-text {
  fill: var(--primary);
  font-size: 14px;
  font-weight: 700;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
  /* Fully opaque — pops against near-black bg. */
}

/* Floating stat badge — dark panel, thin subtle border */
.cluster__stat-bg {
  fill: var(--background-3);
  stroke: var(--primary);
  stroke-width: 1;
}
.cluster__stat-text {
  fill: #fff;
  font-size: 11px;
  font-weight: 600;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
}

/* VRAM fill-bar gauge — uses accent color, bold fill */
.cluster__vram-fill-bg {
  fill: rgba(0, 0, 0, 0.4);
}
.cluster__vram-fill {
  fill: var(--primary);
  transition: width 0.3s ease;
  /* Fully opaque — no blend-into-background. */
}

.cluster__add-worker {
  position: absolute;
  top: 8px;
  right: 8px;
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.75rem;
  padding: 0.25rem 0.5rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}

.cluster__nodes {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  /* The ONLY scroll container in this page. `flex: 1; min-height: 0`
     claims the leftover vertical space inside .cluster, AND
     `max-height` enforces a concrete cap so the list scrolls reliably.
     Updated for the scaled-topology era:
       page padding top (.cluster)   24px
       section header                  0px
       cluster__topology panel padding 16px
       topology SVG               min(55vh, 480px)
       margin-bottom                  16px
     At 55vh with 720px viewport = 396px. Cap = min(55vh, 480px) + ~150
     for header/gap/padding ≈ 600px overhead. Using 680 as a generous
     upper bound so even at 480px SVG height the cards have room. */
  flex: 1;
  min-height: 0;
  max-height: calc(100vh - var(--window-toolbar-height, 48px) - 680px);
  max-height: calc(100dvh - var(--window-toolbar-height, 48px) - 680px);
  overflow-y: auto;
  scrollbar-gutter: stable;
}

.cluster__node {
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  padding: 0.75rem 1rem;
  background: var(--background-2);
}

.cluster__node-head {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.cluster__dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
}

.cluster__dot--on { background: hsl(var(--success)); }
.cluster__dot--off { background: #6b7280; }

.cluster__node-name { font-weight: 700; color: var(--foreground); }
.cluster__node-host { color: var(--muted-foreground); font-size: 0.8rem; }

.cluster__node-role {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}

.cluster__hw-row {
  display: flex;
  gap: 0.75rem;
  margin-top: 0.5rem;
  font-size: 0.85rem;
  align-items: baseline;
}

.cluster__hw-label {
  flex-shrink: 0;
  width: 40px;
  font-weight: 600;
  color: var(--muted-foreground);
}

.cluster__hw-value {
  color: var(--foreground);
}

.cluster__refresh {
  margin-left: auto;
  font-size: 0.75rem;
  padding: 0.2rem 0.5rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}

.cluster__gpus {
  margin-top: 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.cluster__gpu-name { font-size: 0.85rem; color: var(--foreground); }

.cluster__gpu-stats {
  display: flex;
  gap: 1rem;
  font-size: 0.8rem;
  color: var(--muted-foreground);
}

.cluster__offline {
  margin-top: 0.5rem;
  font-size: 0.8rem;
  color: hsl(var(--destructive));
}

.cluster__node-actions {
  margin-top: 0.75rem;
  display: flex;
  align-items: center;
  gap: 1rem;
}

.cluster__launch {
  padding: 0.4rem 0.85rem;
  border-radius: var(--radius-sm);
  border: 1px solid var(--primary);
  background: var(--primary);
  color: #ffffff;
  cursor: pointer;
  font-size: 0.8rem;
  font-weight: 600;
  transition: opacity 0.15s ease;
}

.cluster__launch:disabled { opacity: 0.4; cursor: default; }

.cluster__launch:hover:not(:disabled) {
  opacity: 0.85;
}

.cluster__msg { font-size: 0.8rem; color: var(--muted-foreground); }

/* ============== Add Worker modal ============== */
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

.cluster-modal__close:hover {
  background: var(--background);
  color: var(--foreground);
}

.cluster-modal__body {
  padding: 1rem;
  overflow-y: auto;
}

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
}

.cluster-modal__btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.cluster-modal__btn--ghost {
  background: transparent;
  border-color: var(--border);
  color: var(--foreground);
}

.cluster-modal__btn--ghost:hover:not(:disabled) {
  background: var(--background);
}

.cluster-modal__btn--primary {
  background: var(--primary);
  color: #ffffff;
  border-color: var(--primary);
}

.cluster-modal__btn--primary:hover:not(:disabled) {
  opacity: 0.85;
}

.cluster-form {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.cluster-form__row {
  display: flex;
  align-items: flex-end;
  gap: 0.5rem;
}

.cluster-form__field {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.cluster-form__field--port {
  flex: 0 0 88px;
}

.cluster-form__label {
  color: var(--muted-foreground);
  font-size: 0.75rem;
}

.cluster-form__input {
  width: 100%;
  padding: 0.35rem 0.55rem;
  font-size: 0.85rem;
  background: var(--background);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  color: var(--foreground);
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.cluster-form__input:focus {
  border-color: var(--primary);
  box-shadow: 0 0 0 2px var(--primary);
}

.cluster-form__input::placeholder {
  color: var(--muted-foreground);
  opacity: 0.5;
}

.cluster-form__toggle {
  display: inline-flex;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid var(--border);
  background: var(--background);
  align-self: flex-start;
}

.cluster-form__toggle-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.35rem 0.6rem;
  font-size: 0.8rem;
  background: transparent;
  border: 0;
  color: var(--muted-foreground);
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.cluster-form__toggle-btn + .cluster-form__toggle-btn {
  border-left: 1px solid var(--border);
}

.cluster-form__toggle-btn:hover {
  background: var(--background-2);
  color: var(--foreground);
}

.cluster-form__toggle-btn--active {
  background: var(--clr-accent-dim);
  border-bottom: 2px solid var(--primary);
  color: var(--foreground);
  font-weight: 600;
}

.cluster-form__test {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding-top: 0.25rem;
  flex-wrap: wrap;
}

.cluster-form__test-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.35rem 0.65rem;
  font-size: 0.8rem;
  background: var(--background);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  color: var(--foreground);
  cursor: pointer;
}

.cluster-form__test-btn:hover:not(:disabled) {
  background: var(--background-2);
  border-color: var(--primary);
}

.cluster-form__test-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.cluster-form__test-result {
  font-size: 0.8rem;
  color: var(--muted-foreground);
}

.cluster-form__test-result--ok {
  color: hsl(var(--success));
}

.cluster-form__test-result--err {
  color: hsl(var(--destructive));
}

/* Modal transition */
.cluster-modal-enter-from,
.cluster-modal-leave-to {
  opacity: 0;
}
.cluster-modal-enter-from .cluster-modal__panel,
.cluster-modal-leave-to .cluster-modal__panel {
  transform: translateY(8px) scale(0.98);
}
.cluster-modal-enter-active,
.cluster-modal-leave-active {
  transition: opacity 0.18s ease;
}
.cluster-modal-enter-active .cluster-modal__panel,
.cluster-modal-leave-active .cluster-modal__panel {
  transition: transform 0.18s ease;
}

/* ===================== Empty state (no SSH workers) ===================== */
.cluster__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.75rem;
  padding: 2.5rem 1.5rem;
  margin: 1.5rem 0;
  text-align: center;
  background: var(--background-2);
  border: 1px dashed var(--border);
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
  color: var(--muted-foreground);
  background: var(--background-3);
  border: 2px dashed var(--border);
  border-radius: 50%;
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
}

.cluster__empty-cta:hover:not(:disabled) {
  opacity: 0.85;
}

.cluster__empty-cta:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

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
</style>
