<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';

interface ModelInfo {
  id: string;
  name: string;
  sizeGb: number;
  quant: string;
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

async function searchModels() {
  loadingModels.value = true;
  try {
    const vram = combinedVramGb.value;
    const response = await fetch(`https://huggingface.co/api/models?sort=downloads&limit=20`, {
      method: 'GET',
      headers: { Accept: 'application/json' },
    });
    if (!response.ok) {
      models.value = [];
      return;
    }
    const data = await response.json();
    const results: ModelInfo[] = (data || [])
      .filter((m: Record<string, unknown>) => {
        const tags = (m.tags as string[] || []).join(' ').toLowerCase();
        const likes = (m.likes as number) || 0;
        return likes > 10 && (tags.includes('gguf') || tags.includes('q4'));
      })
      .map((m: Record<string, unknown>) => ({
        id: m.id as string,
        name: (m.id as string).split('/')[1] || m.id as string,
        sizeGb: 7,
        quant: 'Q4_K_M',
      }));
    models.value = results.slice(0, 10);
  } catch {
    models.value = [];
  }
  loadingModels.value = false;
}

async function downloadModel(model: ModelInfo) {
  await invoke('downloader_enqueue', {
    url: `https://huggingface.co/${model.id}/resolve/main/`,
    file_name: `${model.name}.gguf`,
    format_id: null,
    auto_save_folder: 'E:\\ai\\Models',
    chunk_count: null,
  });
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
        {{ loadingModels ? 'Searching...' : 'Search Models' }}
      </button>
    </div>

    <div class="hardware__models">
      <div
        v-for="model in models"
        :key="model.id"
        class="hardware__model"
      >
        <div class="hardware__model-info">
          <div class="hardware__model-name">{{ model.name }}</div>
          <div class="hardware__model-meta">{{ model.sizeGb }}GB · {{ model.quant }}</div>
        </div>
        <button @click="downloadModel(model)">Download</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.hardware {
  padding: 1.5rem;
  height: 100%;
  overflow-y: auto;
}
.hardware__header {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  margin-bottom: 1rem;
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
.hardware__models {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.hardware__model {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.75rem;
  background: hsl(var(--background-2));
  border-radius: var(--radius-sm);
}
.hardware__model-name {
  font-weight: 500;
}
.hardware__model-meta {
  font-size: 0.85rem;
  color: hsl(var(--muted-foreground));
}
</style>