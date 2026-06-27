// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

// SSH/SFTP connection registry + ssh:// path helpers (Phase 7).
// Remote panes use ssh://<host>/<path> URLs; the navigation layer routes those
// to the Rust sftp_read_dir command, which returns the same DirContents shape.

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

// Pre-configured cluster connections (Phase 7 step 3). Both use the
// passphrase-less meridian_black key proven working for key-only auth.
export const SSH_CONNECTIONS: SshConnection[] = [
  {
    host: '192.168.1.67',
    label: 'MAMBA',
    port: 22,
    username: 'jatilq',
    keyPath: 'C:\\Users\\jatilq\\.ssh\\meridian_black',
  },
  {
    host: '192.168.1.64',
    label: 'BLACK',
    port: 22,
    username: 'jatilq',
    keyPath: 'C:\\Users\\jatilq\\.ssh\\meridian_black',
  },
];

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
  const connection = SSH_CONNECTIONS.find(c => c.host === host) ?? null;
  return { host, remotePath, connection };
}

export function findSshConnection(host: string): SshConnection | null {
  return SSH_CONNECTIONS.find(c => c.host === host) ?? null;
}
