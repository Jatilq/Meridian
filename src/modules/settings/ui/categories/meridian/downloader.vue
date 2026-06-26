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
    <div class="downloader-settings">
      <div class="downloader-settings__field">
        <label class="downloader-settings__label" for="downloader-auto-save-folder">
          {{ t('downloader.autoSaveFolder') }}
        </label>
        <Input
          id="downloader-auto-save-folder"
          v-model="autoSaveFolder"
          placeholder="e.g. C:\Users\Name\Downloads"
          class="downloader-settings__input"
        />
      </div>
    </div>
  </SettingsItem>
</template>

<style scoped>
.downloader-settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.downloader-settings__field {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

.downloader-settings__label {
  color: hsl(var(--foreground));
  font-size: 0.875rem;
  font-weight: 500;
}

.downloader-settings__input {
  width: 100%;
}
</style>
