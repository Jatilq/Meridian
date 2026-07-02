<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { DownloadIcon } from '@lucide/vue';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();

const autoSaveFolder = computed({
  get: () => userSettingsStore.userSettings.meridian.downloader.autoSaveFolder,
  set: (value: string) => {
    userSettingsStore.userSettings.meridian.downloader.autoSaveFolder = value;
    userSettingsStore.setUserSettingsStorage('meridian.downloader.autoSaveFolder', value);
  },
});
</script>

<template>
  <SettingsItem
    :title="t('settings.meridian.downloader.title')"
    :description="t('settings.meridian.downloader.description')"
    :icon="DownloadIcon"
  >
    <div class="exo-card exo-card--violet">
      <div class="exo-tile" aria-hidden="true">
        <DownloadIcon :size="28" class="exo-tile__icon" />
      </div>
      <div class="exo-identity">
        <span class="exo-identity__title">Downloader</span>
        <span class="exo-identity__sub">Auto-save destination</span>
      </div>
      <div class="exo-specs">
        <div class="exo-specs__field">
          <label class="exo-specs__label" for="downloader-auto-save-folder">
            {{ t('downloader.autoSaveFolder') }}
          </label>
          <Input
            id="downloader-auto-save-folder"
            v-model="autoSaveFolder"
            placeholder="e.g. C:\Users\Name\Downloads"
            class="exo-specs__input"
          />
          <span class="exo-card-hint">
            Files drop here when downloading via the AI panel, Hardware Scanner, or Backend Manager.
          </span>
        </div>
      </div>
      <div class="exo-actions" aria-hidden="true" />
    </div>
  </SettingsItem>
</template>

<style scoped>
/* Hint, warning, and toggle-row classes now live in src/styles/exo.css
   as `.exo-card-hint` etc. so any settings sub-page can share the same
   copy styling without bypassing Vue scoped-style attribute matching.
   This scoped block intentionally has no local rules — the lifecycle
   only consumes exo.css + the inherited theme tokens. */
</style>
