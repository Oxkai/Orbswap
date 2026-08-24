"use client";

import { useState, useEffect, useCallback, use } from "react";
import Link from "next/link";
import { ArrowLeft, ArrowSquareOut, Check, Circle, Info } from "@phosphor-icons/react";
import { color, typography } from "@/constants";
import { useStellarWallet } from "@/lib/stellar/wallet";
import { TOKENS, SCALE, explorerTx, NETWORK_LABEL } from "@/lib/stellar/config";
import { TICK_POOL, getTickState } from "@/lib/stellar/ticks";
import { getReservesOf, balanceOf, deposit, addLiquidity } from "@/lib/stellar/pool";

// ─── Design helpers ─────────────────────────────────────────────────────────
const MONO = "var(--font-mono)";
const LBL = {
  fontFamily: typography.caption.family,
  fontSize: "10px",
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

// ─── Amounts (7-decimal fixed point) ────────────────────────────────────────
const toNative = (d: number): bigint => BigInt(Math.round(d * SCALE));
const fromNative = (n: bigint): number => Number(n) / SCALE;
const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 4 });
const DEADLINE = () => BigInt(Math.floor(Date.now() / 1000) + 1200);


type Tok = { symbol: string; address: string; color: string; decimals: number };

function TokenBadge({ t, size = 26 }: { t: Tok; size?: number }) {
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        backgroundColor: t.color,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        fontSize: size * 0.36,
        color: "#fff",
        fontFamily: typography.caption.family,
        fontWeight: 700,
      }}
    >
      {t.symbol.slice(0, 2).toUpperCase()}
    </span>
  );
}

// ─── Step indicator ─────────────────────────────────────────────────────────
function StepBar({ step, labels }: { step: number; labels: string[] }) {
  return (
    <div className="flex items-center px-1 py-1">
      {labels.map((label, i) => {
        const n = i + 1;
        const active = n === step;
        const done = n < step;
        return (
          <div key={label} className="flex items-center" style={{ flex: i < labels.length - 1 ? 1 : "0 0 auto" }}>
            <div className="flex items-center gap-2 shrink-0">
              <span
                className="flex items-center justify-center"
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: "50%",
                  backgroundColor: done ? color.success : active ? color.textPrimary : color.surface2,
                  color: done || active ? color.bg : color.textMuted,
                  fontFamily: MONO,
                  fontSize: 11,
                  fontWeight: 600,
                }}
              >
                {done ? <Check size={12} weight="bold" /> : n}
              </span>
              <span style={{ ...body("caption", active ? color.textPrimary : color.textMuted), fontWeight: active ? 600 : 400 }}>
                {label}
              </span>
            </div>
            {i < labels.length - 1 && (
              <div className="flex-1 mx-3 h-px" style={{ backgroundColor: done ? color.success : color.borderSubtle }} />
            )}
          </div>
        );
      })}
    </div>
  );
}

// ─── Shared pieces ──────────────────────────────────────────────────────────
function InfoNote({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-start gap-3 px-5 py-3.5" style={{ backgroundColor: color.surface1 }}>
      <Info size={15} style={{ color: color.textMuted, marginTop: 1, flexShrink: 0 }} />
      <span style={body("caption", color.textMuted)}>{children}</span>
    </div>
  );
}

function AmountRow({
  t,
  value,
  onChange,
  balance,
  onMax,
  insufficient,
}: {
  t: Tok;
  value: string;
  onChange: (v: string) => void;
  balance: bigint | null;
  onMax?: () => void;
  insufficient?: boolean;
}) {
  return (
    <div className="px-5 py-4" style={{ backgroundColor: color.surface1 }}>
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2.5 shrink-0">
          <TokenBadge t={t} />
          <span style={{ ...body("p1"), fontWeight: 500 }}>{t.symbol}</span>
        </div>
        <input
          inputMode="decimal"
          placeholder="0.0"
          value={value}
          onChange={(e) => onChange(e.target.value.replace(/[^0-9.]/g, ""))}
          className="flex-1 min-w-0 bg-transparent outline-none text-right"
          style={{ fontFamily: MONO, fontSize: 22, color: color.textPrimary, fontVariantNumeric: "tabular-nums" }}
        />
      </div>
      <div className="flex items-center justify-between pt-2">
        <span style={body("caption", insufficient ? color.error : color.textMuted)}>
          {balance === null ? "Connect wallet" : `Balance ${fmt(fromNative(balance))}`}
        </span>
        {onMax && balance !== null && (
          <button onClick={onMax} className="hover:opacity-80" style={{ ...body("caption", color.textSecondary), cursor: "pointer" }}>
            MAX
          </button>
        )}
      </div>
    </div>
  );
}

function ReviewRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between px-5 py-3" style={{ backgroundColor: color.surface1 }}>
      <span style={body("caption", color.textMuted)}>{label}</span>
      <span style={{ ...body("p2"), textAlign: "right" }}>{children}</span>
    </div>
  );
}

function PrimaryButton({
  label,
  onClick,
  disabled,
  busy,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  busy?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="w-full flex items-center justify-center h-12 transition-opacity"
      style={{
        backgroundColor: disabled ? color.surface2 : color.textPrimary,
        ...body("p1", disabled ? color.textMuted : color.bg),
        fontWeight: 500,
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: busy ? 0.7 : 1,
      }}
    >
      {label}
    </button>
  );
}

function GhostButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="flex items-center justify-center h-12 px-6 hover:bg-(--color-surface-2) transition-colors"
      style={{ backgroundColor: color.surface1, ...body("p1", color.textSecondary), cursor: "pointer" }}
    >
      {label}
    </button>
  );
}

function SuccessCard({ hash, poolAddr, onReset }: { hash: string; poolAddr: string; onReset: () => void }) {
  return (
    <div className="flex flex-col gap-px">
      <div className="flex flex-col items-center gap-3 px-5 py-10" style={{ backgroundColor: color.surface1 }}>
        <span className="flex items-center justify-center" style={{ width: 44, height: 44, borderRadius: "50%", backgroundColor: `${color.success}22`, color: color.success }}>
          <Check size={22} weight="bold" />
        </span>
        <span style={{ ...body("p1"), fontWeight: 500 }}>Liquidity added</span>
        <a href={explorerTx(hash)} target="_blank" rel="noreferrer" className="flex items-center gap-1.5 hover:opacity-80" style={body("caption", color.textSecondary)}>
          {hash.slice(0, 8)}…{hash.slice(-6)} <ArrowSquareOut size={12} />
        </a>
      </div>
      <div className="flex gap-px">
        <button onClick={onReset} className="flex-1 flex items-center justify-center h-11 hover:bg-(--color-surface-2) transition-colors" style={{ ...body("p2"), backgroundColor: color.surface1, cursor: "pointer" }}>
          Add more
        </button>
        <Link href={`/app/pool/${poolAddr}`} className="flex-1 flex items-center justify-center h-11 hover:opacity-90 transition-opacity" style={{ ...body("p2", color.bg), backgroundColor: color.textPrimary, fontWeight: 500 }}>
          View pool
        </Link>
      </div>
    </div>
  );
}

// ─── Circular tick pool: Range → Amounts → Review ───────────────────────────
const RANGE_PRESETS = [
  { label: "Full", lower: 0, upper: 90, hint: "The whole arc — lowest concentration, always in range." },
  { label: "Wide", lower: 30, upper: 60, hint: "Balanced depth around the peg." },
  { label: "Narrow", lower: 40, upper: 50, hint: "Tight around $1 — highest capital efficiency." },
] as const;

function RangeTrack({ lower, upper }: { lower: number; upper: number }) {
  const pct = (v: number) => (v / 90) * 100;
  return (
    <div className="px-5 pt-6 pb-4" style={{ backgroundColor: color.surface1 }}>
      <div className="relative" style={{ height: 12 }}>
        <div className="absolute inset-x-0 top-1/2 -translate-y-1/2" style={{ height: 4, backgroundColor: color.surface2 }} />
        <div className="absolute top-1/2 -translate-y-1/2" style={{ left: `${pct(lower)}%`, width: `${pct(upper) - pct(lower)}%`, height: 4, backgroundColor: color.accent }} />
        {/* peg at 45° */}
        <div className="absolute top-0 bottom-0" style={{ left: `${pct(45)}%`, width: 2, backgroundColor: color.textPrimary, transform: "translateX(-1px)" }} />
        {[lower, upper].map((v, i) => (
          <div key={i} className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2" style={{ left: `${pct(v)}%`, width: 12, height: 12, borderRadius: "50%", backgroundColor: color.accent, outline: `2px solid ${color.surface1}` }} />
        ))}
      </div>
      <div className="flex justify-between pt-3" style={{ ...body("caption", color.textMuted), fontFamily: MONO }}>
        <span>0°</span>
        <span style={{ color: color.textSecondary }}>45° · peg</span>
        <span>90°</span>
      </div>
    </div>
  );
}

function TickFlow({ poolAddr, tokens }: { poolAddr: string; tokens: Tok[] }) {
  const { address, isConnected, connecting, connect, sign } = useStellarWallet();
  const [step, setStep] = useState(1);
  const [preset, setPreset] = useState(2); // Narrow
  const [balances, setBalances] = useState<(bigint | null)[]>([null, null]);
  const [tick, setTick] = useState<number | null>(null);
  const [amt, setAmt] = useState<[string, string]>(["", ""]);
  const [submitting, setSubmitting] = useState(false);
  const [hash, setHash] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try { setTick((await getTickState()).currentTick); } catch {}
    if (address) setBalances(await Promise.all(tokens.map((t) => balanceOf(t.address, address).catch(() => null))));
  }, [address, tokens]);
  useEffect(() => { load(); }, [load]);

  const range = RANGE_PRESETS[preset];
  const amounts: [bigint, bigint] = [toNative(parseFloat(amt[0]) || 0), toNative(parseFloat(amt[1]) || 0)];
  const insufficientIdx = amounts.findIndex((a, j) => balances[j] !== null && a > (balances[j] as bigint));
  const hasAmount = amounts[0] > 0n || amounts[1] > 0n;

  const submit = async () => {
    if (!isConnected) { connect(); return; }
    if (!address) return;
    setSubmitting(true); setError(null);
    try {
      const h = await addLiquidity({ poolId: poolAddr, from: address, amounts, lower: range.lower, upper: range.upper, minLiquidity: 0n, deadline: DEADLINE(), sign });
      setHash(h);
    } catch (e) { setError(e instanceof Error ? e.message : "Add liquidity failed"); }
    finally { setSubmitting(false); }
  };

  if (hash) return <SuccessCard hash={hash} poolAddr={poolAddr} onReset={() => { setHash(null); setAmt(["", ""]); setStep(1); load(); }} />;

  return (
    <div className="flex flex-col gap-5">
      <StepBar step={step} labels={["Range", "Amounts", "Review"]} />

      {/* Step 1 — Range */}
      {step === 1 && (
        <div className="flex flex-col gap-5">
          <InfoNote>
            Concentrated liquidity lives on an arc of the circle. 45° is the $1 peg{tick !== null ? `; the pool sits at ${tick}° now` : ""}. A tighter range earns more fees per dollar but leaves range sooner.
          </InfoNote>
          <div className="flex flex-col gap-px">
            <div className="grid grid-cols-3 gap-px">
              {RANGE_PRESETS.map((r, i) => {
                const on = i === preset;
                return (
                  <button key={r.label} onClick={() => setPreset(i)} className="flex flex-col items-center gap-1 py-3.5 transition-colors" style={{ backgroundColor: on ? color.surface2 : color.surface1, borderBottom: `2px solid ${on ? color.accent : "transparent"}`, cursor: "pointer" }}>
                    <span style={{ ...body("p2", on ? color.textPrimary : color.textSecondary), fontWeight: 500 }}>{r.label}</span>
                    <span style={{ fontFamily: MONO, fontSize: 10, color: color.textMuted }}>{r.lower}°–{r.upper}°</span>
                  </button>
                );
              })}
            </div>
            <RangeTrack lower={range.lower} upper={range.upper} />
          </div>
          <span style={body("caption", color.textMuted)} className="px-1">{range.hint}</span>
          <PrimaryButton label="Continue" onClick={() => setStep(2)} />
        </div>
      )}

      {/* Step 2 — Amounts */}
      {step === 2 && (
        <div className="flex flex-col gap-5">
          <InfoNote>You provide up to these amounts; the pool pulls what the {range.lower}°–{range.upper}° range needs at the current price.</InfoNote>
          <div className="flex flex-col gap-px">
            {tokens.map((t, j) => (
              <AmountRow
                key={t.address}
                t={t}
                value={amt[j]}
                onChange={(v) => setAmt((p) => (j === 0 ? [v, p[1]] : [p[0], v]))}
                balance={balances[j]}
                onMax={balances[j] !== null ? () => setAmt((p) => (j === 0 ? [String(fromNative(balances[0] as bigint)), p[1]] : [p[0], String(fromNative(balances[1] as bigint))])) : undefined}
                insufficient={balances[j] !== null && amounts[j] > (balances[j] as bigint)}
              />
            ))}
          </div>
          <div className="flex gap-px">
            <GhostButton label="Back" onClick={() => setStep(1)} />
            <div className="flex-1">
              <PrimaryButton
                label={!hasAmount ? "Enter an amount" : insufficientIdx >= 0 ? `Insufficient ${tokens[insufficientIdx].symbol}` : "Review"}
                disabled={!hasAmount || insufficientIdx >= 0}
                onClick={() => setStep(3)}
              />
            </div>
          </div>
        </div>
      )}

      {/* Step 3 — Review */}
      {step === 3 && (
        <div className="flex flex-col gap-5">
          <div className="flex flex-col gap-px">
            <ReviewRow label="Range">{range.lower}°–{range.upper}° ({range.label})</ReviewRow>
            {tokens.map((t, j) => (
              <ReviewRow key={t.address} label={`Deposit ${t.symbol}`}>{fmt(fromNative(amounts[j]))}</ReviewRow>
            ))}
            <ReviewRow label="Network">{NETWORK_LABEL}</ReviewRow>
          </div>
          {error && <span style={body("caption", color.error)} className="px-1">{error}</span>}
          <div className="flex gap-px">
            <GhostButton label="Back" onClick={() => setStep(2)} />
            <div className="flex-1">
              <PrimaryButton
                label={!isConnected ? (connecting ? "Connecting…" : "Connect Freighter") : submitting ? "Adding liquidity…" : "Add liquidity"}
                busy={submitting}
                disabled={isConnected && submitting}
                onClick={submit}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── SuperElliptical: Amounts → Review (balanced multi-token deposit) ────────
function DepositFlow({ poolAddr, tokens }: { poolAddr: string; tokens: Tok[] }) {
  const { address, isConnected, connecting, connect, sign } = useStellarWallet();
  const [step, setStep] = useState(1);
  const [reserves, setReserves] = useState<bigint[] | null>(null);
  const [balances, setBalances] = useState<(bigint | null)[]>(tokens.map(() => null));
  const [driver, setDriver] = useState<{ i: number; value: string }>({ i: 0, value: "" });
  const [submitting, setSubmitting] = useState(false);
  const [hash, setHash] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try { setReserves(await getReservesOf(poolAddr)); } catch {}
    if (address) setBalances(await Promise.all(tokens.map((t) => balanceOf(t.address, address).catch(() => null))));
  }, [poolAddr, address, tokens]);
  useEffect(() => { load(); }, [load]);

  const driverNative = toNative(parseFloat(driver.value) || 0);
  const amounts: bigint[] = tokens.map((_, j) => {
    if (!reserves || reserves[driver.i] <= 0n) return j === driver.i ? driverNative : 0n;
    return (reserves[j] * driverNative) / reserves[driver.i];
  });
  const insufficientIdx = amounts.findIndex((a, j) => balances[j] !== null && a > (balances[j] as bigint));
  const hasAmount = driverNative > 0n;
  const total = reserves ? reserves.reduce((a, b) => a + b, 0n) : 0n;
  const addedTotal = amounts.reduce((a, b) => a + b, 0n);
  const poolShare = total + addedTotal > 0n ? (Number(addedTotal) / Number(total + addedTotal)) * 100 : 0;

  const rowVal = (j: number) => (j === driver.i ? driver.value : amounts[j] > 0n ? fmt(fromNative(amounts[j])) : "");

  const submit = async () => {
    if (!isConnected) { connect(); return; }
    if (!address) return;
    setSubmitting(true); setError(null);
    try {
      const h = await deposit({ poolId: poolAddr, from: address, amounts, minShares: 0n, deadline: DEADLINE(), sign });
      setHash(h);
    } catch (e) { setError(e instanceof Error ? e.message : "Deposit failed"); }
    finally { setSubmitting(false); }
  };

  if (hash) return <SuccessCard hash={hash} poolAddr={poolAddr} onReset={() => { setHash(null); setDriver({ i: 0, value: "" }); setStep(1); load(); }} />;

  return (
    <div className="flex flex-col gap-5">
      <StepBar step={step} labels={["Amounts", "Review"]} />

      {/* Step 1 — Amounts */}
      {step === 1 && (
        <div className="flex flex-col gap-5">
          <InfoNote>Deposits stay balanced across all {tokens.length} assets. Enter one amount and the rest scale to the pool&apos;s current ratio.</InfoNote>
          <div className="flex flex-col gap-px">
            {tokens.map((t, j) => (
              <AmountRow
                key={t.address}
                t={t}
                value={rowVal(j)}
                onChange={(v) => setDriver({ i: j, value: v })}
                balance={balances[j]}
                onMax={balances[j] !== null ? () => setDriver({ i: j, value: String(fromNative(balances[j] as bigint)) }) : undefined}
                insufficient={balances[j] !== null && amounts[j] > (balances[j] as bigint)}
              />
            ))}
          </div>
          <PrimaryButton
            label={!hasAmount ? "Enter an amount" : insufficientIdx >= 0 ? `Insufficient ${tokens[insufficientIdx].symbol}` : "Review"}
            disabled={!hasAmount || insufficientIdx >= 0}
            onClick={() => setStep(2)}
          />
        </div>
      )}

      {/* Step 2 — Review */}
      {step === 2 && (
        <div className="flex flex-col gap-5">
          <div className="flex flex-col gap-px">
            {tokens.map((t, j) => (
              <ReviewRow key={t.address} label={`Deposit ${t.symbol}`}>{fmt(fromNative(amounts[j]))}</ReviewRow>
            ))}
            <ReviewRow label="Pool share">≈ {poolShare < 0.01 ? "<0.01" : poolShare.toFixed(2)}%</ReviewRow>
            <ReviewRow label="Network">{NETWORK_LABEL}</ReviewRow>
          </div>
          {error && <span style={body("caption", color.error)} className="px-1">{error}</span>}
          <div className="flex gap-px">
            <GhostButton label="Back" onClick={() => setStep(1)} />
            <div className="flex-1">
              <PrimaryButton
                label={!isConnected ? (connecting ? "Connecting…" : "Connect Freighter") : submitting ? "Adding liquidity…" : "Add liquidity"}
                busy={submitting}
                disabled={isConnected && submitting}
                onClick={submit}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default function AddLiquidityPage({ params }: { params: Promise<{ address: string }> }) {
  const { address: poolAddr } = use(params);
  const isTick = poolAddr === TICK_POOL.id;

  const tokens: Tok[] = isTick
    ? TICK_POOL.tokens.map((t) => ({ symbol: t.symbol, address: t.address, color: t.color, decimals: 7 }))
    : TOKENS.map((t) => ({ symbol: t.symbol, address: t.address, color: t.color, decimals: t.decimals }));

  const pair = tokens.map((t) => t.symbol).join(" / ");

  return (
    <section className="flex-1 flex flex-col py-8 sm:py-10">
      <div className="w-full max-w-md mx-auto flex flex-col gap-4">
        <Link href={`/app/pool/${poolAddr}`} className="inline-flex items-center gap-1.5 hover:opacity-80 w-fit" style={body("caption", color.textMuted)}>
          <ArrowLeft size={13} /> Back to pool
        </Link>

        {/* Header card */}
        <div className="flex items-center justify-between px-5 py-4" style={{ backgroundColor: color.surface1 }}>
          <div className="flex items-center gap-3 min-w-0">
            <div className="flex items-center shrink-0">
              {tokens.map((t, i) => (
                <span key={t.address} style={{ marginLeft: i === 0 ? 0 : -8, outline: `2px solid ${color.surface1}`, borderRadius: "50%", position: "relative", zIndex: tokens.length - i }}>
                  <TokenBadge t={t} size={28} />
                </span>
              ))}
            </div>
            <div className="flex flex-col min-w-0">
              <span style={{ ...body("p1"), fontWeight: 600 }}>Add liquidity</span>
              <span style={body("caption", color.textMuted)}>{pair}</span>
            </div>
          </div>
          <div className="flex items-center gap-1.5 px-2.5 h-7 shrink-0" style={{ backgroundColor: color.surface2 }}>
            <Circle size={7} color={color.success} weight="fill" />
            <span style={{ fontFamily: typography.caption.family, fontSize: "11px", color: color.textSecondary, whiteSpace: "nowrap" }}>
              {isTick ? "Circular" : "SuperElliptical"}
            </span>
          </div>
        </div>

        {isTick ? <TickFlow poolAddr={poolAddr} tokens={tokens} /> : <DepositFlow poolAddr={poolAddr} tokens={tokens} />}
      </div>
    </section>
  );
}
