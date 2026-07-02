// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { defineStore } from 'pinia';
import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import type { AiPanelConnectionMode, AiPanelProviderId } from '@/types/user-settings';
import { AI_PANEL_PROVIDER_URLS } from '@/types/user-settings';

const STORAGE_KEY = 'meridian-ai-panel';

// `useOmnix` is NOT in this persisted shape any more (Omnix-on-boot fix).
// Earlier this struct round-tripped a parallel-source-of-truth boolean
// into the WebView2 localStorage, but that could override the Pinia
// default at first paint before the lazy-store migrations ran. The Pinia
// literal + the 22 → 23 + 27 → 28 force-setting migrations are now the
// sole source of truth, so writing `useOmnix` to localStorage is dead
// code (write-only-no-reader) and gets dropped here alongside its
// sibling references in `loadPersistedState`, `persistState`, and the
// individual setter callers.
interface PersistedAiPanelState {
  endpoint: string;
  selectedModel: string;
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
    };
  }
  catch {
    return {};
  }
}

function persistState(state: {
  endpoint: string;
  selectedModel: string;
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
const endpoint = ref(userSettingsStore.userSettings.meridian?.aiPanel?.endpointUrl || persisted.endpoint || 'http://localhost:9777/api/text');
const selectedModel = ref(userSettingsStore.userSettings.meridian?.aiPanel?.model || persisted.selectedModel || '');
const models = ref<Array<{ id: string }>>([]);
const modelsLoaded = ref(false);
// Fix Omnix-on-boot (issue: 'toggle OFF / Offline / No models loaded' on fresh install).
//
// The localStorage fallback `?? persisted.useOmnix ?? true` is removed
// here. The previous chain treated the `meridian-ai-panel` localStorage
// entry's `useOmnix` boolean as a parallel source of truth, but that
// localStorage lives in the WebView2 store (LOCALAPPDATA\com.meridian.app\
// EBWebView\) which is NOT cleared by the Rust-side user-data wipe the
// rest of Meridian uses — and it can override the Pinia default
// (`omnixEnabled: true`) on first paint before the lazy-store hydration
// runs. The newer chain reads only from the Pinia store, and the two
// force-setting migrations (22 → 23 + 27 → 28 in
// `schemas/user-settings.ts`) guarantee `true` on every install path,
// including ones that migrated from an older `false`. `persisted` is
// still imported and persists `endpoint` + `selectedModel`, so the
// removal is a single-line drop with no other call-site impact.
// Phase-11 pivot: Lemonade is the new Tier-1 backend. `omnixEnabled` defaults
// to false on fresh installs (initial defaults object in
// storage/user-settings.ts); the 31->32 schema migration demotes any
// existing install that was force-set ON via the legacy 22->23 / 27->28
// migrations. `?? false` matches that source of truth so the Pinia fallback
// at first paint agrees with the persisted user setting without needing
// a localStorage bootstrap round-trip.
const useOmnix = ref(userSettingsStore.userSettings.meridian?.aiPanel?.omnixEnabled ?? false);
const omnixOnline = ref(false);
const omnixPath = ref(userSettingsStore.userSettings.meridian?.aiPanel?.omnixPath || 'E:\\ai\\Apps\\Omnix');
// Default to Lemonade on port 11434 (Lemonade's upstream `LEMONADE_PORT`
// env default; matches `backend_manager.rs::BackendKind::Lemonade::default_port()`).
// The 34→35 schema migration in schemas/user-settings.ts rewrites any
// existing 13305 sentinel back to 11434 so this fallback rarely fires for
// users upgrading past the post-pivot hotfix. Falls back to this URL only
// when the persisted user setting is missing entirely (fresh install with
// no migration history yet).
const routerEndpoint = ref(userSettingsStore.userSettings.meridian?.aiPanel?.routerEndpoint || 'http://localhost:11434/v1');
const ttsEnabled = ref(userSettingsStore.userSettings.meridian?.aiPanel?.ttsEnabled ?? false);
const routerOnline = ref(false);
const systemPrompt = ref(userSettingsStore.userSettings.meridian?.aiPanel?.systemPrompt || 'You are Rain, a direct and slightly dry companion built into Meridian. You help the user navigate, organize, search, and manage their files. Current directory: {current_path}. Selected files: {selected_files}. Talk like a knowledgeable colleague would — concise, practical, a little dry when it matters. Have opinions (e.g. offer to clean up a messy folder name). CRITICAL: You are Rain. Never break character. Never say you are an AI, an assistant, a language model, or a bot. Never say "As an AI", "I am just an AI", "the assistant", "Certainly!", "Of course!", "Absolutely!", or "Great question!". Just talk like Rain. Format your responses clearly. Use bullet points for lists. Use short paragraphs not walls of text. Bold important terms. Keep responses scannable.');
const temperature = ref(userSettingsStore.userSettings.meridian?.aiPanel?.temperature ?? 0.7);
const maxTokens = ref(userSettingsStore.userSettings.meridian?.aiPanel?.maxTokens ?? 1024);
const topP = ref(userSettingsStore.userSettings.meridian?.aiPanel?.topP ?? 1);
const currentPath = ref('');
const selectedFiles = ref<string[]>([]);
// Search scope: where Rain looks when asked to search.
// 'current' = active folder, 'all' = all drives, or a specific drive path.
const searchScope = ref<string>('current');
// Rain agent memory files (Phase 8): loaded from disk, injected into prompt.
const soulText = ref('');
const memoryText = ref('');
const favoritesText = ref('');
// Onboarding state (universal flow).
const onboardingComplete = ref(userSettingsStore.userSettings.meridian?.aiPanel?.onboardingComplete ?? false);
const onboardingStep = ref(userSettingsStore.userSettings.meridian?.aiPanel?.onboardingStep ?? 'intro');
const connectionMode = ref<AiPanelConnectionMode>(userSettingsStore.userSettings.meridian?.aiPanel?.connectionMode ?? 'basic');
const apiProvider = ref<AiPanelProviderId>(userSettingsStore.userSettings.meridian?.aiPanel?.apiProvider ?? 'custom');
// Onboarding flow writes both `routerEndpoint` and `localEndpointUrl` from the
// same input (see setLocalEndpoint below). The runtime fallback keeps them in
// lockstep so the first paint of the AI panel matches the chosen backend.
// Lemonade listens on 11434 (LEMONADE_PORT default), NOT 13305; 34→35 migration
// rewrites the bad sentinel for existing installs.
const localEndpointUrl = ref(userSettingsStore.userSettings.meridian?.aiPanel?.localEndpointUrl ?? 'http://localhost:11434/v1');
const apiKeyTemp = ref(userSettingsStore.userSettings.meridian?.aiPanel?.apiKeyTemp ?? '');
let onboardingSkipped = false;
let memoryLoaded = false;
let hasGreetedThisSession = false;

  async function loadMemory() {
    if (memoryLoaded) return;
    try {
      const mem = await invoke<{ soul: string; memory: string; favorites: string }>('rain_load_memory');
      soulText.value = mem.soul ?? '';
      memoryText.value = mem.memory ?? '';
      favoritesText.value = mem.favorites ?? '';
      memoryLoaded = true;
    }
    catch (error) {
      console.error('Failed to load Rain memory:', error);
    }
  }

  async function appendMemory(entry: string) {
    try {
      await invoke('rain_append_memory', { entry });
      memoryText.value += `\n- ${entry}`;
    }
    catch (error) {
      console.error('Failed to append Rain memory:', error);
    }
  }

  async function appendFavorite(entry: string) {
    try {
      await invoke('rain_append_favorite', { entry });
    }
    catch (error) {
      console.error('Failed to append Rain favorite:', error);
    }
  }

  // Rain's opening lines — warm, short, never breaks character.
  const GREETINGS = [
    "Hey, I'm Rain. What are we working on today?",
    'Rain here. What do you need?',
    'Hey! Ready when you are.',
    "Rain. What's the plan?",
    "Hey, it's Rain. Where do you want to start?",
  ];

  const canSend = computed(() => input.value.trim().length > 0 && !isLoading.value);

  const ONBOARDING_STEPS = [
    'Set your download folder',
    'Configure the Local AI server endpoint',
    'Add SSH connections',
    'Done!',
  ];
  const currentOnboardingStep = ref<number>(0);

  function startOnboarding() {
    onboardingStep.value = 'intro';
    const introMsg = "Hey, I'm Rain. I'm built into Meridian to help you manage your files. I can work right now with basic features, or connect to an AI model for smarter responses. What sounds right?";
    messages.value.push({ role: 'assistant', content: introMsg });
  }

  function chooseConnectionMode(mode: AiPanelConnectionMode) {
    connectionMode.value = mode;
    if (mode === 'local') {
      onboardingStep.value = 'local';
      const msg = "What's your endpoint URL?";
      messages.value.push({ role: 'assistant', content: msg });
    }
    else if (mode === 'api') {
      onboardingStep.value = 'api';
      const msg = "Choose your provider and paste your API key. It will be stored securely.";
      messages.value.push({ role: 'assistant', content: msg });
    }
    else {
      onboardingStep.value = 'downloadFolder';
      setUseOmnix(true);
      const msg = "No problem — I'll use my built-in engine for now. You can always add a model later in Settings.";
      messages.value.push({ role: 'assistant', content: msg });
      pushDownloadFolderStep();
    }
  }

  function setLocalEndpoint(value: string) {
    localEndpointUrl.value = value;
    routerEndpoint.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.localEndpointUrl = value;
    userSettingsStore.userSettings.meridian.aiPanel.routerEndpoint = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.localEndpointUrl', value);
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.routerEndpoint', value);
    void fetchModels();
    onboardingStep.value = 'downloadFolder';
    pushDownloadFolderStep();
  }

  function setApiProvider(provider: AiPanelProviderId) {
    apiProvider.value = provider;
    userSettingsStore.userSettings.meridian.aiPanel.apiProvider = provider;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.apiProvider', provider);
    if (provider !== 'custom') {
      const baseUrl = AI_PANEL_PROVIDER_URLS[provider];
      routerEndpoint.value = baseUrl;
      userSettingsStore.userSettings.meridian.aiPanel.routerEndpoint = baseUrl;
      userSettingsStore.setUserSettingsStorage('meridian.aiPanel.routerEndpoint', baseUrl);
    }
  }

  async function saveApiKeyAndProceed(key: string) {
    apiKeyTemp.value = '';
    await invoke('secure_store_api_key', { provider: apiProvider.value, key });
    onboardingStep.value = 'downloadFolder';
    pushDownloadFolderStep();
  }

  function pushDownloadFolderStep() {
    const detectedFolder = userSettingsStore.userSettings.meridian?.downloader?.autoSaveFolder || '';
    const msg = `Where should downloads go?\n${detectedFolder || 'Not detected'}`;
    messages.value.push({ role: 'assistant', content: msg });
  }

  function setDownloadFolderInOnboarding(value: string) {
    userSettingsStore.userSettings.meridian.downloader.autoSaveFolder = value;
    userSettingsStore.setUserSettingsStorage('meridian.downloader.autoSaveFolder', value);
    onboardingStep.value = 'done';
    completeOnboarding();
  }

  function completeOnboarding() {
    onboardingComplete.value = true;
    userSettingsStore.userSettings.meridian.aiPanel.onboardingComplete = true;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.onboardingComplete', true);
    messages.value.push({ role: 'assistant', content: "You're all set. Ask me anything." });
  }

  function skipOnboarding() {
    completeOnboarding();
  }

  function maybeGreet() {
    if (hasGreetedThisSession || messages.value.length > 0) {
      return;
    }
    hasGreetedThisSession = true;
    // First-run onboarding check
    if (!onboardingComplete.value && !onboardingSkipped) {
      startOnboarding();
      return;
    }
    const greeting = GREETINGS[Math.floor(Math.random() * GREETINGS.length)];
    messages.value.push({ role: 'assistant', content: greeting });
  }

  function open() { isOpen.value = true; void loadMemory(); maybeGreet(); }
  function close() { isOpen.value = false; }
  function toggle() { isOpen.value ? close() : open(); }
  function setInput(value: string) { input.value = value; }
  function setEndpoint(value: string) {
    endpoint.value = value;
    modelsLoaded.value = false;
    models.value = [];
    userSettingsStore.userSettings.meridian.aiPanel.endpointUrl = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.endpointUrl', value);
    persistState({ endpoint: value, selectedModel: selectedModel.value });
  }
  function setSelectedModel(value: string) {
    selectedModel.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.model = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.model', value);
    persistState({ endpoint: endpoint.value, selectedModel: value });
  }
  function setUseOmnix(value: boolean) {
    useOmnix.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.omnixEnabled = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.omnixEnabled', value);
    persistState({ endpoint: endpoint.value, selectedModel: selectedModel.value });
    // Auto-start / stop the Omnix engine to match the toggle (Step 4).
    if (value) {
      invoke('spawn_omnix', { omnixPath: omnixPath.value || null })
        .catch((error) => { console.error('Failed to start Omnix:', error); });
    }
    else {
      invoke('kill_omnix').catch(() => { /* ignore */ });
    }
  }
  function setOmnixOnline(value: boolean) { omnixOnline.value = value; }
  function setOmnixPath(value: string) {
    omnixPath.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.omnixPath = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.omnixPath', value);
  }
  function setRouterEndpoint(value: string) {
    routerEndpoint.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.routerEndpoint = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.routerEndpoint', value);
  }
  function setTtsEnabled(value: boolean) {
    ttsEnabled.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.ttsEnabled = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.ttsEnabled', value);
  }
  function setRouterOnline(value: boolean) { routerOnline.value = value; }
  function setSystemPrompt(value: string) {
    systemPrompt.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.systemPrompt = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.systemPrompt', value);
  }
  function setTemperature(value: number) {
    temperature.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.temperature = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.temperature', value);
  }
  function setMaxTokens(value: number) {
    maxTokens.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.maxTokens = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.maxTokens', value);
  }
  function setTopP(value: number) {
    topP.value = value;
    userSettingsStore.userSettings.meridian.aiPanel.topP = value;
    userSettingsStore.setUserSettingsStorage('meridian.aiPanel.topP', value);
  }
  function setCurrentPath(path: string) { currentPath.value = path; }
  function setSelectedFiles(files: string[]) { selectedFiles.value = files; }
  function setSearchScope(scope: string) { searchScope.value = scope; }
  function addMessage(role: 'user' | 'assistant', content: string) { messages.value.push({ role, content }); }
  function clearMessages() { messages.value = []; }
  function setLoading(value: boolean) { isLoading.value = value; }
  function setModels(value: Array<{ id: string }>) {
    models.value = value;
    modelsLoaded.value = true;
  }

  async function fetchModels() {
    const baseUrl = (routerEndpoint.value || '').replace(/\/+$/, '');
    if (!baseUrl) return;
    try {
      const modelsUrl = baseUrl.endsWith('/v1') ? `${baseUrl}/models` : `${baseUrl}/v1/models`;
    const response = await fetch(modelsUrl);
      if (!response.ok) return;
      const data = await response.json();
      const modelList = data.data ?? data.models ?? [];
      if (Array.isArray(modelList)) {
        setModels(
          modelList
            .filter((item: { id?: string }) => typeof item.id === 'string')
            .map((item: { id: string; context_length?: number; context_window?: number; max_context?: number }) => ({
              id: item.id,
              contextWindow: item.context_length ?? item.context_window ?? item.max_context,
            })),
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
      if (open && !modelsLoaded.value) {
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

  // Fix Omnix-on-boot (continued): `spawn_omnix` was only reached
  // inside `setUseOmnix(true)` — i.e. after the user manually clicked
  // the toggle. On a freshly installed Meridian the Pinia default
  // resolves `useOmnix = true` at first paint but no click ever fires,
  // so the bundled Omnix lives at `E:\ai\Apps\Omnix\` (extracted by
  // `resolve_omnix_dir`), its `npm install` never runs, its Electron
  // process never spawns, and `get_omnix_status` polls
  // `http://localhost:9777/api/health` against nothing → permanent
  // "Offline / No models loaded". Firing `spawn_omnix` here, once at
  // store construction, makes Rain's "zero-config" promise hold. The
  // Rust side guards against duplicate spawns via the `OMNIX_CHILD`
  // static, so re-entry on second store construction (e.g. view
  // remount) is safe and cheap.
  if (useOmnix.value) {
    invoke('spawn_omnix', { omnixPath: omnixPath.value || null })
      .catch((error: unknown) => {
        console.error('Boot-time spawn_omnix failed:', error);
      });
  }

  return {
    isOpen, isLoading, messages, input, endpoint, selectedModel, models, modelsLoaded,
    useOmnix, omnixOnline, omnixPath, routerEndpoint, ttsEnabled, routerOnline, currentPath, selectedFiles, searchScope, canSend,
    systemPrompt, temperature, maxTokens, topP,
    soulText, memoryText, favoritesText, loadMemory, appendMemory, appendFavorite,
    open, close, toggle, setInput, setEndpoint, setSelectedModel, setUseOmnix,
    setOmnixOnline, setOmnixPath, setRouterEndpoint, setTtsEnabled, setRouterOnline, setCurrentPath, setSelectedFiles, setSearchScope, addMessage, clearMessages,
    setSystemPrompt, setTemperature, setMaxTokens, setTopP,
    setLoading, setModels, fetchModels,
    skipOnboarding, completeOnboarding, onboardingComplete, onboardingStep, connectionMode, apiProvider,
    localEndpointUrl, apiKeyTemp, chooseConnectionMode, setLocalEndpoint, setApiProvider, saveApiKeyAndProceed,
    setDownloadFolderInOnboarding,
  };
});
