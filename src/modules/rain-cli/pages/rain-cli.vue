<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
/**
 * Rain CLI — a Codebuff-style terminal-mode interface for Rain.
 *
 * Features:
 *   - Terminal-style output (dark bg, monospace, scrollable)
 *   - Copy-on-hover button on every block (bottom-right, inline)
 *   - Numbered thinking steps
 *   - Tool call cards (collapsible)
 *   - Follow-up suggestions after each response
 *   - Streaming text animation (char-by-char + blink cursor)
 *   - File diffs (green +red/-green)
 *   - Markdown rendering (bold, lists, links, code blocks)
 *   - Turn threading (spacing, horizontal rules, left-border accent)
 *   - Single prompt input line at bottom
 *   - Shared agent loop with ai-panel.vue via aiPanelStore
 *   - Shared conversation state with slide-in overlay
 */

import { computed, ref, nextTick, watch, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useAiPanelStore } from '@/stores/runtime/ai-panel';
import {
  BotIcon,
  SendIcon,
  LoaderCircleIcon,
  ClipboardIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  XIcon,
  HardDriveIcon,
  NetworkIcon,
  UsbIcon,
} from '@lucide/vue';
import { invoke } from '@tauri-apps/api/core';
import { marked } from 'marked';
import { useDrives } from '@/modules/home/composables/use-drives';
import toReadableBytes from '@/utils/to-readable-bytes';
import type { DriveInfo } from '@/types/drive-info';
import UbuntuWslIcon from '@/components/icons/ubuntu-wsl-icon.vue';

const { t } = useI18n();
const aiPanelStore = useAiPanelStore();

// ─── State ────────────────────────────────────────────────────────────────

const { drives } = useDrives();

function getDriveIcon(drive: DriveInfo) {
  if (drive.drive_type === 'Network') return NetworkIcon;
  return drive.is_removable ? UsbIcon : HardDriveIcon;
}

// ─── Markdown renderer (Phase A) ─────────────────────────────────────────

marked.setOptions({ breaks: true, gfm: true });

function renderCliMarkdown(text: string): string {
  try {
    return marked.parse(text ?? '') as string;
  } catch {
    return text ?? '';
  }
}

const outputRef = ref<HTMLElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);
const inputValue = ref('');
const isLoading = ref(false);
const streamingTexts = ref<Map<number, string>>(new Map());
const streamingCursors = ref<Map<number, boolean>>(new Map());
const expandedToolCalls = ref<Set<number>>(new Set());
const copiedBlockId = ref<number | null>(null);
const errorMessage = ref<string>('');

// Conversation history stored as structured blocks
interface CliMessage {
  id: number;
  role: 'user' | 'assistant' | 'tool' | 'thinking';
  content: string;
  toolCalls?: Array<{ id: string; name: string; args: string; result: string }>;
  thinkingSteps?: string[];
  followUps?: string[];
  isStreaming?: boolean;
  diffBlocks?: Array<{ type: 'add' | 'remove' | 'context'; text: string }>;
}

let msgCounter = 0;
const messages = ref<CliMessage[]>([]);
// Tracks whether we've already waited for Omnix once (so retries use a shorter timeout)
let OMNIX_SPAWN_WAITED = false;

// ─── Agent loop helpers (mirrors ai-panel.vue) ───────────────────────────

const DESTRUCTIVE = new Set(['move_files', 'rename_item', 'delete_item', 'write_file', 'run_shell_command']);

/**
 * Run a full agent loop: POST to the local AI server, handle tool calls,
 * stream results back as structured blocks.
 */
async function runAgentLoop(
  routerBase: string,
  model: string | undefined,
  systemPrompt: string,
  prompt: string,
): Promise<string> {
  // Include previous conversation for memory across turns
  const historyMessages = aiPanelStore.messages.map(m => ({
    role: m.role,
    content: m.content,
  }));
  const msgs: Array<Record<string, unknown>> = [
    { role: 'system', content: systemPrompt },
    ...historyMessages,
    { role: 'user', content: prompt },
  ];

  let tools: unknown[] = [];
  try {
    tools = JSON.parse(await invoke<string>('rain_tool_schemas'));
  } catch {
    tools = [];
  }

  const MAX_ITERATIONS = 10;

  for (let i = 0; i < MAX_ITERATIONS; i++) {
    const chatUrl = routerBase.endsWith('/v1') ? `${routerBase}/chat/completions` : `${routerBase}/v1/chat/completions`;
    const res = await fetch(chatUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', ...(model ? { 'X-Model-Id': model } : {}) },
      body: JSON.stringify({
        model: model || 'default',
        messages: msgs,
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

    // No tool calls → final answer.
    if (!toolCalls || toolCalls.length === 0) {
      return choice.content ?? '';
    }

    // Record tool calls as a tool block
    msgs.push({ role: 'assistant', content: choice.content ?? '', tool_calls: toolCalls });

    for (const call of toolCalls) {
      const name = call.function?.name ?? '';
      let args: Record<string, unknown> = {};
      try { args = JSON.parse(call.function?.arguments || '{}'); } catch { args = {}; }

      let resultJson: string;
      let confirmation = 'immediate';
      if (DESTRUCTIVE.has(name)) {
        const confirmed = await requestCliConfirmation(name, args);
        if (confirmed) {
          confirmation = 'confirmed';
          resultJson = await executeCliTool(name, args);
        } else {
          confirmation = 'cancelled';
          resultJson = JSON.stringify({ ok: false, cancelled: true, error: 'User cancelled.' });
        }
      } else if (name === 'search_files') {
        resultJson = await runCliSearch(args);
      } else {
        resultJson = await invoke<string>('rain_run_tool', { name, args });
      }

      // Add tool call block to messages
      const toolMsg: CliMessage = {
        id: ++msgCounter,
        role: 'tool',
        content: '',
        toolCalls: [{
          id: call.id,
          name,
          args: JSON.stringify(args, null, 2),
          result: resultJson,
        }],
      };
      messages.value.push(toolMsg);

      msgs.push({ role: 'tool', tool_call_id: call.id, content: resultJson });

      // Log tool call
      try {
        let outcome = resultJson;
        if (outcome && outcome.length > 2000) outcome = outcome.slice(0, 2000);
        await invoke('rain_log_tool_call', {
          tool: name,
          args: JSON.stringify(args),
          outcome,
          confirmation,
        });
      } catch { /* best-effort */ }
    }
  }

  return 'Stopped after max tool steps. Ask me to continue.';
}

// Confirmation state for CLI
const cliConfirmData = ref<{
  tool: string;
  title: string;
  lines: string[];
  warning?: string;
  resolve: (value: boolean) => void;
} | null>(null);

function requestCliConfirmation(name: string, args: Record<string, unknown>): Promise<boolean> {
  let title: string;
  let lines: string[];
  let warning: string | undefined;

  if (name === 'move_files') {
    const srcs = Array.isArray(args.src) ? (args.src as string[]) : [String(args.src)];
    title = t('aiPanel.confirmMoveTitle');
    lines = srcs.map(s => `${s}  →  ${String(args.dest)}`);
  } else if (name === 'rename_item') {
    title = t('aiPanel.confirmRenameTitle');
    lines = [`${String(args.old)}  →  ${String(args.new)}`];
  } else if (name === 'write_file') {
    title = 'Write file';
    lines = [`Path: ${String(args.path)}`, `Content: ${(String(args.content).slice(0, 200))}${String(args.content).length > 200 ? '...' : ''}`];
  } else if (name === 'run_shell_command') {
    title = 'Run shell command';
    lines = [`$ ${String(args.command)}`];
    warning = 'Shell commands can modify system state. Only confirm if you trust this command.';
  } else {
    // delete_item
    const permanent = args.permanent === true;
    title = permanent ? t('aiPanel.confirmDeletePermanentTitle') : t('aiPanel.confirmDeleteTitle');
    lines = [String(args.path)];
    if (permanent) warning = t('aiPanel.confirmDeleteFolderWarning', { count: 1 }) || 'This will permanently delete the item.';
  }

  return new Promise<boolean>((resolve) => {
    cliConfirmData.value = { tool: name, title, lines, warning, resolve };
  });
}

function resolveCliConfirm(value: boolean) {
  if (cliConfirmData.value) {
    cliConfirmData.value.resolve(value);
    cliConfirmData.value = null;
  }
}

async function executeCliTool(name: string, args: Record<string, unknown>): Promise<string> {
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
    if (name === 'write_file') {
      return await invoke<string>('rain_write_file', { path: String(args.path), content: String(args.content) });
    }
    if (name === 'run_shell_command') {
      return await invoke<string>('rain_run_shell_command', {
        command: String(args.command),
        timeoutSecs: (args.timeout_secs as number) ?? 30,
      });
    }
  } catch (error) {
    return JSON.stringify({ ok: false, error: error instanceof Error ? error.message : String(error) });
  }
  return JSON.stringify({ ok: false, error: `Unknown tool: ${name}` });
}

async function runCliSearch(args: Record<string, unknown>): Promise<string> {
  try {
    const query = String(args.query ?? '').trim();
    if (!query) return JSON.stringify({ ok: false, error: 'Empty search query.' });
    const matches = await invoke<Array<{ name: string; path: string; is_dir?: boolean }>>('global_search_query', {
      query,
      options: { limit: 50 },
    });
    const results = (matches || []).slice(0, 50).map(m => ({ name: m.name, path: m.path, is_dir: m.is_dir ?? false }));
    return JSON.stringify({ ok: true, query, count: results.length, results });
  } catch (error) {
    return JSON.stringify({ ok: false, error: error instanceof Error ? error.message : String(error) });
  }
}

// ─── Send handler ─────────────────────────────────────────────────────────

async function handleSend() {
  const prompt = inputValue.value.trim();
  if (!prompt || isLoading.value) return;

  inputValue.value = '';
  isLoading.value = true;
  errorMessage.value = '';

  // Add user message to both local (for rich UI) and store (for shared state)
  const userMsg: CliMessage = { id: ++msgCounter, role: 'user', content: prompt };
  messages.value.push(userMsg);
  aiPanelStore.addMessage('user', prompt);

  // Add assistant placeholder for streaming
  const asstId = ++msgCounter;
  const asstMsg: CliMessage = { id: asstId, role: 'assistant', content: '', isStreaming: true };
  messages.value.push(asstMsg);

  await nextTick();
  scrollToBottom();

  try {
    const routerBase = (aiPanelStore.routerEndpoint || '').replace(/\/+$/, '');
    const model = aiPanelStore.selectedModel || undefined;
    const currentPath = aiPanelStore.currentPath;
    const scope = aiPanelStore.searchScope || 'current';
    const scopeText = scope === 'current'
      ? `the current folder (${currentPath || 'unknown'})`
      : scope === 'all' ? 'all drives on this machine' : `the drive ${scope}`;

    const systemPrompt = (aiPanelStore.systemPrompt || '')
      .replace(/\{current_path\}/g, currentPath || '')
      .replace(/\{selected_files\}/g, 'none')
      .replace(/\{search_scope\}/g, scopeText)
      + `\n\nWhen searching for files, search ${scopeText} unless the user says otherwise.`
      + (aiPanelStore.soulText ? `\n\n=== SOUL (your identity) ===\n${aiPanelStore.soulText}` : '')
      + (aiPanelStore.memoryText ? `\n\n=== MEMORY (what you've learned) ===\n${aiPanelStore.memoryText}` : '')
      + (aiPanelStore.favoritesText ? `\n\n=== FAVORITES (noticed preferences) ===\n${aiPanelStore.favoritesText}` : '');

    // ── Omnix-first with startup wait ──────────────────────────────────
    // If Omnix is enabled but not yet online, start it and wait.
    // On first launch, npm install can take 60-120s, so we wait up to 120s
    // with real-time progress updates. On subsequent messages (engine already
    // spawned), we silently wait 30s — the background process is likely
    // almost ready.
    const OMNIX_FIRST_LAUNCH_TIMEOUT = 120;
    const OMNIX_RETRY_TIMEOUT = 30;
    let omnixTimedOut = false;

    if (aiPanelStore.useOmnix && !aiPanelStore.omnixOnline) {
      console.debug('[rain-cli] Omnix offline — attempting spawn...');
      try {
        await invoke('spawn_omnix', { omnixPath: aiPanelStore.omnixPath || null });
        console.debug('[rain-cli] spawn_omnix returned Ok');
      } catch (spawnErr) {
        console.error('[rain-cli] spawn_omnix failed:', spawnErr);
      }

      // Determine timeout: long on first launch, shorter on retries
      const waitSecs = OMNIX_SPAWN_WAITED ? OMNIX_RETRY_TIMEOUT : OMNIX_FIRST_LAUNCH_TIMEOUT;
      OMNIX_SPAWN_WAITED = true;

      // Show live progress while waiting (only on first launch)
      if (waitSecs > 30) asstMsg.content = 'Starting Omnix engine...';

      for (let s = 0; s < waitSecs; s++) {
        await new Promise(r => setTimeout(r, 1000));
        try {
          const online = await invoke<boolean>('get_omnix_status');
          if (online) {
            console.debug(`[rain-cli] Omnix online after ${s + 1}s`);
            aiPanelStore.setOmnixOnline(true);
            break;
          }
        } catch (statusErr) {
          console.debug('[rain-cli] get_omnix_status error:', statusErr);
        }
        // Update progress message periodically
        if (waitSecs > 30) {
          const elapsed = s + 1;
          if (elapsed === 5) asstMsg.content = 'Starting Omnix engine... (first launch may take a minute)';
          else if (elapsed === 20) asstMsg.content = 'Still loading Omnix engine... (20s)';
          else if (elapsed === 40) asstMsg.content = 'Still loading Omnix engine... (40s)';
          else if (elapsed === 60) asstMsg.content = 'Still loading Omnix engine... this is taking longer than usual (60s)';
          else if (elapsed === 90) asstMsg.content = 'Almost there... (90s)';
        }
      }
      if (!aiPanelStore.omnixOnline) {
        omnixTimedOut = true;
        console.warn(`[rain-cli] Omnix did not come online within ${waitSecs}s`);
      }
    }

    const isRouterExplicit = routerBase && aiPanelStore.connectionMode !== 'basic';
    let finalText: string;

    // Three-path architecture:
    // 1. Omnix online → direct text inference (always works, zero config)
    // 2. Router explicitly configured → agent loop with tool calling
    // 3. Neither → helpful message, never a raw fetch error
    if (aiPanelStore.useOmnix && aiPanelStore.omnixOnline) {
      finalText = await invoke<string>('omnix_text', {
        prompt: `${systemPrompt}\n\nUser: ${prompt}`,
        systemPrompt,
        temperature: aiPanelStore.temperature,
        maxTokens: aiPanelStore.maxTokens,
        topP: aiPanelStore.topP,
      });
    }
    else if (isRouterExplicit) {
      try {
        finalText = await runAgentLoop(routerBase, model, systemPrompt, prompt);
      } catch {
        // Router is dead — try one last Omnix check before giving up
        try {
          const online = await invoke<boolean>('get_omnix_status');
          if (online) {
            aiPanelStore.setOmnixOnline(true);
            finalText = await invoke<string>('omnix_text', {
              prompt: `${systemPrompt}\n\nUser: ${prompt}`,
              systemPrompt,
              temperature: aiPanelStore.temperature,
              maxTokens: aiPanelStore.maxTokens,
              topP: aiPanelStore.topP,
            });
          } else {
            finalText = 'Could not reach your AI server. Download Lemonade from Backend Manager (the default Tier-1 backend), or enable Omnix in Settings for the legacy Electron fallback.';
          }
        } catch {
          finalText = 'Could not reach your AI server. Download Lemonade from Backend Manager (the default Tier-1 backend), or enable Omnix in Settings for the legacy Electron fallback.';
        }
      }
    }
    else {
      finalText = aiPanelStore.useOmnix
        ? (omnixTimedOut
            ? 'Rain is warming up. Hang tight, I\'ll try again in a moment...'
            : 'Omnix is starting up. Give me a moment and try again.')
        : aiPanelStore.routerOnline
          ? 'No AI endpoint is configured. Download Lemonade from Backend Manager to start with a local model, or enable Omnix in Settings.'
          : 'No AI endpoint is configured. Open Backend Manager to install Lemonade (the default Tier-1 backend), or enable Omnix in Settings.';
    }

    // Stream the final text character by character
    asstMsg.isStreaming = true;
    for (let i = 0; i <= finalText.length; i++) {
      asstMsg.content = finalText.slice(0, i);
      if (i < finalText.length) {
        await new Promise(r => setTimeout(r, 8 + Math.random() * 12));
      }
      await nextTick();
      scrollToBottom();
    }
    asstMsg.isStreaming = false;

    // Mirror final text to shared store so slide-in panel sees the same conversation
    aiPanelStore.addMessage('assistant', finalText);

    // Add follow-up suggestions
    asstMsg.followUps = generateFollowups(finalText);

    // Memory extraction (best-effort)
    extractMemory(prompt, finalText);

  } catch (error) {
    const errMsg = error instanceof Error ? error.message : 'Unknown error';
    errorMessage.value = errMsg;
    // Update the assistant message with the error
    const lastMsg = messages.value[messages.value.length - 1];
    if (lastMsg && lastMsg.role === 'assistant') {
      lastMsg.content = `Error: ${errMsg}`;
      lastMsg.isStreaming = false;
    }
  } finally {
    isLoading.value = false;
    await nextTick();
    scrollToBottom();
    inputRef.value?.focus();
  }
}

function generateFollowups(text: string): string[] {
  const suggestions: string[] = [];
  const lower = text.toLowerCase();

  if (lower.includes('file') || lower.includes('folder') || lower.includes('directory')) {
    suggestions.push('Show me what else is in this folder');
    suggestions.push('Search for more files like this');
  }
  if (lower.includes('model') || lower.includes('gpu') || lower.includes('download')) {
    suggestions.push('Check my hardware specs');
    suggestions.push('Browse trending models on HuggingFace');
  }
  if (lower.includes('error') || lower.includes('failed') || lower.includes('issue')) {
    suggestions.push('Show me the error details');
    suggestions.push('Try running it again');
  }
  if (lower.includes('disk') || lower.includes('space') || lower.includes('storage')) {
    suggestions.push('Show drive usage breakdown');
    suggestions.push('Clean up temporary files');
  }

  suggestions.push('What else can you help me with?');
  suggestions.push('Clear this conversation');

  // Keep max 4
  return suggestions.slice(0, 4);
}

async function extractMemory(prompt: string, finalText: string) {
  try {
    const routerBase = (aiPanelStore.routerEndpoint || '').replace(/\/+$/, '');
    if (!routerBase) return;

    const extractionPrompt = `You are Rain's memory extractor. Given the latest exchange, decide if there is ONE durable fact worth remembering long-term. If yes, reply with "MEMORY:" or "FAVORITE:" followed by the fact. If nothing worth saving, reply "NONE".\n\nUser: ${prompt}\nRain: ${finalText}`;

    const chatUrl = routerBase.endsWith('/v1') ? `${routerBase}/chat/completions` : `${routerBase}/v1/chat/completions`;
    const res = await fetch(chatUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        model: aiPanelStore.selectedModel || 'default',
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
    } else if (favMatch) {
      await aiPanelStore.appendFavorite(favMatch[1].trim());
    }
  } catch { /* best-effort */ }
}

// ─── Copy handler ─────────────────────────────────────────────────────────

async function copyBlock(id: number, text: string) {
  try {
    await navigator.clipboard.writeText(text);
    copiedBlockId.value = id;
    setTimeout(() => { if (copiedBlockId.value === id) copiedBlockId.value = null; }, 1500);
  } catch { /* clipboard error */ }
}

// ─── Follow-up click ──────────────────────────────────────────────────────

function handleFollowUp(text: string) {
  if (text === 'Clear this conversation') {
    messages.value = [];
    msgCounter = 0;
    errorMessage.value = '';
    inputRef.value?.focus();
    return;
  }
  inputValue.value = text;
  nextTick(() => handleSend());
}

// ─── Scroll ───────────────────────────────────────────────────────────────

function scrollToBottom() {
  if (outputRef.value) {
    outputRef.value.scrollTop = outputRef.value.scrollHeight;
  }
}

// ─── Keyboard ─────────────────────────────────────────────────────────────

function handleKeyDown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault();
    handleSend();
  }
}

// ─── Lifecycle ────────────────────────────────────────────────────────────

onMounted(() => {
  // If no messages yet, show a greeting
  if (messages.value.length === 0) {
    const greetings = [
      "Hey, I'm Rain. Terminal mode active. What are we working on?",
      'Rain CLI ready. Ask me anything — files, shell commands, research.',
      'Hey! Rain here in CLI mode. What do you need?',
    ];
    const greeting = greetings[Math.floor(Math.random() * greetings.length)];
    messages.value.push({ id: ++msgCounter, role: 'assistant', content: greeting });
    messages.value[0].followUps = [
      'Show me my system specs',
      'Search for files in my downloads',
      'What can you do in CLI mode?',
    ];
  }
  inputRef.value?.focus();
});

// ─── Parsed content blocks ───────────────────────────────────────────────

function parseDiffBlocks(text: string): Array<{ type: 'add' | 'remove' | 'context'; text: string }> {
  const blocks: Array<{ type: 'add' | 'remove' | 'context'; text: string }> = [];
  const lines = text.split('\n');
  for (const line of lines) {
    if (line.startsWith('+') && !line.startsWith('+++')) {
      blocks.push({ type: 'add', text: line });
    } else if (line.startsWith('-') && !line.startsWith('---')) {
      blocks.push({ type: 'remove', text: line });
    } else {
      blocks.push({ type: 'context', text: line });
    }
  }
  return blocks;
}

function hasDiffContent(text: string): boolean {
  return text.includes('\n+') && (text.includes('\n-') || text.includes('--- a/'));
}

function parseThinkingSteps(text: string): string[] {
  const steps: string[] = [];
  const lines = text.split('\n');
  for (const line of lines) {
    const match = line.match(/^(\d+\.)\s(.+)/);
    if (match) {
      steps.push(match[0]);
    }
  }
  return steps;
}

function toggleToolCall(id: number) {
  if (expandedToolCalls.value.has(id)) {
    expandedToolCalls.value.delete(id);
  } else {
    expandedToolCalls.value.add(id);
  }
}

// ─── Omnix health ─────────────────────────────────────────────────────────

const omnixStatusLabel = computed(() => {
  if (!aiPanelStore.useOmnix) return '';
  return aiPanelStore.omnixOnline ? 'Omnix online' : 'Omnix offline';
});

let healthTimer: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  healthTimer = setInterval(() => {
    if (aiPanelStore.useOmnix) {
      invoke<boolean>('get_omnix_status')
        .then(v => aiPanelStore.setOmnixOnline(v))
        .catch(() => aiPanelStore.setOmnixOnline(false));
    }
  }, 5000);
});

onUnmounted(() => {
  if (healthTimer) clearInterval(healthTimer);
});
</script>

<template>
  <div class="rain-cli">
    <!-- Header bar -->
    <div class="rain-cli__header">
      <div class="rain-cli__header-left">
        <BotIcon :size="16" class="rain-cli__header-icon" />
        <span class="rain-cli__header-title">Rain CLI</span>
        <span
          v-if="omnixStatusLabel"
          class="rain-cli__omnix-dot"
          :class="{
            'rain-cli__omnix-dot--online': aiPanelStore.omnixOnline,
            'rain-cli__omnix-dot--offline': !aiPanelStore.omnixOnline,
          }"
        />
      </div>
      <div class="rain-cli__header-right">
        <span class="rain-cli__header-status">
          {{ aiPanelStore.routerEndpoint || 'No endpoint' }}
        </span>
      </div>
    </div>

    <!-- Output area -->
    <div ref="outputRef" class="rain-cli__output">
      <div class="rain-cli__output-inner">
        <!-- Each message block -->
        <div
          v-for="msg in messages"
          :key="msg.id"
          class="rain-cli__block"
          :class="{
            'rain-cli__block--user': msg.role === 'user',
            'rain-cli__block--assistant': msg.role === 'assistant',
            'rain-cli__block--tool': msg.role === 'tool',
            'rain-cli__block--streaming': msg.isStreaming,
            'rain-cli__block--turn-start': msg.role === 'user' && messages.length > 0 && msg.id !== messages[0]?.id,
          }"
        >
          <!-- User message: prompt-style -->
          <template v-if="msg.role === 'user'">
            <div class="rain-cli__block-body">
              <div class="rain-cli__prompt-line">
                <span class="rain-cli__prompt-arrow">></span>
                <span class="rain-cli__prompt-text">{{ msg.content }}</span>
              </div>
              <button
                class="rain-cli__copy-btn"
                :class="{ 'rain-cli__copy-btn--copied': copiedBlockId === msg.id }"
                :title="copiedBlockId === msg.id ? 'Copied!' : 'Copy'"
                @click="copyBlock(msg.id, msg.content)"
              >
                <ClipboardIcon v-if="copiedBlockId !== msg.id" :size="12" />
                <CheckIcon v-else :size="12" class="rain-cli__check-icon" />
              </button>
            </div>
          </template>

          <!-- Assistant message: streaming or full content -->
          <template v-if="msg.role === 'assistant'">
            <div class="rain-cli__block-body">
              <div
                class="rain-cli__output-text"
                :class="{ 'rain-cli__output-text--streaming': msg.isStreaming }"
              >
                <!-- Numbered thinking steps -->
                <div
                  v-for="(step, si) in parseThinkingSteps(msg.content)"
                  :key="si"
                  class="rain-cli__thinking-step"
                >
                  {{ step }}
                </div>

                <!-- If the content has diff markers, render as diff -->
                <div v-if="hasDiffContent(msg.content)" class="rain-cli__diff-block">
                  <div
                    v-for="(line, li) in parseDiffBlocks(msg.content)"
                    :key="li"
                    class="rain-cli__diff-line"
                    :class="{
                      'rain-cli__diff-line--add': line.type === 'add',
                      'rain-cli__diff-line--remove': line.type === 'remove',
                    }"
                  ><span class="rain-cli__diff-marker">{{ line.text.charAt(0) }}</span>{{ line.text.slice(1) }}</div>
                </div>

                <!-- Rendered markdown (Phase A) -->
                <div v-else class="rain-cli__markdown" v-html="renderCliMarkdown(msg.content)" />

                <!-- Streaming cursor -->
                <span v-if="msg.isStreaming" class="rain-cli__cursor">▋</span>
              </div>

              <!-- Copy button (Phase B: inline bottom-right) -->
              <button
                v-if="msg.content && !msg.isStreaming"
                class="rain-cli__copy-btn"
                :class="{ 'rain-cli__copy-btn--copied': copiedBlockId === msg.id }"
                :title="copiedBlockId === msg.id ? 'Copied!' : 'Copy'"
                @click="copyBlock(msg.id, msg.content)"
              >
                <ClipboardIcon v-if="copiedBlockId !== msg.id" :size="12" />
                <CheckIcon v-else :size="12" class="rain-cli__check-icon" />
              </button>

              <!-- Follow-up suggestions -->
              <div v-if="msg.followUps && msg.followUps.length > 0 && !msg.isStreaming" class="rain-cli__followups">
                <div class="rain-cli__followups-label">Suggestions</div>
                <div class="rain-cli__followups-list">
                  <button
                    v-for="(sug, si) in msg.followUps"
                    :key="si"
                    class="rain-cli__followup-btn"
                    @click="handleFollowUp(sug)"
                  >{{ sug }}</button>
                </div>
              </div>
            </div>
          </template>

          <!-- Tool call card -->
          <template v-if="msg.role === 'tool' && msg.toolCalls">
            <div class="rain-cli__block-body">
              <div
                v-for="(tc, ti) in msg.toolCalls"
                :key="ti"
                class="rain-cli__tool-card"
              >
                <div class="rain-cli__tool-card-header" @click="toggleToolCall(msg.id)">
                  <button class="rain-cli__tool-card-toggle">
                    <ChevronDownIcon v-if="expandedToolCalls.has(msg.id)" :size="14" />
                    <ChevronRightIcon v-else :size="14" />
                  </button>
                  <span class="rain-cli__tool-card-name">{{ tc.name }}</span>
                  <span class="rain-cli__tool-card-status">
                    {{ tc.result.startsWith('{"ok":true') || tc.result.includes('"ok": true') ? '✓ done' : (tc.result.includes('cancelled') ? '✕ cancelled' : '⚠ error') }}
                  </span>
                </div>
                <div v-if="expandedToolCalls.has(msg.id)" class="rain-cli__tool-card-body">
                  <div class="rain-cli__tool-card-section">
                    <div class="rain-cli__tool-card-label">Arguments</div>
                    <pre class="rain-cli__tool-card-code">{{ tc.args }}</pre>
                  </div>
                  <div class="rain-cli__tool-card-section">
                    <div class="rain-cli__tool-card-label">Result</div>
                    <pre class="rain-cli__tool-card-code">{{ (() => { try { return JSON.stringify(JSON.parse(tc.result), null, 2); } catch { return tc.result; } })() }}</pre>
                  </div>
                </div>
              </div>

              <!-- Copy button for tool results (Phase B: inline bottom-right) -->
              <button
                class="rain-cli__copy-btn"
                :class="{ 'rain-cli__copy-btn--copied': copiedBlockId === msg.id }"
                @click="copyBlock(msg.id, msg.toolCalls?.[0]?.result ?? '')"
              >
                <ClipboardIcon v-if="copiedBlockId !== msg.id" :size="12" />
                <CheckIcon v-else :size="12" class="rain-cli__check-icon" />
              </button>
            </div>
          </template>
        </div>

        <!-- Loading indicator -->
        <div v-if="isLoading && messages[messages.length - 1]?.isStreaming === false" class="rain-cli__thinking">
          <LoaderCircleIcon :size="14" class="rain-cli__spinner" />
          <span>Thinking...</span>
        </div>

        <!-- Error message -->
        <div v-if="errorMessage" class="rain-cli__error">
          {{ errorMessage }}
        </div>

        <!-- Drive info panel (fills empty space before conversation) -->
        <div v-if="messages.length <= 1" class="rain-cli__drives-section">
          <div class="rain-cli__drives-header">
            <BotIcon :size="16" class="rain-cli__drives-header-icon" />
            <span class="rain-cli__drives-header-label">Rain CLI ready</span>
            <span class="rain-cli__drives-header-hint">— ask about files, commands, or your system</span>
          </div>
          <div class="rain-cli__drives-grid">
            <div
              v-for="drive in drives"
              :key="drive.path"
              class="rain-cli__drive-card"
            >
              <div class="rain-cli__drive-card-icon">
                <UbuntuWslIcon
                  v-if="drive.drive_type === 'WSL'"
                  :size="18"
                />
                <component
                  v-else
                  :is="getDriveIcon(drive)"
                  :size="18"
                />
              </div>
              <div class="rain-cli__drive-card-body">
                <div class="rain-cli__drive-card-top">
                  <span class="rain-cli__drive-card-name">{{ drive.name }}</span>
                  <span class="rain-cli__drive-card-path">{{ drive.path }}</span>
                  <span class="rain-cli__drive-card-type">{{ drive.drive_type }}</span>
                </div>
                <div class="rain-cli__drive-card-space">
                  <span class="rain-cli__drive-card-available">{{ toReadableBytes(drive.available_space, 1) }}</span>
                  <span class="rain-cli__drive-card-free">free</span>
                  <span class="rain-cli__drive-card-sep">/</span>
                  <span class="rain-cli__drive-card-total">{{ toReadableBytes(drive.total_space, 1) }}</span>
                  <span class="rain-cli__drive-card-percent">{{ drive.percent_used }}%</span>
                </div>
                <div class="rain-cli__drive-card-bar">
                  <div
                    class="rain-cli__drive-card-bar-fill"
                    :style="{ width: (drive.percent_used || 0) + '%' }"
                    :class="{
                      'rain-cli__drive-card-bar-fill--warn': drive.percent_used > 85,
                      'rain-cli__drive-card-bar-fill--crit': drive.percent_used > 95,
                    }"
                  />
                </div>
              </div>
            </div>

            <!-- Empty drives hint (when no drives detected) -->
            <div
              v-if="drives.length === 0"
              class="rain-cli__drive-card rain-cli__drive-card--empty"
            >
              <HardDriveIcon :size="18" class="rain-cli__drive-card-icon" />
              <span class="rain-cli__drive-card-empty-text">No drives detected</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Confirmation card (inline, not dialog) -->
    <div v-if="cliConfirmData" class="rain-cli__confirm">
      <div class="rain-cli__confirm-header">Confirm: {{ cliConfirmData.title }}</div>
      <div class="rain-cli__confirm-lines">
        <div v-for="(line, i) in cliConfirmData.lines" :key="i" class="rain-cli__confirm-line">{{ line }}</div>
      </div>
      <div v-if="cliConfirmData.warning" class="rain-cli__confirm-warning">{{ cliConfirmData.warning }}</div>
      <div class="rain-cli__confirm-actions">
        <button class="rain-cli__confirm-cancel" @click="resolveCliConfirm(false)">
          <XIcon :size="14" /> Cancel
        </button>
        <button class="rain-cli__confirm-ok" @click="resolveCliConfirm(true)">
          <CheckIcon :size="14" /> Confirm
        </button>
      </div>
    </div>

    <!-- Input area -->
    <div class="rain-cli__input-area">
      <div class="rain-cli__input-wrap">
        <span class="rain-cli__input-prompt">></span>
        <input
          ref="inputRef"
          v-model="inputValue"
          class="rain-cli__input"
          :placeholder="isLoading ? 'Waiting for Rain...' : 'Ask Rain anything...'"
          :disabled="isLoading"
          autofocus
          @keydown="handleKeyDown"
        />
        <button
          class="rain-cli__send-btn"
          :disabled="!inputValue.trim() || isLoading"
          @click="handleSend"
        >
          <SendIcon v-if="!isLoading" :size="16" />
          <LoaderCircleIcon v-else :size="16" class="rain-cli__spinner" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.rain-cli {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: hsl(var(--background));
  overflow: hidden;
  font-family: var(--font-mono, 'Cascadia Code', 'Fira Code', 'JetBrains Mono', monospace);
}

/* ─── Header ────────────────────────────────────────────────────────────── */

.rain-cli__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 14px;
  border-bottom: 1px solid hsl(var(--border));
  flex-shrink: 0;
  background-color: hsl(var(--background-2));
}

.rain-cli__header-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.rain-cli__header-icon {
  color: hsl(var(--primary));
}

.rain-cli__header-title {
  font-size: 0.8rem;
  font-weight: 600;
  color: hsl(var(--foreground));
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.rain-cli__omnix-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}

.rain-cli__omnix-dot--online {
  background-color: hsl(var(--success));
}

.rain-cli__omnix-dot--offline {
  background-color: hsl(var(--muted-foreground));
}

.rain-cli__header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.rain-cli__header-status {
  font-size: 0.65rem;
  color: hsl(var(--muted-foreground));
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ─── Output area ───────────────────────────────────────────────────────── */

.rain-cli__output {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 8px 0;
  scrollbar-gutter: stable;
}

.rain-cli__output::-webkit-scrollbar {
  width: 6px;
}

.rain-cli__output::-webkit-scrollbar-track {
  background: transparent;
}

.rain-cli__output::-webkit-scrollbar-thumb {
  background: hsl(var(--border));
  border-radius: 3px;
}

.rain-cli__output-inner {
  display: flex;
  flex-direction: column;
  padding: 0 14px;
  gap: 4px;
}

/* ─── Blocks ────────────────────────────────────────────────────────────── */

.rain-cli__block {
  position: relative;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  transition: background-color 0.1s ease;
}

.rain-cli__block:hover {
  background-color: hsl(var(--foreground) / 2%);
}

.rain-cli__block--user {
  margin-left: 24px;
}

.rain-cli__block--assistant {
  margin-right: 24px;
}

.rain-cli__block--tool {
  margin-left: 24px;
  margin-right: 24px;
}

.rain-cli__block--streaming {
  background-color: hsl(var(--primary) / 3%);
}

/* Phase C: turn-start spacing + horizontal rule */
.rain-cli__block--turn-start {
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid hsl(var(--border) / 30%);
}

/* Phase C: left-border accent on assistant blocks within a turn */
.rain-cli__block--assistant {
  border-left: 2px solid hsl(var(--primary) / 15%);
}

/* Phase C: brighter left-border on the actively-streaming block */
.rain-cli__block--streaming.rain-cli__block--assistant {
  border-left: 2px solid hsl(var(--primary) / 50%);
}

/* ─── Block body (flex column container) ────────────────────────────────── */

.rain-cli__block-body {
  display: flex;
  flex-direction: column;
}

/* ─── User prompt ───────────────────────────────────────────────────────── */

.rain-cli__prompt-line {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}

.rain-cli__prompt-arrow {
  color: hsl(var(--primary));
  font-weight: 700;
  font-size: 0.9rem;
  line-height: 1.5;
  flex-shrink: 0;
  user-select: none;
}

.rain-cli__prompt-text {
  color: hsl(var(--foreground));
  font-size: 0.85rem;
  line-height: 1.5;
  word-break: break-word;
  flex: 1;
}

/* ─── Assistant output ──────────────────────────────────────────────────── */

.rain-cli__output-text {
  color: hsl(var(--foreground) / 90%);
  font-size: 0.85rem;
  line-height: 1.55;
  word-break: break-word;
}

.rain-cli__output-text--streaming {
  color: hsl(var(--foreground));
}

.rain-cli__cursor {
  display: inline-block;
  color: hsl(var(--primary));
  font-size: 0.85rem;
  animation: blink 0.8s step-end infinite;
  margin-left: 1px;
}

@keyframes blink {
  50% { opacity: 0; }
}

/* ─── Markdown rendered content (Phase A) ───────────────────────────────── */

.rain-cli__markdown :deep(p) {
  margin: 0 0 0.5em;
}

.rain-cli__markdown :deep(p:last-child) {
  margin-bottom: 0;
}

.rain-cli__markdown :deep(ul),
.rain-cli__markdown :deep(ol) {
  margin: 0.4em 0;
  padding-left: 1.25em;
}

.rain-cli__markdown :deep(li) {
  margin: 0.2em 0;
}

.rain-cli__markdown :deep(strong),
.rain-cli__markdown :deep(b) {
  color: hsl(var(--foreground));
  font-weight: 600;
}

.rain-cli__markdown :deep(em),
.rain-cli__markdown :deep(i) {
  color: hsl(var(--foreground) / 80%);
}

.rain-cli__markdown :deep(a) {
  color: hsl(var(--primary) / 85%);
  text-decoration: underline;
  text-underline-offset: 2px;
}

.rain-cli__markdown :deep(a:hover) {
  color: hsl(var(--primary));
}

.rain-cli__markdown :deep(h1),
.rain-cli__markdown :deep(h2),
.rain-cli__markdown :deep(h3),
.rain-cli__markdown :deep(h4) {
  margin: 0.6em 0 0.3em;
  font-size: 1em;
  font-weight: 600;
  color: hsl(var(--foreground));
}

.rain-cli__markdown :deep(blockquote) {
  margin: 0.5em 0;
  padding-left: 0.75em;
  border-left: 2px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
}

/* Inline code */
.rain-cli__markdown :deep(code) {
  padding: 1px 5px;
  border-radius: 3px;
  background-color: hsl(var(--background-2));
  font-family: var(--font-mono, monospace);
  font-size: 0.9em;
  color: hsl(var(--primary) / 85%);
}

/* Fenced code blocks (Phase A: CSS-only — bounded, monospace, no syntax coloring) */
.rain-cli__markdown :deep(pre) {
  margin: 0.5em 0;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  background-color: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  overflow-x: auto;
}

.rain-cli__markdown :deep(pre code) {
  padding: 0;
  background: none;
  color: hsl(var(--foreground) / 85%);
  font-size: 0.82rem;
  line-height: 1.45;
  white-space: pre;
}

/* ─── Thinking steps ────────────────────────────────────────────────────── */

.rain-cli__thinking-step {
  color: hsl(var(--primary) / 80%);
  font-size: 0.8rem;
  padding: 2px 0;
  border-left: 2px solid hsl(var(--primary) / 30%);
  padding-left: 8px;
  margin: 4px 0;
}

/* ─── Diff blocks ───────────────────────────────────────────────────────── */

.rain-cli__diff-block {
  margin: 6px 0;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  background-color: hsl(var(--background-2));
}

.rain-cli__diff-line {
  padding: 1px 8px;
  font-size: 0.78rem;
  line-height: 1.4;
  white-space: pre;
}

.rain-cli__diff-line--add {
  background-color: rgba(34, 197, 94, 0.1);
  color: rgb(74, 222, 128);
}

.rain-cli__diff-line--remove {
  background-color: rgba(239, 68, 68, 0.1);
  color: rgb(248, 113, 113);
}

.rain-cli__diff-marker {
  display: inline-block;
  width: 14px;
  font-weight: 700;
  user-select: none;
}

/* ─── Copy button (Phase B: inline, bottom-right of block body) ─────────── */

.rain-cli__copy-btn {
  align-self: flex-end;
  margin-top: 6px;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
  background: hsl(var(--background-2));
  border: 1px solid hsl(var(--border));
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s ease, color 0.15s ease, border-color 0.15s ease;
  flex-shrink: 0;
}

.rain-cli__block:hover .rain-cli__copy-btn {
  opacity: 1;
}

.rain-cli__copy-btn:hover {
  color: hsl(var(--foreground));
  border-color: hsl(var(--primary));
}

.rain-cli__copy-btn--copied {
  opacity: 1;
  color: hsl(var(--success));
  border-color: hsl(var(--success));
}

.rain-cli__check-icon {
  stroke: hsl(var(--success));
}

/* ─── Follow-up suggestions ─────────────────────────────────────────────── */

.rain-cli__followups {
  margin-top: 10px;
  padding-top: 8px;
  border-top: 1px solid hsl(var(--border) / 50%);
}

.rain-cli__followups-label {
  font-size: 0.65rem;
  color: hsl(var(--muted-foreground));
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 6px;
}

.rain-cli__followups-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.rain-cli__followup-btn {
  padding: 4px 10px;
  border-radius: 999px;
  border: 1px solid hsl(var(--border));
  background-color: hsl(var(--background-2));
  color: hsl(var(--muted-foreground));
  font-size: 0.72rem;
  font-family: inherit;
  cursor: pointer;
  transition: all 0.12s ease;
  white-space: nowrap;
}

.rain-cli__followup-btn:hover {
  border-color: hsl(var(--primary) / 50%);
  color: hsl(var(--foreground));
  background-color: hsl(var(--primary) / 6%);
}

/* ─── Tool call card ────────────────────────────────────────────────────── */

.rain-cli__tool-card {
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  overflow: hidden;
  background-color: hsl(var(--background-2));
  margin-bottom: 6px;
}

.rain-cli__tool-card-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  cursor: pointer;
  user-select: none;
  background-color: hsl(var(--background-3, var(--background)));
  border-bottom: 1px solid hsl(var(--border));
}

.rain-cli__tool-card-toggle {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  background: none;
  border: none;
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  padding: 0;
}

.rain-cli__tool-card-name {
  font-size: 0.8rem;
  font-weight: 500;
  color: hsl(var(--foreground));
  flex: 1;
}

.rain-cli__tool-card-status {
  font-size: 0.7rem;
  color: hsl(var(--muted-foreground));
}

.rain-cli__tool-card-body {
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.rain-cli__tool-card-section {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.rain-cli__tool-card-label {
  font-size: 0.65rem;
  color: hsl(var(--muted-foreground));
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

.rain-cli__tool-card-code {
  font-size: 0.75rem;
  font-family: var(--font-mono, monospace);
  color: hsl(var(--foreground) / 80%);
  background-color: hsl(var(--background));
  padding: 6px 8px;
  border-radius: 4px;
  overflow-x: auto;
  white-space: pre-wrap;
  word-break: break-all;
  margin: 0;
  max-height: 300px;
  overflow-y: auto;
}

/* ─── Confirmation card ─────────────────────────────────────────────────── */

.rain-cli__confirm {
  flex-shrink: 0;
  margin: 0 14px 8px;
  padding: 10px 12px;
  border: 1px solid hsl(var(--primary) / 40%);
  border-radius: var(--radius-sm);
  background-color: hsl(var(--background-2));
}

.rain-cli__confirm-header {
  font-size: 0.8rem;
  font-weight: 600;
  color: hsl(var(--foreground));
  margin-bottom: 6px;
}

.rain-cli__confirm-lines {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-bottom: 6px;
}

.rain-cli__confirm-line {
  font-size: 0.75rem;
  color: hsl(var(--muted-foreground));
  word-break: break-all;
  font-family: var(--font-mono, monospace);
}

.rain-cli__confirm-warning {
  font-size: 0.7rem;
  color: rgb(248, 113, 113);
  margin-bottom: 8px;
  padding: 4px 6px;
  border-radius: 4px;
  background-color: rgba(239, 68, 68, 0.08);
}

.rain-cli__confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
}

.rain-cli__confirm-cancel,
.rain-cli__confirm-ok {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  border-radius: var(--radius-sm);
  font-family: inherit;
  font-size: 0.75rem;
  font-weight: 500;
  cursor: pointer;
  border: 1px solid hsl(var(--border));
}

.rain-cli__confirm-cancel {
  background: transparent;
  color: hsl(var(--muted-foreground));
}

.rain-cli__confirm-cancel:hover {
  color: hsl(var(--foreground));
  border-color: hsl(var(--muted-foreground));
}

.rain-cli__confirm-ok {
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  border-color: hsl(var(--primary));
}

.rain-cli__confirm-ok:hover {
  filter: brightness(1.1);
}

/* ─── Input area ────────────────────────────────────────────────────────── */

.rain-cli__input-area {
  flex-shrink: 0;
  padding: 8px 14px;
  border-top: 1px solid hsl(var(--border));
  background-color: hsl(var(--background-2));
}

.rain-cli__input-wrap {
  display: flex;
  align-items: center;
  gap: 8px;
  background-color: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  padding: 4px 8px;
  transition: border-color 0.15s ease;
}

.rain-cli__input-wrap:focus-within {
  border-color: hsl(var(--primary) / 50%);
}

.rain-cli__input-prompt {
  color: hsl(var(--primary));
  font-weight: 700;
  font-size: 0.9rem;
  user-select: none;
  flex-shrink: 0;
}

.rain-cli__input {
  flex: 1;
  min-width: 0;
  background: transparent;
  border: none;
  outline: none;
  color: hsl(var(--foreground));
  font-family: var(--font-mono, monospace);
  font-size: 0.85rem;
  padding: 4px 0;
}

.rain-cli__input::placeholder {
  color: hsl(var(--muted-foreground) / 60%);
}

.rain-cli__send-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  border: none;
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  cursor: pointer;
  flex-shrink: 0;
  transition: filter 0.12s ease;
}

.rain-cli__send-btn:hover:not(:disabled) {
  filter: brightness(1.1);
}

.rain-cli__send-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* ─── Drive info panel ──────────────────────────────────────────────────── */

.rain-cli__drives-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 4px 14px 16px;
}

.rain-cli__drives-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 0 0;
}

.rain-cli__drives-header-icon {
  color: hsl(var(--primary));
  flex-shrink: 0;
}

.rain-cli__drives-header-label {
  font-size: 0.8rem;
  font-weight: 600;
  color: hsl(var(--foreground));
  letter-spacing: 0.02em;
}

.rain-cli__drives-header-hint {
  font-size: 0.72rem;
  color: hsl(var(--muted-foreground));
}

.rain-cli__drives-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 8px;
}

.rain-cli__drive-card {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px 12px;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  background-color: hsl(var(--background-2));
  transition: border-color 0.12s ease, background-color 0.12s ease;
}

.rain-cli__drive-card:hover {
  border-color: hsl(var(--primary) / 30%);
  background-color: hsl(var(--background-3, var(--background)));
}

.rain-cli__drive-card-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  background-color: hsl(var(--foreground) / 4%);
  border-radius: var(--radius-sm);
  color: hsl(var(--muted-foreground));
}

.rain-cli__drive-card-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.rain-cli__drive-card-top {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.rain-cli__drive-card-name {
  font-size: 0.8rem;
  font-weight: 600;
  color: hsl(var(--foreground));
}

.rain-cli__drive-card-path {
  font-size: 0.68rem;
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
}

.rain-cli__drive-card-type {
  font-size: 0.6rem;
  color: hsl(var(--muted-foreground));
  background-color: hsl(var(--foreground) / 6%);
  padding: 0 5px;
  border-radius: 3px;
  text-transform: uppercase;
  letter-spacing: 0.03em;
  margin-left: auto;
}

.rain-cli__drive-card-space {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 0.72rem;
  color: hsl(var(--muted-foreground));
}

.rain-cli__drive-card-available {
  font-weight: 500;
  color: hsl(var(--foreground) / 80%);
  font-family: var(--font-mono, monospace);
}

.rain-cli__drive-card-free,
.rain-cli__drive-card-sep {
  color: hsl(var(--muted-foreground) / 70%);
}

.rain-cli__drive-card-total {
  color: hsl(var(--muted-foreground));
  font-family: var(--font-mono, monospace);
}

.rain-cli__drive-card-percent {
  margin-left: auto;
  font-size: 0.68rem;
  font-family: var(--font-mono, monospace);
  color: hsl(var(--foreground) / 60%);
}

.rain-cli__drive-card-bar {
  width: 100%;
  height: 4px;
  background-color: hsl(var(--foreground) / 8%);
  border-radius: 2px;
  overflow: hidden;
}

.rain-cli__drive-card-bar-fill {
  height: 100%;
  background-color: hsl(var(--primary) / 60%);
  border-radius: 2px;
  transition: width 0.3s ease;
}

.rain-cli__drive-card-bar-fill--warn {
  background-color: hsl(35 90% 55% / 70%);
}

.rain-cli__drive-card-bar-fill--crit {
  background-color: hsl(0 70% 55% / 70%);
}

.rain-cli__drive-card--empty {
  align-items: center;
  justify-content: center;
  gap: 8px;
  opacity: 0.5;
  grid-column: 1 / -1;
}

.rain-cli__drive-card-empty-text {
  font-size: 0.8rem;
  color: hsl(var(--muted-foreground));
}

/* ─── Misc ──────────────────────────────────────────────────────────────── */

.rain-cli__thinking {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  color: hsl(var(--muted-foreground));
  font-size: 0.8rem;
}

.rain-cli__spinner {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.rain-cli__error {
  padding: 8px 10px;
  margin: 4px 14px;
  border-radius: var(--radius-sm);
  background-color: rgba(239, 68, 68, 0.1);
  border: 1px solid rgba(239, 68, 68, 0.3);
  color: rgb(248, 113, 113);
  font-size: 0.8rem;
}

.rain-cli__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 48px 24px;
  gap: 8px;
}

.rain-cli__empty-icon {
  color: hsl(var(--primary) / 30%);
}

.rain-cli__empty-text {
  color: hsl(var(--muted-foreground));
  font-size: 0.85rem;
  margin: 0;
}
</style>
