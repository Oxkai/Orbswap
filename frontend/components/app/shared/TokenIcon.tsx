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
  TokenEURC,
  TokenPYUSD,
  TokenXCHF,
  TokenIDRT,
  TokenGYEN,
  TokenXSGD,
  TokenXLM,
  TokenAQUA,
} from "@web3icons/react";
import { color, typography } from "@/constants";

// Keyed on assets that actually trade on Stellar mainnet, so a symbol resolving to
// a mark here is also a symbol a user could hold. GYEN/XSGD are live Stellar assets
// we don't pool yet; they cost nothing to keep mapped and save a monogram later.
const ICONS: Record<string, React.ElementType> = {
  USDC:  TokenUSDC,
  EURC:  TokenEURC,
  PYUSD: TokenPYUSD,
  XCHF:  TokenXCHF,
  IDRT:  TokenIDRT,
  GYEN:  TokenGYEN,
  XSGD:  TokenXSGD,
  XLM:   TokenXLM,
  AQUA:  TokenAQUA,
};

/**
 * Marks drawn as a light glyph on a black tile. The app's surfaces run #0A0A0A–#232323,
 * so these read as a hole rather than a logo; a hairline ring restores the edge without
 * touching the artwork.
 */
const DARK_MARK = new Set(["XLM"]);

/** Brand colours for the monogram fallback. */
const COLORS: Record<string, string> = {
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
      {glyph}
    </span>
  );
}
