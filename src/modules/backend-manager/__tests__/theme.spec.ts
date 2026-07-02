// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

/**
 * Exhaustive coverage test for src/utils/exo-theme.ts.
 *
 * Why this file exists:
 *   The .exo-card modifier classes share lockstep semantics with three
 *   callers:
 *     1. cluster.vue      — .cluster-node-{mamba,black,default} tokens
 *     2. backend-manager.vue — .bm-runtime--{llama,lemonade,...} tokens
 *     3. settings/* cards — .exo-card--{mamba,black,violet,...} tokens
 *
 *   Drift in either helper here visually desynchronizes those three
 *   call sites. The test exhaustively enumerates the documented input
 *   spaces so a future variant merging into MeridianBackendKind forces
 *   a `themeKeyFor()` update at compile time AND surfaces a clear
 *   diff at test time.
 */
import { describe, expect, expectTypeOf, it } from 'vitest';
import type { MeridianBackendKind } from '@/types/user-settings';
import type { ExoBackendThemeKey, ExoThemeKey } from '@/utils/exo-theme';
import { hostThemeKey, themeKeyFor } from '@/utils/exo-theme';

describe('exo-theme.ts helpers', () => {
  describe('hostThemeKey(host)', () => {
    it('returns "mamba" for the canonical MAMBA machine and all documented alias substrings', () => {
      const mambaHosts = [
        'mamba',
        'MAMBA',
        'Mamba',
        'mamba-gpu',
        'mamba-rig-01',
        'titan',
        'TITAN-rig',
        'triton',
        'triton-01',
        'local',
        'localhost',
        '9router',
        '9ROUTER', // upper-case to lock in case insensitivity
        '192.168.1.67', // documented hardware-ref IP in AGENTS.md
      ];
      for (const host of mambaHosts) {
        expect(hostThemeKey(host), `host="${host}" should map to "mamba"`).toBe('mamba');
      }
    });

    it('returns "black" for the canonical BLACK machine and all documented alias substrings', () => {
      const blackHosts = [
        'black',
        'BLACK',
        'Black',
        'black.local', // FQDN with .local TLD must NOT false-trigger on mamba's `local` keyword
        'obsidian',
        'OBSIDIAN-node',
        'worker',
        'worker-1',
        '192.168.1.64', // documented hardware-ref IP in AGENTS.md
      ];
      for (const host of blackHosts) {
        expect(hostThemeKey(host), `host="${host}" should map to "black"`).toBe('black');
      }
    });

    it('returns "violet" for any other non-empty hostname', () => {
      const otherHosts = [
        'gpu-cluster-3',
        '192.168.1.99',
        'foo',
        'BAR',
        'random-host',
      ];
      for (const host of otherHosts) {
        expect(hostThemeKey(host), `host="${host}" should map to "violet"`).toBe('violet');
      }
    });

    it('handles null / undefined / empty-string hosts without throwing', () => {
      expect(hostThemeKey(undefined)).toBe('violet');
      expect(hostThemeKey(null)).toBe('violet');
      expect(hostThemeKey('')).toBe('violet');
    });

    it('return type is the ExoThemeKey union (compile-time + runtime contract)', () => {
      // Type-level enforcement: the helper signature must widen to the
      // ExoThemeKey union. If a future maintainer loosens the return
      // type to `string`, this assertion fails compile rather than
      // surfacing only at run time.
      expectTypeOf(hostThemeKey('whatever')).toEqualTypeOf<ExoThemeKey>();
      expectTypeOf(hostThemeKey(undefined)).toEqualTypeOf<ExoThemeKey>();
      expectTypeOf(hostThemeKey(null)).toEqualTypeOf<ExoThemeKey>();
    });
  });

  describe('themeKeyFor(kind)', () => {
    // The exhaustive mapping table — typed against the canonical
    // ExoBackendThemeKey (NOT an inline literal union) so drift in
    // src/utils/exo-theme.ts surfaces here as a type error rather
    // than a stringly-typed assertion.
    const EXPECTED: Record<MeridianBackendKind, ExoBackendThemeKey> = {
      'llama.cpp': 'llama',
      lemonade: 'lemonade',
      koboldcpp: 'kobold',
      llamafile: 'llamafile',
      turboquant: 'turboquant',
    };

    it('every MeridianBackendKind maps to its documented CSS modifier', () => {
      for (const [kind, expected] of Object.entries(EXPECTED) as [MeridianBackendKind, ExoBackendThemeKey][]) {
        expect(themeKeyFor(kind), `kind="${kind}" should map to "${expected}"`).toBe(expected);
      }
    });

    it('covers the full MeridianBackendKind union (no future variant goes uncaught)', () => {
      // Iterating Object.keys over EXPECTED matches every literal in
      // the union *and* nothing else: a future variant lands in the
      // union will compile-fail this Record literal AND the sort
      // ordering assertion will flag drift either way.
      const declaredKinds = Object.keys(EXPECTED).sort() as MeridianBackendKind[];
      expect(declaredKinds).toEqual([
        'koboldcpp',
        'lemonade',
        'llama.cpp',
        'llamafile',
        'turboquant',
      ]);
    });

    it('return type is the ExoBackendThemeKey union (compile-time contract)', () => {
      expectTypeOf(themeKeyFor('llama.cpp')).toEqualTypeOf<ExoBackendThemeKey>();
    });
  });
});
