import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import { compact, monthLabel } from "./format";

// One chart for every view: a month-by-month line with the area to zero (red below zero),
// optional bars, a dashed baseline (the plan before a what-if), a dashed floor, and dots on
// months that have tasks (milestones on the projection, as ProjectionLab does). Hover a month,
// or focus the chart (one tab stop, a slider) and use the arrow keys, to see its tooltip.

export interface ChartProps {
  months: string[]; // YYYY-MM
  line: number[];
  base?: number[];
  bars?: number[];
  floor?: number | null;
  markers?: boolean[];
  label: string;
  tooltip: (i: number) => ReactNode;
  height?: number;
}

const PAD = { l: 52, r: 12, t: 12, b: 24 };

export function ticks(lo: number, hi: number): number[] {
  const span = hi - lo || 1;
  const raw = span / 4;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) out.push(Math.round(v));
  // One tick at or above the top, so the line never sits above the highest label.
  if (out.length === 0 || out[out.length - 1] < hi - 1e-9)
    out.push(Math.round((out.length ? out[out.length - 1] : lo) + step));
  return out;
}

function shortMonth(ym: string, i: number): string {
  const [y, m] = ym.split("-").map(Number);
  const name = new Date(y, m - 1, 1).toLocaleDateString("en-CA", { month: "short" });
  return i === 0 || m === 1 ? `${name} '${String(y).slice(2)}` : name;
}

export function Chart({ months, line, base, bars, floor, markers, label, tooltip, height = 220 }: ChartProps) {
  const id = useId().replace(/:/g, "");
  const [active, setActive] = useState<number | null>(null);
  // Draw at the box's real width, so text and strokes keep their size on any screen.
  const box = useRef<HTMLDivElement>(null);
  const [W, setW] = useState(720);
  useEffect(() => {
    const el = box.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([e]) => setW(Math.max(280, Math.round(e.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const n = months.length;
  if (n === 0) return null;
  const all = [...line, ...(base ?? []), ...(bars ?? []), 0, ...(floor != null ? [floor] : [])];
  let lo = Math.min(...all);
  let hi = Math.max(...all);
  const padY = (hi - lo || 1) * 0.08;
  lo = lo < 0 ? lo - padY : 0;
  hi += padY;
  const tk = ticks(lo, hi);
  hi = Math.max(hi, tk[tk.length - 1]);
  // A nearly flat line reads fine in a shorter box; keep tall boxes for real movement.
  const spread = Math.max(...line) - Math.min(...line);
  const H = spread < (hi - lo) * 0.15 ? Math.round(height * 0.6) : height;
  const iw = W - PAD.l - PAD.r;
  const ih = H - PAD.t - PAD.b;
  // Inset the first and last month so bars and dots clear the axis labels.
  const inset = n === 1 ? iw / 2 : Math.min(18, iw / (n - 1) / 2);
  const x = (i: number) => PAD.l + (n === 1 ? iw / 2 : inset + (i * (iw - 2 * inset)) / (n - 1));
  const y = (v: number) => PAD.t + ((hi - v) * ih) / (hi - lo);
  const y0 = y(0);
  const path = (vals: number[]) => vals.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const area = `${path(line)}L${x(n - 1)},${y0}L${x(0)},${y0}Z`;
  const step = Math.max(1, Math.ceil(n / Math.max(3, Math.floor(W / 90))));
  const slot = n === 1 ? iw : (iw - 2 * inset) / (n - 1);
  const barW = Math.min(18, slot * 0.45);
  return (
    <div
      className="chart-box"
      ref={box}
      role="slider"
      aria-label={`${label}. Use the arrow keys to read each month.`}
      aria-valuemin={0}
      aria-valuemax={n - 1}
      aria-valuenow={active ?? 0}
      aria-valuetext={monthLabel(months[active ?? 0])}
      tabIndex={0}
      onFocus={() => setActive((a) => a ?? 0)}
      onBlur={() => setActive(null)}
      onKeyDown={(e) => {
        const to: Record<string, number> = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };
        let next: number | null = null;
        if (e.key in to) next = Math.min(n - 1, Math.max(0, (active ?? 0) + to[e.key]));
        if (e.key === "Home") next = 0;
        if (e.key === "End") next = n - 1;
        if (next === null) return;
        e.preventDefault();
        setActive(next);
      }}
    >
      <svg className="chart" viewBox={`0 0 ${W} ${H}`} aria-hidden="true">
        <defs>
          <clipPath id={`above-${id}`}>
            <rect x={0} y={0} width={W} height={y0} />
          </clipPath>
          <clipPath id={`below-${id}`}>
            <rect x={0} y={y0} width={W} height={H} />
          </clipPath>
        </defs>
        {tk.map((t) => (
          <g key={t}>
            <line className={t === 0 ? "zero" : "grid"} x1={PAD.l} x2={W - PAD.r} y1={y(t)} y2={y(t)} />
            <text className="axis" x={PAD.l - 8} y={y(t) + 4} textAnchor="end">
              {compact(t)}
            </text>
          </g>
        ))}
        {months.map((m, i) =>
          i % step === 0 ? (
            <text key={m} className="axis" x={x(i)} y={H - 6} textAnchor="middle">
              {shortMonth(m, i)}
            </text>
          ) : null,
        )}
        {bars?.map((v, i) =>
          v > 0 ? (
            <rect
              key={months[i]}
              className="bar-invest"
              x={x(i) - barW / 2}
              y={y(v)}
              width={barW}
              height={y0 - y(v)}
              rx={3}
            />
          ) : null,
        )}
        <path className="area-pos" d={area} clipPath={`url(#above-${id})`} />
        <path className="area-neg" d={area} clipPath={`url(#below-${id})`} />
        {floor != null && <line className="floor" x1={PAD.l} x2={W - PAD.r} y1={y(floor)} y2={y(floor)} />}
        {base && <path className="line base" d={path(base)} />}
        <path className="line" d={path(line)} clipPath={`url(#above-${id})`} />
        <path className="line" d={path(line)} clipPath={`url(#below-${id})`} style={{ stroke: "var(--neg)" }} />
        {markers?.map((on, i) =>
          on ? <circle key={months[i]} className="marker" cx={x(i)} cy={y(line[i])} r={4.5} /> : null,
        )}
        {active !== null && (
          <g>
            <line className="cursor" x1={x(active)} x2={x(active)} y1={PAD.t} y2={H - PAD.b} />
            <circle className={`dot${line[active] < 0 ? " neg" : ""}`} cx={x(active)} cy={y(line[active])} r={5} />
          </g>
        )}
        {months.map((m, i) => (
          <rect
            key={m}
            className="hit"
            x={x(i) - slot / 2}
            y={0}
            width={slot}
            height={H}
            onMouseEnter={() => setActive(i)}
            onMouseLeave={() => {
              // Keep the keyboard position while the chart has focus.
              if (document.activeElement !== box.current) setActive(null);
            }}
          />
        ))}
      </svg>
      <span className="visually-hidden" aria-live="polite">
        {active !== null ? monthLabel(months[active]) : ""}
      </span>
      {active !== null && (
        <div className="tooltip" style={{ left: `${(x(active) / W) * 100}%` }} role="tooltip">
          {tooltip(active)}
        </div>
      )}
    </div>
  );
}
