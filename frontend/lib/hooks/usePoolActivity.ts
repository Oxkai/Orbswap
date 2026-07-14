"use client";

import { useCallback, useEffect, useState } from "react";
import { getRecentEvents } from "@/lib/stellar/pool";
import { fromNative } from "@/lib/stellar/config";

/**
 * Real 24h volume + fees for a pool, summed from on-chain Swap events. There is no
 * indexer, so this reads the recent event window directly (bounded by RPC retention).
 * `feePpm` is the pool's fee in parts-per-million (e.g. 3000 = 30 bps = 0.3%).
 */
export function usePoolActivity(poolId: string, feePpm: number) {
  const [volume24h, setVolume24h] = useState(0);
  const [fees24h, setFees24h] = useState(0);

  const load = useCallback(async () => {
    try {
      const events = await getRecentEvents([poolId], 18000);
      const cutoff = Math.floor(Date.now() / 1000) - 86400;
      let vol = 0;
      for (const e of events) {
        if (e.type !== "Swap" || !e.data?.amount_in) continue;
        if (e.ts && e.ts < cutoff) continue;
        vol += fromNative(BigInt(e.data.amount_in));
      }
      setVolume24h(vol);
      setFees24h(vol * (feePpm / 1_000_000));
    } catch {
      /* leave at 0 */
    }
  }, [poolId, feePpm]);

  useEffect(() => {
    load();
  }, [load]);

  return { volume24h, fees24h };
}
