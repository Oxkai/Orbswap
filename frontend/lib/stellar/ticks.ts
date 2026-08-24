// Reads for the Circular concentrated-liquidity (tick) pool. Live testnet pool from
// contracts/deployments/testnet_ticks.json.

import { rpc, Contract, Account, TransactionBuilder, BASE_FEE, scValToNative } from "@stellar/stellar-sdk";
import { STELLAR, fromNative } from "./config";

/** The deployed 2-token Circular tick pool. USDC is shared with the SuperElliptical
 *  pool (config.ts), which connects the two pools for multi-hop routing. */
export const TICK_POOL = {
  id: "CATAWBZMD337WXYZ3R5CX7KNFCBDLI7TAMDU6XSSPRDBLMZXWIGQ37HC",
  fee_bps: 30,
  tokens: [
    { symbol: "USDC", address: "CBNDCO3DMKFVCSVFPHMYK6KSD6CCKVUMI3TFK6ZJ3BP7NCNLUJBJAB6Z", color: "#2775CA" },
    // IDRT (Indonesian rupiah, kbtrading.org) is a live Stellar asset and gives the
    // oracle pair a real local-currency leg — the corridor shape rates.rs targets.
    { symbol: "IDRT", address: "CCUC4GORGIR4MPKFORGHT37HJRCDERYHN3J34DQ4E5XK37JIL3H5EKYL", color: "#BB4E42" },
  ],
};

async function read(method: string): Promise<unknown> {
  const s = new rpc.Server(STELLAR.rpcUrl);
  const src = new Account(STELLAR.readAccount, "0");
  const tx = new TransactionBuilder(src, { fee: BASE_FEE, networkPassphrase: STELLAR.networkPassphrase })
    .addOperation(new Contract(TICK_POOL.id).call(method))
    .setTimeout(30)
    .build();
  const sim = await s.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);
  const rv = "result" in sim ? sim.result?.retval : undefined;
  if (!rv) throw new Error("no return value");
  return scValToNative(rv);
}

export interface TickState {
  currentTick: number; // 0..90 (45 = balanced $1)
  activeLiquidity: number; // display scale
  reserves: number[]; // [USDC, IDRT], display
}

/** Live tick state: current tick (angle), active liquidity, reserves. */
export async function getTickState(): Promise<TickState> {
  const [tick, liq, reserves] = await Promise.all([
    read("current_tick") as Promise<number>,
    read("active_liquidity") as Promise<bigint>,
    read("get_reserves") as Promise<bigint[]>,
  ]);
  return {
    currentTick: Number(tick),
    activeLiquidity: fromNative(BigInt(liq)),
    reserves: reserves.map(fromNative),
  };
}
