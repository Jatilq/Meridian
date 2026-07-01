<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed, provide, ref } from 'vue';
import { useRouter } from 'vue-router';
import { BlocksIcon, HardDriveIcon, NetworkIcon, UsbIcon } from '@lucide/vue';
import { useAppStore } from '@/stores/runtime/app';
import { useExtensionsStore } from '@/stores/runtime/extensions';
import {
  BUILTIN_NAVIGATION_PAGE_SHORTCUTS,
  useShortcutsStore,
} from '@/stores/runtime/shortcuts';
import { useUserSettingsStore } from '@/stores/storage/user-settings';
import { useWorkspacesStore } from '@/stores/storage/workspaces';
import { useDrives } from '@/modules/home/composables';
import { DriveCard } from '@/modules/home/components';
import { getLucideIcon } from '@/utils/lucide-icons';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { Button } from '@/components/ui/button';
import { ContextMenuShortcut } from '@/components/ui/context-menu';
import { CONTEXT_MENU_OPEN_COUNT_KEY } from '@/components/dir-entry-interactive';
import { formatKeybindingKeys } from '@/modules/extensions/api';
import QuickAccessPanel from './components/quick-access-panel.vue';
import UbuntuWslIcon from '@/components/icons/ubuntu-wsl-icon.vue';
import { useTextDirection } from '@/composables/use-text-direction';

const router = useRouter();
const appStore = useAppStore();
const extensionsStore = useExtensionsStore();
const shortcutsStore = useShortcutsStore();
const userSettingsStore = useUserSettingsStore();
const workspacesStore = useWorkspacesStore();
const { drives } = useDrives();
const { inlineEndSide } = useTextDirection();

const quickAccessOnHover = computed(() => userSettingsStore.userSettings.quickAccessOnHover);

const quickAccessContextMenuOpenCount = ref(0);
provide(CONTEXT_MENU_OPEN_COUNT_KEY, quickAccessContextMenuOpenCount);

const quickAccessHoverOpen = ref(false);
const quickAccessTooltipOpen = computed(() =>
  quickAccessHoverOpen.value || quickAccessContextMenuOpenCount.value > 0,
);

function handleQuickAccessTooltipOpenChange(value: boolean) {
  quickAccessHoverOpen.value = value;
}

function isDashboardPage(item: { name?: unknown }) {
  return item.name === 'dashboard';
}

const sortedExtensionPages = computed(() => {
  return [...extensionsStore.sidebarPages].sort((a, b) => {
    const orderA = a.page.order ?? 0;
    const orderB = b.page.order ?? 0;
    return orderA - orderB;
  });
});

function isExtensionPageActive(pageId: string) {
  return router.currentRoute.value.name === 'extension-page'
    && router.currentRoute.value.params.fullPageId === pageId;
}

function openExtensionPage(pageId: string) {
  router.push({
    name: 'extension-page',
    params: { fullPageId: pageId },
  });
}

function getPageShortcutLabel(routeName: unknown): string {
  const shortcut = BUILTIN_NAVIGATION_PAGE_SHORTCUTS.find(
    item => item.routeName === routeName,
  );

  return shortcut ? shortcutsStore.getShortcutLabel(shortcut.id) : '';
}

function getExtensionPageShortcutLabel(pageId: string): string {
  const keybinding = extensionsStore.getSidebarPageKeybinding(pageId);
  return keybinding?.keys?.key ? formatKeybindingKeys(keybinding.keys) : '';
}

async function openDrive(path: string) {
  if (router.currentRoute.value.name === 'navigator') {
    await workspacesStore.openPathInCurrentTab(path);
    return;
  }

  await workspacesStore.openNewTabGroup(path);
  await router.push({ name: 'navigator' });
}

function getDriveIcon(drive: {
  drive_type: string;
  is_removable: boolean;
}) {
  if (drive.drive_type === 'Network') {
    return NetworkIcon;
  }

  return drive.is_removable ? UsbIcon : HardDriveIcon;
}
</script>

<template>
  <div
    class="nav-sidebar"
    data-e2e-root="nav-sidebar"
  >
    <div
      class="nav-sidebar-header"
      data-tauri-drag-region
    >
      <div class="nav-sidebar-header-logo">
        <img
          data-tauri-drag-region
          src="@/assets/icons/logo-32x32.png"
          width="20"
          height="20"
        >
      </div>
    </div>

    <div class="nav-sidebar-items">
      <template
        v-for="(item, index) in appStore.pages"
        :key="index"
      >
        <Tooltip
          v-if="isDashboardPage(item) && quickAccessOnHover"
          :open="quickAccessTooltipOpen"
          @update:open="handleQuickAccessTooltipOpenChange"
        >
          <TooltipTrigger as-child>
            <Button
              class="nav-sidebar-item"
              :class="{ 'nav-sidebar-item--active': item.name === router.currentRoute.value.name }"
              size="sm"
              variant="ghost"
              :value="item.name"
              :is-active="item.name === router.currentRoute.value.name"
              @click="router.push({ name: item.name })"
            >
              <component
                :is="item.icon"
                :size="16"
                class="nav-sidebar-item-icon"
              />
              <span class="nav-sidebar-item-label">{{ item.title }}</span>
            </Button>
          </TooltipTrigger>
          <TooltipContent
            :side="inlineEndSide"
            align="start"
            :side-offset="12"
            :collision-padding="6"
            class="nav-sidebar__quick-access-tooltip"
          >
            <div class="nav-sidebar__quick-access-title">
              <div class="nav-sidebar__tooltip-row">
                <span>{{ item.title }}</span>
                <ContextMenuShortcut v-if="getPageShortcutLabel(item.name)">
                  {{ getPageShortcutLabel(item.name) }}
                </ContextMenuShortcut>
              </div>
            </div>
            <QuickAccessPanel />
          </TooltipContent>
        </Tooltip>
        <Tooltip
          v-else
        >
          <TooltipTrigger as-child>
            <Button
              class="nav-sidebar-item"
              :class="{ 'nav-sidebar-item--active': item.name === router.currentRoute.value.name }"
              size="sm"
              variant="ghost"
              :value="item.name"
              :is-active="item.name === router.currentRoute.value.name"
              @click="router.push({ name: item.name })"
            >
              <component
                :is="item.icon"
                :size="16"
                class="nav-sidebar-item-icon"
              />
              <span class="nav-sidebar-item-label">{{ item.title }}</span>
            </Button>
          </TooltipTrigger>
          <TooltipContent
            :side="inlineEndSide"
            :side-offset="12"
          >
            <div class="nav-sidebar__tooltip-row">
              <span>{{ item.title }}</span>
              <ContextMenuShortcut v-if="getPageShortcutLabel(item.name)">
                {{ getPageShortcutLabel(item.name) }}
              </ContextMenuShortcut>
            </div>
          </TooltipContent>
        </Tooltip>
      </template>

      <Tooltip
        v-for="registration in sortedExtensionPages"
        :key="registration.page.id"
      >
        <TooltipTrigger as-child>
          <Button
            class="nav-sidebar-item"
            size="icon"
            :is-active="isExtensionPageActive(registration.page.id)"
            @click="openExtensionPage(registration.page.id)"
          >
            <component
              :is="getLucideIcon(registration.page.icon) ?? BlocksIcon"
              :size="18"
              class="nav-sidebar-item-icon"
            />
          </Button>
        </TooltipTrigger>
        <TooltipContent
          :side="inlineEndSide"
          :side-offset="12"
        >
          <div class="nav-sidebar__tooltip-row">
            <span>{{ registration.page.title }}</span>
            <ContextMenuShortcut v-if="getExtensionPageShortcutLabel(registration.page.id)">
              {{ getExtensionPageShortcutLabel(registration.page.id) }}
            </ContextMenuShortcut>
          </div>
        </TooltipContent>
      </Tooltip>
    </div>

    <div class="nav-sidebar-spacer" />

    <div class="nav-sidebar-drives">
      <Tooltip
        v-for="drive in drives"
        :key="drive.path"
      >
        <TooltipTrigger as-child>
          <div
            class="nav-sidebar-drive"
            role="button"
            tabindex="0"
            @click="openDrive(drive.path)"
            @keydown.enter="openDrive(drive.path)"
            @keydown.space.prevent="openDrive(drive.path)"
          >
            <UbuntuWslIcon
              v-if="drive.drive_type === 'WSL'"
              :size="16"
              class="nav-sidebar-drive-icon"
            />
            <component
              v-else
              :is="getDriveIcon(drive)"
              :size="16"
              class="nav-sidebar-drive-icon"
            />
            <span class="nav-sidebar-drive-info">
              <span class="nav-sidebar-drive-label">{{ drive.name }}</span>
              <span v-if="drive.is_mounted && drive.total_space > 0" class="nav-sidebar-drive-usage">
                {{ drive.percent_used }}%
              </span>
            </span>
          </div>
        </TooltipTrigger>
        <TooltipContent
          :side="inlineEndSide"
          :side-offset="12"
          :collision-padding="6"
          class="nav-sidebar-drive-tooltip"
        >
          <DriveCard :drive="drive" />
        </TooltipContent>
      </Tooltip>
    </div>
  </div>
</template>

<style scoped>
.nav-sidebar {
  z-index: 10;
  display: flex;
  width: var(--nav-sidebar-width);
  height: calc(100vh - 12px);
  flex-direction: column;
  border-radius: var(--radius-sm);
  margin: 6px;
  background-color: hsl(var(--background-2));
}

.nav-sidebar-header {
  display: flex;
  height: var(--window-toolbar-height);
  height: 40px;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  margin-bottom: 12px;
  background-color: hsl(var(--background-2));
}

.nav-sidebar-items {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  padding: 4px;
  gap: 2px;
}

.nav-sidebar-spacer {
  flex: 1;
}

.nav-sidebar-item {
  display: flex;
  width: 100%;
  height: 32px;
  align-items: center;
  justify-content: flex-start;
  gap: 8px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  background-color: transparent;
  cursor: pointer;
  color: hsl(var(--foreground) / 55%);
  transition: background-color 0.15s ease, color 0.15s ease;
}

.nav-sidebar-item:hover {
  background-color: hsl(var(--foreground) / 5%);
  color: hsl(var(--foreground) / 85%);
}

.nav-sidebar-item--active,
.nav-sidebar-item[is-active="true"] {
  background-color: hsl(var(--primary) / 12%);
  color: hsl(var(--primary));
}

.nav-sidebar-item--active:hover,
.nav-sidebar-item[is-active="true"]:hover {
  background-color: hsl(var(--primary) / 18%);
  color: hsl(var(--primary));
}

.nav-sidebar-item-icon {
  flex-shrink: 0;
  stroke: currentColor;
}

.nav-sidebar-item-label {
  font-size: 0.8rem;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ─── Drives section ────────────────────────────────────────────────────── */

.nav-sidebar-drives {
  display: flex;
  flex-direction: column;
  flex-shrink: 0;
  padding: 4px;
  padding-bottom: 12px;
  gap: 2px;
}

.nav-sidebar-drive {
  display: flex;
  width: 100%;
  height: 28px;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  background-color: transparent;
  cursor: pointer;
  color: hsl(var(--foreground) / 55%);
  transition: background-color 0.15s ease, color 0.15s ease;
}

.nav-sidebar-drive:hover {
  background-color: hsl(var(--foreground) / 5%);
  color: hsl(var(--foreground) / 85%);
}

.nav-sidebar-drive-icon {
  flex-shrink: 0;
  width: 16px;
  height: 16px;
  color: inherit;
  stroke: currentColor;
}

.nav-sidebar-drive-info {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  flex: 1;
}

.nav-sidebar-drive-label {
  font-size: 0.72rem;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: inherit;
}

.nav-sidebar-drive-usage {
  font-size: 0.6rem;
  font-weight: 600;
  white-space: nowrap;
  color: hsl(var(--muted-foreground));
  flex-shrink: 0;
  min-width: max-content;
  font-family: var(--font-mono, 'Consolas', 'Courier New', monospace);
}

</style>

<style>
.nav-sidebar__quick-access-tooltip {
  padding: 0;
  border: 1px solid hsl(var(--border) / 50%);
  margin-top: 0;
}

.nav-sidebar__quick-access-title {
  padding: 8px 10px;
  border-bottom: 1px solid hsl(var(--border) / 50%);
}

.nav-sidebar-drive-tooltip {
  padding: 0;
  border: none;
  background: transparent;
}

.nav-sidebar__tooltip-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.nav-sidebar-drive-tooltip .drive-card {
  min-width: 260px;
}
</style>
