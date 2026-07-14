// Stellar/Soroban network + Orbswap deployment config for the swap widget.
// Mirrors contracts/deployments/testnet_demo.json. USDC is SHARED with the tick
// pool (see ticks.ts), so EURC/USDM/BRLT ↔ NGNC route multi-hop through it.

export const STELLAR = {
  network: "TESTNET" as const,
  networkPassphrase: "Test SDF Network ; September 2015",
  rpcUrl: "https://soroban-testnet.stellar.org",
  // Any existing account works as the source for read-only `quote` simulations.
  readAccount: "GDLAEZGPYY6QDVHIEFWME3UFG6475EOADUDZ4MDEEHBEK6GOLDGIEX3O",
  // SuperElliptical pool: USDC / EURC / USDM / BRLT, 24M TVL.
  pool: "CDGR7RRE72JKAW5UATPKCANAPVX3YVLPDEKSPNPVZ5BKLI43VAUC2RWK",
  // Periphery: factory deploys/registers pools, router does multi-hop swaps.
  factory: "CCKK33NWDQPSONMRAWH2FNF2ZRZ4VR3PLUP73FUHZGOEMUH665WD5TJW",
  router: "CCARGBIGZFOUIOSVM4Q5RNDOIJISYOFSL2VHVYPYWIZP67IV6FHGAVLJ",
  explorerTx: (hash: string) => `https://stellar.expert/explorer/testnet/tx/${hash}`,
};

export interface StellarToken {
  symbol: string;
  address: string;
  decimals: number;
  color: string;
}

// The four SAC test tokens in the SuperElliptical pool. All 7-decimal Stellar assets.
export const TOKENS: StellarToken[] = [
  { symbol: "USDC", address: "CBNDCO3DMKFVCSVFPHMYK6KSD6CCKVUMI3TFK6ZJ3BP7NCNLUJBJAB6Z", decimals: 7, color: "#2775CA" },
  { symbol: "EURC", address: "CAXVONHQX5SHHTEG2AQYSE4YO6CSSRIOGA3MTCTDL3ZS5K2HVWFGEHID", decimals: 7, color: "#14B8A6" },
  { symbol: "USDM", address: "CB6MC6LXWCOGEDPOTJIQHTE6VZF46RWVGWJJE6D52SS4P2FTJSBEJBMN", decimals: 7, color: "#8B5CF6" },
  { symbol: "BRLT", address: "CAD3IHN2D7LYAVAIUMTS5KAWCT6ACXPTV2LAQNVO6Q77WDP3XLY2BGG7", decimals: 7, color: "#F59E0B" },
];

// 7-decimal fixed point ⇄ display float.
export const SCALE = 10_000_000; // 1e7
export const toNative = (display: number): bigint =>
  BigInt(Math.round(display * SCALE));
export const fromNative = (native: bigint): number => Number(native) / SCALE;
