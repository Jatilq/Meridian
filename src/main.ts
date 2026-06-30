// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './app.vue';
import router from './router';
import { i18n } from '@/localization';
import VWave from 'v-wave';
import { installModuleLoadRecovery } from '@/utils/module-load-recovery';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { useDownloaderStore } from '@/stores/runtime/downloader';
import { disableWebViewFeatures } from '@/utils/disable-web-view-features';

import './styles/index.css';

installModuleLoadRecovery({ router });

const app = createApp(App);

app.use(createPinia());
app.use(router);
app.use(i18n);
app.use(VWave, {
  cancellationPeriod: 0,
  color: 'hsl(var(--primary))',
});
disableWebViewFeatures();
app.mount('#app');

listen<{
  url: string;
  fileName?: string;
  formatId?: string;
  autoSaveFolder?: string;
}>('extension-download-request', (event) => {
  const downloaderStore = useDownloaderStore();
  const payload = event.payload;
  void invoke('downloader_enqueue', {
    url: payload.url,
    fileName: payload.fileName ?? null,
    formatId: payload.formatId ?? null,
    autoSaveFolder: payload.autoSaveFolder ?? null,
  }).then(() => {
    if (downloaderStore) {
      downloaderStore.open();
    }
  }).catch(console.error);
});
