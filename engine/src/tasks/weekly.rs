//! Folds recurring finance tasks into one weekly session reminder.
//!
//! Tasks that repeat every week, two weeks or month leave the synced
//! targets (Reminders, Calendar, Notion). One `weekly-session-<date>` task
//! takes their place, due on the chosen weekday, and its notes list them.
//! Yearly and one-time tasks stay as their own items. `bin/cashcast tasks`
//! still lists every task. Ticking a session clears the folded tasks due
//! up to six days after it from the synced targets.

use crate::strategy::Task;
use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// Repeat intervals that fold into the weekly session.
const FOLDED: [&str; 3] = ["week", "2 weeks", "month"];
pub const PREFIX: &str = "weekly-session-";

/// `sunday` → Sunday; `off` (or anything unknown) → None.
pub fn weekday(setting: &str) -> Option<Weekday> {
    setting.trim().to_lowercase().parse::<Weekday>().ok()
}

fn folds(t: &Task) -> bool {
    FOLDED.contains(&t.recurs.as_str())
}

/// The session day: the first `day` on or after today that is not ticked.
/// A missed week rolls to the next one, whose title counts the overdue tasks.
fn session_day(today: NaiveDate, day: Weekday, done: &[String]) -> NaiveDate {
    let mut d = today;
    while d.weekday() != day {
        d += Duration::days(1);
    }
    while done.contains(&format!("{PREFIX}{d}")) {
        d += Duration::days(7);
    }
    d
}

/// `open`: the open tasks. `done`: ids of ticked tasks.
pub fn fold(open: Vec<Task>, today: NaiveDate, day: Weekday, done: &[String]) -> Vec<Task> {
    let (mut folded, mut kept): (Vec<Task>, Vec<Task>) = open.into_iter().partition(folds);
    // A ticked session covers the folded tasks due before the next one.
    let covered = done
        .iter()
        .filter_map(|id| id.strip_prefix(PREFIX)?.parse::<NaiveDate>().ok())
        .max();
    if let Some(last) = covered {
        folded.retain(|t| t.due.is_some_and(|d| d > last + Duration::days(6)));
    }
    if folded.is_empty() {
        return kept;
    }
    let when = session_day(today, day, done);
    folded.sort_by(|a, b| (a.due, &a.id).cmp(&(b.due, &b.id)));
    let mut lines = vec![
        "Run `/cashcast weekly` (or `bin/cashcast weekly`): one command refreshes the \
         banks, syncs MoneyWiz, runs the forecast, plans transfers, reviews your stocks and \
         crypto against your goals, says what to invest where, clears what it can, and \
         lists this week's moves."
            .to_string(),
        String::new(),
        format!("Recurring tasks ({}):", folded.len()),
    ];
    for t in &folded {
        let date = t.due.map_or("no date".into(), |d| {
            let tag = if d < today { " (overdue)" } else { "" };
            format!("{}{tag}", d.format("%a %b %-d"))
        });
        lines.push(format!("- {date}: {}", t.title));
    }
    let risk = if folded.iter().any(|t| t.risk == "money") {
        "money"
    } else {
        "read"
    };
    let overdue = folded
        .iter()
        .filter(|t| t.due.is_some_and(|d| d < today))
        .count();
    let title = match overdue {
        0 => "Weekly cashcast session".to_string(),
        n => format!("Weekly cashcast session ({n} overdue)"),
    };
    let session = Task::new(format!("{PREFIX}{when}"), title)
        .due(when)
        .notes(&lines.join("\n"))
        .risk(risk);
    kept.push(session);
    kept
}

/// Two tasks that ask for the same work: keep one, drop the other.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlap {
    pub keep: String,
    pub drop: String,
    pub why: String,
}

/// Task titles whose work `bin/cashcast weekly` already does every week.
const WEEKLY_DOES: [(&str, &str); 3] = [
    (
        "refresh cashcast data",
        "it imports fresh data and runs the forecast",
    ),
    ("check the forecast", "it runs the forecast"),
    (
        "update priorities & finances",
        "it runs the review and closes what it can",
    ),
];

/// Lowercase words, `$1,400.00` → `1400`, punctuation dropped.
fn words(title: &str) -> Vec<String> {
    title
        .to_lowercase()
        .split_whitespace()
        .map(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '$' && c != '&');
            match w.strip_prefix('$') {
                Some(n) => n.replace(',', "").trim_end_matches(".00").to_string(),
                None => w.to_string(),
            }
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// "Pay <card> down to $N": the card's words (without "the", "my", "card")
/// and the target.
fn paydown(title: &str) -> Option<(Vec<String>, String)> {
    let w = words(title);
    let down = w.iter().position(|x| x == "down")?;
    if w.first()? != "pay" || down < 2 || w.get(down + 1)? != "to" {
        return None;
    }
    let card: Vec<String> = w[1..down]
        .iter()
        .filter(|x| !matches!(x.as_str(), "the" | "my" | "card"))
        .cloned()
        .collect();
    Some((card, w.get(down + 2).cloned().unwrap_or_default()))
}

/// The same card paid down to the same target: one card name starts the other.
fn same_paydown(a: &str, b: &str) -> bool {
    match (paydown(a), paydown(b)) {
        (Some((ca, ta)), Some((cb, tb))) if ta == tb && !ca.is_empty() && !cb.is_empty() => {
            let n = ca.len().min(cb.len());
            ca[..n] == cb[..n]
        }
        _ => false,
    }
}

/// An id cashcast makes for one month or day: it ends in `-YYYY-MM[-DD]`.
pub fn dated(id: &str) -> bool {
    let parts: Vec<&str> = id.rsplit('-').take(3).collect();
    let num = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
    match parts.as_slice() {
        [d, m, y] if num(y, 4) && num(m, 2) && num(d, 2) => true,
        [m, y, ..] => num(y, 4) && num(m, 2),
        _ => false,
    }
}

/// Open tasks that duplicate each other or the weekly command itself.
pub fn overlaps(tasks: &[Task]) -> Vec<Overlap> {
    let mut out = Vec::new();
    for (i, a) in tasks.iter().enumerate() {
        let title = a.title.to_lowercase();
        if let Some((_, why)) = WEEKLY_DOES.iter().find(|(p, _)| title.contains(p)) {
            out.push(Overlap {
                keep: "bin/cashcast weekly".into(),
                drop: a.id.clone(),
                why: (*why).to_string(),
            });
            continue;
        }
        for b in &tasks[i + 1..] {
            let gone = |id: &str| out.iter().any(|o: &Overlap| o.drop == id);
            if gone(&a.id) {
                break;
            }
            if a.due != b.due || gone(&b.id) {
                continue;
            }
            let why = if words(&a.title) == words(&b.title) {
                "same task twice on the same day"
            } else if same_paydown(&a.title, &b.title) {
                "the same card paid down to the same target on the same day"
            } else {
                continue;
            };
            // Keep the one cashcast makes each month (a dated id), then the
            // one that names the card or task in full.
            let rank = |t: &Task| (dated(&t.id), t.title.len());
            let (keep, drop) = if rank(b) > rank(a) { (b, a) } else { (a, b) };
            out.push(Overlap {
                keep: keep.id.clone(),
                drop: drop.id.clone(),
                why: why.into(),
            });
        }
    }
    out
}

/// From the forecast's "Lowest month-end cash: $X in M. Cash floor: $Y."
/// line: Some(true) when the lowest month stays at or above the floor.
pub fn floor_clear(status: &str) -> Option<bool> {
    let line = status
        .lines()
        .find(|l| l.starts_with("Lowest month-end cash:"))?;
    let amount = |s: &str| -> Option<f64> {
        let s = s.trim();
        let neg = s.starts_with('-');
        let n: String = s
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
            .filter(|c| *c != ',')
            .collect();
        let v: f64 = n.trim_end_matches('.').parse().ok()?;
        Some(if neg { -v } else { v })
    };
    let low = amount(
        line.strip_prefix("Lowest month-end cash:")?
            .split(" in ")
            .next()?,
    )?;
    let floor = amount(line.split("Cash floor:").nth(1)?)?;
    Some(low >= floor)
}

/// The moves for the person this week (overdue ones too), by day, without
/// the dropped overlaps: money moves first, then the other things to do.
pub fn moves(tasks: &[Task], today: NaiveDate, dropped: &[String]) -> String {
    let end = today + Duration::days(7);
    let mut due: Vec<&Task> = tasks
        .iter()
        .filter(|t| !t.id.starts_with(PREFIX) && !dropped.contains(&t.id))
        .filter(|t| t.due.is_some_and(|d| d <= end))
        .collect();
    due.sort_by(|a, b| (a.due, &a.id).cmp(&(b.due, &b.id)));
    let line = |t: &Task| {
        let d = t.due.unwrap_or(today);
        let tag = if d < today { " (overdue)" } else { "" };
        format!("- {}{tag}: {} (`{}`)", d.format("%a %b %-d"), t.title, t.id)
    };
    let (money, other): (Vec<&Task>, Vec<&Task>) = due.into_iter().partition(|t| t.risk == "money");
    let mut out = vec![
        format!("## Money moves you make ({})", money.len()),
        String::new(),
    ];
    out.extend(money.iter().map(|t| line(t)));
    if money.is_empty() {
        out.push("- none this week".into());
    }
    out.extend([
        String::new(),
        format!("## Other things for you ({})", other.len()),
        String::new(),
    ]);
    out.extend(other.iter().map(|t| line(t)));
    if other.is_empty() {
        out.push("- none this week".into());
    }
    out.push(String::new());
    out.push(
        "cashcast never moves money. Tick each one when done, then the weekly reminder.".into(),
    );
    out.join("\n")
}

/// The weekly downloads read the bank sites under `agent:browser-read`. Name
/// it when it is missing or ends within 7 days, so the owner grants it before
/// a Sunday run finds it gone.
pub fn browser_read_note(until: Option<NaiveDate>, day: NaiveDate) -> Option<String> {
    let grant = "Run in Terminal: bin/cashcast consent grant agent:browser-read";
    match until {
        None => Some(format!(
            "Bank site access not granted: the Sunday downloads need it. {grant}"
        )),
        Some(end) if end < day + chrono::Duration::days(7) => Some(format!(
            "Bank site access ends {end}, within a week; the Sunday downloads stop then. {grant}"
        )),
        Some(_) => None,
    }
}

/// What the weekly run found out of date: stale exports (`fetch plan`),
/// MoneyWiz accounts behind their bank or with no MoneyWiz account
/// (`moneywiz`), a forecast start month pinned in the past, and a history
/// window that ends before last month. Names only, never amounts.
pub fn not_current(
    fetch_out: &str,
    moneywiz_out: &str,
    start_month: Option<&str>,
    last_month: Option<&str>,
    day: NaiveDate,
) -> Vec<String> {
    let mut out = Vec::new();
    for line in fetch_out.lines() {
        // `- paypal (PayPal): stale, newest 2026-09-27 (50 days old). browser: …`
        let Some(rest) = line.strip_prefix("- ") else {
            continue;
        };
        // The state follows the first ": "; a crypto note "(crypto: stale
        // after 31 days)" can follow it, also on a fresh line.
        let Some((who, state)) = rest.split_once(": ") else {
            continue;
        };
        let Some(state) = state.strip_prefix("stale") else {
            continue;
        };
        let state = match state.trim_start().strip_prefix("(crypto:") {
            Some(note) => note.split_once(')').map_or("", |(_, r)| r),
            None => state,
        };
        let when = state
            .trim_start_matches(',')
            .split(". ")
            .next()
            .unwrap_or_default()
            .trim();
        out.push(format!("Export stale: {who}, {when}"));
    }
    let after = |prefix: &str| {
        moneywiz_out
            .lines()
            .find_map(|l| l.strip_prefix(prefix))
            .and_then(|r| r.split(". ").next())
            .map(|r| r.trim().trim_end_matches('.').to_string())
    };
    // "Exports older than N days: …" (N is `fetch.stale_days`).
    let behind = moneywiz_out
        .lines()
        .filter_map(|l| l.strip_prefix("Exports older than "))
        .find_map(|l| l.split_once(" days: "))
        .and_then(|(_, r)| r.split(". ").next())
        .map(|r| r.trim().trim_end_matches('.').to_string());
    if let Some(names) = behind {
        out.push(format!("MoneyWiz behind the bank: {names}"));
    }
    if let Some(names) = after("Export accounts with no MoneyWiz account: ") {
        out.push(format!("Exports with no MoneyWiz account: {names}"));
    }
    // `moneywiz coverage` with `[moneywiz] since`: rows from that day on that
    // no bank row matches yet, or bank rows MoneyWiz lacks.
    if let Some((head, rest)) = moneywiz_out
        .lines()
        .filter_map(|l| l.strip_prefix("History gaps since "))
        .find_map(|l| l.split_once(": "))
        .filter(|(_, r)| *r != "none.")
    {
        let names = rest.split(". Download each range").next().unwrap_or(rest);
        out.push(format!("MoneyWiz history gaps since {head}: {names}"));
    }
    let this_month = day.format("%Y-%m").to_string();
    if let Some(start) = start_month.filter(|s| *s < this_month.as_str()) {
        out.push(format!(
            "Forecast start pinned to {start}, before this month: remove `start_month` under [forecast] in cashcast.toml so it starts this month"
        ));
    }
    let first = NaiveDate::from_ymd_opt(day.year(), day.month(), 1).unwrap_or(day);
    let prev = (first - chrono::Duration::days(1))
        .format("%Y-%m")
        .to_string();
    if let Some(last) = last_month.filter(|m| *m < prev.as_str()) {
        out.push(format!(
            "History pinned to end in {last}, before last month: remove `last_month` under [window] in cashcast.toml so recent months count"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn task(id: &str, due: &str, recurs: &str) -> Task {
        let mut t = Task::new(id, format!("Title {id}")).due(d(due));
        t.recurs = recurs.into();
        t
    }

    fn sample() -> Vec<Task> {
        vec![
            task("chexy-prepay-2026-11-18", "2026-11-18", "month"),
            task("paydown-td-2026-11", "2026-11-16", "month").risk("money"),
            task("pay-taxes-2027-04-15", "2027-04-15", "year"),
            task("chexy-full-rent", "2027-01-15", ""),
        ]
    }

    #[test]
    fn monthly_tasks_fold_into_one_sunday_session_and_the_rest_stay() {
        // Wed 2026-11-11 → the session is Sun 2026-11-15.
        let out = fold(sample(), d("2026-11-11"), Weekday::Sun, &[]);
        let ids: Vec<&str> = out.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "pay-taxes-2027-04-15",
                "chexy-full-rent",
                "weekly-session-2026-11-15"
            ]
        );
        let s = out.last().unwrap();
        assert_eq!(s.due, Some(d("2026-11-15")));
        assert_eq!(s.risk, "money");
        assert!(s.notes.contains("Recurring tasks (2):"), "{}", s.notes);
        // Sorted by date: the paydown (16th) before the pre-pay (18th).
        let td = s.notes.find("Mon Nov 16: Title paydown-td").unwrap();
        let chexy = s.notes.find("Wed Nov 18: Title chexy-prepay").unwrap();
        assert!(td < chexy, "{}", s.notes);
    }

    #[test]
    fn a_missed_session_counts_overdue() {
        // Missed: on Sat 2026-11-21 the session is the next Sunday, and its
        // title and notes show the paydown (due the 16th) as overdue.
        let out = fold(sample(), d("2026-11-21"), Weekday::Sun, &[]);
        let s = out.last().unwrap();
        assert_eq!(s.id, "weekly-session-2026-11-22");
        assert_eq!(s.title, "Weekly cashcast session (2 overdue)");
        assert!(s.notes.contains("Mon Nov 16 (overdue)"), "{}", s.notes);
    }

    #[test]
    fn ticking_a_session_clears_the_tasks_it_covered() {
        // Ticked Sun Nov 15 covers tasks due through Sat Nov 21.
        let ticked = vec!["weekly-session-2026-11-15".to_string()];
        let out = fold(sample(), d("2026-11-16"), Weekday::Sun, &ticked);
        assert!(out.iter().all(|t| !t.id.starts_with(PREFIX)), "{out:?}");
        assert_eq!(out.len(), 2);
        // A task due after that week still folds into the next session.
        let mut later = sample();
        later.push(task("paydown-td-2026-12", "2026-12-16", "month"));
        let out = fold(later, d("2026-11-16"), Weekday::Sun, &ticked);
        let s = out.last().unwrap();
        assert_eq!(s.id, "weekly-session-2026-11-22");
        assert!(s.notes.contains("Recurring tasks (1):"), "{}", s.notes);
    }

    #[test]
    fn on_the_session_day_itself_the_session_is_today() {
        let out = fold(sample(), d("2026-11-15"), Weekday::Sun, &[]);
        assert_eq!(out.last().unwrap().id, "weekly-session-2026-11-15");
    }

    #[test]
    fn two_paydowns_for_one_card_are_one_overlap() {
        // A hand-written paydown and the generated one: same card, day, target.
        let mine = Task::new(
            "paydown-canary",
            "Pay Canary card down to $400 (closes 25th)",
        )
        .due(d("2026-11-23"))
        .risk("money");
        let generated = Task::new(
            "paydown-canary-momentum-visa-2026-11",
            "Pay Canary Momentum Visa down to $400.00",
        )
        .due(d("2026-11-23"))
        .risk("money");
        let other = Task::new(
            "paydown-other-visa-2026-11",
            "Pay Other Visa down to $400.00",
        )
        .due(d("2026-11-23"))
        .risk("money");
        // Same first word, another card: not an overlap.
        let sibling = Task::new(
            "paydown-canary-cash-back-2026-11",
            "Pay Canary Cash Back Mastercard down to $400.00",
        )
        .due(d("2026-11-23"))
        .risk("money");
        let found = overlaps(&[mine, generated, other, sibling]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].keep, "paydown-canary-momentum-visa-2026-11");
        assert_eq!(found[0].drop, "paydown-canary");
    }

    #[test]
    fn a_task_the_weekly_command_does_is_an_overlap() {
        let t = Task::new("refresh-1", "Refresh cashcast data and check the forecast")
            .due(d("2026-11-27"));
        let found = overlaps(&[t]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].keep, "bin/cashcast weekly");
        // Same title on the same day twice is an overlap too.
        let a = Task::new("a", "Ensure rent is fine").due(d("2026-12-01"));
        let b = Task::new("b", "Ensure  rent is fine.").due(d("2026-12-01"));
        // Three copies: each extra one is dropped once, the first is kept.
        let c = Task::new("c", "Ensure rent is fine!").due(d("2026-12-01"));
        let found = overlaps(&[a, b, c]);
        let drops: Vec<&str> = found.iter().map(|o| o.drop.as_str()).collect();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            !drops.contains(&found[0].keep.as_str()) || found.iter().all(|o| o.keep != o.drop),
            "{found:?}"
        );
        let mut uniq = drops.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), drops.len(), "{found:?}");
    }

    #[test]
    fn the_floor_line_decides_new_money() {
        let ok = "Lowest month-end cash: $30,000.00 in 2027-03. Cash floor: $26,500.00.";
        assert_eq!(floor_clear(ok), Some(true));
        let low = "Lowest month-end cash: -$1,200.50 in 2027-03. Cash floor: $26,500.00.";
        assert_eq!(floor_clear(low), Some(false));
        assert_eq!(floor_clear("no forecast"), None);
    }

    #[test]
    fn moves_list_this_week_by_day_without_dropped_overlaps() {
        let tasks = vec![
            Task::new("pay", "Pay Canary down to $1")
                .due(d("2026-11-16"))
                .risk("money"),
            Task::new("late", "Later move")
                .due(d("2026-11-30"))
                .risk("money"),
            Task::new("call", "Call the bank")
                .due(d("2026-11-14"))
                .risk("settings"),
            Task::new("dup", "Duplicate")
                .due(d("2026-11-16"))
                .risk("money"),
        ];
        let text = moves(&tasks, d("2026-11-15"), &["dup".to_string()]);
        assert!(text.contains("## Money moves you make (1)"), "{text}");
        assert!(
            text.contains("- Mon Nov 16: Pay Canary down to $1 (`pay`)"),
            "{text}"
        );
        assert!(
            text.contains("- Sat Nov 14 (overdue): Call the bank (`call`)"),
            "{text}"
        );
        assert!(
            !text.contains("Later move") && !text.contains("Duplicate"),
            "{text}"
        );
    }

    #[test]
    fn nothing_recurring_means_no_session() {
        let only_once = vec![task("chexy-full-rent", "2027-01-15", "")];
        let out = fold(only_once, d("2026-11-11"), Weekday::Sun, &[]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn the_setting_names_a_weekday_or_turns_the_fold_off() {
        assert_eq!(weekday("sunday"), Some(Weekday::Sun));
        assert_eq!(weekday("Sat"), Some(Weekday::Sat));
        assert_eq!(weekday("off"), None);
        // A typo is refused instead of silently turning the fold off.
        let key = "reminders.weekly";
        assert!(crate::core::settings::parse(key, "sundy").is_err());
        assert!(crate::core::settings::parse(key, "off").is_ok());
        assert!(crate::core::settings::parse(key, "Saturday").is_ok());
    }

    #[test]
    fn weekly_names_what_is_not_current() {
        let fetch = "# Exports to refresh (stale after 7 days)\n\n\
- paypal (PayPal): stale, newest 2026-09-27 (50 days old). browser: x\n\
- wise (Wise): fresh, newest 2026-11-15 (1 days old). API: y\n";
        let mw = "| a | b |\n\nExports older than 6 days: Bank A (ends 2026-10-30, downloaded 2026-11-08), Bank B (no rows). Download newer rows (`bin/cashcast connectors show <bank>`), then sync again.\n\n\
Export accounts with no MoneyWiz account: canary-x, canary-y. Link one in cashcast.toml: [moneywiz.accounts] \"MoneyWiz name\" = \"export name\".\n";
        let got = not_current(fetch, mw, Some("2026-10"), Some("2026-09"), d("2026-11-16"));
        assert_eq!(
            got,
            vec![
                "Export stale: paypal (PayPal), newest 2026-09-27 (50 days old)".to_string(),
                "MoneyWiz behind the bank: Bank A (ends 2026-10-30, downloaded 2026-11-08), Bank B (no rows)".into(),
                "Exports with no MoneyWiz account: canary-x, canary-y".into(),
                "Forecast start pinned to 2026-10, before this month: remove `start_month` under [forecast] in cashcast.toml so it starts this month".into(),
                "History pinned to end in 2026-09, before last month: remove `last_month` under [window] in cashcast.toml so recent months count".into(),
            ]
        );
        // All current: nothing to list.
        let fresh = "- wise (Wise): fresh, newest 2026-11-15 (1 days old). API: y\n";
        assert!(
            not_current(
                fresh,
                "0 rows to add",
                Some("2026-11"),
                Some("2026-10"),
                d("2026-11-16")
            )
            .is_empty()
        );
        assert!(not_current(fresh, "", None, None, d("2026-11-16")).is_empty());
        let gaps = "History gaps since 2026-08-01: Bank A (download 2026-08-01 to 2026-08-31), Bank B (2 bank rows to add: `moneywiz sync`). Download each range (`bin/cashcast connectors show <bank>`), then `moneywiz sync` and `moneywiz edit --fixes`.\n";
        assert_eq!(
            not_current(fresh, gaps, None, None, d("2026-11-16")),
            [
                "MoneyWiz history gaps since 2026-08-01: Bank A (download 2026-08-01 to 2026-08-31), Bank B (2 bank rows to add: `moneywiz sync`)"
            ]
        );
        // A crypto service under its own 31-day limit: fresh is not stale.
        let crypto = "- casa (Casa): fresh (crypto: stale after 31 days), newest 2026-11-01 (16 days old). browser: z\n\
- kraken (Kraken): stale (crypto: stale after 31 days), newest 2026-10-01 (47 days old), downloaded 2026-10-02. API: k\n";
        assert_eq!(
            not_current(crypto, "", None, None, d("2026-11-17")),
            ["Export stale: kraken (Kraken), newest 2026-10-01 (47 days old), downloaded 2026-10-02"]
        );
        let done = "History gaps since 2026-08-01: none.\n";
        assert!(not_current(fresh, done, None, None, d("2026-11-16")).is_empty());
    }
    #[test]
    fn weekly_warns_before_the_browser_grant_ends() {
        let day = d("2026-11-22");
        assert_eq!(
            browser_read_note(None, day).unwrap(),
            "Bank site access not granted: the Sunday downloads need it. Run in Terminal: bin/cashcast consent grant agent:browser-read"
        );
        assert_eq!(
            browser_read_note(Some(d("2026-11-28")), day).unwrap(),
            "Bank site access ends 2026-11-28, within a week; the Sunday downloads stop then. Run in Terminal: bin/cashcast consent grant agent:browser-read"
        );
        assert!(browser_read_note(Some(d("2026-11-29")), day).is_none());
    }
}
