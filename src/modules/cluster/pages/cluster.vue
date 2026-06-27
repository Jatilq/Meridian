<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';

interface NodeStatus {
  name: string;
  host: string;
  online: boolean;
  gpus: GpuStat[];
  error?: string;
}

interface GpuStat {
  index: number;
  name: string;
  utilization: number; // percent
  memoryUsed: number;   // MB
  memoryTotal: number;  // MB
  temperature: number;  // celsius
}

// Static node definitions (IPs configurable in Settings → Cluster later).
const NODES = [
  { name: 'MAMBA', host: '192.168.1.67', role: 'Primary inference (3× RTX 3060)' },
  { name: 'BLACK', host: '192.168.1.64', role: 'Daily driver / RPC slave (RX 6900 XT)' },
];

const nodes = ref<NodeStatus[]>(
  NODES.map(n => ({ name: n.name, host: n.host, online: false, gpus: [] })),
);
const rpcLaunching = ref(false);
const rpcMessage = ref('');
let pollTimer: ReturnType<typeof setInterval> | null = null;

async function refreshNode(idx: number) {
  const def = NODES[idx];
  try {
    // Rust command (Phase 6 backend, pending): returns { online, gpus }.
    const result = await invoke<{ online: boolean; gpus: GpuStat[] }>(
      'check_node_status',
      { host: def.host },
    );
    nodes.value[idx] = {
      name: def.name,
      host: def.host,
      online: result.online,
      gpus: result.gpus ?? [],
    };
  }
  catch (error) {
    nodes.value[idx] = {
      name: def.name,
      host: def.host,
      online: false,
      gpus: [],
      error: String(error),
    };
  }
}

async function refreshAll() {
  await Promise.all(NODES.map((_, i) => refreshNode(i)));
}

async function launchRpcSlave() {
  rpcLaunching.value = true;
  rpcMessage.value = '';
  try {
    const out = await invoke<string>('launch_rpc_slave', { host: '192.168.1.64' });
    rpcMessage.value = out || 'RPC slave launch requested.';
  }
  catch (error) {
    rpcMessage.value = `Failed: ${error}`;
  }
  finally {
    rpcLaunching.value = false;
  }
}

function combinedVram(): string {
  const totalMb = nodes.value
    .flatMap(n => n.gpus)
    .reduce((sum, g) => sum + (g.memoryTotal || 0), 0);
  return totalMb > 0 ? `${(totalMb / 1024).toFixed(0)}GB` : '—';
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
        Combined VRAM: <strong>{{ combinedVram() }}</strong>
      </div>
    </div>

    <div class="cluster__nodes">
      <div
        v-for="(node, idx) in nodes"
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
          <button class="cluster__refresh" @click="refreshNode(idx)">Refresh</button>
        </div>

        <div v-if="node.online && node.gpus.length" class="cluster__gpus">
          <div
            v-for="gpu in node.gpus"
            :key="gpu.index"
            class="cluster__gpu"
          >
            <div class="cluster__gpu-name">GPU {{ gpu.index }}: {{ gpu.name }}</div>
            <div class="cluster__gpu-stats">
              <span>{{ gpu.utilization }}% util</span>
              <span>{{ (gpu.memoryUsed / 1024).toFixed(1) }}/{{ (gpu.memoryTotal / 1024).toFixed(1) }}GB</span>
              <span>{{ gpu.temperature }}°C</span>
            </div>
          </div>
        </div>
        <div v-else class="cluster__offline">
          {{ node.error ? `Offline — ${node.error}` : 'Offline (no SSH connection)' }}
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

