import { color, typography } from "@/constants";
import { SectionLabel } from "./SectionLabel";
import { Emphasized } from "./Emphasized";

const MONO = "var(--font-mono)";

function cap() {
  return {
    fontFamily: MONO,
    fontSize: "10px",
    letterSpacing: "0.1em",
    textTransform: "uppercase" as const,
    color: color.textMuted,
  };
}

function Box({
  children,
  accent = false,
  className = "",
}: {
  children: React.ReactNode;
  accent?: boolean;
  className?: string;
}) {
  return (
    <div
      className={className}
      style={{
        border: `1px ${accent ? "solid" : "dashed"} ${accent ? color.accent : color.border}`,
        backgroundColor: color.surface1,
      }}
    >
      {children}
    </div>
  );
}

// A crate: name + one-line role + the wasm size (libraries link in, so they
// have no standalone wasm).
const PERIPHERY = [
  {
    name: "orbswap-router",
    tag: "stateless · no custody",
    body: "Multi-hop swap_exact_in / swap_exact_out, quote_path, and add / remove-liquidity passthroughs. Moves tokens through pools, never holds them.",
    wasm: "13 KB",
  },
  {
    name: "orbswap-factory",
    tag: "create_pool · registry",
    body: "Deploys and initializes a pool, keyed by sha256(PoolKey) with canonical token ordering and dedup. Two-token pools; larger baskets deploy directly.",
    wasm: "12 KB",
  },
] as const;

const POOL_FEATURES: [string, string][] = [
  ["reserves + LP shares", "custodies its own SEP-41 tokens, mints and transfers pool shares"],
  ["fees outside the curve", "swap and protocol fees are held apart, so the invariant stays exact"],
  ["oracle · pause · depeg block", "spot price, a kill switch, and a wall that isolates a broken asset"],
];

const MATH_MODULES = ["ccmm", "csemm", "ndim", "polar", "ticks", "skew", "fees", "oracle"];

export function Architecture() {
  return (
    <section className="mx-6 my-1">
      <SectionLabel border chapter="IV" section="03" path="ORBSWAP / ARCHITECTURE" />

      <div className="pb-12 pt-20">
        <Emphasized
          size="clamp(40px, 5vw, 72px)"
          lineHeight="1.05"
          letterSpacing="-0.04em"
          fontFamily={typography.h1.family}
          segments={[
            { t: "Four contracts", v: "on" },
            { t: " over one ", v: "off" },
            { t: "math library", v: "on" },
            { t: ".", v: "green" },
            " ",
            { t: "A ", v: "off" },
            { t: "router", v: "on" },
            { t: " and a ", v: "off" },
            { t: "factory", v: "on" },
            { t: " call every ", v: "off" },
            { t: "pool", v: "on" },
            { t: " through a typed client, so they never link its wasm", v: "off" },
            { t: ".", v: "green" },
            " ",
            { t: "Each pool holds its own tokens", v: "on" },
            { t: " and runs the curve in pure, no-float Rust", v: "off" },
            { t: ".", v: "green" },
          ]}
        />
      </div>

      {/* Diagram */}
      <div
        className="border border-dashed mb-24"
        style={{ borderColor: color.border, backgroundColor: color.bg }}
      >
        {/* Layer 0 - who interacts */}
        <div className="px-5 pt-6 pb-7">
          <span style={cap()}>{`// WHO INTERACTS`}</span>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-1 mt-4">
            {[
              {
                name: "Trader",
                via: "router.swap_exact_in",
                path: "→ picks a path, then swaps hop by hop across the pools it names",
              },
              {
                name: "Liquidity Provider",
                via: "pool.deposit / withdraw",
                path: "→ funds a pool directly (or through the router) and mints LP shares",
              },
            ].map((actor) => (
              <Box key={actor.name} className="px-5 py-5">
                <div className="flex flex-wrap items-baseline justify-between gap-3">
                  <span
                    style={{
                      fontFamily: typography.h1.family,
                      fontSize: "clamp(18px, 2.4vw, 26px)",
                      letterSpacing: "-0.03em",
                      color: color.textPrimary,
                      fontWeight: 400,
                    }}
                  >
                    {actor.name}
                  </span>
                  <span style={{ fontFamily: MONO, fontSize: "11px", letterSpacing: "0.02em", color: color.accent }}>
                    {actor.via}
                  </span>
                </div>
                <p
                  style={{
                    fontFamily: typography.p2.family,
                    fontSize: typography.p2.size,
                    lineHeight: "18px",
                    color: color.textMuted,
                    marginTop: 8,
                  }}
                >
                  {actor.path}
                </p>
              </Box>
            ))}
          </div>
        </div>

        {/* Connector */}
        <div
          className="flex items-center justify-center py-2 border-t border-b border-dashed"
          style={{ borderColor: color.borderSubtle }}
        >
          <span style={cap()}>{`enter through the periphery  ↓`}</span>
        </div>

        {/* Layer 1 - periphery: router + factory */}
        <div className="px-5 pt-6 pb-7">
          <span style={cap()}>{`// PERIPHERY · STATELESS`}</span>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-1 mt-4">
            {PERIPHERY.map((c) => (
              <Box key={c.name} className="px-5 py-5">
                <div className="flex flex-wrap items-baseline justify-between gap-3">
                  <span style={{ fontFamily: MONO, fontSize: "14px", letterSpacing: "0.01em", color: color.textPrimary }}>
                    {c.name}
                  </span>
                  <span style={{ fontFamily: MONO, fontSize: "10px", letterSpacing: "0.08em", color: color.textMuted, textTransform: "uppercase" }}>
                    {c.tag}
                  </span>
                </div>
                <p
                  style={{
                    fontFamily: typography.p2.family,
                    fontSize: typography.p2.size,
                    lineHeight: "18px",
                    color: color.textMuted,
                    marginTop: 8,
                  }}
                >
                  {c.body}
                </p>
                <div className="mt-3 pt-2 border-t border-dashed" style={{ borderColor: color.borderSubtle }}>
                  <span style={{ fontFamily: MONO, fontSize: "10px", letterSpacing: "0.06em", color: color.textMuted }}>
                    {`WASM ${c.wasm}`}
                  </span>
                </div>
              </Box>
            ))}
          </div>
        </div>

        {/* Connector */}
        <div
          className="flex items-center justify-center py-2 border-t border-b border-dashed"
          style={{ borderColor: color.borderSubtle }}
        >
          <span style={cap()}>{`call pools through a typed client  ↓`}</span>
        </div>

        {/* Layer 2 - the interface seam */}
        <div className="px-5 py-7">
          <Box className="px-5 py-5">
            <div className="flex flex-wrap items-baseline justify-between gap-3">
              <span style={{ fontFamily: MONO, fontSize: "14px", letterSpacing: "0.01em", color: color.textPrimary }}>
                orbswap-pool-interface
              </span>
              <span style={cap()}>{`// #[contractclient] + SHARED TYPES`}</span>
            </div>
            <p
              style={{
                fontFamily: typography.p2.family,
                fontSize: typography.p2.size,
                lineHeight: "20px",
                color: color.textMuted,
                marginTop: 10,
                maxWidth: 680,
              }}
            >
              The seam between periphery and pool. Router and factory depend only on this crate, so they can
              call any pool by its typed client without linking the pool&apos;s wasm symbols.
            </p>
          </Box>
        </div>

        {/* Connector */}
        <div
          className="flex items-center justify-center py-2 border-t border-b border-dashed"
          style={{ borderColor: color.borderSubtle }}
        >
          <span style={cap()}>{`which dispatches to  ↓`}</span>
        </div>

        {/* Layer 3 - the pool (the heart) */}
        <div className="px-5 py-7">
          <Box accent className="px-5 py-5">
            <div className="flex flex-wrap items-baseline justify-between gap-3">
              <span
                style={{
                  fontFamily: typography.h1.family,
                  fontSize: "clamp(22px, 3vw, 34px)",
                  letterSpacing: "-0.03em",
                  color: color.textPrimary,
                  fontWeight: 400,
                }}
              >
                orbswap-pool
              </span>
              <span style={{ ...cap(), color: color.accent }}>{`// 2-TOKEN & N-TOKEN (n ≤ 8) · WASM 57 KB`}</span>
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-1 mt-4">
              {POOL_FEATURES.map(([t, d]) => (
                <div key={t} className="border-t border-dashed pt-3" style={{ borderColor: color.borderSubtle }}>
                  <div
                    style={{
                      fontFamily: MONO,
                      fontSize: "12px",
                      letterSpacing: "0.02em",
                      color: color.textPrimary,
                    }}
                  >
                    {t}
                  </div>
                  <div
                    style={{
                      fontFamily: typography.p2.family,
                      fontSize: typography.p2.size,
                      lineHeight: "18px",
                      color: color.textMuted,
                      marginTop: 4,
                    }}
                  >
                    {d}
                  </div>
                </div>
              ))}
            </div>
          </Box>
        </div>

        {/* Connector */}
        <div
          className="flex items-center justify-center py-2 border-t border-b border-dashed"
          style={{ borderColor: color.borderSubtle }}
        >
          <span style={cap()}>{`runs the invariant in  ↓`}</span>
        </div>

        {/* Layer 4 - the math library */}
        <div className="px-5 py-7">
          <Box className="px-5 py-5">
            <div className="flex flex-wrap items-baseline justify-between gap-3">
              <span
                style={{
                  fontFamily: typography.h1.family,
                  fontSize: "clamp(20px, 2.6vw, 28px)",
                  letterSpacing: "-0.03em",
                  color: color.textPrimary,
                  fontWeight: 400,
                }}
              >
                orbswap-math
              </span>
              <span style={cap()}>{`// PURE no_std · ZERO-DEP · NO FLOAT · FUZZED`}</span>
            </div>
            <p
              style={{
                fontFamily: typography.p2.family,
                fontSize: typography.p2.size,
                lineHeight: "20px",
                color: color.textMuted,
                marginTop: 10,
                maxWidth: 680,
              }}
            >
              A library, not a contract: it links straight into the pool. All curve math is fixed-point WAD
              integers, so results are deterministic and every LP and protocol balance stays exact.
            </p>
            <div className="flex flex-wrap gap-1 mt-4">
              {MATH_MODULES.map((m) => (
                <span
                  key={m}
                  className="px-2.5 py-1.5 border border-dashed"
                  style={{
                    borderColor: color.borderSubtle,
                    fontFamily: MONO,
                    fontSize: "11px",
                    letterSpacing: "0.02em",
                    color: color.textSecondary,
                  }}
                >
                  {m}
                </span>
              ))}
            </div>
          </Box>
        </div>

        {/* Runtime - the per-swap call path */}
        <div
          className="px-5 py-5 border-t border-dashed"
          style={{ borderColor: color.border, backgroundColor: color.surface1 }}
        >
          <span style={cap()}>{`// ONE HOP, AT RUNTIME`}</span>
          <div className="flex flex-wrap items-center gap-x-2 gap-y-2 mt-4">
            {[
              "router.swap_exact_in",
              "PoolClient",
              "pool.swap",
              "math::ccmm / csemm",
              "token.transfer",
              "next hop",
            ].map((step, i, arr) => (
              <span key={step} className="inline-flex items-center gap-2">
                <span
                  className="px-2.5 py-1.5 border border-dashed"
                  style={{
                    borderColor: color.borderSubtle,
                    fontFamily: MONO,
                    fontSize: "11px",
                    letterSpacing: "0.02em",
                    color: i === 2 || i === 3 ? color.accent : color.textSecondary,
                    whiteSpace: "nowrap",
                  }}
                >
                  {step}
                </span>
                {i < arr.length - 1 && (
                  <span style={{ fontFamily: MONO, fontSize: "11px", color: color.textMuted }}>→</span>
                )}
              </span>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}
