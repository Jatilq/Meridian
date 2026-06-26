// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { defineStore } from 'pinia';
import { ref, computed, watch } from 'vue';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const STORAGE_KEY = 'meridian-ai-panel';

interface PersistedAiPanelState {
  endpoint: string;
  selectedModel: string;
  useOmnix: boolean;
}

function loadPersistedState(): Partial<PersistedAiPanelState> {
  if (typeof window === 'undefined') return {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const data = JSON.parse(raw);
    return {
      endpoint: typeof data.endpoint === 'string' ? data.endpoint : 'http://localhost:11434',
      selectedModel: typeof data.selectedModel === 'string' ? data.selectedModel : '',
      useOmnix: typeof data.useOmnix === 'boolean' ? data.useOmnix : false,
    };
  }
  catch {
    return {};
  }
}

function persistState(state: {
  endpoint: string;
  selectedModel: string;
  useOmnix: boolean;
}) {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  }
  catch {
    // ignore storage errors
  }
}

const persisted = loadPersistedState();

export const useAiPanelStore = defineStore('aiPanel', () => {
  const userSettingsStore = useUserSettingsStore();
  const isOpen = ref(false);
  const isLoading = ref(false);
  const messages = ref<Array<{ role: 'user' | 'assistant'; content: string }>>([]);
  const input = ref('');
  const endpoint = ref(userSettingsStore.userSettings.meridian?.aiPanel?.endpointUrl || persisted.endpoint || 'http://localhost:7770/api/text');
  const selectedModel = ref(userSettingsStore.userSettings.meridian?.aiPanel?.model || persisted.selectedModel || '');
  const models = ref<Array<{ id: string }>>([]);
  const modelsLoaded = ref(false);
  const useOmnix = ref(userSettingsStore.userSettings.meridian?.aiPanel?.omnixEnabled ?? persisted.useOmnix ?? false);
  const omnixOnline = ref(false);
  const currentPath = ref('');
  const selectedFiles = ref<string[]>([]);

  const canSend = computed(() => input.value.trim().length > 0 && !isLoading.value);

  function open() { isOpen.value = true; }
  function close() { isOpen.value = false; }
  function toggle() { isOpen.value ? close() : open(); }
  function setInput(value: string) { input.value = value; }
  function setEndpoint(value: string) {
    endpoint.value = value;
    modelsLoaded.value = false;
    models.value = [];
    userSettingsStore.userSettings.meridian.aiPanel.endpointUrl = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.endpointUrl', value);
    persistState({ endpoint: value, selectedModel: selectedModel.value, useOmnix: useOmnix.value });
  }
  function setSelectedModel(value: string) {
    selectedModel.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.model = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.model', value);
    persistState({ endpoint: endpoint.value, selectedModel: value, useOmnix: useOmnix.value });
  }
  function setUseOmnix(value: boolean) {
    useOmnix.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.omnixEnabled = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.omnixEnabled', value);
    persistState({ endpoint: endpoint.value, selectedModel: selectedModel.value, useOmnix: value });
  }
  function setOmnixOnline(value: boolean) { omnixOnline.value = value; }
  function setCurrentPath(path: string) { currentPath.value = path; }
  function setSelectedFiles(files: string[]) { selectedFiles.value = files; }
  function addMessage(role: 'user' | 'assistant', content: string) { messages.value.push({ role, content }); }
  function clearMessages() { messages.value = []; }
  function setLoading(value: boolean) { isLoading.value = value; }
  function setModels(value: Array<{ id: string }>) {
    models.value = value;
    modelsLoaded.value = true;
  }

  async function fetchModels() {
    if (useOmnix.value) return;
    const baseUrl = endpoint.value.replace(/\/+$/, '');
    try {
      const response = await fetch(`${baseUrl}/v1/models`);
      if (!response.ok) return;
      const data = await response.json();
      const modelList = data.data ?? data.models ?? [];
      if (Array.isArray(modelList)) {
        setModels(
          modelList
            .filter((item: { id?: string }) => typeof item.id === 'string')
            .map((item: { id: string }) => ({ id: item.id })),
        );
      }
    }
    catch {
      models.value = [];
      modelsLoaded.value = true;
    }
  }

  watch(
    () => isOpen.value,
    (open) => {
      if (open && !modelsLoaded.value && !useOmnix.value) {
        void fetchModels();
      }
    },
  );

  watch(
    () => endpoint.value,
    () => {
      modelsLoaded.value = false;
      models.value = [];
    },
  );

  return {
    isOpen, isLoading, messages, input, endpoint, selectedModel, models, modelsLoaded,
    useOmnix, omnixOnline, currentPath, selectedFiles, canSend,
    open, close, toggle, setInput, setEndpoint, setSelectedModel, setUseOmnix,
    setOmnixOnline, setCurrentPath, setSelectedFiles, addMessage, clearMessages,
    setLoading, setModels, fetchModels,
  };
});
