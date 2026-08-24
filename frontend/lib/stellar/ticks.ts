// Reads for the Circular concentrated-liquidity (tick) pool. Live mainnet pool from
// contracts/deployments/mainnet_stable.json.

import { rpc, Contract, Account, TransactionBuilder, BASE_FEE, scValToNative } from "@stellar/stellar-sdk";
import { STELLAR, fromNative } from "./config";

/** The deployed 2-token Circular tick pool. Its first leg is also in the
 *  SuperElliptical pool (config.ts), which connects the two pools for multi-hop
 *  routing. */
export const TICK_POOL = {
  id: "CB5UC4V3XHBE2RKGLF3TEUCNECADIGMLG7ZV5VLK76DHNVUIHUEN5O27",
  fee_bps: 30,
  tokens: [
    // USDC is the shared leg — it is also in the SuperElliptical pool (config.ts),
    // which is what lets the router hop between the two.
    { symbol: "USDC", name: "USD Coin", address: "CCTTKWIGUWJM7ZRBXCFP7AJKZPOQ2YYISBTA4ZIBMTYQBBLKO3FZ7OX6", color: "#2775CA" },
    // FDUSD is the fifth asset and lives only here. Both legs are 1:1, so the
    // concentrated band sits on the peg — seeded full-range plus [40,50].
    { symbol: "FDUSD", name: "First Digital USD", address: "CB2ZT6BM6DFYFP7CY7FRSD2YL76Y6XTBMIGEEDEIH5EVWB4266ZWQM6L", color: "#C8A46A" },
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
  reserves: number[]; // [USDC, FDUSD], display
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
