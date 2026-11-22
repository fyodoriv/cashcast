import { type ReactNode, useEffect, useState } from "react";
import { fetchPlan, fetchUsers } from "./api";
import type { Plan, UserSummary } from "./types";
import { Actions } from "./views/Actions";
import { BankSync } from "./views/BankSync";
import { Categories } from "./views/Categories";
import { Documents } from "./views/Documents";
import { Forecast } from "./views/Forecast";
import { Groceries } from "./views/Groceries";
import { Overview } from "./views/Overview";
import { PlanAhead } from "./views/PlanAhead";
import { HoldingsView, KnowledgeView, PrivacyView, ServicesView } from "./views/Reference";
import { SectionView } from "./views/SectionView";

export interface View {
  id: string;
  label: string;
  group: "Plan" | "Strategy" | "Reference";
  render: (plan: Plan, go: (id: string) => void) => ReactNode;
}

const section = (id: string, label: string, ids: string[]): View => ({
  id,
  label,
  group: "Strategy",
  render: (plan) => <SectionView plan={plan} title={label} ids={ids} />,
});

export const VIEWS: View[] = [
  { id: "overview", label: "Home", group: "Plan", render: (plan, go) => <Overview plan={plan} go={go} /> },
  { id: "plan", label: "Plan ahead", group: "Plan", render: (plan) => <PlanAhead plan={plan} /> },
  { id: "forecast", label: "Forecast", group: "Plan", render: (plan) => <Forecast plan={plan} /> },
  section("net", "Net total", ["net_total"]),
  section("review", "Monthly review", ["review"]),
  section("cashback", "Where to pay", ["cashback"]),
  section("credit", "Credit", ["credit"]),
  section("reserve", "Cash & investing", ["reserve", "invest", "invest_plan"]),
  section("tax", "Tax", ["tax"]),
  section("payments", "Payments", ["payments"]),
  section("budget", "Budget", ["budget"]),
  section("alternatives", "Alternatives", ["alternatives", "banking"]),
  { id: "banksync", label: "Bank sync", group: "Plan", render: (plan) => <BankSync user={plan.user.name} /> },
  { id: "groceries", label: "Groceries", group: "Plan", render: (plan) => <Groceries user={plan.user.name} /> },
  { id: "documents", label: "Documents", group: "Reference", render: (plan) => <Documents user={plan.user.name} /> },
  {
    id: "categories",
    label: "Categories",
    group: "Reference",
    render: (plan) => <Categories user={plan.user.name} />,
  },
  { id: "holdings", label: "Holdings", group: "Reference", render: (plan) => <HoldingsView plan={plan} /> },
  { id: "services", label: "Services", group: "Reference", render: (plan) => <ServicesView plan={plan} /> },
  { id: "knowledge", label: "Knowledge", group: "Reference", render: (plan) => <KnowledgeView plan={plan} /> },
  { id: "privacy", label: "Privacy", group: "Reference", render: (plan) => <PrivacyView plan={plan} /> },
  { id: "actions", label: "Actions", group: "Reference", render: (plan) => <Actions user={plan.user.name} /> },
];

/** The views in the mobile tab bar; the rest sit behind "More". */
const PRIMARY = new Set(["overview", "plan", "forecast", "review"]);
/** One-line tab bar labels on a phone; the full label stays the button's name. */
const SHORT: Record<string, string> = { plan: "Plan", review: "Review" };

// 24px stroke icons, one per view.
const ICONS: Record<string, string> = {
  net: "M3 20h18M6 16l4-5 4 3 6-8",
  overview: "M3 11l9-7 9 7v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z",
  plan: "M4 5h16M4 12h10M4 19h6M18 15l3 3-3 3",
  forecast: "M3 17l5-5 4 4 8-8M15 8h5v5",
  review: "M9 11l2 2 4-4M5 4h14v16H5z",
  cashback: "M3 7h18v10H3zM3 11h18M7 15h3",
  credit: "M12 3a9 9 0 1 0 9 9M12 12l6-6",
  reserve: "M4 10l8-6 8 6M6 10v8M10 10v8M14 10v8M18 10v8M4 20h16",
  tax: "M6 3h9l3 3v15H6zM9 12h6M9 16h6",
  payments: "M4 6h16v12H4zM8 10h.01M12 14h4",
  budget: "M12 3v9h9A9 9 0 1 1 12 3z",
  alternatives: "M7 7h10l-3-3M17 17H7l3 3",
  holdings: "M4 20V10M10 20V4M16 20v-8M22 20H2",
  services: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v3M12 19v3M2 12h3M19 12h3",
  knowledge: "M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2zM4 19V5",
  privacy: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z",
  documents: "M7 3h7l5 5v13H7zM14 3v5h5M10 13h6M10 17h6",
  categories: "M4 6h7v5H4zM13 13h7v5h-7zM11 8h4v5",
  actions: "M5 4l14 8-14 8z",
  groceries: "M3 4h2l2.5 11h11L21 7H6.2M9 20h.01M17 20h.01",
  banksync: "M20 8a8 8 0 0 0-14.5-3M4 4v4h4M4 16a8 8 0 0 0 14.5 3M20 20v-4h-4",
  more: "M4 7h16M4 12h16M4 17h16",
};

function Icon({ id }: { id: string }) {
  return (
    <svg
      className="nav-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      aria-hidden="true"
    >
      <path d={ICONS[id] ?? ICONS.more} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function storedView(): string {
  try {
    return localStorage.getItem("cashcast-view") ?? "overview";
  } catch {
    return "overview";
  }
}

export function Shell({
  plan,
  users,
  user,
  setUser,
  initialView,
}: {
  plan: Plan;
  users: UserSummary[];
  user: string;
  setUser: (u: string) => void;
  initialView?: string;
}) {
  const [view, setView] = useState(initialView ?? storedView());
  const [more, setMore] = useState(false);
  const go = (id: string) => {
    setView(id);
    setMore(false);
    document.querySelector("main")?.scrollIntoView?.({ block: "start" });
    try {
      localStorage.setItem("cashcast-view", id);
    } catch {
      /* private mode: the choice is just not remembered */
    }
  };
  const current = VIEWS.find((v) => v.id === view) ?? VIEWS[0];
  const groups = ["Plan", "Strategy", "Reference"] as const;
  const overdue = plan.tasks.filter((t) => t.due && t.due < plan.today).length;
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            c
          </span>
          Cashcast
        </div>
        <label className="person">
          <span>Person</span>
          <select value={user} onChange={(e) => setUser(e.target.value)} aria-label="person">
            {users.map((u) => (
              <option key={u.name} value={u.name} disabled={!u.ready}>
                {u.display}
                {u.province ? ` (${u.province})` : " (no profile yet)"}
              </option>
            ))}
          </select>
        </label>
        <nav aria-label="views" className={more ? "open" : undefined}>
          {groups.map((g) => (
            <div key={g} className="nav-group">
              <span className="nav-title">{g}</span>
              {VIEWS.filter((v) => v.group === g).map((v) => (
                <button
                  type="button"
                  key={v.id}
                  className={PRIMARY.has(v.id) ? "primary" : undefined}
                  aria-current={v.id === current.id ? "page" : undefined}
                  onClick={() => go(v.id)}
                >
                  <Icon id={v.id} />
                  {SHORT[v.id] ? (
                    <>
                      <span className="label-long">{v.label}</span>
                      <span className="label-short" aria-hidden="true">
                        {SHORT[v.id]}
                      </span>
                    </>
                  ) : (
                    v.label
                  )}
                  {v.id === "plan" && overdue > 0 && (
                    <span className="nav-badge" title={`${overdue} overdue`}>
                      {overdue}
                    </span>
                  )}
                </button>
              ))}
            </div>
          ))}
          <button type="button" className="nav-more" aria-expanded={more} onClick={() => setMore(!more)}>
            <Icon id="more" />
            {more ? "Close" : "More"}
          </button>
        </nav>
      </aside>
      <main>
        <h1 className="visually-hidden">Cashcast: {current.label}</h1>
        {current.render(plan, go)}
        <footer className="muted">{plan.disclaimer}</footer>
      </main>
    </div>
  );
}

export default function App() {
  const [users, setUsers] = useState<UserSummary[]>([]);
  const [user, setUser] = useState<string>("");
  const [plan, setPlan] = useState<Plan | null>(null);
  const [error, setError] = useState<string>("");

  useEffect(() => {
    fetchUsers()
      .then((list) => {
        setUsers(list);
        const first = list.find((u) => u.ready);
        if (first) setUser(first.name);
        else setError("No person has a profile yet. Run: bin/cashcast users add NAME --province XX");
      })
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(() => {
    if (!user) return;
    setPlan(null);
    fetchPlan(user)
      .then(setPlan)
      .catch((e: Error) => setError(e.message));
  }, [user]);

  if (error)
    return (
      <p className="banner" role="alert">
        {error}
      </p>
    );
  if (!plan) return <p className="muted">Loading your plan…</p>;
  return <Shell plan={plan} users={users} user={user} setUser={setUser} />;
}
