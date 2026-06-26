<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { BotIcon } from '@lucide/vue';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip';
import { useShortcutsStore } from '@/stores/runtime/shortcuts';
import { useAiPanelStore } from '@/stores/runtime/ai-panel';

const { t } = useI18n();
const shortcutsStore = useShortcutsStore();
const aiPanelStore = useAiPanelStore();

function handleClick() {
  aiPanelStore.toggle();
}
</script>

<template>
  <div class="ai-panel-toolbar-button animate-fade-in">
    <Tooltip>
      <TooltipTrigger as-child>
        <Button
          variant="ghost"
          size="icon"
          class="ai-panel-toolbar-button__button"
          :class="{ 'ai-panel-toolbar-button__button--active': aiPanelStore.isOpen }"
          @click="handleClick"
        >
          <BotIcon
            :size="16"
            class="ai-panel-toolbar-button__icon"
          />
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        <div class="ai-panel-toolbar-button__tooltip-row">
          {{ t('aiPanel.title') }}
        </div>
      </TooltipContent>
    </Tooltip>
  </div>
</template>

<style scoped>
.ai-panel-toolbar-button :deep(.sigma-ui-button) {
  width: 28px;
  height: 28px;
}

.ai-panel-toolbar-button__icon {
  stroke: hsl(var(--foreground) / 50%);
}

.ai-panel-toolbar-button__button--active {
  background-color: hsl(var(--secondary));
}

.ai-panel-toolbar-button__button--active .ai-panel-toolbar-button__icon {
  stroke: hsl(var(--primary));
}
</style>
