//! `private/cashcast.toml`: the owner's answers.
//!
//! The engine reads the sections below. Forecast-only sections (balances,
//! forecast, extra_income, taxes) live in the same file and are ignored here.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize, Default)]
pub struct Config {
    /// Account name to class: "cash", "card", "other" or "excluded".
    /// A key ending in `*` matches by prefix; an exact key wins.
    /// Accounts not listed are guessed from their name and statement type.
    #[serde(default)]
    pub accounts: BTreeMap<String, String>,
    #[serde(default)]
    pub currencies: Currencies,
    #[serde(default)]
    pub categories: Categories,
    #[serde(default)]
    pub transfers: Transfers,
    #[serde(default)]
    pub pay: Pay,
    /// Per-file settings, keyed by file name in the inbox.
    #[serde(default)]
    pub files: BTreeMap<String, FileConfig>,
    #[serde(default)]
    pub window: Window,
    /// Settings for the `holdings` command (investment and crypto ledgers).
    #[serde(default)]
    pub ledger: Ledger,
    /// The MoneyWiz connection (`moneywiz` commands and the import).
    pub moneywiz: Option<MoneyWiz>,
    #[serde(default)]
    pub rent: Rent,
}

/// `[rent]`: the forecast also keeps `charged` here, so unknown keys are allowed.
#[derive(Deserialize, Default)]
pub struct Rent {
    /// The spending series that holds rent, for example "Housing > Rent".
    pub category: Option<String>,
    /// A rent row dated on this day of the month or later pays next month's rent
    /// (a card service charges about a week early), so it counts in the next month.
    /// Default: rent counts in the month it was paid.
    pub paid_ahead_from_day: Option<u32>,
}

impl Rent {
    /// `paid_ahead_from_day` must be a day every month has, and not the 1st.
    pub fn check(&self) -> Result<(), String> {
        match self.paid_ahead_from_day {
            Some(d) if !(2..=28).contains(&d) => Err(format!(
                "cashcast.toml: [rent] paid_ahead_from_day = {d}: use a day from 2 to 28"
            )),
            _ => Ok(()),
        }
    }

    /// The date a row counts on: the first of next month for rent paid ahead, else `date`.
    /// `category` is the row's full category path; a sub-category of the rent path matches.
    pub fn due_date(&self, category: Option<&str>, date: chrono::NaiveDate) -> chrono::NaiveDate {
        use chrono::Datelike;
        let ahead = match (self.paid_ahead_from_day, category, self.category.as_deref()) {
            (Some(day), Some(c), Some(rent)) => {
                let under = c.strip_prefix(rent).is_some_and(|r| r.is_empty() || r.starts_with(" > "));
                under && date.day() >= day
            }
            _ => false,
        };
        if !ahead {
            return date;
        }
        let (y, m) = if date.month() == 12 {
            (date.year() + 1, 1)
        } else {
            (date.year(), date.month() + 1)
        };
        chrono::NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(date)
    }
}

/// `[moneywiz]`: where the MoneyWiz database is and how its accounts match
/// the bank exports in the inbox.
#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct MoneyWiz {
    /// Path to the MoneyWiz SQLite database. Default: found on this Mac.
    pub db: Option<String>,
    /// MoneyWiz account name to the account name in the exports. Accounts
    /// with the same name match without an entry. Unmatched MoneyWiz
    /// accounts (cash, for example) are never changed.
    #[serde(default)]
    pub accounts: BTreeMap<String, String>,
    /// MoneyWiz account name to a connector id (`connectors/<id>.toml`), for
    /// an account whose name does not say its bank. Used by `coverage`.
    #[serde(default)]
    pub connectors: BTreeMap<String, String>,
    /// MoneyWiz account name to why no export will ever cover it (a closed
    /// bank, a card with no export). `coverage` shows the reason as its gap.
    #[serde(default)]
    pub out_of_scope: BTreeMap<String, String>,
    /// MoneyWiz account name to "<YYYY-MM-DD>: reason": the bank has no
    /// file for its rows up to that day, and the owner accepts MoneyWiz as
    /// the record there. `coverage` counts those rows as decided.
    #[serde(default)]
    pub accepted: BTreeMap<String, String>,
    /// Export account name to why MoneyWiz does not track it (an unused
    /// savings account): it is no longer listed as an unlinked export.
    #[serde(default)]
    pub not_tracked: BTreeMap<String, String>,
    /// MoneyWiz account name to why the bank wins there even when the sync
    /// moves MoneyWiz away from the bank's balance (rows typed in another
    /// form stand in for bank rows). The sync is not held: it sends the
    /// bank's rows, and a balance correction the day before the period
    /// squares the end; `edit --fixes` then cleans up (owner, 2026-11-16).
    #[serde(default)]
    pub trust_bank: BTreeMap<String, String>,
    /// MoneyWiz investment account name to the broker ledger it shows, as
    /// "<format>:<plan type>", such as "wealthsimple-activity-csv:TFSA".
    /// `diagnose` compares their shares; unmapped broker accounts are listed.
    #[serde(default)]
    pub investments: BTreeMap<String, String>,
    /// Export account name to the export account it is part of, for banks
    /// that name each download apart (TD CSVs are named by file). A key
    /// ending in `*` matches by prefix. Alias rows fill only the days the
    /// account's own files do not cover, so overlapping downloads count once.
    #[serde(default)]
    pub export_aliases: BTreeMap<String, String>,
    /// MoneyWiz account name to the account its rows belong in, such as an
    /// old cash account renamed to make room for a new investment account
    /// ("Kraken old" = "Kraken"). `edit --fixes` moves every row in the
    /// target's currency into the target. In a target that shows a broker
    /// file (`investments`), the source's hand-typed investment rows are
    /// deleted instead: the broker's trades replace them.
    #[serde(default)]
    pub merge: BTreeMap<String, String>,
    /// Accounts to empty (name = currency): `edit --fixes` deletes every row;
    /// a transfer's other account keeps a plain row (owner, 2026-11-08). The
    /// owner then deletes the empty account in MoneyWiz.
    #[serde(default)]
    pub retire: BTreeMap<String, String>,
    /// Balances read on a site that gives no export (account name =
    /// "AMOUNT@YYYY-MM-DD", such as a stock plan's cash shown as 0.00).
    /// `edit --fixes` moves the account's opening balance so MoneyWiz shows
    /// that amount at the end of that day (owner, 2026-11-08).
    #[serde(default)]
    pub balances: BTreeMap<String, String>,
    /// Shares read on a site, by MoneyWiz investment account:
    /// "SYMBOL:SHARES@YYYY-MM-DD", such as a crypto vault after network fees
    /// its files never show. One adjusting trade makes the broker side end
    /// at that number on that day, and `edit --fixes` adds it to MoneyWiz.
    #[serde(default)]
    pub holdings: BTreeMap<String, String>,
    /// Yahoo tickers by symbol when the default (SYMBOL.TO in a CAD account,
    /// SYMBOL-CAD for a coin, else SYMBOL) is wrong; "" for no market price.
    #[serde(default)]
    pub tickers: BTreeMap<String, String>,
    /// The bank always wins: in each synced period, every MoneyWiz row that
    /// no bank row pairs with is offset (and `edit --fixes` removes it),
    /// without the balance proof. A row typed for several purchases gives
    /// way to the bank's rows too. Needs `match = "loose"`. Default: false.
    pub bank_wins: Option<bool>,
    /// `YYYY-MM-DD`, quoted: never change transactions before this date.
    pub since: Option<String>,
    /// A MoneyWiz transaction matches a bank row up to this many days away.
    /// Default: 4.
    pub match_days: Option<i64>,
    /// "strict" (default) pairs a MoneyWiz row with a bank row by amount, or
    /// by payee. "loose" also pairs rows typed by hand with a rough amount
    /// (within 25 %), another payee text and a date up to `match_days` away;
    /// the bank row wins.
    #[serde(rename = "match")]
    pub matching: Option<String>,
    /// Bring the MoneyWiz cash accounts (recorded by hand) into `import`.
    /// Default: true.
    pub import: Option<bool>,
    /// What `sync` does with a MoneyWiz amount that differs from the bank:
    /// "adjust" (default) adds a correction transaction for the difference
    /// (and one for a wrong opening balance); "list" only writes it to the
    /// owner's fix list; "edit" changes the MoneyWiz row (and the opening
    /// balance) in place with the Core Data writer (`moneywiz edit --fixes`).
    pub fixes: Option<String>,
    /// "core-data" lets `moneywiz edit` change rows and categories in place.
    pub writer: Option<String>,
    /// The MoneyWiz model checksum the owner allowed for live writes, after
    /// a test on a copy passed. A MoneyWiz update changes it.
    pub writer_checksum: Option<String>,
}

/// Unknown keys are allowed so the table can grow without breaking imports.
#[derive(Deserialize, Default)]
pub struct Ledger {
    /// Positions the files cannot show, keyed by asset symbol, for example
    /// `[ledger.opening.BTC] quantity = 0.5, acb = 20000`. They count as
    /// held from the start, and later deposits of that asset use them up
    /// before they count as "cost unknown".
    #[serde(default)]
    pub opening: BTreeMap<String, Opening>,
    /// Coins sent to a wallet outside the files and never seen coming back
    /// are gone (spent, sold elsewhere or lost), not still held. Default:
    /// false, they still count as held.
    #[serde(default)]
    pub sent_out_is_gone: bool,
    /// Quantities read on the site, by symbol (`[ledger.held] BTC = 1.0`).
    /// The holding ends at this number; its cost scales with it.
    #[serde(default)]
    pub held: BTreeMap<String, f64>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    pub quantity: f64,
    /// Total adjusted cost base in CAD.
    pub acb: f64,
    /// Account type such as "TFSA". Default: non-registered.
    pub bucket: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Window {
    /// `YYYY-MM`. Default: 35 months before `last_month`.
    pub first_month: Option<String>,
    /// `YYYY-MM`. Default: the month before today.
    pub last_month: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Account name for files without an account column.
    pub account: Option<String>,
    pub currency: Option<String>,
    /// chrono format such as `%m/%d/%Y`. Default: detected.
    pub date_format: Option<String>,
    pub category_separator: Option<String>,
    /// true flips signs, false never flips. Default: detected.
    pub invert: Option<bool>,
    /// 1-based column positions by role: date, amount, debit, credit,
    /// balance, account, category, currency, description.
    #[serde(default)]
    pub columns: BTreeMap<String, usize>,
}

/// Converts every amount to `base`. A rate is base units per 1 unit.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Currencies {
    /// Default: the most common currency in the data.
    pub base: Option<String>,
    #[serde(default)]
    pub rates: BTreeMap<String, f64>,
}

/// Category paths such as `"Employer > Salary"`. A top-level name covers its
/// subcategories. The longest matching path wins. Categories that match no
/// rule are classified automatically and reported.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Categories {
    #[serde(default)]
    pub salary: Vec<String>,
    #[serde(default)]
    pub rsu_espp: Vec<String>,
    #[serde(default)]
    pub tax_refund: Vec<String>,
    #[serde(default)]
    pub cashback: Vec<String>,
    #[serde(default)]
    pub gift: Vec<String>,
    #[serde(default)]
    pub other_income: Vec<String>,
    /// Income tax paid or refunded. Kept out of spending.
    #[serde(default)]
    pub income_tax: Vec<String>,
    /// Card payments or own-account moves recorded as a category.
    #[serde(default)]
    pub internal: Vec<String>,
    /// Contributions recorded as a category instead of a transfer.
    #[serde(default)]
    pub invested: Vec<String>,
    /// Always spending, even when the automatic rules would say income.
    #[serde(default)]
    pub spending: Vec<String>,
    /// Spending subcategories that get their own series.
    #[serde(default)]
    pub separate: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transfers {
    /// Income type for money moved from an excluded account into cash,
    /// for example `{ "Brokerage" = "rsu_espp" }`.
    #[serde(default)]
    pub from_excluded: BTreeMap<String, String>,
    #[serde(default = "default_from_excluded")]
    pub default_income: String,
}

impl Default for Transfers {
    fn default() -> Self {
        Transfers {
            from_excluded: BTreeMap::new(),
            default_income: default_from_excluded(),
        }
    }
}

fn default_from_excluded() -> String {
    "from_investments".to_string()
}

/// Pay schedule overrides. The forecast also keeps net pay and holidays
/// here, so unknown keys are allowed.
#[derive(Deserialize, Default)]
pub struct Pay {
    /// "weekly", "biweekly", "semimonthly" or "monthly". Default: detected.
    pub schedule: Option<String>,
    /// A known payday, `YYYY-MM-DD`, quoted. Default: the latest one found.
    pub anchor: Option<String>,
    /// A deposit up to this many days before a payday belongs to it.
    pub early_days: Option<i64>,
    /// A deposit up to this many days after a payday belongs to it.
    pub late_days: Option<i64>,
    /// A salary deposit smaller than this (in the base currency) is side
    /// income, not salary: it counts as "other" income. Default: no floor.
    pub min_salary: Option<f64>,
}

pub fn parse(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| format!("cashcast.toml: {e}"))
}
