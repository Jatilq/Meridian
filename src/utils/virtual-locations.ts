// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import { invoke } from '@tauri-apps/api/core';
import type { DirContents, DirEntry, ReadDirOptions } from '@/types/dir-entry';
import type { DriveInfo } from '@/types/drive-info';
import normalizePath, {
  getParentPath,
  getPathDisplayName,
} from '@/utils/normalize-path';
import { isProtectedSystemPath } from '@/utils/is-protected-system-path';
import {
  isUnderUnixSystemMount,
  isWindowsLocationsScopePath,
} from '@/utils/system-mount-roots';
import {
  isVirtualLocationPath,
  LOCATIONS_VIRTUAL_PATH,
} from '@/utils/virtual-path-constants';
import { createDriveEntryMetadata } from '@/utils/drive-icon';
import { isSshPath, parseSshPath, buildSshPath } from '@/utils/ssh-connections';

export { isVirtualLocationPath, LOCATIONS_VIRTUAL_PATH } from '@/utils/virtual-path-constants';

export function virtualLocationPathExists(path: string): boolean {
  return isVirtualLocationPath(path);
}

export function getVirtualLocationDisplayName(path: string, translate: (key: string) => string): string | null {
  if (!isVirtualLocationPath(path)) {
    return null;
  }

  return translate('locations');
}

export function driveInfoToDirEntry(drive: DriveInfo): DirEntry {
  const normalizedPath = normalizePath(drive.path);
  const displayName = drive.name.trim() || getPathDisplayName(normalizedPath) || normalizedPath;

  return {
    name: displayName,
    path: normalizedPath,
    is_file: false,
    is_dir: true,
    is_hidden: false,
    is_symlink: false,
    size: drive.total_space,
    item_count: null,
    created_time: 0,
    modified_time: 0,
    accessed_time: 0,
    ext: null,
    mime: null,
    drive_metadata: createDriveEntryMetadata(drive),
  };
}

export function createLocationsDirEntry(): DirEntry {
  return {
    name: '',
    path: LOCATIONS_VIRTUAL_PATH,
    is_file: false,
    is_dir: true,
    is_hidden: false,
    is_symlink: false,
    size: 0,
    item_count: null,
    created_time: 0,
    modified_time: 0,
    accessed_time: 0,
    ext: null,
    mime: null,
  };
}

export async function readLocationsDirectory(): Promise<DirContents> {
  const drives = await invoke<DriveInfo[]>('get_system_drives');
  const entries = drives.map(drive => driveInfoToDirEntry(drive));
  const directoryCount = entries.length;

  return {
    path: LOCATIONS_VIRTUAL_PATH,
    entries,
    total_count: directoryCount,
    dir_count: directoryCount,
    file_count: 0,
    opened_directory_times: {
      modified_time: 0,
      accessed_time: 0,
      created_time: 0,
    },
  };
}

export async function resolveDirectoryContents(
  path: string,
  options?: ReadDirOptions,
): Promise<DirContents> {
  if (isVirtualLocationPath(path)) {
    return readLocationsDirectory();
  }

  if (isSshPath(path)) {
    return resolveSftpDirectoryContents(path);
  }

  return invoke<DirContents>('read_dir', {
    path,
    options,
  });
}

interface SftpContents {
  path: string;
  entries: Array<Omit<DirEntry, 'link_type' | 'link_target' | 'link_status' | 'hard_link_count'>>;
  total_count: number;
  dir_count: number;
  file_count: number;
}

// List a remote directory over SFTP and map it into the local DirContents
// shape so the same file pane renders it. The returned `path` stays an
// ssh://host/... URL so navigation/breadcrumbs remain remote-aware.
async function resolveSftpDirectoryContents(path: string): Promise<DirContents> {
  const parsed = parseSshPath(path);
  if (!parsed || !parsed.connection) {
    throw new Error(`Unknown SSH host in path: ${path}`);
  }

  const creds = {
    host: parsed.connection.host,
    port: parsed.connection.port,
    username: parsed.connection.username,
    keyPath: parsed.connection.keyPath,
  };

  const remote = await invoke<SftpContents>('sftp_read_dir', {
    creds,
    path: parsed.remotePath,
  });

  const entries: DirEntry[] = remote.entries.map(entry => ({
    ...entry,
    // Re-wrap the entry path as an ssh:// URL so clicks keep navigating remote.
    path: buildSshPath(parsed.connection!.host, entry.path),
    link_type: null,
    link_target: null,
    link_status: null,
    hard_link_count: null,
  }));

  return {
    path: buildSshPath(parsed.connection.host, remote.path),
    entries,
    total_count: remote.total_count,
    dir_count: remote.dir_count,
    file_count: remote.file_count,
    opened_directory_times: {
      modified_time: 0,
      accessed_time: 0,
      created_time: 0,
    },
  };
}

export async function resolveDirEntry(
  path: string,
  timeoutMs?: number,
): Promise<DirEntry | null> {
  if (isVirtualLocationPath(path)) {
    return createLocationsDirEntry();
  }

  try {
    return await invoke<DirEntry>('get_dir_entry_with_timeout', {
      path,
      timeoutMs,
    });
  }
  catch {
    return null;
  }
}

export async function getLocationsDrivePaths(): Promise<string[]> {
  const contents = await readLocationsDirectory();
  return contents.entries.map(entry => entry.path);
}

export function getNavigableParentPath(path: string, platform: string | null): string | null {
  const normalizedPath = normalizePath(path);

  if (isVirtualLocationPath(normalizedPath)) {
    return null;
  }

  const pathWithoutTrailingSlash = normalizedPath.replace(/\/+$/, '');

  if (isProtectedSystemPath(pathWithoutTrailingSlash, platform)) {
    return LOCATIONS_VIRTUAL_PATH;
  }

  return getParentPath(normalizedPath);
}

export function shouldPrependLocationsCrumb(path: string, platform: string | null): boolean {
  const normalizedPath = normalizePath(path);

  if (isVirtualLocationPath(normalizedPath)) {
    return true;
  }

  if (platform === 'windows') {
    return isWindowsLocationsScopePath(normalizedPath);
  }

  return isUnderUnixSystemMount(normalizedPath);
}
