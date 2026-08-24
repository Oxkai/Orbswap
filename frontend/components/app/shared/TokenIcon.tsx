"use client";

/**
 * The single token-icon renderer for the app.
 *
 * Every asset in a live pool now maps to a published mark from `@web3icons/react`,
 * so the monogram path below is a fallback for assets we haven't mapped yet rather
 * than the normal case.
 *
 * Icons are imported by name rather than through the package's `TokenIcon` symbol
 * resolver: that one is explicitly documented as not tree-shakable, and
 * `dist/icons` is ~115 MB across 1849 components.
 */

import {
  TokenUSDC,
  TokenPYUSD,
  TokenUSDT,
  TokenDAI,
  TokenFDUSD,
  TokenEURC,
  TokenXCHF,
  TokenIDRT,
  TokenXLM,
  TokenAQUA,
} from "@web3icons/react";
import { color, typography } from "@/constants";

// Published marks for real-world assets, kept so any pool listing one renders its
// own mark rather than a monogram. Orbswap's pooled assets (USDA-USDF) are its own
// issuance, have no published mark, and fall through to ORB_MARKS below — which is
// correct: they are not these issuers' tokens.
const ICONS: Record<string, React.ElementType> = {
  USDC:  TokenUSDC,
  PYUSD: TokenPYUSD,
  USDT:  TokenUSDT,
  DAI:   TokenDAI,
  FDUSD: TokenFDUSD,
  EURC:  TokenEURC,
  XCHF:  TokenXCHF,
  IDRT:  TokenIDRT,
  XLM:   TokenXLM,
  AQUA:  TokenAQUA,
};

/**
 * Marks drawn as a light glyph on a black tile. The app's surfaces run #0A0A0A–#232323,
 * so these read as a hole rather than a logo; a hairline ring restores the edge without
 * touching the artwork.
 */
const DARK_MARK = new Set(["XLM"]);

/** Brand colours for the monogram fallback. USDA–USDF are Orbswap's own pooled
 *  test assets: no published mark exists, so they always take this path and each
 *  needs its own hue to stay distinguishable. They match lib/stellar/config.ts. */
const COLORS: Record<string, string> = {
  USDT:  "#26A17B",
  DAI:   "#F5AC37",
  FDUSD: "#C8A46A",
  USDA:  "#3B82F6",
  USDB:  "#06B6D4",
  USDD:  "#F59E0B",
  USDE:  "#10B981",
  USDF:  "#8B5CF6",
  USDC:  "#2775CA",
  EURC:  "#7FC4FF",
  PYUSD: "#009CDE",
  XCHF:  "#CE0E2D",
  IDRT:  "#BB4E42",
};

/**
 * A currency glyph reads far better at 14–16px than a squashed two-letter
 * monogram, and makes the pegged currency legible at a glance.
 */
const GLYPHS: Record<string, string> = {
  USD: "$",
  EUR: "€",
  GBP: "£",
  JPY: "¥",
  NGN: "₦",
  BRL: "R$",
  IDR: "Rp",
  CHF: "Fr",
  SGD: "S$",
};


/**
 * Marks for Orbswap's own pooled assets (USDA-USDF).
 *
 * No icon library publishes these — they are our issued test assets — so the
 * fallback path below always renders them. Colour alone was doing all the work,
 * which fails at 16px and fails entirely for colour-blind users, so each asset
 * also gets its own silhouette. The shapes are drawn from the project's own
 * vocabulary (orbit, crescent, superellipse) and stay legible as solidwhite forms
 * down to ~14px.
 *
 * USDA leads with the orbit mark because it is the shared leg: the only asset in
 * both pools, and the hop every cross-pool route passes through.
 */
const ORB_MARKS: Record<string, React.ReactNode> = {
  // Aster — an eight-point star inside its orbit ring. USDA leads with the ring
  // because it is the shared leg: the only asset in both pools, and the hop every
  // cross-pool route passes through.
  USDA: (
    <>
      <circle cx="12" cy="12" r="8.6" fill="none" stroke="currentColor" strokeWidth="1.15" opacity="0.55" />
      <path
        d="M12 4.6 13.5 9.4 18.2 7.6 15.6 11.9 20.4 13.4 15.4 13.9 17.1 18.6 13.1 15.7 12 20.5 10.9 15.7 6.9 18.6 8.6 13.9 3.6 13.4 8.4 11.9 5.8 7.6 10.5 9.4Z"
        fill="currentColor"
      />
    </>
  ),
  // Beacon — a crescent throwing light.
  USDB: (
    <>
      <path d="M14.9 4.4a7.9 7.9 0 1 0 0 15.2 9 9 0 0 1 0-15.2Z" fill="currentColor" />
      <circle cx="16.6" cy="8.2" r="1.5" fill="currentColor" opacity="0.6" />
    </>
  ),
  // Delta — a triangle with its own counter, so it reads as a mark not a shape.
  USDD: (
    <path
      d="M12 3.9 20.3 18.6H3.7Zm0 4.6L7.7 16h8.6Z"
      fill="currentColor"
      fillRule="evenodd"
    />
  ),
  // Ember — a diamond with a lit core.
  USDE: (
    <>
      <path d="M12 3.4 20.6 12 12 20.6 3.4 12Z" fill="currentColor" opacity="0.5" />
      <path d="M12 7.9 16.1 12 12 16.1 7.9 12Z" fill="currentColor" />
    </>
  ),
  // Facet — a cut hexagon.
  USDF: (
    <>
      <path d="M12 3.5 19.4 7.75v8.5L12 20.5 4.6 16.25v-8.5Z" fill="currentColor" opacity="0.5" />
      <path d="M12 8 15.9 10.25v4.5L12 17 8.1 14.75v-4.5Z" fill="currentColor" />
    </>
  ),
};

export interface TokenIconProps {
  symbol: string;
  size?: number;
  /** Overrides the built-in brand colour for the fallback badge. */
  color?: string;
}

export function TokenIcon({ symbol, size = 24, color: colorOverride }: TokenIconProps) {
  const key = symbol.toUpperCase();
  const Icon = ICONS[key];

  if (Icon) {
    if (!DARK_MARK.has(key)) return <Icon size={size} variant="branded" />;
    return (
      <span
        style={{
          display: "inline-flex",
          borderRadius: "50%",
          boxShadow: `inset 0 0 0 1px ${color.surface4}`,
          lineHeight: 0,
          flexShrink: 0,
        }}
      >
        <Icon size={size} variant="branded" />
      </span>
    );
  }

  const bg = colorOverride ?? COLORS[key] ?? "#555";
  const mark = ORB_MARKS[key];
  // Prefer the currency sign implied by the ticker's leading ISO code (USDx → $).
  const glyph = GLYPHS[key.slice(0, 3)] ?? key.slice(0, 2);
  // Glyphs are single-character and can run larger than a two-letter monogram.
  const scale = glyph.length > 1 ? 0.36 : 0.52;

  return (
    <span
      aria-hidden
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        // Soft top-light so the badge sits alongside the library's shaded marks.
        backgroundImage: `linear-gradient(160deg, ${bg} 0%, ${bg} 55%, rgba(0,0,0,0.22) 100%)`,
        backgroundColor: bg,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        fontSize: Math.max(6, Math.round(size * scale)),
        lineHeight: 1,
        color: "#fff",
        fontFamily: typography.caption.family,
        fontWeight: 700,
        letterSpacing: glyph.length > 1 ? "-0.02em" : 0,
        userSelect: "none",
      }}
    >
      {mark ? (
        <svg
          width={Math.round(size * 0.68)}
          height={Math.round(size * 0.68)}
          viewBox="0 0 24 24"
          style={{ color: "#fff", display: "block" }}
        >
          {mark}
        </svg>
      ) : (
        glyph
      )}
    </span>
  );
}
