//! Investment recommender: aggregates in, ranked moves out, each one honest
//! about what it knows (decision 0013).
//!
//! The input is a JSON file the assistant writes from `bin/cashcast` data:
//! market values per account bucket, holdings with their cost base, cash
//! above the reserve, registered room, the marginal tax rate, the evidence
//! facts from `knowledge/data/invest_evidence.toml` and the owner's
//! overrides. No raw rows reach this module.
//!
//! Rules:
//! - A move's yearly gain is a range (`gain_low`..`gain_high`), built from
//!   cited facts. A return assumption is a long-run planning number and is
//!   always widened by the `return-band` fact; it is never a forecast.
//! - Moves that only cut risk (concentration, rebalancing) have an expected
//!   gain of 0 and are listed apart, so they never look like free money.
//! - "Do nothing" is ranked with the gain moves on the same terms. When no
//!   move clears the owner's simplicity threshold, it wins.
//! - An overridden move is respected: no task, its forgone range is shown
//!   once under "Your choices", without nagging.
//! - Nothing here moves money. A move becomes a one-time task the owner does.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Fact {
    pub value: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub as_of: String,
    #[serde(default)]
    pub review_by: String,
    #[serde(default)]
    pub confidence: String,
    /// A judgment call with no external source behind its number.
    #[serde(default)]
    pub judgment: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub bucket: String,
    /// CAD value: market value when a price is known, else the cost base.
    pub value: f64,
    /// "market" or "cost".
    #[serde(default)]
    pub basis: String,
    pub acb: Option<f64>,
    /// "equity", "bond", "balanced", "cash", "crypto" or "" when unknown.
    #[serde(default)]
    pub asset_class: String,
    #[serde(default)]
    pub region: String,
    /// "CA" or "US" listing; "" when unknown.
    #[serde(default)]
    pub domicile: String,
    pub mer: Option<f64>,
    /// Stock share of a balanced fund, from knowledge/data/investing.toml.
    #[serde(default)]
    pub equity_share: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Cheapest {
    pub ticker: String,
    pub mer: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Room {
    pub account: String,
    pub room: f64,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Employer {
    pub symbol: String,
    pub value: f64,
    /// How `value` was found: "market", "etrade-total" or "cost".
    #[serde(default)]
    pub basis: String,
    pub acb: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Override {
    #[serde(rename = "move")]
    pub move_id: String,
    #[serde(default)]
    pub reason: String,
    /// Optional end date (YYYY-MM-DD). After it the move comes back, so a
    /// choice made for one moment does not hide a cost for years.
    #[serde(default)]
    pub until: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Input {
    #[serde(default)]
    pub today: String,
    pub marginal: Option<f64>,
    #[serde(default = "half")]
    pub inclusion: f64,
    /// The stock share the owner aims for (0 to 1).
    pub target_equity: f64,
    /// Where `target_equity` comes from, for example "your risk level (growth)".
    #[serde(default)]
    pub target_source: String,
    #[serde(default)]
    pub simplicity: f64,
    pub cash: Option<f64>,
    pub reserve: Option<f64>,
    /// The best rate the owner's cash earns now.
    #[serde(default)]
    pub cash_rate: f64,
    #[serde(default)]
    pub room: Vec<Room>,
    /// Market value per bucket (TFSA, RRSP, FHSA, Non-registered, Crypto, ...).
    #[serde(default)]
    pub buckets: BTreeMap<String, f64>,
    #[serde(default)]
    pub positions: Vec<Position>,
    pub employer: Option<Employer>,
    /// Cheapest Canadian-listed fund per "class:region" (and per class for
    /// all-in-one funds), from investing.toml. US-listed funds file only under
    /// "class:region:US-listed", for the RRSP withholding move.
    #[serde(default)]
    pub cheapest: BTreeMap<String, Cheapest>,
    #[serde(default)]
    pub superficial: Vec<String>,
    #[serde(default)]
    pub facts: BTreeMap<String, Fact>,
    #[serde(default)]
    pub overrides: Vec<Override>,
    /// Months until the owner needs the money (a home purchase, say), when known.
    pub horizon_months: Option<f64>,
    /// Debt at 8% a year or more (`[invest] high_interest_debt`), in dollars.
    pub high_interest_debt: Option<f64>,
    /// The largest crypto share of investments the owner accepts
    /// (`[invest] crypto_max`, 0 to 1), when set.
    pub crypto_max: Option<f64>,
}

fn half() -> f64 {
    0.5
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Move {
    pub id: String,
    /// "gain", "risk" or "baseline" (do nothing).
    pub kind: String,
    pub title: String,
    pub gain_low: f64,
    pub gain_high: f64,
    /// One-time cost that is known (tax on a sale, conversion fees).
    pub cost_once: f64,
    pub cost_note: String,
    pub risk: String,
    pub why: String,
    pub assumptions: Vec<String>,
    pub evidence: Vec<String>,
    pub unknowns: Vec<String>,
    /// The one-time task title, when the move is worth doing.
    pub task: Option<String>,
    /// The owner's reason, when they overrode this move.
    pub overridden: Option<String>,
    /// The safety rule that blocks this move, when one does.
    pub vetoed: Option<String>,
    /// The strongest case against this move, from its own evidence.
    pub objection: String,
    /// The account of the holding a per-holding move acts on.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bucket: String,
}

impl Move {
    pub fn mid(&self) -> f64 {
        (self.gain_low + self.gain_high) / 2.0
    }
    /// First-year net: middle of the range minus the known one-time cost.
    pub fn net(&self) -> f64 {
        self.mid() - self.cost_once
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Plan {
    /// Gain moves and "do nothing", best first by first-year net.
    pub ranked: Vec<Move>,
    /// Moves that cut risk with no expected gain.
    pub risk: Vec<Move>,
    /// Moves the owner overrode.
    pub overridden: Vec<Move>,
    /// Moves a safety rule blocked (`veto`).
    pub vetoed: Vec<Move>,
    /// What the plan could not know.
    pub unknowns: Vec<String>,
    /// True when "do nothing" ranks first.
    pub do_nothing_wins: bool,
}

struct Ctx<'a> {
    i: &'a Input,
}

impl Ctx<'_> {
    fn v(&self, id: &str, default: f64) -> f64 {
        self.i
            .facts
            .get(id)
            .and_then(|f| f.value)
            .unwrap_or(default)
    }
    fn band(&self) -> f64 {
        self.v("return-band", 0.02)
    }
    /// Expected long-run return of the owner's target mix, before fees.
    fn mix_return(&self) -> f64 {
        let te = self.i.target_equity.clamp(0.0, 1.0);
        te * self.v("pag-equity", 0.065) + (1.0 - te) * self.v("pag-fixed-income", 0.032)
    }
    fn marginal(&self) -> Option<f64> {
        self.i.marginal
    }
    fn total(&self) -> f64 {
        let buckets: f64 = self.i.buckets.values().sum();
        if buckets > 0.0 {
            buckets
        } else {
            self.i.positions.iter().map(|p| p.value).sum()
        }
    }
}

fn base(id: &str, kind: &str, title: String) -> Move {
    Move {
        id: id.into(),
        kind: kind.into(),
        title,
        ..Move::default()
    }
}

/// Whole dollars with thousands separators; ranges never need cents.
pub fn dollars(x: f64) -> String {
    let neg = x < 0.0;
    let n = x.abs().round() as i64;
    let digits = n.to_string();
    let mut out = String::new();
    for (k, ch) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{}${out}", if neg { "-" } else { "" })
}

fn pct(x: f64) -> String {
    let x = if x.abs() < 0.0005 { 0.0 } else { x };
    let s = format!("{:.1}%", x * 100.0);
    s.replace(".0%", "%")
}

fn is_non_registered(bucket: &str) -> bool {
    bucket.eq_ignore_ascii_case("Non-registered")
}

fn tax_on_gain(c: &Ctx, value: f64, acb: Option<f64>) -> (f64, Option<String>) {
    match (acb, c.marginal()) {
        (Some(acb), Some(m)) => ((value - acb).max(0.0) * c.i.inclusion * m, None),
        (None, _) => (
            0.0,
            Some("the cost base is unknown, so the tax on selling is unknown".into()),
        ),
        (_, None) => (
            0.0,
            Some("your marginal rate is unknown (add [income] to profile.toml)".into()),
        ),
    }
}

fn cash_drag(c: &Ctx) -> Option<Move> {
    let (cash, reserve) = (c.i.cash?, c.i.reserve?);
    let extra = cash - reserve;
    if !extra.is_finite() || extra <= 0.0 {
        return None;
    }
    let short = c.v("short-horizon", 60.0);
    // A horizon that is not a real positive number counts as "needed soon".
    if let Some(h) =
        c.i.horizon_months
            .filter(|h| !(h.is_finite() && *h >= short))
    {
        return Some(near_money(
            c,
            extra,
            if h.is_finite() { h.max(0.0) } else { 0.0 },
        ));
    }
    let r = c.mix_return();
    // An example fund that matches the target mix, never an all-stock fund
    // for a mixed target.
    let fund = if c.i.target_equity >= 0.99 {
        c.i.cheapest.get("equity")
    } else {
        c.i.cheapest
            .get(&format!("mix:{:.0}", c.i.target_equity * 100.0))
    };
    let fee = fund.map(|e| e.mer).unwrap_or(c.v("etf-mers", 0.0019));
    let low = extra * (r - c.band() - fee - c.i.cash_rate);
    let high = extra * (r + c.band() - fee - c.i.cash_rate);
    let into =
        c.i.room
            .iter()
            .filter(|r| r.room > 0.0)
            .map(|r| r.account.as_str())
            .next()
            .unwrap_or("Non-registered");
    let mut m = base(
        "cash-drag",
        "gain",
        format!("Invest the {} of cash above your reserve", dollars(extra)),
    );
    m.gain_low = low;
    m.gain_high = high;
    m.why = format!(
        "Cash earns {} now; a mix of {} stocks is planned at about {} a year over 10+ years. Put it in {} first (your room order).",
        pct(c.i.cash_rate),
        pct(c.i.target_equity),
        pct(r),
        into
    );
    m.risk = format!(
        "Stocks can fall about {} in a bad year: on this amount, a drop of about {}. Only money you will not need for 5+ years.",
        pct(-c.v("one-year-drop", -0.42)),
        dollars(-extra * c.i.target_equity * c.v("one-year-drop", -0.42))
    );
    m.cost_note = if c.v("broker-trade-fee", 0.0) == 0.0 {
        "Trades are free at Wealthsimple (`broker-trade-fee`); other brokers charge per trade."
            .into()
    } else {
        format!(
            "About {} a trade (`broker-trade-fee`).",
            dollars(c.v("broker-trade-fee", 0.0))
        )
    };
    m.assumptions = vec![
        format!(
            "Return of your mix: {} to {} a year (planning assumption ± {} points), minus a {} fund fee.",
            pct(r - c.band()),
            pct(r + c.band()),
            pct(c.band()).trim_end_matches('%'),
            pct(fee)
        ),
        format!("Your cash keeps earning {}.", pct(c.i.cash_rate)),
    ];
    m.evidence = vec![
        "pag-equity".into(),
        "pag-fixed-income".into(),
        "return-band".into(),
        "one-year-drop".into(),
        "short-horizon".into(),
        "etf-mers".into(),
        "broker-trade-fee".into(),
    ];
    m.unknowns = vec!["Returns in any one year: nobody knows them.".into()];
    // Never more than the room left: an over-contribution is taxed 1% a month.
    // Each account gets at most its room; only the leftover goes to Non-registered.
    let mut left = extra;
    let mut parts: Vec<String> = vec![];
    for r in c.i.room.iter().filter(|r| r.room > 0.0) {
        if left <= 0.0 {
            break;
        }
        let put = left.min(r.room);
        parts.push(format!("{} into {}", dollars(put), r.account));
        left -= put;
    }
    let placement = if parts.is_empty() {
        "Non-registered".to_string()
    } else {
        let mut t = parts.join(", ");
        if left > 0.0 {
            t += &format!(", the rest {} in Non-registered", dollars(left));
        }
        t + ", each up to your room left after this year's contributions"
    };
    m.task = Some(format!(
        "Invest {} of cash above your reserve ({}), {}{}",
        dollars(extra),
        placement,
        if c.i.target_equity >= 0.99 {
            "all in stocks".to_string()
        } else {
            format!("{} stocks and the rest bonds", pct(c.i.target_equity))
        },
        fund.map(|e| format!(
            " (for example {}, an all-in-one fund with that mix)",
            e.ticker
        ))
        .unwrap_or_default()
    ));
    Some(m)
}

/// Cash needed within a few years stays out of stocks: the move is only to a
/// better-paying safe place, and its gain is small and nearly certain.
fn near_money(c: &Ctx, extra: f64, months: f64) -> Move {
    // Today's 1-year GIC rate when known (live), else the planning assumption.
    // A 1-year GIC cannot mature in time for money needed sooner.
    let live =
        c.i.facts
            .get("live-gic-1y")
            .and_then(|f| f.value)
            .filter(|_| months >= 12.0);
    let safe = live.unwrap_or_else(|| c.v("pag-short-term", 0.024));
    let mut m = base(
        "cash-drag",
        "gain",
        format!(
            "Keep the {} above your reserve safe, at the best rate (needed in about {} months)",
            dollars(extra),
            months.round()
        ),
    );
    let gap = (safe - c.i.cash_rate).max(0.0);
    // Interest is taxed in full outside a registered account, and money needed
    // within a year earns only for those months.
    let after_tax = 1.0 - c.marginal().unwrap_or(0.0).clamp(0.0, 1.0);
    let part = (months / 12.0).clamp(0.0, 1.0);
    m.gain_low = extra * gap * c.v("cash-rate-capture", 0.5) * after_tax * part;
    m.gain_high = extra * gap * after_tax * part;
    m.why = format!(
        "You need this money within {} months, too soon for stocks. A high-interest account, GIC or cash ETF pays about {} versus your {}.",
        months.round(),
        pct(safe),
        pct(c.i.cash_rate)
    );
    m.risk = "GICs lock money until maturity; cash ETFs are not CDIC-insured.".into();
    m.assumptions = vec![format!(
        "{}{}. Safe rates near {} ({}; low end half of the gap).",
        if c.marginal().is_some() {
            "After tax at your marginal rate"
        } else {
            "Before tax (your marginal rate is unknown)"
        },
        if part < 1.0 {
            format!(
                ", earned for about {} months, not a full year",
                months.round()
            )
        } else {
            String::new()
        },
        pct(safe),
        if live.is_some() {
            "today's 1-year GIC rate, Bank of Canada"
        } else {
            "planning assumption"
        }
    )];
    m.evidence = vec![
        "short-horizon".into(),
        "cash-rate-capture".into(),
        "one-year-drop".into(),
        if live.is_some() {
            "live-gic-1y"
        } else {
            "pag-short-term"
        }
        .into(),
    ];
    m.task = Some(format!(
        "Move {} above your reserve to a higher-rate safe account{}",
        dollars(extra),
        if months >= 12.0 {
            " or GIC"
        } else {
            " you can withdraw in time"
        }
    ));
    // Money for a first home earns the same safe rate inside an FHSA, plus a
    // deduction going in and a tax-free withdrawal for the home.
    if let Some(room) =
        c.i.room
            .iter()
            .find(|r| r.account.eq_ignore_ascii_case("FHSA") && r.room > 0.0)
    {
        let put = extra.min(room.room);
        let deduction = c.marginal().map(|t| put * t);
        m.why += &format!(
            " If it is for a first home, up to {} of it can sit in your FHSA in the same safe holdings: the contribution is deductible{} and a withdrawal for the home is tax-free.",
            dollars(put),
            deduction.map_or(String::new(), |d| format!(
                " (about {} less tax once, at your marginal rate)",
                dollars(d)
            ))
        );
        m.assumptions
            .push("The FHSA tip applies only if this money is for a first home.".into());
        m.evidence.push("fhsa".into());
        // A GIC must mature before the money is needed.
        let (outside, inside) = if months >= 12.0 {
            ("account or GIC", "a GIC or savings")
        } else {
            (
                "account you can withdraw in time",
                "savings you can withdraw in time",
            )
        };
        m.task = Some(format!(
            "Move {} above your reserve to a higher-rate safe {outside}; if it is for a first home, put up to {} of it in your FHSA, in {inside} inside it",
            dollars(extra),
            dollars(put)
        ));
    }
    m
}

/// TFSA room the "cash-drag" stock move will use: none when that move is
/// near-money, blocked or overridden.
fn tfsa_cash(c: &Ctx) -> f64 {
    let Some(m) = cash_drag(c) else { return 0.0 };
    let (Some(cash), Some(res)) = (c.i.cash, c.i.reserve) else {
        return 0.0;
    };
    let near = m.task.as_deref().is_some_and(|t| !t.starts_with("Invest "));
    if near || veto(c.i, &m).is_some() || overridden(c.i, &m).is_some() {
        return 0.0;
    }
    // Cash fills the accounts in room order: only what spills past the
    // accounts listed before the TFSA lands in it.
    let mut left = (cash - res).max(0.0);
    for r in c.i.room.iter().filter(|r| r.room > 0.0) {
        if r.account.eq_ignore_ascii_case("TFSA") {
            return left.min(r.room);
        }
        left = (left - r.room).max(0.0);
    }
    0.0
}

fn fill_room(c: &Ctx) -> Option<Move> {
    let nonreg: f64 =
        c.i.buckets
            .iter()
            .filter(|(b, _)| is_non_registered(b))
            .map(|(_, v)| v)
            .sum();
    let room =
        c.i.room
            .iter()
            .find(|r| r.account.eq_ignore_ascii_case("TFSA"))
            .map(|r| r.room)
            .unwrap_or(0.0);
    let cash_extra = tfsa_cash(c);
    // Cash above the reserve already goes to the TFSA in "cash-drag".
    let amount = nonreg.min((room - cash_extra).max(0.0));
    let m_rate = c.marginal()?;
    if amount <= 0.0 {
        return None;
    }
    let r = c.mix_return();
    // Gains are taxed only when sold, so the yearly drag runs from a low share
    // (gains deferred for years) to the full rate (sold each year).
    let low =
        amount * (r - c.band()).max(0.0) * c.i.inclusion * m_rate * c.v("gain-deferral-share", 0.5);
    // The high end sells every gain each year, taxed at the inclusion rate,
    // the same stock drag as the bond swap.
    let high = amount * (r + c.band()) * c.i.inclusion * m_rate;
    let mut m = base(
        "fill-room",
        "gain",
        format!(
            "Move {} from your taxable account into TFSA room",
            dollars(amount)
        ),
    );
    m.gain_low = low;
    m.gain_high = high;
    let mut tax = 0.0;
    let mut unknown_tax = false;
    let mut losers = vec![];
    // Coins cannot go into a TFSA and are not in the taxable total, so they
    // are not part of this sale.
    for p in
        c.i.positions
            .iter()
            .filter(|p| is_non_registered(&p.bucket) && p.asset_class != "crypto")
    {
        if p.basis != "market" || p.acb.is_none() {
            unknown_tax = true;
            continue;
        }
        let acb = p.acb.unwrap_or(0.0);
        if p.value < acb {
            losers.push(p.symbol.clone());
        }
        // Gains and losses on the same sale offset each other.
        tax += (p.value - acb) * c.i.inclusion * m_rate * (amount / nonreg.max(1.0)).min(1.0);
    }
    let tax = tax.max(0.0);
    // The tax is due anyway when the owner sells one day: moving now only
    // pays it earlier. The real cost is a year of growth on that tax.
    m.cost_once = tax * c.mix_return();
    m.cost_note = if unknown_tax || c.i.positions.is_empty() {
        "Selling first realizes gains; their tax is unknown without each holding's market price ([invest.prices]).".into()
    } else {
        format!(
            "Tax on gains you realize: about {}, due anyway when you sell later; paying it now costs about {} a year of growth on it.",
            dollars(tax),
            dollars(m.cost_once)
        )
    };
    m.why = "Growth in a TFSA is never taxed; in a taxable account it is.".into();
    m.risk = "None from markets (same investments, new account). Room used now comes back only the next calendar year after a withdrawal.".into();
    m.assumptions = vec![
        format!(
            "Your mix returns {} to {} a year; your marginal rate stays {}.",
            pct(r - c.band()),
            pct(r + c.band()),
            pct(m_rate)
        ),
        "Low end: gains deferred for years. High end: gains realized every year.".into(),
    ];
    if !losers.is_empty() {
        m.assumptions.push(format!(
            "{} sit below cost: sell them in the taxable account first, then contribute cash. No buy of the same fund for 30 days before or after the sale, in any of your accounts (pause recurring buys and any DRIP), or buy a similar but not identical one; otherwise the loss is a superficial loss and is lost for good.",
            losers.join(", ")
        ));
        m.evidence.push("in-kind-loss".into());
        m.evidence.push("superficial-loss".into());
        m.risk
            .push_str(" Rebuying a fund sold at a loss within 30 days denies that loss.");
    }
    m.evidence.extend([
        "tfsa-shelter".into(),
        "pag-equity".into(),
        "return-band".into(),
        "pag-fixed-income".into(),
        "gain-deferral-share".into(),
    ]);
    m.task = Some(if losers.is_empty() {
        format!(
            "Move {} from your taxable account into your TFSA (up to your room left after this year's TFSA contributions)",
            dollars(amount)
        )
    } else {
        format!(
            "Move {} from your taxable account into your TFSA (up to your room left after this year's TFSA contributions); sell {} first and contribute the cash (not in kind, or the loss is lost); for 30 days before and after, buy only a similar fund, not the same one, in any account{}",
            dollars(amount),
            losers.join(", "),
            match c.i.employer.as_ref() {
                Some(e) if losers.iter().any(|l| l.eq_ignore_ascii_case(&e.symbol)) =>
                    "; an RSU vest or ESPP purchase of your employer's stock in that window counts as a buy",
                _ => "",
            }
        )
    });
    Some(m)
}

fn fees(c: &Ctx) -> Vec<Move> {
    let mut out = vec![];
    for p in &c.i.positions {
        // Compare like with like: same asset class and region, else the class alone.
        let same_region = c.i.cheapest.get(&format!("{}:{}", p.asset_class, p.region));
        let cheap = same_region.or_else(|| c.i.cheapest.get(&p.asset_class));
        let (Some(mer), Some(cheap)) = (p.mer, cheap) else {
            continue;
        };
        if cheap.ticker.eq_ignore_ascii_case(&p.symbol) || mer - cheap.mer < 0.0005 {
            continue;
        }
        let saved = p.value * (mer - cheap.mer);
        let mut m = base(
            &format!("fee:{}", p.symbol),
            "gain",
            format!(
                "Swap {} ({} fee) for {} ({}) in your {}",
                p.symbol,
                pct(mer),
                cheap.ticker,
                pct(cheap.mer),
                p.bucket
            ),
        );
        m.bucket = p.bucket.clone();
        // Fees are known; the low end allows for MERs that lag fee changes.
        m.gain_low = saved * c.v("mer-lag", 0.8);
        m.gain_high = saved;
        if is_non_registered(&p.bucket) {
            let (tax, unknown) = if p.basis == "market" {
                tax_on_gain(c, p.value, p.acb)
            } else {
                (
                    0.0,
                    Some(
                        "the market value is unknown, so the gain you would realize is unknown"
                            .into(),
                    ),
                )
            };
            // The tax is due anyway on a later sale: the cost is a year of
            // growth on paying it now.
            m.cost_once = tax * c.mix_return();
            m.cost_note = match unknown {
                Some(u) => format!("Selling in a taxable account realizes gains: {u}."),
                None => format!(
                    "Selling realizes gains: tax about {}, due anyway when you sell later; paying it now costs about {} a year of growth on it.",
                    dollars(tax),
                    dollars(m.cost_once)
                ),
            };
        } else {
            m.cost_note =
                "No tax inside a registered account; trades are free at Wealthsimple.".into();
        }
        m.why = "A lower fee is the one return you can count on.".into();
        m.risk = format!(
            "Check {} holds the same kind of assets as {} (region, stock/bond share) before swapping.",
            cheap.ticker, p.symbol
        );
        m.assumptions = vec![format!(
            "Valued at {} ({}).",
            dollars(p.value),
            if p.basis == "market" {
                "market price"
            } else {
                "cost base, market value unknown"
            }
        )];
        m.evidence = vec!["fees-predict".into(), "etf-mers".into(), "mer-lag".into()];
        let loss = is_non_registered(&p.bucket)
            && p.basis == "market"
            && p.acb.is_some_and(|a| a > p.value);
        if loss {
            m.evidence.push("superficial-loss".into());
            m.cost_note = format!(
                "Selling realizes a loss of about {}: no tax due, and the loss can offset gains.",
                dollars(p.acb.unwrap_or(0.0) - p.value)
            );
        }
        if is_non_registered(&p.bucket) {
            // The prepaid-tax cost reads the planning returns.
            m.evidence.extend([
                "pag-equity".into(),
                "pag-fixed-income".into(),
                "return-band".into(),
            ]);
        } else {
            m.evidence.push("broker-trade-fee".into());
        }
        // Only the task reaches Reminders: a fund from the class-wide
        // fallback may cover another region, so say to check.
        m.task = Some(if same_region.is_some() {
            format!(
                "Swap {} for {} in your {}",
                p.symbol, cheap.ticker, p.bucket
            )
        } else {
            format!(
                "Swap {} for {} in your {}, only if {} holds the same region and stock/bond share",
                p.symbol, cheap.ticker, p.bucket, cheap.ticker
            )
        });
        if loss && let Some(t) = m.task.as_mut() {
            t.push_str(&format!(
                "; no buy of {} in any account for 30 days before and after (pause recurring buys and DRIP), and the replacement must not track the same index, or the loss is denied",
                p.symbol
            ));
        }
        out.push(m);
    }
    out
}

fn location(c: &Ctx) -> Vec<Move> {
    let mut out = vec![];
    let rrsp_room =
        c.i.room
            .iter()
            .find(|r| r.account.eq_ignore_ascii_case("RRSP"))
            .map(|r| r.room)
            .unwrap_or(0.0);
    let wht = c.v("us-withholding", 0.15);
    let yf = c.i.facts.get("us-dividend-yield");
    let (ylow, yhigh) = (
        yf.and_then(|f| f.low).unwrap_or(0.010),
        yf.and_then(|f| f.high).unwrap_or(0.015),
    );
    let room_of = |a: &str| {
        c.i.room
            .iter()
            .find(|r| r.account.eq_ignore_ascii_case(a))
            .map_or(0.0, |r| r.room)
    };
    let bucket_of = |a: &str| -> f64 {
        c.i.buckets
            .iter()
            .filter(|(b, _)| b.eq_ignore_ascii_case(a))
            .map(|(_, v)| v)
            .sum()
    };
    // A swap moves only as much as the other side can take.
    let rrsp_cap = bucket_of("RRSP") + rrsp_room.max(0.0);
    let registered_stocks: f64 =
        c.i.positions
            .iter()
            .filter(|q| !is_non_registered(&q.bucket) && q.asset_class == "equity")
            .map(|q| q.value)
            .sum();
    let bond_cap = registered_stocks + rrsp_room.max(0.0) + room_of("TFSA").max(0.0);
    for p in &c.i.positions {
        let tax_free_non_rrsp =
            matches!(p.bucket.to_uppercase().as_str(), "TFSA" | "FHSA" | "RESP");
        if p.domicile == "US" && p.asset_class == "equity" && tax_free_non_rrsp {
            let mut m = base(
                &format!("location:{}", p.symbol),
                "gain",
                format!(
                    "Hold {} in your RRSP instead of your {}",
                    p.symbol, p.bucket
                ),
            );
            let amount = p.value.min(rrsp_cap);
            if amount <= 0.0 {
                continue;
            }
            m.bucket = p.bucket.clone();
            m.gain_low = amount * ylow * wht;
            m.gain_high = amount * yhigh * wht;
            // Buying the US fund in the RRSP needs USD: Norbert's gambit at
            // best, the broker's conversion fee at worst.
            m.cost_once = amount * c.v("norbert", 0.001);

            m.why = format!(
                "{} is US-listed: its dividends lose {} to US tax in a {}, but not in an RRSP.",
                p.symbol,
                pct(wht),
                p.bucket
            );
            m.cost_note = format!(
                "Swap with a Canadian-listed fund already in the RRSP, so no money crosses accounts. Buying a US-listed fund needs USD: Norbert's gambit costs far less than a {} conversion.",
                pct(c.v("broker-fx-fee", 0.015))
            );
            m.risk = "None from markets if you swap like for like.".into();
            m.assumptions = vec![format!(
                "US dividend yield {} to {} a year.",
                pct(ylow),
                pct(yhigh)
            )];
            if amount < p.value {
                m.assumptions.push(format!(
                    "Your RRSP holds or has room for about {}, so only that much can move.",
                    dollars(amount)
                ));
            }
            if rrsp_room <= 0.0 {
                m.unknowns.push("No RRSP room is recorded; this works only as a swap with what the RRSP already holds.".into());
            }
            m.evidence = vec![
                "us-withholding".into(),
                "us-dividend-yield".into(),
                "norbert".into(),
                "broker-fx-fee".into(),
            ];
            m.task = Some(if amount < p.value {
                format!(
                    "Swap {} of {} into your RRSP (like-for-like swap; get USD by Norbert's gambit, not a {} conversion)",
                    dollars(amount),
                    p.symbol,
                    pct(c.v("broker-fx-fee", 0.015))
                )
            } else {
                format!(
                    "Swap {} into your RRSP (like-for-like swap; get USD by Norbert's gambit, not a {} conversion)",
                    p.symbol,
                    pct(c.v("broker-fx-fee", 0.015))
                )
            });
            out.push(m);
        }
        // A Canadian-listed fund of US stocks loses the 15% inside the fund,
        // even in an RRSP; a US-listed fund held directly there does not.
        let us_listed =
            c.i.cheapest
                .get(&format!("{}:{}:US-listed", p.asset_class, p.region));
        if let Some(us) = us_listed.filter(|_| {
            treaty_exempt(&p.bucket)
                && p.region == "US"
                && p.domicile == "CA"
                && p.asset_class == "equity"
                && p.value > 0.0
        }) {
            let mut m = base(
                &format!("location:{}", p.symbol),
                "gain",
                format!(
                    "Hold {} instead of {} in your {}",
                    us.ticker, p.symbol, p.bucket
                ),
            );
            m.bucket = p.bucket.clone();
            m.gain_low = p.value * ylow * wht;
            m.gain_high = p.value * yhigh * wht;
            // Selling the Canadian-listed fund gives CAD; the US fund needs USD.
            m.cost_once = p.value * c.v("norbert", 0.001);
            m.why = format!(
                "{} is Canadian-listed, so the US keeps {} of its US dividends even in an RRSP. {} is US-listed: held directly in an RRSP, the Canada-US treaty exempts its dividends.",
                p.symbol,
                pct(wht),
                us.ticker
            );
            m.cost_note = format!(
                "Converting about {} to USD by Norbert's gambit costs about {}; a plain broker conversion ({}) would cost far more and can wipe out years of the saving.",
                dollars(p.value),
                dollars(m.cost_once),
                pct(c.v("broker-fx-fee", 0.015))
            );
            m.risk = "None from markets if you swap like for like the same day; the US fund is priced in USD, so the RRSP's value in CAD moves with the exchange rate (as the US stocks inside the Canadian fund already do).".into();
            m.assumptions = vec![format!(
                "US dividend yield {} to {} a year.",
                pct(ylow),
                pct(yhigh)
            )];
            m.unknowns.push(format!(
                "Whether {} is CAD-hedged: a hedged fund does not move with the exchange rate, so swapping it for {} adds currency exposure.",
                p.symbol, us.ticker
            ));
            m.evidence = vec![
                "us-withholding".into(),
                "us-dividend-yield".into(),
                "norbert".into(),
                "broker-fx-fee".into(),
            ];
            m.task = Some(format!(
                "In your RRSP, sell {} and buy {} (get USD by Norbert's gambit, not a {} conversion)",
                p.symbol,
                us.ticker,
                pct(c.v("broker-fx-fee", 0.015))
            ));
            out.push(m);
        }
        if p.asset_class == "bond" && is_non_registered(&p.bucket) {
            let Some(mr) = c.marginal() else { continue };
            let y = c.v("pag-fixed-income", 0.032);
            let mut m = base(
                &format!("location:{}", p.symbol),
                "gain",
                format!("Hold {} (bonds) in a registered account", p.symbol),
            );
            m.bucket = p.bucket.clone();
            // The swap puts the same amount of stocks in the taxable account,
            // so their tax (low: gains deferred; high: all taxed) comes off.
            let e = c.v("pag-equity", 0.065);
            let amount = p.value.min(bond_cap);
            if amount <= 0.0 {
                continue;
            }
            let drag_low = amount
                * (e - c.band()).max(0.0)
                * c.i.inclusion
                * mr
                * c.v("gain-deferral-share", 0.5);
            let drag_high = amount * (e + c.band()) * c.i.inclusion * mr;
            m.gain_low = (amount * (y - c.band()).max(0.0) * mr - drag_high).max(0.0);
            m.gain_high = (amount * (y + c.band()) * mr - drag_low).max(0.0);
            m.why = "Bond interest is taxed at your full marginal rate every year in a taxable account; the stocks swapped in are taxed less (gains at the inclusion rate, and only when sold).".into();
            let (tax, unknown) = if p.basis == "market" {
                tax_on_gain(c, amount, p.acb.map(|a| a * amount / p.value))
            } else {
                (
                    0.0,
                    Some(
                        "the market value is unknown, so the gain you would realize is unknown"
                            .into(),
                    ),
                )
            };
            // Tax due anyway on a later sale: count a year of growth on it.
            m.cost_once = tax * c.mix_return();
            m.cost_note = match unknown {
                Some(u) => format!(
                    "Swap with stocks held in your RRSP or TFSA. Selling {} in the taxable account realizes its gain; {u}.",
                    p.symbol
                ),
                None => format!(
                    "Swap with stocks held in your RRSP or TFSA. Selling {} in the taxable account realizes its gain: about {} of tax, due anyway when you sell later; paying it now costs a year of growth on it.",
                    p.symbol,
                    dollars(tax)
                ),
            };
            m.risk = "None from markets if you swap like for like.".into();
            m.assumptions = vec![format!(
                "Bond yield {} to {}; marginal rate {}.",
                pct((y - c.band()).max(0.0)),
                pct(y + c.band()),
                pct(mr)
            )];
            m.evidence = vec![
                "tfsa-shelter".into(),
                "pag-fixed-income".into(),
                "return-band".into(),
                "pag-equity".into(),
                "gain-deferral-share".into(),
            ];
            m.task = Some(if amount < p.value {
                format!(
                    "Swap {} of {} into a registered account (only up to your room or the registered stocks you swap against)",
                    dollars(amount),
                    p.symbol
                )
            } else {
                format!("Swap {} into a registered account", p.symbol)
            });
            out.push(m);
        }
    }
    out
}

fn harvest(c: &Ctx) -> Vec<Move> {
    let mut out = vec![];
    let Some(mr) = c.marginal() else { return out };
    let employer = c.i.employer.as_ref().map(|e| e.symbol.to_uppercase());
    for p in
        c.i.positions
            .iter()
            .filter(|p| is_non_registered(&p.bucket))
    {
        let (Some(acb), true) = (p.acb, p.basis == "market") else {
            continue;
        };
        let loss = acb - p.value;
        if loss <= 0.0 {
            continue;
        }
        // A loss already flagged as possibly superficial may be denied.
        if c.i.superficial.iter().any(|s| {
            s.split(" ×")
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case(&p.symbol)
        }) {
            continue;
        }
        let deferred = loss * c.i.inclusion * mr;
        let mut m = base(
            &format!("harvest:{}", p.symbol),
            "gain",
            format!("Harvest the {} loss on {}", dollars(loss), p.symbol),
        );
        m.bucket = p.bucket.clone();
        // A capital loss offsets only capital gains (this year, the 3 years
        // before, or later), so with no gains to offset it may wait years.
        m.gain_low = 0.0;
        m.gain_high = deferred * (c.mix_return() + c.band());
        m.why = format!(
            "A capital loss offsets capital gains this year, the past 3 years (T1A), or later, not other income: about {} of tax deferred once it offsets a gain.",
            dollars(deferred)
        );
        m.unknowns.push("Whether you have capital gains to offset: with none, the loss carries forward and saves nothing until you do.".into());
        m.risk = "Buy a similar, not identical, fund at once so you stay invested.".into();
        m.cost_note = "Trades are free at Wealthsimple.".into();
        m.assumptions = vec![
            "The tax comes back when you sell the replacement; the gain is the deferral's time value.".into(),
            format!("Deferral earns {} to {} a year.", pct(c.v("pag-short-term", 0.024)), pct(c.mix_return() + c.band())),
        ];
        if employer.as_deref() == Some(&p.symbol.to_uppercase()) {
            m.unknowns.push(format!(
                "{} is your employer's stock: an RSU vest or ESPP purchase within 30 days before or after the sale denies the loss.",
                p.symbol
            ));
        }
        m.evidence = vec![
            "harvest-defers".into(),
            "superficial-loss".into(),
            "pag-short-term".into(),
            "pag-equity".into(),
            "pag-fixed-income".into(),
            "return-band".into(),
        ];
        m.task = Some(format!(
            "Sell {} at a loss and buy a similar, not identical, fund (no buy of {} for 30 days before or after, in any of your accounts, your spouse's, or a company you control; turn off any DRIP on it first)",
            p.symbol, p.symbol
        ));
        if employer.as_deref() == Some(&p.symbol.to_uppercase())
            && let Some(t) = m.task.as_mut()
        {
            t.push_str("; an RSU vest or ESPP purchase in that window counts as a buy");
        }
        out.push(m);
    }
    out
}

fn concentration(c: &Ctx) -> Option<Move> {
    let e = c.i.employer.as_ref()?;
    let total = c.total();
    if total <= 0.0 || e.value <= 0.0 {
        return None;
    }
    let limit = c.v("concentration-limit", 0.10);
    let share = e.value / total;
    if share <= limit {
        return None;
    }
    let sell = e.value - limit * total;
    let mut m = base(
        &format!("concentration:{}", e.symbol),
        "risk",
        format!(
            "Sell down {} from {} to {} of your investments",
            e.symbol,
            pct(share),
            pct(limit)
        ),
    );
    let (tax, unknown) = tax_on_gain(c, e.value, e.acb);
    m.cost_once = tax * (sell / e.value);
    m.cost_note = match unknown {
        Some(u) => format!("Tax on the sale: {u}. Shares sold right at vest have almost no gain."),
        None => format!(
            "Tax on the gain you realize: about {}.",
            dollars(m.cost_once)
        ),
    };
    m.why = format!(
        "Expected gain about $0: one stock and an index have similar expected returns, but the stock's range is far wider, and your pay depends on the same company. A {} fall in {} would cost you about {} today.",
        pct(c.v("single-stock-drop", 0.5)),
        e.symbol,
        dollars(e.value * c.v("single-stock-drop", 0.5))
    );
    m.risk = "Selling can miss a rally in the stock. Keeping it can lose savings and job at once."
        .into();
    m.assumptions = vec![format!(
        "{} valued at {} ({}); total investments {}.",
        e.symbol,
        dollars(e.value),
        match e.basis.as_str() {
            "market" => "market price",
            "etrade-total" => "E*TRADE account total, assumed to hold only this stock",
            _ => "cost base, market value unknown",
        },
        dollars(total)
    )];
    m.evidence = vec![
        "single-stock-risk".into(),
        "single-stock-drop".into(),
        "concentration-limit".into(),
        "spiva-canada".into(),
    ];
    m.unknowns = vec![format!("How {} will do: nobody knows.", e.symbol)];
    let loss = e.acb.is_some_and(|a| a > e.value);
    if loss {
        m.evidence.push("superficial-loss".into());
        m.cost_note.push_str(" The sale is at a loss: an RSU vest or ESPP purchase within 30 days before or after the sale (and still held 30 days after) denies it; time the sale away from vests.");
    }
    m.task = Some(format!(
        "Decide whether to sell about {} of {} (to {} of investments)",
        dollars(sell),
        e.symbol,
        pct(limit)
    ));
    if loss && let Some(t) = m.task.as_mut() {
        t.push_str(", away from any vest or ESPP purchase within 30 days, or the loss is denied");
    }
    Some(m)
}

fn rebalance(c: &Ctx, unknowns: &mut Vec<String>) -> Option<Move> {
    let total = c.total();
    let known: Vec<&Position> =
        c.i.positions
            .iter()
            .filter(|p| {
                matches!(
                    p.asset_class.as_str(),
                    "equity" | "bond" | "balanced" | "cash"
                )
            })
            .collect();
    let known_value: f64 = known.iter().map(|p| p.value).sum();
    // Crypto and gold have a known kind too; they stay out of the stock share.
    let classified = known_value
        + c.i
            .positions
            .iter()
            .filter(|p| matches!(p.asset_class.as_str(), "crypto" | "gold"))
            .map(|p| p.value)
            .sum::<f64>();
    if total <= 0.0 || known_value <= 0.0 || classified < 0.5 * total {
        unknowns.push(format!(
            "Your stock/bond mix: only {} of your investments have a known asset class, so drift from your {} target is not checked.",
            pct(if total > 0.0 { classified / total } else { 0.0 }),
            pct(c.i.target_equity)
        ));
        return None;
    }
    let equity: f64 = known
        .iter()
        .map(|p| match p.asset_class.as_str() {
            "equity" => p.value,
            "balanced" => p.value * p.equity_share.unwrap_or(0.6),
            _ => 0.0,
        })
        .sum();
    let share = equity / known_value;
    let threshold = c.v("rebalance", 0.05);
    if (share - c.i.target_equity).abs() <= threshold {
        return None;
    }
    let mut m = base(
        "rebalance",
        "risk",
        format!(
            "Rebalance: stocks are {} of your mix, target {}",
            pct(share),
            pct(c.i.target_equity)
        ),
    );
    m.why = "Expected gain about $0: rebalancing keeps your risk where you chose it.".into();
    m.risk = if share > c.i.target_equity {
        "Above target you carry more risk than you chose.".into()
    } else {
        "Below target your long-run growth is lower than you planned.".into()
    };
    m.cost_note = "Cheapest with new money; selling in a taxable account realizes gains.".into();
    m.assumptions = vec![
        format!(
            "Target from {}.",
            if c.i.target_source.is_empty() {
                "your profile"
            } else {
                &c.i.target_source
            }
        ),
        "Balanced funds count at their stated stock share (60% when unknown).".into(),
    ];
    m.evidence = vec!["rebalance".into()];
    // Money needed soon should not move toward more stocks.
    let short = c.v("short-horizon", 60.0);
    if share < c.i.target_equity && !c.i.high_interest_debt.is_some_and(|d| d == 0.0) {
        m.risk = "Pay or record your debt at 8%+ a year first (`high_interest_debt` under [invest]): new money goes there before stocks.".into();
        m.task = None;
        return Some(m);
    }
    if share < c.i.target_equity
        && let (Some(cash), Some(res)) = (c.i.cash, c.i.reserve)
        && cash < res
    {
        m.risk = "Refill your reserve first: new money goes to cash until it reaches your reserve."
            .into();
        m.task = None;
        return Some(m);
    }
    if share < c.i.target_equity && c.i.horizon_months.is_none() {
        m.risk = "Set `horizon_years` under [invest] first: the plan does not know when you need this money.".into();
        m.task = None;
        return Some(m);
    }
    if c.i
        .horizon_months
        .is_some_and(|h| !(h.is_finite() && h >= short))
    {
        m.evidence.push("short-horizon".into());
        if share > c.i.target_equity {
            m.task = Some(format!(
                "Rebalance down to {} stocks; move the money you need within {} months to a high-interest account or GIC",
                pct(c.i.target_equity),
                short
            ));
            return Some(m);
        }
        m.risk = format!(
            "You need money within {} months: it belongs in safe holdings (a high-interest account or GIC), not your long-run mix. Do not add stocks for it.",
            short
        );
        m.task = None;
        return Some(m);
    }
    m.task = Some(format!(
        "Rebalance toward {} stocks, with new money first",
        pct(c.i.target_equity)
    ));
    Some(m)
}

/// Crypto held, by symbol, and its total value.
fn crypto_held(i: &Input) -> (Vec<String>, f64) {
    let mut syms: Vec<String> = Vec::new();
    let mut value = 0.0;
    for p in i
        .positions
        .iter()
        .filter(|p| p.asset_class == "crypto" && p.value > 0.0)
    {
        value += p.value;
        if !syms.contains(&p.symbol) {
            syms.push(p.symbol.clone());
        }
    }
    (syms, value)
}

/// Crypto is its own part of the mix, not stocks. It is checked against the
/// owner's own cap; with no cap the plan says it does not check it.
fn crypto_cap(c: &Ctx, unknowns: &mut Vec<String>) -> Option<Move> {
    let (syms, value) = crypto_held(c.i);
    let total = c.total();
    if value <= 0.0 || total <= 0.0 {
        return None;
    }
    let share = value / total;
    if let Some(m) =
        c.i.crypto_max
            .filter(|m| !(m.is_finite() && (0.0..=1.0).contains(m)))
    {
        unknowns.push(format!(
            "Your crypto cap `crypto_max = {m}` is not a fraction from 0 to 1 (write 0.05 for 5%), so crypto is not checked."
        ));
        return None;
    }
    let Some(max) = c.i.crypto_max else {
        unknowns.push(format!(
            "Your crypto cap: crypto ({}) is {} of your investments. Set `crypto_max` under [invest] in profile.toml (for example 0.05) to have it checked.",
            syms.join(", "),
            pct(share)
        ));
        return None;
    };
    if share <= max {
        return None;
    }
    let mut m = base(
        "crypto",
        "risk",
        format!(
            "Crypto is {} of your investments, your cap {}",
            pct(share),
            pct(max)
        ),
    );
    m.why =
        "Expected gain about $0: keeping crypto under your cap keeps your risk where you chose it."
            .into();
    m.risk = format!(
        "Crypto has fallen {} or more within a year more than once; above your cap a fall like that hurts more than you planned.",
        pct(-c.v("crypto-drawdown", -0.7))
    );
    m.evidence = vec!["crypto-drawdown".into()];
    m.cost_note = "Selling or swapping crypto is a disposition: part of any gain is taxable. New money into other funds costs nothing.".into();
    m.assumptions = vec![
        format!("Crypto held: {}.", syms.join(", ")),
        "Your cap comes from `crypto_max` in [invest].".into(),
    ];
    // New money goes to funds only when it is safe to invest at all.
    let short = c.v("short-horizon", 60.0);
    let safe_only =
        c.i.horizon_months
            .is_none_or(|h| !(h.is_finite() && h >= short));
    m.task = Some(if !c.i.high_interest_debt.is_some_and(|d| d == 0.0) {
        format!(
            "Bring crypto back under {}: sell the excess and pay your debt at 8%+ first, not new funds",
            pct(max)
        )
    } else if c
        .i
        .cash
        .zip(c.i.reserve)
        .is_some_and(|(cash, res)| cash < res)
    {
        format!(
            "Bring crypto back under {}: sell the excess into cash until your reserve is full, not new funds",
            pct(max)
        )
    } else if safe_only {
        m.evidence.push("short-horizon".into());
        format!(
            "Bring crypto back under {}: move the excess to safe holdings (a high-interest account or GIC), not stocks",
            pct(max)
        )
    } else {
        format!(
            "Bring crypto back under {}: put new money into your other funds first",
            pct(max)
        )
    });
    Some(m)
}

/// One line with the share of each part of the owner's whole mix.
fn mix_line(i: &Input) -> Option<String> {
    let total: f64 = i.positions.iter().map(|p| p.value).sum();
    if total <= 0.0 {
        return None;
    }
    let mut parts = [0.0f64; 6];
    for p in &i.positions {
        match p.asset_class.as_str() {
            "equity" => parts[0] += p.value,
            "balanced" => {
                let e = p.equity_share.unwrap_or(0.6);
                parts[0] += e * p.value;
                parts[1] += (1.0 - e) * p.value;
            }
            "bond" => parts[1] += p.value,
            "cash" => parts[2] += p.value,
            "crypto" => parts[3] += p.value,
            "gold" => parts[4] += p.value,
            _ => parts[5] += p.value,
        }
    }
    let (syms, _) = crypto_held(i);
    let names = [
        ("stocks", String::new()),
        ("bonds", String::new()),
        ("cash", String::new()),
        ("crypto", format!(" ({})", syms.join(", "))),
        ("gold", String::new()),
        ("of unknown kind", String::new()),
    ];
    let shown: Vec<String> = names
        .iter()
        .zip(parts)
        .filter(|(_, v)| *v > 0.0)
        .map(|((word, rest), v)| format!("{word} {}{rest}", pct(v / total)))
        .collect();
    Some(format!(
        "Your whole mix: {} of {}. Crypto and gold do not count as stocks.\n",
        shown.join(" · "),
        dollars(total)
    ))
}

fn do_nothing(c: &Ctx) -> Move {
    let mut m = base("do-nothing", "baseline", "Do nothing".into());
    m.why = "Zero effort, zero trading cost, no new tax. It is the baseline every move must beat."
        .into();
    let mut risks = vec![];
    if let (Some(cash), Some(res)) = (c.i.cash, c.i.reserve)
        && cash > res
    {
        // Today's inflation when known (live), else the long-run assumption.
        let real = c.i.cash_rate
            - c.i
                .facts
                .get("live-cpi-yoy")
                .and_then(|f| f.value)
                .unwrap_or_else(|| c.v("pag-inflation", 0.021));
        risks.push(format!(
            "the {} above your reserve earns {} a year after inflation ({})",
            dollars(cash - res),
            pct(real),
            dollars((cash - res) * real)
        ));
    }
    if let Some(e) = &c.i.employer {
        let total = c.total();
        if total > 0.0 && e.value / total > c.v("concentration-limit", 0.10) {
            risks.push(format!(
                "{} stays {} of your investments",
                e.symbol,
                pct(e.value / total)
            ));
        }
    }
    m.risk = if risks.is_empty() {
        "None beyond the markets you already hold.".into()
    } else {
        format!("Your money keeps today's risks: {}.", risks.join("; "))
    };
    m.evidence = vec![
        if c.i.facts.contains_key("live-cpi-yoy") {
            "live-cpi-yoy"
        } else {
            "pag-inflation"
        }
        .into(),
    ];
    m
}

/// Evidence every move cites by its kind: tax rules, fund data, typed prices.
/// `veto` adds it too, so every check judges the same evidence.
fn add_evidence(i: &Input, m: &mut Move) {
    // Moves that read the tax rules (inclusion rate, account room) cite
    // them, so a tax year past its review date blocks them.
    let reads_tax = m.id == "fill-room"
        || (m.id == "cash-drag" && m.evidence.iter().any(|e| e == "short-horizon"))
        || m.cost_once > 0.0
        || ["harvest:", "location:", "fee:"]
            .iter()
            .any(|p| m.id.starts_with(p));
    if reads_tax
        && i.facts.contains_key("tax-rules")
        && !m.evidence.iter().any(|e| e == "tax-rules")
    {
        m.evidence.push("tax-rules".into());
    }
    // Fund fees come from investing.toml; its review date guards them.
    if (m.id.starts_with("fee:") || m.id == "cash-drag")
        && i.facts.contains_key("investing-data")
        && !m.evidence.iter().any(|e| e == "investing-data")
    {
        m.evidence.push("investing-data".into());
    }
    // Moves that sell or move holdings at a price you gave need a recent price.
    if ["harvest:", "fee:", "location:"]
        .iter()
        .any(|p| m.id.starts_with(p))
        || m.id == "fill-room"
    {
        if i.facts.contains_key("prices") && !m.evidence.iter().any(|e| e == "prices") {
            m.evidence.push("prices".into());
        }
    }
}

/// Moves that put new money into investments.
fn invests_money(m: &Move) -> bool {
    m.id == "fill-room" || m.id == "cash-drag"
}

/// Deterministic safety rules, outside any model (TradingAgents' risk team,
/// "The Alpha Illusion" arXiv 2605.16895). The first rule that holds blocks the move.
pub fn veto(i: &Input, m: &Move) -> Option<String> {
    let mut full = m.clone();
    add_evidence(i, &mut full);
    let m = &full;
    if m.kind == "baseline" {
        return None;
    }
    if invests_money(m) {
        // fill-room moves invested funds, not cash, so the reserve rule skips it.
        if let (Some(cash), Some(res)) = (i.cash, i.reserve)
            && m.id == "cash-drag"
            && cash < res
        {
            return Some(format!(
                "reserve first: your cash ({}) is below your reserve ({})",
                dollars(cash),
                dollars(res)
            ));
        }
        if let Some(d) = i.high_interest_debt.filter(|d| *d > 0.0) {
            return Some(format!(
                "debt first: paying {} of debt at {}+ a year (`high-interest-debt-rate`) is a sure return that beats investing or saving",
                dollars(d),
                pct(i
                    .facts
                    .get("high-interest-debt-rate")
                    .and_then(|f| f.value)
                    .unwrap_or(0.08))
            ));
        }
        // The safe-account version (money needed soon) is saving, not investing.
        let stocks = m.id != "cash-drag" || m.title.starts_with("Invest");
        if stocks
            && m.id == "cash-drag"
            && i.high_interest_debt
                .is_none_or(|d| !(d.is_finite() && d >= 0.0))
        {
            return Some(format!(
                "debt unknown: the plan does not know whether you carry debt at {}+ a year; set `high_interest_debt` under [invest] (0 when you have none) before investing cash",
                pct(i
                    .facts
                    .get("high-interest-debt-rate")
                    .and_then(|f| f.value)
                    .unwrap_or(0.08))
            ));
        }
        if stocks && m.id == "cash-drag" && i.horizon_months.is_none() {
            return Some(
                "horizon first: the plan does not know when you need this money; set `horizon_years` under [invest] before investing it".into(),
            );
        }
    }
    if let Some(id) = m
        .evidence
        .iter()
        .find(|id| !id.starts_with("live-") && !i.facts.contains_key(*id))
    {
        return Some(format!(
            "missing evidence: `{id}` is not in the knowledge files, so the numbers behind this move are undated defaults"
        ));
    }
    let today = chrono::NaiveDate::parse_from_str(&i.today, "%Y-%m-%d").ok();
    let stale: Vec<&String> = m
        .evidence
        .iter()
        .filter(|id| {
            i.facts.get(*id).is_some_and(|f| {
                !f.review_by.is_empty()
                    && today.is_none_or(|t| {
                        chrono::NaiveDate::parse_from_str(&f.review_by, "%Y-%m-%d")
                            .map_or(true, |d| d < t)
                    })
            })
        })
        .collect();
    // A risk warning stays visible on old data: hiding it would be worse.
    if !stale.is_empty() && m.kind != "risk" {
        return Some(format!(
            "stale evidence: {} is past its review date; refresh it (`knowledge check`, or for `prices` update [invest.prices] and `prices_as_of`) before acting",
            stale
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    None
}

/// The strongest case against a move (the bear side of a bull/bear debate),
/// from the move's own kind and its weakest cited fact.
pub fn objection(i: &Input, m: &Move) -> String {
    let by_kind = match m.id.split(':').next().unwrap_or("") {
        "do-nothing" => {
            "Idle cash and drift compound quietly; doing nothing for years has a real cost."
        }
        "fee" => {
            "The cheaper fund may track a different index; in a taxable account the sale can cost more in tax than years of fee savings."
        }
        "cash-drag" => {
            return format!(
                "Markets can fall about {} in a bad year (estimate, `one-year-drop`); money invested now may be needed before it recovers.",
                pct(i
                    .facts
                    .get("one-year-drop")
                    .and_then(|f| f.value)
                    .unwrap_or(-0.42)
                    .abs())
            );
        }
        "fill-room" => {
            "Registered room never expires; moving money in now only helps if you will not need it back soon."
        }
        "location" => {
            "Tax rules and your income change; the better account today may not be better later."
        }
        "harvest" => {
            "A tax loss only defers tax, and a repurchase within 30 days voids it (superficial loss)."
        }
        "concentration" => "Selling employer stock realizes tax now and may miss its upside.",
        "crypto" => {
            "Crypto has risen more than stocks in some years; trimming it can miss that upside, and selling realizes tax."
        }
        "rebalance" => {
            "Selling winners in a taxable account realizes gains; new money is the cheaper way."
        }
        _ => "This move rests on long-run assumptions that any one year can miss.",
    };
    let weak = m.evidence.iter().find_map(|id| {
        let f = i.facts.get(id)?;
        matches!(f.confidence.as_str(), "estimate" | "heuristic" | "guess")
            .then(|| format!(" Weakest evidence: `{id}` is a {} only.", f.confidence))
    });
    format!("{by_kind}{}", weak.unwrap_or_default())
}

fn overridden(i: &Input, m: &Move) -> Option<String> {
    let id = m.id.as_str();
    let today = chrono::NaiveDate::parse_from_str(&i.today, "%Y-%m-%d").ok();
    i.overrides
        .iter()
        .filter(|o| {
            o.until.is_empty()
                || chrono::NaiveDate::parse_from_str(&o.until, "%Y-%m-%d")
                    .is_ok_and(|u| today.is_some_and(|t| t <= u))
        })
        .find(|o| {
            // A risk warning never hides behind an override with no reason.
            if m.kind == "risk" && o.reason.trim().is_empty() {
                return false;
            }
            // A risk warning hides only behind its own exact id.
            if m.kind == "risk" {
                return o.move_id == id;
            }
            // A move renamed with its account (`fee:X:tfsa`) still answers to
            // `fee:X`, `fee` and `X`; the full id targets one account.
            let suffix = format!(":{}", m.bucket.to_lowercase().replace(' ', "-"));
            let plain = if m.bucket.is_empty() {
                id
            } else {
                id.strip_suffix(suffix.as_str()).unwrap_or(id)
            };
            o.move_id == id
                || o.move_id == plain
                || (!m.bucket.is_empty() && o.move_id == format!("{plain}{suffix}"))
                || plain.split_once(':').map(|(kind, _)| kind) == Some(o.move_id.as_str())
                || plain
                    .split_once(':')
                    .map(|(_, sym)| sym.eq_ignore_ascii_case(&o.move_id))
                    == Some(true)
        })
        .map(|o| {
            let reason = if o.reason.trim().is_empty() {
                "no reason given".to_string()
            } else {
                o.reason.trim().to_string()
            };
            let reason = if o.move_id == id {
                reason
            } else {
                format!("{reason} (matched by `{}`)", o.move_id)
            };
            if o.until.is_empty() {
                reason
            } else {
                format!("{reason} (until {})", o.until)
            }
        })
}

pub fn plan(i: &Input) -> Plan {
    let c = Ctx { i };
    let mut out = Plan::default();
    let mut gain: Vec<Move> = vec![];
    gain.extend(cash_drag(&c));
    gain.extend(fill_room(&c));
    gain.extend(fees(&c));
    let located = location(&c);
    // One sale per holding: the RRSP US-listed swap replaces that symbol's fee swap.
    gain.retain(|f| {
        !f.id.starts_with("fee:")
            || !located.iter().any(|l| {
                treaty_exempt(&l.bucket)
                    && l.id.strip_prefix("location:") == f.id.strip_prefix("fee:")
                    && l.bucket.eq_ignore_ascii_case(&f.bucket)
            })
    });
    gain.extend(located);
    gain.extend(harvest(&c));
    let mut risk: Vec<Move> = vec![];
    risk.extend(concentration(&c));
    risk.extend(rebalance(&c, &mut out.unknowns));
    risk.extend(crypto_cap(&c, &mut out.unknowns));
    let mut unknown_kind: Vec<String> = Vec::new();
    for p in i.positions.iter().filter(|p| {
        !matches!(
            p.asset_class.as_str(),
            "equity" | "bond" | "balanced" | "cash" | "crypto" | "gold"
        )
    }) {
        if !unknown_kind.contains(&p.symbol) {
            unknown_kind.push(p.symbol.clone());
        }
    }
    if !unknown_kind.is_empty() {
        out.unknowns.push(format!(
            "The kind of {}: name each under [invest.classes] in profile.toml (`AAPL = \"equity\"`; equity, bond, balanced, cash, crypto or gold), so your whole mix is checked.",
            unknown_kind.join(", ")
        ));
    }
    for o in i.overrides.iter().filter(|o| {
        !o.until.is_empty() && chrono::NaiveDate::parse_from_str(&o.until, "%Y-%m-%d").is_err()
    }) {
        out.unknowns.push(format!(
            "Your override of `{}` has an `until` date the plan cannot read (\"{}\"); write it as YYYY-MM-DD. Until then it is ignored.",
            o.move_id, o.until
        ));
    }
    if !i.facts.keys().any(|k| k.starts_with("live-")) {
        out.unknowns.push(
            "Today's Bank of Canada rates: none is fresh, so safe-rate and inflation numbers use the yearly planning assumptions. Run `bin/cashcast knowledge refresh`.".into(),
        );
    }
    if i.cash.is_none() || i.reserve.is_none() {
        out.unknowns.push("Cash above your reserve: not checked, because your cash balance or reserve is missing. Import your bank exports and check the reserve section.".into());
    }
    if i.high_interest_debt
        .is_some_and(|d| !(d.is_finite() && d >= 0.0))
    {
        out.unknowns.push("Your `high_interest_debt` is negative or not a number, so it counts as unknown: write the debt as a positive amount (0 when you have none).".into());
    }
    if i.high_interest_debt.is_none() {
        out.unknowns.push("Debt at 8% a year or more: not recorded, so the debt-first rule is not checked. Set `high_interest_debt` under [invest] in profile.toml (0 when you have none).".into());
    }
    if c.marginal().is_none() {
        out.unknowns.push("Your marginal tax rate: add [income] to profile.toml, so tax-based moves can be sized.".into());
    }
    if i.positions.iter().any(|p| p.basis != "market") {
        out.unknowns.push(
            "Market prices per holding: some holdings are valued at cost. Add [invest.prices] to profile.toml for fee, tax and loss moves.".into(),
        );
    }
    if !i.superficial.is_empty() {
        out.unknowns.push(format!(
            "Possible superficial losses already this year: {}. A denied loss is added to the cost base of the shares bought back in a taxable account; if the buy was in a TFSA, RRSP or FHSA, the loss is gone for good. Check where each buy happened.",
            i.superficial.join(", ")
        ));
    }
    // fill-room sells the whole taxable account into the TFSA: a bond there
    // needs no separate swap, and a fee swap rides on the same sale (its
    // tax is already in fill-room's cost).
    let nonreg: f64 = i
        .buckets
        .iter()
        .filter(|(b, _)| is_non_registered(b))
        .map(|(_, v)| v)
        .sum();
    let room = i
        .room
        .iter()
        .find(|r| r.account.eq_ignore_ascii_case("TFSA"))
        .map_or(0.0, |r| r.room);
    let cash_extra = tfsa_cash(&c);
    let covers_all = nonreg > 0.0
        && room - cash_extra >= nonreg - 0.5
        && gain.iter().any(|f| {
            // Only a sale that will happen: not blocked, not skipped, worth a task.
            f.id == "fill-room"
                && veto(i, f).is_none()
                && overridden(i, f).is_none()
                && f.net() >= i.simplicity
        });
    if covers_all {
        // Only moves on the taxable account's own holdings: the TFSA move
        // sells them all (losses included), so a separate swap or loss sale
        // on the same dollars would count twice.
        // Coins never go into the TFSA, so their loss sale stays.
        let coins: Vec<&str> = i
            .positions
            .iter()
            .filter(|p| is_non_registered(&p.bucket) && p.asset_class == "crypto")
            .map(|p| p.symbol.as_str())
            .collect();
        gain.retain(|m| {
            let coin =
                m.id.strip_prefix("harvest:")
                    .is_some_and(|s| coins.contains(&s.split(':').next().unwrap_or(s)));
            coin || !(is_non_registered(&m.bucket)
                && (m.id.starts_with("location:") || m.id.starts_with("harvest:")))
        });
        for m in gain.iter_mut() {
            if m.id.starts_with("fee:") && is_non_registered(&m.bucket) {
                m.cost_once = 0.0;
                m.cost_note = "Do it in the same sale as `fill-room`: rebuy the cheaper fund in the TFSA. The sale tax is counted there once.".into();
                // Only the task reaches Reminders: say where to buy.
                let swap = m.task.as_deref().and_then(|t| {
                    let (old, rest) = t.strip_prefix("Swap ")?.split_once(" for ")?;
                    let (new, _) = rest.split_once(" in your ")?;
                    Some((old.to_string(), new.to_string()))
                });
                if let Some((old, new)) = swap {
                    m.task = Some(format!(
                        "When you move {old} into your TFSA (`fill-room`), buy {new} in the TFSA instead of {old}"
                    ));
                }
            }
        }
    }
    // The same fund in two accounts makes two moves: name the account so
    // each task and override has its own id.
    let ids: Vec<String> = gain.iter().map(|m| m.id.clone()).collect();
    for m in gain.iter_mut() {
        if !m.bucket.is_empty() && ids.iter().filter(|x| **x == m.id).count() > 1 {
            m.id = format!("{}:{}", m.id, m.bucket.to_lowercase().replace(' ', "-"));
        }
    }
    let fee_ids: Vec<String> = gain
        .iter()
        .filter_map(|m| m.id.strip_prefix("fee:").map(String::from))
        .collect();
    for m in gain.iter_mut() {
        if let Some(sym) = m.id.strip_prefix("harvest:")
            && fee_ids.iter().any(|f| f == sym)
        {
            m.assumptions.push(format!(
                "Swapping {sym} for its cheaper fund (`fee:{sym}`) does both moves in one sale."
            ));
        }
    }
    for mut m in gain.into_iter().chain(risk) {
        add_evidence(i, &mut m);
        // A taxable sale whose gain is unknown says to check it first.
        if (m.id.starts_with("fee:") || m.id.starts_with("location:"))
            && is_non_registered(&m.bucket)
            && m.cost_once == 0.0
            && m.cost_note.contains("unknown")
            && let Some(t) = m.task.as_mut()
        {
            t.push_str(", only after you check the capital gain you would realize (cost base or price unknown)");
        }
        m.objection = objection(i, &m);
        if let Some(rule) = veto(i, &m) {
            m.vetoed = Some(rule);
            m.task = None;
            out.vetoed.push(m);
            continue;
        }
        if let Some(reason) = overridden(i, &m) {
            m.overridden = Some(reason);
            m.task = None;
            out.overridden.push(m);
            continue;
        }
        if m.kind == "gain" && m.net() < i.simplicity.max(0.01) {
            m.task = None;
        }
        if m.kind == "risk" {
            out.risk.push(m);
        } else {
            out.ranked.push(m);
        }
    }
    let mut dn = do_nothing(&c);
    dn.objection = objection(i, &dn);
    out.ranked.push(dn);
    // A move whose numbers are not finite cannot be ranked honestly.
    let (ok, bad): (Vec<Move>, Vec<Move>) = std::mem::take(&mut out.ranked)
        .into_iter()
        .partition(|m| m.gain_low.is_finite() && m.gain_high.is_finite() && m.net().is_finite());
    out.ranked = ok;
    for m in bad {
        out.unknowns.push(format!(
            "{}: its numbers are not valid; check the inputs",
            m.title
        ));
    }
    out.ranked.sort_by(|a, b| b.net().total_cmp(&a.net()));
    // Ties and moves below the simplicity threshold lose to "do nothing".
    let best_worth_it = out
        .ranked
        .iter()
        .filter(|m| m.kind == "gain")
        .any(|m| m.net() >= i.simplicity.max(0.01));
    if !best_worth_it {
        let at = out
            .ranked
            .iter()
            .position(|m| m.kind == "baseline")
            .unwrap_or(0);
        let dn = out.ranked.remove(at);
        out.ranked.insert(0, dn);
    }
    out.do_nothing_wins = out
        .ranked
        .first()
        .map(|m| m.kind == "baseline")
        .unwrap_or(true);
    out
}

fn evidence_lines(i: &Input, ids: &[String]) -> Vec<String> {
    ids.iter()
        .map(|id| match i.facts.get(id) {
            Some(f) => {
                let stale = !f.review_by.is_empty() && !i.today.is_empty() && f.review_by < i.today;
                format!(
                    "  - `{id}` ({}{}): {} {}, as of {}, review by {}.",
                    if f.confidence.is_empty() { "unrated" } else { &f.confidence },
                    if stale { ", past its review date: check it before acting" } else { "" },
                    f.text,
                    if f.judgment || !f.source.starts_with("http") {
                        "No external source: a judgment call".to_string()
                    } else {
                        format!("[source]({})", f.source)
                    },
                    f.as_of,
                    f.review_by
                )
            }
            None => format!("  - `{id}`: missing from knowledge/data/invest_evidence.toml, so this claim is unsupported."),
        })
        .collect()
}

fn range(m: &Move) -> String {
    if m.kind == "baseline" {
        "$0 (baseline)".into()
    } else if m.gain_low == 0.0 && m.gain_high == 0.0 {
        "about $0 (risk move)".into()
    } else {
        format!("{} to {} a year", dollars(m.gain_low), dollars(m.gain_high))
    }
}

fn move_block(i: &Input, n: usize, m: &Move) -> String {
    let mut s = format!("### {n}. {} (`{}`)\n\n", m.title, m.id);
    if let Some(t) = &m.task {
        s += &format!("- **Do:** {t}\n");
    }
    s += &format!("- **Gain vs. doing nothing:** {}.", range(m));
    if m.cost_once > 0.0 {
        s += &format!(" First year after costs: about {}.", dollars(m.net()));
    }
    s += "\n";
    s += &format!("- **Why:** {}\n", m.why);
    if !m.risk.is_empty() {
        s += &format!("- **Risk:** {}\n", m.risk);
    }
    if !m.cost_note.is_empty() || m.cost_once > 0.0 {
        s += &format!(
            "- **Cost:** {}\n",
            if m.cost_note.is_empty() {
                "none known"
            } else {
                &m.cost_note
            }
        );
    }
    for a in &m.assumptions {
        s += &format!("- **Assumes:** {a}\n");
    }
    if !m.objection.is_empty() {
        s += &format!("- **Strongest objection:** {}\n", m.objection);
    }
    if let Some(rule) = &m.vetoed {
        s += &format!("- **Blocked:** {rule}.\n");
    }
    for u in &m.unknowns {
        s += &format!("- **Unknown:** {u}\n");
    }
    if !m.evidence.is_empty() {
        s += "- **Evidence:**\n";
        for line in evidence_lines(i, &m.evidence) {
            s += &line;
            s += "\n";
        }
    }
    if m.kind == "gain" && m.task.is_none() && m.overridden.is_none() && m.vetoed.is_none() {
        s += &format!(
            "- Below your simplicity threshold ({} a year): no task.\n",
            dollars(i.simplicity)
        );
    }
    s + "\n"
}

/// Plain words for the terms a plan uses, glossed the first time each appears
/// outside a heading, code span or link.
const GLOSSARY: &[(&str, &str)] = &[
    ("cost base", "what you paid, including fees"),
    (
        "superficial loss",
        "a loss the CRA denies when you rebuy within 30 days",
    ),
    ("disposition", "a sale or swap the CRA taxes"),
    ("marginal rate", "the tax on your next dollar of income"),
    ("GIC", "a deposit with a fixed rate and term"),
    ("CDIC", "the federal deposit insurance"),
    ("FHSA", "First Home Savings Account"),
    ("TFSA", "Tax-Free Savings Account"),
    ("RRSP", "Registered Retirement Savings Plan"),
    ("MER", "the fund's yearly fee"),
    ("DRIP", "automatic dividend reinvestment"),
];

fn gloss(md: &str) -> String {
    let mut done = vec![false; GLOSSARY.len()];
    let mut out = String::with_capacity(md.len() + 512);
    for line in md.split_inclusive('\n') {
        if line.starts_with('#') || line.starts_with('|') {
            out += line;
            continue;
        }
        let mut l = line.to_string();
        for (n, (term, plain)) in GLOSSARY.iter().enumerate() {
            if done[n] {
                continue;
            }
            let Some(at) = find_word(&l, term) else {
                continue;
            };
            let before = &l[..at];
            // Skip a term inside a code span or a link target.
            if before.matches('`').count() % 2 == 1 || before.rfind("](") > before.rfind(')') {
                continue;
            }
            let end = at + term.len();
            l = format!("{} ({plain}){}", &l[..end], &l[end..]);
            done[n] = true;
        }
        out += &l;
    }
    out
}

/// Byte index of `term` as a whole word.
fn find_word(s: &str, term: &str) -> Option<usize> {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric());
    let mut from = 0;
    while let Some(k) = s[from..].find(term) {
        let at = from + k;
        let end = at + term.len();
        if !word(s[..at].chars().last()) && !word(s[end..].chars().next()) {
            return Some(at);
        }
        from = end;
    }
    None
}

pub fn render(i: &Input, p: &Plan) -> String {
    gloss(&render_plain(i, p))
}

fn render_plain(i: &Input, p: &Plan) -> String {
    let mut s = String::from("## Investment moves: ranked, with evidence\n\n");
    // Answer first: the one decision, before the caveats and the table.
    match p.ranked.first() {
        Some(m) if !p.do_nothing_wins => {
            s += &format!(
                "**Answer: do move 1.** {}. {} {}. *Not personal advice: you decide.*\n\n",
                {
                    let full = m.task.as_deref().unwrap_or(&m.title);
                    let head = full.split(';').next().unwrap_or("").trim_end_matches('.');
                    if head.len() < full.trim_end_matches('.').len() {
                        format!("{head} (first do the steps in move 1)")
                    } else {
                        head.to_string()
                    }
                },
                // Money needed soon earns a safe rate only until then.
                if m.id == "cash-drag" && !m.title.starts_with("Invest") {
                    "Gain until you need it:"
                } else {
                    "Long-run gain"
                },
                range(m)
            );
        }
        _ => {
            // Only a blocked move worth doing takes the headline.
            if let Some(v) = p
                .vetoed
                .iter()
                .find(|v| v.kind == "gain" && v.net() >= i.simplicity)
            {
                s += &format!(
                    "**Answer: no move until a safety rule clears.** {} is blocked: {}. Fix that first, then run the plan again.\n\n",
                    v.title,
                    v.vetoed
                        .as_deref()
                        .unwrap_or("see \"Blocked by a safety rule\" below")
                );
            } else {
                s += "**Answer: no move needed now.** No move clears your simplicity threshold after its costs. That is a fine result if nothing is listed under what this plan does not know. \
                  Look again when new cash arrives, when your mix drifts past its band, or in a year.\n\n";
            }
            if let Some(r) = p.risk.iter().find(|m| m.task.is_some()) {
                s += &format!(
                    "**Also decide:** {} (a risk move, no expected gain; see below).\n\n",
                    r.task.as_deref().unwrap_or("")
                );
            }
        }
    }
    s += "*Education and arithmetic on your own data, not personal financial advice. This assistant is not a licensed financial advisor. \
          No stock picks, no market timing: every gain below is a range from long-run planning assumptions, not a forecast. \
          No provider pays for any recommendation. The ranking is fixed rules in code; chat explanations come from an AI model and can be wrong.*\n\n";
    s += "Question any move: say \"challenge `<id>`\" (skill `invest-challenge`). Override one in profile.toml under \
          `[[invest.overrides]]` with `move`, `reason` and an optional `until` date; the plan respects it.\n\n";
    s += "| # | Move | Long-run average gain a year (range, not next year) | One-time cost | Task |\n|---|---|---|---|---|\n";
    for (n, m) in p.ranked.iter().enumerate() {
        let gain = range(m);
        let cost = if m.cost_once > 0.0 {
            dollars(m.cost_once)
        } else {
            "—".into()
        };
        let task = if m.task.is_some() { "yes" } else { "no" };
        s += &format!(
            "| {} | {} (`{}`) | {} | {} | {} |\n",
            n + 1,
            m.title,
            m.id,
            gain,
            cost,
            task
        );
    }
    s += "\n";
    for (n, m) in p.ranked.iter().enumerate() {
        s += &move_block(i, n + 1, m);
    }
    if !p.risk.is_empty() {
        s += "## Risk moves (no expected gain; they cut the chance of a large loss)\n\n";
        for (n, m) in p.risk.iter().enumerate() {
            s += &move_block(i, n + 1, m);
        }
    }
    if !p.vetoed.is_empty() {
        s += "## Blocked by a safety rule\n\nThese moves could gain, but a rule outside the model blocks them until it clears. No task is made.\n\n";
        for (n, m) in p.vetoed.iter().enumerate() {
            s += &move_block(i, n + 1, m);
        }
    }
    if !p.overridden.is_empty() {
        s += "## Your choices\n\nYou overrode these moves. The plan respects each one and shows its estimated cost once.\n\n";
        for m in &p.overridden {
            let cost = if m.kind == "risk" {
                format!("expected cost about $0; the risk stays: {}", m.risk)
            } else {
                format!("forgone gain {}", range(m))
            };
            s += &format!(
                "- `{}` {}: your reason: \"{}\". Estimated cost: {}.\n",
                m.id,
                m.title,
                m.overridden.as_deref().unwrap_or(""),
                cost
            );
        }
        s += "\n";
    }
    if !p.unknowns.is_empty() {
        s += "## What this plan does not know\n\n";
        for u in &p.unknowns {
            s += &format!("- {u}\n");
        }
        s += "\n";
    }
    if let Some(line) = mix_line(i) {
        s += &line;
    }
    s += &format!(
        "Target mix: {} stocks, from {}. Agents never move money: each task is yours to do.\n",
        pct(i.target_equity),
        if i.target_source.is_empty() {
            "your profile"
        } else {
            &i.target_source
        }
    );
    if !i.today.is_empty() {
        let oldest = i
            .facts
            .values()
            .map(|f| f.as_of.as_str())
            .filter(|d| !d.is_empty())
            .min();
        s += &format!(
            "Made on {}{}.\n",
            i.today,
            oldest.map_or(String::new(), |d| format!("; oldest evidence as of {d}"))
        );
    }
    s
}

#[derive(Serialize)]
pub struct Output<'a> {
    pub markdown: String,
    pub do_nothing_wins: bool,
    pub moves: Vec<&'a Move>,
}

/// The engine's `invest-plan` output: the Markdown plus every move as JSON.
pub fn run(text: &str) -> Result<String, String> {
    let input: Input = serde_json::from_str(text).map_err(|e| format!("invest-plan input: {e}"))?;
    let p = plan(&input);
    let out = Output {
        markdown: render(&input, &p),
        do_nothing_wins: p.do_nothing_wins,
        moves: p
            .ranked
            .iter()
            .chain(&p.risk)
            .chain(&p.overridden)
            .chain(&p.vetoed)
            .collect(),
    };
    serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
}

/// Retirement plans the Canada-US treaty (Art. XXI) exempts from US withholding
/// on dividends of US-listed funds held directly: RRSP, RRIF and locked-in plans.
fn treaty_exempt(bucket: &str) -> bool {
    ["RRSP", "RRIF", "LIRA", "LIF"]
        .iter()
        .any(|b| bucket.eq_ignore_ascii_case(b))
}
