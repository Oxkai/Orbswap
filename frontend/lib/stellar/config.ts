// Stellar/Soroban network + Orbswap deployment config.
//
// The app targets Stellar mainnet (the "Public Global" network). Addresses come from
// contracts/deployments/mainnet_stable.json and are compiled in, so `npm run dev`
// works with no env file; individual values can still be overridden. See
// frontend/.env.example.

export const EXPLORER = "https://stellar.expert/explorer/public";

/** Human-readable network name for UI chrome, kept as an export so the label lives
 *  in one place instead of being hardcoded in a dozen components. */
export const NETWORK_LABEL = "Stellar Mainnet";
export const NETWORK_LABEL_UPPER = NETWORK_LABEL.toUpperCase();
export const explorerTx = (hash: string) => `${EXPLORER}/tx/${hash}`;
export const explorerContract = (address: string) => `${EXPLORER}/contract/${address}`;

export const STELLAR = {
  network: "PUBLIC",
  networkPassphrase: "Public Global Stellar Network ; September 2015",
  // Public endpoints rate-limit — override before taking real traffic.
  rpcUrl: process.env.NEXT_PUBLIC_STELLAR_RPC_URL ?? "https://mainnet.sorobanrpc.com",
  // Any existing account works as the source for read-only `quote` simulations.
  readAccount:
    process.env.NEXT_PUBLIC_STELLAR_READ_ACCOUNT ??
    "GDLAEZGPYY6QDVHIEFWME3UFG6475EOADUDZ4MDEEHBEK6GOLDGIEX3O",
  // SuperElliptical pool: USDC / PYUSD / USDT / DAI, 24M TVL.
  pool:
    process.env.NEXT_PUBLIC_ORBSWAP_POOL ??
    "CDBVM2FMNSDDR4FXE53RWCMWMLILFRSTO3345NGIWI63NNMO7TIPY6PM",
  // Periphery: factory deploys/registers pools, router does multi-hop swaps.
  factory:
    process.env.NEXT_PUBLIC_ORBSWAP_FACTORY ??
    "CDMMO6NHPPTFEEUGOXX77QQ3GJJ4HJLGNVLEHYQNFNYGILTYTVSFT3IP",
  router:
    process.env.NEXT_PUBLIC_ORBSWAP_ROUTER ??
    "CA7GUCUQSWKAPGYELJRKJKCLZMNHMJDVBA3D3NMJCK2QB3IJAZOTC6E2",
  explorerTx,
};

export interface StellarToken {
  symbol: string;
  address: string;
  decimals: number;
  color: string;
  /** Issuer/product name shown under the ticker. */
  name?: string;
}

// The four display legs of the SuperElliptical pool. All 7-decimal Stellar assets,
// all pegged 1:1 to each other — the regime the curve is calibrated for.
//
// Issuer: GCASMKFHTQRAAMDGB4IMS3ZXI2FZR7G4XS74MVTBR5D2UADD63ALRN4U
//
// `color` gives each leg a distinct hue so the reserve-distribution bars stay
// readable at bar-width.
export const TOKENS: StellarToken[] = [
  { symbol: "USDC",  name: "USD Coin",       address: "CCTTKWIGUWJM7ZRBXCFP7AJKZPOQ2YYISBTA4ZIBMTYQBBLKO3FZ7OX6", decimals: 7, color: "#2775CA" },
  { symbol: "PYUSD", name: "PayPal USD",     address: "CDRRMZB42WENXRGZ2EEAFNAOHFLNIJZX2OJXG5TGWBR3WE5DAYKHXM2A", decimals: 7, color: "#009CDE" },
  { symbol: "USDT",  name: "Tether USD",     address: "CD22PZOZMXE3NU4VPHMPIKNDQKUZWI4MGYYH3EF25QMK2R2XMJSLP2TV", decimals: 7, color: "#26A17B" },
  { symbol: "DAI",   name: "Dai Stablecoin", address: "CANCIHPUY6LBPH5JH4MFBBQTFOC64C5QNWAL77YZAIX7J7X2LHXSLFCZ", decimals: 7, color: "#F5AC37" },
];

// 7-decimal fixed point ⇄ display float.
export const SCALE = 10_000_000; // 1e7
export const toNative = (display: number): bigint =>
  BigInt(Math.round(display * SCALE));
export const fromNative = (native: bigint): number => Number(native) / SCALE;
