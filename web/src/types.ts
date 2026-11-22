// The shape of GET /api/plan, written by src/cashcast/web.py (plan_payload).

export interface UserSummary {
  name: string;
  display: string;
  province: string | null;
  ready: boolean;
}

export interface ForecastRow {
  month: string; // YYYY-MM
  paycheques: number;
  income: number;
  expenses: number;
  net: number;
  start: number;
  toInvest: number;
  end: number;
  /** 80 % range for month-end cash, from past forecast misses (backtest). */
  low?: number;
  high?: number;
}

/** Share of the past the forecast got right (1 - WAPE), from `bin/cashcast backtest`. */
export interface ForecastAccuracy {
  monthEnd: number;
  netMonthly: number;
  netQuarterly: number;
}

export type Risk = "read" | "settings" | "money";

export interface PlanTask {
  id: string;
  title: string;
  due: string | null; // YYYY-MM-DD
  notes: string;
  risk: Risk;
  service: string; // the connector that does it ("" when none)
  action: string; // that connector's action id
}

/** One CLI command, as `GET /api/actions` lists it (generated from the CLI parser). */
export interface ActionArg {
  name: string;
  flag: string | null;
  kind: "flag" | "choice" | "int" | "text";
  choices: string[] | null;
  required: boolean;
  help: string;
  default: string | number | null;
}

export interface Action {
  name: string;
  command: string;
  help: string;
  args: ActionArg[];
  ui: boolean;
  terminal_only: string | null;
  terminal_only_choices: Record<string, string>;
  confirm: string | null;
  confirm_choices: Record<string, string>;
  agent_note: string | null;
}

export interface ActionResult {
  action: string;
  argv: string[];
  exit: number;
  output: string;
}

export interface Section {
  id: string;
  title: string;
  markdown: string;
}

export interface CashBack {
  gross: number;
  net: number;
  cardFees: number;
  serviceFees: number;
  membershipFees: number;
  routes: { category: string; card: string; amount: number; rate: number }[];
  bankPaid: Record<string, number>;
}

export interface Holding {
  symbol: string;
  account_bucket: string;
  quantity: number;
  acb_cad: number;
  cost_unknown?: boolean;
}

export interface Holdings {
  assets: Holding[];
  realized_by_year?: { year: number; gains_cad: number; losses_cad: number; net_cad: number; sales: number }[];
  income_by_year?: { year: number; kind: string; cad: number }[];
  superficial_loss_flags?: { asset: string; year: number; count: number }[];
  needs_cost?: { asset: string; account_bucket: string; reason: string; quantity: number }[];
  /** Computed holdings compared with counts the vendor reports. */
  checks?: string[];
}

export interface Connector {
  id: string;
  name: string;
  kinds: string[];
  formats: string[];
  noExport: string;
  api: string | null;
  actions: { id: string; title: string; risk: Risk; via: string[] }[];
}

export interface ConsentScope {
  scope: string;
  shares: string;
  recipient: string;
  grantedUntil: string | null;
}

/** The main number: all you have minus all you owe (engine/src/net_total.rs). */
export interface NetOption {
  id: string;
  title: string;
  section: string;
  netEffect: number;
  cashOut: number | null;
  why: string;
  refused: string[]; // the boundaries it breaks; empty when it is inside them
}

export interface NetTotal {
  total: number;
  byKind: Record<string, number>; // cash, investment, crypto, other, debt (negative)
  changes: { days: number; since: string; change: number }[];
  history: { date: string; total: number }[];
  ranked: NetOption[];
  refused: NetOption[];
  stale: string[];
  missing: string[];
}

export interface Plan {
  user: { name: string; display: string; province: string };
  today: string;
  disclaimer: string;
  headline: {
    netCashBack: number | null;
    canInvest: number | null;
    lowestMonthEnd: number | null;
    lowestMonth: string | null;
    nextReview: string | null;
    pivot: boolean;
    emergencyFund: number | null;
    investAbove: number | null;
    marginalRate: number | null;
  };
  sections: Section[];
  forecast: ForecastRow[];
  forecastAccuracy?: ForecastAccuracy | null;
  tasks: PlanTask[];
  netTotal: NetTotal | null;
  cashback: CashBack | null;
  holdings: Holdings | null;
  connectors: Connector[];
  knowledge: {
    files: number;
    facts: number;
    estimates: number;
    heuristics: number;
    stale: string[];
    problems: string[];
    topics: { name: string; title: string; asOf: string; reviewBy: string }[];
  };
  consent: ConsentScope[];
}

// GET /api/docs and /api/doc (src/cashcast/docs.py): the owner's private documents.
export interface DocEntry {
  path: string;
  title: string;
  folder: string;
  snippet?: string;
}

export interface Doc {
  path: string;
  title: string;
  markdown: string;
}

// GET /api/moneywiz-categories (src/cashcast/moneywiz_write.py `view`): names and counts only.
/** One step of a category migration, as in `inbox/moneywiz-migrations/*.toml` (decision 0012). */
export interface CategoryOp {
  add?: string;
  kind?: "income" | "expense";
  rename?: string;
  to?: string;
  move?: string;
  only?: string;
  delete?: string;
  move_to?: string;
}

export interface MoneyWizCategory {
  path: string;
  kind: "income" | "expense" | "unknown";
  rows: number;
  incomeRows: number;
  subcategories: number;
  /** The cashcast category ("Parent > Child") this one matches, if any. */
  cashcast: string | null;
  /** Two MoneyWiz categories share this path: cashcast leaves it alone. */
  shared: boolean;
  /** Indexes into `suggestions` that touch this category. */
  suggested: number[];
}

export interface MoneyWizCategories {
  error?: string;
  tree: string;
  categories: MoneyWizCategory[];
  /** Categories cashcast uses ("Parent/Child") that MoneyWiz lacks. */
  missing: string[];
  suggestions: { reason: string; ops: CategoryOp[] }[];
  applied: number;
  lastApplied: { file: string; backup: string | null } | null;
  pending: string[];
}

/** One row a bank sync adds to MoneyWiz. Owner's eyes only. */
export interface SyncRow {
  date: string;
  text: string;
  amount: string;
  cents: number;
  category: string;
}

export interface SyncAccount {
  moneywiz: string;
  kind: "bank" | "card";
  export: string;
  from: string | null;
  to: string | null;
  daysOld: number | null;
  bankRows: number;
  matched: number;
  add: SyncRow[];
  fix: { date: string; text: string; moneywiz: string; bank: string }[];
  onlyInMoneywiz: number;
  conflicts: number;
  balance: string;
  /** Why the sync gate holds this account (nothing is sent for it), or null. */
  held?: string | null;
  /** MoneyWiz reconcile rows in the period that the sync offsets. */
  reconcile?: number;
  /** MoneyWiz rows with no bank row that the sync offsets (loose mode, balance-checked). */
  standIn?: number;
  onlyInMoneywizRows?: { date: string; text: string; amount: string }[];
  /** From the sync ledger: bank rows per status, and the last day one was checked. */
  ledger?: { statuses: Record<string, number>; lastChecked: string | null } | null;
}

/** One sync, from the ledger (newest first). */
export interface SyncRun {
  at: string;
  apply: boolean;
  sent: number;
  held: number;
}

/** GET /api/moneywiz-sync: what `moneywiz sync` would do now. */
export interface BankSync {
  error?: string;
  database: string;
  mode: "list" | "adjust" | "edit";
  today: string;
  accounts: SyncAccount[];
  untouched: string[];
  unlinkedExports: string[];
  links: { toSend: number; waiting: number; gone: number; held?: number };
  runs?: SyncRun[];
  recheck?: { account: string | null; from: string | null; to: string | null }[];
  coverage: {
    percent: number | null;
    covered: number;
    rows: number;
    accounts: {
      name: string;
      kind: string;
      rows: number;
      covered: number;
      connector: string | null;
      source: string;
      gap: string | null;
      last: string | null;
    }[];
    suggestions: { moneywiz: string; export: string }[];
  };
  moneywizExports: number;
}
