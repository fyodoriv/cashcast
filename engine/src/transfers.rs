//! Transfer plan: the balances an agent read from the bank sites in, the
//! e-Transfers that bring each bank to its keep amount out.
//!
//! Each bank keeps `keep` in chequing, plus what its own card needs to come
//! down to `target`. The hub account (`[transfers] hub`) sends what a bank is
//! short and takes back what a bank holds above that. One account sends at
//! most `daily_limit` a day, and every amount is whole dollars that are quick
//! to type (few runs of the same digit: 999, 2000, 1555). Nothing here moves
//! money: the owner sends each e-Transfer.

use std::collections::BTreeMap;

use crate::money::fmt_cents;

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub keep: i64,
}

#[derive(Debug, Clone)]
pub struct Card {
    pub id: String,
    pub target: i64,
    pub paid_from: String,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub hub: String,
    pub daily_limit: u32,
    pub min_move: u32,
    /// Percent above `keep` each bank aims for, as a safety margin (0 to 20).
    pub buffer: u32,
    pub accounts: Vec<Account>,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Move {
    pub day: u32,
    pub from: String,
    pub to: String,
    pub dollars: u32,
}

/// Pay a card's whole balance from its bank, after the money lands.
#[derive(Debug, Clone, PartialEq)]
pub struct Payment {
    pub card: String,
    pub from: String,
    pub cents: i64,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub moves: Vec<Move>,
    pub payments: Vec<Payment>,
    /// Cents each account needs (positive) or holds above its need (negative).
    pub gaps: BTreeMap<String, i64>,
}

const DEFAULT_DAILY_LIMIT: u32 = 3000;
const DEFAULT_MIN_MOVE: u32 = 100;
const DEFAULT_BUFFER: u32 = 5;
const MAX_BUFFER: u32 = 20;

fn cents(v: Option<&toml::Value>) -> i64 {
    match v {
        Some(toml::Value::Integer(i)) => i * 100,
        Some(toml::Value::Float(f)) => (f * 100.0).round() as i64,
        _ => 0,
    }
}

/// Reads `[[banking.accounts]]` (`id`, `keep`), `[[wallet.cards]]` (`id`,
/// `target`, optional `paid_from`) and `[transfers]` (`hub`, `daily_limit`,
/// `min_move`) from a profile. A card without `paid_from` is paid from the
/// account whose id starts with the same word (`td-…` card → `td-…` account).
pub fn parse(text: &str) -> Result<Profile, String> {
    let v: toml::Value =
        toml::from_str(text).map_err(|e| format!("profile.toml: {}", e.message()))?;
    let list = |a: &str, b: &str| {
        v.get(a)
            .and_then(|t| t.get(b))
            .and_then(|l| l.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let accounts: Vec<Account> = list("banking", "accounts")
        .iter()
        .filter_map(|a| {
            Some(Account {
                id: a.get("id")?.as_str()?.to_string(),
                keep: cents(a.get("keep")),
            })
        })
        .collect();
    if accounts.is_empty() {
        return Err("profile.toml has no [[banking.accounts]] with an id and keep".into());
    }
    let word = |id: &str| id.split('-').next().unwrap_or(id).to_string();
    let mut cards = Vec::new();
    for c in list("wallet", "cards") {
        let Some(id) = c.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        let paid_from = match c.get("paid_from").and_then(|x| x.as_str()) {
            Some(p) => p.to_string(),
            None => match accounts.iter().find(|a| word(&a.id) == word(id)) {
                Some(a) => a.id.clone(),
                None => continue, // no bank account to fund it from: not this plan's job
            },
        };
        cards.push(Card {
            id: id.to_string(),
            target: cents(c.get("target")),
            paid_from,
        });
    }
    let t = v.get("transfers");
    let hub = t
        .and_then(|t| t.get("hub"))
        .and_then(|h| h.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| accounts[0].id.clone());
    if !accounts.iter().any(|a| a.id == hub) {
        return Err(format!(
            "[transfers] hub = \"{hub}\" is not in [[banking.accounts]]"
        ));
    }
    let num = |k: &str, d: u32| {
        t.and_then(|t| t.get(k))
            .and_then(|x| x.as_integer())
            .map_or(d, |x| x.max(1) as u32)
    };
    let buffer = t
        .and_then(|t| t.get("buffer"))
        .and_then(|x| x.as_integer())
        .unwrap_or(i64::from(DEFAULT_BUFFER));
    if !(0..=i64::from(MAX_BUFFER)).contains(&buffer) {
        return Err(format!(
            "[transfers] buffer = {buffer}: use a percent from 0 to {MAX_BUFFER}"
        ));
    }
    Ok(Profile {
        hub,
        buffer: buffer as u32,
        daily_limit: num("daily_limit", DEFAULT_DAILY_LIMIT),
        min_move: num("min_move", DEFAULT_MIN_MOVE),
        accounts,
        cards,
    })
}

/// Runs of the same digit: 999 → 1, 2000 → 2, 1555 → 2, 1234 → 4.
pub fn runs(n: u32) -> usize {
    let s = n.to_string();
    let b = s.as_bytes();
    1 + b.windows(2).filter(|w| w[0] != w[1]).count()
}

/// How hard an amount is to type: its digit runs, or its digits before the
/// trailing zeros, whichever is fewer. 555 → 1, 500 → 1, 1500 → 2, 522 → 2.
pub fn ease(n: u32) -> usize {
    let s = n.to_string();
    let sig = s.trim_end_matches('0').len().max(1);
    runs(n).min(sig)
}

fn slack(n: u32) -> u32 {
    (n / 10).max(50)
}

/// The quickest-to-type amount at or above `n`, at most 10 % (or $50) more.
/// The easiest to type wins (see `ease`), then the closest to `n`.
pub fn easy_up(n: u32) -> u32 {
    (n..=n + slack(n))
        .min_by_key(|a| (ease(*a), *a - n))
        .unwrap_or(n)
}

/// The quickest-to-type amount at or below `n`, at most 10 % (or $50) less.
pub fn easy_down(n: u32) -> u32 {
    (n.saturating_sub(slack(n)).max(1)..=n)
        .min_by_key(|a| (ease(*a), n - *a))
        .unwrap_or(n)
}

fn dollars_up(c: i64) -> u32 {
    ((c + 99) / 100).max(0) as u32
}

/// What a bank aims to hold before its card paydown: `keep` plus the buffer.
pub fn aim(p: &Profile, a: &Account) -> i64 {
    a.keep + a.keep * i64::from(p.buffer) / 100
}

/// What a card needs paid now: its whole balance once it owes more than
/// `target` (one tap on "current balance", and the longest time until the
/// next paydown). A card in credit or under its target needs nothing.
pub fn payment(c: &Card, balances: &BTreeMap<String, i64>) -> i64 {
    match balances.get(&c.id) {
        Some(b) if *b > c.target => *b,
        _ => 0,
    }
}

/// Spreads one transfer over days so a sender stays under the daily limit.
struct Schedule {
    limit: u32,
    used: BTreeMap<String, (u32, u32)>,
}

impl Schedule {
    fn send(&mut self, moves: &mut Vec<Move>, from: &str, to: &str, mut left: u32) {
        let limit = self.limit;
        let (day, used) = self.used.entry(from.to_string()).or_insert((1, 0));
        while left > 0 {
            let room = limit - *used;
            let part = if left <= room {
                left
            } else if *used == 0 {
                easy_down(limit)
            } else {
                *day += 1;
                *used = 0;
                continue;
            };
            moves.push(Move {
                day: *day,
                from: from.to_string(),
                to: to.to_string(),
                dollars: part,
            });
            *used += part;
            left = if left > part { easy_up(left - part) } else { 0 };
            if *used >= limit {
                *day += 1;
                *used = 0;
            }
        }
    }
}

/// The moves, day by day, and the card payments. `balances` maps account
/// and card ids to cents; a card balance is what it owes (negative: credit).
///
/// A bank gets money only when its card payment would take it below `keep`;
/// then it gets enough for `keep` plus the buffer. A bank above that sends
/// its surplus to a short bank directly when it can cover the whole need
/// (one move, not two), else back to the hub.
pub fn plan(p: &Profile, balances: &BTreeMap<String, i64>) -> Result<Plan, String> {
    for id in balances.keys() {
        if !p.accounts.iter().any(|a| &a.id == id) && !p.cards.iter().any(|c| &c.id == id) {
            return Err(format!(
                "{id} is not an account in [[banking.accounts]] or a card in [[wallet.cards]] of your profile"
            ));
        }
    }
    let mut out = Plan::default();
    for c in &p.cards {
        let cents = payment(c, balances);
        if cents > 0 && balances.contains_key(&c.paid_from) {
            out.payments.push(Payment {
                card: c.id.clone(),
                from: c.paid_from.clone(),
                cents,
            });
        }
    }
    let mut short: Vec<(String, u32)> = Vec::new();
    let mut extra: Vec<(String, u32)> = Vec::new();
    for a in p.accounts.iter().filter(|a| a.id != p.hub) {
        let Some(bal) = balances.get(&a.id) else {
            continue;
        };
        let cards: i64 = out
            .payments
            .iter()
            .filter(|x| x.from == a.id)
            .map(|x| x.cents)
            .sum();
        let gap = aim(p, a) + cards - bal;
        out.gaps.insert(a.id.clone(), gap);
        if bal - cards < a.keep {
            short.push((a.id.clone(), easy_up(dollars_up(gap))));
        } else if gap < 0 && (-gap) / 100 >= i64::from(p.min_move) {
            extra.push((a.id.clone(), ((-gap) / 100) as u32));
        }
    }
    short.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
    let mut sched = Schedule {
        limit: p.daily_limit,
        used: BTreeMap::new(),
    };
    for (to, need) in short {
        match extra
            .iter_mut()
            .filter(|e| e.1 >= need && need <= p.daily_limit)
            .min_by_key(|e| e.1)
        {
            Some(e) => {
                sched.send(&mut out.moves, &e.0.clone(), &to, need);
                e.1 -= need;
            }
            None => sched.send(&mut out.moves, &p.hub, &to, need),
        }
    }
    for (from, left) in extra {
        let back = easy_down(left);
        if back >= p.min_move {
            sched.send(&mut out.moves, &from, &p.hub, back);
        }
    }
    out.moves.sort_by(|a, b| {
        a.day
            .cmp(&b.day)
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });
    Ok(out)
}

fn dollars(d: u32) -> String {
    let c = fmt_cents(i64::from(d) * 100);
    c.trim_end_matches(".00").to_string()
}

/// The plan as Markdown: moves per day, each account after them, and what
/// was not checked.
pub fn render(p: &Profile, balances: &BTreeMap<String, i64>, plan: &Plan) -> String {
    let mut s = String::from("# Transfers to balance your banks\n\n");
    s += &format!(
        "Each account sends at most ${} a day. Each bank aims for its keep plus {} % as a safety margin. \
         Amounts are whole dollars that are quick to type. A card that owes more than its target gets its full balance paid. You send each e-Transfer; cashcast never moves money.\n\n",
        dollars(p.daily_limit),
        p.buffer
    );
    if plan.moves.is_empty() {
        s += "No transfers needed: every bank is at its keep amount plus its margin (within the minimum move).\n\n";
    }
    let mut day = 0;
    for m in &plan.moves {
        if m.day != day {
            day = m.day;
            s += &format!("## Day {day}{}\n\n", if day == 1 { " (today)" } else { "" });
        }
        let why = if m.to == p.hub {
            "surplus back to the hub".to_string()
        } else {
            let keep = p
                .accounts
                .iter()
                .find(|a| a.id == m.to)
                .map_or(0, |a| aim(p, a));
            let mut w = format!("holds ${}", fmt_cents(keep));
            if plan.payments.iter().any(|x| x.from == m.to) {
                w += " after its card payment";
            }
            if m.from != p.hub {
                w += "; straight from the bank with the surplus";
            }
            w
        };
        s += &format!(
            "- {} → {}: **${}** ({why})\n",
            m.from,
            m.to,
            dollars(m.dollars)
        );
        if m.day == plan.moves.last().map_or(0, |l| l.day) && Some(m) == plan.moves.last() {
            s += "\n";
        }
    }
    if !plan.moves.is_empty() && !s.ends_with("\n\n") {
        s += "\n";
    }
    if !plan.payments.is_empty() {
        s += "## Card payments (once the money is in)\n\n";
        for x in &plan.payments {
            s += &format!(
                "- Pay the full current balance of {} from {}: ${}\n",
                x.card,
                x.from,
                fmt_cents(x.cents)
            );
        }
        s += "\n";
    }
    let credits: Vec<String> = p
        .cards
        .iter()
        .filter_map(|c| {
            let b = *balances.get(&c.id)?;
            (b < 0).then(|| {
                format!(
                    "- {} has a credit of ${}: leave it, new purchases use it up.",
                    c.id,
                    fmt_cents(-b)
                )
            })
        })
        .collect();
    if !credits.is_empty() {
        s += "## Cards in credit\n\n";
        s += &credits.join("\n");
        s += "\n\n";
    }
    s +=
        "## After the moves\n\n| Account | Now | After | Aim | Keep |\n|---|---:|---:|---:|---:|\n";
    let mut warn = Vec::new();
    for a in &p.accounts {
        let Some(now) = balances.get(&a.id) else {
            continue;
        };
        let net = plan
            .moves
            .iter()
            .map(|m| {
                let c = i64::from(m.dollars) * 100;
                if m.to == a.id {
                    c
                } else if m.from == a.id {
                    -c
                } else {
                    0
                }
            })
            .sum::<i64>()
            - plan
                .payments
                .iter()
                .filter(|x| x.from == a.id)
                .map(|x| x.cents)
                .sum::<i64>();
        let after = now + net;
        s += &format!(
            "| {} | {} | {} | {} | {} |\n",
            a.id,
            fmt_cents(*now),
            fmt_cents(after),
            fmt_cents(aim(p, a)),
            fmt_cents(a.keep)
        );
        if after < a.keep {
            warn.push(format!(
                "- {} ends ${} below its keep. Send less, or wait for pay to land first.",
                a.id,
                fmt_cents(a.keep - after)
            ));
        }
    }
    if !warn.is_empty() {
        s += "\n## Check\n\n";
        s += &warn.join("\n");
        s += "\n";
    }
    let missing: Vec<String> = p
        .accounts
        .iter()
        .filter(|a| !balances.contains_key(&a.id))
        .map(|a| format!("- {}: no balance given", a.id))
        .collect();
    if !missing.is_empty() {
        s += "\n## Not checked\n\n";
        s += &missing.join("\n");
        s += "\n";
    }
    s
}
