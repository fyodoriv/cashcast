import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Shell, VIEWS } from "../App";
import { ticks } from "../Chart";
import { friendlyDates } from "../format";
import { Markdown } from "../markdown";
import type { Plan, UserSummary } from "../types";
import type { Action } from "../types";
import type { ActionResult, BankSync as BankSyncData, Groceries as GroceriesData, MoneyWizCategories } from "../types";
import { Actions, type ActionsApi } from "../views/Actions";
import { BankSync, type BankSyncApi, accountState } from "../views/BankSync";
import { Categories, type CategoriesApi, buildOps } from "../views/Categories";
import { type DocsApi, Documents } from "../views/Documents";
import { Groceries, type GroceriesApi, belowPriciest, parseItems } from "../views/Groceries";
import { applyWhatIf } from "../views/PlanAhead";

// jsdom has no ResizeObserver; Recharts' ResponsiveContainer needs one.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;
import fixture from "./fixture.json";

const plan = fixture.plan as unknown as Plan;
const users = fixture.users as UserSummary[];

afterEach(cleanup);

// What each view must show from the synthetic person (see scripts/web-fixture.sh).
const EXPECT: Record<string, RegExp> = {
  plan: /You can invest/,
  overview: /Net total[\s\S]*Best next step inside your boundaries[\s\S]*Net cash back a year/,
  net: /Net total[\s\S]*Options, best first/,
  forecast: /Month end/,
  review: /Next review/,
  cashback: /Test Grocery 4%/,
  credit: /Target at close/,
  reserve: /Emergency fund/,
  tax: /marginal/,
  payments: /avoidable fee/,
  budget: /Budget proposals/,
  alternatives: /Test Costly/,
  holdings: /XEQT[\s\S]*Checks[\s\S]*E\*TRADE reports 40 sellable/,
  services: /Kraken/,
  knowledge: /sourced facts/,
  privacy: /agent:aggregates/,
  documents: /Documents/,
  categories: /Categories/,
  banksync: /Bank sync/,
  groceries: /Groceries/,
  actions: /bin\/cashcast actions/,
};

describe("every view renders the synthetic plan", () => {
  it("has 21 views, each with an expectation", () => {
    expect(VIEWS).toHaveLength(21);
    expect(Object.keys(EXPECT).sort()).toEqual(VIEWS.map((v) => v.id).sort());
  });

  for (const view of VIEWS) {
    it(`renders ${view.label}`, () => {
      render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView={view.id} />);
      const main = screen.getByRole("main");
      expect(main.textContent).toMatch(EXPECT[view.id]);
      expect(main.textContent).not.toMatch(/CANARY/);
      // One h1, and no skipped heading level (axe page-has-heading-one, heading-order).
      const levels = [...main.querySelectorAll("h1,h2,h3,h4,h5,h6")].map((h) => Number(h.tagName[1]));
      expect(levels[0]).toBe(1);
      levels.forEach((l, i) => i && expect(l).toBeLessThanOrEqual(levels[i - 1] + 1));
      // Dates read as "Sun, Oct 18", never ISO (code samples excepted).
      const text = [...main.querySelectorAll("*:not(code):not(pre)")]
        .flatMap((e) => [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent))
        .join(" ");
      expect(text.match(/\b\d{4}-\d{2}-\d{2}\b/g) ?? []).toEqual([]);
    });
  }

  it("switches views from the navigation", () => {
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="plan" />);
    fireEvent.click(screen.getByRole("button", { name: "Tax" }));
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Tax");
  });

  it("opens the net total's options from home", () => {
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="overview" />);
    fireEvent.click(screen.getByRole("button", { name: "All options" }));
    expect(screen.getByRole("main").textContent).toMatch(/Your boundaries/);
  });

  it("plans ahead with a what-if", () => {
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="plan" />);
    const before = screen.getByRole("status").textContent;
    fireEvent.change(screen.getByLabelText("one-off cost"), { target: { value: "5000" } });
    expect(screen.getByRole("status").textContent).not.toBe(before);
  });
});

describe("what-if math", () => {
  const rows = [
    { month: "2026-11", paycheques: 2, income: 5000, expenses: 3000, net: 2000, start: 0, toInvest: 1000, end: 6000 },
    { month: "2026-12", paycheques: 2, income: 5000, expenses: 3000, net: 2000, start: 0, toInvest: 1000, end: 6000 },
  ];
  it("subtracts extra spending from what can be invested", () => {
    const r = applyWhatIf(rows, { spendingChange: 0.1, oneOff: 0, oneOffMonth: "2026-11" });
    expect(r.canInvest).toBe(2000 - 600);
    expect(r.firstShortfall).toBeNull();
  });
  it("finds the first month under the floor", () => {
    const r = applyWhatIf(rows, { spendingChange: 0, oneOff: 1500, oneOffMonth: "2026-11" });
    expect(r.firstShortfall).toBe("2026-11");
    expect(r.canInvest).toBe(500);
  });
});

describe("markdown", () => {
  it("never turns text into markup", () => {
    const { container } = render(
      <Markdown text={'## T\n<img src=x onerror="alert(1)"> **bold**\n| a |\n|---|\n| <b>x</b> |'} />,
    );
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("b")).toBeNull();
    expect(container.querySelector("strong")?.textContent).toBe("bold");
    expect(container.querySelector("td")?.textContent).toBe("<b>x</b>");
  });
});

describe("design patterns (docs/design.md)", () => {
  it("promises no 80 % range when the forecast has none (judge round 1)", () => {
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="forecast" />);
    const main = screen.getByRole("main");
    expect(main.textContent).not.toMatch(/80 %/);
    expect(main.textContent).toMatch(/once there are a few months of history/);
  });

  it("stacks every table on a phone, each cell labeled by its column (judge round 1)", () => {
    const { container } = render(
      <Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="privacy" />,
    );
    const wrap = container.querySelector(".table-wrap");
    expect(wrap?.classList.contains("stack")).toBe(true);
    const cells = [...container.querySelectorAll("tbody td")];
    expect(cells.length).toBeGreaterThan(0);
    expect(cells.every((td) => td.getAttribute("data-label"))).toBe(true);
  });

  it("shows the forecast's 80 % range and backtest accuracy", () => {
    const p = {
      ...plan,
      today: `${plan.forecast[0].month}-15`,
      forecast: plan.forecast.map((r) => ({ ...r, low: r.end - 1000, high: r.end + 2000 })),
      forecastAccuracy: { monthEnd: 0.75, netMonthly: 0.59, netQuarterly: 0.824 },
    };
    render(<Shell plan={p} users={users} user="alex" setUser={() => {}} initialView="forecast" />);
    expect(screen.getByLabelText(/80 percent range and money to invest each month/)).toBeTruthy();
    expect(screen.getByText("to invest")).toBeTruthy();
    expect(screen.getByText("75%", { selector: "b" })).toBeTruthy();
    expect(screen.getAllByText(/ to /).length).toBeGreaterThan(0);
    // One row per forecast month from now on, plus the header: the same months Plan ahead counts.
    expect(screen.getAllByRole("row").length).toBeLessThanOrEqual(plan.forecast.length + 1);
  });

  it("shows a month's numbers when a chart month gets keyboard focus", () => {
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="overview" />);
    expect(screen.queryByRole("tooltip")).toBeNull();
    const chart = screen.getAllByRole("slider", { name: /arrow keys/ })[0];
    fireEvent.focus(chart);
    expect(screen.getByRole("tooltip").textContent).toMatch(/Month end/);
  });

  it("is one tab stop per chart; arrow keys move between months", () => {
    const { container } = render(
      <Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="overview" />,
    );
    expect(container.querySelectorAll(".chart [tabindex]").length).toBe(0);
    const chart = screen.getAllByRole("slider", { name: /arrow keys/ })[0];
    fireEvent.focus(chart);
    const first = screen.getByRole("tooltip").textContent;
    fireEvent.keyDown(chart, { key: "ArrowRight" });
    expect(screen.getByRole("tooltip").textContent).not.toBe(first);
    fireEvent.keyDown(chart, { key: "Home" });
    expect(screen.getByRole("tooltip").textContent).toBe(first);
  });

  it("opens every view from More on a phone", () => {
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="overview" />);
    const more = screen.getByRole("button", { name: "More" });
    fireEvent.click(more);
    expect(screen.getByRole("button", { name: "Close" }).getAttribute("aria-expanded")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Privacy" }));
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Privacy");
    expect(screen.getByRole("button", { name: "More" }).getAttribute("aria-expanded")).toBe("false");
  });

  it("right-aligns table columns that hold only amounts", () => {
    const { container } = render(
      <Markdown text={"| Card | Per year |\n|---|---|\n| A | $1,200.00 |\n| B | $60.00 |"} />,
    );
    const cells = container.querySelectorAll("td");
    expect(cells[0].className).toBe("");
    expect(cells[1].className).toBe("num");
  });

  it("renders a line that starts with # but is not a heading as text", () => {
    const { container } = render(<Markdown text={"#1 goal: CANARY_HASH\n#####deep"} />);
    expect(container.querySelector("p")?.textContent).toContain("CANARY_HASH");
    expect(container.textContent).toContain("#####deep");
  });
});

describe("documents moved from Notion", () => {
  const pages: Record<string, string> = {
    "notion/Finances.md":
      "# Finances\n\n[Taxes](doc:notion/Finances/Taxes.md) · [site](https://example.com/x)\n\n![chart](doc:notion/Finances/chart.png) ![remote](https://example.com/a.png)\n",
    "notion/Finances/Taxes.md": "# Taxes\n\n> A quote\n\n```\ncode line\n```\n\n- [x] Filed\n",
  };
  const files: string[] = [];
  const api: DocsApi = {
    list: async (_u, q) =>
      Object.keys(pages)
        .filter((p) => !q || pages[p].toLowerCase().includes(q.toLowerCase()))
        .map((p) => ({
          path: p,
          title: p.split("/").pop()?.replace(".md", "") ?? p,
          folder: p.split("/").slice(0, -1).join("/"),
        })),
    read: async (_u, path) => ({ path, title: path, markdown: pages[path] }),
    file: async (_u, path) => {
      files.push(path);
      return "blob:fake";
    },
  };

  it("lists, opens, follows doc links and loads only local images", async () => {
    render(<Documents user="alex" api={api} />);
    fireEvent.click(await screen.findByRole("button", { name: "Finances" }));
    const site = await screen.findByRole("link", { name: "site" });
    expect(site.getAttribute("rel")).toBe("noreferrer noopener");
    expect(site.getAttribute("target")).toBe("_blank");
    await waitFor(() => expect(files).toEqual(["notion/Finances/chart.png"]));
    expect(document.querySelectorAll("img")).toHaveLength(1);
    expect(screen.getByText("remote")).toBeTruthy();
    fireEvent.click(screen.getByRole("link", { name: "Taxes" }));
    expect(await screen.findByText("code line")).toBeTruthy();
    expect(document.querySelector("blockquote")?.textContent).toBe("A quote");
  });

  it("searches the documents", async () => {
    render(<Documents user="alex" api={api} />);
    fireEvent.change(await screen.findByLabelText("search documents"), { target: { value: "quote" } });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Finances" })).toBeNull());
    expect(screen.getByRole("button", { name: "Taxes" })).toBeTruthy();
  });

  it("shows how to import when there is nothing yet", async () => {
    render(<Documents user="alex" api={{ ...api, list: async () => [] }} />);
    expect(await screen.findByText(/bin\/cashcast notion import/)).toBeTruthy();
  });
});

describe("the Actions view runs the same catalog as the CLI", () => {
  const catalog = fixture.actions as unknown as Action[];

  it("lists every CLI command, with terminal-only reasons", async () => {
    const api: ActionsApi = {
      list: async () => catalog,
      run: async () => ({ action: "", argv: [], exit: 0, output: "" }),
    };
    render(<Actions user="alex" api={api} />);
    await screen.findByText("Run optimize");
    expect(screen.getAllByRole("listitem")).toHaveLength(catalog.length);
    expect(screen.getByText(/it is the web app itself/)).toBeTruthy();
  });

  it("runs an action and asks to confirm the hard-to-undo ones", async () => {
    const calls: unknown[][] = [];
    const api: ActionsApi = {
      list: async () => catalog,
      run: async (...a) => {
        calls.push(a);
        return { action: a[1], argv: ["bin/cashcast", a[1]], exit: 0, output: "Coming up" };
      },
    };
    render(<Actions user="alex" api={api} />);
    fireEvent.click(await screen.findByText("Run changes"));
    await screen.findByText(/Coming up/);
    expect(calls[0]).toEqual(["alex", "changes", { action: "list", id: "" }, false]);
    const card = screen.getByText("Run calendar").closest("li") as HTMLElement;
    expect((within(card).getByText("Run calendar") as HTMLButtonElement).disabled).toBe(false); // preview needs no confirm
    fireEvent.change(within(card).getByLabelText("action"), { target: { value: "sync" } });
    expect((within(card).getByText("Run calendar") as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByText(/Yes, this writes your dated deadlines/));
    expect((screen.getByText("Run calendar") as HTMLButtonElement).disabled).toBe(false);
    // a yes covers one choice: switching away and back asks again
    fireEvent.change(within(card).getByLabelText("action"), { target: { value: "preview" } });
    fireEvent.change(within(card).getByLabelText("action"), { target: { value: "sync" } });
    expect((within(card).getByText("Run calendar") as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("the Categories view matches MoneyWiz to cashcast and applies with a backup", () => {
  const data = fixture.moneywizCategories as unknown as MoneyWizCategories;
  const calls: { values: Record<string, string | boolean>; confirm: boolean }[] = [];
  const out = (output: string, exit = 0): ActionResult => ({ action: "moneywiz", argv: [], exit, output });
  const api: CategoriesApi = {
    load: async () => data,
    run: async (_u, _n, values, confirm) => {
      calls.push({ values, confirm });
      if (values.step === "migrate" && values.dry_run) return out("Migration 0001-web.toml: 1 add");
      if (values.step === "migrate")
        return out("Backup of the database before the change: private/data/moneywiz-backups/x");
      return out("Wrote");
    },
  };

  it("shows each MoneyWiz category beside its cashcast match, and the missing ones", async () => {
    render(<Categories user="alex" api={api} />);
    const table = await screen.findByRole("table", { name: "MoneyWiz and cashcast categories" });
    const row = within(table).getByText("Food/Groceries").closest("tr") as HTMLElement;
    expect(row.textContent).toMatch(/Food > Groceries[\s\S]*Matches/);
    expect(within(table).getByText("Old").closest("tr")?.textContent).toMatch(/Not used by cashcast/);
    expect(table.textContent).toMatch(/Food > Coffee[\s\S]*Missing in MoneyWiz/);
    expect(table.textContent).toMatch(/Suggestion/);
  });

  it("previews the picked plan, then applies only after a confirm, and shows the backup", async () => {
    calls.length = 0;
    render(<Categories user="alex" api={api} />);
    fireEvent.click(await screen.findByLabelText(/Add to MoneyWiz/));
    fireEvent.change(screen.getByLabelText("change Old"), { target: { value: "merge" } });
    fireEvent.change(screen.getByLabelText("target for Old"), { target: { value: "Food" } });
    const picked = screen.getByRole("list", { name: "picked changes" });
    expect(picked.textContent).toMatch(/Add "Food\/Coffee"[\s\S]*Merge "Old" into "Food"/);
    fireEvent.click(screen.getByRole("button", { name: "Preview plan" }));
    expect((await screen.findByLabelText("plan preview")).textContent).toMatch(/1 add/);
    expect(JSON.parse(String(calls[0].values.ops))).toEqual([
      { add: "Food/Coffee" },
      { delete: "Old", move_to: "Food" },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Apply to MoneyWiz" }));
    const apply = screen.getByRole("button", { name: "Back up and apply" }) as HTMLButtonElement;
    expect(apply.disabled).toBe(true);
    fireEvent.click(screen.getByLabelText(/Yes, change my/));
    fireEvent.click(apply);
    expect(await screen.findByText("private/data/moneywiz-backups/x")).toBeTruthy();
    expect(calls.at(-1)).toEqual({ values: { action: "categories", step: "migrate", yes: true }, confirm: true });
  });

  it("matches across parents by adding the cashcast category and merging into it", () => {
    const ops = buildOps(data, { Old: { mode: "map", value: "Food/Coffee" } }, [], []);
    expect(ops).toEqual([{ add: "Food/Coffee" }, { delete: "Old", move_to: "Food/Coffee" }]);
  });
});

describe("the Bank sync view previews every row and sends only after a confirm", () => {
  const data = fixture.bankSync as unknown as BankSyncData;
  const calls: { name: string; values: Record<string, string | boolean>; confirm: boolean }[] = [];
  const api: BankSyncApi = {
    load: async () => data,
    run: async (_u, name, values, confirm) => {
      calls.push({ name, values, confirm });
      return { action: name, argv: [], exit: 0, output: "Sent 6 links to MoneyWiz.\nMoneyWiz saved 6 of 6." };
    },
  };

  it("shows each linked account with the rows it adds, and leaves cash alone", async () => {
    render(<BankSync user="sam" api={api} />);
    const chequing = await screen.findByRole("article", { name: "Chequing" });
    expect(chequing.textContent).toMatch(/from Chequing[\s\S]*to add/);
    const rows = within(chequing).getByRole("table", { name: "rows to add to Chequing" });
    expect(within(rows).getAllByRole("row").length).toBeGreaterThan(1);
    expect(screen.getByText(/You record these by hand: Cash/)).toBeTruthy();
    expect(screen.getByText(/changes are ready/)).toBeTruthy();
  });

  it("asks for a yes before it sends, then sends one confirmed sync", async () => {
    calls.length = 0;
    render(<BankSync user="sam" api={api} />);
    fireEvent.click(await screen.findByRole("button", { name: "Send to MoneyWiz" }));
    const send = screen.getByRole("button", { name: "Send now" }) as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    fireEvent.click(screen.getByLabelText(/Yes, add them/));
    fireEvent.click(send);
    expect((await screen.findByLabelText("last result")).textContent).toMatch(/saved 6 of 6/);
    expect(calls).toEqual([{ name: "moneywiz", values: { action: "sync" }, confirm: true }]);
  });

  it("collects fresh exports from Downloads without a confirm", async () => {
    calls.length = 0;
    render(<BankSync user="sam" api={api} />);
    fireEvent.click(await screen.findByRole("button", { name: "Collect exports from Downloads" }));
    await waitFor(() => expect(calls).toEqual([{ name: "fetch", values: { action: "collect" }, confirm: false }]));
  });

  it("names what each account needs: a pick, a fresh export, rows, or nothing", () => {
    const base = data.accounts[0];
    expect(accountState({ ...base, conflicts: 2 }).label).toBe("2 to pick");
    expect(accountState({ ...base, conflicts: 0, daysOld: 30 }).label).toBe("Export 30 days old");
    expect(accountState({ ...base, conflicts: 0, daysOld: 1, add: [], fix: [] }).label).toBe("Up to date");
  });

  it("shows the ledger, a held account and the sync history, and asks a resync per account", async () => {
    calls.length = 0;
    const held = {
      ...data,
      accounts: data.accounts.map((a, i) =>
        i === 0
          ? {
              ...a,
              held: "the sync moves MoneyWiz away from the bank's balance",
              ledger: { statuses: { matched: 4, add: 2 }, lastChecked: "2026-11-02" },
              onlyInMoneywizRows: [{ date: "2026-09-10", text: "DUPLICATE", amount: "-12.00" }],
              standIn: 3,
            }
          : a,
      ),
      runs: [{ at: "2026-11-02 09:15", apply: true, sent: 3, held: 1 }],
    };
    render(<BankSync user="sam" api={{ ...api, load: async () => held }} />);
    const first = await screen.findByRole("article", { name: held.accounts[0].moneywiz });
    expect(within(first).getByText("Held")).toBeTruthy();
    expect(first.textContent).toMatch(/Last checked Mon, Nov 2 · 4 in MoneyWiz · 2 to add/);
    expect(first.textContent).toMatch(/Nothing is sent for this account/);
    expect(first.textContent).toMatch(/3 rows with no bank row get an offset/);
    expect(within(first).getByRole("table", { name: /no bank row/ })).toBeTruthy();
    const history = screen.getByRole("table", { name: "sync history" });
    expect(history.textContent).toMatch(/Sync\s*3\s*1/);
    fireEvent.click(within(first).getByRole("button", { name: "Check this account again" }));
    await waitFor(() =>
      expect(calls).toEqual([
        { name: "moneywiz", values: { action: "resync", account: held.accounts[0].moneywiz }, confirm: false },
      ]),
    );
  });

  it("explains a MoneyWiz that is not set up", async () => {
    render(<BankSync user="sam" api={{ ...api, load: async () => ({ ...data, error: "no database" }) }} />);
    expect((await screen.findByRole("alert")).textContent).toMatch(/MoneyWiz is not ready: no database/);
  });
});

describe("friendlyDates", () => {
  it("formats a real date and leaves a rolled-over one alone", () => {
    expect(friendlyDates("due 2027-03-04")).toBe("due Mar 4, 2027");
    expect(friendlyDates("id 2027-13-40")).toBe("id 2027-13-40");
  });
});

describe("round-13 design notes stay fixed", () => {
  const text = JSON.stringify(fixture);
  it("engine text: plural rows, no $0 pace, a plan sentence, signed rises", () => {
    expect(text).not.toMatch(/\(1 rows\)/);
    expect(text).not.toMatch(/at your pace of \$0\.00/);
    expect(text).not.toMatch(/plan:\*\* membership saves/);
    expect(text).toMatch(/\| \+200% \|/);
  });

  it("the top axis tick is at or above the highest value", () => {
    for (const [lo, hi] of [
      [0, 950],
      [0, 1234],
      [-300, 41000],
      [0, 1],
    ]) {
      expect(ticks(lo, hi).at(-1)).toBeGreaterThanOrEqual(hi);
    }
  });

  it("Forecast and Plan ahead name the same number of months", () => {
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="forecast" />);
    const n = /next (\d+) months/.exec(screen.getByRole("main").textContent ?? "")?.[1];
    cleanup();
    render(<Shell plan={plan} users={users} user="alex" setUser={() => {}} initialView="plan" />);
    expect(screen.getByRole("main").textContent).toMatch(new RegExp(`over ${n} months`));
  });

  it("a zero net-total change is neutral, not green with a plus", () => {
    const zero = {
      ...plan,
      netTotal: { ...plan.netTotal, changes: [{ days: 30, since: "2026-09-18", change: 0 }] },
    } as Plan;
    render(<Shell plan={zero} users={users} user="alex" setUser={() => {}} initialView="overview" />);
    const chip = screen.getByText(/in 30 days/);
    expect(chip.className).not.toMatch(/pos/);
    expect(chip.textContent).not.toMatch(/^\+/);
  });

  it("Bank sync amounts carry a dollar sign", async () => {
    const data = fixture.bankSync as unknown as BankSyncData;
    render(<BankSync user="sam" api={{ load: async () => data, run: async () => ({}) as ActionResult }} />);
    const table = await screen.findByRole("table", { name: "rows to add to Chequing" });
    const cells = [...table.querySelectorAll("td.num")].map((c) => c.textContent ?? "");
    expect(cells.length).toBeGreaterThan(0);
    for (const c of cells) expect(c).toMatch(/^-?\$/);
  });
});

describe("groceries", () => {
  const price = (store: string, name: string, p: number) => ({
    item: "oats",
    store,
    name,
    price: p,
    was: null,
    organic: false,
    per100: p / 10,
    checked: "2026-11-20",
    link: null,
  });
  const basket = (store: string, total: number | null, missing: string[] = []) => ({
    store,
    lines: [{ item: "oats", best: price(store, `Oats at ${store}`, 4), sub: null }],
    missing,
    subtotal: 4,
    discount: 0,
    delivery: total === null ? null : total - 4,
    service: 0,
    total,
    fee_note: "no fee rule saved",
  });
  const data: GroceriesData = {
    items: [{ item: "oats", stores: 2, checked: "2026-11-20" }],
    fees: [
      {
        store: "store-a",
        delivery: 5,
        service: 1,
        free_over: 30,
        pct_off: null,
        max_off: null,
        min_order: null,
        note: "",
        checked: "2026-11-20",
      },
    ],
    months: [{ month: "2026-11", orders: 2, total: 160.25 }],
    orders: [{ day: "2026-11-15", store: "store-a", total: 60, note: "" }],
    compare: [],
    asked: [],
    freshDays: 3,
    rules: ["rules:", "  - never anchovies"],
  };

  it("reads items and ranks full baskets against the priciest", () => {
    expect(parseItems(" Oats, ,tea ")).toEqual(["oats", "tea"]);
    const below = belowPriciest([basket("a", 80), basket("b", 100), basket("c", 10, ["tea"])]);
    expect(below.get("a")).toBe(20);
    expect(below.get("b")).toBe(0);
    expect(below.has("c")).toBe(false);
  });

  it("shows spend, fees and rules, then compares the asked items", async () => {
    const asked: string[][] = [];
    const api: GroceriesApi = {
      load: async (_u, items) => {
        asked.push(items);
        return items.length ? { ...data, asked: items, compare: [basket("store-a", 9), basket("store-b", 12)] } : data;
      },
    };
    render(<Groceries user="alex" api={api} />);
    expect(await screen.findAllByText("$160.25")).toHaveLength(2);
    expect(screen.getByRole("table", { name: "store fees after memberships" }).textContent).toMatch(
      /\$5\.00 \(free over \$30\.00\) \+ \$1\.00 service/,
    );
    expect(screen.getByText(/never anchovies/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "+ oats" }));
    fireEvent.click(screen.getByRole("button", { name: "Compare" }));
    expect(await screen.findByText(/store-a: \$9\.00 · 25 % below the priciest/)).toBeTruthy();
    expect(asked.at(-1)).toEqual(["oats"]);
  });

  it("says how to start when there are no prices", async () => {
    render(<Groceries user="alex" api={{ load: async () => ({ ...data, items: [], months: [], orders: [] }) }} />);
    expect(await screen.findByText(/grocery import --from OLD\.sqlite/)).toBeTruthy();
  });
});
