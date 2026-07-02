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
    <div class="exo-card exo-card--amber">
      <div class="exo-tile" aria-hidden="true">
        <FolderIcon :size="28" class="exo-tile__icon" />
      </div>
      <div class="exo-identity">
        <span class="exo-identity__title">Models folder</span>
        <span class="exo-identity__sub">{{ modelsFolder || '(not configured)' }}</span>
      </div>
      <div class="exo-specs">
        <div class="exo-specs__field">
          <label class="exo-specs__label" for="models-folder-input">On-disk path</label>
          <div class="files-settings__row">
            <Input
              id="models-folder-input"
              v-model="modelsFolder"
              placeholder="e.g. E:\ai\Models"
              class="exo-specs__input"
            />
            <Button
              type="button"
              variant="secondary"
              size="sm"
              class="exo-actions__btn"
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
              class="exo-actions__btn"
              title="Clear models folder"
              @click="clearFolder"
            >
              Clear
            </Button>
          </div>
          <span class="exo-card-hint">
            Path persists across sessions and is read on app boot.
          </span>
        </div>
      </div>
      <div class="exo-actions" aria-hidden="true" />
    </div>
  </SettingsItem>
</template>

<style scoped>
/* Browse + Clear button row — `.exo-actions__btn` from exo.css handles
   each button's individual look, this just lays them out inline with
   the input field above. Don't migrate to exo.css because the layout
   is `.files-settings`-specific (the Browse/Clear pairing isn't used
   anywhere else). */
.files-settings__row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
</style>
