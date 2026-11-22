//! `moneywiz categorize`: a category for every synced row that has none.
//!
//! It learns from the owner's own categorized rows: a payee that went to one
//! category at least twice, and in at least 60 % of its rows, sends its
//! uncategorized rows there. Otherwise the engine's spending guess counts,
//! when MoneyWiz has a category of that name. The result is an edits file
//! (`moneywiz edit` applies it through the Core Data writer, decision 0007);
//! the report shows counts only.

use crate::export::Txn;
use crate::inbox::Inbox;
use crate::moneywiz::{MwTxn, Plan, Store, guessed_category, uncategorized};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// One uncategorized row and the category it gets.
pub struct Pick<'a> {
    pub row: &'a MwTxn,
    pub path: Vec<String>,
    /// "payee" (learned from the owner's rows) or "guess".
    pub source: &'static str,
}

/// A word seen under this many of the owner's categories or more names no
/// payee (a card network or "point of sale" prefix, say): keys skip it.
const STOP_SPREAD: usize = 20;

/// The share (percent) of a key's rows its top category needs.
const MIN_SHARE: usize = 60;

/// The same for the bank's and the engine's category guesses: they are
/// right less often, so they need a clearer winner.
const GUESS_SHARE: usize = 75;

/// The words of 3+ letters in a payee text, upper case.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_uppercase)
}

/// Payee keys, most specific first: the first two words of 3+ letters that
/// are not stop words, then the first such word alone.
fn keys(text: &str, stop: &HashSet<String>) -> Vec<String> {
    let words: Vec<String> = words(text).filter(|w| !stop.contains(w)).take(2).collect();
    match words.len() {
        0 => vec![],
        1 => vec![format!("1:{}", words[0])],
        _ => vec![
            format!("2:{} {}", words[0], words[1]),
            format!("1:{}", words[0]),
        ],
    }
}

/// The keys of a MoneyWiz row, then those of its bank row: the owner's own
/// words name the payee better than the bank's.
fn row_keys(own: &str, bank: Option<&str>, stop: &HashSet<String>) -> Vec<String> {
    let mut all = keys(own, stop);
    if let Some(b) = bank {
        for k in keys(b, stop) {
            if !all.contains(&k) {
                all.push(k);
            }
        }
    }
    all
}

/// A key's category: the one the owner used most for it, when it holds at
/// least `min_share` percent of the key's rows (one row is
/// enough). A tie goes to the category the owner uses most overall.
fn winners(
    votes: HashMap<(String, bool), BTreeMap<Vec<String>, usize>>,
    overall: &HashMap<Vec<String>, usize>,
    min_share: usize,
) -> HashMap<(String, bool), Vec<String>> {
    votes
        .into_iter()
        .filter_map(|(k, v)| {
            let total: usize = v.values().sum();
            let (path, n) = v
                .into_iter()
                .max_by_key(|(p, n)| (*n, overall.get(p).copied().unwrap_or(0)))?;
            // A key the owner split over several categories names none.
            (n * 100 >= total * min_share).then_some((k, path))
        })
        .collect()
}

fn vote(
    votes: &mut HashMap<(String, bool), BTreeMap<Vec<String>, usize>>,
    k: String,
    out: bool,
    path: &[String],
) {
    *votes
        .entry((k, out))
        .or_default()
        .entry(path.to_vec())
        .or_default() += 1;
}

/// The bank row for each matched MoneyWiz row.
fn bank_rows<'a>(inbox: &'a Inbox, plan: &Plan) -> HashMap<i64, &'a Txn> {
    plan.accounts
        .iter()
        .flat_map(|ap| &ap.pairs)
        .map(|&(pk, i)| (pk, &inbox.txns[i]))
        .collect()
}

/// The bank's own category for a row, when its file has one.
fn bank_category(b: &Txn) -> Option<String> {
    (b.category_from_file && !b.category.is_empty()).then(|| format!("3:{}", b.category.join("/")))
}

/// What the owner's categorized rows teach: payee key → category (from the
/// row's own text and from the bank's text for it), and the engine's guess →
/// the owner's category for it (so names need not match).
fn learned(
    store: &Store,
    bank: &HashMap<i64, &Txn>,
    hidden: &dyn Fn(&MwTxn) -> bool,
) -> (
    HashMap<(String, bool), Vec<String>>,
    HashMap<(String, bool), Vec<String>>,
    HashSet<String>,
) {
    let mut payees = HashMap::new();
    let mut guesses = HashMap::new();
    let train = |t: &&MwTxn| {
        !(t.category.is_empty()
            || t.other_account.is_some()
            || !t.is_cash()
            || t.is_correction()
            || hidden(t))
    };
    // Stop words: words spread over many categories.
    let mut spread: HashMap<String, HashSet<&[String]>> = HashMap::new();
    for t in store.txns.iter().filter(train) {
        let bank_text = bank.get(&t.pk).map(|b| b.text.as_str()).unwrap_or("");
        for w in words(&t.text).chain(words(bank_text)) {
            spread.entry(w).or_default().insert(&t.category);
        }
    }
    let stop: HashSet<String> = spread
        .into_iter()
        .filter(|(_, c)| c.len() >= STOP_SPREAD)
        .map(|(w, _)| w)
        .collect();
    for t in store.txns.iter().filter(train) {
        let b = bank.get(&t.pk);
        if let Some(c) = b.and_then(|b| bank_category(b)) {
            vote(&mut guesses, c, t.cents < 0, &t.category);
        }
        let ks = row_keys(&t.text, b.map(|b| b.text.as_str()), &stop);
        for k in ks {
            vote(&mut payees, k, t.cents < 0, &t.category);
        }
        if let Some(g) = guessed_category(t) {
            vote(&mut guesses, format!("2:{g}"), t.cents < 0, &t.category);
        }
    }
    let mut overall: HashMap<Vec<String>, usize> = HashMap::new();
    for t in &store.txns {
        if !t.category.is_empty() {
            *overall.entry(t.category.clone()).or_default() += 1;
        }
    }
    (
        winners(payees, &overall, MIN_SHARE),
        winners(guesses, &overall, GUESS_SHARE),
        stop,
    )
}

/// Rows left without a pick: no payee text, or nothing learned for it.
#[derive(Default)]
pub struct Unpicked {
    pub no_text: usize,
    pub unknown: usize,
    /// Rows cashcast categorized that the rule still picks the same for.
    pub agree: usize,
    /// Rows cashcast categorized that the rule no longer picks for (kept).
    pub unsure: usize,
    /// The rows (pk) with no category found: shown only with `--list`.
    pub rows: Vec<i64>,
}

/// What the owner's categorized rows teach, ready to pick for any row.
pub struct Learned<'a> {
    bank: HashMap<i64, &'a Txn>,
    payees: HashMap<(String, bool), Vec<String>>,
    guesses: HashMap<(String, bool), Vec<String>>,
    stop: HashSet<String>,
}

pub fn learn<'a>(store: &Store, inbox: &'a Inbox, plan: &Plan) -> Learned<'a> {
    learn_from(store, inbox, plan, &Record::new())
}

/// What the owner's rows teach, leaving out the rows cashcast categorized.
pub fn learn_from<'a>(
    store: &Store,
    inbox: &'a Inbox,
    plan: &Plan,
    record: &Record,
) -> Learned<'a> {
    let bank = bank_rows(inbox, plan);
    let mine = ours(store, record);
    let (payees, guesses, stop) = learned(store, &bank, &|t| mine.contains(&t.pk));
    Learned {
        bank,
        payees,
        guesses,
        stop,
    }
}

impl Learned<'_> {
    /// A category and its source ("payee" or "guess") from payee keys, a
    /// bank category and the engine's guess, in that order.
    fn pick(
        &self,
        ks: &[String],
        file: Option<String>,
        engine: Option<&str>,
        out: bool,
    ) -> Option<(Vec<String>, &'static str)> {
        if let Some(p) = ks.iter().find_map(|k| self.payees.get(&(k.clone(), out))) {
            return Some((p.clone(), "payee"));
        }
        [file, engine.map(|g| format!("2:{g}"))]
            .into_iter()
            .flatten()
            .find_map(|k| self.guesses.get(&(k, out)))
            .map(|p| (p.clone(), "guess"))
    }

    /// The category for a bank row the sync adds to MoneyWiz.
    pub fn for_bank(&self, t: &Txn) -> Option<Vec<String>> {
        let engine = crate::text::derive(&t.text, t.cents).category;
        self.pick(
            &keys(&t.text, &self.stop),
            bank_category(t),
            engine,
            t.cents < 0,
        )
        .map(|(p, _)| p)
    }
}

/// The rows cashcast categorized, by MoneyWiz GID, with the category it set.
/// They are not the owner's choice, so learning skips them and a better rule
/// may pick again. A row the owner moved since is the owner's again.
pub type Record = BTreeMap<String, Vec<String>>;

pub const RECORD_FILE: &str = "moneywiz-categorized.json";

pub fn load_record(data: &Path) -> Record {
    std::fs::read_to_string(data.join(RECORD_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_record(data: &Path, record: &Record) -> Result<(), String> {
    let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let tmp = data.join(format!("{RECORD_FILE}.tmp"));
    crate::write_private(&tmp, &text)?;
    std::fs::rename(&tmp, data.join(RECORD_FILE)).map_err(|e| e.to_string())
}

/// Rows with no category in `before` (a backup) that have one now: what a
/// categorize run set, for a run made before the record existed.
pub fn record_since(before: &Store, now: &Store) -> Record {
    let empty: HashSet<&str> = before
        .txns
        .iter()
        .filter(|t| t.category.is_empty() && !t.gid.is_empty())
        .map(|t| t.gid.as_str())
        .collect();
    now.txns
        .iter()
        .filter(|t| !t.category.is_empty() && empty.contains(t.gid.as_str()))
        .map(|t| (t.gid.clone(), t.category.clone()))
        .collect()
}

/// MoneyWiz pks of rows that still hold the category cashcast set.
fn ours(store: &Store, record: &Record) -> HashSet<i64> {
    store
        .txns
        .iter()
        .filter(|t| record.get(&t.gid).is_some_and(|p| *p == t.category))
        .map(|t| t.pk)
        .collect()
}

pub fn picks<'a>(store: &'a Store, inbox: &Inbox, plan: &Plan) -> (Vec<Pick<'a>>, Unpicked) {
    picks_with(store, inbox, plan, &Record::new())
}

/// Picks for rows with no category, and new picks for rows cashcast
/// categorized before when the rule now names another category.
pub fn picks_with<'a>(
    store: &'a Store,
    inbox: &Inbox,
    plan: &Plan,
    record: &Record,
) -> (Vec<Pick<'a>>, Unpicked) {
    let l = learn_from(store, inbox, plan, record);
    let mine = ours(store, record);
    let best = BestGuess::new(store, &mine, &l.stop);
    let mut out = Vec::new();
    let mut left = Unpicked::default();
    let again = store.txns.iter().filter(|t| mine.contains(&t.pk));
    let empty = uncategorized(store, plan).into_iter().map(|(t, _)| t);
    for t in empty.chain(again) {
        if t.cents == 0 || t.is_correction() || !t.is_cash() {
            continue;
        }
        let b = l.bank.get(&t.pk);
        let ks = row_keys(&t.text, b.map(|b| b.text.as_str()), &l.stop);
        let file = b.and_then(|b| bank_category(b));
        match l.pick(&ks, file, guessed_category(t), t.cents < 0) {
            Some((path, _)) if path == t.category => left.agree += 1,
            Some((path, source)) => out.push(Pick {
                row: t,
                path,
                source,
            }),
            // A row cashcast categorized keeps its category.
            None if !t.category.is_empty() => left.unsure += 1,
            None => match best.guess(&ks, t.account, t.cents < 0) {
                Some(path) => out.push(Pick {
                    row: t,
                    path,
                    source: "best guess",
                }),
                None if ks.is_empty() => {
                    left.no_text += 1;
                    left.rows.push(t.pk);
                }
                None => {
                    left.unknown += 1;
                    left.rows.push(t.pk);
                }
            },
        }
    }
    (out, left)
}

/// The owner's choice to fill every empty row (no review): the category
/// most of the owner's rows with any of the row's words carry, else the
/// category most of the owner's rows of that account and sign carry.
struct BestGuess {
    words: HashMap<(String, bool), BTreeMap<Vec<String>, usize>>,
    accounts: HashMap<(i64, bool), BTreeMap<Vec<String>, usize>>,
}

impl BestGuess {
    fn new(store: &Store, mine: &HashSet<i64>, stop: &HashSet<String>) -> Self {
        let mut words = HashMap::new();
        let mut accounts: HashMap<(i64, bool), BTreeMap<Vec<String>, usize>> = HashMap::new();
        for t in &store.txns {
            if t.category.is_empty() || t.cents == 0 || mine.contains(&t.pk) || !t.is_cash() {
                continue;
            }
            let out = t.cents < 0;
            for k in keys(&t.text, stop) {
                vote(&mut words, k, out, &t.category);
            }
            *accounts
                .entry((t.account, out))
                .or_default()
                .entry(t.category.clone())
                .or_default() += 1;
        }
        BestGuess { words, accounts }
    }

    fn guess(&self, ks: &[String], account: i64, out: bool) -> Option<Vec<String>> {
        let top = |v: &BTreeMap<Vec<String>, usize>| {
            v.iter().max_by_key(|(_, n)| **n).map(|(p, _)| p.clone())
        };
        ks.iter()
            .find_map(|k| self.words.get(&(k.clone(), out)).and_then(top))
            .or_else(|| self.accounts.get(&(account, out)).and_then(top))
    }
}

/// A TOML basic string: quotes, backslashes and control characters escaped.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The category type the owner used most for rows of one sign on `path`,
/// when one type clearly wins.
fn kind_for(store: &Store, path: &[String], positive: bool) -> Option<i64> {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for t in &store.txns {
        if t.category == path && t.cents != 0 && (t.cents > 0) == positive {
            if let Some(k) = t.category_kind {
                *counts.entry(k).or_default() += 1;
            }
        }
    }
    let mut v: Vec<(usize, i64)> = counts.into_iter().map(|(k, n)| (n, k)).collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    match v.as_slice() {
        [(_, k)] => Some(*k),
        [(a, k), (b, _), ..] if a > b => Some(*k),
        _ => None,
    }
}

/// Writes the edits file for these picks. Refuses to overwrite one that
/// exists (the owner's own edits). Rows that cannot be named apart (same
/// account, date, amount and payee) are skipped and counted, and so is a
/// category path two categories share when neither type fits the row.
pub fn write_edits(store: &Store, picks: &[Pick], path: &Path) -> Result<(usize, usize), String> {
    write_edits_into(store, picks, path, None)
}

/// `write_edits`, adding each written row to `record`.
pub fn write_edits_into(
    store: &Store,
    picks: &[Pick],
    path: &Path,
    mut record: Option<&mut Record>,
) -> Result<(usize, usize), String> {
    // An edits file MoneyWiz already shows in full is parked next to it, so
    // an applied file never blocks the next weekly.
    if path.exists() {
        let r = crate::moneywiz_edit::resolve(store, &crate::moneywiz_edit::load(path)?, None);
        if r.problems.is_empty() && crate::moneywiz_edit::is_empty(&r.plan) {
            let day = chrono::Local::now().format("%Y%m%d");
            let parked = (0..)
                .map(|n| {
                    let suffix = if n == 0 {
                        String::new()
                    } else {
                        format!("-{n}")
                    };
                    path.with_file_name(format!("moneywiz-edits-applied-{day}{suffix}.toml"))
                })
                .find(|p| !p.exists())
                .expect("a free name");
            std::fs::rename(path, &parked)
                .map_err(|e| format!("cannot park the applied edits file: {e}"))?;
        }
    }
    if path.exists() {
        return Err(format!(
            "{} exists: apply it (`bin/cashcast moneywiz edit`) or move it away, then run categorize again",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("the edits file")
        ));
    }
    let mut text = String::from(
        "# Written by `bin/cashcast moneywiz categorize`. Preview with `bin/cashcast moneywiz edit --dry-run`,\n# apply with `bin/cashcast moneywiz edit`. Remove a [[row]] block to skip it.\n",
    );
    let (mut written, mut skipped) = (0, 0);
    let mut done = Vec::new();
    let slash_names = store
        .categories
        .iter()
        .any(|c| c.iter().any(|x| x.contains('/')));
    for p in picks {
        let t = p.row;
        let same: Vec<&MwTxn> = store
            .txns
            .iter()
            .filter(|o| o.account == t.account && o.date == t.date && o.cents == t.cents)
            .collect();
        let account = store
            .account(t.account)
            .map(|a| a.name.as_str())
            .unwrap_or_default();
        // An edit names its account; two accounts of that name are ambiguous.
        if store.accounts.iter().filter(|a| a.name == account).count() != 1 {
            skipped += 1;
            continue;
        }
        // A path that names two categories (one per type, say) is ambiguous:
        // the writer refuses it, so it is left for the owner.
        let kind = match store.categories.iter().filter(|c| **c == p.path).count() {
            1 => None,
            0 => {
                skipped += 1;
                continue;
            }
            _ => {
                // The owner's own rows on this path with the same sign name
                // the type (a salary is income; a refund may be an expense).
                let want = match kind_for(store, &p.path, t.cents > 0) {
                    Some(k) => k,
                    None => {
                        skipped += 1;
                        continue;
                    }
                };
                let kinds = store.path_kinds.get(&p.path).cloned().unwrap_or_default();
                if kinds.iter().filter(|&&k| k == want).count() != 1 {
                    skipped += 1;
                    continue;
                }
                Some(if want == crate::moneywiz_categories::EXPENSE {
                    "expense"
                } else {
                    "income"
                })
            }
        };
        let needs_text = same.len() > 1;
        if needs_text {
            let lower = t.text.to_lowercase();
            let clash = same
                .iter()
                .filter(|o| o.text.to_lowercase().contains(&lower))
                .count();
            if t.text.is_empty() || clash > 1 {
                skipped += 1;
                continue;
            }
        }

        text.push_str(&format!(
            "\n[[row]]\naccount = {}\ndate = \"{}\"\namount = {}.{:02}\n",
            quote(account),
            t.date,
            if t.cents < 0 {
                format!("-{}", t.cents.unsigned_abs() / 100)
            } else {
                (t.cents / 100).to_string()
            },
            t.cents.unsigned_abs() % 100
        ));
        if needs_text {
            text.push_str(&format!("text = {}\n", quote(&t.text)));
        }
        // Exact parts whenever a category name holds a slash: "A/B/C" could
        // then name two categories, and the edit refuses it.
        if slash_names || p.path.iter().any(|x| x.contains('/')) {
            let parts: Vec<String> = p.path.iter().map(|x| quote(x)).collect();
            text.push_str(&format!("set_category_path = [{}]\n", parts.join(", ")));
        } else {
            text.push_str(&format!("set_category = {}\n", quote(&p.path.join("/"))));
        }
        if let Some(k) = kind {
            text.push_str(&format!("set_category_kind = \"{k}\"\n"));
        }
        written += 1;
        if !t.gid.is_empty() {
            done.push((t.gid.clone(), p.path.clone()));
        }
    }
    // No edit, no file: an empty one would block the next run.
    if written > 0 {
        crate::write_private(path, &text)?;
    }
    if let Some(r) = record.as_deref_mut() {
        r.extend(done);
    }
    Ok((written, skipped))
}

/// The counts-only report.
pub fn render(picks: &[Pick], left: &Unpicked, written: Option<(usize, usize)>) -> String {
    let payee = picks.iter().filter(|p| p.source == "payee").count();
    let guess = picks.len() - payee;
    let mut by: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    for p in picks {
        by.entry(p.path.join(" > ")).or_default()[usize::from(p.source != "payee")] += 1;
    }
    let mut out = format!(
        "# MoneyWiz categorize\n\n{} synced rows with no category: {payee} from your own rows for the same payee, {guess} from the engine's guess mapped to your categories, {} with no category found ({} have no payee text, {} have a payee seen too rarely; set them in MoneyWiz).\n\n| Category | Rows | From your rows | Engine guess |\n|---|---:|---:|---:|\n",
        picks.len() + left.no_text + left.unknown,
        left.no_text + left.unknown,
        left.no_text,
        left.unknown
    );
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by(|a, b| {
        let (x, y) = (a.1.iter().sum::<usize>(), b.1.iter().sum::<usize>());
        y.cmp(&x).then(a.0.cmp(&b.0))
    });
    for (path, [p, g]) in rows {
        out.push_str(&format!(
            "| {path} | {} | {p} | {g} |
",
            p + g
        ));
    }
    match written {
        Some((0, s)) => out.push_str(&format!(
            "\nNo edits file written: {s} skipped (rows that cannot be told apart, or a category name used twice). Set them in MoneyWiz.\n"
        )),
        Some((w, s)) => out.push_str(&format!(
            "\nWrote {w} edits to inbox/moneywiz-edits.toml ({s} skipped: rows that cannot be told apart, or a category name used twice). Preview with `bin/cashcast moneywiz edit --dry-run`, then apply with `bin/cashcast moneywiz edit`.\n"
        )),
        None => out.push_str("\nDry run. `bin/cashcast moneywiz categorize` writes the edits file.\n"),
    }
    out
}

/// How often learning agrees with the owner: every 5th categorized cash row
/// is hidden, the rest teach, and the hidden rows are predicted. Returns
/// (rows checked, right, wrong) per source: payee, then guess.
pub fn check(store: &Store, inbox: &Inbox, plan: &Plan) -> [(usize, usize); 2] {
    check_misses(store, inbox, plan, &Record::new()).0
}

/// The check, plus its most common misses: (your category, the pick, rows).
pub fn check_misses(
    store: &Store,
    inbox: &Inbox,
    plan: &Plan,
    record: &Record,
) -> ([(usize, usize); 2], Vec<(String, String, usize)>) {
    let mut misses: HashMap<(String, String), usize> = HashMap::new();
    // Rows cashcast categorized are neither taught nor tested.
    let mine = ours(store, record);
    let test = |t: &MwTxn| t.pk % 5 == 0 && !mine.contains(&t.pk);
    let hidden = |t: &MwTxn| t.pk % 5 == 0 || mine.contains(&t.pk);
    let bank = bank_rows(inbox, plan);
    let (payees, guesses, stop) = learned(store, &bank, &hidden);
    let l = Learned {
        bank,
        payees,
        guesses,
        stop,
    };
    let mut out = [(0, 0); 2];
    for t in &store.txns {
        if t.category.is_empty()
            || t.other_account.is_some()
            || !t.is_cash()
            || t.is_correction()
            || !test(t)
        {
            continue;
        }
        let b = l.bank.get(&t.pk);
        let ks = row_keys(&t.text, b.map(|b| b.text.as_str()), &l.stop);
        let file = b.and_then(|b| bank_category(b));
        if let Some((path, source)) = l.pick(&ks, file, guessed_category(t), t.cents < 0) {
            let i = usize::from(source != "payee");
            out[i].0 += 1;
            out[i].1 += usize::from(path == t.category);
            if path != t.category {
                *misses
                    .entry((t.category.join(" > "), path.join(" > ")))
                    .or_default() += 1;
            }
        }
    }
    let mut top: Vec<(String, String, usize)> =
        misses.into_iter().map(|((a, b), n)| (a, b, n)).collect();
    top.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    top.truncate(10);
    (out, top)
}

/// The rows with no category found, for the owner only (`--list`): day,
/// account, amount and payee text. Never part of the counts report.
/// Each guessed row, so the owner can answer row by row (`categorize --list`).
pub fn render_picks(store: &Store, picks: &[Pick]) -> String {
    let mut out = String::from(
        "\n## Rows with a suggested category\n\n| Day | Account | Amount | Payee and description | Suggested category | From |\n|---|---|---:|---|---|---|\n",
    );
    for p in picks {
        let t = p.row;
        let account = store.account(t.account).map_or("?", |a| a.name.as_str());
        out.push_str(&format!(
            "| {} | {account} | {} | {} | {} | {} |\n",
            t.date,
            crate::money::fmt_cents(t.cents),
            t.text.replace('|', "/"),
            p.path.join(" > "),
            if p.source == "payee" {
                "your rows"
            } else {
                "engine guess"
            },
        ));
    }
    out
}

pub fn render_left(store: &Store, left: &Unpicked) -> String {
    let mut out = String::from(
        "\n## Rows with no category found\n\nThe other side: a row in another account with the opposite amount (within 1 %) within 10 days (a transfer needs no category).\n\n| Day | Account | Amount | Payee and description | Possible other side |\n|---|---|---:|---|---|\n",
    );
    for pk in &left.rows {
        let Some(t) = store.txns.iter().find(|t| t.pk == *pk) else {
            continue;
        };
        let account = store
            .accounts
            .iter()
            .find(|a| a.pk == t.account)
            .map_or("?", |a| a.name.as_str());
        let name = |pk: i64| {
            store
                .accounts
                .iter()
                .find(|a| a.pk == pk)
                .map_or("?", |a| a.name.as_str())
        };
        let other: Vec<String> = store
            .txns
            .iter()
            .filter(|o| {
                o.account != t.account
                    && o.cents.signum() == -t.cents.signum()
                    && (o.cents + t.cents).abs() <= t.cents.abs() / 100
                    && (o.date - t.date).num_days().abs() <= 10
            })
            .map(|o| format!("{} {}", o.date, name(o.account)))
            .collect();
        out.push_str(&format!(
            "| {} | {account} | {} | {} | {} |\n",
            t.date,
            crate::money::fmt_cents(t.cents),
            t.text.replace('|', "/"),
            if other.is_empty() {
                "none".into()
            } else {
                other.join("; ")
            }
        ));
    }
    out
}
