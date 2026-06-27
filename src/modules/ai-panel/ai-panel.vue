<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, watch, nextTick, ref, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useAiPanelStore } from '@/stores/runtime/ai-panel';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Switch } from '@/components/ui/switch';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  BotIcon,
  SendIcon,
  LoaderCircleIcon,
  CheckIcon,
  XIcon,
} from '@lucide/vue';
import { invoke } from '@tauri-apps/api/core';
import { toast } from '@/components/ui/toaster';

const { t } = useI18n();
const aiPanelStore = useAiPanelStore();
const userSettingsStore = useUserSettingsStore();

const resultAreaRef = ref<InstanceType<typeof ScrollArea> | null>(null);
const confirmDialogOpen = ref(false);
const confirmDialogData = ref<{ title: string; description: string; onConfirm: () => void | Promise<void> } | null>(null);

watch(
  () => aiPanelStore.isOpen,
  (open) => {
    if (open && !aiPanelStore.modelsLoaded && !aiPanelStore.useOmnix) {
      void aiPanelStore.fetchModels();
    }
    if (open && aiPanelStore.useOmnix) {
      void checkOmnixStatus();
    }
  },
);

watch(
  () => aiPanelStore.messages.length,
  async () => {
    await nextTick();
    const viewport = resultAreaRef.value?.$el?.querySelector(
      '.sigma-ui-scroll-area__viewport',
    ) as HTMLElement | undefined;
    viewport?.scrollTo({ top: viewport.scrollHeight, behavior: 'smooth' });
  },
);

async function maybeSpeak(text: string) {
  // Optional TTS: speak the assistant response via Omnix Kokoro when enabled
  // and Omnix is online. Plays the returned float audio through Web Audio.
  if (!aiPanelStore.ttsEnabled || !aiPanelStore.omnixOnline) return;
  try {
    const raw = await invoke<string>('omnix_tts', { text, voiceId: null });
    const data = JSON.parse(raw);
    const samples: number[] = data.audio || [];
    const rate = Number(data.sampling_rate) || 24000;
    if (!samples.length) return;
    const AudioCtx = (window as unknown as { AudioContext?: typeof AudioContext; webkitAudioContext?: typeof AudioContext }).AudioContext
      || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioCtx) return;
    const ctx = new AudioCtx();
    const buffer = ctx.createBuffer(1, samples.length, rate);
    buffer.copyToChannel(Float32Array.from(samples), 0);
    const src = ctx.createBufferSource();
    src.buffer = buffer;
    src.connect(ctx.destination);
    src.start();
  }
  catch {
    // TTS is best-effort; ignore failures
  }
}

async function handleSend() {
  const prompt = aiPanelStore.input.trim();
  if (!prompt || aiPanelStore.isLoading) return;

  aiPanelStore.addMessage('user', prompt);
  aiPanelStore.setInput('');
  aiPanelStore.setLoading(true);

  try {
    // Revised architecture: Omnix handles ONLY vision (+ TTS/Director).
    // ALL text inference goes to 9Router (OpenAI-compatible). Vision is used
    // when an image is selected and Omnix is online; everything else is text.
    const omnixVisionReady = aiPanelStore.useOmnix && aiPanelStore.omnixOnline;
    const routerBase = (aiPanelStore.routerEndpoint || '').replace(/\/+$/, '');
    const model = aiPanelStore.selectedModel || undefined;
    const currentPath = aiPanelStore.currentPath;
    const selectedFiles = aiPanelStore.selectedFiles;
    const hasImage = selectedFiles.some((file: string) => {
      const ext = file.split('.').pop()?.toLowerCase();
      return ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp'].includes(ext || '');
    });

    const systemPrompt = `You are a file management assistant inside Meridian.\nCurrent directory: ${currentPath}\nSelected files: ${selectedFiles.length > 0 ? selectedFiles.join(', ') : 'none'}\n\nRespond ONLY with valid JSON:\n{\n  "intent": "search|organize|analyze|rename|chat|vision",\n  "scope": "current|selected|all",\n  "preview_only": true,\n  "action": {},\n  "message": "human readable explanation"\n}`;

    let response: Response;
    if (omnixVisionReady && hasImage) {
      // Vision: send the image file to Omnix as multipart via the Rust command
      // (the /api/vision contract requires multipart/form-data, not JSON).
      const imageFile = selectedFiles.find((file: string) => {
        const ext = file.split('.').pop()?.toLowerCase();
        return ['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp'].includes(ext || '');
      });
      const visionText = await invoke<string>('omnix_vision', {
        imagePath: imageFile,
        prompt: `${systemPrompt}\n\nUser: ${prompt}`,
      });
      aiPanelStore.addMessage('assistant', visionText);
      await maybeSpeak(visionText);
      try {
        const parsed = JSON.parse(visionText);
        if (parsed.intent && ['organize', 'rename', 'delete'].includes(parsed.intent)) {
          handleIntentConfirmation(parsed);
        }
      }
      catch {
        // response was not JSON, leave as plain text
      }
      aiPanelStore.setLoading(false);
      return;
    }
    else {
      // Text inference -> 9Router (OpenAI-compatible chat completions).
      if (!routerBase) {
        throw new Error('9Router endpoint not configured. Set it in Settings.');
      }
      response = await fetch(`${routerBase}/v1/chat/completions`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...(model ? { 'X-Model-Id': model } : {}),
        },
        body: JSON.stringify({
          model: model || 'default',
          messages: [
            { role: 'system', content: systemPrompt },
            ...aiPanelStore.messages.map((m: { role: 'user' | 'assistant'; content: string }) => ({ role: m.role, content: m.content })),
            { role: 'user', content: prompt },
          ],
          temperature: 0.7,
          max_tokens: 1024,
        }),
      });
    }

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }

    const data = await response.json();
    const content = data.choices?.[0]?.message?.content ?? data.response ?? data.text ?? 'No response received.';
    aiPanelStore.addMessage('assistant', content);
    await maybeSpeak(content);

    try {
      const parsed = JSON.parse(content);
      if (parsed.intent && ['organize', 'rename', 'delete'].includes(parsed.intent)) {
        handleIntentConfirmation(parsed);
      }
    }
    catch {
      // response was not JSON, leave as plain text
    }
  }
  catch (error) {
    const message = error instanceof Error ? error.message : 'Unknown error occurred';
    aiPanelStore.addMessage('assistant', `Error: ${message}`);
  }
  finally {
    aiPanelStore.setLoading(false);
  }
}

function showConfirm(title: string, description: string, onConfirm: () => void | Promise<void>) {
  confirmDialogData.value = { title, description, onConfirm };
  confirmDialogOpen.value = true;
}

async function confirmAction() {
  if (!confirmDialogData.value) return;
  const { onConfirm } = confirmDialogData.value;
  confirmDialogOpen.value = false;
  await onConfirm();
  confirmDialogData.value = null;
}

function cancelAction() {
  confirmDialogOpen.value = false;
  confirmDialogData.value = null;
}

async function handleIntentConfirmation(intent: { intent: string; scope?: string; action: Record<string, unknown>; message: string }) {
  const action = intent.action || {};
  switch (intent.intent) {
    case 'organize': {
      const criteria = (action.criteria as string) || 'type';
      showConfirm(
        t('aiPanel.confirmOrganizeTitle'),
        t('aiPanel.confirmOrganizeDescription', { criteria }),
        async () => {
          const files = aiPanelStore.selectedFiles.length > 0
            ? aiPanelStore.selectedFiles
            : [];
          if (files.length === 0) return;
          await invoke('move_items', { files, destination: '' });
          toast({ title: t('aiPanel.organized'), description: criteria });
        },
      );
      break;
    }
    case 'rename': {
      const pattern = (action.pattern as string) || '';
      showConfirm(
        t('aiPanel.confirmRenameTitle'),
        t('aiPanel.confirmRenameDescription', { pattern }),
        async () => {
          const files = aiPanelStore.selectedFiles;
          for (const file of files) {
            await invoke('rename_item', { path: file, name: pattern });
          }
          toast({ title: t('aiPanel.renamed'), description: pattern });
        },
      );
      break;
    }
    case 'delete': {
      const count = aiPanelStore.selectedFiles.length || intent.scope;
      showConfirm(
        t('aiPanel.confirmDeleteTitle'),
        t('aiPanel.confirmDeleteDescription', { count }),
        async () => {
          const files = aiPanelStore.selectedFiles;
          if (files.length === 0) return;
          await invoke('delete_items', { files });
          toast({ title: t('aiPanel.deleted') });
        },
      );
      break;
    }
    default:
      break;
  }
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault();
    handleSend();
  }
}

async function checkOmnixStatus() {
  if (aiPanelStore.useOmnix) {
    try {
      const online = await invoke<boolean>('get_omnix_status');
      aiPanelStore.setOmnixOnline(online);
    }
    catch {
      aiPanelStore.setOmnixOnline(false);
    }
  }
  // 9Router health (text inference backend) — probe /v1/models.
  const routerBase = (aiPanelStore.routerEndpoint || '').replace(/\/+$/, '');
  if (routerBase) {
    try {
      const res = await fetch(`${routerBase}/v1/models`, { method: 'GET' });
      aiPanelStore.setRouterOnline(res.ok);
    }
    catch {
      aiPanelStore.setRouterOnline(false);
    }
  }
  else {
    aiPanelStore.setRouterOnline(false);
  }
}

async function spawnOmnix() {
  try {
    await invoke('spawn_omnix', { omnixPath: aiPanelStore.omnixPath || null });
  }
  catch {
    // ignore spawn errors
  }
}

watch(
  () => aiPanelStore.useOmnix,
  async (enabled) => {
    if (enabled) {
      await spawnOmnix();
      await checkOmnixStatus();
    }
    else {
      aiPanelStore.setOmnixOnline(false);
      await invoke('kill_omnix');
    }
  },
);

const omnixStatusLabel = computed(() => {
  if (!aiPanelStore.useOmnix) return '';
  return aiPanelStore.omnixOnline ? 'Omnix online' : 'Omnix offline';
});

// Poll Omnix health on an interval so the status dot stays live (Step 6).
let omnixPollTimer: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  void checkOmnixStatus();
  void aiPanelStore.fetchModels();
  omnixPollTimer = setInterval(() => {
    void checkOmnixStatus();
  }, 5000);
});

onUnmounted(() => {
  if (omnixPollTimer !== null) {
    clearInterval(omnixPollTimer);
    omnixPollTimer = null;
  }
});

const confirmTitle = computed(() => confirmDialogData.value?.title || '');
const confirmDescription = computed(() => confirmDialogData.value?.description || '');
</script>

<template>
  <div class="ai-panel">
    <div class="ai-panel__header">
      <span class="ai-panel__title">{{ t('aiPanel.title') }}</span>
      <div class="ai-panel__header-actions">
        <span
          v-if="aiPanelStore.useOmnix && omnixStatusLabel"
          class="ai-panel__omnix-status"
          :class="{
            'ai-panel__omnix-status--online': aiPanelStore.omnixOnline,
            'ai-panel__omnix-status--offline': !aiPanelStore.omnixOnline,
          }"
        />
        <Switch
          :model-value="aiPanelStore.useOmnix"
          @update:model-value="aiPanelStore.setUseOmnix"
        />
      </div>
    </div>
    <div class="ai-panel__controls">
      <Input
        v-model="aiPanelStore.endpoint"
        :placeholder="t('aiPanel.endpointPlaceholder')"
        class="ai-panel__endpoint-input"
        :disabled="aiPanelStore.useOmnix"
      />
      <select
        :value="aiPanelStore.selectedModel"
        @change="aiPanelStore.setSelectedModel(($event.target as HTMLSelectElement).value)"
        class="ai-panel__model-select"
        :disabled="aiPanelStore.useOmnix"
      >
        <option value="">{{ t('aiPanel.defaultModel') }}</option>
        <option
          v-for="model in aiPanelStore.models"
          :key="model.id"
          :value="model.id"
        >
          {{ model.id }}
        </option>
      </select>
    </div>
    <ScrollArea ref="resultAreaRef" class="ai-panel__results">
      <div class="ai-panel__results-content">
        <div
          v-for="(msg, index) in aiPanelStore.messages"
          :key="index"
          class="ai-panel__message"
          :class="{
            'ai-panel__message--user': msg.role === 'user',
            'ai-panel__message--assistant': msg.role === 'assistant',
          }"
        >
          <div class="ai-panel__message-role">
            {{ msg.role === 'user' ? t('aiPanel.you') : t('aiPanel.assistant') }}
          </div>
          <div class="ai-panel__message-content">{{ msg.content }}</div>
        </div>
        <div v-if="aiPanelStore.messages.length === 0" class="ai-panel__placeholder">
          {{ t('aiPanel.placeholder') }}
        </div>
      </div>
    </ScrollArea>
    <div class="ai-panel__input-row">
      <Input
        v-model="aiPanelStore.input"
        :placeholder="t('aiPanel.inputPlaceholder')"
        class="ai-panel__input"
        :disabled="aiPanelStore.isLoading"
        @keydown="handleKeyDown"
      />
      <Button
        variant="secondary"
        size="icon"
        class="ai-panel__send"
        :disabled="!aiPanelStore.canSend"
        @click="handleSend"
      >
        <SendIcon v-if="!aiPanelStore.isLoading" :size="16" />
        <LoaderCircleIcon v-else :size="16" class="animate-spin" />
      </Button>
    </div>

    <Dialog :open="confirmDialogOpen" @update:open="confirmDialogOpen = $event">
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{{ confirmTitle }}</DialogTitle>
          <DialogDescription>{{ confirmDescription }}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="ghost" @click="cancelAction">
            <XIcon :size="16" />
            {{ t('common.cancel') }}
          </Button>
          <Button @click="confirmAction">
            <CheckIcon :size="16" />
            {{ t('common.confirm') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

<style scoped>
.ai-panel {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  width: 100%;
  min-width: 0;
  height: 100%;
  background-color: hsl(var(--background-3));
  border-radius: var(--radius-sm);
}

.ai-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border-bottom: 1px solid hsl(var(--border));
  flex-shrink: 0;
}

.ai-panel__title {
  color: hsl(var(--muted-foreground));
  font-size: 11px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.02em;
}

.ai-panel__header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.ai-panel__omnix-status {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.ai-panel__omnix-status--online {
  background-color: hsl(var(--success));
}

.ai-panel__omnix-status--offline {
  background-color: hsl(var(--muted-foreground));
}

.ai-panel__controls {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 10px;
  border-bottom: 1px solid hsl(var(--border) / 50%);
  flex-shrink: 0;
}

.ai-panel__endpoint-input {
  width: 100%;
}

.ai-panel__model-select {
  width: 100%;
  padding: 4px 8px;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  font-size: 12px;
  outline: none;
}

.ai-panel__model-select:focus {
  border-color: hsl(var(--ring));
}

.ai-panel__results {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.ai-panel__results-content {
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ai-panel__placeholder {
  color: hsl(var(--muted-foreground));
  font-size: 12px;
  text-align: center;
  padding: 24px 12px;
}

.ai-panel__message {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-width: 100%;
}

.ai-panel__message--user {
  align-items: flex-end;
}

.ai-panel__message--assistant {
  align-items: flex-start;
}

.ai-panel__message-role {
  font-size: 10px;
  font-weight: 500;
  text-transform: uppercase;
  color: hsl(var(--muted-foreground));
  letter-spacing: 0.03em;
}

.ai-panel__message-content {
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  line-height: 1.45;
  word-break: break-word;
  max-width: 95%;
}

.ai-panel__message--user .ai-panel__message-content {
  background-color: hsl(var(--primary) / 15%);
  color: hsl(var(--foreground));
}

.ai-panel__message--assistant .ai-panel__message-content {
  background-color: hsl(var(--secondary));
  color: hsl(var(--foreground));
}

.ai-panel__input-row {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  border-top: 1px solid hsl(var(--border));
  flex-shrink: 0;
}

.ai-panel__input {
  flex: 1;
  min-width: 0;
}

.ai-panel__send {
  flex-shrink: 0;
  width: 32px;
  height: 32px;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.animate-spin {
  animation: spin 1s linear infinite;
}
</style>
