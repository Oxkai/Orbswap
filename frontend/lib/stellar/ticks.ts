// Reads for the Circular concentrated-liquidity (tick) pool. Live testnet pool from
// contracts/deployments/testnet_ticks.json.

import { rpc, Contract, Account, TransactionBuilder, BASE_FEE, scValToNative } from "@stellar/stellar-sdk";
import { STELLAR, fromNative } from "./config";

/** The deployed 2-token Circular tick pool. */
export const TICK_POOL = {
  id: "CCAZ3IADGGP4K5NRWMM5RCA63J76SHDITSY6HJLCUEXGAKUFAMEWC2NL",
  fee_bps: 30,
  tokens: [
    { symbol: "CIRA", address: "CAL5IWELZEBZ3V7JT5W3CS2RABEWHBSJCJ6QBSZYGSZ33SJ3OTS3EXRV", color: "#4F9DFF" },
    { symbol: "CIRB", address: "CCPYE62VMOIQIOMANUHLD5YWZCPJ5P7G6XDPSAIRP4QZNJE5AUDIC5NG", color: "#35C08E" },
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
  reserves: number[]; // [CIRA, CIRB], display
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
