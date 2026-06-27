<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
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
