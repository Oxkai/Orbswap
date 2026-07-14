"use client";

import { useCallback, useEffect, useState } from "react";
import { getRecentEvents } from "@/lib/stellar/pool";
import { TOKENS, STELLAR, fromNative } from "@/lib/stellar/config";
import { TICK_POOL } from "@/lib/stellar/ticks";

export type TxType = "Swap" | "Add" | "Remove" | "Collect";

export interface TxRecord {
  type: TxType;
  hash: string;
  blockNumber: bigint; // ledger sequence
  timestamp: number; // unix seconds
  actor: string;
  amountIn: string;
  amountOut: string;
}

const SYMBOLS = new Map<string, string>([
  ...TOKENS.map((t) => [t.address, t.symbol] as const),
  ...TICK_POOL.tokens.map((t) => [t.address, t.symbol] as const),
]);
const symbolOf = (addr: string) => SYMBOLS.get(addr) ?? addr.slice(0, 4);
const fmt = (v: bigint) => fromNative(v).toFixed(4);

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function sumAmounts(amounts: any): number {
  if (!Array.isArray(amounts)) return 0;
  return amounts.reduce((a: number, b) => a + fromNative(BigInt(b)), 0);
}

/**
 * Recent pool activity from Soroban contract events. Single fetch of the recent
 * ledger window (testnet event retention is short), newest first.
 */
export function useTransactions(_poolTokens: { symbol: string; address: string }[]) {
  const [txs, setTxs] = useState<TxRecord[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setIsLoading(true);
    setError(null);
    try {
      const events = await getRecentEvents([STELLAR.pool, TICK_POOL.id]);
      const records: TxRecord[] = events
        .map((e): TxRecord | null => {
          const base = {
            hash: e.txHash,
            blockNumber: BigInt(e.ledger),
            timestamp: e.ts,
            actor: e.from ?? "",
          };
          if (e.type === "Swap") {
            return {
              ...base,
              type: "Swap",
              amountIn: `${fmt(BigInt(e.data.amount_in))} ${symbolOf(e.data.token_in)}`,
              amountOut: `${fmt(BigInt(e.data.amount_out))} ${symbolOf(e.data.token_out)}`,
            };
          }
          if (e.type === "Deposit") {
            return { ...base, type: "Add", amountIn: `${sumAmounts(e.data.amounts).toFixed(2)} added`, amountOut: "" };
          }
          if (e.type === "Withdraw") {
            return { ...base, type: "Remove", amountIn: `${sumAmounts(e.data.amounts).toFixed(2)} removed`, amountOut: "" };
          }
          return null;
        })
        .filter((r): r is TxRecord => r !== null);
      setTxs(records);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load activity");
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // Single-fetch model: no pagination against the recent-events window.
  return {
    txs,
    isLoading,
    isLoadingMore: false,
    hasMore: false,
    loadMore: () => {},
    error,
    refetch: load,
  };
}
