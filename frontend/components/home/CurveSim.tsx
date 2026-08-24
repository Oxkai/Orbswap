"use client";

/**
 * An interactive 2-D section through the Orbswap curve.
 *
 * The math here is a direct f64 port of `contracts/orbswap-math` — no illustrative
 * stand-in. Each function below names the Rust it mirrors, and the on-curve residual
 * is displayed so the reader can see the invariant actually closing rather than take
 * the drawing's word for it. The contract works in WAD fixed point and rounds every
 * result toward the pool; f64 here rounds to nearest, so the two agree to ~1e-15 but
 * are not bit-identical, and this panel is a diagram rather than a quoting engine.
 */

import { useMemo, useState } from "react";
import { color, colors, typography } from "@/constants";
import { SectionLabel } from "./SectionLabel";

const MONO = "var(--font-mono)";

/* ── Curve math (port of orbswap-math) ─────────────────────────────────────── */

/** `csemm::u` — the shape exponent `u(α) = ln2 / ln(α/(α−1))`, α ≥ 2. */
const uOf = (alpha: number) => Math.LN2 / Math.log(alpha / (alpha - 1));

/** Inverse of {@link uOf}: `α = 2^{1/u} / (2^{1/u} − 1)`. Lets the UI drive the
 *  shape by the exponent (the number with meaning) and derive the contract's α. */
const alphaOf = (u: number) => {
  const t = Math.pow(2, 1 / u);
  return t / (t - 1);
};

/**
 * `csemm::partner` — the reserve paired with `x` on the curve:
 * `y = β·[1 − (1 − (1 − x/α)^{u(α)})^{1/u(β)}]`, here with the symmetric α = β.
 */
function partner(x: number, alpha: number): number {
  const u = uOf(alpha);
  const term = Math.pow(Math.max(0, 1 - x / alpha), u);
  const outer = Math.pow(Math.max(0, 1 - term), 1 / u);
  return alpha * (1 - outer);
}

/**
 * `csemm::spot_price` — marginal price `p = −dy/dx`, which for α = β reduces to
 * `[(1 − x/α)/(1 − y/α)]^{u−1}`. Returns 0 at `x = α` and ∞ at `y = α`.
 */
function spotPrice(x: number, y: number, alpha: number): number {
  const u = uOf(alpha);
  const bx = 1 - x / alpha;
  const by = 1 - y / alpha;
  if (bx <= 0) return 0;
  if (by <= 0) return Infinity;
  return Math.pow(bx, u - 1) / Math.pow(by, u - 1);
}

/** `ndim::invariant_residual_n` — `Σ|xᵢ/αᵢ − 1|^u(αᵢ) − 1`. Zero on the curve. */
function residual(x: number, y: number, alpha: number): number {
  const u = uOf(alpha);
  return (
    Math.pow(Math.abs(x / alpha - 1), u) + Math.pow(Math.abs(y / alpha - 1), u) - 1
  );
}

/** Reserve `x` at which the marginal price equals `target`. Price is strictly
 *  decreasing in `x` on the arc, so a bisection is exact to f64 in ~60 steps. */
function xAtPrice(alpha: number, target: number): number {
  let lo = 1e-9;
  let hi = alpha - 1e-9;
  for (let i = 0; i < 70; i++) {
    const m = (lo + hi) / 2;
    if (spotPrice(m, partner(m, alpha), alpha) > target) lo = m;
    else hi = m;
  }
  return (lo + hi) / 2;
}

/* ── Plot geometry ─────────────────────────────────────────────────────────── */

// A fixed window on the peg. Every shape passes through (1,1), and the family
// stays inside 2.6 across the whole slider range, so the frame never jumps.
const VIEW = 2.6;
const W = 360;
const H = 312;
const PAD = { l: 38, r: 14, t: 14, b: 42 };
const PW = W - PAD.l - PAD.r;
const PH = H - PAD.t - PAD.b;

// Coordinates are rounded before they reach the DOM. `Math.pow` is not required to
// be correctly rounded, and Node and the browser can disagree by an ULP — enough to
// emit a different `points` string on the server than on the client and trip a
// hydration mismatch. Two decimals is far below a pixel and collapses that drift.
const px2 = (v: number) => Math.round(v * 100) / 100;

const sx = (x: number) => px2(PAD.l + (x / VIEW) * PW);
const sy = (y: number) => px2(PAD.t + PH - (y / VIEW) * PH);

// Price panel uses a log axis — price is a ratio, so equal ratios deserve equal space.
const P_MIN = 0.3;
const P_MAX = 3.3;
const sp = (p: number) =>
  px2(PAD.t + PH - ((Math.log(p) - Math.log(P_MIN)) / (Math.log(P_MAX) - Math.log(P_MIN))) * PH);

/* ── Whole-shape geometry ──────────────────────────────────────────────────── */

// The panels above zoom on the peg; these two draw the object itself. Working in
// normalized reserves X = x/α turns the invariant into |X−1|^u + |Y−1|^u = 1 — a
// superellipse centred on (1,1) with unit radius, so the frame is fixed for every
// shape and a circle actually renders as a circle.
const SW = 320;
const SPAD = 30;
const SS = SW - 2 * SPAD; // square plot area — an uneven one would draw the circle as an ellipse

const nx = (X: number) => px2(SPAD + (X / 2) * SS);
const ny = (Y: number) => px2(SPAD + SS - (Y / 2) * SS);

const sgn = (v: number) => (v < 0 ? -1 : 1);

/**
 * Exact parametrization of the closed superellipse:
 * `X = 1 + sgn(cos t)|cos t|^{2/u}`, `Y = 1 + sgn(sin t)|sin t|^{2/u}`.
 * Substituting gives `|X−1|^u + |Y−1|^u = cos²t + sin²t = 1`, so this traces the
 * whole curve — including the three folds the contract refuses to trade on.
 * `t ∈ [π, 3π/2]` is precisely the usable arc.
 */
function shapePoint(t: number, u: number): [number, number] {
  const c = Math.cos(t);
  const sn = Math.sin(t);
  return [
    1 + sgn(c) * Math.pow(Math.abs(c), 2 / u),
    1 + sgn(sn) * Math.pow(Math.abs(sn), 2 / u),
  ];
}

/**
 * `ndim::swap_out_n`'s solved reserve, in normalized coordinates: with two legs
 * fixed at `X, Y`, the third sits at `Z = 1 − (1 − S)^{1/u}`, `S = (1−X)^u + (1−Y)^u`.
 * `S > 1` means the point is off the arc — there is no third reserve there.
 */
function surfaceZ(X: number, Y: number, u: number): number | null {
  const S = Math.pow(1 - X, u) + Math.pow(1 - Y, u);
  if (S > 1) return null;
  return 1 - Math.pow(1 - S, 1 / u);
}

// A three-quarter view of the 3-asset patch. A true isometric would sight straight
// down the (1,1,1) diagonal — the patch's own axis of symmetry — which projects the
// bulge to nothing and makes a sphere look flat. Yawing off that diagonal restores
// the depth. One scale on both screen axes keeps proportions honest.
// This projection's view axis is (−sin YAW, cos YAW, tan PITCH), so the patch's own
// symmetry axis (1,1,1) sits at YAW = −45°, PITCH = atan(1/√2) ≈ 35.26°. Sighting
// exactly there gives a perfectly equilateral cap with no depth at all; a small
// offset keeps it near-symmetric while letting the dome read as a dome.
const ISO = SS / 1.9;
const YAW = (-34 * Math.PI) / 180;
const PITCH = (50 * Math.PI) / 180;

function iso(X: number, Y: number, Z: number): [number, number] {
  // Rotate about the middle of the unit cube so the view is centred.
  const x = X - 0.5;
  const y = Y - 0.5;
  const z = Z - 0.5;
  const rx = x * Math.cos(YAW) + y * Math.sin(YAW);
  const ry = -x * Math.sin(YAW) + y * Math.cos(YAW);
  return [
    px2(SW / 2 + rx * ISO),
    px2(SW / 2 + (ry * Math.sin(PITCH) - z * Math.cos(PITCH)) * ISO),
  ];
}

/** The 12 edges of the unit reserve cube [0,1]³, for orientation. */
const CUBE: [number, number, number][][] = (() => {
  const c: [number, number, number][] = [];
  for (let i = 0; i < 8; i++) c.push([(i >> 2) & 1, (i >> 1) & 1, i & 1]);
  const out: [number, number, number][][] = [];
  for (let a = 0; a < 8; a++)
    for (let b = a + 1; b < 8; b++) {
      const d = Math.abs(c[a][0] - c[b][0]) + Math.abs(c[a][1] - c[b][1]) + Math.abs(c[a][2] - c[b][2]);
      if (d === 1) out.push([c[a], c[b]]);
    }
  return out;
})();

/** What the 3-asset surface is called at this exponent. At u = 2 it is literally
 *  Orbital's n-sphere Σ(xᵢ−k)² = k²; at u → 1 the three terms are linear, so the
 *  surface flattens into the constant-sum plane. */
function surfaceName(u: number): string {
  if (u < 1.08) return "plane";
  if (Math.abs(u - 2) < 0.02) return "sphere";
  return "superellipsoid";
}

/** What the 2-asset curve is called at this exponent. */
function shapeName(u: number): string {
  if (u < 1.08) return "diamond";
  if (Math.abs(u - 2) < 0.02) return "circle";
  if (u < 2) return "sub-elliptical";
  if (u > 6) return "near-square";
  return "superellipse";
}

const PRESETS = [
  { label: "Constant sum", u: 1.02, note: "u → 1" },
  { label: "Circle · CCMM", u: 2, note: "u = 2" },
  { label: "Boxy", u: 4, note: "u = 4" },
] as const;

/** Residual below the f64 noise floor is zero; printing 1.1e-16 on one side of the
 *  hydration boundary and 0 on the other is both a mismatch and a lie. */
const fmtResidual = (v: number) => (Math.abs(v) < 1e-12 ? "0.0e+0" : v.toExponential(1));

const fmt = (v: number, d = 4) =>
  !isFinite(v) ? "∞" : Math.abs(v) < 1e-12 ? (0).toFixed(d) : v.toFixed(d);

/* ── Small presentational pieces ───────────────────────────────────────────── */

function Label({ children }: { children: React.ReactNode }) {
  return (
    <span
      style={{
        fontFamily: MONO,
        fontSize: "10px",
        letterSpacing: "0.08em",
        textTransform: "uppercase",
        color: color.textMuted,
      }}
    >
      {children}
    </span>
  );
}

function Metric({
  label,
  value,
  sub,
  accent,
}: {
  label: string;
  value: string;
  sub?: string;
  accent?: string;
}) {
  return (
    <div className="flex flex-col gap-1.5 px-4 py-3.5" style={{ backgroundColor: color.surface1 }}>
      <Label>{label}</Label>
      <span
        style={{
          fontFamily: MONO,
          fontSize: "17px",
          letterSpacing: "-0.01em",
          color: accent ?? color.textPrimary,
          fontVariantNumeric: "tabular-nums",
        }}
      >
        {value}
      </span>
      {sub && (
        <span style={{ fontFamily: MONO, fontSize: "10px", color: color.textMuted }}>{sub}</span>
      )}
    </div>
  );
}

function Slider({
  value,
  min,
  max,
  step,
  accent,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  accent: string;
  onChange: (v: number) => void;
}) {
  const pct = ((value - min) / (max - min)) * 100;
  return (
    <input
      type="range"
      className="modern-slider"
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(e) => onChange(parseFloat(e.target.value))}
      style={
        {
          "--slider-accent": accent,
          "--slider-track": color.surface3,
          "--slider-pct": `${pct}%`,
        } as React.CSSProperties
      }
    />
  );
}

/* ── Component ─────────────────────────────────────────────────────────────── */

export function CurveSim() {
  const [u, setU] = useState(2);
  const [dx, setDx] = useState(0.15);

  const m = useMemo(() => {
    const alpha = alphaOf(u);

    // Every pool in this panel starts from the same balanced deposit. That is not a
    // simplification: u(α) is defined so the price-1 point sits at exactly (1,1) for
    // every shape, which is what makes the shapes comparable at a glance.
    const x0 = 1;
    const y0 = partner(x0, alpha);

    const x1 = Math.min(x0 + dx, alpha - 1e-9);
    const y1 = partner(x1, alpha);
    const out = y0 - y1;
    const execPrice = dx > 0 ? out / dx : 1;

    // Curve + price polylines over the visible window.
    const pts: string[] = [];
    const prices: string[] = [];
    const N = 240;
    for (let i = 0; i <= N; i++) {
      const x = (i / N) * Math.min(alpha, VIEW);
      const y = partner(x, alpha);
      if (y <= VIEW) pts.push(`${sx(x)},${sy(y)}`);
      const p = spotPrice(x, y, alpha);
      if (isFinite(p) && p >= P_MIN && p <= P_MAX) prices.push(`${sx(x)},${sp(p)}`);
    }

    // Depth: the x-interval that clears inside a ±1% price move, for this fixed
    // deposit. Its width is the whole capital-efficiency story.
    const bandLo = xAtPrice(alpha, 1.01);
    const bandHi = xAtPrice(alpha, 0.99);
    const band = bandHi - bandLo;
    // Isolation: how much of the good asset survives a 5% depeg of the other leg.
    const survives = partner(xAtPrice(alpha, 0.95), alpha);

    // The closed curve, and the quarter of it the pool actually trades on.
    const closed: string[] = [];
    for (let i = 0; i <= 360; i++) {
      const [X, Y] = shapePoint((i / 360) * 2 * Math.PI, u);
      closed.push(`${nx(X)},${ny(Y)}`);
    }
    const arc: string[] = [];
    for (let i = 0; i <= 120; i++) {
      const [X, Y] = shapePoint(Math.PI + (i / 120) * (Math.PI / 2), u);
      arc.push(`${nx(X)},${ny(Y)}`);
    }

    // 3-asset patch. The wireframe runs both ways; each rib stops where S > 1,
    // which is exactly where the surface meets a coordinate plane.
    const ribs: string[] = [];
    const RIBS = 8;
    const STEP = 48;
    for (let i = 1; i < RIBS; i++) {
      const a = i / RIBS;
      const alongY: string[] = [];
      const alongX: string[] = [];
      for (let j = 0; j <= STEP; j++) {
        const b = j / STEP;
        const z1 = surfaceZ(a, b, u);
        if (z1 !== null) { const [px_, py_] = iso(a, b, z1); alongY.push(`${px_},${py_}`); }
        const z2 = surfaceZ(b, a, u);
        if (z2 !== null) { const [px_, py_] = iso(b, a, z2); alongX.push(`${px_},${py_}`); }
      }
      if (alongY.length > 1) ribs.push(alongY.join(" "));
      if (alongX.length > 1) ribs.push(alongX.join(" "));
    }

    // The three edges of the patch: one reserve pinned at its cap (Xᵢ = 1), leaving
    // the other two on the 2-asset curve. So the n = 3 surface is bounded on all three
    // sides by the n = 2 problem, meeting at the corners (0,1,1), (1,0,1), (1,1,0).
    const edgePts: [number, number][][] = [];
    for (const plane of [0, 1, 2]) {
      const e: [number, number][] = [];
      for (let i = 0; i <= 120; i++) {
        const [A, B] = shapePoint(Math.PI + (i / 120) * (Math.PI / 2), u);
        e.push(plane === 0 ? iso(A, B, 1) : plane === 1 ? iso(A, 1, B) : iso(1, A, B));
      }
      edgePts.push(e);
    }
    const edges = edgePts.map((e) => e.map(([a, b]) => `${a},${b}`).join(" "));

    // Walk the three edges into one closed boundary so the patch can be filled:
    // (0,1,1) →e0→ (1,0,1) →e2 reversed→ (1,1,0) →e1 reversed→ (0,1,1).
    const patch = [...edgePts[0], ...[...edgePts[2]].reverse(), ...[...edgePts[1]].reverse()]
      .map(([a, b]) => `${a},${b}`)
      .join(" ");

    // Balanced deposit in n dimensions: all legs equal, so n(1−X)^u = 1.
    const balN = (n: number) => 1 - Math.pow(1 / n, 1 / u);
    const bal2 = balN(2);
    const bal3 = balN(3);

    return {
      alpha,
      closed: closed.join(" "),
      arc: arc.join(" "),
      ribs,
      edges,
      patch,
      bal2,
      bal3,
      x0,
      y0,
      x1,
      y1,
      out,
      execPrice,
      spotAfter: spotPrice(x1, y1, alpha),
      residual: residual(x1, y1, alpha),
      curve: pts.join(" "),
      price: prices.join(" "),
      band,
      bandLo,
      bandHi,
      survives,
    };
  }, [u, dx]);

  // The two cards above this section own these colours: green is the circle, purple
  // the general superellipse. Letting the curve flip at u = 2 ties them together.
  const isCircle = Math.abs(u - 2) < 0.02;
  const accent = isCircle ? colors.green.hex : colors.purple.hex;
  const kind = isCircle ? "CCMM" : "CSEMM";

  const axisTicks = [0, 0.5, 1, 1.5, 2, 2.5];

  return (
    <section className="mx-6 my-1">
      <SectionLabel border chapter="III" section="02" path="ORBSWAP / SIMULATION" />

      <div className="pt-16 pb-6 grid grid-cols-12 gap-5">
        <h2
          className="col-span-12 md:col-span-7 min-w-0"
          style={{
            fontFamily: typography.h1.family,
            fontSize: "clamp(30px, 3.6vw, 48px)",
            lineHeight: "1.02",
            letterSpacing: "-0.04em",
            fontWeight: 400,
            color: color.textPrimary,
          }}
        >
          Move the exponent, watch the trade-off.
        </h2>
        <p
          className="col-span-12 md:col-span-5 min-w-0 md:pt-2"
          style={{
            fontFamily: typography.p2.family,
            fontSize: typography.p2.size,
            lineHeight: "22px",
            letterSpacing: typography.p2.letterSpacing,
            color: color.textMuted,
          }}
        >
          Both curves are one family. <span style={{ color: color.textSecondary }}>u</span> is the
          only dial: at <span style={{ color: color.textSecondary }}>u&nbsp;=&nbsp;1</span> the pool
          is a constant-sum line, at{" "}
          <span style={{ color: colors.green.hex }}>u&nbsp;=&nbsp;2</span> it is the circle, and
          past that it stiffens toward a wall. Every number below is computed from the same
          functions the contract runs.
        </p>
      </div>

      {/* ── Controls ─────────────────────────────────────────────────────── */}
      <div className="grid grid-cols-12 gap-1 mb-1">
        <div
          className="col-span-12 md:col-span-6 min-w-0 flex flex-col gap-3 px-5 py-4"
          style={{ backgroundColor: color.surface1 }}
        >
          <div className="flex items-baseline justify-between">
            <Label>Shape exponent u</Label>
            <span
              style={{
                fontFamily: MONO,
                fontSize: "12px",
                color: accent,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              u = {u.toFixed(2)} · α = {m.alpha.toFixed(3)}
            </span>
          </div>
          <Slider value={u} min={1.02} max={8} step={0.01} accent={accent} onChange={setU} />
          <div className="flex flex-wrap gap-1 pt-1">
            {PRESETS.map((p) => {
              const on = Math.abs(u - p.u) < 0.02;
              return (
                <button
                  key={p.label}
                  onClick={() => setU(p.u)}
                  className="flex items-center gap-2 px-3 py-1.5 transition-colors"
                  style={{
                    backgroundColor: on ? color.surface3 : color.surface2,
                    color: on ? color.textPrimary : color.textMuted,
                    fontFamily: MONO,
                    fontSize: "10px",
                    letterSpacing: "0.06em",
                    textTransform: "uppercase",
                    cursor: "pointer",
                  }}
                >
                  {p.label}
                  <span style={{ color: on ? accent : color.textMuted }}>{p.note}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div
          className="col-span-12 md:col-span-6 min-w-0 flex flex-col gap-3 px-5 py-4"
          style={{ backgroundColor: color.surface1 }}
        >
          <div className="flex items-baseline justify-between">
            <Label>Sell X into the pool</Label>
            <span
              style={{
                fontFamily: MONO,
                fontSize: "12px",
                color: color.textSecondary,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              Δx = {dx.toFixed(3)}
            </span>
          </div>
          <Slider value={dx} min={0} max={1.2} step={0.005} accent={accent} onChange={setDx} />
          <div
            className="pt-1"
            style={{
              fontFamily: MONO,
              fontSize: "10px",
              letterSpacing: "0.04em",
              color: color.textMuted,
            }}
          >
            {"//"} pool holds 1.00 of each leg, so Δx is a fraction of the pool
          </div>
        </div>
      </div>

      {/* ── The object itself ────────────────────────────────────────────── */}
      <div className="grid grid-cols-12 gap-1">
        {/* Whole 2-asset curve */}
        <figure
          className="col-span-12 md:col-span-6 min-w-0 flex flex-col"
          style={{ backgroundColor: color.surface1 }}
        >
          <figcaption
            className="flex items-center justify-between px-5 py-3 border-b border-dashed"
            style={{ borderColor: color.borderSubtle }}
          >
            <Label>A / whole curve · 2 assets</Label>
            <span style={{ fontFamily: MONO, fontSize: "10px", color: accent }}>
              {shapeName(u)}
            </span>
          </figcaption>
          <svg viewBox={`0 0 ${SW} ${SW}`} className="block w-full max-w-full" role="img"
               aria-label={`The full closed curve at shape exponent u equals ${u.toFixed(2)}: a ${shapeName(u)}`}>
            {/* the box the reserves live in: X, Y ∈ [0,1] */}
            <rect x={nx(0)} y={ny(1)} width={nx(1) - nx(0)} height={ny(0) - ny(1)}
                  fill={color.surface2} opacity="0.55" />

            {[0, 1, 2].map((t) => (
              <g key={t}>
                <line x1={nx(t)} y1={ny(0)} x2={nx(t)} y2={ny(2)}
                      stroke={color.borderSubtle} strokeWidth="1" strokeDasharray="2 4" />
                <line x1={nx(0)} y1={ny(t)} x2={nx(2)} y2={ny(t)}
                      stroke={color.borderSubtle} strokeWidth="1" strokeDasharray="2 4" />
              </g>
            ))}

            {/* the three folds — real solutions of the invariant, refused by the pool */}
            <polyline points={m.closed} fill="none" stroke={color.textMuted}
                      strokeWidth="1" strokeDasharray="3 3" opacity="0.5" />
            {/* the quarter that trades */}
            <polyline points={m.arc} fill="none" stroke={accent} strokeWidth="2.5" />

            {/* centre (1,1) = (α,α) in real reserves */}
            <line x1={nx(1) - 4} y1={ny(1)} x2={nx(1) + 4} y2={ny(1)}
                  stroke={color.textMuted} strokeWidth="1" />
            <line x1={nx(1)} y1={ny(1) - 4} x2={nx(1)} y2={ny(1) + 4}
                  stroke={color.textMuted} strokeWidth="1" />

            {/* balanced deposit */}
            <rect x={nx(m.bal2) - 3} y={ny(m.bal2) - 3} width="6" height="6" fill={color.textPrimary} />
            <text x={nx(m.bal2) + 9} y={ny(m.bal2) + 3}
                  fill={color.textSecondary} fontFamily={MONO} fontSize="9">
              peg
            </text>

            <text x={nx(0.05)} y={ny(0.12)} fill={color.textMuted} fontFamily={MONO} fontSize="9">
              tradable quadrant
            </text>
            <text x={nx(1.35)} y={ny(1.72)} fill={color.textMuted} fontFamily={MONO} fontSize="9"
                  opacity="0.8">
              folds — rejected
            </text>
          </svg>
        </figure>

        {/* 3-asset surface */}
        <figure
          className="col-span-12 md:col-span-6 min-w-0 flex flex-col"
          style={{ backgroundColor: color.surface1 }}
        >
          <figcaption
            className="flex items-center justify-between px-5 py-3 border-b border-dashed"
            style={{ borderColor: color.borderSubtle }}
          >
            <Label>B / same invariant · 3 assets</Label>
            <span style={{ fontFamily: MONO, fontSize: "10px", color: accent }}>
              {surfaceName(u)}
            </span>
          </figcaption>
          <svg viewBox={`0 0 ${SW} ${SW}`} className="block w-full max-w-full" role="img"
               aria-label={`The three-asset surface at shape exponent u equals ${u.toFixed(2)}`}>
            {/* the reserve cube [0,1]³ — the box every reserve lives in */}
            {CUBE.map(([a, b], i) => {
              const [x1, y1] = iso(...a);
              const [x2, y2] = iso(...b);
              return (
                <line key={i} x1={x1} y1={y1} x2={x2} y2={y2}
                      stroke={color.border} strokeWidth="1" strokeDasharray="2 3" />
              );
            })}

            {/* axis ticks read off the cube's near corner */}
            {([[1, 0, 0, "x"], [0, 1, 0, "y"], [0, 0, 1, "z"]] as const).map(([X, Y, Z, l]) => {
              const [ax, ay] = iso(X, Y, Z);
              return (
                <text key={l} x={ax} y={ay + 12} textAnchor="middle"
                      fill={color.textMuted} fontFamily={MONO} fontSize="9">
                  {l}
                </text>
              );
            })}

            <polygon points={m.patch} fill={accent} opacity="0.15" />
            {m.ribs.map((r, i) => (
              <polyline key={i} points={r} fill="none" stroke={accent}
                        strokeWidth="1" opacity="0.32" />
            ))}
            {m.edges.map((e, i) => (
              <polyline key={i} points={e} fill="none" stroke={accent} strokeWidth="2" />
            ))}

            {(() => {
              const [bx, by] = iso(m.bal3, m.bal3, m.bal3);
              return (
                <>
                  <rect x={bx - 3} y={by - 3} width="6" height="6" fill={color.textPrimary} />
                  <text x={bx + 10} y={by + 3} fill={color.textSecondary}
                        fontFamily={MONO} fontSize="9">
                    peg
                  </text>
                </>
              );
            })()}

            <text x={SPAD} y={SW - 10} fill={color.textMuted} fontFamily={MONO} fontSize="9">
              every edge is the 2-asset curve
            </text>
          </svg>
        </figure>
      </div>

      {/* ── Trading behaviour ────────────────────────────────────────────── */}
      <div className="grid grid-cols-12 gap-1 mt-1">
        {/* Reserve space */}
        <figure
          className="col-span-12 md:col-span-6 min-w-0 flex flex-col"
          style={{ backgroundColor: color.surface1 }}
        >
          <figcaption
            className="flex items-center justify-between px-5 py-3 border-b border-dashed"
            style={{ borderColor: color.borderSubtle }}
          >
            <Label>C / peg detail</Label>
            <span style={{ fontFamily: MONO, fontSize: "10px", color: accent }}>{kind}</span>
          </figcaption>
          <svg viewBox={`0 0 ${W} ${H}`} className="block w-full max-w-full" role="img"
               aria-label={`Reserve curve for shape exponent u equals ${u.toFixed(2)}`}>
            {/* grid */}
            {axisTicks.map((t) => (
              <g key={t}>
                <line x1={sx(t)} y1={PAD.t} x2={sx(t)} y2={PAD.t + PH}
                      stroke={color.borderSubtle} strokeWidth="1" strokeDasharray="2 4" />
                <line x1={PAD.l} y1={sy(t)} x2={PAD.l + PW} y2={sy(t)}
                      stroke={color.borderSubtle} strokeWidth="1" strokeDasharray="2 4" />
                <text x={sx(t)} y={PAD.t + PH + 15} textAnchor="middle"
                      fill={color.textMuted} fontFamily={MONO} fontSize="9">
                  {t}
                </text>
                <text x={PAD.l - 8} y={sy(t) + 3} textAnchor="end"
                      fill={color.textMuted} fontFamily={MONO} fontSize="9">
                  {t}
                </text>
              </g>
            ))}

            {/* the u → 1 limit, kept on screen as the thing the curve departs from */}
            <line x1={sx(0)} y1={sy(2)} x2={sx(2)} y2={sy(0)}
                  stroke={color.textMuted} strokeWidth="1" strokeDasharray="3 3" opacity="0.4" />

            {/* the curve */}
            <polyline points={m.curve} fill="none" stroke={accent} strokeWidth="2" />

            {/* balanced point — square, matching the token-swatch language */}
            <rect x={sx(1) - 3} y={sy(1) - 3} width="6" height="6" fill={color.textMuted} />
            <text x={sx(1) + 8} y={sy(1) - 7} fill={color.textMuted} fontFamily={MONO} fontSize="9">
              (1,1) p=1
            </text>

            {/* the trade: start → end along the arc */}
            {dx > 0 && (
              <>
                <line x1={sx(m.x0)} y1={sy(m.y0)} x2={sx(m.x1)} y2={sy(m.y0)}
                      stroke={accent} strokeWidth="1" strokeDasharray="2 2" opacity="0.7" />
                <line x1={sx(m.x1)} y1={sy(m.y0)} x2={sx(m.x1)} y2={sy(m.y1)}
                      stroke={accent} strokeWidth="1" strokeDasharray="2 2" opacity="0.7" />
                <circle cx={sx(m.x1)} cy={sy(m.y1)} r="4" fill={accent} />
                <text x={sx(m.x1) + 8} y={sy(m.y1) + 12}
                      fill={accent} fontFamily={MONO} fontSize="9">
                  p={fmt(m.spotAfter, 3)}
                </text>
              </>
            )}

            {/* axes */}
            <line x1={PAD.l} y1={PAD.t + PH} x2={PAD.l + PW} y2={PAD.t + PH}
                  stroke={color.border} strokeWidth="1" />
            <line x1={PAD.l} y1={PAD.t} x2={PAD.l} y2={PAD.t + PH}
                  stroke={color.border} strokeWidth="1" />
            <text x={PAD.l + PW} y={PAD.t + PH + 31} textAnchor="end"
                  fill={color.textMuted} fontFamily={MONO} fontSize="9">
              reserve x
            </text>
          </svg>
        </figure>

        {/* Price */}
        <figure
          className="col-span-12 md:col-span-6 min-w-0 flex flex-col"
          style={{ backgroundColor: color.surface1 }}
        >
          <figcaption
            className="flex items-center justify-between px-5 py-3 border-b border-dashed"
            style={{ borderColor: color.borderSubtle }}
          >
            <Label>D / marginal price</Label>
            <span style={{ fontFamily: MONO, fontSize: "10px", color: color.textMuted }}>
              log scale
            </span>
          </figcaption>
          <svg viewBox={`0 0 ${W} ${H}`} className="block w-full max-w-full" role="img"
               aria-label={`Marginal price against reserve x for shape exponent u equals ${u.toFixed(2)}`}>
            {/* The ±1% peg band, read on the reserve axis: every x in this strip
                trades within 1% of par. It collapses as u rises — that collapse is
                the capital-efficiency trade-off the whole panel is about. */}
            <rect
              x={sx(m.bandLo)}
              y={PAD.t}
              width={Math.max(1.5, sx(m.bandHi) - sx(m.bandLo))}
              height={PH}
              fill={colors.green.hex}
              opacity="0.22"
            />
            <text x={sx(m.bandHi) + 6} y={PAD.t + 11}
                  fill={colors.green.hex} fontFamily={MONO} fontSize="9" opacity="0.9">
              ±1% depth {m.band < 0.1 ? m.band.toFixed(3) : m.band.toFixed(2)}
            </text>

            {[0.5, 1, 2].map((p) => (
              <g key={p}>
                <line x1={PAD.l} y1={sp(p)} x2={PAD.l + PW} y2={sp(p)}
                      stroke={p === 1 ? color.border : color.borderSubtle}
                      strokeWidth="1" strokeDasharray={p === 1 ? "" : "2 4"} />
                <text x={PAD.l - 8} y={sp(p) + 3} textAnchor="end"
                      fill={color.textMuted} fontFamily={MONO} fontSize="9">
                  {p.toFixed(p === 1 ? 2 : 1)}
                </text>
              </g>
            ))}
            {axisTicks.map((t) => (
              <g key={t}>
                <line x1={sx(t)} y1={PAD.t} x2={sx(t)} y2={PAD.t + PH}
                      stroke={color.borderSubtle} strokeWidth="1" strokeDasharray="2 4" />
                <text x={sx(t)} y={PAD.t + PH + 15} textAnchor="middle"
                      fill={color.textMuted} fontFamily={MONO} fontSize="9">
                  {t}
                </text>
              </g>
            ))}

            <polyline points={m.price} fill="none" stroke={accent} strokeWidth="2" />

            {dx > 0 && isFinite(m.spotAfter) && m.spotAfter >= P_MIN && (
              <>
                <line x1={sx(m.x1)} y1={PAD.t} x2={sx(m.x1)} y2={PAD.t + PH}
                      stroke={accent} strokeWidth="1" strokeDasharray="2 2" opacity="0.5" />
                <circle cx={sx(m.x1)} cy={sp(m.spotAfter)} r="4" fill={accent} />
              </>
            )}

            <line x1={PAD.l} y1={PAD.t + PH} x2={PAD.l + PW} y2={PAD.t + PH}
                  stroke={color.border} strokeWidth="1" />
            <line x1={PAD.l} y1={PAD.t} x2={PAD.l} y2={PAD.t + PH}
                  stroke={color.border} strokeWidth="1" />
            <text x={PAD.l + PW} y={PAD.t + PH + 31} textAnchor="end"
                  fill={color.textMuted} fontFamily={MONO} fontSize="9">
              reserve x
            </text>
          </svg>
        </figure>
      </div>

      {/* ── Live invariant ───────────────────────────────────────────────── */}
      <div
        className="mt-1 px-5 py-4 flex flex-wrap items-center justify-between gap-x-6 gap-y-2"
        style={{ backgroundColor: color.surface1 }}
      >
        <span
          style={{
            fontFamily: MONO,
            fontSize: "13px",
            letterSpacing: "0.01em",
            color: color.textSecondary,
            fontVariantNumeric: "tabular-nums",
          }}
        >
          |x/{m.alpha.toFixed(3)} − 1|
          <sup style={{ color: accent }}>{u.toFixed(2)}</sup> + |y/{m.alpha.toFixed(3)} − 1|
          <sup style={{ color: accent }}>{u.toFixed(2)}</sup> = 1
        </span>
        <span style={{ fontFamily: MONO, fontSize: "11px", color: color.textMuted }}>
          residual at the traded point ={" "}
          <span style={{ color: Math.abs(m.residual) < 1e-9 ? colors.green.hex : color.warning }}>
            {fmtResidual(m.residual)}
          </span>
        </span>
      </div>

      {/* ── Readouts ─────────────────────────────────────────────────────── */}
      <div className="grid grid-cols-2 md:grid-cols-5 gap-1 mt-1 mb-24">
        <Metric label="Y received" value={fmt(m.out)} sub={`for ${dx.toFixed(3)} X`} />
        <Metric
          label="Execution price"
          value={fmt(m.execPrice, 5)}
          sub={`${((m.execPrice - 1) * 100).toFixed(2)}% vs peg`}
          accent={m.execPrice < 0.9 ? color.warning : undefined}
        />
        <Metric label="Spot after" value={fmt(m.spotAfter, 5)} sub="marginal" />
        <Metric
          label="Depth ±1%"
          value={fmt(m.band, 3)}
          sub="X inside a 1% move"
          accent={accent}
        />
        <Metric
          label="Survives 5% depeg"
          value={`${(m.survives * 100).toFixed(1)}%`}
          sub="of Y still held"
          accent={accent}
        />
      </div>
    </section>
  );
}
