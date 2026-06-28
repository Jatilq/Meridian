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

// Drives available as explicit search-scope targets (loaded on demand).
const scopeDrives = ref<string[]>([]);

async function loadScopeDrives() {
  try {
    const drives = await invoke<Array<{ path: string }>>('get_system_drives');
    scopeDrives.value = drives.map(d => d.path).filter(Boolean);
  }
  catch (error) {
    console.error('Failed to load drives for search scope:', error);
  }
}

function handleScopeChange(value: string) {
  aiPanelStore.setSearchScope(value);
}

const resultAreaRef = ref<InstanceType<typeof ScrollArea> | null>(null);
const confirmDialogOpen = ref(false);
const confirmDialogData = ref<{ title: string; description: string; onConfirm: () => void | Promise<void> } | null>(null);
// Pending resolver for an in-flight tool confirmation (Rain agent loop).
let toolConfirmResolve: ((confirmed: boolean) => void) | null = null;
// Structured details for the in-panel confirmation card (Step 4).
const toolConfirmDetails = ref<{
  tool: string;
  title: string;
  lines: string[];
  warning?: string;
} | null>(null);

watch(
  () => aiPanelStore.isOpen,
  (open) => {
    if (open && !aiPanelStore.modelsLoaded) {
      void aiPanelStore.fetchModels();
    }
    if (open) {
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
// Rain agent loop: POST to 9Router with tool schemas; while the model returns
// tool_calls, execute them (read-only/create immediately, destructive ones via
// confirmation) and feed results back. Caps at 10 iterations. Returns the final
// assistant text. `pendingDestructive` collects tool calls needing confirmation.
async function runAgentLoop(
  routerBase: string,
  model: string | undefined,
  systemPrompt: string,
  prompt: string,
): Promise<string> {
  // OpenAI-style message list we grow across iterations.
  const messages: Array<Record<string, unknown>> = [
    { role: 'system', content: systemPrompt },
    ...aiPanelStore.messages.map((m: { role: 'user' | 'assistant'; content: string }) => ({ role: m.role, content: m.content })),
    { role: 'user', content: prompt },
  ];

  // Tool schemas from the backend (OpenAI tools array).
  let tools: unknown[] = [];
  try {
    tools = JSON.parse(await invoke<string>('rain_tool_schemas'));
  }
  catch {
    tools = [];
  }

  const DESTRUCTIVE = new Set(['move_files', 'rename_item', 'delete_item']);
  const MAX_ITERATIONS = 10;

  for (let i = 0; i < MAX_ITERATIONS; i++) {
    const res = await fetch(`${routerBase}/v1/chat/completions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...(model ? { 'X-Model-Id': model } : {}) },
      body: JSON.stringify({
        model: model || 'default',
        messages,
        tools,
        tool_choice: 'auto',
        temperature: aiPanelStore.temperature,
        max_tokens: aiPanelStore.maxTokens,
        top_p: aiPanelStore.topP,
      }),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}: ${res.statusText}`);

    const data = JSON.parse((await res.text()).replace(/\s*data:\s*\[DONE\]\s*$/i, '').trim());
    const choice = data?.choices?.[0]?.message;
    if (!choice) return 'No response received.';

    const toolCalls = choice.tool_calls as Array<{ id: string; function: { name: string; arguments: string } }> | undefined;

    // No tool calls -> final answer.
    if (!toolCalls || toolCalls.length === 0) {
      return choice.content ?? 'No response received.';
    }

    // Record the assistant turn (with its tool_calls) before appending results.
    messages.push({ role: 'assistant', content: choice.content ?? '', tool_calls: toolCalls });

    for (const call of toolCalls) {
      const name = call.function?.name ?? '';
      let args: Record<string, unknown> = {};
      try { args = JSON.parse(call.function?.arguments || '{}'); }
      catch { args = {}; }

      let resultJson: string;
      if (DESTRUCTIVE.has(name)) {
        // Gate behind a confirmation card; await the user's decision.
        const confirmed = await requestToolConfirmation(name, args);
        if (confirmed) {
          resultJson = await executeDestructiveTool(name, args);
        }
        else {
          resultJson = JSON.stringify({ ok: false, cancelled: true, error: 'User cancelled the operation.' });
        }
      }
      else {
        resultJson = await invoke<string>('rain_run_tool', { name, args });
      }

      void logToolCall(name, args, resultJson);
      messages.push({ role: 'tool', tool_call_id: call.id, content: resultJson });
    }
  }

  return 'Stopped after the maximum number of tool steps. Ask me to continue if you need more.';
}

// --- Step 4 (confirmation) + Step 5 (memory) helpers ---
// requestToolConfirmation / executeDestructiveTool are fleshed out in Step 4.
// logToolCall / maybeRememberFromTurn are fleshed out in Step 5.

async function requestToolConfirmation(name: string, args: Record<string, unknown>): Promise<boolean> {
  // Build a structured confirmation card and resolve on Confirm/Cancel.
  let details: { tool: string; title: string; lines: string[]; warning?: string };
  if (name === 'move_files') {
    const srcs = Array.isArray(args.src) ? (args.src as string[]) : [String(args.src)];
    details = {
      tool: name,
      title: t('aiPanel.confirmMoveTitle'),
      lines: srcs.map(s => `${s}  →  ${String(args.dest)}`),
    };
  }
  else if (name === 'rename_item') {
    details = {
      tool: name,
      title: t('aiPanel.confirmRenameTitle'),
      lines: [`${String(args.old)}  →  ${String(args.new)}`],
    };
  }
  else {
    // delete_item
    const permanent = args.permanent === true;
    let warning: string | undefined;
    // Warn if the delete target is a non-empty folder.
    try {
      const listRaw = await invoke<string>('rain_run_tool', { name: 'list_directory', args: { path: String(args.path) } });
      const parsed = JSON.parse(listRaw);
      const count = parsed?.contents?.entries?.length ?? 0;
      if (parsed?.ok && count > 0) {
        warning = t('aiPanel.confirmDeleteFolderWarning', { count });
      }
    }
    catch {
      // Not a directory or unreadable — no warning.
    }
    details = {
      tool: name,
      title: permanent ? t('aiPanel.confirmDeletePermanentTitle') : t('aiPanel.confirmDeleteTitle'),
      lines: [String(args.path)],
      warning,
    };
  }

  return new Promise<boolean>((resolve) => {
    toolConfirmResolve = resolve;
    toolConfirmDetails.value = details;
  });
}

function confirmToolAction() {
  toolConfirmDetails.value = null;
  if (toolConfirmResolve) {
    const r = toolConfirmResolve; toolConfirmResolve = null;
    r(true);
  }
}

function cancelToolAction() {
  toolConfirmDetails.value = null;
  if (toolConfirmResolve) {
    const r = toolConfirmResolve; toolConfirmResolve = null;
    r(false);
  }
}

async function executeDestructiveTool(name: string, args: Record<string, unknown>): Promise<string> {
  try {
    if (name === 'move_files') {
      await invoke('move_items', { sourcePaths: args.src, destinationPath: args.dest });
      return JSON.stringify({ ok: true, moved: args.src, dest: args.dest });
    }
    if (name === 'rename_item') {
      await invoke('rename_item', { sourcePath: args.old, newName: args.new });
      return JSON.stringify({ ok: true, renamed: args.old, to: args.new });
    }
    if (name === 'delete_item') {
      await invoke('delete_items', { files: [args.path], permanent: args.permanent === true });
      return JSON.stringify({ ok: true, deleted: args.path });
    }
  }
  catch (error) {
    return JSON.stringify({ ok: false, error: error instanceof Error ? error.message : String(error) });
  }
  return JSON.stringify({ ok: false, error: `Unknown destructive tool: ${name}` });
}

async function logToolCall(name: string, args: Record<string, unknown>, resultJson: string): Promise<void> {
  // Placeholder — SQLite logging wired in a later pass. Console for now.
  // eslint-disable-next-line no-console
  console.debug('[rain tool]', name, args, resultJson?.slice(0, 200));
}

async function maybeRememberFromTurn(prompt: string, finalText: string): Promise<void> {
  // After the turn, ask the model to extract any durable fact worth saving to
  // long-term memory (preferences, recurring paths, conventions). Cheap, bounded,
  // and best-effort — never blocks or surfaces errors to the user.
  try {
    const routerBase = (aiPanelStore.routerEndpoint || '').replace(/\/+$/, '');
    if (!routerBase) return;
    const model = aiPanelStore.selectedModel || undefined;

    const extractionPrompt = `You are Rain's memory extractor. Given the latest exchange, decide if there is ONE durable fact worth remembering long-term about the user or their files (a preference, a frequently used path, a naming convention, a recurring workflow). If yes, reply with a single short line starting with "MEMORY:" (for a fact about the user/their habits) or "FAVORITE:" (for a path/model/preference used repeatedly). If nothing is worth saving, reply exactly "NONE". Do not explain.\n\nUser: ${prompt}\nRain: ${finalText}`;

    const res = await fetch(`${routerBase}/v1/chat/completions`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...(model ? { 'X-Model-Id': model } : {}) },
      body: JSON.stringify({
        model: model || 'default',
        messages: [{ role: 'user', content: extractionPrompt }],
        temperature: 0,
        max_tokens: 80,
      }),
    });
    if (!res.ok) return;

    const data = JSON.parse((await res.text()).replace(/\s*data:\s*\[DONE\]\s*$/i, '').trim());
    const out = (data?.choices?.[0]?.message?.content ?? '').trim();
    if (!out || /^NONE$/i.test(out)) return;

    const memMatch = out.match(/^MEMORY:\s*(.+)$/i);
    const favMatch = out.match(/^FAVORITE:\s*(.+)$/i);
    if (memMatch) {
      await aiPanelStore.appendMemory(memMatch[1].trim());
    }
    else if (favMatch) {
      await aiPanelStore.appendFavorite(favMatch[1].trim());
    }
  }
  catch {
    // Best-effort; memory extraction failures are silent.
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

    const scope = aiPanelStore.searchScope || 'current';
    const scopeText = scope === 'current'
      ? `the current folder (${currentPath || 'unknown'})`
      : scope === 'all'
        ? 'all drives on this machine'
        : `the drive ${scope}`;

    const systemPrompt = (aiPanelStore.systemPrompt || '')
      .replace(/\{current_path\}/g, currentPath || '')
      .replace(/\{selected_files\}/g, selectedFiles.length > 0 ? selectedFiles.join(', ') : 'none')
      .replace(/\{search_scope\}/g, scopeText)
      + `\n\nWhen searching for files, search ${scopeText} unless the user says otherwise.`
      + (aiPanelStore.soulText ? `\n\n=== SOUL (your identity) ===\n${aiPanelStore.soulText}` : '')
      + (aiPanelStore.memoryText ? `\n\n=== MEMORY (what you've learned) ===\n${aiPanelStore.memoryText}` : '')
      + (aiPanelStore.favoritesText ? `\n\n=== FAVORITES (noticed preferences) ===\n${aiPanelStore.favoritesText}` : '');

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
      // Text inference -> 9Router via the Rain agent loop (tool calling).
      if (!routerBase) {
        throw new Error('9Router endpoint not configured. Set it in Settings.');
      }
      const finalText = await runAgentLoop(routerBase, model, systemPrompt, prompt);
      aiPanelStore.addMessage('assistant', finalText);
      await maybeSpeak(finalText);
      // Step 5 (memory auto-append) runs after the turn — see maybeRememberFromTurn.
      void maybeRememberFromTurn(prompt, finalText);
      aiPanelStore.setLoading(false);
      return;
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
  if (toolConfirmResolve) {
    const r = toolConfirmResolve; toolConfirmResolve = null;
    r(false);
  }
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
  void loadScopeDrives();
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
        v-model="aiPanelStore.routerEndpoint"
        :placeholder="t('aiPanel.endpointPlaceholder')"
        class="ai-panel__endpoint-input"
      />
      <select
        :value="aiPanelStore.selectedModel"
        @change="aiPanelStore.setSelectedModel(($event.target as HTMLSelectElement).value)"
        class="ai-panel__model-select"
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
    <div class="ai-panel__scope-row">
      <span class="ai-panel__scope-label">{{ t('aiPanel.searchScope') }}</span>
      <select
        :value="aiPanelStore.searchScope"
        class="ai-panel__scope-select"
        @change="handleScopeChange(($event.target as HTMLSelectElement).value)"
      >
        <option value="current">{{ t('aiPanel.scopeCurrent') }}</option>
        <option value="all">{{ t('aiPanel.scopeAll') }}</option>
        <option
          v-for="drive in scopeDrives"
          :key="drive"
          :value="drive"
        >
          {{ drive }}
        </option>
      </select>
    </div>
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

    <div
      v-if="toolConfirmDetails"
      class="ai-panel__confirm-card"
    >
      <div class="ai-panel__confirm-title">{{ toolConfirmDetails.title }}</div>
      <div class="ai-panel__confirm-lines">
        <div
          v-for="(line, i) in toolConfirmDetails.lines"
          :key="i"
          class="ai-panel__confirm-line"
        >
          {{ line }}
        </div>
      </div>
      <div
        v-if="toolConfirmDetails.warning"
        class="ai-panel__confirm-warning"
      >
        {{ toolConfirmDetails.warning }}
      </div>
      <div class="ai-panel__confirm-actions">
        <Button variant="ghost" size="sm" @click="cancelToolAction">
          <XIcon :size="14" />
          {{ t('common.cancel') }}
        </Button>
        <Button size="sm" @click="confirmToolAction">
          <CheckIcon :size="14" />
          {{ t('common.confirm') }}
        </Button>
      </div>
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

.ai-panel__confirm-card {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  margin: 0 8px 8px;
  padding: 10px;
  background-color: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
}

.ai-panel__confirm-title {
  color: hsl(var(--foreground));
  font-size: 0.8125rem;
  font-weight: 600;
}

.ai-panel__confirm-lines {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.ai-panel__confirm-line {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
  word-break: break-all;
}

.ai-panel__confirm-warning {
  color: hsl(var(--destructive, 0 70% 60%));
  font-size: 0.72rem;
}

.ai-panel__confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.375rem;
}

.ai-panel__scope-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px 0;
}

.ai-panel__scope-label {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
}

.ai-panel__scope-select {
  flex: 1;
  min-width: 0;
  padding: 4px 6px;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  font-size: 0.75rem;
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
