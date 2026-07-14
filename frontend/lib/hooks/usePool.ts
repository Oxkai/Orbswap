"use client";

import { useCallback, useEffect, useState } from "react";
import { type Pool } from "@/lib/mock/data";
import { TOKENS, STELLAR, fromNative } from "@/lib/stellar/config";
import { getReserves } from "@/lib/stellar/pool";

// The Orbswap pool is a single-range n-dim pool (no ticks). We surface it in the
// existing `Pool` shape so the pool/position UI renders it unchanged.
export function usePool(_poolAddress?: string) {
  const [pool, setPool] = useState<Pool | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isError, setIsError] = useState(false);

  const load = useCallback(async () => {
    setIsLoading(true);
    setIsError(false);
    try {
      const reservesNative = await getReserves();
      const reserves = reservesNative.map(fromNative);
      const tvl = reserves.reduce((a, b) => a + b, 0);
      const tokens = TOKENS.map((t, i) => ({
        address: t.address,
        symbol: t.symbol,
        name: t.symbol,
        color: t.color,
        balance: reserves[i] ?? 0,
      }));
      setPool({
        address: STELLAR.pool,
        name: tokens.map((t) => t.symbol).join(" / "),
        tokens,
        fee: 3000, // 30 bps, in the UI's parts-per-million convention (fee/10000 = %)
        rInt: 0,
        reserves,
        ticks: [], // single-range pool → no ticks
        tvl,
        volume24h: 0, // no indexer wired yet
        fees24h: 0,
        kBound: 0,
        sumX: 0n,
        depeggedTokenIndices: [],
      });
    } catch {
      setIsError(true);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return { pool, isLoading, isError, refetch: load };
}
