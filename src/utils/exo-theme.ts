// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

import type { MeridianBackendKind } from '@/types/user-settings';

/**
 * exo-theme — shared helpers that compute the per-card accent
 * class for exo-style row cards. Centralized so cluster.vue (the
 * topology panel) and the settings sub-pages (Cluster Nodes,
 * SSH Connections) consume the same mapping. Drift here = visible
 * color mismatch between the topology and the settings cards
 * that point to the same host.
 *
 * Conventions:
 *   • The accent class name is a BEM-style suffix that matches the
 *     modifier classes defined in src/styles/exo.css:
 *       .exo-card--mamba    (teal — local / MAMBA / 9Router)
 *       .exo-card--black    (coral — BLACK / SSH)
 *       .exo-card--violet   (default — anything not mamba/black)
 *       .exo-card--amber    (Hardware Scanner / Models folder)
 *       .exo-card--indigo   (AI Panel)
 *   • The mapping is intentionally tolerant: case-insensitive name
 *     match, falls back to violet if no known key matches. Adding
 *     a new host key requires only an `if` arm here — no CSS
 *     change.
 *   • The mapping function is named `hostThemeKey` (not
 *     `exoTheme`) so the contract is identical to the per-host
 *     classification JS-side: same string → same CSS modifier.
 */

/** Shared union of valid exo theme class suffixes. */
export type ExoThemeKey =
  | 'mamba'
  | 'black'
  | 'violet'
  | 'amber'
  | 'indigo';

/**
 * Map a hostname / display name to an exo theme key.
 *
 * @param host - hostname (`192.168.1.64`), full label
 *   (`MAMBA`), or username@host string. Trimmed and
 *   lowercased before matching.
 *
 * Heuristics are case-insensitive and substring-tolerant — a
 * connection labelled "mamba-gpu" still matches mamba teal.
 * Ip-only hosts (no nickname match) fall through to violet.
 * The list grows with usage: if a new machine becomes a regular,
 * add its host/nickname here in one line.
 */
export function hostThemeKey(host: string | undefined | null): ExoThemeKey {
  if (!host) return 'violet';
  const lower = host.toLowerCase();

  // MAMBA family (local Titan / 9Router / Triton / left node).
  // Match on nickname or the canonical 192.168.1.67 IP.
  if (
    lower.includes('mamba') ||
    lower.includes('titan') ||
    lower.includes('triton') ||
    lower.includes('local') ||
    lower.includes('9router') ||
    lower.includes('192.168.1.67')
  ) {
    return 'mamba';
  }

  // BLACK family (worker / obsidian / right node).
  // Match on nickname or the canonical 192.168.1.64 IP.
  if (
    lower.includes('black') ||
    lower.includes('obsidian') ||
    lower.includes('worker') ||
    lower.includes('192.168.1.64')
  ) {
    return 'black';
  }

  return 'violet';
}

/**
 * Map a MeridianBackendKind to its exo-css modifier suffix. Statically
 * ties backend-manager.vue's
 * `.bm-runtime--llama/lemonade/kobold/llamafile/turboquant`
 * modifier names into a typed lookup that a vitest test can
 * exhaustively verify (see
 * src/modules/backend-manager/__tests__/theme.spec.ts).
 *
 * The function is exhaustive over MeridianBackendKind — TypeScript
 * will refuse to compile if a future variant is added to the
 * CANONICAL union (src/types/user-settings.ts) without a matching
 * `case` here. We import MeridianBackendKind rather than re-declare
 * it so the exhaustiveness check fires against live state, not a
 * stale snapshot.
 */
export function themeKeyFor(kind: MeridianBackendKind): string {
  switch (kind) {
    case 'llama.cpp':   return 'llama';
    case 'lemonade':    return 'lemonade';
    case 'koboldcpp':   return 'kobold';
    case 'llamafile':   return 'llamafile';
    case 'turboquant':  return 'turboquant';
  }
}
