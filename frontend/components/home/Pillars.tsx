import { color, colors, typography } from "@/constants";
import { SectionLabel } from "./SectionLabel";
import { Emphasized } from "./Emphasized";

type Pillar = {
  n: string;
  tag: string;
  title: string;
  lede: string;
  body: string;
  note: string;
  metric: { value: string; unit: string; caption: string };
  accent: string;
};

const PILLARS: Pillar[] = [
  {
    n: "01",
    tag: "EXECUTION",
    title: "Low slippage",
    lede: "While the coins hold their peg, even large trades barely move the price.",
    body: "Every LP concentrates liquidity right at the peg, where stablecoins actually trade. Depth is highest exactly where orders land, so a big swap clears at almost the same rate as a small one.",
    note: "A constant-product pool spreads the same capital thinly across every price, most of it parked at rates that never trade. Orbswap puts all of it to work at par.",
    metric: { value: "< 1", unit: "bps", caption: "price impact at peg, p = 0.99" },
    accent: colors.green.hex,
  },
  {
    n: "02",
    tag: "DENSITY",
    title: "High capital efficiency",
    lede: "A dollar at the peg does the work of a hundred in a constant-product pool.",
    body: "Two to eight assets share a single pool, with nothing split across separate pairs. The curve's shape sets how tightly liquidity packs around par, so almost none of it sits idle at prices that never print.",
    note: "Concentration is one shape parameter, tunable from a gentle constant-sum line up to a near-boxy LMSR, and no asset ever has to leave the pool.",
    metric: { value: "100", unit: "×", caption: "vs constant product, at peg" },
    accent: colors.purple.hex,
  },
  {
    n: "03",
    tag: "RESILIENCE",
    title: "Automatic depeg isolation",
    lede: "If one coin breaks its peg, the others keep trading as if nothing happened.",
    body: "The failing asset is walled off the instant its price leaves the tick. The pool stops buying it, so it can't drain the healthy reserves, and the rest of the basket stays fully liquid against itself.",
    note: "No governance vote, no emergency pause, no oracle to trust. The isolation is a property of the curve, so it happens on its own and on-chain.",
    metric: { value: "N − 1", unit: "assets", caption: "keep trading through a depeg" },
    accent: colors.yellow.hex,
  },
];

export function Pillars() {
  return (
    <section className="mx-6">
      <SectionLabel border chapter="V" section="04" path="ORBSWAP / PRINCIPLES" />

      <div className="grid grid-cols-12 gap-5 pt-20 pb-12">
        <h2
          className="col-span-12 text-left"
          style={{
            fontFamily: typography.h1.family,
            fontSize: "clamp(44px, 7vw, 76px)",
            lineHeight: "0.9",
            letterSpacing: "-0.05em",
            fontWeight: 400,
            color: color.textPrimary,
          }}
        >
          Principles
        </h2>
      </div>

      <div className="pb-20" style={{ borderColor: color.borderSubtle }}>
        <Emphasized
          size="clamp(22px, 2.4vw, 32px)"
          lineHeight="1.35"
          letterSpacing="-0.025em"
          fontFamily={typography.h2.family}
          maxWidth="58ch"
          segments={[
            { t: "Three properties", v: "on" },
            { t: " make Orbswap work", v: "off" },
            { t: ".", v: "green" },
            " ",
            { t: "Trades stay cheap", v: "on" },
            { t: " while the coins hold their peg", v: "off" },
            { t: ".", v: "green" },
            " ",
            { t: "Liquidity stays dense", v: "on" },
            { t: " across every asset in the basket", v: "off" },
            { t: ".", v: "green" },
            " ",
            { t: "And when one coin ", v: "off" },
            { t: "breaks", v: "on" },
            { t: ", the curve ", v: "off" },
            { t: "walls it off", v: "on" },
            { t: " before it can drain the pool", v: "off" },
            { t: ".", v: "green" },
          ]}
        />
      </div>

      <div className="grid grid-cols-12 gap-5">
        {PILLARS.map((p, i) => {
          const letter = String.fromCharCode(65 + i);
          return (
            <article
              key={p.n}
              className="col-span-12 border-t border-dashed"
              style={{ borderColor: color.borderSubtle }}
            >
              <div
                className="grid grid-cols-12 items-center gap-5 py-3"
                style={{
                  fontFamily: "var(--font-mono)",
                  fontSize: typography.caption.size,
                  letterSpacing: typography.caption.letterSpacing,
                  color: color.textMuted,
                  textTransform: "uppercase",
                }}
              >
                <span className="col-span-12 md:col-span-2">{`// V / 04 / ${letter}`}</span>
                <span className="col-span-12 md:col-span-10">{`// PRINCIPLES / ${p.tag}`}</span>

              </div>

              <div className="grid grid-cols-1 md:grid-cols-12 gap-y-10 md:gap-x-10 py-20 md:py-12">
                <div className="md:col-span-2">
                  <span
                    style={{
                       fontFamily: typography.h1.family,
                      fontSize: "clamp(36px, 3.6vw, 40px)",
                      lineHeight: "1.05",
                      letterSpacing: "-0.03em",
                      color: color.textMuted,
                      fontWeight: 400,
                    }}
                  >
                    {letter}
                  </span>
                </div>

                <div className="md:col-span-4">
                  <h3
                    style={{
                      fontFamily: typography.h1.family,
                      fontSize: "clamp(36px, 3.6vw, 40px)",
                      lineHeight: "1.05",
                      letterSpacing: "-0.03em",
                      color: color.textPrimary,
                      fontWeight: 400,
                    }}
                  >
                    {p.title}
                  </h3>
                </div>

                <div className="md:col-span-6 md:col-start-7  grid gap-10">
                  <p
                    style={{
                       fontFamily: typography.h2.family,
                        fontSize: "clamp(22px, 2.4vw, 32px)",
                        lineHeight: "1.35",

                     color: color.textPrimary,

                    }}
                  >
                    {p.lede} {p.body}
                  </p>

                  <p
                    style={{
                      fontFamily: typography.p1.family,
                      fontSize: "clamp(20px, 1.6vw, 26px)",
                      lineHeight: "1.45",
                      color: color.textMuted,
                      maxWidth: "42ch",
                    }}
                  >
                    {p.note}
                  </p>

                  <div
                    className="grid grid-cols-12 items-baseline gap-5 pt-6 border-t border-dashed max-w-md"
                    style={{ borderColor: color.borderSubtle }}
                  >
                    <p
                      className="col-span-7"
                      style={{
                        fontFamily: "var(--font-mono)",
                        fontSize: typography.caption.size,
                        lineHeight: typography.caption.lineHeight,
                        letterSpacing: typography.caption.letterSpacing,
                        color: color.textMuted,
                        textTransform: "uppercase",
                      }}
                    >
                      {p.metric.caption}
                    </p>
                    <div className="col-span-5 inline-grid grid-flow-col auto-cols-max items-baseline gap-1.5 whitespace-nowrap justify-self-end">
                      <span
                        style={{
                          fontFamily: typography.h2.family,
                          fontSize: typography.h2.size,
                          lineHeight: "1",
                          letterSpacing: typography.h2.letterSpacing,
                          color: p.accent,
                          fontWeight: 500,
                        }}
                      >
                        {p.metric.value}
                      </span>
                      <span
                        style={{
                          fontFamily: typography.p2.family,
                          fontSize: typography.p2.size,
                          lineHeight: typography.p2.lineHeight,
                          letterSpacing: typography.p2.letterSpacing,
                          color: color.textSecondary,
                        }}
                      >
                        {p.metric.unit}
                      </span>
                    </div>
                  </div>
                </div>
              </div>
            </article>
          );
        })}
        <div
          className=""
          style={{ borderColor: color.borderSubtle }}
        />
      </div>
    </section>
  );
}
