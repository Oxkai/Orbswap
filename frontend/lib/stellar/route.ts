// Routing layer — how a swap finds its way from one token to another.
//
// The live pools form a graph. An N-token pool (SuperElliptical) is a *clique*:
// it serves any pair among its tokens directly via `pool.swap(token_in, token_out)`
// (Curve/Balancer-style). A 2-token pool (Circular tick) is a single edge. To go
// A → B we BFS this graph for the shortest pool path; a direct pair is one hop, and
// disjoint tokens (i.e. a pair sharing no pool) simply have no route.
//
// Single-hop routes execute straight on the pool. Multi-hop routes chain 2-token
// pools through `orbswap-router` (its `other_token` is 2-token only, which is why
// the N-token pool is a clique node here, not something the router walks).

import { STELLAR, TOKENS } from "./config";
import { TICK_POOL } from "./ticks";
import { quote, quoteViaSwapSim, swap, routerSwapExactIn } from "./pool";

export type PoolKind = "SuperElliptical" | "Circular";
export interface PoolInfo {
  id: string;
  kind: PoolKind;
  tokens: string[]; // token contract addresses
}

/** The live pool graph. */
export const POOLS: PoolInfo[] = [
  { id: STELLAR.pool, kind: "SuperElliptical", tokens: TOKENS.map((t) => t.address) },
  { id: TICK_POOL.id, kind: "Circular", tokens: TICK_POOL.tokens.map((t) => t.address) },
];

export interface Hop {
  pool: string;
  kind: PoolKind;
  tokenIn: string;
  tokenOut: string;
}
export type Route = Hop[];

const MAX_HOPS = 3;

/**
 * Shortest pool path from `tokenIn` to `tokenOut`, or null if none exists.
 * BFS over the pool graph — a token connects to every other token in any pool it
 * belongs to (N-token pools are cliques). Returns one hop per pool traversed.
 */
export function findRoute(tokenIn: string, tokenOut: string): Route | null {
  if (tokenIn === tokenOut) return null;
  const queue: { token: string; path: Hop[] }[] = [{ token: tokenIn, path: [] }];
  const visited = new Set<string>([tokenIn]);

  while (queue.length) {
    const { token, path } = queue.shift() as { token: string; path: Hop[] };
    for (const pool of POOLS) {
      if (!pool.tokens.includes(token)) continue;
      for (const next of pool.tokens) {
        if (next === token) continue;
        const hop: Hop = { pool: pool.id, kind: pool.kind, tokenIn: token, tokenOut: next };
        if (next === tokenOut) return [...path, hop];
        if (!visited.has(next) && path.length + 1 < MAX_HOPS) {
          visited.add(next);
          queue.push({ token: next, path: [...path, hop] });
        }
      }
    }
  }
  return null;
}

/**
 * Chain read-only quotes along a route. N-token pools answer with the cheap `quote`
 * view; tick (Circular) pools have no fungible-share quote, so they're priced by
 * simulating the swap (exact, either way).
 */
export async function quoteRoute(route: Route, amountIn: bigint): Promise<bigint> {
  let amt = amountIn;
  for (const hop of route) {
    amt =
      hop.kind === "Circular"
        ? await quoteViaSwapSim(hop.pool, hop.tokenIn, amt, hop.tokenOut)
        : await quote(hop.pool, hop.tokenIn, amt, hop.tokenOut);
  }
  return amt;
}

/** Execute a route: single hop straight on the pool, multi-hop through the router. */
export async function swapRoute(opts: {
  route: Route;
  from: string;
  amountIn: bigint;
  minOut: bigint;
  deadline: bigint;
  sign: (xdr: string) => Promise<string>;
}): Promise<string> {
  const { route, from, amountIn, minOut, deadline, sign } = opts;
  if (route.length === 1) {
    const h = route[0];
    return swap({ pool: h.pool, from, tokenIn: h.tokenIn, amountIn, tokenOut: h.tokenOut, minOut, deadline, sign });
  }
  return routerSwapExactIn({
    from,
    pools: route.map((h) => h.pool),
    tokenIn: route[0].tokenIn,
    amountIn,
    minOut,
    deadline,
    sign,
  });
}
