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
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-endpoint">
          {{ t('aiPanel.endpointUrl') }}
        </label>
        <Input
          id="ai-panel-endpoint"
          v-model="endpointUrl"
          placeholder="http://localhost:9777/api/text"
          class="ai-panel-settings__input"
        />
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-model">
          {{ t('aiPanel.model') }}
        </label>
        <Input
          id="ai-panel-model"
          v-model="model"
          placeholder="e.g. gpt-4, llama3"
          class="ai-panel-settings__input"
        />
      </div>
      <div class="ai-panel-settings__toggle">
        <label class="ai-panel-settings__label" for="ai-panel-omnix">
          {{ t('aiPanel.omnixEnabled') }}
        </label>
        <Switch
          id="ai-panel-omnix"
          :model-value="omnixEnabled"
          @update:model-value="omnixEnabled = $event"
        />
      </div>
      <div class="ai-panel-settings__field">
        <label class="ai-panel-settings__label" for="ai-panel-router">
          9Router endpoint (text inference)
        </label>
        <Input
          id="ai-panel-router"
          v-model="routerEndpoint"
          placeholder="http://192.168.1.67:9000"
          class="ai-panel-settings__input"
        />
      </div>
      <div class="ai-panel-settings__toggle">
        <label class="ai-panel-settings__label" for="ai-panel-tts">
          Speak responses (Omnix TTS)
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
</style>
