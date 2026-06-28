// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import HomePage from '@/modules/home/pages/home.vue';
import {
  BlocksIcon, CpuIcon, FolderClosedIcon, HomeIcon, BookmarkIcon, ServerIcon, SettingsIcon, XIcon,
  ZapIcon,
} from '@lucide/vue';
import type { RouteRecordRaw } from 'vue-router';

export const quickViewRoute: RouteRecordRaw = {
  path: '/quick-view',
  name: 'quick-view',
  component: () => import('@/modules/quick-view/pages/quick-view.vue'),
  meta: { layout: 'quick-view' },
};

export const printViewRoute: RouteRecordRaw = {
  path: '/print-view',
  name: 'print-view',
  component: () => import('@/modules/print-view/pages/print-view.vue'),
  meta: { layout: 'quick-view' },
};

export const extensionPageRoute: RouteRecordRaw = {
  path: '/extension/:fullPageId',
  name: 'extension-page',
  component: () => import('@/modules/extensions/pages/extension-page.vue'),
  meta: { keepAlive: true },
};

export const routes: Array<RouteRecordRaw & { icon: typeof XIcon }> = [
  {
    path: '/',
    name: 'home',
    icon: HomeIcon,
    component: HomePage,
  },
  {
    path: '/navigator',
    name: 'navigator',
    icon: FolderClosedIcon,
    component: () => import('@/modules/navigator/pages/navigator.vue'),
  },
  {
    path: '/dashboard',
    name: 'dashboard',
    icon: BookmarkIcon,
    component: () => import('@/modules/dashboard/pages/dashboard.vue'),
  },
  {
    path: '/cluster',
    name: 'cluster',
    icon: ServerIcon,
    component: () => import('@/modules/cluster/pages/cluster.vue'),
  },
  {
    path: '/hardware',
    name: 'hardware',
    icon: ZapIcon,
    component: () => import('@/modules/hardware/pages/hardware.vue'),
  },
  {
    path: '/backend-manager',
    name: 'backend-manager',
    icon: CpuIcon,
    component: () => import('@/modules/backend-manager/pages/backend-manager.vue'),
  },
  {
    path: '/settings',
    name: 'settings',
    icon: SettingsIcon,
    component: () => import('@/modules/settings/pages/settings.vue'),
  },
  {
    path: '/extensions',
    name: 'extensions',
    icon: BlocksIcon,
    component: () => import('@/modules/extensions/pages/extensions.vue'),
  },
];
