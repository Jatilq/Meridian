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

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();
const sshConnections = computed(() => userSettingsStore.userSettings.meridian?.sshConnections ?? []);

interface GpuStat {
  index: number;
  name: string;
  utilization: number; // percent
  memoryUsed: number;   // MB
  memoryTotal: number;  // MB
  temperature: number;  // celsius
}

interface CpuInfo {
  name: string;
  cores: number;
  utilization: number; // percent
}

interface RamInfo {
  totalMb: number;
  usedMb: number;
  freeMb: number;
  utilization: number; // percent
}

interface HardwareSnapshot {
  online: boolean;
  cpu: CpuInfo | null;
  ram: RamInfo | null;
  gpus: GpuStat[];
  error: string | null;
}

interface NodeView extends HardwareSnapshot {
  name: string;
  host: string;
  role: string;
}

// Node definition for the topology map
interface NodeDef {
  id: string;
  name: string;
  host: string;
  role: string;
  local: boolean;
}

// Build node list from SSH connections + mark local status
const nodeDefs = computed<NodeDef[]>(() => {
  const conns = sshConnections.value || [];
  // MAMBA is special - it's local (where Meridian runs)
  const mambaConn = conns.find(c => c.label === 'MAMBA');
  const nodes: NodeDef[] = [];

  if (mambaConn) {
    nodes.push({
      id: mambaConn.host,
      name: mambaConn.label || mambaConn.host,
      host: mambaConn.host,
      role: 'Primary inference',
      local: true,
    });
  }

  // Other connections are remote
  conns.filter(c => c.label !== 'MAMBA').forEach(c => {
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

const nodeViews = ref<NodeView[]>([]);
const rpcLaunching = ref(false);
const rpcMessage = ref('');
let pollTimer: ReturnType<typeof setInterval> | null = null;

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

async function refreshNodeViews() {
  const views: NodeView[] = [];
  for (const def of nodeDefs.value) {
    // Use stored credentials from the SSH connection
    const conn = sshConnections.value?.find(c => c.host === def.host);

    try {
      const snap = def.local
        ? await invoke<HardwareSnapshot>('get_local_hardware')
        : await invoke<HardwareSnapshot>('get_remote_hardware', { creds: credsFromConn(conn) });
      views.push({
        name: def.name,
        host: def.host,
        role: def.role,
        ...snap,
      });
    } catch (error) {
      views.push({
        name: def.name,
        host: def.host,
        role: def.role,
        online: false,
        cpu: null,
        ram: null,
        gpus: [],
        error: String(error),
      });
    }
  }
  nodeViews.value = views;
}

async function refreshAll() {
  await refreshNodeViews();
}

async function launchRpcSlave() {
  rpcLaunching.value = true;
  rpcMessage.value = '';
  const blackConn = sshConnections.value?.find(c => c.label === 'BLACK');
  if (blackConn) {
    try {
      const out = await invoke<string>('launch_rpc_slave', {
        creds: credsFromConn(blackConn),
        rpcCommand: 'llama-server --rpc 0.0.0.0:50052',
      });
      rpcMessage.value = out || 'RPC slave launch requested.';
    } catch (error) {
      rpcMessage.value = `Failed: ${error}`;
    }
  }
  rpcLaunching.value = false;
}

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
    userSettingsStore.userSettings.meridian.sshConnections.push(conn);
    await userSettingsStore.setUserSettingsStorage(
      'meridian.sshConnections',
      userSettingsStore.userSettings.meridian.sshConnections,
    );
    showAddWorker.value = false;
    await refreshAll();
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

onMounted(() => {
  void refreshAll();
  pollTimer = setInterval(() => void refreshAll(), 30000);
  window.addEventListener('keydown', onDialogKeydown);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
  window.removeEventListener('keydown', onDialogKeydown);
});
</script>

<template>
  <div class="cluster">
    <div class="cluster__header">
      <h1 class="cluster__title">Cluster Control</h1>
      <div class="cluster__summary">
        Combined VRAM: <strong class="cluster__vram-value">{{ combinedVram }}</strong>
      </div>
    </div>

    <!-- Topology Map -->
    <div class="cluster__topology">
      <svg
        viewBox="0 0 600 200"
        class="cluster__topology-svg"
        xmlns="http://www.w3.org/2000/svg"
      >
        <!-- Connection line -->
        <line
          v-if="nodeViews.length > 1"
          x1="150"
          y1="100"
          :x2="150 + (nodeViews.length - 1) * 180"
          y2="100"
          class="cluster__connection-line"
        />
        <text
          v-if="nodeViews.length > 1"
          x="250"
          y="90"
          class="cluster__connection-label"
        >
          LAN
        </text>

        <!-- Node cards -->
        <g
          v-for="(node, idx) in nodeViews"
          :key="node.host"
        >
          <rect
            :x="idx * 180 + 20"
            y="40"
            width="140"
            height="120"
            rx="6"
            class="cluster__node-svg-card"
            :class="{
              'cluster__node-svg-card--online': node.online,
              'cluster__node-svg-card--offline': !node.online,
            }"
          />
          <circle
            :cx="idx * 180 + 30"
            cy="55"
            r="5"
            class="cluster__dot-svg"
            :class="node.online ? 'cluster__dot-svg--on' : 'cluster__dot-svg--off'"
          />
          <text
            :x="idx * 180 + 40"
            y="58"
            class="cluster__node-name-svg"
          >
            {{ node.name }}
          </text>
          <text
            :x="idx * 180 + 40"
            y="78"
            class="cluster__node-host-svg"
          >
            {{ node.host }}
          </text>
          <text
            :x="idx * 180 + 40"
            y="98"
            class="cluster__node-gpu-svg"
          >
            {{ node.gpus.length > 0 ? node.gpus[0].name : 'No GPU' }}
          </text>
          <text
            :x="idx * 180 + 40"
            y="118"
            class="cluster__node-vram-svg"
          >
            {{ node.gpus.length > 0 ? `${gb(node.gpus[0].memoryUsed)}/${gb(node.gpus[0].memoryTotal)}GB` : '—' }}
          </text>
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
          <button class="cluster__refresh" @click="refreshAll()">Refresh</button>
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
        </template>
        <div v-else class="cluster__offline">
          {{ node.error ? `Offline — ${node.error}` : 'Offline (no connection)' }}
        </div>
      </div>
    </div>

    <div class="cluster__actions">
      <button
        class="cluster__launch"
        :disabled="rpcLaunching"
        @click="launchRpcSlave"
      >
        {{ rpcLaunching ? 'Launching…' : 'Launch RPC Slave on BLACK' }}
      </button>
      <span v-if="rpcMessage" class="cluster__msg">{{ rpcMessage }}</span>
    </div>

    <!-- Add Worker dialog -->
    <Teleport to="body">
      <Transition name="cluster-modal">
        <div
          v-if="showAddWorker"
          class="cluster-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="cluster-modal-title"
          @click.self="closeAddWorker"
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
.cluster {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 1.5rem;
  overflow-y: auto;
  height: 100%;
}

.cluster__header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}

.cluster__title {
  font-size: 1.25rem;
  font-weight: 600;
  color: hsl(var(--foreground));
}

.cluster__summary {
  color: hsl(var(--muted-foreground));
  font-size: 0.875rem;
}

.cluster__vram-value {
  color: #c9a84c;
  font-weight: 600;
}

.cluster__topology {
  margin-bottom: 1rem;
  position: relative;
}

.cluster__topology-svg {
  width: 100%;
  height: 160px;
  background: #1e1e1e;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
}

.cluster__connection-line {
  stroke: hsl(var(--primary) / 40%);
  stroke-width: 2;
  stroke-dasharray: 4 2;
}

.cluster__connection-label {
  fill: hsl(var(--muted-foreground));
  font-size: 10px;
  text-anchor: middle;
}

.cluster__node-svg-card {
  fill: hsl(var(--background-2));
  stroke: hsl(var(--border));
  stroke-width: 1;
}

.cluster__node-svg-card--online {
  stroke: #c9a84c;
  stroke-width: 1.5;
}

.cluster__dot-svg {
  fill: #6b7280;
}

.cluster__dot-svg--on {
  fill: #34d399;
}

.cluster__node-name-svg {
  fill: hsl(var(--foreground));
  font-size: 11px;
  font-weight: 600;
}

.cluster__node-host-svg {
  fill: hsl(var(--muted-foreground));
  font-size: 10px;
}

.cluster__node-gpu-svg {
  fill: hsl(var(--muted-foreground));
  font-size: 10px;
}

.cluster__node-vram-svg {
  fill: hsl(var(--foreground));
  font-size: 10px;
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
  border: 1px solid hsl(var(--primary));
  background: hsl(var(--primary) / 10%);
  color: hsl(var(--foreground));
  cursor: pointer;
}

.cluster__nodes {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.cluster__node {
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  padding: 0.75rem 1rem;
  background: hsl(var(--background-2));
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

.cluster__dot--on { background: #34d399; }
.cluster__dot--off { background: #6b7280; }

.cluster__node-name { font-weight: 600; color: hsl(var(--foreground)); }
.cluster__node-host { color: hsl(var(--muted-foreground)); font-size: 0.8rem; }

.cluster__node-role {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
  font-style: italic;
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
  color: hsl(var(--muted-foreground));
}

.cluster__hw-value {
  color: hsl(var(--foreground));
}

.cluster__refresh {
  margin-left: auto;
  font-size: 0.75rem;
  padding: 0.2rem 0.5rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
  background: transparent;
  color: hsl(var(--foreground));
  cursor: pointer;
}

.cluster__gpus {
  margin-top: 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.cluster__gpu-name { font-size: 0.85rem; color: hsl(var(--foreground)); }

.cluster__gpu-stats {
  display: flex;
  gap: 1rem;
  font-size: 0.8rem;
  color: hsl(var(--muted-foreground));
}

.cluster__offline {
  margin-top: 0.5rem;
  font-size: 0.8rem;
  color: hsl(var(--muted-foreground));
}

.cluster__actions {
  display: flex;
  align-items: center;
  gap: 1rem;
}

.cluster__launch {
  padding: 0.5rem 1rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--primary));
  background: hsl(var(--primary) / 10%);
  color: hsl(var(--foreground));
  cursor: pointer;
}

.cluster__launch:disabled { opacity: 0.5; cursor: default; }

.cluster__msg { font-size: 0.8rem; color: hsl(var(--muted-foreground)); }

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
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-md, 8px);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
  color: hsl(var(--foreground));
  overflow: hidden;
}

.cluster-modal__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.875rem 1rem;
  border-bottom: 1px solid hsl(var(--border));
  background: hsl(var(--background));
}

.cluster-modal__title {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 1rem;
  font-weight: 600;
  margin: 0;
  color: hsl(var(--foreground));
}

.cluster-modal__close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 0;
  padding: 0.25rem;
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  border-radius: var(--radius-sm);
}

.cluster-modal__close:hover {
  background: hsl(var(--background));
  color: hsl(var(--foreground));
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
  border-top: 1px solid hsl(var(--border));
  background: hsl(var(--background));
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
  opacity: 0.5;
  cursor: not-allowed;
}

.cluster-modal__btn--ghost {
  background: transparent;
  border-color: hsl(var(--border));
  color: hsl(var(--foreground));
}

.cluster-modal__btn--ghost:hover:not(:disabled) {
  background: hsl(var(--background));
}

.cluster-modal__btn--primary {
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground, hsl(var(--background))));
  border-color: hsl(var(--primary));
}

.cluster-modal__btn--primary:hover:not(:disabled) {
  filter: brightness(1.1);
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
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
}

.cluster-form__input {
  width: 100%;
  padding: 0.35rem 0.55rem;
  font-size: 0.85rem;
  background: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  color: hsl(var(--foreground));
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.cluster-form__input:focus {
  border-color: hsl(var(--primary));
  box-shadow: 0 0 0 3px hsl(var(--primary) / 20%);
}

.cluster-form__input::placeholder {
  color: hsl(var(--muted-foreground));
  opacity: 0.65;
}

.cluster-form__toggle {
  display: inline-flex;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background));
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
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  transition: background 0.15s ease, color 0.15s ease;
}

.cluster-form__toggle-btn + .cluster-form__toggle-btn {
  border-left: 1px solid hsl(var(--border));
}

.cluster-form__toggle-btn:hover {
  background: hsl(var(--background-2));
  color: hsl(var(--foreground));
}

.cluster-form__toggle-btn--active {
  background: hsl(var(--primary) / 18%);
  color: hsl(var(--foreground));
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
  background: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  color: hsl(var(--foreground));
  cursor: pointer;
}

.cluster-form__test-btn:hover:not(:disabled) {
  background: hsl(var(--background-2));
  border-color: hsl(var(--primary) / 40%);
}

.cluster-form__test-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.cluster-form__test-result {
  font-size: 0.8rem;
  color: hsl(var(--muted-foreground));
}

.cluster-form__test-result--ok {
  color: #34d399;
}

.cluster-form__test-result--err {
  color: #f87171;
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
</style>
