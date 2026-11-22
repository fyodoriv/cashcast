import { Chart } from "../Chart";
import { TaskList } from "../Tasks";
import { money, monthLabel, pct, sign, weekdayLabel } from "../format";
import type { NetTotal, Plan } from "../types";

interface Tile {
  label: string;
  value: string;
  sub?: string;
  tone?: string;
  view: string;
}

const PARTS: [string, string][] = [
  ["cash", "Cash"],
  ["investment", "Investments"],
  ["crypto", "Crypto"],
  ["other", "Other"],
  ["debt", "Debts"],
];

/** The main number, first: the net total, its trend, and the best option inside the owner's boundaries. */
function NetTotalHero({ nt, go }: { nt: NetTotal; go: (view: string) => void }) {
  const best = nt.ranked.find((o) => o.netEffect > 0);
  const gaps = nt.stale.length + nt.missing.length;
  // The engine writes "Action (timing) (range; caveat)": show the action alone, the rest as a quiet note.
  const cut = best ? best.title.indexOf(" (") : -1;
  const action = best && cut > 0 ? best.title.slice(0, cut) : (best?.title ?? "");
  const groups = best && cut > 0 ? [...best.title.slice(cut).matchAll(/\(([^)]*)\)/g)].map((m) => m[1]) : [];
  const note = groups.length ? `${groups.join(" · ").replace(/^./, (c) => c.toUpperCase())}.` : "";
  return (
    <div className="panel net-hero" aria-labelledby="net-total-label">
      <div className="panel-head">
        <h3 id="net-total-label">Net total</h3>
        <button type="button" className="link" onClick={() => go("net")}>
          All options
        </button>
      </div>
      <p className={`net-value${nt.total < 0 ? " neg" : ""}`}>{money(nt.total)}</p>
      <p className="net-trend">
        {nt.changes.length === 0
          ? "First point saved. The trend shows after 30 days."
          : nt.changes.map((c) => (
              <span key={c.days} className={`chip${c.change <= -0.005 ? " neg" : c.change >= 0.005 ? " pos" : ""}`}>
                {c.change >= 0.005 ? "+" : ""}
                {money(c.change)} in {c.days} days
              </span>
            ))}
      </p>
      <ul className="net-parts" aria-label="Parts of the net total">
        {PARTS.filter(([k]) => nt.byKind[k] !== undefined).map(([k, label]) => (
          <li key={k}>
            <span className="muted">{label}</span> <span className={sign(nt.byKind[k])}>{money(nt.byKind[k])}</span>
          </li>
        ))}
      </ul>
      {best ? (
        <div className="net-best">
          <p className="net-best-label muted">Best next step inside your boundaries</p>
          <p className="net-best-action">
            <b>{action}</b>
            {best.cashOut ? ` (${money(best.cashOut)})` : ""}
          </p>
          <p>
            About <b className="pos">+{money(best.netEffect)}</b> a year.
          </p>
          {note && <p className="muted">{note}</p>}
        </div>
      ) : (
        <p className="net-best muted">No option inside your boundaries adds to the net total right now.</p>
      )}
      {(nt.refused.length > 0 || gaps > 0) && (
        <p className="muted">
          {nt.refused.length > 0 &&
            `${nt.refused.length} ${nt.refused.length === 1 ? "option is" : "options are"} outside your boundaries. `}
          {gaps > 0 && `${gaps} ${gaps === 1 ? "source is" : "sources are"} stale or not counted yet.`}
        </p>
      )}
    </div>
  );
}

/** Home: the net total, then alerts, the four numbers that matter, the balance ahead and what to do next (YNAB "Home"). */
export function Overview({ plan, go }: { plan: Plan; go: (view: string) => void }) {
  const h = plan.headline;
  const rows = plan.forecast;
  // Cash that goes below zero later makes "can invest" a trap: say so on both tiles.
  const dips = (h.lowestMonthEnd ?? 0) < 0;
  const firstNeg = rows.find((r) => r.end < 0)?.month ?? h.lowestMonth;
  const noPay = rows.length > 0 && rows.every((r) => r.income === 0);
  const invest: Tile = {
    label: `Can invest over ${rows.length} months`,
    value: money(h.canInvest),
    sub:
      dips && firstNeg
        ? `only once cash covers ${monthLabel(firstNeg)}; invest nothing before that`
        : "above your cash floor",
    view: "plan",
  };
  const low: Tile = {
    label: "Lowest month-end cash",
    value: money(h.lowestMonthEnd),
    sub: h.lowestMonth
      ? `${monthLabel(h.lowestMonth)}${dips ? (noPay ? " · no pay in the forecast yet" : " · below zero") : ""}`
      : undefined,
    tone: sign(h.lowestMonthEnd),
    view: "forecast",
  };
  // When cash goes below zero, the low month leads: it decides whether investing is safe.
  const primary: Tile[] = [
    ...(dips ? [low, invest] : [invest, low]),
    { label: "Net cash back a year", value: money(h.netCashBack), sub: "after card fees", view: "cashback" },
    {
      label: "Emergency fund target",
      value: money(h.emergencyFund),
      sub: "how much to keep for emergencies",
      view: "reserve",
    },
  ];
  const secondary: Tile[] = [
    { label: "Invest cash above", value: money(h.investAbove), view: "reserve" },
    { label: "Marginal tax rate", value: pct(h.marginalRate), view: "tax" },
  ];
  const next = plan.tasks.filter((t) => t.due).slice(0, 6);
  const tile = (c: Tile) => (
    <button type="button" className="card" key={c.label} onClick={() => go(c.view)}>
      <span className="card-label">{c.label}</span>
      <span className={`card-value${c.tone ? ` ${c.tone}` : ""}`}>{c.value}</span>
      {c.sub && <span className="card-sub">{c.sub}</span>}
    </button>
  );
  return (
    <section>
      <div className="page-head">
        <h2>Home</h2>
        <span className="muted">
          Plan for {plan.user.display}, made {weekdayLabel(plan.today)}
        </span>
      </div>
      {h.pivot && (
        <p className="banner" role="alert">
          Your last two months missed the forecast by more than 15%. Re-check the plan in the monthly review.
        </p>
      )}
      {noPay && (
        <p className="banner">
          No pay in the forecast yet, so every month looks short. Add a bank export that shows your pay deposits to your
          inbox, then run <code>bin/cashcast import</code>.
        </p>
      )}
      {/* What needs you comes first (an overdue task above every number). */}
      <div className="panel next-up">
        <div className="panel-head">
          <h3>Next up</h3>
          <button type="button" className="link" onClick={() => go("plan")}>
            Open plan ahead
          </button>
        </div>
        <TaskList tasks={next} today={plan.today} empty="Nothing dated yet." />
        <p className="muted">Next monthly review: {h.nextReview ? weekdayLabel(h.nextReview) : "—"}.</p>
      </div>
      {plan.netTotal && <NetTotalHero nt={plan.netTotal} go={go} />}
      <div className="cards">{primary.map(tile)}</div>
      <div className="panel">
        <div className="panel-head">
          <h3>Cash ahead</h3>
          <button type="button" className="link" onClick={() => go("forecast")}>
            Open forecast
          </button>
        </div>
        {rows.length > 0 ? (
          <Chart
            months={rows.map((r) => r.month)}
            line={rows.map((r) => r.end)}
            floor={h.investAbove}
            label="Month-end cash for the months ahead"
            height={200}
            tooltip={(i) => (
              <>
                <b>{monthLabel(rows[i].month)}</b>
                Month end {money(rows[i].end)}
              </>
            )}
          />
        ) : (
          <p className="muted">No forecast yet: add exports to your inbox and run bin/cashcast import.</p>
        )}
      </div>
      <div className="cards secondary">{secondary.map(tile)}</div>
    </section>
  );
}
