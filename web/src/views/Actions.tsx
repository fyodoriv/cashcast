import { useEffect, useState } from "react";
import { fetchActions, runAction } from "../api";
import { Markdown } from "../markdown";
import type { Action, ActionArg, ActionResult } from "../types";

export interface ActionsApi {
  list: () => Promise<Action[]>;
  run: (
    user: string,
    name: string,
    values: Record<string, string | boolean>,
    confirm: boolean,
  ) => Promise<ActionResult>;
}

const realApi: ActionsApi = { list: fetchActions, run: runAction };

function Field({ arg, value, set }: { arg: ActionArg; value: string | boolean; set: (v: string | boolean) => void }) {
  const label = `${arg.name.replace(/_/g, " ")}${arg.required ? "" : " (optional)"}`;
  if (arg.kind === "flag") {
    return (
      <label className="action-field">
        <input type="checkbox" checked={value === true} onChange={(e) => set(e.target.checked)} /> {label}
        {arg.help && <span className="muted"> ({arg.help})</span>}
      </label>
    );
  }
  if (arg.kind === "choice" && arg.choices) {
    return (
      <label className="action-field">
        {label}{" "}
        <select aria-label={arg.name} value={String(value)} onChange={(e) => set(e.target.value)}>
          {!arg.required && <option value="">none</option>}
          {arg.choices.map((c) => (
            <option key={c} value={c}>
              {c}
            </option>
          ))}
        </select>
      </label>
    );
  }
  return (
    <label className="action-field">
      {label}{" "}
      <input
        aria-label={arg.name}
        type={arg.kind === "int" ? "number" : "text"}
        value={String(value)}
        placeholder={arg.help}
        onChange={(e) => set(e.target.value)}
      />
    </label>
  );
}

function initial(a: Action): Record<string, string | boolean> {
  return Object.fromEntries(
    a.args.map((x) => [
      x.name,
      x.kind === "flag" ? false : x.default != null ? String(x.default) : x.required && x.choices ? x.choices[0] : "",
    ]),
  );
}

/** Show `backtick` spans from engine text as code, not raw backticks. */
function ticks(text: string) {
  return text.split(/`([^`]+)`/).map((part, i) => (i % 2 ? <code key={`${i}-${part}`}>{part}</code> : part));
}

/** One action: a form generated from its CLI arguments, or the reason it runs in a terminal only. */
function ActionCard({ action, user, api }: { action: Action; user: string; api: ActionsApi }) {
  const [values, setValues] = useState(() => initial(action));
  const [confirmed, setConfirmed] = useState(false);
  const [result, setResult] = useState<ActionResult | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  if (action.terminal_only) {
    return (
      <li className="action">
        <strong>{action.name}</strong>: {ticks(action.help.replace(/ \(also: [^)]*\)/, ""))}
        <p className="muted">
          Terminal only: {ticks(action.terminal_only)}.
          {/* Name the command once: skip it when the reason already shows a command. */}
          {!action.terminal_only.includes("`") && (
            <>
              {" "}
              Run <code>{action.command}</code>
            </>
          )}
        </p>
      </li>
    );
  }
  const choice = String(values.action ?? "");
  const terminalChoice = action.terminal_only_choices[choice];
  const needsConfirm =
    action.confirm ?? action.confirm_choices[choice] ?? (values.yes === true ? "changes something hard to undo" : null);
  const go = async () => {
    setBusy(true);
    setError("");
    try {
      setResult(await api.run(user, action.name, values, confirmed));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setConfirmed(false); // a yes covers one run only
    }
  };
  return (
    <li className="action">
      <strong>{action.name}</strong>: {ticks(action.help.replace(/ \(also: [^)]*\)/, ""))}
      <div className="action-form">
        {action.args.map((a) => (
          <Field
            key={a.name}
            arg={a}
            value={values[a.name]}
            set={(v) => {
              setValues({ ...values, [a.name]: v });
              if (a.name === "action") setConfirmed(false); // a yes covers one choice only
            }}
          />
        ))}
        {terminalChoice ? (
          <p className="muted">
            {choice} runs in a terminal only: {ticks(terminalChoice)}. Run <code>{`${action.command} ${choice}`}</code>
          </p>
        ) : (
          <>
            {needsConfirm && (
              <label className="action-field">
                <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} /> Yes, this{" "}
                {needsConfirm}
              </label>
            )}
            <button type="button" className="primary" disabled={busy || (!!needsConfirm && !confirmed)} onClick={go}>
              {busy ? "Running…" : `Run ${action.name}`}
            </button>
          </>
        )}
      </div>
      {error && <p role="alert">{error}</p>}
      {result && (
        <div className="action-output" aria-label={`${action.name} output`}>
          <code className="muted">{`$ ${result.argv.join(" ")}  (exit ${result.exit})`}</code>
          <div className="md">
            <Markdown text={result.output} />
          </div>
        </div>
      )}
    </li>
  );
}

/** Every CLI command, runnable here; the same list agents read with `bin/cashcast actions`. */
export function Actions({ user, api = realApi }: { user: string; api?: ActionsApi }) {
  const [list, setList] = useState<Action[] | null>(null);
  const [error, setError] = useState("");
  const [filter, setFilter] = useState("");
  useEffect(() => {
    api
      .list()
      .then(setList)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [api]);
  const shown = (list ?? [])
    .filter((a) => `${a.name} ${a.help}`.toLowerCase().includes(filter.toLowerCase()))
    // Actions with a form come first; terminal-only ones go last.
    .sort((a, b) => Number(Boolean(a.terminal_only)) - Number(Boolean(b.terminal_only)));
  return (
    <section>
      <h2>Actions</h2>
      <p>
        Every <code>bin/cashcast</code> command, with a form. Agents read the same list with{" "}
        <code>bin/cashcast actions</code>. Money is never moved here: those steps stay in your reminders.
      </p>
      <input
        className="actions-filter"
        aria-label="filter actions"
        placeholder="Filter"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      {error && <p role="alert">{error}</p>}
      {list === null && !error && <p className="muted">Loading…</p>}
      <ul className="actions">
        {shown.map((a) => (
          <ActionCard key={a.name} action={a} user={user} api={api} />
        ))}
      </ul>
    </section>
  );
}
