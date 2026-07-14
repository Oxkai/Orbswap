"use client";

import { useCallback, useEffect, useState } from "react";
import { type Pool } from "@/lib/mock/data";
import { getTickState, TICK_POOL } from "@/lib/stellar/ticks";

// The live 2-token Circular concentrated-liquidity (tick) pool, surfaced in the
// existing `Pool` shape so it renders as a second card on the Pools page.
export function useTickPool() {
  const [pool, setPool] = useState<Pool | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isError, setIsError] = useState(false);

  const load = useCallback(async () => {
    setIsLoading(true);
    setIsError(false);
    try {
      const st = await getTickState();
      const tokens = TICK_POOL.tokens.map((t, i) => ({
        address: t.address,
        symbol: t.symbol,
        name: t.symbol,
        color: t.color,
        balance: st.reserves[i] ?? 0,
      }));
      const tvl = st.reserves.reduce((a, b) => a + b, 0);
      // Initialized ticks: 0, 40, 50, 90 (full-range + concentrated [40,50]). All
      // active/healthy (arc ends aren't depeg boundaries here).
      const ticks = [0, 40, 50, 90].map((deg) => ({
        kWad: BigInt(deg),
        r: 0,
        isInterior: true,
        feeGrowthInside: 0n,
        liquidityGross: 0n,
        depegPrice: 0,
        capitalEfficiency: 0,
      }));
      setPool({
        address: TICK_POOL.id,
        name: tokens.map((t) => t.symbol).join(" / "),
        tokens,
        fee: 3000, // 30 bps
        rInt: 0,
        reserves: st.reserves,
        ticks,
        tvl,
        volume24h: 0,
        fees24h: 0,
        kBound: st.currentTick, // current tick (45 = balanced $1)
        sumX: BigInt(Math.round(st.activeLiquidity)),
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
