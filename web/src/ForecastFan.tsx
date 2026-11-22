import {
  Area,
  Bar,
  CartesianGrid,
  ComposedChart,
  Line,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { compact, money, monthLabel } from "./format";
import type { ForecastRow } from "./types";

// Fan chart (Recharts): the forecast month-end cash as a line, inside a shaded
// 80 % range that comes from how far past forecasts missed at the same distance ahead.

export function ForecastFan({ rows, floor }: { rows: ForecastRow[]; floor?: number | null }) {
  const data = rows.map((r) => ({
    month: monthLabel(r.month),
    end: r.end,
    invest: r.toInvest,
    range: [r.low ?? r.end, r.high ?? r.end] as [number, number],
  }));
  return (
    <figure
      className="fan"
      aria-label="Month-end cash forecast with an 80 percent range and money to invest each month"
    >
      <ResponsiveContainer width="100%" height={320}>
        <ComposedChart data={data} margin={{ top: 12, right: 12, bottom: 4, left: 4 }}>
          <CartesianGrid stroke="var(--line)" strokeDasharray="3 3" vertical={false} />
          <XAxis dataKey="month" tick={{ fill: "var(--muted)", fontSize: 14 }} interval="preserveStartEnd" />
          <YAxis tickFormatter={compact} tick={{ fill: "var(--muted)", fontSize: 14 }} width={60} />
          <Tooltip
            contentStyle={{ background: "var(--bg)", border: "1px solid var(--line)", color: "var(--fg)" }}
            formatter={(v: number | [number, number], name: string) =>
              Array.isArray(v) ? [`${money(v[0])} to ${money(v[1])}`, "80 % range"] : [money(v), name]
            }
          />
          <Bar dataKey="invest" fill="var(--invest)" name="To invest" barSize={18} isAnimationActive={false} />
          <Area
            dataKey="range"
            stroke="none"
            fill="var(--accent)"
            fillOpacity={0.18}
            name="80 % range"
            isAnimationActive={false}
          />
          <Line
            dataKey="end"
            stroke="var(--accent)"
            strokeWidth={2.5}
            dot={{ r: 3 }}
            name="Month-end cash"
            isAnimationActive={false}
          />
          {floor != null && <ReferenceLine y={floor} stroke="var(--warn)" strokeDasharray="6 4" />}
          {rows.some((r) => (r.low ?? r.end) < 0) && <ReferenceLine y={0} stroke="var(--neg)" />}
        </ComposedChart>
      </ResponsiveContainer>
    </figure>
  );
}
