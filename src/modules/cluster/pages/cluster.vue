<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';

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
  }

  // Other connections are remote
  conns.filter(c => c.label !== 'MAMBA').forEach(c => {
    nodes.push({
      id: c.host,
      name: c.label || c.host,
      host: c.host,
      role: 'Worker node',
      local: false,
      vendor: 'nvidia', // default, could be detected
    });
  });

  return nodes;
});

const nodeViews = ref<NodeView[]>([]);
const rpcLaunching = ref(false);
const rpcMessage = ref('');
let pollTimer: ReturnType<typeof setInterval> | null = null;

async function refreshNodeViews() {
  const views: NodeView[] = [];
  for (const def of nodeDefs.value) {
    // Use stored credentials from the SSH connection
    const conn = sshConnections.value?.find(c => c.host === def.host);
    const creds = {
      host: def.host,
      port: conn?.port ?? 22,
      username: conn?.username ?? 'jatilq',
      keyPath: conn?.keyPath ?? 'C:\\Users\\jatilq\\.ssh\\meridian_black',
    };

    try {
      const snap = def.local
        ? await invoke<HardwareSnapshot>('get_local_hardware')
        : await invoke<HardwareSnapshot>('get_remote_hardware', { creds });
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
        creds: {
          host: blackConn.host,
          port: blackConn.port,
          username: blackConn.username,
          keyPath: blackConn.keyPath,
        },
        rpcCommand: 'llama-server --rpc 0.0.0.0:50052',
      });
      rpcMessage.value = out || 'RPC slave launch requested.';
    } catch (error) {
      rpcMessage.value = `Failed: ${error}`;
    }
  }
  rpcLaunching.value = false;
}

function openSettings() {
  // Navigate to settings SSH connections panel
  window.dispatchEvent(new CustomEvent('open-settings-ssh'));
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
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
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
        @click="openSettings"
      >
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
</style>

