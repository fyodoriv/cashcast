import type {
  Action,
  ActionResult,
  BankSync,
  Doc,
  DocEntry,
  Groceries,
  MoneyWizCategories,
  Plan,
  UserSummary,
} from "./types";

// The token comes in the URL fragment (#t=...) that `bin/cashcast web` prints.
// Fragments never reach a server log; we keep it for this tab only.
const KEY = "cashcast-token";

export function token(): string {
  const match = window.location.hash.match(/t=([\w-]+)/);
  if (match) {
    sessionStorage.setItem(KEY, match[1]);
    history.replaceState(null, "", window.location.pathname);
  }
  return sessionStorage.getItem(KEY) ?? "";
}

async function raw(path: string): Promise<Response> {
  const res = await fetch(path, { headers: { "X-Cashcast-Token": token() } });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(
      res.status === 401 ? "Open the link that bin/cashcast web printed (it holds the access token)." : text,
    );
  }
  return res;
}

async function get<T>(path: string): Promise<T> {
  return (await (await raw(path)).json()) as T;
}

export const fetchUsers = () => get<UserSummary[]>("/api/users");
export const fetchPlan = (user: string) => get<Plan>(`/api/plan?user=${encodeURIComponent(user)}`);

const q = (user: string) => `user=${encodeURIComponent(user)}`;
export const fetchDocs = (user: string, search = "") =>
  get<DocEntry[]>(`/api/docs?${q(user)}${search ? `&q=${encodeURIComponent(search)}` : ""}`);
export const fetchDoc = (user: string, path: string) =>
  get<Doc>(`/api/doc?${q(user)}&path=${encodeURIComponent(path)}`);
/** An image from the owner's documents, as a blob: URL (the token cannot ride on an <img> request). */
export async function fetchDocFile(user: string, path: string): Promise<string> {
  const res = await raw(`/api/doc-file?${q(user)}&path=${encodeURIComponent(path)}`);
  return URL.createObjectURL(await res.blob());
}

export const fetchBankSync = (user: string) => get<BankSync>(`/api/moneywiz-sync?${q(user)}`);
export const fetchCategories = (user: string) => get<MoneyWizCategories>(`/api/moneywiz-categories?${q(user)}`);

export const fetchGroceries = (user: string, items: string[] = []) =>
  get<Groceries>(`/api/groceries?${q(user)}${items.length ? `&items=${encodeURIComponent(items.join(","))}` : ""}`);

export const fetchActions = () => get<Action[]>("/api/actions");
/** Runs one CLI action as the owner. A 400 carries the reason (a missing field, a needed confirm). */
export async function runAction(
  user: string,
  name: string,
  values: Record<string, string | boolean>,
  confirm: boolean,
): Promise<ActionResult> {
  const res = await fetch(`/api/actions/${encodeURIComponent(name)}?${q(user)}`, {
    method: "POST",
    headers: { "X-Cashcast-Token": token(), "Content-Type": "application/json" },
    body: JSON.stringify({ values, confirm }),
  });
  const body = await res.json().catch(() => ({ error: res.statusText }));
  if (!res.ok) throw new Error(body.error ?? res.statusText);
  return body as ActionResult;
}
