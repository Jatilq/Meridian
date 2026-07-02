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
import { hostThemeKey } from '@/utils/exo-theme';
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
  field: Exclude<keyof SshConnectionSetting, 'passwordSecureKey'>,
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
  const conn = userSettingsStore.userSettings.meridian.sshConnections[index];
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
  userSettingsStore.userSettings.meridian.sshConnections.push({
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
  const conn = userSettingsStore.userSettings.meridian.sshConnections[index];
  if (conn?.passwordSecureKey) {
    await clearSshPassword(conn.passwordSecureKey);
  }
  delete passwordDrafts[index];
  userSettingsStore.userSettings.meridian.sshConnections.splice(index, 1);
  persist();
}

async function setAuthMethod(index: number, method: SshAuthMethod) {
  const conn = userSettingsStore.userSettings.meridian.sshConnections[index];
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
    :title="t('settings.meridian.ssh.title')"
    :description="t('settings.meridian.ssh.description')"
    :icon="ServerIcon"
  >
    <div class="ssh-settings">
      <article
        v-for="(conn, index) in connections"
        :key="index"
        class="exo-card"
        :class="`exo-card--${hostThemeKey(conn.label || conn.host)}`"
      >
        <div class="exo-tile" aria-hidden="true">
          <ServerIcon :size="28" class="exo-tile__icon" />
          <span
            class="exo-tile__led"
            :class="{ 'exo-tile__led--installed': conn.passwordSecureKey || conn.keyPath }"
          />
        </div>

        <div class="exo-identity">
          <span class="exo-identity__title">{{ conn.label || 'New connection' }}</span>
          <span class="exo-identity__sub">
            {{ conn.username || 'user' }}@{{ conn.host || 'host' }}:{{ conn.port }}
          </span>
        </div>

        <div class="exo-specs exo-specs--two-col">
          <div class="exo-specs__field">
            <label class="exo-specs__label">Auth method</label>
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
          <div class="exo-specs__field">
            <label v-if="conn.authMethod === 'key'" class="exo-specs__label">
              Key file path
            </label>
            <label v-else class="exo-specs__label">
              Password
            </label>
            <div v-if="conn.authMethod === 'key'" class="ssh-settings__row">
              <Input
                :model-value="conn.keyPath"
                placeholder="C:\Users\name\.ssh\id_ed25519"
                @update:model-value="(v) => updateField(index, 'keyPath', v)"
              />
            </div>
            <div v-else class="ssh-settings__password-row">
              <input
                type="password"
                class="ssh-settings__password-input"
                :value="getPasswordDraft(index)"
                :placeholder="conn.passwordSecureKey ? '•••••• Enter new password to replace' : '••••••'"
                autocomplete="off"
                @input="(e) => setPasswordDraft(index, (e.target as HTMLInputElement).value)"
                @blur="commitPasswordField(index)"
                @keydown.enter="(e) => { (e.target as HTMLInputElement).blur(); }"
              />
              <span
                v-if="conn.passwordSecureKey"
                class="ssh-settings__password-status"
                title="A password is securely stored for this connection"
              >
                <ShieldCheckIcon :size="13" />
                Encrypted
              </span>
            </div>
          </div>
        </div>

        <div class="exo-actions">
          <Button
            variant="ghost"
            size="icon"
            class="exo-actions__btn exo-actions__btn--danger"
            :title="t('settings.meridian.ssh.remove')"
            @click="removeConnection(index)"
          >
            <Trash2Icon :size="16" />
          </Button>
        </div>
      </article>

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
  gap: 0.75rem;
}

/* Auth-method toggle group — visual parity with cluster-nodes but lives
   in its own scoped styles block because the SettingsItem wrapper
   styles this card independently. */
.ssh-settings__toggle {
  display: inline-flex;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background));
  align-self: flex-start;
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
  transition: background 0.15s ease, color 0.15s ease;
  font-family: inherit;
}
.ssh-settings__toggle-btn + .ssh-settings__toggle-btn {
  border-left: 1px solid hsl(var(--border));
}
.ssh-settings__toggle-btn:hover {
  background: hsl(var(--foreground) / 5%);
  color: hsl(var(--foreground));
}
.ssh-settings__toggle-btn--active {
  background: hsl(var(--primary) / 18%);
  color: hsl(var(--foreground));
  font-weight: 600;
  border-bottom: 2px solid hsl(var(--primary));
}

.ssh-settings__row {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}
.ssh-settings__password-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.ssh-settings__password-input {
  flex: 1;
  width: 100%;
  padding: 0.45rem 0.55rem;
  font-size: 0.8rem;
  font-family: var(--font-mono, monospace);
  background: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-sm);
  color: hsl(var(--foreground));
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}
.ssh-settings__password-input:focus {
  border-color: hsl(var(--rt-accent, var(--primary)));
  box-shadow: 0 0 0 3px hsl(var(--rt-accent, var(--primary)) / 20%);
}
.ssh-settings__password-status {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.7rem;
  color: #34d399;
  flex-shrink: 0;
}

.ssh-settings__add {
  align-self: flex-start;
  gap: 0.375rem;
}
</style>
