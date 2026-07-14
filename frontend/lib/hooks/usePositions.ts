"use client";

import { useCallback, useEffect, useState } from "react";
import { fromNative } from "@/lib/stellar/config";
import { getReserves, totalShares, sharesOf } from "@/lib/stellar/pool";

export interface LpPosition {
  shares: bigint;
  sharePct: number; // % of the pool owned
  tokenAmounts: number[]; // pro-rata underlying, display units
  value: number; // sum of tokenAmounts
}

/** The connected account's LP position in the single Orbswap pool. */
export function usePositions(account: string | undefined) {
  const [position, setPosition] = useState<LpPosition | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  const load = useCallback(async () => {
    if (!account) {
      setPosition(null);
      return;
    }
    setIsLoading(true);
    try {
      const [shares, total, reserves] = await Promise.all([
        sharesOf(account),
        totalShares(),
        getReserves(),
      ]);
      if (shares <= 0n || total <= 0n) {
        setPosition(null);
        return;
      }
      const frac = Number(shares) / Number(total);
      const tokenAmounts = reserves.map((r) => fromNative(r) * frac);
      setPosition({
        shares,
        sharePct: frac * 100,
        tokenAmounts,
        value: tokenAmounts.reduce((a, b) => a + b, 0),
      });
    } catch {
      setPosition(null);
    } finally {
      setIsLoading(false);
    }
  }, [account]);

  useEffect(() => {
    load();
  }, [load]);

  return { position, isLoading, refetch: load };
}
