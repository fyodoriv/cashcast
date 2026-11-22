import { TableWrap } from "../TableWrap";
import { friendlyDates, money } from "../format";
import type { Plan } from "../types";

export function HoldingsView({ plan }: { plan: Plan }) {
  const h = plan.holdings;
  if (!h) {
    return (
      <section>
        <h2>Holdings</h2>
        <p>
          No investment or crypto exports yet. Put broker or wallet exports (Wealthsimple activity, E*TRADE, Kraken,
          Casa) in your inbox and run <code>bin/cashcast holdings</code>.
        </p>
      </section>
    );
  }
  return (
    <section>
      <h2>Holdings</h2>
      <TableWrap>
        <table>
          <thead>
            <tr>
              <th>Asset</th>
              <th>Account</th>
              <th className="num">Quantity</th>
              <th className="num">Cost base (CAD)</th>
            </tr>
          </thead>
          <tbody>
            {h.assets.map((a) => (
              <tr key={`${a.symbol}-${a.account_bucket}`}>
                <td>{a.symbol}</td>
                <td>{a.account_bucket}</td>
                <td className="num">{a.quantity}</td>
                <td className="num">{a.cost_unknown ? "cost unknown" : money(a.acb_cad)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>
      {(h.checks ?? []).length > 0 && (
        <>
          <h3>Checks</h3>
          <ul>
            {(h.checks ?? []).map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
        </>
      )}
      <h3>Realized gains by tax year</h3>
      <ul>
        {(h.realized_by_year ?? []).map((g) => (
          <li key={g.year}>
            {g.year}: net {money(g.net_cad)} ({g.sales} sales)
          </li>
        ))}
        {(h.realized_by_year ?? []).length === 0 && <li>None.</li>}
      </ul>
      {(h.superficial_loss_flags ?? []).length > 0 && (
        <p className="banner">
          Possible superficial losses:{" "}
          {(h.superficial_loss_flags ?? []).map((f) => `${f.asset} ${f.year} ×${f.count}`).join(", ")}
        </p>
      )}
      {(h.needs_cost ?? []).length > 0 && (
        <>
          <h3>Needs a cost</h3>
          <ul>
            {(h.needs_cost ?? []).map((n) => (
              <li key={`${n.asset}-${n.reason}`}>
                {n.asset} ({n.account_bucket}): {n.reason}
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

export function ServicesView({ plan }: { plan: Plan }) {
  return (
    <section>
      <div className="page-head">
        <h2>Services</h2>
        <p className="lead">
          Read actions an agent may do; settings changes need your yes; money actions are always yours. Details:{" "}
          <code>bin/cashcast connectors show SERVICE</code>.
        </p>
        <ul className="legend action-list" aria-label="What each dot means">
          <li className="risk-read">
            <span className="dot" aria-hidden="true" />
            an agent may do it
          </li>
          <li className="risk-settings">
            <span className="dot" aria-hidden="true" />
            needs your yes
          </li>
          <li className="risk-money">
            <span className="dot" aria-hidden="true" />
            only you do it
          </li>
        </ul>
      </div>
      <TableWrap>
        <table>
          <thead>
            <tr>
              <th>Service</th>
              <th>Import</th>
              <th>API</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {plan.connectors.map((c) => (
              <tr key={c.id}>
                <td>{c.name}</td>
                <td className="import-cell">
                  {c.formats.length ? (
                    c.formats.join(", ")
                  ) : (
                    <span>
                      No export. <span className="muted">{c.noExport?.replace(/^[^.]*\.\s*/, "")}</span>
                    </span>
                  )}
                </td>
                <td>{c.api ?? "no"}</td>
                <td>
                  <ul className="action-list">
                    {c.actions.map((a) => (
                      <li key={a.id} className={`risk-${a.risk}`}>
                        <span className="dot" aria-hidden="true" />
                        {a.title}
                        {a.risk !== "read" && (
                          <span className="risk-word"> ({a.risk === "money" ? "only you" : "needs your yes"})</span>
                        )}
                      </li>
                    ))}
                  </ul>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>
    </section>
  );
}

export function KnowledgeView({ plan }: { plan: Plan }) {
  const k = plan.knowledge;
  return (
    <section>
      <div className="page-head">
        <h2>Knowledge</h2>
        <p className="lead">
          {k.facts} sourced facts in {k.files} files; {k.estimates} are estimates to verify and {k.heuristics} are rules
          of thumb. Stale: {k.stale.length ? k.stale.join(", ") : "none"}.
        </p>
      </div>
      {k.problems.length > 0 && <p className="banner">Problems: {k.problems.join("; ")}</p>}
      <TableWrap>
        <table>
          <thead>
            <tr>
              <th>Topic</th>
              <th>Checked</th>
              <th>Review by</th>
            </tr>
          </thead>
          <tbody>
            {k.topics.map((t) => (
              <tr key={t.name}>
                <td>{t.title}</td>
                <td>{friendlyDates(t.asOf)}</td>
                <td>{friendlyDates(t.reviewBy)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>
    </section>
  );
}

export function PrivacyView({ plan }: { plan: Plan }) {
  return (
    <section>
      <div className="page-head">
        <h2>Privacy</h2>
        <p className="lead">
          Your data stays on this computer. Each line below is one way something can leave it, and only after you grant
          it in a terminal: <code>bin/cashcast consent grant SCOPE</code>.
        </p>
      </div>
      <TableWrap>
        <table>
          <thead>
            <tr>
              <th>Scope</th>
              <th>What leaves</th>
              <th>To</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {plan.consent.map((c) => (
              <tr key={c.scope}>
                <td>
                  <code>{c.scope}</code>
                </td>
                <td>{c.shares}</td>
                <td>{c.recipient}</td>
                <td className="nowrap">
                  {c.grantedUntil ? `granted until ${friendlyDates(c.grantedUntil)}` : "not granted"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>
    </section>
  );
}
