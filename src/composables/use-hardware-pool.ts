// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the project root for the full license text.
// Copyright © 2026 Meridian Agent. All rights reserved.

/**
 * useHardwarePool — single source of truth for "what's the live VRAM/RAM/CPU
 * Meridian can address right now?".
 *
 * Two consumers care about this answer today:
 *   • Cluster Control    (display per-node cards + sum stats)
 *   • Hardware Scanner    (combine local + workers into a single VRAM budget
 *                          used by the "fits my hardware" fit check)
 *
 * Both previously polled `get_local_hardware` and `get_remote_hardware` from
 * the page itself and recomputed `combinedVramMb`. The Hardware Scanner path
 * was missing the workers entirely — combined VRAM read as local-only, even
 * when BLACK was online and contributing 16 GB.
 *
 * Pipeline:
 *   1. On mount: fetch local + each `meridian.clusterWorkers` entry once.
 *   2. Poll every `pollMs` ms (default 30 s — matches Cluster's cadence).
 *   3. Per-source failure becomes an `online: false, error: <msg>` entry;
 *      downstream computations (e.g. `combinedVramGb`) gracefully degrade
 *      to "local VRAM only" when a worker is offline.
 *   4. When the user's clusterWorkers list changes, refresh immediately so
 *      the pool reactivity matches the saved settings.
 *
 * `pollMs` defaults to 30 s. Cluster uses the default; Status page (Fix 3)
 * overrides to 5 s for fresher GPU utilization polling.
 */

import { computed, onMounted, onUnmounted, ref, watch, type ComputedRef, type Ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useUserSettingsStore } from '@/stores/storage/user-settings';

export type HardwareSnapshot = {
  online: boolean;
  cpu: { name: string; cores: number; utilization: number } | null;
  ram: { totalMb: number; usedMb: number; freeMb: number; utilization: number } | null;
  gpus: Array<{
    index: number;
    name: string;
    utilization: number;
    memoryUsed: number; // MiB
    memoryTotal: number; // MiB
    temperature: number;
  }>;
  error: string | null;
};

export type HardwarePoolEntry = {
  host: string;
  name: string;
  role: 'local' | 'worker';
  isLocal: boolean;
  online: boolean;
  cpu: HardwareSnapshot['cpu'];
  ram: HardwareSnapshot['ram'];
  gpus: HardwareSnapshot['gpus'];
  error: string | null;
  /** Ms since epoch; staleness signal for cards that show "last seen N min ago". */
  lastFetchedAt: number;
};

/**
 * Role convention for clusterWorkers entries (Cluster Control document):
 *   • `label === 'MAMBA'` → Meridian runs on this box; routes via
 *     `get_local_hardware` (not SSH).
 *   • All other labels  → remote worker, routes via `get_remote_hardware`
 *     with creds derived from the user's stored settings.
 *
 * The composable does NOT re-derive this. Cluster Control's `nodeDefs`
 * computed owns the `isMamba = label === 'MAMBA'` heuristic because the
 * label is a Cluster Control domain concept (it owns the topology map
 * + RPC launch targeting). Future consumers (Status page — Fix 3) will
 * re-import the same heuristic so the local-vs-remote decision stays in
 * one place per page, not duplicated across the composable.
 */
function coerceSnapshot(raw: unknown): HardwareSnapshot {
  const v = (raw ?? {}) as Partial<HardwareSnapshot>;
  return {
    online: v.online ?? false,
    cpu: v.cpu ?? null,
    ram: v.ram ?? null,
    gpus: Array.isArray(v.gpus) ? v.gpus : [],
    error: v.error ?? null,
  };
}

/** Connection shape expected by `cluster.rs::get_remote_hardware`. Mirrors
 *  the camelCase rename on the Rust side so we don't trip serde. */
type SshCredentialsPayload = {
  host: string;
  port?: number;
  username: string;
  keyPath?: string;
  password?: string;
  passwordSecureKey?: string;
  authMethod: 'key' | 'password';
};

/**
 * Resolve the live hardware pool (local Meridian box + every entry in
 * `meridian.clusterWorkers`). Consumers read `entries` reactively and
 * `combinedVramGb` for the "fits my hardware" budget.
 *
 * Returns a `useHardwarePool` handle. Call from inside `<script setup>`.
 */
export function useHardwarePool(options: { pollMs?: number } = {}): {
  entries: Ref<HardwarePoolEntry[]>;
  combinedVramMb: ComputedRef<number>;
  combinedVramGb: ComputedRef<number>;
  combinedGpuCount: ComputedRef<number>;
  refresh: () => Promise<void>;
} {
  const userSettingsStore = useUserSettingsStore();
  const entries = ref<HardwarePoolEntry[]>([]);
  const pollMs = options.pollMs ?? 30_000;
  let timer: ReturnType<typeof setInterval> | null = null;
  let fetching = false;

  async function fetchOne(
    host: string,
    name: string,
    role: 'local' | 'worker',
    isLocal: boolean,
    creds?: SshCredentialsPayload,
  ): Promise<HardwarePoolEntry> {
    try {
      const raw = isLocal
        ? await invoke<unknown>('get_local_hardware')
        : await invoke<unknown>('get_remote_hardware', { creds });
      const snap = coerceSnapshot(raw);
      return {
        host,
        name,
        role,
        isLocal,
        online: snap.online,
        cpu: snap.cpu,
        ram: snap.ram,
        gpus: snap.gpus,
        error: snap.error,
        lastFetchedAt: Date.now(),
      };
    }
    catch (err) {
      return {
        host,
        name,
        role,
        isLocal,
        online: false,
        cpu: null,
        ram: null,
        gpus: [],
        error: err instanceof Error ? err.message : String(err),
        lastFetchedAt: Date.now(),
      };
    }
  }

  /** Build the pool-spec the hardware pool needs. Local Meridian entry is
   *  always included; remote entries come from the user's clusterWorkers. */
  function buildSpecs(): Array<{ host: string; name: string; role: 'local' | 'worker'; isLocal: boolean; creds?: SshCredentialsPayload }> {
    const workers = userSettingsStore.userSettings?.meridian?.clusterWorkers ?? [];
    const specs: Array<{ host: string; name: string; role: 'local' | 'worker'; isLocal: boolean; creds?: SshCredentialsPayload }> = [
      { host: 'local', name: 'Local', role: 'local', isLocal: true },
    ];
    for (const w of workers) {
      if (!w?.host) continue;
      specs.push({
        host: w.host,
        name: w.label?.trim() || w.host,
        role: 'worker',
        isLocal: false,
        creds: {
          host: w.host,
          port: w.port || 22,
          username: w.username,
          keyPath: w.keyPath || undefined,
          passwordSecureKey: w.passwordSecureKey || undefined,
          authMethod: w.authMethod || 'key',
        },
      });
    }
    return specs;
  }

  async function refresh() {
    if (fetching) return;
    fetching = true;
    try {
      const specs = buildSpecs();
      const results = await Promise.all(
        specs.map(s => fetchOne(s.host, s.name, s.role, s.isLocal, s.creds)),
      );
      entries.value = results;
    }
    finally {
      fetching = false;
    }
  }

  const combinedVramMb = computed(() =>
    entries.value
      .flatMap(e => e.gpus)
      .reduce((sum, g) => sum + (g.memoryTotal || 0), 0),
  );

  const combinedVramGb = computed(() =>
    combinedVramMb.value > 0 ? Math.floor(combinedVramMb.value / 1024) : 0,
  );

  const combinedGpuCount = computed(() =>
    entries.value.reduce((n, e) => n + e.gpus.length, 0),
  );

  onMounted(() => {
    void refresh();
    timer = setInterval(() => void refresh(), pollMs);
  });

  onUnmounted(() => {
    if (timer) clearInterval(timer);
    timer = null;
  });

  // When the clusterWorkers list changes (user added/removed a worker), the
  // specs derivation flips — re-fetch so the pool reflects the new shape
  // instead of waiting up to pollMs for the next cycle.
  watch(
    () => userSettingsStore.userSettings?.meridian?.clusterWorkers,
    () => { void refresh(); },
    { deep: true },
  );

  return {
    entries,
    combinedVramMb,
    combinedVramGb,
    combinedGpuCount,
    refresh,
  };
}
