// Reads for a RATE-AWARE Orbswap pool — one whose balanced point tracks a SEP-40
// FX rate instead of sitting at 1:1. Addresses come from
// contracts/deployments/<network>_rates.json (written by scripts/deploy_rate_pool.sh).
//
// Everything here is a read-only simulation; nothing is signed.

import {
  rpc,
  Contract,
  Account,
  Address,
  TransactionBuilder,
  BASE_FEE,
  nativeToScVal,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";
import { STELLAR } from "./config";

/** The deployed rate-aware pool. Null until one is deployed and pasted in. */
export const RATE_POOL: {
  id: string;
  feed: string;
  feeBps: number;
  /** Index 0 is the numeraire (rate pinned at 1.0); index 1 is the quote leg. */
  tokens: { symbol: string; address: string; decimals: number; color: string }[];
} | null = null;

const WAD = 10n ** 18n;

async function read(poolId: string, method: string, ...args: xdr.ScVal[]): Promise<unknown> {
  const s = new rpc.Server(STELLAR.rpcUrl);
  const src = new Account(STELLAR.readAccount, "0");
  const tx = new TransactionBuilder(src, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(new Contract(poolId).call(method, ...args))
    .setTimeout(30)
    .build();
  const sim = await s.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);
  const rv = "result" in sim ? sim.result?.retval : undefined;
  if (!rv) throw new Error(`no return value from ${method}`);
  return scValToNative(rv);
}

/** Why the pool is refusing to trade, or `null` when it is open. */
export type PoolHalt = "breaker" | "stale" | "repeg" | null;

export interface RateStatus {
  /** Quote-leg rate in WAD (1e18 = parity). */
  rateWad: bigint;
  /** Human-readable rate: how much numeraire one quote unit is worth. */
  rate: number;
  /** Ledger timestamp of the last accepted rate. */
  lastUpdate: number;
  fresh: boolean;
  breakerTripped: boolean;
  /** A repeg is pending, so trading is closed until `re_anchor` lands. */
  needsReAnchor: boolean;
  halt: PoolHalt;
}

/**
 * Live rate + halt state. The three halt conditions are ordered by severity, the
 * same way the contract's `require_tradeable` surfaces them.
 */
export async function rateStatus(poolId: string): Promise<RateStatus> {
  const [status, needsReAnchor] = await Promise.all([
    read(poolId, "rate_status") as Promise<[bigint, bigint, boolean, boolean]>,
    read(poolId, "needs_reanchor") as Promise<boolean>,
  ]);
  const [rateWad, lastUpdate, fresh, breakerTripped] = status;

  const halt: PoolHalt = breakerTripped
    ? "breaker"
    : !fresh
      ? "stale"
      : needsReAnchor
        ? "repeg"
        : null;

  return {
    rateWad,
    rate: Number(rateWad) / 1e18,
    lastUpdate: Number(lastUpdate),
    fresh,
    breakerTripped,
    needsReAnchor,
    halt,
  };
}

/** Cached WAD rate for one token. Always 1e18 on a parity pool. */
export async function getRate(poolId: string, token: string): Promise<bigint> {
  return (await read(poolId, "get_rate", new Address(token).toScVal())) as bigint;
}

/** Whether the pool currently sits on its invariant (monitoring only). */
export async function isOnCurve(poolId: string): Promise<boolean> {
  return (await read(poolId, "is_on_curve")) as boolean;
}

/** Signed invariant residual, WAD: 0 on-curve, negative inside, positive outside. */
export async function curveDrift(poolId: string): Promise<bigint> {
  return (await read(poolId, "curve_drift")) as bigint;
}

/** `(operatorModeEnabled, addressIsAllowedToProvideLiquidity)`. */
export async function operatorStatus(
  poolId: string,
  who: string
): Promise<{ enabled: boolean; allowed: boolean }> {
  const [enabled, allowed] = (await read(
    poolId,
    "operator_status",
    new Address(who).toScVal()
  )) as [boolean, boolean];
  return { enabled, allowed };
}

/**
 * The pool's spread against the oracle mid, in basis points.
 *
 * `amountIn` of the numeraire should fetch `amountIn / rate` of the quote leg at
 * a frictionless oracle rate; whatever the pool actually quotes is worse by the
 * fee plus slippage. This is the number the anchor comparison is built on — the
 * same calculation `scripts/keeper.sh` writes to its spread log.
 */
export async function spreadBps(
  poolId: string,
  tokenIn: string,
  tokenOut: string,
  amountIn: bigint
): Promise<{ poolOut: bigint; oracleOut: bigint; spreadBps: number } | null> {
  const [rateWad, poolOut] = await Promise.all([
    getRate(poolId, tokenOut),
    read(
      poolId,
      "quote",
      new Address(tokenIn).toScVal(),
      nativeToScVal(amountIn, { type: "i128" }),
      new Address(tokenOut).toScVal()
    ) as Promise<bigint>,
  ]);
  if (rateWad <= 0n) return null;
  const oracleOut = (amountIn * WAD) / rateWad;
  if (oracleOut <= 0n) return null;
  return {
    poolOut,
    oracleOut,
    spreadBps: Number(((oracleOut - poolOut) * 10_000n) / oracleOut),
  };
}

/** One-line explanation of a halt, for the UI banner. */
export function haltMessage(halt: PoolHalt): string | null {
  switch (halt) {
    case "breaker":
      return "Trading halted — the oracle circuit breaker is latched. Withdrawals remain open.";
    case "stale":
      return "Trading paused — the FX rate is stale. It resumes automatically on the next feed update.";
    case "repeg":
      return "Trading paused — repegging to a new FX rate. Withdrawals remain open.";
    default:
      return null;
  }
}
