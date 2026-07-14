// Soroban read/write helpers for the Orbswap pool: `quote` (read-only simulation),
// `swap` (sign + submit), and token `balance`. Amounts are 7-decimal i128 (native).

import {
  rpc,
  Contract,
  Address,
  Account,
  Transaction,
  TransactionBuilder,
  BASE_FEE,
  nativeToScVal,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";
import { STELLAR } from "./config";

const server = () => new rpc.Server(STELLAR.rpcUrl);
const addrScVal = (a: string) => new Address(a).toScVal();
const i128 = (v: bigint) => nativeToScVal(v, { type: "i128" });
const u32 = (v: number) => nativeToScVal(v, { type: "u32" });
const u64 = (v: bigint) => nativeToScVal(v, { type: "u64" });
const i128Vec = (vs: bigint[]) => xdr.ScVal.scvVec(vs.map(i128));

function retvalOf(sim: rpc.Api.SimulateTransactionResponse): xdr.ScVal {
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);
  const rv = "result" in sim ? sim.result?.retval : undefined;
  if (!rv) throw new Error("no return value from simulation");
  return rv;
}

/** Read-only: expected output of an exact-in swap on `poolId` (native i128). */
export async function quote(
  poolId: string,
  tokenIn: string,
  amountIn: bigint,
  tokenOut: string
): Promise<bigint> {
  const s = server();
  const src = new Account(STELLAR.readAccount, "0");
  const contract = new Contract(poolId);
  const tx = new TransactionBuilder(src, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(
      contract.call("quote", addrScVal(tokenIn), i128(amountIn), addrScVal(tokenOut))
    )
    .setTimeout(30)
    .build();
  const sim = await s.simulateTransaction(tx);
  return scValToNative(retvalOf(sim)) as bigint;
}

/** Generic read-only call on the pool that returns a scalar/vector via simulation. */
async function poolRead(method: string, ...args: xdr.ScVal[]): Promise<unknown> {
  const s = server();
  const src = new Account(STELLAR.readAccount, "0");
  const contract = new Contract(STELLAR.pool);
  const tx = new TransactionBuilder(src, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(contract.call(method, ...args))
    .setTimeout(30)
    .build();
  const sim = await s.simulateTransaction(tx);
  return scValToNative(retvalOf(sim));
}

/** Pool reserves per token, native i128 (parallel to the token list). */
export async function getReserves(): Promise<bigint[]> {
  return (await poolRead("get_reserves")) as bigint[];
}

/** Total LP shares outstanding (native i128, WAD-scale). */
export async function totalShares(): Promise<bigint> {
  return (await poolRead("total_shares")) as bigint;
}

/** LP shares held by `who`. */
export async function sharesOf(who: string): Promise<bigint> {
  return (await poolRead("shares_of", addrScVal(who))) as bigint;
}

/**
 * Quote by simulating a `swap` read-only. Works for **any** pool type — including
 * tick (Circular) pools, whose `quote` view is fungible-share only (`s == 0` in tick
 * mode). Simulation runs the real curve math and returns the exact output without
 * submitting; a funded reference account stands in as the swapper.
 */
export async function quoteViaSwapSim(
  poolId: string,
  tokenIn: string,
  amountIn: bigint,
  tokenOut: string
): Promise<bigint> {
  const s = server();
  const src = await s.getAccount(STELLAR.readAccount);
  const built = new TransactionBuilder(src, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(
      new Contract(poolId).call(
        "swap",
        addrScVal(STELLAR.readAccount),
        addrScVal(tokenIn),
        i128(amountIn),
        addrScVal(tokenOut),
        i128(0n),
        u64(18446744073709551615n)
      )
    )
    .setTimeout(30)
    .build();
  const sim = await s.simulateTransaction(built);
  return scValToNative(retvalOf(sim)) as bigint;
}

/** Generic read-only call against an arbitrary pool id (SuperElliptical or tick). */
async function poolReadOn(poolId: string, method: string, ...args: xdr.ScVal[]): Promise<unknown> {
  const s = server();
  const src = new Account(STELLAR.readAccount, "0");
  const tx = new TransactionBuilder(src, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(new Contract(poolId).call(method, ...args))
    .setTimeout(30)
    .build();
  const sim = await s.simulateTransaction(tx);
  return scValToNative(retvalOf(sim));
}

/** Reserves of any pool, native i128 (parallel to its token list). */
export async function getReservesOf(poolId: string): Promise<bigint[]> {
  return (await poolReadOn(poolId, "get_reserves")) as bigint[];
}

/** Total LP shares of any pool (native i128, WAD-scale). */
export async function totalSharesOf(poolId: string): Promise<bigint> {
  return (await poolReadOn(poolId, "total_shares")) as bigint;
}

/** Read-only: a token's balance for an account (native i128). */
export async function balanceOf(token: string, who: string): Promise<bigint> {
  const s = server();
  const src = new Account(STELLAR.readAccount, "0");
  const contract = new Contract(token);
  const tx = new TransactionBuilder(src, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(contract.call("balance", addrScVal(who)))
    .setTimeout(30)
    .build();
  const sim = await s.simulateTransaction(tx);
  return scValToNative(retvalOf(sim)) as bigint;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Common write tail: simulate → assemble → sign (Freighter) → submit → poll. */
async function signAndSubmit(
  s: rpc.Server,
  built: Transaction,
  sign: (xdr: string) => Promise<string>
): Promise<string> {
  const sim = await s.simulateTransaction(built);
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);
  const prepared = rpc.assembleTransaction(built, sim).build();
  const signedXdr = await sign(prepared.toXDR());
  const signedTx = TransactionBuilder.fromXDR(signedXdr, STELLAR.networkPassphrase);
  const sent = await s.sendTransaction(signedTx);
  if (sent.status === "ERROR") throw new Error("Transaction submission failed");
  let get = await s.getTransaction(sent.hash);
  for (let i = 0; i < 30 && get.status === "NOT_FOUND"; i++) {
    await sleep(1500);
    get = await s.getTransaction(sent.hash);
  }
  if (get.status !== "SUCCESS") throw new Error(`Transaction ${get.status.toLowerCase()}`);
  return sent.hash;
}

/**
 * SuperElliptical (fungible-share) deposit: pull `amounts[i]` of each token and
 * mint LP shares. The first deposit must be balanced; later ones proportional to
 * reserves. Source-account auth covers the token pulls. Returns the tx hash.
 */
export async function deposit(opts: {
  poolId: string;
  from: string;
  amounts: bigint[];
  minShares: bigint;
  deadline: bigint;
  sign: (xdr: string) => Promise<string>;
}): Promise<string> {
  const s = server();
  const source = await s.getAccount(opts.from);
  const built = new TransactionBuilder(source, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(
      new Contract(opts.poolId).call(
        "deposit",
        addrScVal(opts.from),
        i128Vec(opts.amounts),
        i128(opts.minShares),
        u64(opts.deadline)
      )
    )
    .setTimeout(120)
    .build();
  return signAndSubmit(s, built, opts.sign);
}

/**
 * Multi-hop swap through the router: chains 2-token pools, computing the output
 * token at each hop. Used for routes longer than one hop; single-hop swaps go
 * straight to the pool. Returns the tx hash.
 */
export async function routerSwapExactIn(opts: {
  from: string;
  pools: string[];
  tokenIn: string;
  amountIn: bigint;
  minOut: bigint;
  deadline: bigint;
  sign: (xdr: string) => Promise<string>;
}): Promise<string> {
  const s = server();
  const source = await s.getAccount(opts.from);
  const poolsVec = xdr.ScVal.scvVec(opts.pools.map(addrScVal));
  const built = new TransactionBuilder(source, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(
      new Contract(STELLAR.router).call(
        "swap_exact_in",
        addrScVal(opts.from),
        poolsVec,
        addrScVal(opts.tokenIn),
        i128(opts.amountIn),
        i128(opts.minOut),
        u64(opts.deadline)
      )
    )
    .setTimeout(120)
    .build();
  return signAndSubmit(s, built, opts.sign);
}

/**
 * Circular tick pool concentrated deposit: add liquidity over the angle range
 * `[lower, upper]` (integer degrees in [0,90], 45 = the peg), pulling at most
 * `amounts = [xMax, yMax]`. Returns the tx hash.
 */
export async function addLiquidity(opts: {
  poolId: string;
  from: string;
  amounts: [bigint, bigint];
  lower: number;
  upper: number;
  minLiquidity: bigint;
  deadline: bigint;
  sign: (xdr: string) => Promise<string>;
}): Promise<string> {
  const s = server();
  const source = await s.getAccount(opts.from);
  const built = new TransactionBuilder(source, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(
      new Contract(opts.poolId).call(
        "add_liquidity",
        addrScVal(opts.from),
        i128Vec(opts.amounts),
        u32(opts.lower),
        u32(opts.upper),
        i128(opts.minLiquidity),
        u64(opts.deadline)
      )
    )
    .setTimeout(120)
    .build();
  return signAndSubmit(s, built, opts.sign);
}

/**
 * Build → simulate → sign (Freighter) → submit an exact-in swap. `from` is the
 * connected wallet and the tx source, so its single signature also authorizes the
 * token pull (Soroban source-account auth). Returns the tx hash.
 */
export async function swap(opts: {
  pool: string;
  from: string;
  tokenIn: string;
  amountIn: bigint;
  tokenOut: string;
  minOut: bigint;
  deadline: bigint;
  sign: (xdr: string) => Promise<string>;
}): Promise<string> {
  const s = server();
  const source = await s.getAccount(opts.from);
  const contract = new Contract(opts.pool);

  const built = new TransactionBuilder(source, {
    fee: BASE_FEE,
    networkPassphrase: STELLAR.networkPassphrase,
  })
    .addOperation(
      contract.call(
        "swap",
        addrScVal(opts.from),
        addrScVal(opts.tokenIn),
        i128(opts.amountIn),
        addrScVal(opts.tokenOut),
        i128(opts.minOut),
        nativeToScVal(opts.deadline, { type: "u64" })
      )
    )
    .setTimeout(120)
    .build();

  const sim = await s.simulateTransaction(built);
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);

  // Attach soroban resource fees + footprint + auth from the simulation.
  const prepared = rpc.assembleTransaction(built, sim).build();

  const signedXdr = await opts.sign(prepared.toXDR());
  const signedTx = TransactionBuilder.fromXDR(signedXdr, STELLAR.networkPassphrase);

  const sent = await s.sendTransaction(signedTx);
  if (sent.status === "ERROR") {
    throw new Error("Transaction submission failed");
  }

  // Poll for the result.
  let get = await s.getTransaction(sent.hash);
  for (let i = 0; i < 30 && get.status === "NOT_FOUND"; i++) {
    await sleep(1500);
    get = await s.getTransaction(sent.hash);
  }
  if (get.status !== "SUCCESS") {
    throw new Error(`Transaction ${get.status.toLowerCase()}`);
  }
  return sent.hash;
}

export interface PoolEvent {
  type: "Swap" | "Deposit" | "Withdraw" | "Other";
  from?: string;
  pool?: string;
  ledger: number;
  ts: number; // unix seconds
  txHash: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  data: any;
}

/**
 * Recent contract events (newest first) across one or more pools. The Soroban RPC
 * scans at most ~10k ledgers per `getEvents` call, so we page in sub-cap chunks up
 * to the latest ledger (a single large `startLedger` would scan an old chunk and
 * miss recent activity). Testnet retention bounds how far back this can reach.
 */
export async function getRecentEvents(
  pools: string[] = [STELLAR.pool],
  maxLedgers = 18000
): Promise<PoolEvent[]> {
  const s = server();
  const latest = (await s.getLatestLedger()).sequence;
  const CAP = 9000; // stay safely under the RPC's ~10k-ledger scan cap
  const from = Math.max(1, latest - maxLedgers);
  const out: PoolEvent[] = [];
  const seen = new Set<string>();

  for (const pool of pools) {
    for (let start = from; start <= latest; start += CAP) {
      let res;
      try {
        res = await s.getEvents({
          startLedger: start,
          filters: [{ type: "contract", contractIds: [pool] }],
          limit: 100,
        });
      } catch {
        continue; // out-of-retention or transient — skip this chunk
      }
      for (const ev of res.events) {
        const key = ev.id ?? `${ev.txHash}:${ev.ledger}`;
        if (seen.has(key)) continue; // chunks overlap slightly; dedupe
        seen.add(key);
        const name = ev.topic?.[0] ? String(scValToNative(ev.topic[0])) : "Other";
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const data: any = scValToNative(ev.value);
        const type: PoolEvent["type"] =
          name === "Swap" || name === "Deposit" || name === "Withdraw" ? name : "Other";
        const ts = ev.ledgerClosedAt ? Math.floor(new Date(ev.ledgerClosedAt).getTime() / 1000) : 0;
        out.push({ type, ledger: ev.ledger, ts, txHash: ev.txHash, data, from: data?.from, pool });
      }
    }
  }
  return out.sort((a, b) => b.ledger - a.ledger);
}
