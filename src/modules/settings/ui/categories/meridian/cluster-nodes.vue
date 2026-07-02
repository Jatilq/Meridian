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
      <article
        v-for="(conn, index) in connections"
        :key="index"
        class="exo-card"
        :class="`exo-card--${hostThemeKey(conn.label || conn.host)}`"
      >
        <!-- Tile: ServerIcon themed from the row's accent token. Falls
             back to a gradient block when the host doesn't match any
             known machine. The LED inside the tile reflects whether
             a password is currently encrypted (i.e. the connection is
             configured end-to-end). -->
        <div class="exo-tile" aria-hidden="true">
          <ServerIcon :size="28" class="exo-tile__icon" />
          <span
            class="exo-tile__led"
            :class="{ 'exo-tile__led--installed': conn.passwordSecureKey || conn.keyPath }"
          />
        </div>

        <!-- Identity: connection label (gradient title) + user@host:port (mono sub). -->
        <div class="exo-identity">
          <span class="exo-identity__title">{{ conn.label || 'New worker' }}</span>
          <span class="exo-identity__sub">
            {{ conn.username || 'user' }}@{{ conn.host || 'host' }}:{{ conn.port }}
          </span>
        </div>

        <!-- Specs: 2-up grid with auth-method toggle (left) + key/password field (right).
             The key/password field swaps via v-if so the username/key-or-password pair
             always fits a single row at standard widths. -->
        <div class="exo-specs exo-specs--two-col">
          <div class="exo-specs__field">
            <label class="exo-specs__label">Auth method</label>
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
          <div class="exo-specs__field">
            <label v-if="conn.authMethod === 'key'" class="exo-specs__label">
              Key file path
            </label>
            <label v-else class="exo-specs__label">
              Password
            </label>
            <div v-if="conn.authMethod === 'key'" class="cluster-nodes-settings__row">
              <Input
                :model-value="conn.keyPath"
                placeholder="C:\Users\name\.ssh\id_ed25519"
                @update:model-value="(v) => updateField(index, 'keyPath', v)"
              />
            </div>
            <div v-else class="cluster-nodes-settings__password-row">
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

        <!-- Actions: Trash only. (Test + Save are on the modal launched from
             Cluster Control's "Add Worker" CTA, not the settings panel —
             keeping this row lean so the trash button has space to breathe.) -->
        <div class="exo-actions">
          <Button
            variant="ghost"
            size="icon"
            class="exo-actions__btn exo-actions__btn--danger"
            :title="t('settings.meridian.cluster.remove')"
            @click="removeConnection(index)"
          >
            <Trash2Icon :size="16" />
          </Button>
        </div>
      </article>

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
.cluster-nodes-settings {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

/* Auth-method toggle group (used inside the specs column of each card).
   Co-named with the older dedicated styles so the day-1 settings UI
   still matches wherever it might be consumed outside this card. */
.cluster-nodes-settings__toggle {
  display: inline-flex;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background));
  align-self: flex-start;
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
  transition: background 0.15s ease, color 0.15s ease;
  font-family: inherit;
}
.cluster-nodes-settings__toggle-btn + .cluster-nodes-settings__toggle-btn {
  border-left: 1px solid hsl(var(--border));
}
.cluster-nodes-settings__toggle-btn:hover {
  background: hsl(var(--foreground) / 5%);
  color: hsl(var(--foreground));
}
.cluster-nodes-settings__toggle-btn--active {
  background: hsl(var(--primary) / 18%);
  color: hsl(var(--foreground));
  font-weight: 600;
  border-bottom: 2px solid hsl(var(--primary));
}

/* Single-row container when keyPath auth is selected — keeps the row
   compact inside the 2-up grid. */
.cluster-nodes-settings__row {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}

/* Password-then-input row with an "Encrypted" status indicator. */
.cluster-nodes-settings__password-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.cluster-nodes-settings__password-input {
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
.cluster-nodes-settings__password-input:focus {
  border-color: hsl(var(--rt-accent, var(--primary)));
  box-shadow: 0 0 0 3px hsl(var(--rt-accent, var(--primary)) / 20%);
}
.cluster-nodes-settings__password-status {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  font-size: 0.7rem;
  color: #34d399;
  flex-shrink: 0;
}

.cluster-nodes-settings__add {
  align-self: flex-start;
  gap: 0.375rem;
}
</style>
