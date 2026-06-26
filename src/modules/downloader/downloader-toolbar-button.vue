<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { useDownloaderStore } from '@/stores/runtime/downloader';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Popover, PopoverTrigger, PopoverContent } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';
import {
  DownloadIcon,
  LoaderCircleIcon,
  CheckIcon,
  XIcon,
  PauseIcon,
  PlayIcon,
  TrashIcon,
  HistoryIcon,
} from '@lucide/vue';
import { invoke } from '@tauri-apps/api/core';

const { t } = useI18n();
const store = useDownloaderStore();

const popoverOpen = computed({
  get: () => store.isOpen,
  set: (open) => { store.isOpen = open; },
});

const activeCount = computed(() => store.queue.filter(i => i.status === 'downloading').length);

async function handleAddUrl() {
  const url = store.urlInput.trim();
  if (!url) return;
  try {
    await invoke('downloader_enqueue', { url, fileName: null, formatId: null, autoSaveFolder: store.autoSaveFolder || null });
    store.setUrlInput('');
    void refreshState();
  }
  catch (error) {
    console.error('Download failed:', error);
  }
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault();
    void handleAddUrl();
  }
}

async function refreshState() {
  try {
    const state = await invoke<{ queue: unknown[]; history: unknown[] }>('downloader_get_state');
    store.setQueue(state.queue as any);
    store.setHistory(state.history as any);
  }
  catch (error) {
    console.error('Failed to refresh downloader state:', error);
  }
}

watch(() => store.isOpen, (open) => {
  if (open) void refreshState();
});

async function handleCancel(id: string) {
  try {
    await invoke('downloader_cancel', { id });
    void refreshState();
  }
  catch (error) {
    console.error('Cancel failed:', error);
  }
}

async function handlePause(id: string) {
  try {
    await invoke('downloader_pause', { id });
    void refreshState();
  }
  catch (error) {
    console.error('Pause failed:', error);
  }
}

async function handleResume(id: string) {
  try {
    await invoke('downloader_resume', { id });
    void refreshState();
  }
  catch (error) {
    console.error('Resume failed:', error);
  }
}

function formatBytes(bytes: number | null) {
  if (!bytes) return '-';
  const units = ['B', 'KB', 'MB', 'GB'];
  let size = bytes;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex++;
  }
  return `${size.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function statusLabel(status: string) {
  switch (status) {
    case 'downloading': return t('downloader.statusDownloading');
    case 'paused': return t('downloader.statusPaused');
    case 'completed': return t('downloader.statusCompleted');
    case 'failed': return t('downloader.statusFailed');
    case 'cancelled': return t('downloader.statusCancelled');
    default: return t('downloader.statusPending');
  }
}

function timeAgo(ts: number) {
  const diff = Math.floor((Date.now() / 1000) - ts);
  if (diff < 60) return `${diff}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  return `${Math.floor(diff / 3600)}h`;
}
</script>

<template>
  <div class="downloader-toolbar-button animate-fade-in">
    <Tooltip>
      <Popover v-model:open="popoverOpen">
        <TooltipTrigger as-child>
          <PopoverTrigger as-child>
            <Button
              variant="ghost"
              size="icon"
              class="downloader-toolbar-button__button"
              :class="{ 'downloader-toolbar-button__button--active': store.isOpen }"
            >
              <DownloadIcon :size="16" class="downloader-toolbar-button__icon" />
              <span v-if="activeCount" class="downloader-toolbar-button__badge">
                {{ activeCount }}
              </span>
            </Button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>
          {{ t('downloader.title') }}
        </TooltipContent>
        <PopoverContent align="end" :side-offset="8" class="downloader-popover">
          <div class="downloader">
            <div class="downloader__header">
              <h3 class="downloader__title">{{ t('downloader.title') }}</h3>
            </div>
            <div class="downloader__add">
              <Input
                v-model="store.urlInput"
                :placeholder="t('downloader.urlPlaceholder')"
                class="downloader__url-input"
                @keydown="handleKeyDown"
              />
              <Button size="sm" class="downloader__add-btn" @click="handleAddUrl">
                {{ t('downloader.add') }}
              </Button>
            </div>
            <Tabs v-model:value="store.activeTab" class="downloader__tabs">
              <TabsList class="downloader__tab-list">
                <TabsTrigger value="queue">
                  {{ t('downloader.queue') }}
                  <span v-if="store.queueCount" class="downloader__tab-count">
                    {{ store.queueCount }}
                  </span>
                </TabsTrigger>
                <TabsTrigger value="history">
                  {{ t('downloader.history') }}
                  <span v-if="store.historyCount" class="downloader__tab-count">
                    {{ store.historyCount }}
                  </span>
                </TabsTrigger>
              </TabsList>
              <TabsContent value="queue" class="downloader__tab-content">
                <ScrollArea class="downloader__scroll">
                  <div v-if="store.queue.length === 0" class="downloader__empty">
                    <DownloadIcon :size="20" />
                    <span>{{ t('downloader.emptyQueue') }}</span>
                  </div>
                  <div v-for="item in store.queue" :key="item.id" class="downloader__item">
                    <div class="downloader__item-info">
                      <div class="downloader__item-name">{{ item.file_name }}</div>
                      <div class="downloader__item-meta">
                        <span class="downloader__item-status" :class="`downloader__item-status--${item.status}`">
                          {{ statusLabel(item.status) }}
                        </span>
                        <span class="downloader__item-size">
                          {{ formatBytes(item.total_bytes ?? item.downloaded_bytes) }}
                        </span>
                      </div>
                      <div class="downloader__item-progress">
                        <div class="downloader__progress-bar">
                          <div class="downloader__progress-fill" :style="{ width: `${item.progress * 100}%` }" />
                        </div>
                      </div>
                    </div>
                    <div class="downloader__item-actions">
                      <Button v-if="item.status === 'downloading'" variant="ghost" size="xs" @click="handlePause(item.id)">
                        <PauseIcon :size="14" />
                      </Button>
                      <Button v-if="item.status === 'paused'" variant="ghost" size="xs" @click="handleResume(item.id)">
                        <PlayIcon :size="14" />
                      </Button>
                      <Button variant="ghost" size="xs" @click="handleCancel(item.id)">
                        <XIcon :size="14" />
                      </Button>
                    </div>
                  </div>
                </ScrollArea>
              </TabsContent>
              <TabsContent value="history" class="downloader__tab-content">
                <ScrollArea class="downloader__scroll">
                  <div v-if="store.history.length === 0" class="downloader__empty">
                    <HistoryIcon :size="20" />
                    <span>{{ t('downloader.emptyHistory') }}</span>
                  </div>
                  <div v-for="item in store.history" :key="item.id" class="downloader__item">
                    <div class="downloader__item-info">
                      <div class="downloader__item-name">{{ item.file_name }}</div>
                      <div class="downloader__item-meta">
                        <span class="downloader__item-status" :class="`downloader__item-status--${item.status}`">
                          {{ statusLabel(item.status) }}
                        </span>
                        <span class="downloader__item-time">{{ timeAgo(item.created_at) }}</span>
                      </div>
                    </div>
                  </div>
                </ScrollArea>
              </TabsContent>
            </Tabs>
          </div>
        </PopoverContent>
      </Popover>
    </Tooltip>
  </div>
</template>

<style scoped>
.downloader-toolbar-button :deep(.sigma-ui-button.downloader-toolbar-button__button) {
  position: relative;
  width: 28px;
  height: 28px;
  min-height: 28px;
  padding: 0;
}

.downloader-toolbar-button__icon {
  stroke: hsl(var(--foreground) / 50%);
}

.downloader-toolbar-button__button--active {
  background-color: hsl(var(--secondary));
}

.downloader-toolbar-button__button--active .downloader-toolbar-button__icon {
  stroke: hsl(var(--primary));
}

.downloader-toolbar-button__badge {
  position: absolute;
  top: 2px;
  right: 2px;
  min-width: 14px;
  height: 14px;
  padding: 0 3px;
  border-radius: 9999px;
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  font-size: 9px;
  font-weight: 700;
  line-height: 14px;
  text-align: center;
}

:global(.downloader-popover) {
  width: 360px;
  padding: 0;
}

.downloader {
  display: flex;
  flex-direction: column;
}

.downloader__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border-bottom: 1px solid hsl(var(--border));
}

.downloader__title {
  margin: 0;
  color: hsl(var(--muted-foreground));
  font-size: 11px;
  font-weight: 500;
  text-transform: uppercase;
}

.downloader__add {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  border-bottom: 1px solid hsl(var(--border) / 50%);
}

.downloader__url-input {
  flex: 1;
  min-width: 0;
}

.downloader__add-btn {
  flex-shrink: 0;
}

.downloader__tabs {
  display: flex;
  flex-direction: column;
}

.downloader__tab-list {
  padding: 6px 10px 0;
}

.downloader__tab-content {
  padding: 0;
}

.downloader__scroll {
  --downloader-scroll-max: min(320px, calc(100vh - 180px));
  max-height: var(--downloader-scroll-max);
}

.downloader__scroll :deep(.sigma-ui-scroll-area__viewport) {
  max-height: var(--downloader-scroll-max);
}

.downloader__empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px 12px;
  color: hsl(var(--muted-foreground));
  font-size: 12px;
  gap: 6px;
}

.downloader__empty svg {
  opacity: 0.5;
}

.downloader__item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-bottom: 1px solid hsl(var(--border) / 40%);
}

.downloader__item:last-child {
  border-bottom: none;
}

.downloader__item-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.downloader__item-name {
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.downloader__item-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: hsl(var(--muted-foreground));
}

.downloader__item-status {
  padding: 1px 5px;
  border-radius: 3px;
  font-size: 10px;
  font-weight: 500;
  text-transform: uppercase;
}

.downloader__item-status--downloading {
  background: hsl(var(--primary) / 15%);
  color: hsl(var(--primary));
}

.downloader__item-status--paused {
  background: hsl(var(--warning) / 15%);
  color: hsl(var(--warning));
}

.downloader__item-status--completed {
  background: hsl(var(--success) / 15%);
  color: hsl(var(--success));
}

.downloader__item-status--failed {
  background: hsl(var(--destructive) / 15%);
  color: hsl(var(--destructive));
}

.downloader__item-status--cancelled {
  background: hsl(var(--muted));
  color: hsl(var(--muted-foreground));
}

.downloader__item-progress {
  margin-top: 4px;
}

.downloader__progress-bar {
  height: 3px;
  border-radius: 2px;
  background: hsl(var(--border));
  overflow: hidden;
}

.downloader__progress-fill {
  height: 100%;
  border-radius: 2px;
  background: hsl(var(--primary));
  transition: width 0.2s ease;
}

.downloader__item-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}

.downloader__tab-count {
  margin-left: 4px;
  padding: 1px 5px;
  border-radius: 3px;
  background: hsl(var(--primary) / 15%);
  color: hsl(var(--primary));
  font-size: 10px;
  font-weight: 500;
}
</style>
