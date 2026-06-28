<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { BotIcon } from '@lucide/vue';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { useAiPanelStore } from '@/stores/runtime/ai-panel';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();
const aiPanelStore = useAiPanelStore();

onMounted(() => {
  // Populate the model dropdown from 9Router when the settings page opens,
  // independent of whether the AI panel has been opened yet.
  void aiPanelStore.fetchModels();
});

const endpointUrl = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.endpointUrl,
  set: (value: string) => {
    userSettingsStore.userSettings.meridian.aiPanel.endpointUrl = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.endpointUrl', value);
    aiPanelStore.setEndpoint(value);
  },
});

const model = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.model,
  set: (value: string) => {
    userSettingsStore.userSettings.meridian.aiPanel.model = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.model', value);
    aiPanelStore.setSelectedModel(value);
  },
});

const omnixEnabled = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.omnixEnabled,
  set: (value: boolean) => {
    userSettingsStore.userSettings.meridian.aiPanel.omnixEnabled = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.omnixEnabled', value);
    aiPanelStore.setUseOmnix(value);
  },
});

const routerEndpoint = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.routerEndpoint,
  set: (value: string) => {
    userSettingsStore.userSettings.meridian.aiPanel.routerEndpoint = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.routerEndpoint', value);
    aiPanelStore.setRouterEndpoint(value);
  },
});

const ttsEnabled = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.ttsEnabled,
  set: (value: boolean) => {
    userSettingsStore.userSettings.meridian.aiPanel.ttsEnabled = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.ttsEnabled', value);
    aiPanelStore.setTtsEnabled(value);
  },
});

const systemPrompt = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.systemPrompt,
  set: (value: string) => {
    userSettingsStore.userSettings.meridian.aiPanel.systemPrompt = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.systemPrompt', value);
    aiPanelStore.setSystemPrompt(value);
  },
});

const temperature = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.temperature ?? 0.7,
  set: (value: number) => {
    userSettingsStore.userSettings.meridian.aiPanel.temperature = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.temperature', value);
    aiPanelStore.setTemperature(value);
  },
});

const maxTokens = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.maxTokens ?? 1024,
  set: (value: number) => {
    userSettingsStore.userSettings.meridian.aiPanel.maxTokens = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.maxTokens', value);
    aiPanelStore.setMaxTokens(value);
  },
});

const topP = computed({
  get: () => userSettingsStore.userSettings.meridian.aiPanel.topP ?? 1,
  set: (value: number) => {
    userSettingsStore.userSettings.meridian.aiPanel.topP = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.topP', value);
    aiPanelStore.setTopP(value);
  },
});

// Read-only: context window of the selected model, if the endpoint reports it.
// 9Router's /v1/models currently returns only id/object/owned_by, so this is
// informational and falls back to "Not reported by endpoint".
const contextWindow = computed(() => {
  const selected = aiPanelStore.models.find(m => m.id === model.value) as
    | { id: string; contextWindow?: number }
    | undefined;
  const ctx = selected?.contextWindow;
  return ctx && ctx > 0 ? `${ctx.toLocaleString()} tokens` : 'Not reported by endpoint';
});

// Heuristic: does the selected model support OpenAI-style tool/function calling?
// Rain's agent mode (tools + memory) needs this. We can't always know for sure
// from the id alone, so we match known tool-capable families and warn otherwise.
const TOOL_CAPABLE_PATTERNS = [
  /qwen\s*3|qwen3|qwen2\.5/i,
  /gpt-4|gpt-4o|gpt-3\.5-turbo|o1|o3/i,
  /claude/i,
  /mistral|mixtral|magistral/i,
  /llama-?3\.[12]|llama-?3\.3/i,
  /hermes\s*[23]|hermes-?[23]/i,
  /command-?r/i,
  /firefunction|functionary/i,
];

const modelSupportsTools = computed(() => {
  const id = (model.value || '').toLowerCase();
  if (!id) return true; // don't warn when nothing is selected yet
  return TOOL_CAPABLE_PATTERNS.some(re => re.test(id));
});
</script>

<template>
  <SettingsItem
    :title="t('settings.meridian.aiPanel.title')"
    :description="t('settings.meridian.aiPanel.description')"
    :icon="BotIcon"
  >
    <div class="ai-panel-settings">
      <!-- Primary AI: 9Router (handles all text inference) -->
      <div class="ai-panel-settings__section-title">Primary AI (9Router)</div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-router">
          Endpoint URL
        </label>
        <Input
          id="ai-panel-router"
          v-model="routerEndpoint"
          placeholder="http://localhost:20128/v1"
          class="ai-panel-settings__input"
        />
        <span class="ai-panel-settings__hint">
          {{ aiPanelStore.routerOnline ? 'Connected' : 'Offline' }}
        </span>
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-model">
          Model
        </label>
        <select
          id="ai-panel-model"
          v-model="model"
          class="ai-panel-settings__select"
        >
          <option v-if="aiPanelStore.models.length === 0" :value="model">
            {{ model || 'No models loaded' }}
          </option>
          <option
            v-for="m in aiPanelStore.models"
            :key="m.id"
            :value="m.id"
          >
            {{ m.id }}
          </option>
        </select>
        <div
          v-if="!modelSupportsTools"
          class="ai-panel-settings__tool-warning"
        >
          Rain agent mode requires a tool-capable model. Try Qwen3.6 via 9Router.
        </div>
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label">
          Context window
        </label>
        <div class="ai-panel-settings__readonly">
          {{ contextWindow }}
        </div>
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-system-prompt">
          System prompt
        </label>
        <textarea
          id="ai-panel-system-prompt"
          v-model="systemPrompt"
          rows="4"
          class="ai-panel-settings__select"
        />
        <span class="ai-panel-settings__hint">
          Placeholders: {current_path}, {selected_files}
        </span>
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-temperature">
          Temperature ({{ temperature }})
        </label>
        <Input
          id="ai-panel-temperature"
          v-model.number="temperature"
          type="number"
          step="0.1"
          min="0"
          max="2"
          class="ai-panel-settings__input"
        />
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-max-tokens">
          Max tokens
        </label>
        <Input
          id="ai-panel-max-tokens"
          v-model.number="maxTokens"
          type="number"
          step="64"
          min="1"
          class="ai-panel-settings__input"
        />
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-top-p">
          Top-p ({{ topP }})
        </label>
        <Input
          id="ai-panel-top-p"
          v-model.number="topP"
          type="number"
          step="0.05"
          min="0"
          max="1"
          class="ai-panel-settings__input"
        />
      </div>

      <!-- Local AI Enhancement: Omnix (optional, off by default) -->
      <div class="ai-panel-settings__section-title">Local AI Enhancement (Omnix) — optional</div>
      <div class="ai-panel-settings__toggle">
        <div>
          <label class="ai-panel-settings__label" for="ai-panel-omnix">
            Enable Omnix
          </label>
          <span class="ai-panel-settings__hint">
            Adds Vision, TTS, and Director. App works fully without it via 9Router.
          </span>
        </div>
        <Switch
          id="ai-panel-omnix"
          :model-value="omnixEnabled"
          @update:model-value="omnixEnabled = $event"
        />
      </div>
      <div v-if="omnixEnabled" class="ai-panel-settings__toggle">
        <label class="ai-panel-settings__label" for="ai-panel-tts">
          Speak responses (TTS)
        </label>
        <Switch
          id="ai-panel-tts"
          :model-value="ttsEnabled"
          @update:model-value="ttsEnabled = $event"
        />
      </div>
    </div>
  </SettingsItem>
</template>

<style scoped>
.ai-panel-settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.ai-panel-settings__field {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

.ai-panel-settings__label {
  color: hsl(var(--foreground));
  font-size: 0.875rem;
  font-weight: 500;
}

.ai-panel-settings__input {
  width: 100%;
}

.ai-panel-settings__toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}

.ai-panel-settings__section-title {
  color: hsl(var(--foreground));
  font-size: 0.95rem;
  font-weight: 600;
  margin-top: 0.5rem;
  padding-bottom: 0.25rem;
  border-bottom: 1px solid hsl(var(--border));
}

.ai-panel-settings__hint {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
}

.ai-panel-settings__readonly {
  width: 100%;
  padding: 0.5rem;
  border-radius: 0.375rem;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--muted) / 30%);
  color: hsl(var(--muted-foreground));
  font-size: 0.875rem;
}

.ai-panel-settings__tool-warning {
  margin-top: 0.375rem;
  color: hsl(var(--destructive, 38 92% 50%));
  font-size: 0.75rem;
  line-height: 1.3;
}

.ai-panel-settings__select {
  width: 100%;
  padding: 0.5rem;
  border-radius: 0.375rem;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  font-size: 0.875rem;
}
</style>
