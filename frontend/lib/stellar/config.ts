// Stellar/Soroban network + Orbswap deployment config for the swap widget.
// Mirrors contracts/deployments/testnet_seeded.json (the 24M-seeded 4-token pool).

export const STELLAR = {
  network: "TESTNET" as const,
  networkPassphrase: "Test SDF Network ; September 2015",
  rpcUrl: "https://soroban-testnet.stellar.org",
  // Any existing account works as the source for read-only `quote` simulations.
  readAccount: "GDLAEZGPYY6QDVHIEFWME3UFG6475EOADUDZ4MDEEHBEK6GOLDGIEX3O",
  pool: "CBMYB2V3U4IMQBNRGSSE2B7646YG756KJONZPAKAAJYFQ7L6OJGDNDLW",
  // Periphery: factory deploys/registers pools, router does multi-hop swaps.
  factory: "CC7J3JNSBILDA264Y3YKFQUQ6KAEIICPTENS2FN3O7BLYSFDCKVYDGEN",
  router: "CAV7RWVFGHLKH64R7IGKP5HCQ57SM5WTX2CDNTWBXW5C2S4346YIZUVW",
  explorerTx: (hash: string) => `https://stellar.expert/explorer/testnet/tx/${hash}`,
};

export interface StellarToken {
  symbol: string;
  address: string;
  decimals: number;
  color: string;
}

// The four SAC test tokens the pool trades. All are 7-decimal Stellar assets.
export const TOKENS: StellarToken[] = [
  { symbol: "USDA", address: "CBIMNDUMDFBE22ZLLGRLY46J2E4GTFGHOCA2KVE75HLZGJQBELEV4EPL", decimals: 7, color: "#4F9DFF" },
  { symbol: "USDB", address: "CD3SALJPZFKLBE5RBBLV2DDSBVUIDQRHMADKJBRY2VSVBL3KITUWO5JA", decimals: 7, color: "#35C08E" },
  { symbol: "USDC", address: "CBNDCO3DMKFVCSVFPHMYK6KSD6CCKVUMI3TFK6ZJ3BP7NCNLUJBJAB6Z", decimals: 7, color: "#F5A623" },
  { symbol: "USDD", address: "CDJVB5YTBHBNIZNVRKE7VYKQ6OHWEDDEYKO7U4DSEHPRGPERGEMTTPCB", decimals: 7, color: "#B26BFF" },
];

// 7-decimal fixed point ⇄ display float.
export const SCALE = 10_000_000; // 1e7
export const toNative = (display: number): bigint =>
  BigInt(Math.round(display * SCALE));
export const fromNative = (native: bigint): number => Number(native) / SCALE;
