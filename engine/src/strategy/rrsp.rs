//! Keeps this year's RRSP contributions within the deduction limit, as
//! `src/cashcast/rrsp.py`.
//!
//! The window for year Y runs from day 61 of Y to day 60 of Y+1. The CRA
//! allows $2,000 of cumulative excess; above that, it charges 1% of the
//! excess per month. The check also projects the window's total from the
//! last three months' pace.

use super::Task;
use super::py::{self, getf, gets, list, money, obj};
use crate::core::pyjson::Json;
use chrono::{Datelike, Duration, NaiveDate};

const PACE_MONTHS: usize = 3;

/// The year whose limit a contribution on `day` uses (the engine's rule).
pub fn deduction_year(day: NaiveDate) -> i32 {
    if day.ordinal() <= 60 {
        day.year() - 1
    } else {
        day.year()
    }
}

/// First and last day of the contribution window for `year`.
pub fn window(year: i32) -> (NaiveDate, NaiveDate) {
    let jan1 = |y| NaiveDate::from_ymd_opt(y, 1, 1).expect("valid");
    (
        jan1(year) + Duration::days(60),
        jan1(year + 1) + Duration::days(59),
    )
}

fn prev_months(today: NaiveDate, n: usize) -> Vec<String> {
    let (mut y, mut m) = (today.year(), today.month() as i32);
    let mut out = Vec::new();
    for _ in 0..n {
        m -= 1;
        if m == 0 {
            y -= 1;
            m = 12;
        }
        out.push(format!("{y:04}-{m:02}"));
    }
    out
}

fn long(d: NaiveDate) -> String {
    d.format("%b %-d, %Y").to_string()
}

/// `statistics.median`.
fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

pub struct Check {
    pub year: i32,
    pub status: String,
    pub lines: Vec<String>,
    pub tasks: Vec<Task>,
    pub data: Vec<(String, Json)>,
}

fn f(x: f64) -> Json {
    Json::Float(x)
}

/// `limits` is `[limits]` of the tax knowledge file (the buffer and the monthly tax rate).
pub fn check(profile: &Json, holdings: Option<&Json>, today: NaiveDate, limits: &Json) -> Check {
    let buffer = getf(limits, "rrsp_excess_buffer", 2000.0);
    let penalty_rate = getf(limits, "rrsp_excess_tax_monthly", 0.01);
    let year = deduction_year(today);
    let (start, end) = window(year);
    let rows: Vec<&Json> = holdings
        .map(|h| {
            list(h, "contributions")
                .iter()
                .filter(|c| gets(c, "account_bucket", "") == "RRSP")
                .collect()
        })
        .unwrap_or_default();
    let mine: Vec<&&Json> = rows
        .iter()
        .filter(|c| py::num(c.get("deduction_year")).unwrap_or(0.0) as i32 == year)
        .collect();
    let is_employer = |c: &Json| c.get("source").is_some_and(|s| s.py_str() == "employer");
    let member = py::sum(
        mine.iter()
            .filter(|c| !is_employer(c))
            .map(|c| getf(c, "cad", 0.0)),
    );
    let employer = py::sum(
        mine.iter()
            .filter(|c| is_employer(c))
            .map(|c| getf(c, "cad", 0.0)),
    );
    let room = obj(profile, "room");
    let other = getf(room, "rrsp_other", 0.0);
    let contributed = member + employer + other;
    let by_month: Vec<f64> = prev_months(today, PACE_MONTHS)
        .iter()
        .map(|m| {
            py::sum(
                rows.iter()
                    .filter(|c| c.get("month").map(Json::py_str).as_ref() == Some(m))
                    .map(|c| getf(c, "cad", 0.0)),
            )
        })
        .collect();
    let pace = median(by_month);
    let newest = rows
        .iter()
        .map(|c| c.get("month").map_or(String::new(), Json::py_str))
        .max()
        .unwrap_or_default();
    let months_left = ((end - today).num_days() as f64 / 30.44).max(0.0);
    let projected = contributed + pace * months_left;
    let mut res = Check {
        year,
        status: "ok".into(),
        lines: Vec::new(),
        tasks: Vec::new(),
        data: Vec::new(),
    };
    res.data = vec![
        ("year".into(), Json::Int(year as i64)),
        (
            "window".into(),
            Json::List(vec![
                Json::Str(start.to_string()),
                Json::Str(end.to_string()),
            ]),
        ),
        ("member".into(), f(py::round(member, 2))),
        ("employer".into(), f(py::round(employer, 2))),
        ("other".into(), f(py::round(other, 2))),
        ("contributed".into(), f(py::round(contributed, 2))),
        ("monthly_pace".into(), f(py::round(pace, 2))),
        ("projected".into(), f(py::round(projected, 2))),
        ("newest_month".into(), Json::Str(newest.clone())),
    ];
    res.lines.push(format!(
        "- RRSP contributions for {year} (window {} to {}): **{}** (you {}, employer {}{}).",
        long(start),
        long(end),
        money(contributed),
        money(member),
        money(employer),
        if other != 0.0 {
            format!(", other {}", money(other))
        } else {
            String::new()
        }
    ));
    let prev2 = prev_months(today, 2);
    if !newest.is_empty() && newest < prev2[prev2.len() - 1] {
        res.lines.push(format!(
            "- The newest contribution in your exports is from {newest}, so the pace may miss recent payroll: \
             export the Sun Life transaction history again."
        ));
        let mut t = Task::new(format!("rrsp-refresh-{newest}"), "Download the Sun Life RRSP transaction history into the inbox")
            .due(today)
            .notes("An agent can do it (connectors/playbooks/sunlife.md); the next sync parses it and checks the limit.");
        t.service = "sunlife".into();
        res.tasks.push(t);
    }
    if rows.is_empty() && other == 0.0 {
        res.lines.push(
            "- No RRSP contributions found. Add your Sun Life transaction history to the inbox (connectors show sunlife), \
             or set `[room] rrsp_other` for contributions no export shows."
                .into(),
        );
    }
    let limit = getf(room, "rrsp", 0.0);
    let limit_year = room
        .get("rrsp_limit_year")
        .filter(|v| v.truthy())
        .and_then(|v| py::num(Some(v)))
        .unwrap_or(0.0) as i32;
    if limit <= 0.0 || limit_year != year {
        res.status = "no-limit".into();
        let why = if limit <= 0.0 {
            "is missing".to_string()
        } else {
            format!(
                "is for {}, not {year}",
                if limit_year != 0 {
                    limit_year.to_string()
                } else {
                    "an unknown year".into()
                }
            )
        };
        res.lines.push(format!(
            "- Your RRSP deduction limit for {year} {why}, so the check cannot say whether you are within it."
        ));
        res.tasks.push(
            Task::new(format!("rrsp-limit-{year}"), format!("Copy your {year} RRSP deduction limit into cashcast"))
                .due(today)
                .notes(&format!(
                    "CRA My Account > RRSP and TFSA (or the latest Notice of Assessment): copy 'RRSP deduction limit for {year}' \
                     to profile.toml [room] rrsp, set rrsp_limit_year = {year}, and rrsp_unused if the NOA lists unused contributions."
                )),
        );
        return res;
    }
    let unused = getf(room, "rrsp_unused", 0.0);
    let available = limit - unused;
    let left = available - contributed;
    res.data.extend([
        ("limit".into(), f(limit)),
        ("unused".into(), f(unused)),
        ("available".into(), f(py::round(available, 2))),
        ("left".into(), f(py::round(left, 2))),
    ]);
    res.lines.push(format!(
        "- Limit {}{}: **{} left**.",
        money(limit),
        if unused != 0.0 {
            format!(" minus {} unused contributions", money(unused))
        } else {
            String::new()
        },
        money(left)
    ));
    let first_60 = year != today.year();
    if left < 0.0 && first_60 {
        res.lines.push(format!(
            "- It is the first 60 days of {y}: the CRA's monthly excess test already counts your new \
             {y} limit, so check CRA My Account before you withdraw anything.",
            y = today.year()
        ));
    }
    if left < -buffer {
        res.status = "penalty".into();
        let excess = -left - buffer;
        res.data.push((
            "penalty_per_month".into(),
            f(py::round(excess * penalty_rate, 2)),
        ));
        res.lines.push(format!(
            "- **Over the limit by {}**, {} above the $2,000 buffer: the CRA charges about \
             {} a month until it is withdrawn.",
            money(-left),
            money(excess),
            money(excess * penalty_rate)
        ));
        let mut t = Task::new(format!("rrsp-over-{year}"), format!("Stop RRSP contributions and withdraw the {} excess", money(excess)))
            .due(today)
            .notes(
                "Pause Sun Life payroll contributions, ask for a withdrawal of the excess, and file form T1-OVP. \
                 Check the numbers in CRA My Account first.",
            )
            .risk("money");
        t.service = "sunlife".into();
        res.tasks.push(t);
    } else if left < 0.0 {
        res.status = "over".into();
        res.lines.push(format!(
            "- Over the limit by {}, inside the $2,000 buffer: no tax yet, but any more contribution \
             this window uses the buffer. {} of buffer left.",
            money(-left),
            money(buffer + left)
        ));
        res.tasks.push(pause_task(
            year,
            today,
            "You are over the limit, inside the $2,000 buffer.",
        ));
    } else if projected > available {
        res.status = "projected-over".into();
        let cross = if pace > 0.0 {
            today + Duration::days((left / pace * 30.44) as i64)
        } else {
            end
        };
        res.data
            .push(("crosses_on".into(), Json::Str(cross.to_string())));
        res.lines.push(format!(
            "- At your pace of {} a month you reach the limit around **{}** \
             and end the window at {}, {} over.",
            money(pace),
            long(cross),
            money(projected),
            money(projected - available)
        ));
        res.tasks.push(pause_task(
            year,
            today.max(cross - Duration::days(14)),
            &format!(
                "At this pace you reach the limit around {}.",
                cross.format("%b %-d")
            ),
        ));
    } else if left < pace * 2.0 {
        res.status = "near".into();
        res.lines.push(format!(
            "- Close to the limit: less than two months of contributions left ({} a month).",
            money(pace)
        ));
    } else if pace < 0.005 {
        res.lines.push(format!(
            "- Within the limit: no contributions so far in this window (to {}).",
            long(end)
        ));
    } else {
        res.lines.push(format!(
            "- Within the limit, also at your pace of {} a month to {}.",
            money(pace),
            long(end)
        ));
    }
    res
}

fn pause_task(year: i32, due: NaiveDate, why: &str) -> Task {
    let mut t = Task::new(format!("rrsp-pause-{year}"), "Lower or pause your Sun Life RRSP payroll contributions")
        .due(due)
        .notes(&format!(
            "{why} The employer match may stop too; check your plan's rules. Often changed through HR or payroll."
        ))
        .risk("settings");
    t.service = "sunlife".into();
    t.action = "change-contribution-rate".into();
    t
}

pub fn report(res: &Check) -> String {
    let mut parts = vec![
        "## RRSP contributions vs your limit".to_string(),
        String::new(),
    ];
    parts.extend(res.lines.iter().cloned());
    parts.push(String::new());
    parts.join("\n")
}
