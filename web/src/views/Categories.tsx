import { useCallback, useEffect, useState } from "react";
import { TableWrap } from "../TableWrap";
import { fetchCategories, runAction } from "../api";
import type { ActionResult, CategoryOp, MoneyWizCategories, MoneyWizCategory } from "../types";

export interface CategoriesApi {
  load: (user: string) => Promise<MoneyWizCategories>;
  run: (
    user: string,
    name: string,
    values: Record<string, string | boolean>,
    confirm: boolean,
  ) => Promise<ActionResult>;
}

const realApi: CategoriesApi = { load: fetchCategories, run: runAction };

type Mode = "keep" | "rename" | "merge" | "map";
export interface Pick {
  mode: Mode;
  value: string;
}

const parent = (path: string) => path.split("/").slice(0, -1).join("/");
const leaf = (path: string) => path.split("/").pop() ?? path;

/** The migration ops for what the owner picked: adds first, then renames and suggestions, then merges. */
export function buildOps(
  data: MoneyWizCategories,
  picks: Record<string, Pick>,
  adds: string[],
  accepted: number[],
): CategoryOp[] {
  const first: CategoryOp[] = [];
  const middle: CategoryOp[] = [];
  const last: CategoryOp[] = [];
  const added = new Set<string>();
  const add = (path: string, kind: string) => {
    if (added.has(path) || data.categories.some((c) => c.path === path)) return;
    added.add(path);
    first.push(path.includes("/") ? { add: path } : { add: path, kind: kind === "income" ? "income" : "expense" });
  };
  for (const path of adds) add(path, "expense");
  for (const c of data.categories) {
    const p = picks[c.path];
    const value = p?.value.trim() ?? "";
    if (!p || p.mode === "keep" || !value) continue;
    if (p.mode === "rename") middle.push({ rename: c.path, to: value });
    else if (p.mode === "merge") last.push({ delete: c.path, move_to: value });
    else if (parent(value) === parent(c.path)) middle.push({ rename: c.path, to: leaf(value) });
    else {
      add(value, c.kind);
      last.push({ delete: c.path, move_to: value });
    }
  }
  for (const i of accepted) {
    for (const op of data.suggestions[i]?.ops ?? []) {
      if (op.add) add(op.add, op.kind ?? "expense");
      else (op.delete ? last : middle).push(op);
    }
  }
  return [...first, ...middle, ...last];
}

/** One op in plain words. */
export function describe(op: CategoryOp): string {
  if (op.add) return `Add "${op.add}"`;
  if (op.rename) return `Rename "${op.rename}" to "${op.to}"`;
  if (op.move) return `Move the rows of "${op.move}" to "${op.to}"`;
  if (op.delete && op.move_to) return `Merge "${op.delete}" into "${op.move_to}" (its rows move)`;
  return `Delete "${op.delete}" (it has no rows)`;
}

function status(c: MoneyWizCategory): { label: string; tone: string } {
  if (c.shared) return { label: "Two share this name", tone: "warn" };
  if (c.cashcast) return { label: "Matches", tone: "ok" };
  if (c.suggested.length) return { label: "Suggestion", tone: "accent" };
  return { label: "Not used by cashcast", tone: "muted" };
}

const BACKUP = /Backup of the database before the change: (.+)/;

/** MoneyWiz categories beside cashcast's: pick changes, preview the plan, apply with a backup, undo. */
export function Categories({ user, api = realApi }: { user: string; api?: CategoriesApi }) {
  const [data, setData] = useState<MoneyWizCategories | null>(null);
  const [error, setError] = useState("");
  const [picks, setPicks] = useState<Record<string, Pick>>({});
  const [adds, setAdds] = useState<string[]>([]);
  const [accepted, setAccepted] = useState<number[]>([]);
  const [preview, setPreview] = useState<ActionResult | null>(null);
  const [result, setResult] = useState<ActionResult | null>(null);
  const [backup, setBackup] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<"apply" | "undo" | null>(null);
  const [yes, setYes] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api
      .load(user)
      .then(setData)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [user, api]);
  useEffect(() => load(), [load]);

  if (error || !data) {
    return (
      <section>
        <h2>Categories</h2>
        {error ? <p role="alert">{error}</p> : <p className="muted">Loading…</p>}
      </section>
    );
  }
  if (data.error) {
    return (
      <section>
        <h2>Categories</h2>
        <p role="alert">MoneyWiz is not ready: {data.error}</p>
        <p className="muted">
          Set it up with <code>connectors/playbooks/moneywiz.md</code>, then reload this page.
        </p>
      </section>
    );
  }

  const ops = buildOps(data, picks, adds, accepted);
  const reset = () => {
    setPreview(null);
    setConfirming(null);
    setYes(false);
  };
  const pick = (path: string, p: Pick) => {
    setPicks({ ...picks, [path]: p });
    reset();
  };
  const step = async (values: Record<string, string | boolean>, confirm = false) => {
    const r = await api.run(user, "moneywiz", { action: "categories", ...values }, confirm);
    if (r.exit !== 0) throw Object.assign(new Error(r.output.trim() || `exit ${r.exit}`), { result: r });
    return r;
  };
  const guard = async (work: () => Promise<void>) => {
    setBusy(true);
    setError("");
    setResult(null);
    try {
      await work();
    } catch (e) {
      const r = (e as { result?: ActionResult }).result;
      if (r) setResult(r);
      else setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setConfirming(null);
      setYes(false);
    }
  };
  const doPreview = () =>
    guard(async () => {
      await step({ step: "new", name: "Picked in the web app", ops: JSON.stringify(ops) });
      setPreview(await step({ step: "migrate", dry_run: true }));
    });
  const doApply = (which: "migrate" | "rollback") =>
    guard(async () => {
      const r = await step({ step: which, yes: true }, true);
      setResult(r);
      setBackup(r.output.match(BACKUP)?.[1]?.trim() ?? null);
      setPicks({});
      setAdds([]);
      setAccepted([]);
      setPreview(null);
      load();
    });

  const matched = data.categories.filter((c) => c.cashcast).length;
  const unused = data.categories.length - matched;
  const targets = data.categories.map((c) => c.path);

  return (
    <section className="categories">
      <h2>Categories</h2>
      <p className="lead">
        Each MoneyWiz category beside the cashcast category it matches. Pick changes, preview them, then apply them to
        MoneyWiz. A backup comes first, and you can undo.
      </p>
      <div className="cards">
        <div className="card">
          <div className="card-label">Match cashcast</div>
          <div className="card-value">{matched}</div>
        </div>
        <div className="card">
          <div className="card-label">Not used by cashcast</div>
          <div className="card-value">{unused}</div>
        </div>
        <div className="card">
          <div className="card-label">Missing in MoneyWiz</div>
          <div className="card-value">{data.missing.length}</div>
        </div>
        <div className="card">
          <div className="card-label">Suggestions</div>
          <div className="card-value">{data.suggestions.filter((s) => s.ops.length).length}</div>
        </div>
      </div>

      <TableWrap>
        <table aria-label="MoneyWiz and cashcast categories">
          <thead>
            <tr>
              <th scope="col">MoneyWiz category</th>
              <th scope="col">cashcast category</th>
              <th scope="col" className="num">
                Rows
              </th>
              <th scope="col">Change</th>
            </tr>
          </thead>
          <tbody>
            {data.categories.map((c) => {
              const s = status(c);
              const p = picks[c.path] ?? { mode: "keep", value: "" };
              return (
                <tr key={c.path}>
                  <td>
                    {c.path}
                    <div className="muted cat-kind">{c.kind}</div>
                  </td>
                  <td>
                    {c.cashcast ?? <span className="muted">none</span>}
                    <div>
                      <span className={`chip cat-${s.tone}`}>{s.label}</span>
                    </div>
                    {c.suggested.map((i) => (
                      <p key={i} className="muted cat-why">
                        {data.suggestions[i]?.reason}
                      </p>
                    ))}
                  </td>
                  <td className="num">{c.rows}</td>
                  <td>
                    {c.shared ? (
                      <span className="muted">Rename one in MoneyWiz first</span>
                    ) : (
                      <div className="cat-change">
                        <select
                          aria-label={`change ${c.path}`}
                          value={p.mode}
                          onChange={(e) => pick(c.path, { mode: e.target.value as Mode, value: "" })}
                        >
                          <option value="keep">Keep</option>
                          <option value="rename">Rename</option>
                          <option value="merge">Merge into…</option>
                          {data.missing.length > 0 && <option value="map">Match to cashcast…</option>}
                        </select>
                        {p.mode === "rename" && (
                          <input
                            aria-label={`new name for ${c.path}`}
                            value={p.value}
                            placeholder="New name"
                            onChange={(e) => pick(c.path, { ...p, value: e.target.value })}
                          />
                        )}
                        {(p.mode === "merge" || p.mode === "map") && (
                          <select
                            aria-label={`target for ${c.path}`}
                            value={p.value}
                            onChange={(e) => pick(c.path, { ...p, value: e.target.value })}
                          >
                            <option value="">Pick one</option>
                            {(p.mode === "merge" ? targets.filter((t) => t !== c.path) : data.missing).map((t) => (
                              <option key={t} value={t}>
                                {t}
                              </option>
                            ))}
                          </select>
                        )}
                      </div>
                    )}
                  </td>
                </tr>
              );
            })}
            {data.missing.map((m) => (
              <tr key={`missing-${m}`}>
                <td>
                  <span className="muted">none</span>
                </td>
                <td>
                  {m.split("/").join(" > ")}
                  <div>
                    <span className="chip cat-warn">Missing in MoneyWiz</span>
                  </div>
                </td>
                <td className="num">0</td>
                <td>
                  <label className="cat-check">
                    <input
                      type="checkbox"
                      checked={adds.includes(m)}
                      onChange={(e) => {
                        setAdds(e.target.checked ? [...adds, m] : adds.filter((a) => a !== m));
                        reset();
                      }}
                    />{" "}
                    Add to MoneyWiz
                  </label>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>

      {data.suggestions.some((s) => s.ops.length) && (
        <div className="panel cat-panel">
          <h3>Suggestions</h3>
          <ul className="cat-list">
            {data.suggestions.map((s, i) =>
              s.ops.length ? (
                <li key={s.reason}>
                  <label className="cat-check">
                    <input
                      type="checkbox"
                      checked={accepted.includes(i)}
                      onChange={(e) => {
                        setAccepted(e.target.checked ? [...accepted, i] : accepted.filter((a) => a !== i));
                        reset();
                      }}
                    />{" "}
                    {s.reason}
                  </label>
                </li>
              ) : null,
            )}
          </ul>
        </div>
      )}

      <div className="panel cat-panel">
        <h3>Your plan</h3>
        {data.pending.length > 0 && (
          <p className="muted">
            Waiting to apply: {data.pending.join(", ")}. A new plan here replaces a suggested one.
          </p>
        )}
        {ops.length === 0 ? (
          <p className="muted">No changes picked yet. Pick a change in the table or a suggestion.</p>
        ) : (
          <ol className="cat-list" aria-label="picked changes">
            {ops.map((op) => (
              <li key={describe(op)}>{describe(op)}</li>
            ))}
          </ol>
        )}
        <div className="cat-buttons">
          <button type="button" className="cat-btn" disabled={busy || ops.length === 0} onClick={doPreview}>
            {busy && !confirming ? "Working…" : "Preview plan"}
          </button>
          {preview && (
            <button type="button" className="cat-btn primary" disabled={busy} onClick={() => setConfirming("apply")}>
              Apply to MoneyWiz
            </button>
          )}
        </div>
        {preview && (
          <pre className="action-output" aria-label="plan preview">
            {preview.output}
          </pre>
        )}
        {confirming === "apply" && (
          <fieldset className="cat-confirm" aria-label="confirm apply">
            <p>
              cashcast first copies your MoneyWiz database to a backup folder, then tests the change on a copy, then
              writes it. MoneyWiz closes while it writes and opens again after.
            </p>
            <label className="cat-check">
              <input type="checkbox" checked={yes} onChange={(e) => setYes(e.target.checked)} /> Yes, change my MoneyWiz
              categories
            </label>
            <button
              type="button"
              className="cat-btn primary"
              disabled={!yes || busy}
              onClick={() => doApply("migrate")}
            >
              {busy ? "Applying…" : "Back up and apply"}
            </button>
          </fieldset>
        )}
      </div>

      {(backup || data.lastApplied) && (
        <div className="panel cat-panel" aria-label="last change">
          <h3>Last change</h3>
          {data.lastApplied && <p>Applied: {data.lastApplied.file}</p>}
          <p>
            Backup: <code>{backup ?? data.lastApplied?.backup ?? "none recorded"}</code>
          </p>
          {data.lastApplied &&
            (confirming === "undo" ? (
              <div className="cat-buttons">
                <button type="button" className="cat-btn primary" disabled={busy} onClick={() => doApply("rollback")}>
                  {busy ? "Undoing…" : "Yes, undo it"}
                </button>
                <button type="button" className="cat-btn" onClick={() => setConfirming(null)}>
                  Cancel
                </button>
              </div>
            ) : (
              <button type="button" className="cat-btn" disabled={busy} onClick={() => setConfirming("undo")}>
                Undo last change
              </button>
            ))}
        </div>
      )}

      {result && (
        <pre className="action-output" aria-label="result">
          {result.output}
        </pre>
      )}
    </section>
  );
}
