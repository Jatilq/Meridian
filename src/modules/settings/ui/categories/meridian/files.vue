<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { FolderIcon, FolderOpenIcon } from '@lucide/vue';
import { open } from '@tauri-apps/plugin-dialog';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();

const modelsFolder = computed({
  get: () => userSettingsStore.userSettings.meridian.modelsFolder ?? '',
  set: (value: string) => {
    userSettingsStore.userSettings.meridian.modelsFolder = value;
    userSettingsStore.setUserSettingsStorage('meridian.modelsFolder', value);
  },
});

async function browse() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select Models folder',
    });
    if (typeof selected === 'string') {
      modelsFolder.value = selected;
    }
  }
  catch (error) {
    console.error('Folder picker failed:', error);
  }
}

function clearFolder() {
  modelsFolder.value = '';
}
</script>

<template>
  <SettingsItem
    title="Models folder"
    description="Shared folder where downloaded model files are stored. Used by the AI panel, hardware scanner, and backend manager."
    :icon="FolderIcon"
  >
    <div class="files-settings">
      <div class="files-settings__field">
        <label class="files-settings__label" for="models-folder-input">
          Models folder
        </label>
        <div class="files-settings__row">
          <Input
            id="models-folder-input"
            v-model="modelsFolder"
            placeholder="e.g. E:\ai\Models"
            class="files-settings__input"
          />
          <Button
            type="button"
            variant="secondary"
            size="sm"
            class="files-settings__browse"
            @click="browse"
          >
            <FolderOpenIcon :size="14" />
            Browse
          </Button>
          <Button
            v-if="modelsFolder"
            type="button"
            variant="ghost"
            size="sm"
            class="files-settings__clear"
            title="Clear models folder"
            @click="clearFolder"
          >
            Clear
          </Button>
        </div>
      </div>
    </div>
  </SettingsItem>
</template>

<style scoped>
.files-settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.files-settings__field {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

.files-settings__label {
  color: hsl(var(--foreground));
  font-size: 0.875rem;
  font-weight: 500;
}

.files-settings__row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.files-settings__input {
  flex: 1;
  width: 100%;
}

.files-settings__browse {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
}

.files-settings__clear {
  flex-shrink: 0;
}
</style>
