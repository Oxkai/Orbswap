# Mainnet runbook

Everything needed to take Orbswap from a testnet rehearsal to Stellar mainnet.
The deploy sequence is identical on both networks — `scripts/deploy_pipeline.sh`
branches in exactly one place (stage 4: testnet mints its own SACs, mainnet binds
to the real issuers) — so a green testnet run is a genuine rehearsal.

---

## 1. Configure the RPC

The stellar-cli ships `mainnet` with no RPC URL ("Bring Your Own"). Add one:

```bash
stellar network add mainnet \
  --rpc-url https://mainnet.sorobanrpc.com \
  --network-passphrase 'Public Global Stellar Network ; September 2015'

stellar network health --network mainnet     # -> Healthy
```

Endpoints checked, latency and retention measured live:

| Endpoint | Latency | Retention |
|---|---|---|
| `https://mainnet.sorobanrpc.com` | ~660 ms | ~120,959 ledgers |
| `https://soroban-rpc.creit.tech` | ~1000 ms | ~17,279 ledgers |

Both are public and rate-limited. They are fine for a deploy, but put a dedicated
provider behind the frontend before taking real traffic.

## 2. Create a dedicated deployer key

**Do not reuse a testnet key.** Testnet keys are generated as throwaways, live in
the local CLI keystore, and have signed dozens of public transactions. The
deployer also becomes `admin` on both pools.

```bash
stellar keys generate orbswap-mainnet --network mainnet   # or: keys add --secret-key
stellar keys address orbswap-mainnet
```

Then set `DEPLOYER_IDENTITY=orbswap-mainnet` in `.env`.

## 3. Fund the account

| | XLM |
|---|---|
| pool wasm upload (91,916 B) | 117.83 |
| factory wasm upload (12,369 B) | 19.43 |
| router wasm upload (10,819 B) | 16.66 |
| instances, initialize, balance entries, seeding | 1.28 |
| 18 simulation swaps | 0.81 |
| **network fees** | **156.00** |
| base reserve + 5 trustlines (locked, recoverable) | 3.50 |
| **total** | **159.50** |
| **recommended (`REQUIRED_XLM`)** | **220** |

Nearly all of it is the three wasm uploads, and most of *that* is prepaid rent:
mainnet charges code rent on the parsed module footprint (~7.7x the file size)
with a 120-day minimum TTL. Figures come from simulating the real upload
transactions against mainnet RPC, cross-checked against the code-entry rent slope
measured on testnet — the two agree to the stroop.

Trading itself is negligible: **all 18 simulation swaps cost 0.81 XLM total**, and
per-swap cost is flat regardless of trade size or how far the pool sits from 1:1.

### You also need the liquidity

Stage 7 seeds 24M into pool A and 5M into pool B. On testnet the pipeline mints
these; **on mainnet it cannot**. Hold the balances first, or lower `SUPER_SEED`,
`CIRC_FULL` and `CIRC_NARROW` in the script — the XLM figures barely move, since
fees do not scale with deposit size.

### Rent recurs

~156 XLM per 120 days (~468 XLM/year) to keep the three code entries alive. The
first 120 days are prepaid above. Miss a renewal and the entries are archived and
must be restored before the pools work again.

> Mainnet Soroban state is ~1.84 GB against a 2.0 GB rent cliff. Past it the rent
> rate climbs off its floor toward 10x at 3 GB. These numbers assume the floor.

## 4. Preflight

```bash
DRY_RUN=1 STELLAR_NETWORK=mainnet bash scripts/deploy_pipeline.sh
```

Checks, before spending anything:

- RPC reachable
- deployer account exists
- XLM balance ≥ `REQUIRED_XLM`
- a trustline **and** sufficient balance for every asset stage 7 will seed

This ordering matters: uploads are paid at stage 3, seeding happens at stage 7. A
run that dies at stage 7 has already burnt ~154 non-refundable XLM.

## 5. Deploy

```bash
STELLAR_NETWORK=mainnet DEPLOYER_IDENTITY=orbswap-mainnet \
  bash scripts/deploy_pipeline.sh
```

You will be asked to type `DEPLOY MAINNET`. Set `CONFIRM="DEPLOY MAINNET"` to skip
it in CI. Results land in `deployments/mainnet_stable.json`.

**Do a cheap pass first.** `SWAPS=0` with small seed amounts proves the whole
sequence for a couple of extra XLM. The uploads dominate and cannot be refunded,
so it is worth confirming the sequence before committing real liquidity.

## 6. Point the frontend at it

```bash
cd frontend && cp .env.example .env.local
```

```ini
NEXT_PUBLIC_STELLAR_NETWORK=PUBLIC
NEXT_PUBLIC_ORBSWAP_POOL=<super_pool>
NEXT_PUBLIC_ORBSWAP_FACTORY=<factory>
NEXT_PUBLIC_ORBSWAP_ROUTER=<router>
NEXT_PUBLIC_ORBSWAP_TICK_POOL=<circle_pool>
```

Setting the network flips the passphrase, the default RPC, every stellar.expert
link, and the token address table together. Contract addresses are **required** on
mainnet: the app throws at startup rather than silently pointing at testnet
contracts.

Token addresses are not env-driven — mainnet SACs are derived from each asset's
issuer, so they are compiled into `lib/stellar/config.ts`.

## 7. Assets

Five USD-pegged assets, all trading 1:1. Pool A takes the first four; pool B takes
USDC plus USDS, so **USDC is the shared leg** that connects the two for routing.

| Asset | Trustlines | SAC on mainnet |
|---|---|---|
| USDC (Circle) | 2,350,506 | already deployed |
| PYUSD (Paxos) | 691 | already deployed |
| USDGLO (Glo) | 626 | already deployed |
| USDx | 575 | already deployed |
| USDS | 2,763 | **pipeline deploys it** |

EURC and XCHF are excluded (they track EUR/CHF, so a 1:1 curve misprices them),
and yUSDC is excluded because it is yield-bearing and drifts off peg by design.

Two things to verify before going live:

- Only USDC has deep liquidity. The other four are thin, so organic flow may be
  minimal even though anyone can trade against the pools.
- USDS was selected on ticker and trustline count. Confirm its issuer actually
  maintains a hard 1:1 USD peg — that assumption is what the SuperElliptical curve
  rests on.

## 8. After deploying

- Record the addresses and the pool wasm hash somewhere durable.
- Diarise the rent renewal at day ~110.
- `admin` on both pools is the deployer key. It can pause deposits/swaps/
  withdrawals and set the protocol fee — treat it as a privileged operational key.
