import { useMemo, useState } from "react";
import { Chart } from "../Chart";
import { TaskList } from "../Tasks";
import { money, monthLabel, monthOf } from "../format";
import type { ForecastRow, Plan, PlanTask } from "../types";

export interface WhatIf {
  spendingChange: number; // -0.3 … +0.3 of monthly expenses
  oneOff: number; // a one-off cost (or, if negative, income)
  oneOffMonth: string; // YYYY-MM
}

export interface PlannedMonth extends ForecastRow {
  delta: number; // extra spending this month from the what-if
  headroom: number; // investable cash so far after the what-if; below 0 means under the floor
}

/** Applies a what-if to the forecast. Cash above the floor is what the model invests, so a cost first
 * eats into "to invest"; once the running total is negative the person would dip under their floor. */
export function applyWhatIf(
  rows: ForecastRow[],
  w: WhatIf,
): { months: PlannedMonth[]; canInvest: number; firstShortfall: string | null } {
  let cumInvest = 0;
  let cumDelta = 0;
  let firstShortfall: string | null = null;
  const months = rows.map((r) => {
    const delta = r.expenses * w.spendingChange + (r.month === w.oneOffMonth ? w.oneOff : 0);
    cumInvest += r.toInvest;
    cumDelta += delta;
    const headroom = cumInvest - cumDelta;
    if (headroom < 0 && firstShortfall === null) firstShortfall = r.month;
    return { ...r, delta, headroom };
  });
  return { months, canInvest: Math.max(0, cumInvest - cumDelta), firstShortfall };
}

export function PlanAhead({ plan }: { plan: Plan }) {
  const first = plan.forecast[0]?.month ?? plan.today.slice(0, 7);
  const [w, setW] = useState<WhatIf>({ spendingChange: 0, oneOff: 0, oneOffMonth: first });
  const result = useMemo(() => applyWhatIf(plan.forecast, w), [plan.forecast, w]);
  const base = plan.forecast.reduce((s, r) => s + r.toInvest, 0);
  const changed = w.spendingChange !== 0 || w.oneOff !== 0;
  const byMonth = new Map<string, PlanTask[]>();
  const undated: PlanTask[] = [];
  for (const t of plan.tasks) {
    if (!t.due) undated.push(t);
    else byMonth.set(monthOf(t.due), [...(byMonth.get(monthOf(t.due)) ?? []), t]);
  }
  const earlyTasks = plan.tasks.filter((t) => t.due && monthOf(t.due) < first);
  let running = 0;
  const baseline = plan.forecast.map((r) => {
    running += r.toInvest;
    return running;
  });
  const diff = result.canInvest - base;
  return (
    <section>
      <div className="page-head">
        <h2>Plan ahead</h2>
        <p className="lead">
          Month by month: what comes in, what goes out, what you can invest, and what to do. Try a change before you
          make it.
        </p>
      </div>
      <fieldset className="whatif">
        <legend>What if…</legend>
        <label>
          Monthly spending{" "}
          <b className="num">
            {w.spendingChange >= 0 ? "+" : ""}
            {Math.round(w.spendingChange * 100)}%
          </b>
          <input
            type="range"
            min={-30}
            max={30}
            step={5}
            value={Math.round(w.spendingChange * 100)}
            onChange={(e) => setW({ ...w, spendingChange: Number(e.target.value) / 100 })}
            aria-label="monthly spending change"
          />
        </label>
        <label>
          One-off cost
          <input
            type="number"
            step={100}
            value={w.oneOff}
            onChange={(e) => setW({ ...w, oneOff: Number(e.target.value) || 0 })}
            aria-label="one-off cost"
          />
        </label>
        <label>
          in
          <select
            value={w.oneOffMonth}
            onChange={(e) => setW({ ...w, oneOffMonth: e.target.value })}
            aria-label="one-off month"
          >
            {plan.forecast.map((r) => (
              <option key={r.month} value={r.month}>
                {monthLabel(r.month)}
              </option>
            ))}
          </select>
        </label>
        {changed && (
          <button
            type="button"
            className="link"
            onClick={() => setW({ spendingChange: 0, oneOff: 0, oneOffMonth: first })}
          >
            Reset
          </button>
        )}
        <output className="whatif-result">
          You can invest <strong>{money(result.canInvest)}</strong> over {plan.forecast.length} months
          {changed && diff !== 0 && <span className={`delta${diff > 0 ? " up" : ""}`}>{money(diff)}</span>}
          {changed ? ` (now ${money(base)})` : ""}.
          {result.firstShortfall && (
            <span className="neg"> Your cash would dip under your floor in {monthLabel(result.firstShortfall)}.</span>
          )}
          {!result.firstShortfall && (plan.headline.lowestMonthEnd ?? 0) < 0 && plan.headline.lowestMonth && (
            <span className="neg">
              {" "}
              But cash first goes below zero in{" "}
              {monthLabel(plan.forecast.find((r) => r.end < 0)?.month ?? plan.headline.lowestMonth)}: cover that before
              you invest.
            </span>
          )}
        </output>
      </fieldset>
      {plan.forecast.length > 0 && (
        <div className="panel">
          <div className="panel-head">
            <h3>Cash above your floor at each month's end</h3>
          </div>
          <Chart
            months={result.months.map((m) => m.month)}
            line={result.months.map((m) => m.headroom)}
            base={changed ? baseline : undefined}
            markers={result.months.map((m) => (byMonth.get(m.month) ?? []).length > 0)}
            label="Investable cash above your floor, month by month, with the months that have tasks"
            height={190}
            tooltip={(i) => {
              const m = result.months[i];
              const n = (byMonth.get(m.month) ?? []).length;
              return (
                <>
                  <b>{monthLabel(m.month)}</b>
                  Investable so far {money(m.headroom)}
                  {changed && (
                    <>
                      <br />
                      Without the change {money(baseline[i])}
                    </>
                  )}
                  {n > 0 && (
                    <>
                      <br />
                      {n} {n === 1 ? "task" : "tasks"}
                    </>
                  )}
                </>
              );
            }}
          />
          <p className="legend">
            <span>
              <span className="swatch end" />
              investable cash{result.firstShortfall ? " (red: under your floor)" : ""}
            </span>
            {changed && (
              <span>
                <span className="swatch base" />
                without the change
              </span>
            )}
            <span>
              <span className="swatch task" />
              month with tasks
            </span>
          </p>
        </div>
      )}
      {earlyTasks.length > 0 && (
        <div className="panel">
          <h3>Overdue or this month</h3>
          <TaskList tasks={earlyTasks} today={plan.today} />
        </div>
      )}
      <ol className="timeline">
        {result.months.map((m) => {
          const tasks = byMonth.get(m.month) ?? [];
          const cls = [m.headroom < 0 ? "short" : "", tasks.length === 0 ? "quiet" : ""].filter(Boolean).join(" ");
          return (
            <li key={m.month} className={cls || undefined}>
              <div className="tl-head">
                <h3>{monthLabel(m.month)}</h3>
                <span className="tl-figures">
                  <span className="tl-in">
                    in <b>{money(m.income)}</b>
                  </span>
                  <span className="tl-out">
                    out <b>{money(m.expenses + m.delta)}</b>
                  </span>
                  <span className="tl-invest">
                    invest <b>{money(Math.max(0, m.toInvest - Math.max(0, m.delta)))}</b>
                  </span>
                </span>
              </div>
              {tasks.length > 0 && (
                <div className="tl-tasks">
                  <TaskList tasks={tasks} today={plan.today} />
                </div>
              )}
            </li>
          );
        })}
      </ol>
      {undated.length > 0 && (
        <div className="panel">
          <h3>When the date is known</h3>
          <TaskList tasks={undated} today={plan.today} />
        </div>
      )}
      {plan.forecast.length === 0 && <p>No forecast yet: add exports to your inbox and run bin/cashcast import.</p>}
    </section>
  );
}
