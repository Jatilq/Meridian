// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

// SSH/SFTP connection registry + ssh:// path helpers (Phase 7).
// Remote panes use ssh://<host>/<path> URLs; the navigation layer routes those
// to the Rust sftp_read_dir command, which returns the same DirContents shape.

import { invoke } from '@tauri-apps/api/core';

export interface SshConnection {
  /** Stable id used in ssh://<id>/... URLs (the hostname/IP). */
  host: string;
  /** Display label shown in bookmarks/breadcrumbs. */
  label: string;
  port: number;
  username: string;
  /** Absolute path to an UNENCRYPTED private key file (russh requirement). */
  keyPath: string;
}

// Pre-configured cluster connections (Phase 7 step 3) — used as the DEFAULT
// seed. The live list is driven by user settings via setSshConnections();
// lookups below read the runtime registry, falling back to these defaults.
// Seed is intentionally empty so Meridian ships without any user-specific
// entries; users add their own SSH connections via Settings → SSH or the
// Add Worker dialog in Cluster Control.
export const SSH_CONNECTIONS: SshConnection[] = [];

// Runtime registry — populated from user settings at app start / on change.
// Defaults to the seed list so lookups work even before settings load.
let activeConnections: SshConnection[] = [...SSH_CONNECTIONS];

/** Replace the live SSH connection list (called from user settings). */
export function setSshConnections(connections: SshConnection[]): void {
  activeConnections = Array.isArray(connections) && connections.length > 0
    ? connections.filter(c => c && c.host)
    : [...SSH_CONNECTIONS];
}

/** Read the live SSH connection list (settings-driven, seed fallback). */
export function getSshConnections(): SshConnection[] {
  return activeConnections;
}

const SSH_PREFIX = 'ssh://';

export function isSshPath(path: string): boolean {
  return typeof path === 'string' && path.startsWith(SSH_PREFIX);
}

/** Build an ssh:// URL from a host and remote path. */
export function buildSshPath(host: string, remotePath: string): string {
  const clean = remotePath.replace(/^\/+/, '');
  return `${SSH_PREFIX}${host}/${clean}`;
}

export interface ParsedSshPath {
  host: string;
  /** Remote absolute path ('' => remote home / canonicalized "."). */
  remotePath: string;
  connection: SshConnection | null;
}

/** Parse ssh://<host>/<remotePath> into its parts + matching connection. */
export function parseSshPath(path: string): ParsedSshPath | null {
  if (!isSshPath(path)) {
    return null;
  }
  const rest = path.slice(SSH_PREFIX.length);
  const slash = rest.indexOf('/');
  const host = slash === -1 ? rest : rest.slice(0, slash);
  const remotePath = slash === -1 ? '' : rest.slice(slash + 1);
  const connection = activeConnections.find(c => c.host === host) ?? null;
  return { host, remotePath, connection };
}

export function findSshConnection(host: string): SshConnection | null {
  return activeConnections.find(c => c.host === host) ?? null;
}

// ---------------------------------------------------------------------------
// Secure SSH password storage (Fix 5)
//
// SSH passwords MUST NEVER be stored as plaintext in the main user-settings
// blob. They live in the secure-keys.json Tauri store by reference (a short
// uuid key). plaintext briefly exists in form state during typing and is
// dropped on save (or clear).
//
// API:
//   generateSecureSshKey() -> string   (e.g. "ssh:<uuid>")
//   storeSshPassword(plain, existingKey?) -> Promise<string>
//     Returns the secure key actually used (existing key if provided, else
//     a fresh uuid).
//   clearSshPassword(key) -> Promise<void>
//     Removes the secret from the secure-keys store. No-op if absent.
// ---------------------------------------------------------------------------

export function generateSecureSshKey(): string {
  const uuid = typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : Math.random().toString(36).slice(2) + Date.now().toString(36);
  return `ssh:${uuid}`;
}

export async function storeSshPassword(plain: string, existingKey?: string): Promise<string> {
  if (!plain) {
    throw new Error('Cannot store an empty SSH password');
  }
  const key = existingKey ?? generateSecureSshKey();
  await invoke('secure_store_secret', { key, value: plain });
  return key;
}

export async function clearSshPassword(key: string | undefined | null): Promise<void> {
  if (!key) return;
  try {
    await invoke('secure_delete_secret', { key });
  }
  catch (error) {
    // Don't throw on cleanup — best-effort. Log so we notice orphan entries.
    console.warn(`Failed to clear SSH secret "${key}":`, error);
  }
}
