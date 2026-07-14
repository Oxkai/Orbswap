"use client";

import { color, typography } from "@/constants";
import { PoolCard } from "@/components/app/pools/PoolCard";
import { usePool } from "@/lib/hooks/usePool";
import { useTickPool } from "@/lib/hooks/useTickPool";

export default function PoolsPage() {
  const { pool: superPool, isLoading: l1, isError: e1 } = usePool();
  const { pool: tickPool, isLoading: l2, isError: e2 } = useTickPool();
  const isLoading = l1 || l2;
  const isError = e1 && e2;
  const pools = [superPool, tickPool].filter(Boolean) as NonNullable<
    ReturnType<typeof usePool>["pool"]
  >[];

  return (
    <section className="flex-1 flex flex-col py-8 sm:py-10">
      {/* ── Hero ─────────────────────────────────────────────────── */}
      <header className="flex flex-col gap-1.5 mb-7">
        <h1
          style={{
            fontFamily: typography.h2.family,
            fontSize: typography.h2.size,
            lineHeight: typography.h2.lineHeight,
            letterSpacing: typography.h2.letterSpacing,
            fontWeight: 500,
            color: color.textPrimary,
          }}
        >
          Pools
        </h1>
        <p
          style={{
            fontFamily: typography.p2.family,
            fontSize: typography.p2.size,
            color: color.textMuted,
            lineHeight: typography.p2.lineHeight,
          }}
        >
          Multi-asset stable liquidity pools with capital-efficient ticks.
        </p>
      </header>

        {/* ── List ─────────────────────────────────────────────────── */}
        {isLoading && (
          <div
            className="py-20 text-center"
            style={{
              fontFamily: typography.p3.family,
              fontSize: typography.p3.size,
              color: color.textMuted,
            }}
          >
            Fetching on-chain data…
          </div>
        )}

        {isError && !isLoading && (
          <div
            className="py-20 text-center"
            style={{
              fontFamily: typography.p3.family,
              fontSize: typography.p3.size,
              color: color.error,
            }}
          >
            Failed to load pool data. Check RPC connection.
          </div>
        )}

        {!isLoading && !isError && (
          <div className="flex flex-col gap-3">
            {pools.map(p => (
              <PoolCard key={p.address} pool={p} />
            ))}
          </div>
        )}
    </section>
  );
}
