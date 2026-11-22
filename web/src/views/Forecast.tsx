import { ForecastFan } from "../ForecastFan";
import { TableWrap } from "../TableWrap";
import { money, monthLabel, pct, sign } from "../format";
import type { Plan } from "../types";
import { SectionText } from "./SectionView";

export function Forecast({ plan }: { plan: Plan }) {
  const thisMonth = plan.today.slice(0, 7);
  const rows = plan.forecast.filter((r) => r.month >= thisMonth);
  const acc = plan.forecastAccuracy;
  const floor = plan.headline.investAbove;
  // The 80 % range comes from past forecast misses; with too little history there is none to show.
  const hasRange = rows.some((r) => r.low != null && r.high != null);
  return (
    <section>
      <div className="page-head">
        <h2>Balance forecast</h2>
        {rows.length > 0 && rows.every((r) => r.income === 0) && (
          <p className="banner">
            No pay in the forecast yet, so every month looks short. Add a bank export that shows your pay deposits to
            your inbox, then run <code>bin/cashcast import</code>.
          </p>
        )}
        <p className="lead">
          Cash at the end of each month for the next {rows.length} {rows.length === 1 ? "month" : "months"}.{" "}
          {hasRange
            ? "The shaded band is the 80 % range: in past months, the real month-end cash landed inside a band this wide 8 times out of 10."
            : "A likely range shows here once there are a few months of history to check the forecast against."}
        </p>
        {acc && (
          <p className="lead accuracy">
            Accuracy on past months: <b>{pct(acc.netQuarterly, 0)}</b> for cash flow over 3 months,{" "}
            <b>{pct(acc.monthEnd, 0)}</b> for month-end cash, <b>{pct(acc.netMonthly, 0)}</b> for single-month cash
            flow.
          </p>
        )}
      </div>
      {rows.length > 0 && (
        <div className="panel">
          <ForecastFan rows={rows} floor={floor} />
          <p className="legend">
            <span>
              <span className="swatch end" />
              month-end cash
            </span>
            {hasRange && (
              <span>
                <span className="swatch range" />
                80 % range
              </span>
            )}
            <span>
              <span className="swatch invest" />
              to invest
            </span>
            {floor != null && (
              <span>
                <span className="swatch floor" />
                cash floor {money(floor)}
              </span>
            )}
          </p>
        </div>
      )}
      <TableWrap>
        <table>
          <thead>
            <tr>
              <th>Month</th>
              <th className="num">Cheques</th>
              <th className="num">Income</th>
              <th className="num">Expenses</th>
              <th className="num">Net</th>
              <th className="num">To invest</th>
              <th className="num">Month end</th>
              {hasRange && <th className="num">Likely range (80 %)</th>}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.month}>
                <td>{monthLabel(r.month)}</td>
                <td className="num">{r.paycheques}</td>
                <td className="num">{money(r.income)}</td>
                <td className="num">{money(r.expenses)}</td>
                <td className={`num ${sign(r.net) ?? ""}`}>{money(r.net)}</td>
                <td className="num">{money(r.toInvest)}</td>
                <td
                  className={`num ${sign(r.end) ?? (floor != null && r.end < floor ? "low" : "")}`}
                  title={floor != null && r.end < floor ? "under your cash floor" : undefined}
                >
                  <b>{money(r.end)}</b>
                </td>
                {hasRange && (
                  <td className="num">
                    {r.low != null && r.high != null ? `${money(r.low)} to ${money(r.high)}` : "—"}
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>
      <div className="panel md-panel">
        <SectionText plan={plan} id="forecast" />
      </div>
    </section>
  );
}
