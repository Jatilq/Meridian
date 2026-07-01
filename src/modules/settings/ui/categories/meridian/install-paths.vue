<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
// Wiring note: the Rust layer (`src-tauri/src/backend_manager.rs::download_backend`
// → `resolve_github_release_url`) reads this value via the `download_backend`
// Tauri command's optional `githubToken` argument; the consumer in
// `src/modules/backend-manager/pages/backend-manager.vue:550` already wires
// the value into the invoke payload. The wiring BREAK before this commit was:
// NO Vue component was writing `meridian.githubToken` to the lazy store, so
// the consumer always received `''` and silently downgraded to anonymous
// (60 req/hr GitHub anonymous rate limit). This panel closes that loop —
// typing here commits to disk via
// `userSettingsStore.setUserSettingsStorage('meridian.githubToken', trimmed)`,
// matching the convention used by `ai-panel.vue` / `downloader.vue`
// (direct in-memory mutation + direct storage write, no double-rewrite).
//
// The Rust error message tells users where to look: "Configure a GitHub
// Personal Access Token in Settings > Advanced > Install Paths (githubToken)".
// That's the user-facing breadcrumb this UI satisfies.

import { computed, ref } from 'vue';
import { KeyIcon, EyeIcon, EyeOffIcon, Trash2Icon } from '@lucide/vue';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const userSettingsStore = useUserSettingsStore();
const showToken = ref(false);

// `meridian.githubToken` is `string | undefined`. The schema migration
// 28→29 backfills `''` on disk for fresh installs (`src/stores/schemas/user-settings.ts:684`).
// The in-memory Pinia default
// (`src/stores/storage/user-settings.ts:393` — the `meridian: { ... }` literal)
// does NOT include `githubToken` as a key, so the field's first paint may
// legitimately be `undefined` until the lazy-store load replaces that
// literal in place. The `?? ''` makes that round-trip into a stable
// empty-string starting state for the input control, regardless of whether
// the lazy-store load has landed yet by the time this panel first renders.
const githubTokenDraft = computed<string>({
  get: () => userSettingsStore.userSettings.meridian?.githubToken ?? '',
  set: (value: string) => {
    const trimmed = value.trim();
    // Mirror the ai-panel.vue / downloader.vue convention: direct in-memory
    // mutation followed by a direct lazy-store write. Avoids the double
    // disk-write path of `userSettingsStore.set()` (which mutates in-memory
    // AND re-walks the nested path before persisting).
    userSettingsStore.userSettings.meridian.githubToken = trimmed;
    userSettingsStore.setUserSettingsStorage('meridian.githubToken', trimmed);
  },
});

function clearToken() {
  githubTokenDraft.value = '';
}
</script>

<template>
  <SettingsItem
    title="Install Paths &amp; Auth"
    description="Configure install roots and GitHub API authentication for the Backend Manager."
    :icon="KeyIcon"
  >
    <div class="install-paths-settings">
      <div class="install-paths-settings__field">
        <label
          class="install-paths-settings__label"
          for="install-paths-github-token"
        >
          GitHub Personal Access Token
        </label>
        <div class="install-paths-settings__token-row">
          <Input
            id="install-paths-github-token"
            v-model="githubTokenDraft"
            :type="showToken ? 'text' : 'password'"
            placeholder="ghp_…"
            class="install-paths-settings__input"
            autocomplete="off"
            spellcheck="false"
          />
          <button
            type="button"
            class="install-paths-settings__icon-btn"
            :title="showToken ? 'Hide token' : 'Show token'"
            :aria-pressed="showToken"
            @click="showToken = !showToken"
          >
            <component :is="showToken ? EyeOffIcon : EyeIcon" />
          </button>
          <button
            type="button"
            class="install-paths-settings__icon-btn"
            title="Clear saved token"
            :disabled="!githubTokenDraft"
            @click="clearToken"
          >
            <Trash2Icon />
          </button>
        </div>
        <div class="install-paths-settings__hint">
          <span v-if="githubTokenDraft">
            A token is configured.
            The next Backend Manager release check or runtime install will
            send <code>Authorization: Bearer …</code> on the GitHub Releases
            API call, lifting the rate limit to 5000&nbsp;req/hr.
          </span>
          <span v-else>
            No token configured.
            The Backend Manager will use GitHub's anonymous rate limit
            (60&nbsp;requests/hr). Adding a token lifts the ceiling to
            5000&nbsp;req/hr and prevents burst-failures on multi-backend installs.
          </span>
        </div>
        <div class="install-paths-settings__subhint">
          Required scopes: none — the GitHub Releases resolver is a
          read-only public API, so a token with zero scopes is enough.
          Generate one at
          <a
            href="https://github.com/settings/tokens"
            target="_blank"
            rel="noreferrer noopener"
          >github.com/settings/tokens</a>
          (no scopes, expiration up to you).
        </div>
      </div>
    </div>
  </SettingsItem>
</template>

<style scoped>
.install-paths-settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.install-paths-settings__field {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

.install-paths-settings__label {
  color: hsl(var(--foreground));
  font-size: 0.875rem;
  font-weight: 500;
}

.install-paths-settings__token-row {
  display: flex;
  align-items: center;
  gap: 0.4rem;
}

.install-paths-settings__input {
  flex: 1;
  /* Monospace so the ghp_ prefix reads as a token, not a sentence. */
  font-family: var(--font-mono, monospace);
}

.install-paths-settings__icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 2rem;
  height: 2rem;
  border-radius: var(--radius-sm);
  border: 1px solid hsl(var(--border));
  background: hsl(var(--background-3));
  color: hsl(var(--muted-foreground));
  cursor: pointer;
  flex-shrink: 0;
}

.install-paths-settings__icon-btn:hover:not(:disabled) {
  color: hsl(var(--foreground));
  background: hsl(var(--foreground) / 5%);
}

.install-paths-settings__icon-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.install-paths-settings__hint {
  color: hsl(var(--muted-foreground));
  font-size: 0.75rem;
  line-height: 1.35;
}

.install-paths-settings__hint code {
  font-family: var(--font-mono, monospace);
  font-size: 0.7rem;
  padding: 0.05rem 0.25rem;
  border-radius: 0.25rem;
  background: hsl(var(--background-3));
  color: hsl(var(--foreground));
}

.install-paths-settings__subhint {
  color: hsl(var(--muted-foreground));
  font-size: 0.7rem;
  margin-top: 0.25rem;
  line-height: 1.35;
}

.install-paths-settings__subhint a {
  color: hsl(var(--primary));
  text-decoration: underline;
}
</style>
