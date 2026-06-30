<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, reactive } from 'vue';
import { useI18n } from 'vue-i18n';
import { ServerIcon, PlusIcon, Trash2Icon, KeyRoundIcon, LockIcon, ShieldCheckIcon } from '@lucide/vue';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { storeSshPassword, clearSshPassword } from '@/utils/ssh-connections';
import type { SshConnectionSetting, SshAuthMethod } from '@/types/user-settings';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();

// Round-26 reset: cluster infrastructure has its own storage array,
// parallel to but independent from `meridian.sshConnections`. The
// `sshConnections` array is reserved for the file-browser feature.
const connections = computed<SshConnectionSetting[]>(
  () => userSettingsStore.userSettings.meridian.clusterWorkers ?? [],
);

function persist() {
  userSettingsStore.setUserSettingsStorage(
    'meridian.clusterWorkers',
    userSettingsStore.userSettings.meridian.clusterWorkers,
  );
}

function updateField(
  index: number,
  field: Exclude<keyof SshConnectionSetting, 'passwordSecureKey'>,
  value: string | number | undefined,
) {
  const conn = userSettingsStore.userSettings.meridian.clusterWorkers[index];
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

// Per-row password drafts so typing does not hammer the secure-keys store;
// the actual `secure_store_secret` call happens on blur (or "Save" click),
// and the plaintext is held only in component-local reactive state.
const passwordDrafts = reactive<Record<number, string>>({});

function getPasswordDraft(index: number): string {
  return passwordDrafts[index] ?? '';
}

function setPasswordDraft(index: number, value: string) {
  passwordDrafts[index] = value;
}

async function commitPasswordField(index: number) {
  const conn = userSettingsStore.userSettings.meridian.clusterWorkers[index];
  if (!conn) return;
  const draft = passwordDrafts[index];
  const trimmed = (draft ?? '').trim();

  // Empty input -> clear any existing secure key.
  if (!trimmed) {
    if (conn.passwordSecureKey) {
      await clearSshPassword(conn.passwordSecureKey);
      conn.passwordSecureKey = undefined;
      persist();
    }
    passwordDrafts[index] = '';
    return;
  }

  // Reuse existing key when present (in-place rotation); otherwise mint one.
  const key = await storeSshPassword(trimmed, conn.passwordSecureKey);
  conn.passwordSecureKey = key;
  persist();
  passwordDrafts[index] = '';
}

function addConnection() {
  userSettingsStore.userSettings.meridian.clusterWorkers.push({
    host: '',
    label: '',
    port: 22,
    username: '',
    keyPath: '',
    authMethod: 'key',
  });
  persist();
}

async function removeConnection(index: number) {
  const conn = userSettingsStore.userSettings.meridian.clusterWorkers[index];
  if (conn?.passwordSecureKey) {
    await clearSshPassword(conn.passwordSecureKey);
  }
  delete passwordDrafts[index];
  userSettingsStore.userSettings.meridian.clusterWorkers.splice(index, 1);
  persist();
}

async function setAuthMethod(index: number, method: SshAuthMethod) {
  const conn = userSettingsStore.userSettings.meridian.clusterWorkers[index];
  if (!conn) return;
  const prevAuth = conn.authMethod;
  conn.authMethod = method;
  // Drop the secure-store entry when the user is no longer using password
  // auth so it doesn't linger forever on disk for stale credentials.
  if (prevAuth === 'password' && method === 'key' && conn.passwordSecureKey) {
    await clearSshPassword(conn.passwordSecureKey);
    conn.passwordSecureKey = undefined;
  }
  persist();
}
</script>

<template>
  <SettingsItem
    :title="t('settings.meridian.cluster.title')"
    :description="t('settings.meridian.cluster.description')"
    :icon="ServerIcon"
  >
    <div class="cluster-nodes-settings">
      <div
        v-for="(conn, index) in connections"
        :key="index"
        class="cluster-nodes-settings__card"
      >
        <div class="cluster-nodes-settings__row">
          <div class="cluster-nodes-settings__field">
            <label class="cluster-nodes-settings__label">{{ t('settings.meridian.cluster.label') }}</label>
            <Input
              :model-value="conn.label"
              placeholder="MAMBA"
              @update:model-value="(v) => updateField(index, 'label', v)"
            />
          </div>
          <div class="cluster-nodes-settings__field">
            <label class="cluster-nodes-settings__label">{{ t('settings.meridian.cluster.host') }}</label>
            <Input
              :model-value="conn.host"
              placeholder="192.168.1.67"
              @update:model-value="(v) => updateField(index, 'host', v)"
            />
          </div>
          <div class="cluster-nodes-settings__field cluster-nodes-settings__field--port">
            <label class="cluster-nodes-settings__label">{{ t('settings.meridian.cluster.port') }}</label>
            <Input
              :model-value="String(conn.port)"
              placeholder="22"
              @update:model-value="(v) => updateField(index, 'port', v)"
            />
          </div>
        </div>
        <div class="cluster-nodes-settings__row">
          <div class="cluster-nodes-settings__field">
            <label class="cluster-nodes-settings__label">{{ t('settings.meridian.cluster.username') }}</label>
            <Input
              :model-value="conn.username"
              placeholder="username"
              @update:model-value="(v) => updateField(index, 'username', v)"
            />
          </div>
          <div class="cluster-nodes-settings__field cluster-nodes-settings__field--auth">
            <label class="cluster-nodes-settings__label">Auth method</label>
            <div class="cluster-nodes-settings__toggle">
              <button
                type="button"
                class="cluster-nodes-settings__toggle-btn"
                :class="{ 'cluster-nodes-settings__toggle-btn--active': conn.authMethod === 'key' }"
                title="Key file authentication"
                @click="setAuthMethod(index, 'key')"
              >
                <KeyRoundIcon :size="14" />
                Key file
              </button>
              <button
                type="button"
                class="cluster-nodes-settings__toggle-btn"
                :class="{ 'cluster-nodes-settings__toggle-btn--active': conn.authMethod === 'password' }"
                title="Password authentication"
                @click="setAuthMethod(index, 'password')"
              >
                <LockIcon :size="14" />
                Password
              </button>
            </div>
          </div>
          <Button
            variant="ghost"
            size="icon"
            class="cluster-nodes-settings__remove"
            :title="t('settings.meridian.cluster.remove')"
            @click="removeConnection(index)"
          >
            <Trash2Icon :size="16" />
          </Button>
        </div>
        <div class="cluster-nodes-settings__row">
          <div v-if="conn.authMethod === 'key'" class="cluster-nodes-settings__field cluster-nodes-settings__field--wide">
            <label class="cluster-nodes-settings__label">{{ t('settings.meridian.cluster.keyPath') }}</label>
            <Input
              :model-value="conn.keyPath"
              placeholder="C:\Users\name\.ssh\id_ed25519"
              @update:model-value="(v) => updateField(index, 'keyPath', v)"
            />
          </div>
          <div v-else class="cluster-nodes-settings__field cluster-nodes-settings__field--wide">
            <label class="cluster-nodes-settings__label">Password</label>
            <div class="cluster-nodes-settings__password-row">
              <input
                type="password"
                class="cluster-nodes-settings__password-input"
                :value="getPasswordDraft(index)"
                :placeholder="conn.passwordSecureKey ? '•••••• Enter new password to replace' : '••••••'"
                autocomplete="off"
                @input="(e) => setPasswordDraft(index, (e.target as HTMLInputElement).value)"
                @blur="commitPasswordField(index)"
                @keydown.enter="(e) => { (e.target as HTMLInputElement).blur(); }"
              />
              <span
                v-if="conn.passwordSecureKey"
                class="cluster-nodes-settings__password-status"
                title="A password is securely stored for this connection"
              >
                <ShieldCheckIcon :size="13" />
                Encrypted
              </span>
            </div>
          </div>
        </div>
      </div>

      <Button
        variant="secondary"
        class="cluster-nodes-settings__add"
        @click="addConnection"
      >
        <PlusIcon :size="16" />
        {{ t('settings.meridian.cluster.add') }}
      </Button>
    </div>
  </SettingsItem>
</template>

<style scoped>
/* Intentionally co-named with the SSH Connections settings page so the
   styles are 1:1 compatible. Both settings pages have the same form UX
   and live in the same Settings → Meridian category; sharing a style
   block keeps visual consistency without copy-pasting the entire <style>
   block into a shared CSS file. The prefix `cluster-nodes-settings__`
   is unique enough not to collide with `ssh-settings__` rules. */
.cluster-nodes-settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.cluster-nodes-settings__card {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  padding: 0.75rem;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
}

.cluster-nodes-settings__row {
  display: flex;
  align-items: flex-end;
  gap: 0.5rem;
}

.cluster-nodes-settings__field {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}

.cluster-nodes-settings__field--port {
  flex: 0 0 70px;
}

.cluster-nodes-settings__field--auth {
  flex: 0 0 auto;
}

.cluster-nodes-settings__field--wide {
  flex: 2;
}

.cluster-nodes-settings__label {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
}

.cluster-nodes-settings__toggle {
  display: inline-flex;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background));
}

.cluster-nodes-settings__toggle-btn {
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

.cluster-nodes-settings__toggle-btn + .cluster-nodes-settings__toggle-btn {
  border-left: 1px solid hsl(var(--border));
}

.cluster-nodes-settings__toggle-btn:hover {
  background: hsl(var(--button-hover, var(--background-2)));
  color: hsl(var(--foreground));
}

.cluster-nodes-settings__toggle-btn--active {
  background: hsl(var(--primary) / 15%);
  color: hsl(var(--foreground));
  font-weight: 600;
}

.cluster-nodes-settings__password-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.cluster-nodes-settings__password-input {
  flex: 1;
  width: 100%;
  padding: 0.35rem 0.55rem;
  font-size: 0.85rem;
  background: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  color: hsl(var(--foreground));
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.cluster-nodes-settings__password-input:focus {
  border-color: hsl(var(--primary));
  box-shadow: 0 0 0 3px hsl(var(--primary) / 20%);
}

.cluster-nodes-settings__password-status {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.7rem;
  color: #34d399;
  flex-shrink: 0;
}

.cluster-nodes-settings__remove {
  flex-shrink: 0;
}

.cluster-nodes-settings__add {
  align-self: flex-start;
  gap: 0.375rem;
}
</style>
