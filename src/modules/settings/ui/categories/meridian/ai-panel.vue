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
  // Populate the model dropdown from the local AI server when the settings page opens,
  // independent of whether the AI panel has been opened yet.
  void aiPanelStore.fetchModels();
});

// (Removed: legacy `endpointUrl` binding. The original settings UI had a
//  separate "Endpoint URL" input that duplicated `routerEndpoint`. With
//  the exo-style rewrite the Server URL row is wired to `routerEndpoint`
//  only — keeping `endpointUrl` as a silent reactive would be a drift
//  landmine for future maintainers, so we drop it from the script. The
//  underlying userSettings.meridian.aiPanel.endpointUrl value is left
//  untouched for any installed user that has it on disk; it just has
//  no in-app control writing it.)

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
// Local AI server's /v1/models currently returns only id/object/owned_by, so this is
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
    <div class="exo-card exo-card--indigo">
      <!-- Tile: Bot icon themed from the indigo accent token. -->
      <div class="exo-tile" aria-hidden="true">
        <BotIcon :size="28" class="exo-tile__icon" />
        <span class="exo-tile__led" />
      </div>

      <!-- Identity: AI Panel section title + connection status pill.
           Pill sits as a sibling of the sub-line (NOT inside it) so the
           rounded geometry isn't squashed by `.exo-identity__sub`'s
           mono font-family + word-break: break-all. -->
      <div class="exo-identity">
        <span class="exo-identity__title">Primary AI</span>
        <span class="exo-identity__sub">Local AI server</span>
        <span
          class="exo-status-pill"
          :class="aiPanelStore.routerOnline ? 'exo-status-pill--running' : 'exo-status-pill--offline'"
        >
          <span class="exo-status-pill__dot" />
          {{ aiPanelStore.routerOnline ? 'Online' : 'Offline' }}
        </span>
      </div>

      <!-- Specs: full-width Server URL row at the top so the user can
           change the endpoint address, then 2-up grids for Model + Context,
           Temperature + Max tokens, Top-p + System-prompt. -->
      <div class="exo-specs">
        <div class="exo-specs__field">
          <label class="exo-specs__label" for="ai-panel-router">
            Server URL
          </label>
          <Input
            id="ai-panel-router"
            v-model="routerEndpoint"
            placeholder="http://localhost:11434/v1"
            class="exo-specs__input"
          />
          <span class="exo-card-hint">
            Local OpenAI-compatible endpoint (Lemonade / llama.cpp / 9Router).
          </span>
        </div>

        <div class="exo-specs--two-col">
          <div class="exo-specs__field">
            <label class="exo-specs__label" for="ai-panel-model">Model</label>
            <select
              id="ai-panel-model"
              v-model="model"
              class="exo-specs__select"
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
              class="exo-card-tool-warning"
            >
              Rain agent mode requires a tool-capable model. Try a Qwen,
              Llama 3.1+, or GPT-4 class model.
            </div>
          </div>
          <div class="exo-specs__field">
            <label class="exo-specs__label">Context window</label>
            <div class="exo-card-readonly">{{ contextWindow }}</div>
          </div>
        </div>

        <div class="exo-specs--two-col">
          <div class="exo-specs__field">
            <label class="exo-specs__label" for="ai-panel-temperature">
              Temperature ({{ temperature }})
            </label>
            <Input
              id="ai-panel-temperature"
              v-model.number="temperature"
              type="number"
              step="0.1"
              min="0"
              max="2"
              class="exo-specs__input"
            />
          </div>
          <div class="exo-specs__field">
            <label class="exo-specs__label" for="ai-panel-max-tokens">Max tokens</label>
            <Input
              id="ai-panel-max-tokens"
              v-model.number="maxTokens"
              type="number"
              step="64"
              min="1"
              class="exo-specs__input"
            />
          </div>
        </div>

        <div class="exo-specs--two-col">
          <div class="exo-specs__field">
            <label class="exo-specs__label" for="ai-panel-top-p">
              Top-p ({{ topP }})
            </label>
            <Input
              id="ai-panel-top-p"
              v-model.number="topP"
              type="number"
              step="0.05"
              min="0"
              max="1"
              class="exo-specs__input"
            />
          </div>
          <div class="exo-specs__field" style="justify-content: flex-end;">
            <span class="exo-card-hint">No selection = baseline defaults (temp 0.7, max 1024, top_p 1).</span>
          </div>
        </div>

        <div class="exo-specs__field">
          <label class="exo-specs__label" for="ai-panel-system-prompt">System prompt</label>
          <textarea
            id="ai-panel-system-prompt"
            v-model="systemPrompt"
            rows="3"
            class="exo-specs__textarea"
          />
          <span class="exo-card-hint">
            Placeholders: &#123;current_path&#125;, &#123;selected_files&#125;
          </span>
        </div>
      </div>

      <!-- Actions: Omnix toggle + TTS toggle stacked with their inline hints.
           Status indicator (online/offline) is now inside the identity column
           so the actions stay focused on user-controllable switches. -->
      <div class="exo-actions">
        <div class="exo-card-toggle-row">
          <div>
            <label class="exo-specs__label" for="ai-panel-omnix">Enable Omnix</label>
            <span class="exo-card-hint">
              Optional add-on for Vision / TTS. Lemonade (above) already ships
              with vision + TTS, so most users can leave this off.
            </span>
          </div>
          <Switch
            id="ai-panel-omnix"
            :model-value="omnixEnabled"
            @update:model-value="omnixEnabled = $event"
          />
        </div>
        <div v-if="omnixEnabled" class="exo-card-toggle-row">
          <label class="exo-specs__label" for="ai-panel-tts">Speak responses (TTS)</label>
          <Switch
            id="ai-panel-tts"
            :model-value="ttsEnabled"
            @update:model-value="ttsEnabled = $event"
          />
        </div>
      </div>
    </div>
  </SettingsItem>
</template>

<style scoped>
/* Local styles were moved to src/styles/exo.css as `.exo-card-hint`,
   `.exo-card-readonly`, `.exo-card-tool-warning`, `.exo-card-toggle-row`
   so any sibling Settings sub-page can re-use the same look without
   falling outside Vue scoped-style attribute matching. This scoped
   block is intentionally empty — only import / global tokens drive
   the visual language now. */
</style>
