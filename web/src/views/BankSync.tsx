import { useCallback, useEffect, useState } from "react";
import { TableWrap } from "../TableWrap";
import { fetchBankSync, runAction } from "../api";
import type { ActionResult, BankSync as BankSyncData, SyncAccount } from "../types";

export interface BankSyncApi {
  load: (user: string) => Promise<BankSyncData>;
  run: (
    user: string,
    name: string,
    values: Record<string, string | boolean>,
    confirm: boolean,
  ) => Promise<ActionResult>;
}

const realApi: BankSyncApi = { load: fetchBankSync, run: runAction };

/** An export older than this many days asks for a fresh download. */
export const STALE_DAYS = 7;
const SHOW = 5;
// "-12.34" from the engine reads as "-$12.34".
const cash = (s: string) => {
  if (s.includes("$")) return s;
  return s.startsWith("-") ? `-$${s.slice(1)}` : `$${s}`;
};

const day = (iso: string | null) =>
  iso
    ? new Date(`${iso}T12:00:00`).toLocaleDateString("en-CA", { weekday: "short", month: "short", day: "numeric" })
    : "—";

/** What one account needs from the owner now, in plain words. */
export function accountState(a: SyncAccount): { label: string; tone: "ok" | "warn" | "accent" | "muted" } {
  if (a.held) return { label: "Held", tone: "warn" };
  if (a.conflicts > 0) return { label: `${a.conflicts} to pick`, tone: "warn" };
  if (a.daysOld !== null && a.daysOld > STALE_DAYS) return { label: `Export ${a.daysOld} days old`, tone: "warn" };
  if (a.add.length > 0) return { label: `${a.add.length} to add`, tone: "accent" };
  if (a.fix.length > 0)
    return { label: a.fix.length === 1 ? "1 amount differs" : `${a.fix.length} amounts differ`, tone: "accent" };
  return { label: "Up to date", tone: "ok" };
}

/** Bank rows per ledger status, in plain words. */
const STATUS_WORDS: Record<string, string> = {
  matched: "in MoneyWiz",
  fix: "amount differs",
  add: "to add",
  sent: "sent",
  conflict: "to pick",
  held: "held",
};

export function ledgerLine(a: SyncAccount): string | null {
  if (!a.ledger) return null;
  const parts = Object.entries(a.ledger.statuses)
    .filter(([, n]) => n > 0)
    .map(([s, n]) => `${n} ${STATUS_WORDS[s] ?? s}`);
  return `Last checked ${day(a.ledger.lastChecked)} · ${parts.join(" · ")}`;
}

function OnlyInMoneywiz({ a }: { a: SyncAccount }) {
  const rows = a.onlyInMoneywizRows ?? [];
  if (rows.length === 0) return null;
  return (
    <details>
      <summary>{rows.length === 1 ? "1 MoneyWiz row has" : `${rows.length} MoneyWiz rows have`} no bank row</summary>
      <TableWrap>
        <table aria-label={`MoneyWiz rows with no bank row in ${a.moneywiz}`}>
          <thead>
            <tr>
              <th scope="col">Date</th>
              <th scope="col">MoneyWiz text</th>
              <th scope="col" className="num">
                Amount
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={`${r.date}-${i}`}>
                <td>{day(r.date)}</td>
                <td>{r.text || <span className="muted">no text</span>}</td>
                <td className="num">{cash(r.amount)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>
    </details>
  );
}

function Rows({ a }: { a: SyncAccount }) {
  const [all, setAll] = useState(false);
  if (a.add.length === 0) return null;
  const rows = all ? a.add : a.add.slice(0, SHOW);
  return (
    <TableWrap>
      <table aria-label={`rows to add to ${a.moneywiz}`}>
        <thead>
          <tr>
            <th scope="col">Date</th>
            <th scope="col">Bank text</th>
            <th scope="col">Category</th>
            <th scope="col" className="num">
              Amount
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={`${r.date}-${r.text}-${i}`}>
              <td>{day(r.date)}</td>
              <td>{r.text}</td>
              <td>{r.category ? r.category.split("/").join(" > ") : <span className="muted">none</span>}</td>
              <td className="num">{cash(r.amount)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {a.add.length > SHOW && (
        <button type="button" className="cat-btn" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show ${a.add.length - SHOW} more`}
        </button>
      )}
    </TableWrap>
  );
}

/** Banks and cards into MoneyWiz: fresh exports, a preview of every row, then one confirmed send. */
export function BankSync({ user, api = realApi }: { user: string; api?: BankSyncApi }) {
  const [data, setData] = useState<BankSyncData | null>(null);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ActionResult | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [yes, setYes] = useState(false);
  const [busy, setBusy] = useState("");

  const load = useCallback(() => {
    setError("");
    api
      .load(user)
      .then(setData)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [user, api]);
  useEffect(() => load(), [load]);

  const act = async (label: string, name: string, values: Record<string, string | boolean>, confirm = false) => {
    setBusy(label);
    setResult(null);
    try {
      setResult(await api.run(user, name, values, confirm));
      load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy("");
      setConfirming(false);
      setYes(false);
    }
  };

  if (error && !data) {
    return (
      <section>
        <h2>Bank sync</h2>
        <p role="alert">{error}</p>
      </section>
    );
  }
  if (!data) {
    return (
      <section>
        <h2>Bank sync</h2>
        <p className="muted">Loading…</p>
      </section>
    );
  }
  if (data.error) {
    return (
      <section>
        <h2>Bank sync</h2>
        <p role="alert">MoneyWiz is not ready: {data.error}</p>
        <p className="muted">
          Set it up with <code>connectors/playbooks/moneywiz.md</code>, then reload this page.
        </p>
      </section>
    );
  }

  const toAdd = data.accounts.reduce((n, a) => n + a.add.length, 0);
  const differ = data.accounts.reduce((n, a) => n + a.fix.length, 0);
  const conflicts = data.accounts.reduce((n, a) => n + a.conflicts, 0);
  const stale = data.accounts.filter((a) => a.daysOld !== null && a.daysOld > STALE_DAYS);
  const gaps = data.coverage.accounts.filter((a) => a.gap);
  const nothing = data.links.toSend === 0;

  return (
    <section className="banksync">
      <h2>Bank sync</h2>
      <p className="lead">
        Bank and card rows go into MoneyWiz for you. You record only cash, and fix a category when MoneyWiz guesses
        wrong.
      </p>

      <div className="cards">
        <div className="card">
          <div className="card-label">Covered, last 12 months</div>
          <div className="card-value">{data.coverage.percent === null ? "—" : `${data.coverage.percent} %`}</div>
        </div>
        <div className="card">
          <div className="card-label">Rows to add</div>
          <div className="card-value">{toAdd}</div>
        </div>
        <div className="card">
          <div className="card-label">Amounts differ</div>
          <div className="card-value">{differ}</div>
        </div>
        <div className="card">
          <div className="card-label">To pick by hand</div>
          <div className="card-value">{conflicts}</div>
        </div>
      </div>

      <ol className="sync-steps">
        <li className="panel">
          <h3>1. Fresh exports</h3>
          {stale.length === 0 ? (
            <p>Every linked export is from the last {STALE_DAYS} days.</p>
          ) : (
            <p>
              Download a new export for {stale.map((a) => a.export).join(", ")}, then collect it. The Services view
              names each bank's steps.
            </p>
          )}
          <button
            type="button"
            className="cat-btn primary"
            disabled={busy !== ""}
            onClick={() => act("collect", "fetch", { action: "collect" })}
          >
            {busy === "collect" ? "Collecting…" : "Collect exports from Downloads"}
          </button>
        </li>

        <li className="panel">
          <h3>2. Check what changes</h3>
          {data.accounts.length === 0 && (
            <p className="muted">No MoneyWiz account follows an export yet. Link one below.</p>
          )}
          {data.accounts.map((a) => {
            const s = accountState(a);
            return (
              <article key={a.moneywiz} className="sync-account" aria-label={a.moneywiz}>
                <header>
                  <strong>{a.moneywiz}</strong> <span className="muted">from {a.export}</span>{" "}
                  <span className={`chip cat-${s.tone}`}>{s.label}</span>
                </header>
                <p className="muted">
                  {day(a.from)} to {day(a.to)} · {a.matched} of {a.bankRows} bank rows already in MoneyWiz · {a.balance}
                </p>
                {ledgerLine(a) && <p className="muted">{ledgerLine(a)}</p>}
                {a.held && (
                  <p role="note">Held: {a.held}. Nothing is sent for this account until the plan looks right.</p>
                )}
                {(a.reconcile ?? 0) > 0 && (
                  <p className="muted">
                    {a.reconcile} reconcile {a.reconcile === 1 ? "row gets" : "rows get"} an offset: the bank rows they
                    stood in for are added.
                  </p>
                )}
                {(a.standIn ?? 0) > 0 && (
                  <p className="muted">
                    {a.standIn} {a.standIn === 1 ? "row" : "rows"} with no bank row {a.standIn === 1 ? "gets" : "get"}{" "}
                    an offset: they stood in for the bank rows that are added, and the balance stays right.
                  </p>
                )}
                <Rows a={a} />
                <OnlyInMoneywiz a={a} />
                {a.fix.length > 0 && (
                  <p>
                    {a.fix.length === 1 ? "1 amount differs" : `${a.fix.length} amounts differ`} from the bank.{" "}
                    {data.mode === "list"
                      ? "They stay on your fix list."
                      : "The sync corrects them (memo “cashcast fix”)."}
                  </p>
                )}
                <button
                  type="button"
                  className="cat-btn"
                  disabled={busy !== ""}
                  onClick={() => act(`resync-${a.moneywiz}`, "moneywiz", { action: "resync", account: a.moneywiz })}
                >
                  {busy === `resync-${a.moneywiz}` ? "Asking…" : "Check this account again"}
                </button>
              </article>
            );
          })}
          {data.untouched.length > 0 && <p className="muted">You record these by hand: {data.untouched.join(", ")}.</p>}
        </li>

        <li className="panel">
          <h3>3. Send to MoneyWiz</h3>
          {nothing ? (
            <p>Nothing to send. MoneyWiz matches your exports.</p>
          ) : (
            <p>
              {data.links.toSend === 1 ? "1 change is" : `${data.links.toSend} changes are`} ready. MoneyWiz opens in
              the background and saves each one.
            </p>
          )}
          {data.links.waiting > 0 && (
            <p className="muted">{data.links.waiting} were sent today already; they go again tomorrow if missing.</p>
          )}
          {(data.links.held ?? 0) > 0 && (
            <p className="muted">{data.links.held} changes wait in held accounts and are not sent.</p>
          )}
          {data.links.gone > 0 && (
            <p className="muted">
              {data.links.gone} were sent earlier but MoneyWiz no longer holds them (deleted there?). They stay out.
            </p>
          )}
          {!confirming ? (
            <button
              type="button"
              className="cat-btn primary"
              disabled={nothing || busy !== ""}
              onClick={() => setConfirming(true)}
            >
              Send to MoneyWiz
            </button>
          ) : (
            <fieldset className="cat-confirm" aria-label="confirm send">
              <p>This adds {data.links.toSend} changes to MoneyWiz. Undo means deleting them in MoneyWiz.</p>
              <label className="cat-check">
                <input type="checkbox" checked={yes} onChange={(e) => setYes(e.target.checked)} /> Yes, add them to
                MoneyWiz
              </label>
              <button
                type="button"
                className="cat-btn primary"
                disabled={!yes || busy !== ""}
                onClick={() => act("send", "moneywiz", { action: "sync" }, true)}
              >
                {busy === "send" ? "Sending…" : "Send now"}
              </button>
            </fieldset>
          )}
        </li>
      </ol>

      {(data.runs ?? []).length > 0 && (
        <details className="panel">
          <summary>Sync history</summary>
          <TableWrap>
            <table aria-label="sync history">
              <thead>
                <tr>
                  <th scope="col">When</th>
                  <th scope="col">Kind</th>
                  <th scope="col" className="num">
                    Sent
                  </th>
                  <th scope="col" className="num">
                    Held accounts
                  </th>
                </tr>
              </thead>
              <tbody>
                {(data.runs ?? []).map((r) => (
                  <tr key={r.at + String(r.apply)}>
                    <td>
                      {day(r.at.slice(0, 10))} {r.at.slice(11)}
                    </td>
                    <td>{r.apply ? "Sync" : "Check"}</td>
                    <td className="num">{r.sent}</td>
                    <td className="num">{r.held}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </TableWrap>
          {(data.recheck ?? []).length > 0 && (
            <p className="muted">
              The next sync checks again:{" "}
              {(data.recheck ?? [])
                .map((w) => `${w.account ?? "every account"} ${w.from ?? "start"}–${w.to ?? "today"}`)
                .join(", ")}
              .
            </p>
          )}
        </details>
      )}

      {error && <p role="alert">{error}</p>}
      {result && (
        <pre className="action-output" aria-label="last result">
          {result.output}
        </pre>
      )}

      {(data.coverage.suggestions.length > 0 || data.unlinkedExports.length > 0 || gaps.length > 0) && (
        <details className="panel">
          <summary>Accounts that do not sync yet</summary>
          {data.coverage.suggestions.length > 0 && (
            <>
              <p>Add these links under [moneywiz.accounts] in your cashcast.toml:</p>
              <pre>{data.coverage.suggestions.map((s) => `"${s.moneywiz}" = "${s.export}"`).join("\n")}</pre>
            </>
          )}
          {data.unlinkedExports.length > 0 && (
            <p>Exports with no MoneyWiz account: {data.unlinkedExports.join(", ")}.</p>
          )}
          {gaps.length > 0 && (
            <ul>
              {gaps.map((g) => (
                <li key={g.name}>
                  {g.name}: {g.gap}
                </li>
              ))}
            </ul>
          )}
        </details>
      )}
    </section>
  );
}
