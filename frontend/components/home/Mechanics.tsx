import { color, typography } from "@/constants";
import { SectionLabel } from "./SectionLabel";
import { Emphasized } from "./Emphasized";
import { Tex } from "@/components/Tex";

const MONO = "var(--font-mono)";

const CARDS = [
  {
    n: "01",
    title: "Polar",
    lede: "Reserves ride the curve, indexed by one angle.",
    tex: "x = k(1-\\cos\\theta),\\; y = k(1-\\sin\\theta)",
    body: "Every point on the curve is a single angle. At 45 degrees the pair trades 1:1; the price is just the slope, so a swap is one step along the arc.",
  },
  {
    n: "02",
    title: "Ticks",
    lede: "Each LP picks an arc, its own price range.",
    tex: "\\theta_{\\text{lo}} \\le \\theta \\le \\theta_{\\text{hi}}",
    body: "In a two-token circle, liquidity sits between two angles, like Uniswap v3 but in angle space. Capital packs near the peg and earns fees only while price stays inside the arc.",
  },
  {
    n: "03",
    title: "Shape",
    lede: "One exponent sets how tightly it packs.",
    tex: "\\textstyle\\sum_i \\lvert x_i - k\\rvert^{u} = k^{u}",
    body: "For pools of many coins the curve itself concentrates. The exponent u slides from a constant-sum line, through a circle, up to a boxy LMSR, trading depth at the peg against range.",
  },
] as const;

function MechanicsCard({ card }: { card: typeof CARDS[number] }) {
  return (
    <article
      className="flex flex-col"
      style={{ backgroundColor: color.surface1, minHeight: 320 }}
    >
      <div
        className="px-6 py-4 border-b border-dashed"
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
          {`// ${card.n} / ${card.title.toUpperCase()}`}
        </span>
      </div>

      <div className="flex flex-col gap-5 px-6 pt-8 pb-7">
        <p
          style={{
            fontFamily: typography.h1.family,
            fontSize: "clamp(24px, 2.6vw, 34px)",
            lineHeight: "1.12",
            letterSpacing: "-0.03em",
            color: color.textPrimary,
            fontWeight: 400,
            minHeight: "2.24em",
          }}
        >
          {card.lede}
        </p>

        {/* Formula */}
        <div
          className="px-4 border border-dashed flex items-center justify-center [&_.katex-display]:my-0"
          style={{ borderColor: color.borderSubtle, color: color.accent, fontSize: "18px", height: 56 }}
        >
          <Tex block>{card.tex}</Tex>
        </div>

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
          {card.body}
        </p>
      </div>
    </article>
  );
}

export function Mechanics() {
  return (
    <section className="mx-6 my-1">
      <SectionLabel border chapter="IV" section="03" path="ORBSWAP / MECHANICS" />

      <div className="pb-12 pt-20">
        <Emphasized
          size="clamp(40px, 5vw, 72px)"
          lineHeight="1.05"
          letterSpacing="-0.04em"
          fontFamily={typography.h1.family}
          segments={[
            { t: "The curve", v: "on" },
            { t: " is written in ", v: "off" },
            { t: "polar coordinates", v: "on" },
            { t: ".", v: "green" },
            " ",
            { t: "One ", v: "off" },
            { t: "angle", v: "on" },
            { t: " fixes every reserve on it", v: "off" },
            { t: ".", v: "green" },
            " ",
            { t: "A ", v: "off" },
            { t: "range of angles", v: "on" },
            { t: " is an LP's tick", v: "off" },
            { t: ".", v: "green" },
            " ",
            { t: "And one ", v: "off" },
            { t: "exponent", v: "on" },
            { t: " sets how tightly liquidity packs around the peg", v: "off" },
            { t: ".", v: "green" },
          ]}
        />
      </div>

      <div className="grid grid-cols-1 gap-1 md:grid-cols-3 mb-24">
        {CARDS.map((card) => (
          <MechanicsCard key={card.n} card={card} />
        ))}
      </div>
    </section>
  );
}
