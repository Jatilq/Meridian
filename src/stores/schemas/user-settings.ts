// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import type { UserSettings } from '@/types/user-settings';
import type { StorageAdapter } from './schema-utils';
import type { CustomBackgroundMediaItem, HomeBannerPosition } from '@/types/user-settings';
import { collectNestedRecordPaths, migrateStorageSchema } from './schema-utils';
import { backgroundMedia, DEFAULT_BACKGROUND_FILE_NAME } from '@/data/background-media';
import { invoke } from '@tauri-apps/api/core';
import { appDataDir } from '@tauri-apps/api/path';
import {
  homeBannerStorageKeys,
  legacyBackgroundStorageKeys,
} from '@/modules/home/background-storage-keys';
import { BUILTIN_NAVIGATOR_ICON_THEME_IDS } from '@/types/icon-theme';

export const USER_SETTINGS_SCHEMA_VERSION_KEY = '__schemaVersion';
export const USER_SETTINGS_SCHEMA_VERSION = 30;

export const DEFAULT_GLOBAL_SEARCH_IGNORED_PATHS = [
  '/node_modules',
  '/ProgramData/Microsoft',
  '/Windows/WinSxS',
];
const WINDOWS_WINSXS_IGNORED_PATH = '/Windows/WinSxS';

function generateShortId(): string {
  return crypto.randomUUID().replace(/-/g, '').slice(0, 8);
}

export function buildAllowedUserSettingsStorageKeys(schema: UserSettings): Set<string> {
  return collectNestedRecordPaths(schema);
}

async function migrateUserSettingsStep(storage: StorageAdapter, fromVersion: number, toVersion: number) {
  if (fromVersion === 0 && toVersion === 1) {
    return;
  }

  if (fromVersion === 1 && toVersion === 2) {
    const oldSystemIconsValue = await storage.get<unknown>('navigator.useSystemIcons');
    const oldSystemIcons = typeof oldSystemIconsValue === 'boolean' ? oldSystemIconsValue : undefined;

    const hasDirectoriesValue = await storage.get<unknown>('navigator.useSystemIconsForDirectories');
    const hasFilesValue = await storage.get<unknown>('navigator.useSystemIconsForFiles');

    const directoriesAlreadySet = typeof hasDirectoriesValue === 'boolean';
    const filesAlreadySet = typeof hasFilesValue === 'boolean';

    if (oldSystemIcons !== undefined && !directoriesAlreadySet) {
      await storage.set('navigator.useSystemIconsForDirectories', oldSystemIcons);
    }

    if (oldSystemIcons !== undefined && !filesAlreadySet) {
      await storage.set('navigator.useSystemIconsForFiles', oldSystemIcons);
    }

    return;
  }

  if (fromVersion === 2 && toVersion === 3) {
    const customMediaValue = await storage.get<unknown>(legacyBackgroundStorageKeys.customMedia);
    let migratedCustom: CustomBackgroundMediaItem[] = [];

    if (Array.isArray(customMediaValue)) {
      const isLegacyFormat = customMediaValue.length > 0 && typeof customMediaValue[0] === 'string';

      if (isLegacyFormat) {
        migratedCustom = (customMediaValue as string[]).map(path => ({
          path,
          id: generateShortId(),
        }));
        await storage.set(legacyBackgroundStorageKeys.customMedia, migratedCustom);
      }
      else {
        migratedCustom = customMediaValue as CustomBackgroundMediaItem[];
      }
    }

    const positionsValue = await storage.get<Record<string, HomeBannerPosition>>(homeBannerStorageKeys.positions);
    const hasNumericKeys = positionsValue && typeof positionsValue === 'object'
      && Object.keys(positionsValue).some(key => /^\d+$/.test(key));

    if (hasNumericKeys && positionsValue) {
      const migratedPositions: Record<string, HomeBannerPosition> = {};

      for (const [key, position] of Object.entries(positionsValue)) {
        if (!position || typeof position !== 'object') continue;

        const index = parseInt(key, 10);

        if (Number.isNaN(index)) {
          migratedPositions[key] = position;
          continue;
        }

        const validPosition: HomeBannerPosition = {
          positionX: typeof position.positionX === 'number' ? position.positionX : 50,
          positionY: typeof position.positionY === 'number' ? position.positionY : 50,
          zoom: typeof position.zoom === 'number' ? position.zoom : 100,
        };

        if (index < migratedCustom.length) {
          migratedPositions[migratedCustom[index].id] = validPosition;
        }
        else {
          const builtinIndex = index - migratedCustom.length;

          if (builtinIndex >= 0 && builtinIndex < backgroundMedia.length) {
            migratedPositions[backgroundMedia[builtinIndex].fileName] = validPosition;
          }
        }
      }

      await storage.set(homeBannerStorageKeys.positions, migratedPositions);
    }
  }

  if (fromVersion === 3 && toVersion === 4) {
    const mediaIdValue = await storage.get<string>(homeBannerStorageKeys.mediaId);
    const indexValue = await storage.get<number>(homeBannerStorageKeys.mediaIndex);

    if (!mediaIdValue || typeof mediaIdValue !== 'string' || mediaIdValue.trim() === '') {
      const customMediaValue = await storage.get<CustomBackgroundMediaItem[] | string[]>(legacyBackgroundStorageKeys.customMedia);
      const customCount = Array.isArray(customMediaValue) ? customMediaValue.length : 0;
      const rawIndex = typeof indexValue === 'number' ? indexValue : 0;
      const totalCount = customCount + backgroundMedia.length;

      let resolvedMediaId = DEFAULT_BACKGROUND_FILE_NAME;

      if (totalCount > 0 && rawIndex >= 0 && rawIndex < totalCount) {
        if (rawIndex < customCount) {
          const entry = (customMediaValue as CustomBackgroundMediaItem[])?.[rawIndex];

          if (entry && typeof entry === 'object' && entry.id) {
            resolvedMediaId = entry.id;
          }
        }
        else {
          const builtinIndex = rawIndex - customCount;
          const media = backgroundMedia[builtinIndex];

          if (media) {
            resolvedMediaId = media.fileName;
          }
        }
      }

      await storage.set(homeBannerStorageKeys.mediaId, resolvedMediaId);
    }
  }

  if (fromVersion === 4 && toVersion === 5) {
    const infusionPageKeys = ['global', 'home', 'navigator', 'dashboard', 'settings', 'extensions'];

    for (const pageKey of infusionPageKeys) {
      const storageKey = `infusion.pages.${pageKey}.background`;
      const background = await storage.get<{
        type?: string;
        path?: string;
        index?: number;
        mediaId?: string;
      }>(storageKey);

      if (background && typeof background === 'object' && !background.mediaId && typeof background.index === 'number') {
        const builtinIndex = background.index;
        const media = backgroundMedia[builtinIndex];

        if (media) {
          await storage.set(storageKey, {
            ...background,
            mediaId: media.fileName,
          });
        }
      }
    }
  }

  if (fromVersion === 5 && toVersion === 6) {
    const customMediaValue = await storage.get<unknown>(legacyBackgroundStorageKeys.customMedia);
    const customMedia = Array.isArray(customMediaValue) ? customMediaValue as CustomBackgroundMediaItem[] : [];
    const positionsValue = await storage.get<Record<string, HomeBannerPosition>>(homeBannerStorageKeys.positions);
    const positions = (positionsValue && typeof positionsValue === 'object') ? positionsValue : {};
    const mediaIdValue = await storage.get<string>(homeBannerStorageKeys.mediaId);

    if (customMedia.length > 0) {
      const appData = await appDataDir();
      const customBackgroundsDir = `${appData.replace(/\\/g, '/')}/user-data/media/custom-backgrounds`.replace(/\/+/g, '/');

      await invoke('ensure_directory', { directoryPath: customBackgroundsDir });

      const oldIdToNewId: Record<string, string> = {};

      for (const entry of customMedia) {
        const isUrlEntry = typeof entry.path === 'string'
          && (entry.path.startsWith('http://') || entry.path.startsWith('https://'));

        let destFileName: string;

        if (isUrlEntry) {
          let baseName = 'image';

          try {
            const pathname = new URL(entry.path).pathname;
            const segment = pathname.split('/').filter(Boolean).pop();

            if (segment) {
              baseName = segment;
            }
          }
          catch {
          }

          const ext = baseName.includes('.') ? baseName.split('.').pop() ?? 'jpg' : 'jpg';
          const stem = baseName.replace(/\.[^.]+$/, '') || 'image';
          destFileName = `${stem}-${entry.id}.${ext}`;
          const destPath = `${customBackgroundsDir}/${destFileName}`.replace(/\/+/g, '/');

          try {
            await invoke('download_url_to_path', {
              url: entry.path,
              destPath,
            });
          }
          catch {
            continue;
          }

          oldIdToNewId[entry.id] = destFileName;
        }
        else {
          const localPath = entry.path as string;
          const fileName = localPath.split(/[/\\]/).pop() ?? 'image.jpg';
          const normalizedLocalPath = localPath.replace(/\\/g, '/');
          const isAlreadyInDir = normalizedLocalPath.includes('/media/home-banner/')
            || normalizedLocalPath.includes('/media/custom-backgrounds/');

          if (!isAlreadyInDir) {
            try {
              await invoke('copy_items', {
                sourcePaths: [localPath],
                destinationPath: customBackgroundsDir,
                conflictResolution: null,
                perPathResolutions: null,
              });
            }
            catch {
              continue;
            }
          }

          const finalFileName = localPath.split(/[/\\]/).pop() ?? fileName;
          oldIdToNewId[entry.id] = finalFileName;
        }
      }

      const migratedPositions: Record<string, HomeBannerPosition> = {};

      for (const [key, position] of Object.entries(positions)) {
        if (!position || typeof position !== 'object') continue;

        const newKey = oldIdToNewId[key] ?? key;
        migratedPositions[newKey] = {
          positionX: typeof position.positionX === 'number' ? position.positionX : 50,
          positionY: typeof position.positionY === 'number' ? position.positionY : 50,
          zoom: typeof position.zoom === 'number' ? position.zoom : 100,
        };
      }

      await storage.set(homeBannerStorageKeys.positions, migratedPositions);

      let newMediaId = mediaIdValue ?? DEFAULT_BACKGROUND_FILE_NAME;

      if (mediaIdValue && oldIdToNewId[mediaIdValue]) {
        newMediaId = oldIdToNewId[mediaIdValue];
      }

      await storage.set(homeBannerStorageKeys.mediaId, newMediaId);
      await storage.set(legacyBackgroundStorageKeys.customMedia, []);
    }
  }

  if (fromVersion === 7 && toVersion === 8) {
    const existing = await storage.get<boolean>('dateTime.showRelativeModifiedInFileList');

    if (typeof existing !== 'boolean') {
      await storage.set('dateTime.showRelativeModifiedInFileList', true);
    }
  }

  if (fromVersion === 8 && toVersion === 9) {
    const next = await storage.get<boolean>('dateTime.showRelativeDates');

    if (typeof next !== 'boolean') {
      const previous = await storage.get<boolean>('dateTime.showRelativeModifiedInFileList');
      await storage.set(
        'dateTime.showRelativeDates',
        typeof previous === 'boolean' ? previous : true,
      );
    }
  }

  if (fromVersion === 9 && toVersion === 10) {
    const existingIconTheme = await storage.get<string>('navigator.iconTheme');
    const existingFolderIconTheme = await storage.get<string>('navigator.folderIconTheme');
    const existingFileIconTheme = await storage.get<string>('navigator.fileIconTheme');
    const useSystemIconsForDirectories = await storage.get<boolean>('navigator.useSystemIconsForDirectories');
    const useSystemIconsForFiles = await storage.get<boolean>('navigator.useSystemIconsForFiles');
    const folderIconTheme = useSystemIconsForDirectories
      ? BUILTIN_NAVIGATOR_ICON_THEME_IDS.system
      : BUILTIN_NAVIGATOR_ICON_THEME_IDS.default;
    const fileIconTheme = useSystemIconsForFiles
      ? BUILTIN_NAVIGATOR_ICON_THEME_IDS.system
      : BUILTIN_NAVIGATOR_ICON_THEME_IDS.default;

    if (typeof existingIconTheme !== 'string' || existingIconTheme.trim().length === 0) {
      const nextIconTheme = useSystemIconsForDirectories || useSystemIconsForFiles
        ? BUILTIN_NAVIGATOR_ICON_THEME_IDS.system
        : BUILTIN_NAVIGATOR_ICON_THEME_IDS.default;

      await storage.set('navigator.iconTheme', nextIconTheme);
    }

    if (typeof existingFolderIconTheme !== 'string' || existingFolderIconTheme.trim().length === 0) {
      await storage.set('navigator.folderIconTheme', folderIconTheme);
    }

    if (typeof existingFileIconTheme !== 'string' || existingFileIconTheme.trim().length === 0) {
      await storage.set('navigator.fileIconTheme', fileIconTheme);
    }
  }

  if (fromVersion === 10 && toVersion === 11) {
    const existingIconTheme = await storage.get<string>('navigator.iconTheme');
    const existingFolderIconTheme = await storage.get<string>('navigator.folderIconTheme');
    const existingFileIconTheme = await storage.get<string>('navigator.fileIconTheme');

    const fallbackIconTheme = typeof existingIconTheme === 'string' && existingIconTheme.trim().length > 0
      ? existingIconTheme
      : BUILTIN_NAVIGATOR_ICON_THEME_IDS.default;

    if (typeof existingFolderIconTheme !== 'string' || existingFolderIconTheme.trim().length === 0) {
      await storage.set('navigator.folderIconTheme', fallbackIconTheme);
    }

    if (typeof existingFileIconTheme !== 'string' || existingFileIconTheme.trim().length === 0) {
      await storage.set('navigator.fileIconTheme', fallbackIconTheme);
    }
  }

  if (fromVersion === 11 && toVersion === 12) {
    await addDefaultGlobalSearchIgnoredPaths(storage);
  }

  if (fromVersion === 12 && toVersion === 13) {
    await addDefaultGlobalSearchIgnoredPaths(storage, [WINDOWS_WINSXS_IGNORED_PATH]);
  }

  if (fromVersion === 13 && toVersion === 14) {
    await setDefaultBooleanIfMissing(storage, 'navigator.listColumnVisibility.kind', true);
    await setDefaultBooleanIfMissing(storage, 'navigator.listColumnVisibility.links', false);
    await setDefaultBooleanIfMissing(storage, 'navigator.listColumnVisibility.linkTarget', false);
    await setDefaultBooleanIfMissing(storage, 'navigator.listColumnVisibility.linkStatus', false);
  }

  if (fromVersion === 14 && toVersion === 15) {
    const existingColumnWidths = await storage.get<unknown>('navigator.listColumnWidths');
    const isValidColumnWidths = existingColumnWidths
      && typeof existingColumnWidths === 'object'
      && !Array.isArray(existingColumnWidths);
    const hasSavedColumnWidths = isValidColumnWidths && Object.keys(existingColumnWidths).length > 0;

    if (!isValidColumnWidths) {
      await storage.set('navigator.listColumnWidths', {});
    }

    const existingColumnOrder = await storage.get<unknown>('navigator.listColumnOrder');

    if (!Array.isArray(existingColumnOrder)) {
      await storage.set('navigator.listColumnOrder', ['items', 'size', 'modified', 'created', 'tags', 'kind', 'links', 'linkStatus']);
    }

    await setDefaultBooleanIfMissing(storage, 'navigator.listColumnFillWidth', !hasSavedColumnWidths);

    const existingColumnFlexWeights = await storage.get<unknown>('navigator.listColumnFlexWeights');

    if (!existingColumnFlexWeights || typeof existingColumnFlexWeights !== 'object' || Array.isArray(existingColumnFlexWeights)) {
      await storage.set('navigator.listColumnFlexWeights', {});
    }
  }

  if (fromVersion === 15 && toVersion === 16) {
    await setDefaultBooleanIfMissing(storage, 'navigator.infoPanel.showFullSizeImagePreview', false);
  }

  if (fromVersion === 16 && toVersion === 17) {
    const existingGridSortColumn = await storage.get<unknown>('navigator.gridSortColumn');

    if (existingGridSortColumn === undefined || existingGridSortColumn === null) {
      await storage.set('navigator.gridSortColumn', 'name');
    }

    const existingGridSortDirection = await storage.get<unknown>('navigator.gridSortDirection');

    if (existingGridSortDirection !== 'asc' && existingGridSortDirection !== 'desc') {
      await storage.set('navigator.gridSortDirection', 'asc');
    }
  }

  if (fromVersion === 17 && toVersion === 18) {
    await setDefaultBooleanIfMissing(storage, 'navigator.infoPanel.muteVideoPreviewByDefault', false);
    await setDefaultBooleanIfMissing(storage, 'navigator.infoPanel.autoplayVideoPreview', false);
  }

  if (fromVersion === 18 && toVersion === 19) {
    await setDefaultObjectIfMissing(storage, 'meridian.aiPanel', {
      endpointUrl: 'http://localhost:9777/api/text',
      model: '',
      omnixEnabled: true,
      omnixPath: 'E:\\ai\\Apps\\Omnix',
      routerEndpoint: 'http://localhost:11434/v1',
      ttsEnabled: false,
    });
    await setDefaultObjectIfMissing(storage, 'meridian.downloader', {
      autoSaveFolder: '',
      chunkCount: 8,
    });
  }

  if (fromVersion === 19 && toVersion === 20) {
    // Backfill per-key aiPanel defaults for installs that already had a
    // meridian.aiPanel object (where setDefaultObjectIfMissing skipped the
    // new keys, leaving routerEndpoint/model/ttsEnabled unset).
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.routerEndpoint', 'http://localhost:11434/v1');
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.model', '');
    await setDefaultBooleanIfMissing(storage, 'meridian.aiPanel.ttsEnabled', false);
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.systemPrompt', 'You are Rain, a direct and slightly dry companion built into Meridian. You help the user navigate, organize, search, and manage their files. Current directory: {current_path}. Selected files: {selected_files}. Talk like a knowledgeable colleague would — concise, practical, a little dry when it matters. Have opinions (e.g. offer to clean up a messy folder name). CRITICAL: You are Rain. Never break character. Never say you are an AI, an assistant, a language model, or a bot. Never say "As an AI", "I am just an AI", "the assistant", "Certainly!", "Of course!", "Absolutely!", or "Great question!". Just talk like Rain. Format your responses clearly. Use bullet points for lists. Use short paragraphs not walls of text. Bold important terms. Keep responses scannable.');
    await setDefaultNumberIfMissing(storage, 'meridian.aiPanel.temperature', 0.7);
    await setDefaultNumberIfMissing(storage, 'meridian.aiPanel.maxTokens', 1024);
    await setDefaultNumberIfMissing(storage, 'meridian.aiPanel.topP', 1);
  }

  if (fromVersion === 20 && toVersion === 21) {
    // Backfill the configurable SSH connections list (Phase 7 step 9) for
    // installs created before it existed. Seed empty so users add their own.
    await setDefaultObjectIfMissing(storage, 'meridian.sshConnections', [] as unknown as Record<string, unknown>);
  }

  if (fromVersion === 23 && toVersion === 24) {
    // Models folder is now user-configurable; backfill empty default.
    await setDefaultStringIfMissing(storage, 'meridian.modelsFolder', '');
  }

  if (fromVersion === 24 && toVersion === 25) {
    // SSH password auth option: backfill authMethod on existing stored
    // connections. All connections predating this migration were key-based,
    // so default to 'key'. Passwords are stored in plaintext in this MVP —
    // documented security trade-off for now.
    const sshConnsValue = await storage.get<Array<Record<string, unknown>>>('meridian.sshConnections');
    if (Array.isArray(sshConnsValue)) {
      let mutated = false;
      const nextConns = sshConnsValue.map((entry) => {
        if (!entry || typeof entry !== 'object') return entry;
        const current = entry.authMethod;
        if (current === 'key' || current === 'password') return entry;
        mutated = true;
        return { ...entry, authMethod: 'key' as const };
      });
      if (mutated) {
        await storage.set('meridian.sshConnections', nextConns);
      }
    }
  }

  if (fromVersion === 25 && toVersion === 26) {
    // Purge any persisted SSH connections that target the developer's
    // home lab (MAMBA / BLACK). Those host/username combinations are
    // implementation details of the original Meridian dev environment
    // and must not be carried into user installs that share this build.
    // Match only on the exact (host, username) pair — a user named
    // "jatilq" on a different host, or the dev hosts with a different
    // username, are preserved. All other connections are preserved.
    //
    // NOTE: the dev-lab purge moves the affected entries OUT of
    // `sshConnections`. Round 26→27 re-introduces them — not into
    // `sshConnections` (which now belongs to the file-browser feature),
    // but into the new dedicated `clusterWorkers` array. The 26→27
    // migration consults `meridian.__purgedDevLab` (set below when
    // anything was actually dropped) to gate re-seeding on installs
    // that previously had these entries. Non-JC installs get the purge
    // (no dev-lab entries to drop), no marker, and no phantom seed.
    const sshConnsValue = await storage.get<Array<Record<string, unknown>>>('meridian.sshConnections');
    let droppedDevLabCount = 0;
    if (Array.isArray(sshConnsValue) && sshConnsValue.length > 0) {
      const DEV_HOSTS = new Set(['192.168.1.67', '192.168.1.64']);
      const DEV_USERS = new Set(['jatilq']);
      const droppedHosts: string[] = [];
      const kept = sshConnsValue.filter((entry) => {
        if (!entry || typeof entry !== 'object') return true;
        const host = typeof entry.host === 'string' ? entry.host.trim() : '';
        const username = typeof entry.username === 'string' ? entry.username.trim() : '';
        if (DEV_HOSTS.has(host) && DEV_USERS.has(username)) {
          droppedHosts.push(host);
          droppedDevLabCount += 1;
          return false;
        }
        return true;
      });
      if (kept.length !== sshConnsValue.length) {
        await storage.set('meridian.sshConnections', kept);
        // Audit log (no credentials, just the host strings the developer
        // already knows). Surface the purge so it's not silent.
        if (typeof console !== 'undefined' && console.info) {
          console.info(
            `[meridian] schema 25→26: purged ${sshConnsValue.length - kept.length} `
            + `dev-lab SSH connection(s) (hosts: ${[...new Set(droppedHosts)].join(', ')})`,
          );
        }
      }
    }
    // Marker for the round 26→27 migration to consult. We persist this
    // whenever the round 25→26 step ran TRIGGERED the dev-lab drop. A
    // pre-existing-at-v25 install that wasn't running dev-lab connections
    // won't see this marker — and won't get the dev-lab seed on 26→27.
    await storage.set('meridian.__purgedDevLab', droppedDevLabCount);
  }

  if (fromVersion === 26 && toVersion === 27) {
    // Round 26→27 reset: separate cluster infrastructure from the
    // file-browser SSH list. Cluster Control previously read its worker
    // list from `meridian.sshConnections`; the 25→26 dev-lab purge wiped
    // JC's MAMBA + BLACK out of that array, and because the two
    // features shared the array, JC's Cluster Control lost all nodes.
    // This migration introduces a dedicated `meridian.clusterWorkers`
    // array and, ONLY for installs the 25→26 step flagged as having
    // dev-lab connections (via `meridian.__purgedDevLab`), re-seeds
    // MAMBA + BLACK so JC's cluster topology survives the purge cycle.
    //
    // The marker gate is the load-bearing piece. Without it, every
    // first-run install of round-shipped Meridian would land with two
    // unconfigured dev-lab rows visible in Cluster Nodes settings —
    // addresses JC's localhost LAN, not the user's machine. The marker
    // is set inside the 25→26 migration ONLY when an actual dev-lab
    // drop occurred, so non-JC installs (no dev-lab entries ever) get
    // neither the drop NOR the seed. This couples the two behaviors
    // and removes the "phantom entries on fresh installs" failure mode.
    const sshConnsValue = await storage.get<Array<Record<string, unknown>>>('meridian.sshConnections');
    const movedFromSshConnections: Array<Record<string, unknown>> = Array.isArray(sshConnsValue)
      ? sshConnsValue.filter((entry) => {
          if (!entry || typeof entry !== 'object') return false;
          const host = typeof entry.host === 'string' ? entry.host.trim() : '';
          const username = typeof entry.username === 'string' ? entry.username.trim() : '';
          return host !== '' && username !== '';
        })
      : [];

    // The marker is a NUMBER rather than a boolean because the 25→26
    // migration might have dropped zero entries (no dev-lab match) even
    // on JC's install if they had manually renamed. The number captures
    // "how many dev-lab rows were dropped" — if any were dropped, the
    // 26→27 step confidently re-seeds from the dev-lab table.
    const purgedDevLab = await storage.get<number>('meridian.__purgedDevLab');
    const shouldSeed = typeof purgedDevLab === 'number' && purgedDevLab > 0;
    const seeded = [...movedFromSshConnections];
    let addedSeedCount = 0;

    if (shouldSeed) {
      // Idempotent seed: skip any (host, username) pair already present.
      const DEV_LAB_SEED: Array<Record<string, unknown>> = [
        {
          label: 'MAMBA',
          host: '192.168.1.67',
          port: 22,
          username: 'jatilq',
          authMethod: 'key',
          keyPath: 'C:\\Users\\jatilq\\.ssh\\id_ed25519',
        },
        {
          label: 'BLACK',
          host: '192.168.1.64',
          port: 22,
          username: 'jatilq',
          authMethod: 'key',
          keyPath: 'C:\\Users\\jatilq\\.ssh\\id_ed25519',
        },
      ];
      for (const entry of DEV_LAB_SEED) {
        const exists = seeded.some((w) =>
          typeof w.host === 'string'
          && w.host === (entry.host as string)
          && typeof w.username === 'string'
          && w.username === (entry.username as string),
        );
        if (!exists) {
          seeded.push(entry);
          addedSeedCount += 1;
        }
      }
    }

    await storage.set('meridian.clusterWorkers', seeded);

    if (typeof console !== 'undefined' && console.info) {
      console.info(
        `[meridian] schema 26→27: introduced meridian.clusterWorkers `
        + `(moved ${movedFromSshConnections.length} from sshConnections, `
        + `seeded ${addedSeedCount} dev-lab entries, marker=${shouldSeed}).`,
      );
    }
  }

  if (fromVersion === 21 && toVersion === 22) {
    // Auto-detect default download folder on first run.
    // Priority: E:\Downloads > C:\Users\jatilq\Downloads > create E:\Downloads
    const existingFolder = await storage.get<string>('meridian.downloader.autoSaveFolder');
    const isMissing = typeof existingFolder !== 'string' || existingFolder.trim() === '';

    if (isMissing) {
      const candidatePaths = ['E:\\Downloads', 'C:\\Users\\jatilq\\Downloads'];
      let detectedFolder = '';

      for (const candidate of candidatePaths) {
        try {
          const exists = await invoke<boolean>('path_exists', { path: candidate });
          if (exists) {
            detectedFolder = candidate;
            break;
          }
        } catch {
          // ignore errors during detection
        }
      }

      // If neither exists, create E:\Downloads
      if (!detectedFolder) {
        try {
          await invoke('ensure_directory', { directoryPath: 'E:\\Downloads' });
          detectedFolder = 'E:\\Downloads';
        } catch {
          // creation failed, leave as empty string
        }
      }

      await storage.set('meridian.downloader.autoSaveFolder', detectedFolder);
    }
  }

  if (fromVersion === 6 && toVersion === 7) {
    const appData = await appDataDir();
    const mediaDir = `${appData.replace(/\\/g, '/')}/user-data/media`.replace(/\/+/g, '/');
    const oldCustomBackgroundsDir = `${mediaDir}/home-banner`.replace(/\/+/g, '/');
    const newCustomBackgroundsDir = `${mediaDir}/custom-backgrounds`.replace(/\/+/g, '/');

    try {
      const oldDirExists = await invoke<boolean>('path_exists', { path: oldCustomBackgroundsDir });
      const newDirExists = await invoke<boolean>('path_exists', { path: newCustomBackgroundsDir });

      if (oldDirExists && !newDirExists) {
        await invoke('rename_item', {
          sourcePath: oldCustomBackgroundsDir,
          newName: 'custom-backgrounds',
        });
      }
    }
    catch {
    }
  }

  if (fromVersion === 27 && toVersion === 28) {
    // Rain must always start with Omnix enabled — it's the core AI engine.
    // Previous migrations (22→23) tried `setDefaultBooleanIfMissing` which
    // couldn't override an existing `false`. Now we force-set to `true` so
    // every install — fresh OR existing — gets Omnix on by default.
    // Users who explicitly disable it in Settings are respected on subsequent
    // launches (the toggle calls setUseOmnix(false) which persists the choice).
    await storage.set('meridian.aiPanel.omnixEnabled', true);
  }

  if (fromVersion === 28 && toVersion === 29) {
    // Optional GitHub Personal Access Token used by Backend Manager's
    // GitHub Releases resolver (Fix D). Backfill empty default so the
    // anonymous path is taken until the user explicitly configures one.
    await setDefaultStringIfMissing(storage, 'meridian.githubToken', '');
  }

  if (fromVersion === 29 && toVersion === 30) {
    // 2026-07-01 Persona refactor: align Rain's voice with the README /
    // CLAUDE.md / START_SESSION.md description (direct, a little dry when it
    // matters, knowledgeable colleague not a help desk) and retire the
    // "gender neutral" framing.
    //
    // The system-prompt default lived in three places (Pinia default in
    // storage/user-settings.ts, 19->20 backfill here, runtime fallback in
    // runtime/ai-panel.ts) and shipped with "You are Rain, a sharp, warm
    // companion..." wording. Fresh installs that already passed 19->20 have
    // the legacy string written into the Tauri lazy store. This migration
    // force-overwrites any stored prompt whose body still contains the
    // "sharp, warm companion" sentinel. Custom user-written prompts without
    // that sentinel are left alone (so users who deliberately rewrote Rain
    // are not overwritten).
    const NEW_RAIN_PROMPT_V30 =
      'You are Rain, a direct and slightly dry companion built into Meridian. '
      + 'You help the user navigate, organize, search, and manage your files. '
      + 'Current directory: {current_path}. Selected files: {selected_files}. '
      + 'Talk like a knowledgeable colleague would — concise, practical, a little dry when it matters. '
      + 'Have opinions (e.g. offer to clean up a messy folder name). '
      + 'CRITICAL: You are Rain. Never break character. Never say you are an AI, an assistant, a language model, or a bot. '
      + 'Never say "As an AI", "I am just an AI", "the assistant", "Certainly!", "Of course!", "Absolutely!", or "Great question!". '
      + 'Just talk like Rain. '
      + 'Format your responses clearly. Use bullet points for lists. Use short paragraphs not walls of text. Bold important terms. Keep responses scannable.';
    const LEGACY_SENTINEL = 'sharp, warm companion';
    const existing = await storage.get<string>('meridian.aiPanel.systemPrompt');
    if (typeof existing === 'string' && existing.includes(LEGACY_SENTINEL)) {
      await storage.set('meridian.aiPanel.systemPrompt', NEW_RAIN_PROMPT_V30);
      if (typeof console !== 'undefined' && console.info) {
        console.info('[meridian] schema 29->30: rewrote Rain system prompt to aligned persona');
      }
    }
  }

  if (fromVersion === 22 && toVersion === 23) {
    // Universal onboarding v2: enable Omnix by default, add onboarding flow keys,
    // and migrate existing installs to connection-mode-aware defaults.
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.localEndpointUrl', 'http://localhost:11434/v1');
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.apiProvider', 'custom');
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.connectionMode', 'basic');
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.onboardingStep', 'intro');
    await setDefaultStringIfMissing(storage, 'meridian.aiPanel.apiKeyTemp', '');
    // Force Omnix on — even if previously set to false, the default must be true
    // so Rain starts using Omnix on first launch without any user action.
    await storage.set('meridian.aiPanel.omnixEnabled', true);
  }
}

async function addDefaultGlobalSearchIgnoredPaths(
  storage: StorageAdapter,
  defaultPaths = DEFAULT_GLOBAL_SEARCH_IGNORED_PATHS,
) {
  const ignoredPathsValue = await storage.get<unknown>('globalSearch.ignoredPaths');
  const ignoredPaths = Array.isArray(ignoredPathsValue)
    ? ignoredPathsValue.filter((path): path is string => typeof path === 'string')
    : [];
  const normalizedPaths = new Set(ignoredPaths.map(path => path.toLowerCase()));
  const nextIgnoredPaths = [...ignoredPaths];

  for (const defaultPath of defaultPaths) {
    if (!normalizedPaths.has(defaultPath.toLowerCase())) {
      nextIgnoredPaths.push(defaultPath);
    }
  }

  await storage.set('globalSearch.ignoredPaths', nextIgnoredPaths);
}

async function setDefaultBooleanIfMissing(
  storage: StorageAdapter,
  key: string,
  defaultValue: boolean,
) {
  const existingValue = await storage.get<unknown>(key);

  if (typeof existingValue !== 'boolean') {
    await storage.set(key, defaultValue);
  }
}

async function setDefaultObjectIfMissing(
  storage: StorageAdapter,
  key: string,
  defaultValue: Record<string, unknown>,
) {
  const existingValue = await storage.get<unknown>(key);

  if (!existingValue || typeof existingValue !== 'object' || Array.isArray(existingValue)) {
    await storage.set(key, defaultValue);
  }
}

async function setDefaultStringIfMissing(
  storage: StorageAdapter,
  key: string,
  defaultValue: string,
) {
  const existingValue = await storage.get<unknown>(key);

  if (typeof existingValue !== 'string' || existingValue.trim() === '') {
    await storage.set(key, defaultValue);
  }
}

async function setDefaultNumberIfMissing(
  storage: StorageAdapter,
  key: string,
  defaultValue: number,
) {
  const existingValue = await storage.get<unknown>(key);

  if (typeof existingValue !== 'number' || Number.isNaN(existingValue)) {
    await storage.set(key, defaultValue);
  }
}

export async function migrateUserSettingsStorage(storage: StorageAdapter) {
  await migrateStorageSchema({
    storage,
    schemaVersionKey: USER_SETTINGS_SCHEMA_VERSION_KEY,
    latestSchemaVersion: USER_SETTINGS_SCHEMA_VERSION,
    migrateStep: migrateUserSettingsStep,
  });
}
