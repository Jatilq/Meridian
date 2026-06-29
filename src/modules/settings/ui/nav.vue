<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { SettingsNavItem } from '@/modules/settings';
import { useSettingsStore } from '@/stores/runtime/settings';

const settingsStore = useSettingsStore();
</script>

<template>
  <div
    v-if="!settingsStore.search"
    class="settings-nav"
  >
    <div class="settings-nav__items">
      <SettingsNavItem
        v-for="tab in settingsStore.tabs"
        :key="tab.name"
        :name="tab.name"
        :label="tab.label"
      />
    </div>
  </div>
</template>

<style scoped>
.settings-nav {
  display: flex;
  flex-direction: column;
  align-self: stretch;
  /* The nav is a grid item inside `.settings-content__inner` which has
     `height: 100%`. Switching the cap from `100vh` (which ignores the
     actual grid cell height) to `100%` (which respects it) prevents
     the bleed-past-overflow-hidden bug: when the grid cell is shorter
     than 100vh - toolbar, the nav used to be capped at the larger
     viewport-derived value and clipped by the parent's overflow. Now
     the nav's scrollable area matches the actual available space.
     `position: sticky` is intentionally dropped — the nav lives in a
     grid that already handles its own vertical context. */
  min-height: 0;
  max-height: 100%;
  padding-right: 1rem;
  border-right: 1px solid hsl(var(--border));
  gap: 1rem;
  overflow-y: auto;
}

.settings-nav__items {
  display: flex;
  flex-direction: column;
  padding: 4px;
  gap: 0.25rem;
}

@media (width <= 768px) {
  .settings-nav {
    max-height: none;
    padding-right: 0;
    border-right: none;
    overflow-y: visible;
  }
}
</style>
