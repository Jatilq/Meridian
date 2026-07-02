<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
/**
 * Rain CLI Slide-In — a compact overlay that slides in from the right
 * on any page. Shares conversation state with the full CLI tab via
 * aiPanelStore. Provides quick access to Rain for small tasks without
 * navigating away from the current view.
 */

import { computed, ref, nextTick, watch } from 'vue';
import { useAiPanelStore } from '@/stores/runtime/ai-panel';
import { useI18n } from 'vue-i18n';
import {
  BotIcon,
  SendIcon,
  LoaderCircleIcon,
  XIcon,
  PanelRightCloseIcon,
} from '@lucide/vue';
import { invoke } from '@tauri-apps/api/core';

const { t } = useI18n();
const aiPanelStore = useAiPanelStore();

const emit = defineEmits<{
  close: [];
}>();

const inputValue = ref('');
const isLoading = ref(false);
const messagesContainerRef = ref<HTMLElement | null>(null);
// Tracks whether we've already waited for Omnix once (so retries use a shorter timeout)
let OMNIX_SPAWN_WAITED = false;

// Use existing aiPanelStore messages for shared conversation state
const messages = computed(() => aiPanelStore.messages);

function scrollToBottom() {
  nextTick(() => {
    if (messagesContainerRef.value) {
      messagesContainerRef.value.scrollTop = messagesContainerRef.value.scrollHeight;
    }
  });
}

watch(messages, () => scrollToBottom(), { deep: true });

async function handleSend() {
  const prompt = inputValue.value.trim();
  if (!prompt || isLoading.value) return;

  inputValue.value = '';
  isLoading.value = true;
  aiPanelStore.addMessage('user', prompt);

  // Simple text inference — Omnix-first with startup wait (same as ai-panel.vue)
  try {
    if (aiPanelStore.useOmnix && !aiPanelStore.omnixOnline) {
      console.debug('[slide-in] Omnix offline — attempting spawn...');
      try {
        await invoke('spawn_omnix', { omnixPath: aiPanelStore.omnixPath || null });
        console.debug('[slide-in] spawn_omnix returned Ok');
      } catch (spawnErr) {
        console.error('[slide-in] spawn_omnix failed:', spawnErr);
      }
      // First launch: npm install can take 60-120s. Wait up to 120s.
      // Retries (background process already spawned): wait 30s.
      const waitSecs = OMNIX_SPAWN_WAITED ? 30 : 120;
      OMNIX_SPAWN_WAITED = true;
      for (let s = 0; s < waitSecs; s++) {
        await new Promise(r => setTimeout(r, 1000));
        try {
          const online = await invoke<boolean>('get_omnix_status');
          if (online) {
            console.debug(`[slide-in] Omnix online after ${s + 1}s`);
            aiPanelStore.setOmnixOnline(true);
            break;
          }
        } catch (statusErr) {
          console.debug('[slide-in] get_omnix_status error:', statusErr);
        }
      }
      if (!aiPanelStore.omnixOnline) {
        console.warn(`[slide-in] Omnix did not come online within ${waitSecs}s`);
      }
    }

    const routerBase = (aiPanelStore.routerEndpoint || '').replace(/\/+$/, '');
    const model = aiPanelStore.selectedModel || undefined;

    if (aiPanelStore.useOmnix && aiPanelStore.omnixOnline) {
      const omnixText = await invoke<string>('omnix_text', {
        prompt: (aiPanelStore.systemPrompt || '').replace(/\{current_path\}/g, aiPanelStore.currentPath || '') + `\n\nUser: ${prompt}`,
        systemPrompt: aiPanelStore.systemPrompt || '',
        temperature: aiPanelStore.temperature,
        maxTokens: aiPanelStore.maxTokens,
        topP: aiPanelStore.topP,
      });
      aiPanelStore.addMessage('assistant', omnixText);
      isLoading.value = false;
      scrollToBottom();
      return;
    }

    const isRouterExplicit = routerBase && aiPanelStore.connectionMode !== 'basic';
    if (!isRouterExplicit) {
    const hint = aiPanelStore.useOmnix
      ? 'Rain is warming up. Hang tight — I\'ll try again in a moment...'
      : aiPanelStore.routerOnline
        ? 'No AI endpoint is configured. Download Lemonade from Backend Manager to start with a local model, or enable Omnix in Settings.'
        : 'No AI endpoint is configured. Open Backend Manager to install Lemonade (the default Tier-1 backend), or enable Omnix in Settings.';
      aiPanelStore.addMessage('assistant', hint);
      isLoading.value = false;
      scrollToBottom();
      return;
    }

    const msgs = [
      { role: 'system', content: (aiPanelStore.systemPrompt || '').replace(/\{current_path\}/g, aiPanelStore.currentPath || '') },
      ...aiPanelStore.messages.map(m => ({ role: m.role, content: m.content })),
      { role: 'user', content: prompt },
    ];

    const chatUrl = routerBase.endsWith('/v1') ? `${routerBase}/chat/completions` : `${routerBase}/v1/chat/completions`;
    const res = await fetch(chatUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...(model ? { 'X-Model-Id': model } : {}) },
      body: JSON.stringify({
        model: model || 'default',
        messages: msgs,
        temperature: aiPanelStore.temperature,
        max_tokens: aiPanelStore.maxTokens,
        top_p: aiPanelStore.topP,
      }),
    });

    if (!res.ok) throw new Error(`HTTP ${res.status}`);

    const data = JSON.parse((await res.text()).replace(/\s*data:\s*\[DONE\]\s*$/i, '').trim());
    const text = data?.choices?.[0]?.message?.content ?? 'No response.';
    aiPanelStore.addMessage('assistant', text);
  } catch (error) {
    const msg = error instanceof Error ? error.message : 'Error';
    aiPanelStore.addMessage('assistant', `Error: ${msg}`);
  } finally {
    isLoading.value = false;
    scrollToBottom();
  }
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault();
    handleSend();
  }
}

function closeSlideIn() {
  emit('close');
}
</script>

<template>
  <div class="rain-slide-in">
    <!-- Header -->
    <div class="rain-slide-in__header">
      <div class="rain-slide-in__header-left">
        <BotIcon :size="16" class="rain-slide-in__header-icon" />
        <span class="rain-slide-in__header-title">Rain</span>
        <span
          class="rain-slide-in__dot"
          :class="{
            'rain-slide-in__dot--online': aiPanelStore.omnixOnline,
            'rain-slide-in__dot--offline': !aiPanelStore.omnixOnline,
          }"
        />
      </div>
      <button class="rain-slide-in__close-btn" @click="closeSlideIn">
        <XIcon :size="16" />
      </button>
    </div>

    <!-- Messages -->
    <div ref="messagesContainerRef" class="rain-slide-in__messages">
      <div v-if="messages.length === 0" class="rain-slide-in__empty">
        <BotIcon :size="24" class="rain-slide-in__empty-icon" />
        <p class="rain-slide-in__empty-text">Ask Rain anything</p>
      </div>
      <div
        v-for="(msg, i) in messages"
        :key="i"
        class="rain-slide-in__message"
        :class="{
          'rain-slide-in__message--user': msg.role === 'user',
          'rain-slide-in__message--assistant': msg.role === 'assistant',
        }"
      >
        <div class="rain-slide-in__bubble">
          {{ msg.content }}
        </div>
      </div>
      <div v-if="isLoading" class="rain-slide-in__loading">
        <LoaderCircleIcon :size="14" class="rain-slide-in__spinner" />
        <span>Thinking...</span>
      </div>
    </div>

    <!-- Input -->
    <div class="rain-slide-in__input-area">
      <div class="rain-slide-in__input-wrap">
        <span class="rain-slide-in__prompt">></span>
        <input
          v-model="inputValue"
          class="rain-slide-in__input"
          placeholder="Ask Rain..."
          :disabled="isLoading"
          @keydown="handleKeyDown"
        />
        <button
          class="rain-slide-in__send-btn"
          :disabled="!inputValue.trim() || isLoading"
          @click="handleSend"
        >
          <SendIcon v-if="!isLoading" :size="14" />
          <LoaderCircleIcon v-else :size="14" class="rain-slide-in__spinner" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.rain-slide-in {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: hsl(var(--background-2));
  border-inline-start: 1px solid hsl(var(--border));
  width: 340px;
  min-width: 320px;
  max-width: 40%;
}

.rain-slide-in__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid hsl(var(--border));
  flex-shrink: 0;
}

.rain-slide-in__header-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.rain-slide-in__header-icon {
  color: hsl(var(--primary));
}

.rain-slide-in__header-title {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.03em;
  color: hsl(var(--foreground));
}

.rain-slide-in__dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}

.rain-slide-in__dot--online {
  background-color: hsl(var(--success));
}

.rain-slide-in__dot--offline {
  background-color: hsl(var(--muted-foreground));
}

.rain-slide-in__close-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-sm);
  background: transparent;
  border: none;
  color: hsl(var(--muted-foreground));
  cursor: pointer;
}

.rain-slide-in__close-btn:hover {
  color: hsl(var(--foreground));
  background-color: hsl(var(--foreground) / 5%);
}

.rain-slide-in__messages {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.rain-slide-in__message {
  display: flex;
}

.rain-slide-in__message--user {
  justify-content: flex-end;
}

.rain-slide-in__message--assistant {
  justify-content: flex-start;
}

.rain-slide-in__bubble {
  max-width: 90%;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  font-size: 0.8rem;
  line-height: 1.4;
  word-break: break-word;
}

.rain-slide-in__message--user .rain-slide-in__bubble {
  background-color: hsl(var(--primary) / 15%);
  color: hsl(var(--foreground));
}

.rain-slide-in__message--assistant .rain-slide-in__bubble {
  background-color: hsl(var(--background-3, var(--background)));
  color: hsl(var(--foreground));
  border: 1px solid hsl(var(--border));
}

.rain-slide-in__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 32px 16px;
  gap: 6px;
}

.rain-slide-in__empty-icon {
  color: hsl(var(--primary) / 30%);
}

.rain-slide-in__empty-text {
  color: hsl(var(--muted-foreground));
  font-size: 0.8rem;
  margin: 0;
}

.rain-slide-in__loading {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
}

.rain-slide-in__spinner {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.rain-slide-in__input-area {
  flex-shrink: 0;
  padding: 8px 10px;
  border-top: 1px solid hsl(var(--border));
}

.rain-slide-in__input-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
  background-color: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  padding: 4px 8px;
}

.rain-slide-in__input-wrap:focus-within {
  border-color: hsl(var(--primary) / 50%);
}

.rain-slide-in__prompt {
  color: hsl(var(--primary));
  font-weight: 700;
  font-size: 0.85rem;
  user-select: none;
}

.rain-slide-in__input {
  flex: 1;
  min-width: 0;
  background: transparent;
  border: none;
  outline: none;
  color: hsl(var(--foreground));
  font-size: 0.82rem;
  font-family: inherit;
  padding: 3px 0;
}

.rain-slide-in__input::placeholder {
  color: hsl(var(--muted-foreground) / 60%);
}

.rain-slide-in__send-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  border: none;
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  cursor: pointer;
  flex-shrink: 0;
}

.rain-slide-in__send-btn:hover:not(:disabled) {
  filter: brightness(1.1);
}

.rain-slide-in__send-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
</style>
