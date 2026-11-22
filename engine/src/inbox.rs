//! Loads every export file in the inbox folder and merges them. Ledger
//! files (investments and crypto, see `ledger`) are listed, not imported.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::export::{self, ErrorRow, Export, LoadOptions, Txn};

const EXTENSIONS: &[&str] = &["csv", "tsv", "txt", "ofx", "qfx", "xlsx", "json", "pdf"];

pub struct Inbox {
    pub files: Vec<Export>,
    /// Files that could not be read at all, with the reason.
    pub failed: Vec<(String, String)>,
    pub txns: Vec<Txn>,
    /// Rows dropped because another file holds the same transactions.
    pub duplicates_dropped: usize,
    /// Investment and crypto ledgers, with their format label. They are
    /// kept out of the cash import; the `holdings` command reads them.
    pub ledger_files: Vec<(String, String)>,
    /// MoneyWiz's own exports, left out of what `moneywiz sync` compares:
    /// MoneyWiz is what the sync fills, not a bank.
    pub moneywiz_files: Vec<String>,
    /// Per file, the accounts it held rows of before duplicates were
    /// dropped: a new download that repeats rows still shows the account.
    pub file_accounts: Vec<std::collections::BTreeSet<String>>,
}

impl Inbox {
    /// Merges files. A transaction that appears in several overlapping
    /// exports is kept from the one file that has the most copies of it.
    pub fn merge(mut files: Vec<Export>, failed: Vec<(String, String)>) -> Inbox {
        let file_accounts = files
            .iter()
            .map(|f| f.txns.iter().map(|t| t.account.clone()).collect())
            .collect();
        let mut all = Vec::new();
        for (i, f) in files.iter_mut().enumerate() {
            for mut t in f.txns.drain(..) {
                t.file = i;
                all.push(t);
            }
        }
        let mut counts: HashMap<(u64, usize), usize> = HashMap::new();
        for t in &all {
            *counts.entry((t.fingerprint, t.file)).or_insert(0) += 1;
        }
        let mut owner: HashMap<u64, (usize, usize)> = HashMap::new();
        for (&(fp, file), &n) in &counts {
            let best = owner.entry(fp).or_insert((file, n));
            if n > best.1 || (n == best.1 && file < best.0) {
                *best = (file, n);
            }
        }
        let before = all.len();
        all.retain(|t| owner[&t.fingerprint].0 == t.file);
        let duplicates_dropped = before - all.len();
        Inbox {
            files,
            failed,
            txns: all,
            duplicates_dropped,
            ledger_files: Vec::new(),
            moneywiz_files: Vec::new(),
            file_accounts,
        }
    }

    pub fn single(export: Export) -> Inbox {
        Inbox::merge(vec![export], Vec::new())
    }

    /// Transactions read per file, after duplicates were dropped.
    pub fn rows_per_file(&self) -> Vec<usize> {
        let mut rows = vec![0; self.files.len()];
        for t in &self.txns {
            rows[t.file] += 1;
        }
        rows
    }
}

pub fn options_for(cfg: &Config, name: &str) -> LoadOptions {
    let f = cfg.files.get(name).cloned().unwrap_or_default();
    LoadOptions {
        date_format: f.date_format,
        category_separator: f.category_separator,
        account: f.account,
        currency: f.currency,
        invert: f.invert,
        columns: f.columns,
    }
}

/// The day a file was saved (downloaded) into the inbox, from its modified
/// time. None when unknown, or after `today`: a run with a fixed past
/// `--today` (tests, parity) then judges by row dates alone.
pub fn saved_day(path: &Path, today: chrono::NaiveDate) -> Option<chrono::NaiveDate> {
    let saved = std::fs::metadata(path).ok()?.modified().ok()?;
    let day = chrono::DateTime::<chrono::Local>::from(saved).date_naive();
    (day <= today).then_some(day)
}

pub fn inbox_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot open the inbox folder {}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| !export::file_name(p).starts_with('.'))
        .filter(|p| {
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            EXTENSIONS.contains(&ext.as_str())
        })
        .collect();
    paths.sort();
    Ok(paths)
}

/// Loads every supported file in `dir`, plus the MoneyWiz accounts recorded
/// by hand when `[moneywiz]` is set. A file that cannot be read is reported
/// in `failed` and the rest still load.
pub fn load_dir(dir: &Path, cfg: &Config) -> Result<Inbox, String> {
    load(dir, cfg, true)
}

/// The exports only, without MoneyWiz (what `moneywiz sync` compares).
pub fn load_exports(dir: &Path, cfg: &Config) -> Result<Inbox, String> {
    load(dir, cfg, false)
}

fn load(dir: &Path, cfg: &Config, with_moneywiz: bool) -> Result<Inbox, String> {
    let paths = inbox_files(dir)?;
    if paths.is_empty() {
        return Err(format!(
            "no export files in {}. Add .csv, .ofx or .qfx files there.",
            dir.display()
        ));
    }
    let mut files = Vec::new();
    let mut failed = Vec::new();
    let mut ledger_files = Vec::new();
    let mut moneywiz_files = Vec::new();
    for path in paths {
        let name = export::file_name(&path);
        let is_pdf = crate::pdf::is_pdf_path(&path);
        if let Some(label) = (!is_pdf).then(|| crate::ledger::sniff(&path)).flatten() {
            // `moneywiz sync` and `coverage` also match the ledger's cash
            // rows; the cash import (with MoneyWiz) never sees them.
            if !with_moneywiz {
                if let Some(e) = crate::ledger::cash_export(&path) {
                    files.push(e);
                }
            }
            ledger_files.push((name, label.to_string()));
            continue;
        }
        if !is_pdf && crate::ledger::is_xlsx_path(&path) {
            failed.push((
                name,
                "only investment ledgers can be .xlsx; save this file as CSV".into(),
            ));
            continue;
        }
        match export::load(&path, &options_for(cfg, &name)) {
            Ok(e) if !with_moneywiz && e.format == "MoneyWiz CSV" => moneywiz_files.push(name),
            // An E*TRADE statement holds an investment account's cash: like a
            // cash ledger, only `moneywiz sync` and `coverage` read it.
            Ok(e) if with_moneywiz && e.format == crate::pdf::ETRADE_STOCKPLAN => {
                ledger_files.push((name, e.format.clone()));
            }
            Ok(e) => files.push(e),
            Err(e) => failed.push((name, e)),
        }
    }
    if with_moneywiz {
        match crate::moneywiz::import_source(cfg, &files) {
            Some(Ok(e)) => files.push(e),
            Some(Err(e)) => failed.push(("MoneyWiz database".into(), e)),
            None => {}
        }
    }
    if files.is_empty() && failed.is_empty() && !moneywiz_files.is_empty() {
        return Err(format!(
            "no bank or card exports in {}: only MoneyWiz exports, which the sync fills, not reads. Add bank and card exports.",
            dir.display()
        ));
    }
    if files.is_empty() && failed.is_empty() {
        return Err(format!(
            "only ledger files in {}: they hold investments, not cash. Run the holdings command, or add bank exports.",
            dir.display()
        ));
    }
    if !with_moneywiz {
        if let Some(aliases) = cfg.moneywiz.as_ref().map(|m| &m.export_aliases) {
            apply_aliases(&mut files, aliases);
        }
    }
    trim_pdf_overlap(&mut files);
    let mut inbox = Inbox::merge(files, failed);
    inbox.ledger_files = ledger_files;
    inbox.moneywiz_files = moneywiz_files;
    Ok(inbox)
}

/// Renames alias export accounts to their target (`[moneywiz.export_aliases]`).
/// Alias rows on days that the target's own files cover are dropped: those
/// files already hold them. Alias balances never replace the target's own.
pub fn apply_aliases(files: &mut [Export], aliases: &std::collections::BTreeMap<String, String>) {
    let target_of = |name: &str| -> Option<String> {
        aliases
            .iter()
            .find(|(k, _)| match k.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == k.as_str(),
            })
            .map(|(_, v)| v.clone())
            .filter(|v| v != name)
    };
    // Days each target's own files cover.
    let mut own: HashMap<String, (chrono::NaiveDate, chrono::NaiveDate)> = HashMap::new();
    for f in files.iter() {
        for t in &f.txns {
            if target_of(&t.account).is_none() {
                let e = own.entry(t.account.clone()).or_insert((t.date, t.date));
                e.0 = e.0.min(t.date);
                e.1 = e.1.max(t.date);
            }
        }
    }
    let own_balance: std::collections::HashSet<String> = files
        .iter()
        .flat_map(|f| f.balances.keys())
        .filter(|k| target_of(k).is_none())
        .cloned()
        .collect();
    for f in files.iter_mut() {
        f.txns.retain_mut(|t| match target_of(&t.account) {
            None => true,
            Some(target) => {
                let covered = own
                    .get(&target)
                    .is_some_and(|(a, b)| *a <= t.date && t.date <= *b);
                // The old key holds the file's own name ("activities-2026-11-08"):
                // key the row by its target, so two downloads that hold it
                // keep one copy (`Inbox::merge`).
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&(&target, t.date, t.cents, &t.currency, &t.text), &mut h);
                t.fingerprint = std::hash::Hasher::finish(&h);
                t.account = target;
                !covered
            }
        });
        let renamed: Vec<(String, String)> = f
            .balances
            .keys()
            .filter_map(|k| target_of(k).map(|v| (k.clone(), v)))
            .collect();
        for (from, to) in renamed {
            if let Some(b) = f.balances.remove(&from) {
                if !own_balance.contains(&to) {
                    f.balances.entry(to).or_insert(b);
                }
            }
        }
        for st in &mut f.statements {
            if let Some(to) = target_of(&st.account) {
                st.account = to;
            }
        }
        let kinds: Vec<(String, String)> = f
            .account_kinds
            .keys()
            .filter_map(|k| target_of(k).map(|v| (k.clone(), v)))
            .collect();
        for (from, to) in kinds {
            if let Some(k) = f.account_kinds.remove(&from) {
                f.account_kinds.entry(to).or_insert(k);
            }
        }
        let ids: Vec<(String, String)> = f
            .ledger_ids
            .keys()
            .filter_map(|k| target_of(k).map(|v| (k.clone(), v)))
            .collect();
        for (from, to) in ids {
            if let Some(i) = f.ledger_ids.remove(&from) {
                f.ledger_ids.entry(to).or_default().extend(i);
            }
        }
    }
}

/// PDF statements fill history the downloads do not reach. A PDF row on a
/// day that another file of the same account covers (from its first to its
/// last row) is dropped: the download already holds it. MoneyWiz is not
/// a download, so it never counts.
pub fn trim_pdf_overlap(files: &mut [Export]) {
    let mut spans: HashMap<String, Vec<(chrono::NaiveDate, chrono::NaiveDate)>> = HashMap::new();
    for f in files
        .iter()
        .filter(|f| !crate::pdf::is_pdf_format(&f.format) && !f.format.starts_with("MoneyWiz"))
    {
        let mut own: HashMap<&str, (chrono::NaiveDate, chrono::NaiveDate)> = HashMap::new();
        for t in &f.txns {
            let e = own.entry(t.account.as_str()).or_insert((t.date, t.date));
            e.0 = e.0.min(t.date);
            e.1 = e.1.max(t.date);
        }
        for (account, span) in own {
            spans.entry(account.to_string()).or_default().push(span);
        }
    }
    for f in files
        .iter_mut()
        .filter(|f| crate::pdf::is_pdf_format(&f.format))
    {
        f.txns.retain(|t| {
            !spans
                .get(&t.account)
                .is_some_and(|s| s.iter().any(|(a, b)| *a <= t.date && t.date <= *b))
        });
    }
}

/// The first row that fails in any file, or a file that fails as a whole.
pub enum FirstProblem {
    Row { file: String, row: ErrorRow },
    File { file: String, error: String },
}

pub fn first_problem(dir: &Path, cfg: &Config) -> Result<Option<FirstProblem>, String> {
    for path in inbox_files(dir)? {
        if !crate::pdf::is_pdf_path(&path) && crate::ledger::sniff(&path).is_some() {
            continue;
        }
        let name = export::file_name(&path);
        let opts = options_for(cfg, &name);
        match export::first_error(&path, &opts) {
            Ok(Some(row)) => return Ok(Some(FirstProblem::Row { file: name, row })),
            Ok(None) => {}
            Err(error) => return Ok(Some(FirstProblem::File { file: name, error })),
        }
    }
    Ok(None)
}
