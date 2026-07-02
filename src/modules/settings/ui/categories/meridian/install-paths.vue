<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
// Wiring note: the Rust layer (`src-tauri/src/backend_manager.rs::download_backend`
// → `resolve_github_release_url`) reads this value via the `download_backend`
// Tauri command's optional `githubToken` argument; the consumer in
// `src/modules/backend-manager/pages/backend-manager.vue` already wires
// the value into the invoke payload.

import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { KeyIcon, EyeIcon, EyeOffIcon, Trash2Icon } from '@lucide/vue';
import { SettingsItem } from '@/modules/settings';
import { Input } from '@/components/ui/input';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

const { t } = useI18n();
const userSettingsStore = useUserSettingsStore();
const showToken = ref(false);

// `meridian.githubToken` is `string | undefined`. The schema migration
// 28→29 backfills `''` on disk for fresh installs; the in-memory Pinia
// default does NOT include `githubToken` so the field's first paint may
// legitimately be `undefined`. The `?? ''` makes that round-trip into
// a stable empty-string starting state for the input control.
const githubTokenDraft = computed<string>({
  get: () => userSettingsStore.userSettings.meridian?.githubToken ?? '',
  set: (value: string) => {
    const trimmed = value.trim();
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
    <div class="exo-card exo-card--mamba">
      <div class="exo-tile" aria-hidden="true">
        <KeyIcon :size="28" class="exo-tile__icon" />
      </div>
      <div class="exo-identity">
        <span class="exo-identity__title">Install Paths &amp; Auth</span>
        <span class="exo-identity__sub">
          {{ githubTokenDraft ? 'GitHub PAT configured' : 'No PAT configured' }}
        </span>
      </div>
      <div class="exo-specs">
        <div class="exo-specs__field">
          <label class="exo-specs__label" for="install-paths-github-token">
            GitHub Personal Access Token
          </label>
          <Input
            id="install-paths-github-token"
            v-model="githubTokenDraft"
            :type="showToken ? 'text' : 'password'"
            placeholder="ghp_…"
            class="exo-specs__input"
            autocomplete="off"
            spellcheck="false"
          />
          <div class="exo-card-hint">
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
          <div class="exo-card-hint" style="margin-top: 0.2rem;">
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
      <div class="exo-actions">
        <button
          type="button"
          class="exo-actions__btn"
          :title="showToken ? 'Hide token' : 'Show token'"
          :aria-pressed="showToken"
          @click="showToken = !showToken"
        >
          <component :is="showToken ? EyeOffIcon : EyeIcon" :size="14" />
          {{ showToken ? 'Hide' : 'Show' }}
        </button>
        <button
          type="button"
          class="exo-actions__btn exo-actions__btn--danger"
          title="Clear saved token"
          :disabled="!githubTokenDraft"
          @click="clearToken"
        >
          <Trash2Icon :size="14" />
          Clear
        </button>
      </div>
    </div>
  </SettingsItem>
</template>

<style scoped>
/* .exo-card-hint (with `code` and `a` descendants) lives in
   src/styles/exo.css — globally scoped so this card AND every other
   settings sub-panel shares the same muted boilerplate look. The
   password-type input styling is inherited from `.exo-specs__input`
   (in exo.css) via the class on the Input component. This scoped
   block intentionally has no local rules. */
</style>
