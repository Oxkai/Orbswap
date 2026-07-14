# Orbswap — Frontend

The web app for **Orbswap**, a concentrated N-dimensional stableswap (CCMM circle /
CSEMM superellipse) on **Stellar / Soroban**. Swap, provide liquidity, and inspect the
live testnet pools — wired directly to the on-chain contracts with the
[`@stellar/stellar-sdk`](https://github.com/stellar/js-stellar-sdk) and signed in-browser
with [Freighter](https://www.freighter.app/).

The contracts and their deployed addresses live in [`../contracts`](../contracts) — see
[`../contracts/README.md`](../contracts/README.md).

## Stack

| Layer | Choice |
|---|---|
| Framework | Next.js 16 (App Router, Turbopack) |
| UI | React 19, inline-styled design tokens (`constants/`) |
| Chain | `@stellar/stellar-sdk` 16 (Soroban RPC) |
| Wallet | `@stellar/freighter-api` (source-account auth) |
| Fonts | KMR Apparat (self-hosted, `public/TTF`), Geist Mono, IBM Plex Mono (ASCII canvas) |

## Run

```bash
npm install
npm run dev          # dev server on http://localhost:3000
npm run build        # production build
npm start            # serve the production build (recommended on low-RAM machines)
```

No env vars are required — the network (testnet) and all contract addresses are
committed in [`lib/stellar/config.ts`](lib/stellar/config.ts). To use the app you need the
Freighter extension set to **Testnet**.

## How the on-chain wiring works

Everything the app does on-chain flows through four small files in
[`lib/stellar/`](lib/stellar/):

| File | Responsibility |
|---|---|
| [`config.ts`](lib/stellar/config.ts) | Network passphrase, RPC URL, and the live **contract + token addresses** (SuperElliptical pool, factory, router, tokens). 7-decimal `toNative`/`fromNative` helpers. |
| [`wallet.ts`](lib/stellar/wallet.ts) | `useStellarWallet()` — Freighter connect + `sign()`. No global provider needed. |
| [`pool.ts`](lib/stellar/pool.ts) | Read + write calls: `quote`, `swap`, `deposit`, `addLiquidity`, `getReservesOf`, `totalSharesOf`, `balanceOf`, `getRecentEvents`. Writes follow **build → simulate → assemble → sign (Freighter) → submit → poll**; a single source-account signature also authorizes the token pulls. |
| [`ticks.ts`](lib/stellar/ticks.ts) | The 2-token **Circular tick pool**: its address/tokens plus `getTickState()` (current tick, active liquidity, reserves). |

React data hooks in [`lib/hooks/`](lib/hooks/) wrap these reads into the shared `Pool` /
`Position` shapes from [`lib/mock/data.ts`](lib/mock/data.ts) (types + formatters, despite
the legacy name):
[`usePool`](lib/hooks/usePool.ts) (SuperElliptical pool),
[`useTickPool`](lib/hooks/useTickPool.ts) (Circular tick pool),
[`usePositions`](lib/hooks/usePositions.ts),
[`useTransactions`](lib/hooks/useTransactions.ts).

## Routes

| Route | File | What |
|---|---|---|
| `/` | [`app/page.tsx`](app/page.tsx) | Landing page — sections in [`components/home/`](components/home/). |
| `/app/swap` | [`app/app/swap/page.tsx`](app/app/swap/page.tsx) | Swap widget ([`SwapWidget`](components/app/swap/SwapWidget.tsx)) over an ASCII-field background. |
| `/app/pools` | [`app/app/pools/page.tsx`](app/app/pools/page.tsx) | Both live pools as [`PoolCard`](components/app/pools/PoolCard.tsx)s. |
| `/app/pool/[address]` | [`app/app/pool/[address]/page.tsx`](app/app/pool/[address]/page.tsx) | Pool detail; **tick UI only shows for the Circular pool** (`address === TICK_POOL.id`). |
| `/app/pool/[address]/add` | [`app/app/pool/[address]/add/page.tsx`](app/app/pool/[address]/add/page.tsx) | Add-liquidity **wizard that branches by pool type** (see below). |
| `/app/positions` | [`app/app/positions/page.tsx`](app/app/positions/page.tsx) | LP positions. |
| `/app/transactions` | [`app/app/transactions/page.tsx`](app/app/transactions/page.tsx) | Recent on-chain pool events. |

## Add-liquidity, per pool type

The add page reads the pool type from the address and renders the matching flow:

- **SuperElliptical** (multi-asset, `address !== TICK_POOL.id`) → **balanced `deposit`**.
  Enter one token; the rest auto-scale to the pool's current reserve ratio. `Amounts → Review`.
- **Circular** (2-token tick pool) → **concentrated `add_liquidity`** over an angle range
  on the circular arc (45° = the $1 peg). `Range → Amounts → Review`.

## Design system

Colors, theme variables, and typography are centralized in
[`constants/`](constants/) (`colors.ts`, `typography.ts`, `index.ts`) and consumed as
inline styles. Landing-page sections live in [`components/home/`](components/home/); the
app shell (nav, footer, widgets, modals) in [`components/app/`](components/app/).

## Known gaps

- The **app header wallet control** ([`components/app/layout/AppNav.tsx`](components/app/layout/AppNav.tsx))
  still uses the legacy EVM `wagmi` path and is not the Freighter flow the pages actually use;
  the real wallet connect lives inside the swap / add-liquidity flows. Slated for replacement.
- There is no indexer, so 24h volume / fee figures are not shown.
