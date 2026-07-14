"use client";

import { useState, useRef, useEffect, useCallback, useMemo } from "react";
import {
  GearSix,
  ArrowDown,
  ArrowsDownUp,
  CaretDown,
  CaretUp,
  X,
  Check,
  Circle,
  ArrowSquareOut,
} from "@phosphor-icons/react";
import { TokenDAI, TokenUSDT, TokenUSDC, TokenFRAX } from "@token-icons/react";
import { color, typography } from "@/constants";
import { useStellarWallet } from "@/lib/stellar/wallet";
import { balanceOf } from "@/lib/stellar/pool";
import { findRoute, quoteRoute, swapRoute } from "@/lib/stellar/route";
import { TOKENS, STELLAR, toNative, fromNative } from "@/lib/stellar/config";
import { TICK_POOL } from "@/lib/stellar/ticks";

// Every tradable token across all live pools, tagged with the pool it trades in.
// A pair is only valid within one pool (the two pools share no token), so the
// widget keeps both sides in the same pool.
type SwapToken = { symbol: string; address: string; color: string; decimals: number; pool: string };
const ALL_TOKENS: SwapToken[] = [
  ...TOKENS.map((t) => ({ symbol: t.symbol, address: t.address, color: t.color, decimals: t.decimals, pool: STELLAR.pool })),
  ...TICK_POOL.tokens.map((t) => ({ symbol: t.symbol, address: t.address, color: t.color, decimals: 7, pool: TICK_POOL.id })),
];

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const TOKEN_ICON_MAP: Record<string, React.ComponentType<any>> = {
  DAI:  TokenDAI,
  USDT: TokenUSDT,
  USDC: TokenUSDC,
  FRAX: TokenFRAX,
};
const TOKEN_COLOR_MAP: Record<string, string> = {
  CRVUSD: "#FF6B35",
  USDA: "#4F9DFF", USDB: "#35C08E", USDD: "#B26BFF",
  CIRA: "#4F9DFF", CIRB: "#35C08E",
};

function TokenIcon({ symbol, size = 28 }: { symbol: string; size?: number }) {
  const Icon = TOKEN_ICON_MAP[symbol.toUpperCase()];
  if (Icon) return <Icon size={size} variant="branded" />;
  const fallbackBg = TOKEN_COLOR_MAP[symbol.toUpperCase()] ?? "#555";
  const inner = Math.round(size * 0.6);
  return (
    <span
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        backgroundColor: fallbackBg,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        fontSize: Math.max(6, inner * 0.42),
        color: "#fff",
        fontFamily: typography.caption.family,
        fontWeight: 700,
      }}
    >
      {symbol.slice(0, 2).toUpperCase()}
    </span>
  );
}

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

function StatusPill({ healthy, label }: { healthy: boolean; label: string }) {
  const c = healthy ? color.success : color.warning;
  return (
    <span
      className="inline-flex items-center gap-1.5"
      style={{
        backgroundColor: `${c}1f`,
        color: c,
        fontFamily: typography.caption.family,
        fontSize: "11px",
        fontWeight: 500,
        letterSpacing: "0.04em",
        padding: "3px 9px",
        borderRadius: 999,
        whiteSpace: "nowrap",
      }}
    >
      <Circle size={6} color={c} weight="fill" />
      {label}
    </span>
  );
}

function fmtBalance(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + "M";
  if (n >= 1_000)     return (n / 1_000).toFixed(2) + "K";
  return n.toFixed(4);
}

// ─── Token dropdown ───────────────────────────────────────────────────────────

interface DropdownProps {
  tokens: { symbol: string; address: string; color: string; balance: number }[];
  selected: number;
  excluded: number;
  onSelect: (idx: number) => void;
  onClose: () => void;
}

function TokenDropdown({ tokens, selected, excluded, onSelect, onClose }: DropdownProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  return (
    <div
      ref={ref}
      className="absolute right-0 top-full mt-2 z-50 w-60 flex flex-col gap-px"
      style={{ boxShadow: "0 12px 32px rgba(0,0,0,0.4)" }}
    >
      {tokens.map((token, idx) => (
        <button
          key={token.address}
          onClick={() => { onSelect(idx); onClose(); }}
          className="w-full flex items-center gap-3 px-4 py-3 hover:bg-(--color-surface-2) transition-colors"
          style={{ backgroundColor: idx === selected ? color.surface2 : color.surface1 }}
        >
          <TokenIcon symbol={token.symbol} size={22} />
          <span className="flex-1 text-left" style={{ ...body("p2", color.textPrimary), fontWeight: 500 }}>
            {token.symbol}
          </span>
          <div className="flex items-center gap-2">
            <span style={body("caption", color.textMuted)}>
              {token.balance.toFixed(2)}
            </span>
            {idx === selected && <Check size={11} color={color.success} weight="bold" />}
            {idx === excluded && (
              <ArrowsDownUp size={11} color={color.textMuted} weight="regular" />
            )}
          </div>
        </button>
      ))}
    </div>
  );
}

// ─── Token box ────────────────────────────────────────────────────────────────

function TokenBox({ mode, token, otherIdx, tokenIdx, tokens, value, onChange, onTokenSelect, isConnected }: {
  mode: "in" | "out";
  token: { symbol: string; address: string; color: string; balance: number };
  tokenIdx: number;
  otherIdx: number;
  tokens: { symbol: string; address: string; color: string; balance: number }[];
  value: string;
  onChange?: (v: string) => void;
  onTokenSelect: (idx: number) => void;
  isConnected: boolean;
}) {
  const [open, setOpen] = useState(false);
  const numVal = parseFloat(value) || 0;
  const balanceStr = !isConnected ? "—" : fmtBalance(token.balance);

  return (
    <div className="relative px-5 py-5" style={{ backgroundColor: color.surface1 }}>
      {/* Top row — label + balance */}
      <div className="flex items-center justify-between mb-3">
        <span style={{ ...LBL, color: color.textMuted }}>
          {mode === "in" ? "You pay" : "You receive"}
        </span>
        {mode === "in" && (
          <span style={body("caption", color.textMuted)}>
            Balance {balanceStr}
          </span>
        )}
      </div>

      {/* Big input row */}
      <div className="flex items-center gap-3">
        <div className="flex-1 min-w-0">
          {mode === "in" ? (
            <input
              type="text" inputMode="decimal" placeholder="0" value={value}
              onChange={e => { if (/^\d*(?:\.\d*)?$/.test(e.target.value)) onChange?.(e.target.value); }}
              className="w-full bg-transparent outline-none"
              style={{
                fontFamily: typography.h1.family,
                fontSize: "36px",
                letterSpacing: "-0.03em",
                fontWeight: 500,
                color: value ? color.textPrimary : color.textMuted,
                lineHeight: 1,
                fontVariantNumeric: "tabular-nums",
              }}
            />
          ) : (
            <div
              style={{
                fontFamily: typography.h1.family,
                fontSize: "36px",
                letterSpacing: "-0.03em",
                fontWeight: 500,
                color: value ? color.textPrimary : color.textMuted,
                lineHeight: 1,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {value || "0"}
            </div>
          )}
        </div>

        <div className="relative shrink-0">
          <button
            onClick={() => setOpen(v => !v)}
            className="flex items-center gap-2 pl-2 pr-3 h-10 hover:bg-(--color-surface-3) transition-colors"
            style={{ backgroundColor: color.surface2 }}
          >
            <TokenIcon symbol={token.symbol} size={24} />
            <span style={{ ...body("p2", color.textPrimary), fontWeight: 500 }}>
              {token.symbol}
            </span>
            <CaretDown
              size={11}
              color={color.textMuted}
              weight="regular"
              style={{ transform: open ? "rotate(180deg)" : "rotate(0deg)", transition: "transform 0.15s" }}
            />
          </button>
          {open && (
            <TokenDropdown tokens={tokens} selected={tokenIdx} excluded={otherIdx}
              onSelect={onTokenSelect} onClose={() => setOpen(false)} />
          )}
        </div>
      </div>

      {/* Bottom row — USD + max */}
      <div className="flex items-center justify-between mt-3">
        <span style={body("caption", color.textMuted)}>
          ≈ ${numVal > 0 ? numVal.toFixed(2) : "0.00"}
        </span>
        {mode === "in" && isConnected && token.balance > 0 && (
          <button
            onClick={() => onChange?.(token.balance.toFixed(6))}
            className="hover:opacity-90 transition-opacity"
            style={{
              fontFamily: typography.caption.family,
              fontSize: "11px",
              fontWeight: 500,
              letterSpacing: "0.06em",
              textTransform: "uppercase",
              color: color.accent,
              cursor: "pointer",
            }}
          >
            Max
          </button>
        )}
      </div>
    </div>
  );
}

// ─── Settings panel ──────────────────────────────────────────────────────────

function SettingsPanel({ slippage, setSlippage, deadline, setDeadline, onClose }: {
  slippage: number; setSlippage: (v: number) => void;
  deadline: number; setDeadline: (v: number) => void;
  onClose: () => void;
}) {
  return (
    <div className="px-5 py-5 flex flex-col gap-4" style={{ backgroundColor: color.surface1 }}>
      <div className="flex items-center justify-between">
        <span style={{ ...LBL, color: color.textMuted }}>Settings</span>
        <button onClick={onClose} className="hover:opacity-100 opacity-70 transition-opacity">
          <X size={13} color={color.textMuted} weight="regular" />
        </button>
      </div>

      <div className="flex flex-col gap-2">
        <span style={body("caption", color.textMuted)}>Slippage tolerance</span>
        <div className="flex gap-2">
          {[0.1, 0.5, 1].map(v => {
            const active = slippage === v;
            return (
              <button
                key={v}
                onClick={() => setSlippage(v)}
                className="flex-1 flex items-center justify-center h-9 hover:opacity-90 transition-opacity"
                style={{
                  backgroundColor: active ? `${color.accent}1f` : color.surface2,
                  color: active ? color.accent : color.textSecondary,
                  fontFamily: typography.p3.family,
                  fontSize: typography.p3.size,
                  fontWeight: active ? 500 : 400,
                  cursor: "pointer",
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                {v}%
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <span style={body("caption", color.textMuted)}>Transaction deadline</span>
        <div className="flex gap-2">
          {[5, 10, 30].map(v => {
            const active = deadline === v;
            return (
              <button
                key={v}
                onClick={() => setDeadline(v)}
                className="flex-1 flex items-center justify-center h-9 hover:opacity-90 transition-opacity"
                style={{
                  backgroundColor: active ? `${color.accent}1f` : color.surface2,
                  color: active ? color.accent : color.textSecondary,
                  fontFamily: typography.p3.family,
                  fontSize: typography.p3.size,
                  fontWeight: active ? 500 : 400,
                  cursor: "pointer",
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                {v}m
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ─── Info panel ───────────────────────────────────────────────────────────────

function SwapInfoPanel({ tokenIn, tokenOut, tokens, numIn, amountOut, slippage, fee }: {
  tokenIn: number; tokenOut: number;
  tokens: { symbol: string }[];
  numIn: number; amountOut: number; slippage: number; fee: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const [rateFlipped, setRateFlipped] = useState(false);
  const inSym  = tokens[tokenIn]?.symbol  ?? "—";
  const outSym = tokens[tokenOut]?.symbol ?? "—";
  const hasValues = numIn > 0 && amountOut > 0;
  const feeAmt    = hasValues ? numIn * fee / 1_000_000 : 0;
  const minOut    = hasValues ? amountOut * (1 - slippage / 100) : 0;
  const rate      = hasValues ? amountOut / numIn : 0;
  const rateStr   = hasValues
    ? rateFlipped ? `1 ${outSym} = ${(1 / rate).toFixed(5)} ${inSym}` : `1 ${inSym} = ${rate.toFixed(5)} ${outSym}`
    : `1 ${inSym} = — ${outSym}`;

  return (
    <div style={{ backgroundColor: color.surface1 }}>
      <button
        onClick={() => setExpanded(v => !v)}
        className="w-full flex items-center justify-between px-5 py-4 hover:bg-(--color-surface-2) transition-colors"
      >
        <span style={body("p3", color.textSecondary)}>{rateStr}</span>
        <div className="flex items-center gap-2">
          {hasValues && (
            <span
              onClick={e => { e.stopPropagation(); setRateFlipped(v => !v); }}
              style={{ lineHeight: 0, cursor: "pointer" }}
            >
              <ArrowsDownUp size={12} color={color.textMuted} weight="regular" />
            </span>
          )}
          {expanded
            ? <CaretUp size={12} color={color.textMuted} weight="regular" />
            : <CaretDown size={12} color={color.textMuted} weight="regular" />}
        </div>
      </button>
      {expanded && (
        <div
          className="px-5 py-4 flex flex-col gap-2.5"
          style={{ borderTop: `1px dashed ${color.borderSubtle}` }}
        >
          {[
            { label: "Fee",          value: hasValues ? `${feeAmt.toFixed(4)} ${inSym}` : "—", note: hasValues ? `${(fee / 10000).toFixed(2)}%` : undefined },
            { label: "Slippage",     value: `${slippage}%` },
            { label: "Min received", value: hasValues ? `${minOut.toFixed(4)} ${outSym}` : "—" },
            { label: "Route",        value: "Direct" },
          ].map(r => (
            <div key={r.label} className="flex items-center justify-between">
              <span style={body("p3", color.textMuted)}>{r.label}</span>
              <span style={body("p3", color.textSecondary)}>
                {r.value}
                {r.note && <span style={{ color: color.textMuted, marginLeft: 4 }}>({r.note})</span>}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Main widget ──────────────────────────────────────────────────────────────


export function SwapWidget() {
  const [tokenIn,      setTokenIn]      = useState(0);
  const [tokenOut,     setTokenOut]     = useState(1);
  const [amountIn,     setAmountIn]     = useState("");
  const [amountOut,    setAmountOut]    = useState(0);
  const [quoting,      setQuoting]      = useState(false);
  const [submitting,   setSubmitting]   = useState(false);
  const [balances,     setBalances]     = useState<number[]>(ALL_TOKENS.map(() => 0));
  const [showSettings, setShowSettings] = useState(false);
  const [deadline,     setDeadline]     = useState(10);
  const [slippage,     setSlippage]     = useState(0.5);
  const [swapResult,   setSwapResult]   = useState<{ success: boolean; hash?: string; msg: string } | null>(null);
  const resultTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const { address, isConnected, connecting, connect, sign } = useStellarWallet();

  function showResult(result: { success: boolean; hash?: string; msg: string }) {
    if (resultTimer.current) clearTimeout(resultTimer.current);
    setSwapResult(result);
    resultTimer.current = setTimeout(() => setSwapResult(null), 6000);
  }

  // Tokens across every live pool; a pair resolves to a route through the pool graph.
  const tokens = ALL_TOKENS;
  const fee = 3000; // 30 bps, expressed as parts-per-million for the info panel
  const route = useMemo(
    () => (tokens[tokenIn] && tokens[tokenOut] ? findRoute(tokens[tokenIn].address, tokens[tokenOut].address) : null),
    [tokenIn, tokenOut, tokens],
  );

  const refreshBalances = useCallback(async () => {
    if (!address) { setBalances(ALL_TOKENS.map(() => 0)); return; }
    const bs = await Promise.all(
      ALL_TOKENS.map(t => balanceOf(t.address, address).then(fromNative).catch(() => 0))
    );
    setBalances(bs);
  }, [address]);

  useEffect(() => { refreshBalances(); }, [refreshBalances]);

  const tokensWithBalance = tokens.map((t, i) => ({ ...t, balance: balances[i] ?? 0 }));

  const numIn = parseFloat(amountIn) || 0;

  // Live quote (read-only simulation) whenever the input or pair changes.
  useEffect(() => {
    let cancelled = false;
    if (numIn <= 0 || tokenIn === tokenOut || !route) { setAmountOut(0); return; }
    setQuoting(true);
    quoteRoute(route, toNative(numIn))
      .then(out => { if (!cancelled) setAmountOut(fromNative(out)); })
      .catch(() => { if (!cancelled) setAmountOut(0); })
      .finally(() => { if (!cancelled) setQuoting(false); });
    return () => { cancelled = true; };
  }, [numIn, tokenIn, tokenOut, route]);

  const amountOutStr = amountOut > 0 ? amountOut.toFixed(5) : "";
  const inBalance      = tokensWithBalance[tokenIn]?.balance ?? 0;
  const isInsufficient = numIn > inBalance;

  const flip = useCallback(() => {
    const prev = amountOut;
    setTokenIn(tokenOut);
    setTokenOut(tokenIn);
    setAmountIn(prev > 0 ? prev.toFixed(5) : "");
  }, [tokenIn, tokenOut, amountOut]);

  // Free token selection; the route (or its absence) is derived from the pair.
  const handleInSelect  = useCallback((idx: number) => { if (idx === tokenOut) flip(); else setTokenIn(idx); }, [tokenOut, flip]);
  const handleOutSelect = useCallback((idx: number) => { if (idx === tokenIn)  flip(); else setTokenOut(idx); }, [tokenIn, flip]);

  async function handleSwap() {
    if (!address || !route) return;
    const inSym  = tokensWithBalance[tokenIn]?.symbol  ?? "";
    const outSym = tokensWithBalance[tokenOut]?.symbol ?? "";
    const minOut = amountOut > 0 ? toNative(amountOut * (1 - slippage / 100)) : 0n;
    const dl     = BigInt(Math.floor(Date.now() / 1000) + deadline * 60);
    setSubmitting(true);
    try {
      const hash = await swapRoute({
        route,
        from: address,
        amountIn: toNative(numIn),
        minOut,
        deadline: dl,
        sign,
      });
      setAmountIn("");
      setAmountOut(0);
      showResult({ success: true, hash, msg: `Swapped ${numIn.toFixed(4)} ${inSym} → ${amountOut.toFixed(4)} ${outSym}` });
      refreshBalances();
    } catch (e) {
      showResult({ success: false, msg: e instanceof Error ? e.message.split("\n")[0] : "Swap failed" });
    } finally {
      setSubmitting(false);
    }
  }

  const btnLabel = !isConnected
    ? (connecting ? "Connecting…" : "Connect Freighter")
    : !route
    ? "No route for this pair"
    : numIn <= 0
    ? "Enter an amount"
    : isInsufficient
    ? `Insufficient ${tokensWithBalance[tokenIn]?.symbol ?? ""}`
    : submitting
    ? "Swapping…"
    : quoting
    ? "Fetching quote…"
    : "Swap";

  const btnDisabled = isConnected && (!route || numIn <= 0 || isInsufficient || submitting);

  function handleBtn() {
    if (!isConnected) { connect(); return; }
    handleSwap();
  }

  return (
    <div className="flex flex-col w-full">
      {/* ── Header row ─────────────────────────────────────────── */}
      <div
        className="flex items-center justify-between px-5 py-3.5"
        style={{ backgroundColor: color.surface1 }}
      >
        <span
          style={{
            fontFamily: typography.p2.family,
            fontSize: typography.p2.size,
            fontWeight: 500,
            letterSpacing: "-0.01em",
            color: color.textPrimary,
          }}
        >
          Swap
        </span>
        <div className="flex items-center gap-2.5">
          <div
            className="flex items-center gap-1.5 px-2.5 h-7"
            style={{ backgroundColor: color.surface2 }}
          >
            <Circle size={7} color={color.success} weight="fill" />
            <span
              style={{
                fontFamily: typography.caption.family,
                fontSize: "11px",
                letterSpacing: "0.02em",
                color: color.textSecondary,
                whiteSpace: "nowrap",
              }}
            >
              Stellar Testnet
            </span>
          </div>
          <button
            onClick={() => setShowSettings(v => !v)}
            className="flex items-center justify-center w-7 h-7 hover:opacity-100 opacity-70 transition-opacity"
            style={{ cursor: "pointer" }}
          >
            <GearSix
              size={14}
              color={showSettings ? color.textPrimary : color.textMuted}
              weight="regular"
            />
          </button>
        </div>
      </div>

      {/* ── Settings (when open) ─────────────────────────────────── */}
      {showSettings && (
        <div className="mt-px">
          <SettingsPanel
            slippage={slippage} setSlippage={setSlippage}
            deadline={deadline} setDeadline={setDeadline}
            onClose={() => setShowSettings(false)}
          />
        </div>
      )}

      {tokens.length > 0 ? (
        <>
          {/* ── You pay ────────────────────────────────────────── */}
          <div className="mt-px relative">
            <TokenBox
              mode="in"
              token={tokensWithBalance[tokenIn] ?? { symbol: "…", address: "0x", color: "#888", balance: 0 }}
              tokenIdx={tokenIn} otherIdx={tokenOut} tokens={tokensWithBalance}
              value={amountIn} onChange={setAmountIn} onTokenSelect={handleInSelect}
              isConnected={isConnected}
            />
          </div>

          {/* ── Flip button (overlapping divider) ─────────────────── */}
          <div className="relative" style={{ height: 0 }}>
            <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-10">
              <button
                onClick={flip}
                className="flex items-center justify-center w-9 h-9 hover:bg-(--color-surface-3) transition-colors"
                style={{
                  backgroundColor: color.surface2,
                  outline: `2px solid ${color.bg}`,
                  cursor: "pointer",
                  borderRadius: 2,
                }}
              >
                <ArrowDown size={14} color={color.textPrimary} weight="regular" />
              </button>
            </div>
          </div>

          {/* ── You receive ────────────────────────────────────── */}
          <div className="mt-px">
            <TokenBox
              mode="out"
              token={tokensWithBalance[tokenOut] ?? { symbol: "…", address: "0x", color: "#888", balance: 0 }}
              tokenIdx={tokenOut} otherIdx={tokenIn} tokens={tokensWithBalance}
              value={amountOutStr} onTokenSelect={handleOutSelect}
              isConnected={isConnected}
            />
          </div>

          {/* ── Info row ───────────────────────────────────────── */}
          {numIn > 0 && amountOut > 0 && (
            <div className="mt-px">
              <SwapInfoPanel
                tokenIn={tokenIn} tokenOut={tokenOut} tokens={tokens}
                numIn={numIn} amountOut={amountOut} slippage={slippage} fee={fee}
              />
            </div>
          )}
        </>
      ) : (
        <div
          className="py-12 text-center mt-px"
          style={{ ...body("p3", color.textMuted), backgroundColor: color.surface1 }}
        >
          Loading pool…
        </div>
      )}

      {/* ── Action button ─────────────────────────────────────── */}
      <button
        disabled={btnDisabled}
        onClick={handleBtn}
        className="w-full flex items-center justify-center h-12 mt-3 hover:opacity-90 transition-opacity"
        style={{
          backgroundColor: isInsufficient
            ? color.surface1
            : numIn <= 0 && isConnected
            ? color.surface1
            : color.textPrimary,
          color: isInsufficient
            ? color.error
            : numIn <= 0 && isConnected
            ? color.textMuted
            : color.bg,
          fontFamily: typography.p1.family,
          fontSize: typography.p1.size,
          fontWeight: 500,
          letterSpacing: "-0.01em",
          cursor: btnDisabled ? "not-allowed" : "pointer",
        }}
      >
        {btnLabel}
      </button>

      {/* ── Result modal ─────────────────────────────────────── */}
      {swapResult && (() => {
        const successColor = swapResult.success ? color.success : color.error;
        const halves       = swapResult.msg.replace("Swapped ", "").split(" → ");
        const [inAmt = "", inSym = ""]   = (halves[0] ?? "").split(" ");
        const [outAmt = "", outSym = ""] = (halves[1] ?? "").split(" ");

        return (
          <>
            <div
              onClick={() => setSwapResult(null)}
              style={{
                position: "fixed", inset: 0, zIndex: 9998,
                backgroundColor: "rgba(0,0,0,0.85)",
                backdropFilter: "blur(2px)",
              }}
            />
            <div
              style={{
                position: "fixed",
                top: "50%", left: "50%",
                transform: "translate(-50%, -50%)",
                zIndex: 9999,
                width: "min(440px, calc(100vw - 24px))",
                backgroundColor: color.bg,
              }}
              className="flex flex-col gap-px"
            >
              {/* Status row */}
              <div
                className="flex items-center justify-between px-5 py-4"
                style={{ backgroundColor: color.surface1 }}
              >
                <StatusPill
                  healthy={swapResult.success}
                  label={swapResult.success ? "Confirmed" : "Failed"}
                />
                <button
                  onClick={() => setSwapResult(null)}
                  className="hover:opacity-100 opacity-70 transition-opacity"
                >
                  <X size={14} color={color.textMuted} weight="regular" />
                </button>
              </div>

              {/* Body */}
              {halves.length === 2 ? (
                <div
                  className="px-6 py-7 flex items-center gap-4"
                  style={{ backgroundColor: color.surface1 }}
                >
                  <div className="flex-1">
                    <div style={{ ...LBL, color: color.textMuted, marginBottom: 10 }}>From</div>
                    <div className="flex items-center gap-3">
                      <TokenIcon symbol={inSym} size={36} />
                      <div>
                        <div
                          style={{
                            fontFamily: typography.h2.family,
                            fontSize: "26px",
                            fontWeight: 500,
                            letterSpacing: "-0.03em",
                            color: color.textPrimary,
                            lineHeight: 1,
                            fontVariantNumeric: "tabular-nums",
                          }}
                        >
                          {inAmt}
                        </div>
                        <div style={{ ...body("caption", color.textMuted), marginTop: 4 }}>
                          {inSym}
                        </div>
                      </div>
                    </div>
                  </div>
                  <ArrowDown size={18} color={color.textMuted} weight="regular" style={{ transform: "rotate(-90deg)" }} />
                  <div className="flex-1 text-right">
                    <div style={{ ...LBL, color: color.textMuted, marginBottom: 10 }}>To</div>
                    <div className="flex items-center gap-3 justify-end">
                      <div>
                        <div
                          style={{
                            fontFamily: typography.h2.family,
                            fontSize: "26px",
                            fontWeight: 500,
                            letterSpacing: "-0.03em",
                            color: successColor,
                            lineHeight: 1,
                            fontVariantNumeric: "tabular-nums",
                          }}
                        >
                          {outAmt}
                        </div>
                        <div style={{ ...body("caption", color.textMuted), marginTop: 4 }}>
                          {outSym}
                        </div>
                      </div>
                      <TokenIcon symbol={outSym} size={36} />
                    </div>
                  </div>
                </div>
              ) : (
                <div
                  className="px-5 py-5"
                  style={{ backgroundColor: color.surface1 }}
                >
                  <p style={{ ...body("p3", color.textSecondary), lineHeight: 1.6 }}>
                    {swapResult.msg}
                  </p>
                </div>
              )}

              {/* Footer */}
              <div
                className="flex items-center justify-between gap-3 px-5 py-3"
                style={{ backgroundColor: color.surface1 }}
              >
                {swapResult.hash ? (
                  <a
                    href={STELLAR.explorerTx(swapResult.hash)}
                    target="_blank"
                    rel="noreferrer"
                    className="inline-flex items-center gap-1.5 hover:opacity-100 opacity-70 transition-opacity"
                    style={body("caption", color.textMuted)}
                  >
                    {swapResult.hash.slice(0, 12)}…{swapResult.hash.slice(-6)}
                    <ArrowSquareOut size={11} weight="regular" />
                  </a>
                ) : <span />}
                <button
                  onClick={() => setSwapResult(null)}
                  className="hover:opacity-100 opacity-70 transition-opacity"
                  style={{
                    ...LBL,
                    color: color.textPrimary,
                    cursor: "pointer",
                  }}
                >
                  Dismiss
                </button>
              </div>
            </div>
          </>
        );
      })()}
    </div>
  );
}
