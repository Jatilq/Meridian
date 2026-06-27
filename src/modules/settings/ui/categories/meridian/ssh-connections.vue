<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { ServerIcon, PlusIcon, Trash2Icon } from '@lucide/vue';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import type { SshConnectionSetting } from '@/types/user-settings';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();

const connections = computed<SshConnectionSetting[]>(
  () => userSettingsStore.userSettings.meridian.sshConnections ?? [],
);

function persist() {
  userSettingsStore.setUserSettingsStorage(
    'meridian.sshConnections',
    userSettingsStore.userSettings.meridian.sshConnections,
  );
}

function updateField(index: number, field: keyof SshConnectionSetting, value: string | number | undefined) {
  const conn = userSettingsStore.userSettings.meridian.sshConnections[index];
  if (!conn) return;
  if (field === 'port') {
    conn.port = Number(value) || 22;
  }
  else {
    (conn[field] as string) = String(value ?? '');
  }
  persist();
}

function addConnection() {
  userSettingsStore.userSettings.meridian.sshConnections.push({
    host: '',
    label: '',
    port: 22,
    username: '',
    keyPath: '',
  });
  persist();
}

function removeConnection(index: number) {
  userSettingsStore.userSettings.meridian.sshConnections.splice(index, 1);
  persist();
}
</script>

<template>
  <SettingsItem
    :title="t('settings.meridian.ssh.title')"
    :description="t('settings.meridian.ssh.description')"
    :icon="ServerIcon"
  >
    <div class="ssh-settings">
      <div
        v-for="(conn, index) in connections"
        :key="index"
        class="ssh-settings__card"
      >
        <div class="ssh-settings__row">
          <div class="ssh-settings__field">
            <label class="ssh-settings__label">{{ t('settings.meridian.ssh.label') }}</label>
            <Input
              :model-value="conn.label"
              placeholder="MAMBA"
              @update:model-value="(v) => updateField(index, 'label', v)"
            />
          </div>
          <div class="ssh-settings__field">
            <label class="ssh-settings__label">{{ t('settings.meridian.ssh.host') }}</label>
            <Input
              :model-value="conn.host"
              placeholder="192.168.1.67"
              @update:model-value="(v) => updateField(index, 'host', v)"
            />
          </div>
          <div class="ssh-settings__field ssh-settings__field--port">
            <label class="ssh-settings__label">{{ t('settings.meridian.ssh.port') }}</label>
            <Input
              :model-value="String(conn.port)"
              placeholder="22"
              @update:model-value="(v) => updateField(index, 'port', v)"
            />
          </div>
        </div>
        <div class="ssh-settings__row">
          <div class="ssh-settings__field">
            <label class="ssh-settings__label">{{ t('settings.meridian.ssh.username') }}</label>
            <Input
              :model-value="conn.username"
              placeholder="jatilq"
              @update:model-value="(v) => updateField(index, 'username', v)"
            />
          </div>
          <div class="ssh-settings__field ssh-settings__field--wide">
            <label class="ssh-settings__label">{{ t('settings.meridian.ssh.keyPath') }}</label>
            <Input
              :model-value="conn.keyPath"
              placeholder="C:\Users\name\.ssh\id_ed25519"
              @update:model-value="(v) => updateField(index, 'keyPath', v)"
            />
          </div>
          <Button
            variant="ghost"
            size="icon"
            class="ssh-settings__remove"
            :title="t('settings.meridian.ssh.remove')"
            @click="removeConnection(index)"
          >
            <Trash2Icon :size="16" />
          </Button>
        </div>
      </div>

      <Button
        variant="secondary"
        class="ssh-settings__add"
        @click="addConnection"
      >
        <PlusIcon :size="16" />
        {{ t('settings.meridian.ssh.add') }}
      </Button>
    </div>
  </SettingsItem>
</template>

<style scoped>
.ssh-settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.ssh-settings__card {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 0.75rem;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
}

.ssh-settings__row {
  display: flex;
  align-items: flex-end;
  gap: 0.5rem;
}

.ssh-settings__field {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.ssh-settings__field--port {
  flex: 0 0 70px;
}

.ssh-settings__field--wide {
  flex: 2;
}

.ssh-settings__label {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
}

.ssh-settings__remove {
  flex-shrink: 0;
}

.ssh-settings__add {
  align-self: flex-start;
  gap: 0.375rem;
}
</style>
