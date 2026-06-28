<!-- SPDX-License-Identifier: GPL-3.0-or-later
License: GNU GPLv3 or later. See the license file in the project root for more information.
Copyright © 2021 - present Aleksey Hoffman. All rights reserved.
-->

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  BotIcon,
  CpuIcon,
  FolderSyncIcon,
  DownloadIcon,
  EyeIcon,
  Wand2Icon,
  SparklesIcon,
} from '@lucide/vue';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { useChangelog } from '@/modules/changelog/composables/use-changelog';

const { t } = useI18n();
const {
  isOpen: isWhatsNewOpen,
  close: closeWhatsNew,
  appVersion,
  open,
} = useChangelog();

interface Feature {
  icon: unknown;
  title: string;
  description: string;
}

const features = computed<Feature[]>(() => [
  {
    icon: BotIcon,
    title: t('changelog.feature.rain.title'),
    description: t('changelog.feature.rain.description'),
  },
  {
    icon: CpuIcon,
    title: t('changelog.feature.cluster.title'),
    description: t('changelog.feature.cluster.description'),
  },
  {
    icon: FolderSyncIcon,
    title: t('changelog.feature.sftp.title'),
    description: t('changelog.feature.sftp.description'),
  },
  {
    icon: DownloadIcon,
    title: t('changelog.feature.downloader.title'),
    description: t('changelog.feature.downloader.description'),
  },
  {
    icon: EyeIcon,
    title: t('changelog.feature.vision.title'),
    description: t('changelog.feature.vision.description'),
  },
  {
    icon: Wand2Icon,
    title: t('changelog.feature.agent.title'),
    description: t('changelog.feature.agent.description'),
  },
]);

function viewFullChangelog() {
  closeWhatsNew();
  open();
}
</script>

<template>
  <Dialog v-model:open="isWhatsNewOpen">
    <DialogContent class="whats-new-dialog">
      <DialogHeader class="whats-new-dialog__header">
        <div class="whats-new-dialog__badge">
          <SparklesIcon :size="14" />
          {{ t('changelog.whatsNew.badge', { version: appVersion }) }}
        </div>
        <DialogTitle class="whats-new-dialog__title">
          {{ t('changelog.whatsNew.title') }}
        </DialogTitle>
        <DialogDescription class="whats-new-dialog__description">
          {{ t('changelog.whatsNew.description') }}
        </DialogDescription>
      </DialogHeader>

      <div class="whats-new-dialog__features">
        <div
          v-for="(feature, index) in features"
          :key="index"
          class="whats-new-dialog__feature"
        >
          <div class="whats-new-dialog__feature-icon">
            <component :is="feature.icon" :size="20" />
          </div>
          <div class="whats-new-dialog__feature-content">
            <div class="whats-new-dialog__feature-title">
              {{ feature.title }}
            </div>
            <div class="whats-new-dialog__feature-description">
              {{ feature.description }}
            </div>
          </div>
        </div>
      </div>

      <div class="whats-new-dialog__footer">
        <Button
          variant="secondary"
          size="sm"
          @click="viewFullChangelog"
        >
          {{ t('changelog.whatsNew.viewFullChangelog') }}
        </Button>
        <Button
          variant="default"
          size="sm"
          @click="closeWhatsNew"
        >
          {{ t('changelog.whatsNew.gotIt') }}
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>

<style scoped>
.whats-new-dialog {
  width: min(560px, 92vw);
  max-width: 560px;
  padding: 0;
  overflow: hidden;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius-lg);
  background: #1e1e1e;
  color: hsl(var(--foreground));
}

.whats-new-dialog__header {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 1.75rem 1.5rem 1.25rem;
  text-align: center;
  gap: 0.75rem;
}

.whats-new-dialog__badge {
  display: inline-flex;
  align-items: center;
  padding: 0.375rem 0.75rem;
  border: 1px solid #c9a84c40;
  border-radius: 999px;
  background: #c9a84c15;
  color: #c9a84c;
  font-size: 0.75rem;
  font-weight: 600;
  gap: 0.375rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.whats-new-dialog__title {
  color: hsl(var(--foreground));
  font-size: 1.5rem;
  font-weight: 700;
}

.whats-new-dialog__description {
  max-width: 420px;
  color: hsl(var(--muted-foreground));
  font-size: 0.9375rem;
  line-height: 1.5;
}

.whats-new-dialog__features {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: 0.75rem;
  padding: 0 1.5rem 1.5rem;
}

.whats-new-dialog__feature {
  display: flex;
  padding: 0.875rem;
  border: 1px solid hsl(var(--border));
  border-radius: var(--radius);
  background: hsl(var(--background) / 60%);
  gap: 0.75rem;
  transition: border-color 0.15s ease, background-color 0.15s ease;
}

.whats-new-dialog__feature:hover {
  border-color: #c9a84c60;
  background: #c9a84c08;
}

.whats-new-dialog__feature-icon {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: var(--radius-sm);
  background: #c9a84c20;
  color: #c9a84c;
}

.whats-new-dialog__feature-content {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 0.25rem;
}

.whats-new-dialog__feature-title {
  color: hsl(var(--foreground));
  font-size: 0.9375rem;
  font-weight: 600;
}

.whats-new-dialog__feature-description {
  color: hsl(var(--muted-foreground));
  font-size: 0.8125rem;
  line-height: 1.45;
}

.whats-new-dialog__footer {
  display: flex;
  justify-content: space-between;
  padding: 1rem 1.5rem 1.25rem;
  border-top: 1px solid hsl(var(--border));
  gap: 0.75rem;
}

@media (width <= 560px) {
  .whats-new-dialog__features {
    grid-template-columns: 1fr;
  }

  .whats-new-dialog__footer {
    flex-direction: column-reverse;
  }
}
</style>
