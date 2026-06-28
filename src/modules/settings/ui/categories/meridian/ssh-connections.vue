<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { ServerIcon, PlusIcon, Trash2Icon, KeyRoundIcon, LockIcon } from '@lucide/vue';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import type { SshConnectionSetting, SshAuthMethod } from '@/types/user-settings';

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

function updateField(
  index: number,
  field: keyof SshConnectionSetting,
  value: string | number | undefined,
) {
  const conn = userSettingsStore.userSettings.meridian.sshConnections[index];
  if (!conn) return;
  if (field === 'port') {
    conn.port = Number(value) || 22;
  }
  else if (field === 'authMethod') {
    conn.authMethod = value === 'password' ? 'password' : 'key';
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
    authMethod: 'key',
    password: '',
  });
  persist();
}

function removeConnection(index: number) {
  userSettingsStore.userSettings.meridian.sshConnections.splice(index, 1);
  persist();
}

function setAuthMethod(index: number, method: SshAuthMethod) {
  updateField(index, 'authMethod', method);
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
              placeholder="username"
              @update:model-value="(v) => updateField(index, 'username', v)"
            />
          </div>
          <div class="ssh-settings__field ssh-settings__field--auth">
            <label class="ssh-settings__label">Auth method</label>
            <div class="ssh-settings__toggle">
              <button
                type="button"
                class="ssh-settings__toggle-btn"
                :class="{ 'ssh-settings__toggle-btn--active': conn.authMethod === 'key' }"
                title="Key file authentication"
                @click="setAuthMethod(index, 'key')"
              >
                <KeyRoundIcon :size="14" />
                Key file
              </button>
              <button
                type="button"
                class="ssh-settings__toggle-btn"
                :class="{ 'ssh-settings__toggle-btn--active': conn.authMethod === 'password' }"
                title="Password authentication"
                @click="setAuthMethod(index, 'password')"
              >
                <LockIcon :size="14" />
                Password
              </button>
            </div>
          </div>
          <div class="ssh-settings__field ssh-settings__field--wide">
            <label v-if="conn.authMethod === 'key'" class="ssh-settings__label">{{ t('settings.meridian.ssh.keyPath') }}</label>
            <label v-else class="ssh-settings__label">Password</label>
            <Input
              v-if="conn.authMethod === 'key'"
              :model-value="conn.keyPath"
              placeholder="C:\Users\name\.ssh\id_ed25519"
              @update:model-value="(v) => updateField(index, 'keyPath', v)"
            />
            <Input
              v-else
              :model-value="conn.password ?? ''"
              type="password"
              placeholder="••••••••"
              @update:model-value="(v) => updateField(index, 'password', v)"
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

.ssh-settings__field--auth {
  flex: 0 0 auto;
}

.ssh-settings__field--wide {
  flex: 2;
}

.ssh-settings__label {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
}

.ssh-settings__toggle {
  display: inline-flex;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background));
}

.ssh-settings__toggle-btn {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.3rem 0.5rem;
  font-size: 0.75rem;
  background: transparent;
  border: 0;
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  transition: background 0.15s ease;
}

.ssh-settings__toggle-btn + .ssh-settings__toggle-btn {
  border-left: 1px solid hsl(var(--border));
}

.ssh-settings__toggle-btn:hover {
  background: hsl(var(--button-hover, var(--background-2)));
  color: hsl(var(--foreground));
}

.ssh-settings__toggle-btn--active {
  background: hsl(var(--primary) / 15%);
  color: hsl(var(--foreground));
  font-weight: 600;
}

.ssh-settings__remove {
  flex-shrink: 0;
}

.ssh-settings__add {
  align-self: flex-start;
  gap: 0.375rem;
}
</style>
