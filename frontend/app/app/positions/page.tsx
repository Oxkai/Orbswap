"use client";

import { color, typography } from "@/constants";
import { CurrencyDollar, Pulse, Wallet } from "@phosphor-icons/react";
import { useStellarWallet } from "@/lib/stellar/wallet";
import { usePositions } from "@/lib/hooks/usePositions";
import { usePool } from "@/lib/hooks/usePool";
import { TOKENS } from "@/lib/stellar/config";

const LBL = {
  fontFamily: typography.caption.family,
  fontSize: typography.caption.size,
  letterSpacing: "0.12em",
  textTransform: "uppercase" as const,
  fontWeight: 500,
};

function body(size: "p1" | "p2" | "p3" | "caption" = "p2", c: string = color.textPrimary) {
  const t = typography[size];
  return {
    fontFamily: t.family,
    fontSize: t.size,
    lineHeight: t.lineHeight,
    letterSpacing: t.letterSpacing,
    color: c,
    fontVariantNumeric: "tabular-nums" as const,
  };
}

function TokenIcon({ symbol, color: c, size = 24 }: { symbol: string; color: string; size?: number }) {
  return (
    <span
      style={{
        width: size, height: size, borderRadius: "50%", backgroundColor: c,
        display: "inline-flex", alignItems: "center", justifyContent: "center",
        flexShrink: 0, fontSize: Math.max(7, size * 0.36), color: "#fff",
        fontFamily: typography.caption.family, fontWeight: 700,
      }}
    >
      {symbol.slice(0, 2).toUpperCase()}
    </span>
  );
}

function StatItem({ icon, label, value, accent }: {
  icon: React.ReactNode; label: string; value: string; accent?: string;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-1.5">
        <span style={{ color: color.textMuted, lineHeight: 0 }}>{icon}</span>
        <span style={{ ...LBL, color: color.textMuted }}>{label}</span>
      </div>
      <span style={{ ...body("p1", accent ?? color.textPrimary), fontWeight: 500 }}>{value}</span>
    </div>
  );
}

const fmt = (n: number) =>
  n >= 1_000_000 ? (n / 1_000_000).toFixed(2) + "M" : n >= 1_000 ? (n / 1_000).toFixed(2) + "K" : n.toFixed(2);

export default function PositionsPage() {
  const { address, isConnected, connecting, connect } = useStellarWallet();
  const { position, isLoading } = usePositions(address ?? undefined);
  const { pool } = usePool();
  const tokens = pool?.tokens ?? TOKENS.map((t) => ({ ...t, name: t.symbol, balance: 0 }));

  return (
    <section className="flex-1 flex flex-col py-8 sm:py-10">
      <header className="flex flex-col gap-1.5 mb-7">
        <h1 style={{ fontFamily: typography.h2.family, fontSize: typography.h2.size, lineHeight: typography.h2.lineHeight, letterSpacing: typography.h2.letterSpacing, fontWeight: 500, color: color.textPrimary }}>
          Positions
        </h1>
        <p style={{ fontFamily: typography.p2.family, fontSize: typography.p2.size, color: color.textMuted, lineHeight: typography.p2.lineHeight }}>
          Your liquidity in the Orbswap 4-token pool on Stellar testnet.
        </p>
      </header>

      {!isConnected ? (
        <div className="flex flex-col items-center gap-4 py-20" style={{ backgroundColor: color.surface1 }}>
          <Wallet size={28} color={color.textMuted} weight="regular" />
          <p style={body("p3", color.textMuted)}>Connect your wallet to view positions.</p>
          <button
            onClick={connect}
            className="flex items-center justify-center h-11 px-6 hover:opacity-90 transition-opacity"
            style={{ backgroundColor: color.textPrimary, ...body("p2"), color: color.bg, fontWeight: 500, cursor: "pointer" }}
          >
            {connecting ? "Connecting…" : "Connect Freighter"}
          </button>
        </div>
      ) : isLoading ? (
        <div className="py-20 text-center" style={{ ...body("p3", color.textMuted), backgroundColor: color.surface1 }}>
          Loading your position…
        </div>
      ) : !position ? (
        <div className="flex flex-col items-center gap-3 py-20" style={{ backgroundColor: color.surface1 }}>
          <p style={body("p3", color.textMuted)}>No liquidity positions in this pool yet.</p>
          <p style={body("caption", color.textMuted)}>Deposit into the pool to see your stake here.</p>
        </div>
      ) : (
        <div className="flex flex-col gap-px">
          {/* Header row */}
          <div className="flex items-center justify-between px-5 py-4" style={{ backgroundColor: color.surface1 }}>
            <div className="flex items-center gap-3">
              <div className="flex -space-x-2">
                {tokens.map((t, i) => (
                  <span key={t.address} style={{ zIndex: tokens.length - i, outline: `2px solid ${color.bg}`, borderRadius: "50%" }}>
                    <TokenIcon symbol={t.symbol} color={t.color} size={26} />
                  </span>
                ))}
              </div>
              <div className="flex flex-col">
                <span style={{ ...body("p2"), fontWeight: 500 }}>{tokens.map((t) => t.symbol).join(" / ")}</span>
                <span style={body("caption", color.textMuted)}>4-asset SuperElliptical · 0.30% fee</span>
              </div>
            </div>
          </div>

          {/* Stat row */}
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-5 px-5 py-6" style={{ backgroundColor: color.surface1 }}>
            <StatItem icon={<CurrencyDollar size={13} weight="regular" />} label="Position value" value={fmt(position.value)} accent={color.success} />
            <StatItem icon={<Pulse size={13} weight="regular" />} label="Pool share" value={`${position.sharePct.toFixed(4)}%`} />
            <StatItem icon={<Pulse size={13} weight="regular" />} label="LP shares" value={fmt(Number(position.shares) / 1e7)} />
          </div>

          {/* Underlying breakdown */}
          <div className="px-5 py-5 flex flex-col gap-3" style={{ backgroundColor: color.surface1 }}>
            <span style={{ ...LBL, color: color.textMuted }}>Underlying</span>
            {tokens.map((t, i) => (
              <div key={t.address} className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <TokenIcon symbol={t.symbol} color={t.color} size={20} />
                  <span style={body("p3", color.textSecondary)}>{t.symbol}</span>
                </div>
                <span style={body("p3", color.textPrimary)}>{fmt(position.tokenAmounts[i] ?? 0)}</span>
              </div>
            ))}
          </div>

          <div className="px-5 py-3" style={{ backgroundColor: color.surface1 }}>
            <span style={body("caption", color.textMuted)}>
              Add / remove liquidity from the UI is coming next — for now use the seed/deposit scripts.
            </span>
          </div>
        </div>
      )}
    </section>
  );
}
