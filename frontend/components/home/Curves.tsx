import { color, colors, typography } from "@/constants";
import { SectionLabel } from "./SectionLabel";
import { Emphasized } from "./Emphasized";
import { Tex } from "@/components/Tex";

const MONO = "var(--font-mono)";

type Curve = {
  n: string;
  kind: string;
  title: string;
  tag: string;
  tex: string;
  lede: string;
  body: string;
  points: string[];
  accent: string;
};

const CURVES: Curve[] = [
  {
    n: "01",
    kind: "CCMM",
    title: "Circular",
    tag: "2 tokens · polar ticks",
    tex: "(x-k)^{2} + (y-k)^{2} = k^{2}",
    lede: "Two coins ride a circular arc.",
    body: "Concentration comes from polar ticks: each LP places liquidity between two angles, like Uniswap v3 but in angle space, so capital packs tight around the 1:1 point. When price leaves the arc, that position stops trading and stops taking risk.",
    points: [
      "Balanced at 45 degrees, priced by the slope",
      "Per-position liquidity and fee growth",
      "A depeg is walled off at the tick edge",
    ],
    accent: colors.green.hex,
  },
  {
    n: "02",
    kind: "CSEMM",
    title: "SuperElliptical",
    tag: "2 to 8 tokens · shape concentration",
    tex: "\\textstyle\\sum_i \\lvert x_i - k\\rvert^{u} = k^{u}",
    lede: "A whole basket shares one superellipse.",
    body: "Here concentration is baked into the curve itself. The exponent u decides how boxy it is, from a gentle constant-sum line up to an LMSR-like wall. No ticks to manage, and every pair in the basket trades from the same reserves.",
    points: [
      "N assets in one pool, up to eight",
      "Concentration is a single shape parameter",
      "Isolates a single depegged asset on its own",
    ],
    accent: colors.purple.hex,
  },
];

function CurveCard({ c }: { c: Curve }) {
  return (
    <article
      className="flex flex-col"
      style={{ backgroundColor: color.surface1 }}
    >
      {/* header */}
      <div
        className="flex items-center justify-between px-6 py-4 border-b border-dashed"
        style={{ borderColor: color.borderSubtle }}
      >
        <span
          style={{
            fontFamily: MONO,
            fontSize: "11px",
            letterSpacing: "0.1em",
            color: color.textMuted,
            textTransform: "uppercase",
          }}
        >
          {`// ${c.n} / ${c.kind}`}
        </span>
        <span
          style={{
            fontFamily: MONO,
            fontSize: "10px",
            letterSpacing: "0.08em",
            color: c.accent,
            textTransform: "uppercase",
          }}
        >
          {c.tag}
        </span>
      </div>

      <div className="flex flex-col gap-6 px-6 pt-8 pb-8 flex-1">
        <h3
          style={{
            fontFamily: typography.h1.family,
            fontSize: "clamp(34px, 4vw, 52px)",
            lineHeight: "0.98",
            letterSpacing: "-0.04em",
            color: color.textPrimary,
            fontWeight: 400,
          }}
        >
          {c.title}
        </h3>

        {/* Formula */}
        <div
          className="px-4 border border-dashed flex items-center justify-center [&_.katex-display]:my-0"
          style={{ borderColor: color.borderSubtle, color: color.accent, fontSize: "19px", height: 60 }}
        >
          <Tex block>{c.tex}</Tex>
        </div>

        <p
          style={{
            fontFamily: typography.h2.family,
            fontSize: "clamp(20px, 2.1vw, 26px)",
            lineHeight: "1.3",
            letterSpacing: "-0.02em",
            color: color.textPrimary,
          }}
        >
          {c.lede}
        </p>

        <p
          className="flex-1"
          style={{
            fontFamily: typography.p2.family,
            fontSize: typography.p2.size,
            lineHeight: "22px",
            letterSpacing: typography.p2.letterSpacing,
            color: color.textMuted,
          }}
        >
          {c.body}
        </p>

        {/* Points */}
        <ul className="flex flex-col mt-1">
          {c.points.map((p, i) => (
            <li
              key={p}
              className="grid grid-cols-[20px_1fr] items-baseline gap-3 py-3 border-t border-dashed"
              style={{ borderColor: color.borderSubtle }}
            >
              <span
                style={{
                  fontFamily: MONO,
                  fontSize: "11px",
                  letterSpacing: "0.04em",
                  color: c.accent,
                }}
              >
                {String.fromCharCode(65 + i)}
              </span>
              <span
                style={{
                  fontFamily: typography.p1.family,
                  fontSize: "16px",
                  lineHeight: "1.35",
                  letterSpacing: "-0.01em",
                  color: color.textSecondary,
                }}
              >
                {p}
              </span>
            </li>
          ))}
        </ul>
      </div>
    </article>
  );
}

export function Curves() {
  return (
    <section className="mx-6 my-1">
      <SectionLabel border chapter="II" section="01" path="ORBSWAP / CURVES" />

      <div className="pb-12 pt-20">
        <Emphasized
          size="clamp(40px, 5vw, 72px)"
          lineHeight="1.05"
          letterSpacing="-0.04em"
          fontFamily={typography.h1.family}
          segments={[
            { t: "Two curves", v: "on" },
            { t: ", one idea", v: "off" },
            { t: ".", v: "green" },
            " ",
            { t: "A ", v: "off" },
            { t: "circle", v: "on" },
            { t: " concentrates two coins with ", v: "off" },
            { t: "polar ticks", v: "on" },
            { t: ".", v: "green" },
            " ",
            { t: "A ", v: "off" },
            { t: "superellipse", v: "on" },
            { t: " concentrates a whole basket by its ", v: "off" },
            { t: "shape", v: "on" },
            { t: ".", v: "green" },
            " ",
            { t: "Both keep liquidity dense at the peg", v: "on" },
            { t: ", where stablecoins actually trade", v: "off" },
            { t: ".", v: "green" },
          ]}
        />
      </div>

      <div className="grid grid-cols-1 gap-1 md:grid-cols-2 mb-24 items-stretch">
        {CURVES.map((c) => (
          <CurveCard key={c.n} c={c} />
        ))}
      </div>
    </section>
  );
}
