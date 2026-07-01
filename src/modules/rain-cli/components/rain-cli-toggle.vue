<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
/**
 * Floating button to toggle the Rain slide-in overlay from any page.
 * Positioned at the bottom-right corner of the content area.
 */

import { useAiPanelStore } from '@/stores/runtime/ai-panel';
import { BotIcon } from '@lucide/vue';

const aiPanelStore = useAiPanelStore();

function toggle() {
  aiPanelStore.toggle();
}
</script>

<template>
  <button
    class="rain-toggle"
    :class="{ 'rain-toggle--active': aiPanelStore.isOpen }"
    :title="aiPanelStore.isOpen ? 'Close Rain panel' : 'Open Rain panel'"
    @click="toggle"
  >
    <BotIcon :size="18" />
  </button>
</template>

<style scoped>
.rain-toggle {
  position: fixed;
  bottom: 20px;
  right: 20px;
  z-index: 100;
  width: 40px;
  height: 40px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: hsl(var(--primary));
  color: hsl(var(--primary-foreground));
  border: none;
  cursor: pointer;
  box-shadow: 0 2px 12px hsl(var(--primary) / 30%);
  transition: all 0.15s ease;
}

.rain-toggle:hover {
  transform: scale(1.08);
  box-shadow: 0 3px 16px hsl(var(--primary) / 40%);
}

.rain-toggle--active {
  background: hsl(var(--foreground) / 10%);
  color: hsl(var(--primary));
  box-shadow: 0 2px 8px hsl(var(--primary) / 20%);
}
</style>
