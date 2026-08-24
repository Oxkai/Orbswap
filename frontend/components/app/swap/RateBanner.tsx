"use client";

import { useEffect, useState } from "react";
import { color } from "@/constants";
import { Badge } from "@/components/app/shared/Badge";
import {
  rateStatus,
  spreadBps,
  haltMessage,
  type RateStatus,
} from "@/lib/stellar/rates";

interface RateBannerProps {
  poolId: string;
  /** Numeraire leg (index 0) — the side the rate is quoted against. */
  numeraire: { symbol: string; address: string; decimals: number };
  /** Quote leg (index 1) — the local-currency side the oracle prices. */
  quote: { symbol: string; address: string; decimals: number };
  /** Probe size for the spread comparison, in numeraire display units. */
  probe?: number;
  /** Poll interval, ms. Defaults to 30s; the feed itself updates far slower. */
  refreshMs?: number;
}

interface View {
  status: RateStatus;
  spread: number | null;
}

/**
 * Live FX rate, the pool's spread against the oracle mid, and — when the pool is
 * closed — why.
 *
 * The three halt states are not interchangeable, so the banner says which one it
 * is: a stale rate clears itself on the next feed update, while a latched breaker
 * needs an operator. In every case withdrawals stay open, and saying so is the
 * point: a paused pool is not a trapped one.
 */
export function RateBanner({
  poolId,
  numeraire,
  quote,
  probe = 1,
  refreshMs = 30_000,
}: RateBannerProps) {
  const [view, setView] = useState<View | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const status = await rateStatus(poolId);
        // Skip the spread probe while the pool is closed — `quote` would only
        // describe a price nobody can trade at.
        let spread: number | null = null;
        if (!status.halt) {
          const amountIn = BigInt(Math.round(probe * 10 ** numeraire.decimals));
          const s = await spreadBps(poolId, numeraire.address, quote.address, amountIn);
          spread = s?.spreadBps ?? null;
        }
        if (!cancelled) {
          setView({ status, spread });
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : "rate unavailable");
      }
    }

    load();
    const t = setInterval(load, refreshMs);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [poolId, numeraire.address, numeraire.decimals, quote.address, probe, refreshMs]);

  if (error) {
    return (
      <div style={wrap}>
        <Badge variant="error" dot>
          rate unavailable
        </Badge>
      </div>
    );
  }

  if (!view) {
    return (
      <div style={wrap}>
        <Badge variant="muted" dot>
          loading rate
        </Badge>
      </div>
    );
  }

  const { status, spread } = view;
  const halt = haltMessage(status.halt);
  // 1 quote unit is worth `rate` numeraire, so one numeraire buys 1/rate.
  const perNumeraire = status.rate > 0 ? 1 / status.rate : 0;

  return (
    <div style={wrap}>
      <div className="flex items-center justify-between gap-3">
        <span style={label}>oracle rate</span>
        <span style={value}>
          1 {numeraire.symbol} = {fmt(perNumeraire)} {quote.symbol}
        </span>
      </div>

      {spread !== null && (
        <div className="flex items-center justify-between gap-3">
          <span style={label}>pool spread vs mid</span>
          <span style={value}>{spread.toFixed(1)} bps</span>
        </div>
      )}

      <div className="flex items-center justify-between gap-3">
        <span style={label}>status</span>
        {status.halt ? (
          <Badge variant={status.halt === "breaker" ? "error" : "warning"} dot>
            {status.halt === "breaker"
              ? "halted"
              : status.halt === "stale"
                ? "stale rate"
                : "repegging"}
          </Badge>
        ) : (
          <Badge variant="success" dot>
            live
          </Badge>
        )}
      </div>

      {halt && (
        <p
          style={{
            color: color.textMuted,
            fontFamily: "var(--font-mono)",
            fontSize: 10,
            lineHeight: "15px",
            margin: 0,
          }}
        >
          {halt}
        </p>
      )}
    </div>
  );
}

function fmt(n: number): string {
  if (n === 0) return "—";
  if (n >= 1000) return n.toLocaleString(undefined, { maximumFractionDigits: 0 });
  if (n >= 1) return n.toLocaleString(undefined, { maximumFractionDigits: 4 });
  return n.toPrecision(4);
}

const wrap: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
  padding: "10px 12px",
  border: `1px solid ${color.borderSubtle}`,
  backgroundColor: color.surface1,
};

const label: React.CSSProperties = {
  color: color.textMuted,
  fontFamily: "var(--font-mono)",
  fontSize: 9,
  letterSpacing: "0.08em",
  textTransform: "uppercase",
};

const value: React.CSSProperties = {
  color: color.textPrimary,
  fontFamily: "var(--font-mono)",
  fontSize: 11,
};
