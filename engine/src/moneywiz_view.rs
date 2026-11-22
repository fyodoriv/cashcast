//! The web app's Bank sync view: what `moneywiz sync` would do, as JSON.
//!
//! The owner reads it at their own screen (the web app needs the token), so it
//! holds the rows to add with date, text and amount. The CLI and agents get
//! the counts-only report of `moneywiz sync --dry-run` instead.

use crate::config::Config;
use crate::inbox::Inbox;
use crate::money::fmt_cents;
use crate::moneywiz::{self, SentList, SentState, Store, corrected};
use crate::moneywiz_coverage::{self, Connector};
use chrono::NaiveDate;
use serde_json::{Value, json};
use std::path::Path;

fn row(t: &crate::export::Txn) -> Value {
    json!({
        "date": t.date.to_string(),
        "text": t.text,
        "amount": fmt_cents(t.cents),
        "cents": t.cents,
        "category": t.category.join("/"),
    })
}

/// The Bank sync view for one person.
pub fn view(
    db: &Path,
    inbox: &Inbox,
    cfg: &Config,
    home: &Path,
    connectors: &[Connector],
    today: NaiveDate,
) -> Result<Value, String> {
    let mw = cfg.moneywiz.as_ref();
    let mode = moneywiz::mode(mw)?;
    let store: Store = moneywiz::open(db)?;
    let plan = moneywiz::plan(&store, inbox, mw)?;
    let links = moneywiz::links_for(&store, inbox, &plan, mode);
    let sent = SentList::load(&home.join("data"), today, false)?;
    let ledger = crate::moneywiz_ledger::Ledger::load(&home.join("data"));
    let amounts = corrected(&store);
    let held: std::collections::HashMap<i64, String> = plan
        .accounts
        .iter()
        .filter_map(|ap| {
            crate::moneywiz_ledger::hold_reason(&store, inbox, ap, &amounts, mode)
                .map(|r| (ap.mw, r))
        })
        .collect();
    let held_exports: std::collections::HashSet<&str> = plan
        .accounts
        .iter()
        .filter(|ap| held.contains_key(&ap.mw))
        .map(|ap| ap.export_account.as_str())
        .collect();
    let summary = ledger.summary();
    let (mut waiting, mut gone, mut to_send, mut held_links) = (0, 0, 0, 0);
    for l in &links {
        let account = l
            .rows
            .first()
            .map(|&i| inbox.txns[i].account.as_str())
            .or_else(|| l.id.split('|').nth(1));
        if account.is_some_and(|a| held_exports.contains(a)) {
            held_links += 1;
            continue;
        }
        match sent.state(l, today) {
            SentState::Waiting => waiting += 1,
            SentState::Gone => gone += 1,
            _ => to_send += 1,
        }
    }
    let accounts: Vec<Value> = plan
        .accounts
        .iter()
        .map(|ap| {
            let a = store.account(ap.mw).expect("linked account exists");
            let fixes: Vec<Value> = ap
                .fix
                .iter()
                .map(|&(pk, i)| {
                    let b = &inbox.txns[i];
                    json!({
                        "date": b.date.to_string(),
                        "text": b.text,
                        "moneywiz": fmt_cents(amounts[&pk]),
                        "bank": fmt_cents(b.cents),
                    })
                })
                .collect();
            let age = ap.to.map(|t| (today - t).num_days());
            let kind = if a.is_card() { "card" } else { "bank" };
            let (diff, added) = moneywiz::sums(inbox, ap, &amounts);
            let balance = moneywiz::balance_check(&store, ap, mode, diff, added);
            json!({
                "moneywiz": a.name,
                "kind": kind,
                "export": ap.export_account,
                "from": ap.from.map(|d| d.to_string()),
                "to": ap.to.map(|d| d.to_string()),
                "daysOld": age,
                "bankRows": ap.bank_rows,
                "matched": ap.matched,
                "add": ap.add.iter().map(|&i| row(&inbox.txns[i])).collect::<Vec<_>>(),
                "fix": fixes,
                "onlyInMoneywiz": ap.only_in_moneywiz.len(),
                "conflicts": ap.conflicts.len(),
                "balance": balance,
                "held": held.get(&ap.mw),
                "reconcile": ap.reconcile.len(),
                "standIn": ap.stand_in.len(),
                "onlyInMoneywizRows": ap.only_in_moneywiz.iter().filter_map(|pk| {
                    let m = store.txns.iter().find(|t| t.pk == *pk)?;
                    Some(json!({"date": m.date.to_string(), "text": m.text, "amount": fmt_cents(amounts[pk])}))
                }).collect::<Vec<_>>(),
                "ledger": summary.get(&ap.export_account).map(|(counts, last)| json!({
                    "statuses": counts,
                    "lastChecked": last.map(|d| d.to_string()),
                })),
            })
        })
        .collect();
    let mut c = moneywiz_coverage::coverage(&store, inbox, mw, connectors, today)?;
    moneywiz_coverage::add_trades(&mut c, &store, &home.join("inbox"), cfg, today);
    let pct = |a: usize, b: usize| {
        if b == 0 {
            None
        } else {
            Some((a * 100 / b) as i64)
        }
    };
    let covered: usize = c.accounts.iter().map(|a| a.covered_recent).sum();
    let rows: usize = c.accounts.iter().map(|a| a.rows_recent).sum();
    Ok(json!({
        "database": store.file_name,
        "mode": mode,
        "today": today.to_string(),
        "accounts": accounts,
        "untouched": plan.untouched.iter().filter_map(|pk| store.account(*pk)).map(|a| a.name.clone()).collect::<Vec<_>>(),
        "unlinkedExports": plan.unlinked_exports,
        "links": {"toSend": to_send, "waiting": waiting, "gone": gone, "held": held_links},
        "runs": ledger.runs.iter().rev().take(8).map(|r| json!({
            "at": r["at"],
            "apply": r["apply"],
            "sent": r["sent"],
            "held": r["accounts"].as_object().map(|o| o.values().filter(|a| !a["held"].is_null()).count()).unwrap_or(0),
        })).collect::<Vec<_>>(),
        "recheck": ledger.recheck.iter().map(|w| json!({
            "account": w.account,
            "from": w.from.map(|d| d.to_string()),
            "to": w.to.map(|d| d.to_string()),
        })).collect::<Vec<_>>(),
        "coverage": {
            "percent": pct(covered, rows),
            "covered": covered,
            "rows": rows,
            "accounts": c.accounts.iter().map(|a| json!({
                "name": a.name,
                "kind": a.kind,
                "rows": a.rows_recent,
                "covered": a.covered_recent,
                "connector": a.connector,
                "source": a.source,
                "gap": a.gap,
                "last": a.last.map(|d| d.to_string()),
            })).collect::<Vec<_>>(),
            "suggestions": c.suggestions.iter().map(|(m, e)| json!({"moneywiz": m, "export": e})).collect::<Vec<_>>(),
        },
        "moneywizExports": inbox.moneywiz_files.len(),
    }))
}
