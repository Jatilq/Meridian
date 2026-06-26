// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { defineStore } from 'pinia';
import { ref, computed } from 'vue';
import type { DownloadItem, DownloadStatus } from '@/types/downloader';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

export const useDownloaderStore = defineStore('downloader', () => {
  const userSettingsStore = useUserSettingsStore();
  const isOpen = ref(false);
  const queue = ref<DownloadItem[]>([]);
  const history = ref<DownloadItem[]>([]);
  const isLoading = ref(false);
  const urlInput = ref('');
  const formats = ref<Array<{ format_id: string; ext: string; resolution?: string; filesize?: number; format_note?: string }>>([]);
  const selectedFormat = ref<string>('');
  const showFormatSelector = ref(false);
  const activeTab = ref<'queue' | 'history'>('queue');
  const autoSaveFolder = ref(userSettingsStore.userSettings.meridian?.downloader?.autoSaveFolder || '');

  const activeCount = computed(() => queue.value.filter(item => item.status === 'downloading').length);
  const hasActive = computed(() => activeCount.value > 0);
  const queueCount = computed(() => queue.value.length);
  const historyCount = computed(() => history.value.length);

  function open() { isOpen.value = true; }
  function close() { isOpen.value = false; }
  function toggle() { isOpen.value ? close() : open(); }

  function setUrlInput(value: string) { urlInput.value = value; }
  function setSelectedFormat(value: string) { selectedFormat.value = value; }
  function setShowFormatSelector(value: boolean) { showFormatSelector.value = value; }
  function setActiveTab(value: 'queue' | 'history') { activeTab.value = value; }
  function setAutoSaveFolder(value: string) {
    autoSaveFolder.value = value;
    userSettingsStore.userSettings.meridian.downloader.autoSaveFolder = value;
    userSettingsStore.setUserSettingsStorage('meridian.downloader.autoSaveFolder', value);
  }

  function setQueue(items: DownloadItem[]) { queue.value = items; }
  function setHistory(items: DownloadItem[]) { history.value = items; }
  function setLoading(value: boolean) { isLoading.value = value; }
  function setFormats(items: Array<{ format_id: string; ext: string; resolution?: string; filesize?: number; format_note?: string }>) {
    formats.value = items;
    showFormatSelector.value = items.length > 0;
  }

  function upsertItem(item: DownloadItem) {
    const idx = queue.value.findIndex(i => i.id === item.id);
    if (idx >= 0) {
      queue.value[idx] = item;
    }
    if (item.status === 'completed' || item.status === 'failed' || item.status === 'cancelled') {
      queue.value = queue.value.filter(i => i.id !== item.id);
      if (!history.value.some(i => i.id === item.id)) {
        history.value.unshift(item);
      }
    }
  }

  function removeFromQueue(id: string) {
    queue.value = queue.value.filter(i => i.id !== id);
  }

  function clearHistory() {
    history.value = [];
  }

  return {
    isOpen, queue, history, isLoading, urlInput, formats, selectedFormat,
    showFormatSelector, activeTab, autoSaveFolder, activeCount, hasActive, queueCount, historyCount,
    open, close, toggle, setUrlInput, setSelectedFormat, setShowFormatSelector,
    setActiveTab, setAutoSaveFolder, setQueue, setHistory, setLoading, setFormats, upsertItem,
    removeFromQueue, clearHistory,
  };
});
