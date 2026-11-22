//! Consent: nothing private leaves the computer unless its owner said yes.
//!
//! A grant names one scope. It is made only at a real terminal: the owner
//! types the scope name back. An agent cannot type into that prompt, so it
//! cannot grant itself access. Grants expire, can be revoked, and live in the
//! user's `consent.toml` (mode 600). That file is also editable by hand.
//!
//! Limits: this gate stops accidental sharing by cooperating agents. It is not
//! a sandbox against hostile code on your account.

use super::events;
use super::paths::{AppError, UserHome};
use super::pyjson::Json;
use chrono::NaiveDate;
use std::collections::BTreeMap;
use std::io::{BufRead, Write};

pub struct Scope {
    pub name: &'static str,
    pub shares: &'static str,
    pub recipient: &'static str,
    pub days: i64,
}

pub const SCOPES: [Scope; 13] = [
    Scope {
        name: "agent:aggregates",
        shares: "reports with totals, account and category names (never single transactions)",
        recipient: "the AI agent you run and its model provider",
        days: 90,
    },
    Scope {
        name: "agent:raw-row",
        shares: "one raw export row, to debug an unreadable file",
        recipient: "the AI agent and its model provider",
        days: 1,
    },
    Scope {
        name: "agent:strategy",
        shares: "your strategy report: card routing, targets, balances, room and dated checklist",
        recipient: "the AI agent and its model provider",
        days: 90,
    },
    Scope {
        name: "cloud:sheets",
        shares: "the forecast table you choose to sync",
        recipient: "Google Sheets (your spreadsheet)",
        days: 365,
    },
    Scope {
        name: "cloud:notion",
        shares: "task titles with every amount removed, due dates, the kind of task and whether it is done",
        recipient: "Notion (your own tasks database)",
        days: 365,
    },
    Scope {
        name: "net:market-data",
        shares: "ticker symbols from your watchlist (no amounts, no balances)",
        recipient: "the market-data provider you configure",
        days: 365,
    },
    Scope {
        name: "net:usage-counts",
        shares: "anonymous usage counts: command names, ok or failed, durations and counts (never amounts, names, payees, services or paths)",
        recipient: "PostHog EU (product analytics of the cashcast author), only once a key is set",
        days: 365,
    },
    Scope {
        name: "local:reminders",
        shares: "task titles with amounts and due dates",
        recipient: "Apple Reminders on this Mac, and iCloud if you sync Reminders",
        days: 365,
    },
    Scope {
        name: "local:calendar",
        shares: "dated task titles with amounts and due dates",
        recipient: "Apple Calendar on this Mac, and iCloud if you sync Calendar",
        days: 365,
    },
    Scope {
        name: "local:moneywiz",
        shares: "transactions from your exports (dates, amounts, payees, categories) written into MoneyWiz",
        recipient: "MoneyWiz on this Mac, and the MoneyWiz cloud if you turned on its sync",
        days: 365,
    },
    Scope {
        name: "actions:browser",
        shares: "on-screen account pages while an agent performs an approved action",
        recipient: "the AI agent and its model provider",
        days: 30,
    },
    Scope {
        name: "agent:browser-read",
        shares: "finance site pages an agent reads to check a task, and statements it saves to your private inbox",
        recipient: "the AI agent and its model provider (files stay on this Mac)",
        // 35 days: one grant a month covers every Sunday run in it; `weekly`
        // warns a week before it ends (owner's choice, 2026-11-17).
        days: 35,
    },
    Scope {
        name: "actions:api",
        shares: "API responses (balances, statements) fetched with your token",
        recipient: "stays local; reports follow agent:aggregates",
        days: 365,
    },
];

pub fn scope(name: &str) -> Option<&'static Scope> {
    SCOPES.iter().find(|s| s.name == name)
}

fn err(msg: impl Into<String>) -> AppError {
    AppError::new("consenterror", msg)
}

fn read(user: &UserHome) -> Result<Vec<toml::Table>, AppError> {
    let Ok(text) = std::fs::read_to_string(user.consent()) else {
        return Ok(Vec::new());
    };
    let table: toml::Table = text
        .parse()
        .map_err(|e| AppError::new("tomldecodeerror", format!("{e}")))?;
    Ok(match table.get("grants") {
        Some(toml::Value::Array(items)) => {
            items.iter().filter_map(|v| v.as_table().cloned()).collect()
        }
        _ => Vec::new(),
    })
}

/// A TOML date without a time, as Python's `tomllib` gives a `date`.
fn date_of(v: Option<&toml::Value>) -> Option<NaiveDate> {
    let toml::Value::Datetime(dt) = v? else {
        return None;
    };
    if dt.time.is_some() || dt.offset.is_some() {
        return None;
    }
    let d = dt.date?;
    NaiveDate::from_ymd_opt(d.year as i32, d.month as u32, d.day as u32)
}

/// f-string of a grant field: strings as they are, dates as ISO.
fn field(g: &toml::Table, key: &str) -> Result<String, AppError> {
    match g.get(key) {
        Some(toml::Value::String(s)) => Ok(s.clone()),
        Some(other) => Ok(other.to_string()),
        None => Err(AppError::new("keyerror", format!("'{key}'"))),
    }
}

fn write(user: &UserHome, grants: &[toml::Table]) -> Result<(), AppError> {
    let mut lines = vec![
        "# What may leave this computer. Edit by hand or run: bin/cashcast consent".to_string(),
        String::new(),
    ];
    for g in grants {
        lines.push("[[grants]]".into());
        lines.push(format!("scope = \"{}\"", field(g, "scope")?));
        lines.push(format!("granted_on = {}", field(g, "granted_on")?));
        lines.push(format!("expires_on = {}", field(g, "expires_on")?));
        lines.push(format!("via = \"{}\"", field(g, "via")?));
        lines.push(String::new());
    }
    std::fs::create_dir_all(&user.home).map_err(|e| err(e.to_string()))?;
    crate::write_private(&user.consent(), &lines.join("\n")).map_err(err)
}

/// Active scopes and when each ends.
pub fn active(user: &UserHome, today: NaiveDate) -> Result<BTreeMap<String, NaiveDate>, AppError> {
    let mut out = BTreeMap::new();
    for g in read(user)? {
        let Some(toml::Value::String(name)) = g.get("scope") else {
            continue;
        };
        if scope(name).is_none() {
            continue;
        }
        if let Some(expires) = date_of(g.get("expires_on")).filter(|e| *e >= today) {
            let best = out
                .get(name)
                .map_or(expires, |old: &NaiveDate| expires.max(*old));
            out.insert(name.clone(), best);
        }
    }
    Ok(out)
}

pub fn allowed(user: &UserHome, scope: &str, today: NaiveDate) -> Result<bool, AppError> {
    Ok(active(user, today)?.contains_key(scope))
}

pub fn require(user: &UserHome, name: &str, today: NaiveDate) -> Result<(), AppError> {
    if allowed(user, name, today)? {
        return Ok(());
    }
    let s = scope(name).ok_or_else(|| AppError::new("keyerror", format!("'{name}'")))?;
    Err(err(format!(
        "needs consent '{name}' ({} → {}). The owner grants it in a terminal: bin/cashcast consent grant {name}",
        s.shares, s.recipient
    )))
}

fn date_value(d: NaiveDate) -> toml::Value {
    toml::Value::Datetime(d.format("%Y-%m-%d").to_string().parse().expect("ISO date"))
}

/// Asks the owner on `tty` and records the grant. Fails when not confirmed.
pub fn grant(
    user: &UserHome,
    name: &str,
    tty: &mut dyn BufRead,
    out: &mut dyn Write,
    today: NaiveDate,
) -> Result<NaiveDate, AppError> {
    let Some(s) = scope(name) else {
        let all: Vec<&str> = SCOPES.iter().map(|s| s.name).collect();
        return Err(err(format!(
            "unknown scope '{name}'; scopes: {}",
            all.join(", ")
        )));
    };
    let expires = today + chrono::Duration::days(s.days);
    let _ = write!(
        out,
        "Consent for user '{}': {name}\n  Shares: {}\n  With: {}\n  Until: {expires}\nType the scope name to agree, or press Enter to cancel: ",
        user.name, s.shares, s.recipient
    );
    let _ = out.flush();
    let mut line = String::new();
    let _ = tty.read_line(&mut line);
    if line.trim() != name {
        events::log(
            user,
            "consent_declined",
            &[("scope", Json::Str(name.into()))],
        );
        return Err(err("not granted"));
    }
    record(user, name, today, "terminal")
}

/// Records a grant the owner already gave on a real terminal (`via` says how).
/// Callers check the terminal themselves: `grant` (the typed scope name) and
/// the telemetry ask (y/n on the start screen, `telemetry::ask`).
pub fn record(
    user: &UserHome,
    name: &str,
    today: NaiveDate,
    via: &str,
) -> Result<NaiveDate, AppError> {
    let s = scope(name).ok_or_else(|| err(format!("unknown scope '{name}'")))?;
    let expires = today + chrono::Duration::days(s.days);
    let mut grants: Vec<toml::Table> = read(user)?
        .into_iter()
        .filter(|g| g.get("scope").and_then(|v| v.as_str()) != Some(name))
        .collect();
    let mut g = toml::Table::new();
    g.insert("scope".into(), toml::Value::String(name.into()));
    g.insert("granted_on".into(), date_value(today));
    g.insert("expires_on".into(), date_value(expires));
    g.insert("via".into(), toml::Value::String(via.into()));
    grants.push(g);
    write(user, &grants)?;
    events::log(
        user,
        "consent_granted",
        &[("scope", Json::Str(name.into()))],
    );
    Ok(expires)
}

/// Removes a grant. `name` is None when the call named no scope.
pub fn revoke(user: &UserHome, name: Option<&str>) -> Result<bool, AppError> {
    let grants = read(user)?;
    let kept: Vec<toml::Table> = grants
        .iter()
        .filter(|g| {
            g.get("scope").and_then(|v| v.as_str()) != name
                || name.is_none() && g.get("scope").is_some()
        })
        .cloned()
        .collect();
    write(user, &kept)?;
    let fields = name
        .map(|n| vec![("scope", Json::Str(n.into()))])
        .unwrap_or_default();
    events::log(user, "consent_revoked", &fields);
    Ok(kept.len() != grants.len())
}

/// The controlling terminal. Agents' shells have none, so they cannot grant.
pub fn open_tty() -> Result<(std::io::BufReader<std::fs::File>, std::fs::File), AppError> {
    let open = || -> std::io::Result<_> {
        let r = std::fs::File::open("/dev/tty")?;
        let w = std::fs::OpenOptions::new().write(true).open("/dev/tty")?;
        Ok((std::io::BufReader::new(r), w))
    };
    open().map_err(|_| {
        err("consent needs a real terminal. Open Terminal, cd to this folder and run the same command \
             (in Claude Code you may also type: ! bin/cashcast consent grant SCOPE)")
    })
}

pub const AGENT_ENV: [&str; 9] = [
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CODEX_SANDBOX",
    "CODEX_MANAGED_BY_NPM",
    "CURSOR_AGENT",
    "CURSOR_TRACE_ID",
    "AIDER_MODEL",
    "GEMINI_CLI",
    "CASHCAST_AGENT",
];

/// True when output probably goes to an AI agent rather than a person.
pub fn agent_context() -> bool {
    use std::io::IsTerminal;
    // An action the owner started in the token-guarded web app (its output goes
    // to the owner's browser): the server passes its own access token.
    if std::env::var("CASHCAST_OWNER_UI").is_ok_and(|t| crate::tasks::start::owner_token_ok(&t)) {
        return false;
    }
    if std::env::var("CASHCAST_OWNER_RUN").is_ok_and(|t| owner_run_ok(&t)) {
        return false;
    }
    if AGENT_ENV
        .iter()
        .any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()))
    {
        return true;
    }
    !std::io::stdout().is_terminal()
}

/// The token file of one run the owner started in a terminal
/// (`bin/cashcast weekly`): it exists only while the run lasts, and the run
/// passes the token to its own steps as `CASHCAST_OWNER_RUN`, so their output,
/// captured for the saved page, is still the owner's. A run an agent starts
/// gets no token.
pub fn owner_run_file() -> std::path::PathBuf {
    super::paths::private_root()
        .join("out")
        .join("owner-run.token")
}

/// True when `given` is the token of the owner's run in progress.
pub fn owner_run_ok(given: &str) -> bool {
    let Ok(token) = std::fs::read_to_string(owner_run_file()) else {
        return false;
    };
    let token = token.trim();
    // Compare in constant time.
    !token.is_empty()
        && token.len() == given.len()
        && token
            .bytes()
            .zip(given.bytes())
            .fold(0u8, |d, (a, b)| d | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(dir: &std::path::Path) -> UserHome {
        UserHome::new("alex", dir.to_path_buf())
    }

    #[test]
    fn grant_needs_the_typed_scope_and_expires() {
        let dir = std::env::temp_dir().join(format!("cc-consent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let u = user(&dir);
        let today = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let mut out = Vec::new();
        assert!(
            grant(
                &u,
                "agent:aggregates",
                &mut "no\n".as_bytes(),
                &mut out,
                today
            )
            .is_err()
        );
        assert!(!allowed(&u, "agent:aggregates", today).unwrap());
        let until = grant(
            &u,
            "agent:aggregates",
            &mut "agent:aggregates\n".as_bytes(),
            &mut out,
            today,
        )
        .unwrap();
        assert_eq!(until.to_string(), "2027-01-16");
        assert!(allowed(&u, "agent:aggregates", today).unwrap());
        assert!(!allowed(&u, "agent:aggregates", until + chrono::Duration::days(1)).unwrap());
        assert!(require(&u, "agent:strategy", today).is_err());
        assert!(revoke(&u, Some("agent:aggregates")).unwrap());
        assert!(!revoke(&u, Some("agent:aggregates")).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browser_read_covers_a_month_of_sunday_runs() {
        // Granted on a Sunday, it holds the next 5 Sunday runs (35 days), and not the 6th.
        let dir = std::env::temp_dir().join(format!("cc-consent-week-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let u = user(&dir);
        let sunday = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let mut out = Vec::new();
        let scope = "agent:browser-read";
        let until = grant(
            &u,
            scope,
            &mut format!("{scope}\n").as_bytes(),
            &mut out,
            sunday,
        )
        .unwrap();
        assert_eq!(until.to_string(), "2026-12-20");
        assert!(allowed(&u, scope, sunday + chrono::Duration::days(35)).unwrap());
        assert!(!allowed(&u, scope, sunday + chrono::Duration::days(42)).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
