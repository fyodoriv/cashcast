//! Paperwork state: steps with statuses, what is missing and the next action,
//! plus the physical presence calculator. Ported from sidequest's
//! `paperwork.ts`.
//!
//! Private: the owner's state lives sealed in `<user dir>/paperwork/`
//! (folder 700, files 600, `.sqv`). Agents and pipes get `agent_view`, never
//! dates, notes, places or files. Facts are official and cited (`facts`).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::facts::{self, Item, Process, Source};
use super::vault;

pub const LOCK: &str = "state.lock";
pub const BREAKER: &str = "state.lock.break";
const STALE_DAYS: i64 = 90;

/// Personal fields the owner may set. No ID numbers: the owner types those
/// into the official form themself (data minimization, threat model).
pub const FIELDS: &[(&str, &str)] = &[
    (
        "pr_date",
        "the day you became a permanent resident (YYYY-MM-DD)",
    ),
    (
        "sign_date",
        "the day you plan to sign (YYYY-MM-DD; default today)",
    ),
    (
        "birth_date",
        "your date of birth (YYYY-MM-DD), for the 18-54 language and test rules",
    ),
    (
        "test_invited",
        "the day your test invitation came (YYYY-MM-DD)",
    ),
    ("method", "online or paper (default online)"),
];

pub fn field_help(key: &str) -> Option<&'static str> {
    FIELDS.iter().find(|(k, _)| *k == key).map(|(_, h)| *h)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Trip {
    pub left: String,
    pub returned: String,
    #[serde(default, rename = "where", skip_serializing_if = "Option::is_none")]
    pub where_: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Period {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileRef {
    pub id: String,
    pub item: String,
    pub name: String,
    pub bytes: u64,
    pub added: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub mark: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub at: String,
    /// The long trips a "not needed" police certificate was marked against.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub covers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessState {
    pub method: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub items: BTreeMap<String, Saved>,
    #[serde(default)]
    pub trips: Vec<Trip>,
    #[serde(default)]
    pub temp_periods: Vec<Period>,
    #[serde(default)]
    pub files: Vec<FileRef>,
}

impl Default for ProcessState {
    fn default() -> Self {
        ProcessState {
            method: "online".into(),
            fields: BTreeMap::new(),
            items: BTreeMap::new(),
            trips: vec![],
            temp_periods: vec![],
            files: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub processes: BTreeMap<String, ProcessState>,
}

impl Default for State {
    fn default() -> Self {
        State {
            version: 1,
            processes: BTreeMap::new(),
        }
    }
}

impl State {
    pub fn state_of(&mut self, id: &str) -> &mut ProcessState {
        self.processes.entry(id.to_string()).or_default()
    }
}

// ── dates ────────────────────────────────────────────────────────────────────
pub fn parse_day(s: &str) -> Option<NaiveDate> {
    let b = s.as_bytes();
    let shape = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shape {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn years_before(d: NaiveDate, y: u32) -> NaiveDate {
    d.checked_sub_months(Months::new(12 * y)).unwrap_or(d)
}

fn years_after(d: NaiveDate, y: u32) -> NaiveDate {
    d.checked_add_months(Months::new(12 * y)).unwrap_or(d)
}

fn dn(d: NaiveDate) -> i64 {
    d.num_days_from_ce() as i64
}

fn day_or(s: &str) -> Result<NaiveDate, String> {
    parse_day(s).ok_or_else(|| "dates must be YYYY-MM-DD".to_string())
}

/// "1,095", as IRCC writes it.
pub fn count(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

pub fn find(id: &str) -> Result<Process, String> {
    let all = facts::processes();
    let known: Vec<&str> = all.iter().map(|p| p.id).collect();
    let known = known.join(", ");
    if id.is_empty() {
        return Err(format!("no process ''; known: {known}"));
    }
    all.into_iter()
        .find(|p| p.id == id || p.id.contains(id))
        .ok_or_else(|| format!("no process '{id}'; known: {known}"))
}

// ── sealed state ─────────────────────────────────────────────────────────────
/// The paperwork folder, made with mode 700.
pub fn dir(user_dir: &Path) -> Result<PathBuf, String> {
    vault::private_dir(&user_dir.join("paperwork"))
}

fn state_file(user_dir: &Path) -> PathBuf {
    user_dir.join("paperwork").join("state.sqv")
}

fn files_dir(user_dir: &Path) -> Result<PathBuf, String> {
    vault::private_dir(&dir(user_dir)?.join("files"))
}

pub fn load(user_dir: &Path) -> Result<State, String> {
    match vault::read_sealed(&state_file(user_dir), "state")? {
        None => Ok(State::default()),
        Some(raw) => serde_json::from_slice(&raw)
            .map_err(|_| "the paperwork state does not parse; nothing was changed".to_string()),
    }
}

pub fn save(user_dir: &Path, s: &State) -> Result<(), String> {
    dir(user_dir)?;
    let body = serde_json::to_vec(s).map_err(|e| e.to_string())?;
    vault::write_sealed(&state_file(user_dir), &body, "state")
}

/// Load, change one process, save. One writer at a time across processes, so
/// two saves never lose a change. The key is read before the lock: a Keychain
/// prompt may wait minutes, and others wait on the lock.
pub fn update<T>(
    user_dir: &Path,
    id: &str,
    change: impl FnOnce(&mut ProcessState, &Process) -> Result<T, String>,
) -> Result<T, String> {
    let p = find(id)?;
    let root = dir(user_dir)?;
    let key = vault::key_for(&state_file(user_dir))?;
    vault::with_key(key, || {
        locked(&root, || {
            let mut s = load(user_dir)?;
            let out = change(s.state_of(p.id), &p)?;
            save(user_dir, &s)?;
            Ok(out)
        })
    })
}

// The lock is a file made with O_EXCL that holds the writer's pid and a
// nonce. A live lock is waited for, up to 10 seconds. A lock left by a crash
// is broken (break_lock), never just removed: a lock that is gone when we look
// was released, and the next one may already be someone else's.
fn locked<T>(root: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let lock = root.join(LOCK);
    let mut nonce = [0u8; 8];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let token = format!(
        "{} {}",
        std::process::id(),
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        match create_new(&lock) {
            Ok(mut file) => {
                file.write_all(token.as_bytes())
                    .map_err(|e| e.to_string())?;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("cannot make the save lock: {e}")),
        }
        if stale(&lock) {
            break_lock(root, &lock);
        } else if Instant::now() > until {
            return Err("another paperwork save is still running; try again (the doctor finds a lock left behind)".into());
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let out = f();
    // Only our own lock: one that is gone or holds another token is the next writer's.
    if std::fs::read_to_string(&lock).ok().as_deref() == Some(token.as_str()) {
        let _ = std::fs::remove_file(&lock);
    }
    out
}

fn create_new(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

fn age_of(path: &Path) -> Option<Duration> {
    let at = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(SystemTime::now().duration_since(at).unwrap_or_default())
}

/// Was the lock left by a writer that is gone? A live writer's lock is never
/// stale, however old: the Mac may have slept in the middle of a save. A lock
/// from before the Mac started is always stale: pids start over at boot.
fn stale(lock: &Path) -> bool {
    let (Ok(text), Some(age)) = (std::fs::read_to_string(lock), age_of(lock)) else {
        return false; // released meanwhile: try again
    };
    if let Some(boot) = booted_ago()
        && age > boot + Duration::from_secs(5)
    {
        return true;
    }
    match text
        .split_whitespace()
        .next()
        .and_then(|w| w.parse::<u32>().ok())
    {
        // No pid yet: still being written, or left empty by a crash (then it is old).
        None | Some(0) => age > Duration::from_secs(5),
        Some(pid) => !alive(pid),
    }
}

/// True unless the process is surely gone: a check that cannot run says alive.
fn alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    match std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(out) => out.status.success() || !out.stdout.is_empty(),
        Err(_) => true,
    }
}

/// How long ago the Mac started, or None when a sandbox hides it.
fn booted_ago() -> Option<Duration> {
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.boottime"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let sec: u64 = text
        .split("sec = ")
        .nth(1)?
        .split(',')
        .next()?
        .trim()
        .parse()
        .ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(Duration::from_secs(now.checked_sub(sec)?))
}

// Only the holder of a second lock may break a stale one, and it checks
// again first. So two waiters never both break it. A breaker lock left by a
// crash is taken over after a second.
fn break_lock(root: &Path, lock: &Path) {
    let breaker = root.join(BREAKER);
    if create_new(&breaker).is_err() {
        if age_of(&breaker).is_some_and(|a| a > Duration::from_secs(1)) {
            let _ = std::fs::remove_file(&breaker);
        }
        return;
    }
    if stale(lock) {
        let _ = std::fs::remove_file(lock);
    }
    let _ = std::fs::remove_file(&breaker);
}

// ── changes ──────────────────────────────────────────────────────────────────
fn all_items(p: &Process) -> impl Iterator<Item = &Item> {
    p.steps.iter().flat_map(|s| s.items.iter())
}

fn item_ids(p: &Process) -> String {
    all_items(p).map(|x| x.id).collect::<Vec<_>>().join(", ")
}

pub fn mark(
    ps: &mut ProcessState,
    p: &Process,
    item: &str,
    m: &str,
    note: Option<&str>,
    today: NaiveDate,
) -> Result<(), String> {
    if !all_items(p).any(|x| x.id == item) {
        return Err(format!("no item '{item}'; items: {}", item_ids(p)));
    }
    if !["todo", "done", "na"].contains(&m) {
        return Err("state must be todo, done or na".into());
    }
    let v = view(p, ps, today)?;
    let shown = v
        .steps
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|x| x.item.id == item);
    if shown.is_some_and(|x| x.item.auto == Some("presence")) {
        return Err(
            "the day count comes from your dates: enter your PR date and trips instead".into(),
        );
    }
    if m == "na"
        && let Some(x) = shown
        && !x.can_skip
    {
        return Err(format!(
            "\"{}\" applies to you, so it stays on your list",
            x.item.title
        ));
    }
    // A new mark keeps the owner's note unless a new note replaces it.
    let kept = note
        .map(str::to_string)
        .or_else(|| ps.items.get(item).and_then(|s| s.note.clone()));
    let covers = if m == "na" && shown.is_some_and(|x| x.item.auto == Some("police")) {
        let sign = sign_day(ps, today);
        police_trips(p, ps, sign)
            .iter()
            .map(|t| trip_key(t))
            .collect()
    } else {
        vec![]
    };
    ps.items.insert(
        item.to_string(),
        Saved {
            mark: m.to_string(),
            note: kept,
            at: iso(today),
            covers,
        },
    );
    Ok(())
}

pub fn set_field(ps: &mut ProcessState, key: &str, value: &str) -> Result<(), String> {
    if field_help(key).is_none() {
        let names: Vec<&str> = FIELDS.iter().map(|(k, _)| *k).collect();
        return Err(format!(
            "unknown field '{key}'; fields: {}",
            names.join(", ")
        ));
    }
    if key == "method" {
        if value != "online" && value != "paper" {
            return Err("method must be online or paper".into());
        }
        ps.method = value.to_string();
        return Ok(());
    }
    if value.is_empty() {
        ps.fields.remove(key);
        return Ok(());
    }
    let Some(d) = parse_day(value) else {
        return Err(format!("{key} must be a date YYYY-MM-DD"));
    };
    // A year typed in part (0020) is a valid date too, but no one means it.
    if !(1900..=2100).contains(&d.year()) {
        return Err(format!("{key} must be a year from 1900 to 2100"));
    }
    ps.fields.insert(key.to_string(), value.to_string());
    Ok(())
}

pub fn add_trip(
    ps: &mut ProcessState,
    left: &str,
    returned: &str,
    where_: &str,
) -> Result<Trip, String> {
    if parse_day(left).is_none() || parse_day(returned).is_none() {
        return Err("trip dates must be YYYY-MM-DD".into());
    }
    if returned < left {
        return Err("a trip must end on or after the day it starts".into());
    }
    let w = where_.trim();
    let trip = Trip {
        left: left.into(),
        returned: returned.into(),
        where_: (!w.is_empty()).then(|| w.to_string()),
    };
    ps.trips.push(trip.clone());
    ps.trips.sort_by(|a, b| a.left.cmp(&b.left));
    Ok(trip)
}

/// Remove one trip with these dates and place; false when none is left. Without
/// a place, the dates must name one trip.
pub fn remove_trip_on(
    ps: &mut ProcessState,
    left: &str,
    returned: &str,
    where_: Option<&str>,
) -> Result<bool, String> {
    let same: Vec<&Trip> = ps
        .trips
        .iter()
        .filter(|t| t.left == left && t.returned == returned)
        .collect();
    let places: std::collections::BTreeSet<&str> = same
        .iter()
        .map(|t| t.where_.as_deref().unwrap_or(""))
        .collect();
    if where_.is_none() && places.len() > 1 {
        let tail = if places.contains("") {
            " (- for the one with none)"
        } else {
            " too"
        };
        return Err(format!(
            "{} trips have these dates; run it again and type the country{tail}",
            same.len()
        ));
    }
    let i = ps.trips.iter().position(|t| {
        t.left == left
            && t.returned == returned
            && where_.is_none_or(|w| t.where_.as_deref().unwrap_or("") == w.trim())
    });
    if let Some(i) = i {
        ps.trips.remove(i);
    }
    Ok(i.is_some())
}

pub fn add_temp_period(ps: &mut ProcessState, from: &str, to: &str) -> Result<(), String> {
    day_or(from)?;
    day_or(to)?;
    if to < from {
        return Err("a period must end on or after it starts".into());
    }
    ps.temp_periods.push(Period {
        from: from.into(),
        to: to.into(),
    });
    ps.temp_periods.sort_by(|a, b| a.from.cmp(&b.from));
    Ok(())
}

pub fn remove_temp_on(ps: &mut ProcessState, from: &str, to: &str) -> bool {
    match ps
        .temp_periods
        .iter()
        .position(|t| t.from == from && t.to == to)
    {
        Some(i) => {
            ps.temp_periods.remove(i);
            true
        }
        None => false,
    }
}

// ── documents ────────────────────────────────────────────────────────────────
/// Store a sealed copy of a file for an item. The original stays where it is.
pub fn attach(
    user_dir: &Path,
    ps: &mut ProcessState,
    p: &Process,
    item: &str,
    path: &Path,
    today: NaiveDate,
) -> Result<FileRef, String> {
    if !all_items(p).any(|x| x.id == item) {
        return Err(format!("no item '{item}'"));
    }
    if !path.is_file() {
        return Err("no such file".into());
    }
    let mut raw = [0u8; 8];
    getrandom::getrandom(&mut raw).map_err(|e| e.to_string())?;
    let id: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let bytes = std::fs::read(path).map_err(|_| "cannot read the file".to_string())?;
    vault::write_sealed(
        &files_dir(user_dir)?.join(format!("{id}.sqv")),
        &bytes,
        &format!("file:{id}"),
    )?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(id.clone());
    let r = FileRef {
        id,
        item: item.into(),
        name,
        bytes: bytes.len() as u64,
        added: iso(today),
    };
    ps.files.push(r.clone());
    Ok(r)
}

/// Write a plain copy (mode 600) to a new path. Never overwrites.
pub fn export_file(
    user_dir: &Path,
    ps: &ProcessState,
    id: &str,
    out: &Path,
) -> Result<FileRef, String> {
    let r = ps
        .files
        .iter()
        .find(|f| f.id == id)
        .ok_or("no such file id; list them with `cashcast paperwork files`")?;
    if out.exists() {
        return Err("the output path exists; pick a new one".into());
    }
    let plain = vault::read_sealed(
        &files_dir(user_dir)?.join(format!("{id}.sqv")),
        &format!("file:{id}"),
    )?
    .ok_or("the encrypted copy is missing")?;
    let mut f = create_new(out).map_err(|e| format!("cannot write the copy: {e}"))?;
    f.write_all(&plain)
        .map_err(|e| format!("cannot write the copy: {e}"))?;
    Ok(r.clone())
}

// ── presence calculator ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Window {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Presence {
    pub sign: String,
    pub window: Window,
    pub pr_days: i64,
    pub temp_days: i64,
    pub temp_credit: i64,
    pub absent_days: i64,
    pub total: i64,
    pub needed: i64,
    pub met: bool,
    pub short_by: i64,
    /// First day you could sign, with the trips entered so far.
    pub earliest: Option<String>,
}

/// Full days outside Canada on a trip: the day you leave and the day you
/// return count as in.
pub fn absent_days(t: &Trip) -> i64 {
    match (parse_day(&t.left), parse_day(&t.returned)) {
        (Some(a), Some(b)) => ((b - a).num_days() - 1).max(0),
        _ => 0,
    }
}

fn sign_day(ps: &ProcessState, today: NaiveDate) -> NaiveDate {
    ps.fields
        .get("sign_date")
        .and_then(|s| parse_day(s))
        .unwrap_or(today)
}

struct Count {
    start: NaiveDate,
    end: NaiveDate,
    pr: i64,
    temp: i64,
    credit: i64,
    absent: i64,
    total: i64,
}

// Day counts for any sign day in [lo, hi]: prefix sums over day numbers, so
// the earliest sign day search stays fast.
struct Counter {
    lo: i64,
    sums: [Vec<i64>; 3],
    years: u32,
    factor: f64,
    cap: i64,
}

impl Counter {
    fn new(p: &Process, ps: &ProcessState, pr: NaiveDate, lo: i64, hi: i64) -> Counter {
        let n = (hi - lo + 1) as usize;
        let mut away = vec![false; n];
        let mut temp = vec![false; n];
        for t in &ps.trips {
            if let (Some(a), Some(b)) = (parse_day(&t.left), parse_day(&t.returned)) {
                for x in (dn(a) + 1).max(lo)..=(dn(b) - 1).min(hi) {
                    away[(x - lo) as usize] = true;
                }
            }
        }
        for t in &ps.temp_periods {
            if let (Some(a), Some(b)) = (parse_day(&t.from), parse_day(&t.to)) {
                for x in dn(a).max(lo)..=dn(b).min(hi) {
                    temp[(x - lo) as usize] = true;
                }
            }
        }
        let pr_day = dn(pr);
        let mut sums = [vec![0i64; n + 1], vec![0i64; n + 1], vec![0i64; n + 1]];
        for i in 0..n {
            let in_pr = lo + i as i64 >= pr_day;
            sums[0][i + 1] = sums[0][i] + (!away[i] && in_pr) as i64;
            sums[1][i + 1] = sums[1][i] + (!away[i] && !in_pr && temp[i]) as i64;
            sums[2][i + 1] = sums[2][i] + away[i] as i64;
        }
        Counter {
            lo,
            sums,
            years: p.rules.window_years,
            factor: p.rules.temp_factor,
            cap: p.rules.temp_cap,
        }
    }

    // The window is [sign - 5 years, sign - 1 day]: the day you sign does not count.
    fn at(&self, sign: NaiveDate) -> Count {
        let start = years_before(sign, self.years);
        let a = (dn(start) - self.lo) as usize;
        let b = (dn(sign) - self.lo) as usize;
        let span = |k: usize| self.sums[k][b] - self.sums[k][a];
        let pr = span(0);
        let temp = span(1);
        let credit = self.cap.min((temp as f64 * self.factor).floor() as i64);
        Count {
            start,
            end: sign.pred_opt().unwrap_or(sign),
            pr,
            temp,
            credit,
            absent: span(2),
            total: pr + credit,
        }
    }
}

pub fn presence(
    p: &Process,
    ps: &ProcessState,
    on: Option<&str>,
    today: NaiveDate,
) -> Result<Option<Presence>, String> {
    let on = match on {
        Some(s) => Some(parse_day(s).ok_or("the sign day must be a date YYYY-MM-DD")?),
        None => None,
    };
    let Some(pr) = ps.fields.get("pr_date").and_then(|s| parse_day(s)) else {
        return Ok(None);
    };
    let r = &p.rules;
    let sign = on.unwrap_or_else(|| sign_day(ps, today));
    let from = today.max(pr.succ_opt().unwrap_or(pr));
    let horizon = 366 * r.window_years as i64;
    let lo = dn(sign).min(dn(from)) - horizon - 2;
    let hi = dn(sign).max(dn(from) + horizon) + 2;
    let at = Counter::new(p, ps, pr, lo, hi);
    let c = at.at(sign);
    // Earliest sign day from today (or the day after PR), with the trips entered so far.
    let earliest = (0..=horizon)
        .map(|n| from + chrono::Days::new(n as u64))
        .find(|x| at.at(*x).total >= r.presence_days)
        .map(iso);
    Ok(Some(Presence {
        sign: iso(sign),
        window: Window {
            from: iso(c.start),
            to: iso(c.end),
        },
        pr_days: c.pr,
        temp_days: c.temp,
        temp_credit: c.credit,
        absent_days: c.absent,
        total: c.total,
        needed: r.presence_days,
        met: c.total >= r.presence_days,
        short_by: (r.presence_days - c.total).max(0),
        earliest,
    }))
}

/// Trips with 183+ days in a row abroad in the 4 years before signing, since
/// age 18: each may need a police certificate. Counts the day you left and the
/// day you came back, so a trip right at the line asks for one.
pub fn police_trips(p: &Process, ps: &ProcessState, sign: NaiveDate) -> Vec<Trip> {
    let back = years_before(sign, p.rules.police_years);
    let adult = ps
        .fields
        .get("birth_date")
        .and_then(|b| parse_day(b))
        .map(|b| years_after(b, p.rules.age_min));
    let from = adult.map_or(back, |a| a.max(back));
    ps.trips
        .iter()
        .filter(|t| {
            let (Some(l), Some(r)) = (parse_day(&t.left), parse_day(&t.returned)) else {
                return false;
            };
            let (a, b) = (l.max(from), r.min(sign));
            let days = if b < a { 0 } else { (b - a).num_days() + 1 };
            days >= p.rules.police_days
        })
        .cloned()
        .collect()
}

fn trip_key(t: &Trip) -> String {
    format!("{}/{}", t.left, t.returned)
}

// ── progress ─────────────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Applies {
    Yes(bool),
    Maybe(&'static str),
}

pub const MAYBE: Applies = Applies::Maybe("maybe");

#[derive(Clone, Debug, Serialize)]
pub struct ItemView {
    #[serde(flatten)]
    pub item: Item,
    /// done, todo or na.
    pub status: &'static str,
    pub applies: Applies,
    /// "Not needed" sticks: false when the rule applies now (it would come back).
    pub can_skip: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub files: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct StepView {
    pub id: &'static str,
    pub title: &'static str,
    /// done, now or later.
    pub status: &'static str,
    /// Items done; "not needed" ones count in neither done nor total.
    pub done: usize,
    pub total: usize,
    pub missing: Vec<&'static str>,
    pub items: Vec<ItemView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Deadline {
    pub what: &'static str,
    pub date: String,
    pub source: Source,
}

#[derive(Clone, Debug, Serialize)]
pub struct TripView {
    #[serde(flatten)]
    pub trip: Trip,
    pub absent: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct View {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub method: String,
    pub done: usize,
    pub total: usize,
    pub not_needed: usize,
    pub open: usize,
    pub next: Option<ItemView>,
    pub needs_you: Vec<ItemView>,
    pub steps: Vec<StepView>,
    pub presence: Option<Presence>,
    pub deadlines: Vec<Deadline>,
    pub fields: BTreeMap<String, String>,
    pub trips: Vec<TripView>,
    pub temp_periods: Vec<Period>,
    pub files: Vec<FileRef>,
    pub facts_checked: String,
    pub sources: Vec<Source>,
}

/// Whole years from birth to `on`.
fn age_on(birth: NaiveDate, on: NaiveDate) -> i64 {
    let mut y = (on.year() - birth.year()) as i64;
    if (on.month(), on.day()) < (birth.month(), birth.day()) {
        y -= 1;
    }
    y
}

fn plural(n: impl Into<i64>, word: &str) -> String {
    let n = n.into();
    format!("{} {word}{}", count(n), if n == 1 { "" } else { "s" })
}

pub fn view(p: &Process, ps: &ProcessState, today: NaiveDate) -> Result<View, String> {
    let r = &p.rules;
    let sign = sign_day(ps, today);
    let pres = presence(p, ps, None, today)?;
    let police = police_trips(p, ps, sign);
    let age = ps
        .fields
        .get("birth_date")
        .and_then(|b| parse_day(b))
        .map(|b| age_on(b, sign));
    let applies = |it: &Item| -> Applies {
        match it.when {
            Some("age-18-54") => match age {
                None => MAYBE,
                Some(a) => Applies::Yes(a >= r.age_min as i64 && a <= r.age_max as i64),
            },
            // Short trips never rule it out: a country you lived in before PR counts too.
            Some("police") if police.is_empty() => MAYBE,
            Some("if-applicable") => MAYBE,
            _ => Applies::Yes(true),
        }
    };
    // Items of the other way to apply are left out: they never apply.
    let fits = |it: &Item| match it.when {
        Some(w @ ("online" | "paper")) => w == ps.method,
        _ => true,
    };
    let mut steps: Vec<StepView> = p
        .steps
        .iter()
        .map(|st| {
            let items: Vec<ItemView> = st
                .items
                .iter()
                .filter(|it| fits(it))
                .map(|it| {
                    let saved = ps.items.get(it.id);
                    let a = applies(it);
                    // "Not needed" on a rule-based item comes back when the rule starts
                    // to apply. A police certificate comes back only for a long trip its
                    // "not needed" did not cover. A step with no rule is everyone's.
                    let reopened = saved.is_some_and(|s| {
                        s.mark == "na"
                            && (it.when.is_none()
                                || (a == Applies::Yes(true)
                                    && (it.auto != Some("police")
                                        || police
                                            .iter()
                                            .any(|t| !s.covers.contains(&trip_key(t))))))
                    });
                    let mut status: &'static str = if reopened {
                        "todo"
                    } else {
                        match saved.map(|s| s.mark.as_str()) {
                            Some("done") => "done",
                            Some("na") => "na",
                            Some(_) => "todo",
                            None if a == Applies::Yes(false) => "na",
                            None => "todo",
                        }
                    };
                    let mut why = if reopened {
                        Some(format!(
                            "you marked it not needed, but {}",
                            if it.when.is_some() {
                                "it applies now"
                            } else {
                                "everyone needs this step"
                            }
                        ))
                    } else if it.when == Some("age-18-54") && a == Applies::Yes(false) {
                        Some(format!(
                            "only ages {} to {} need this",
                            r.age_min, r.age_max
                        ))
                    } else {
                        None
                    };
                    // The day count comes from the dates only: a saved mark never changes it.
                    if it.auto == Some("presence") {
                        status = if pres.as_ref().is_some_and(|x| x.met) {
                            "done"
                        } else {
                            "todo"
                        };
                        why = Some(match &pres {
                            None => "enter the day you became a permanent resident".into(),
                            Some(x) if x.met => {
                                format!("{} days · {} needed", count(x.total), count(x.needed))
                            }
                            Some(x) => format!("{} short", plural(x.short_by, "day")),
                        });
                    }
                    if it.auto == Some("police")
                        && a == Applies::Yes(true)
                        && (saved.is_none() || reopened)
                    {
                        why = Some(format!(
                            "{} of {}+ days in the last {} years",
                            plural(police.len() as i64, "trip"),
                            r.police_days,
                            r.police_years
                        ));
                    }
                    let files = ps.files.iter().filter(|f| f.item == it.id).count();
                    // Only a rule can make a step not needed, and never the day count.
                    let can_skip = it.auto != Some("presence")
                        && (it.auto == Some("police")
                            || (it.when.is_some() && a != Applies::Yes(true)));
                    ItemView {
                        item: it.clone(),
                        status,
                        applies: a,
                        can_skip,
                        why,
                        note: saved.and_then(|s| s.note.clone()),
                        files,
                    }
                })
                .collect();
            let open: Vec<&'static str> = items
                .iter()
                .filter(|x| x.status == "todo")
                .map(|x| x.item.title)
                .collect();
            StepView {
                id: st.id,
                title: st.title,
                status: if open.is_empty() { "done" } else { "later" },
                done: items.iter().filter(|x| x.status == "done").count(),
                total: items.iter().filter(|x| x.status != "na").count(),
                missing: open,
                items,
            }
        })
        .collect();
    if let Some(s) = steps.iter_mut().find(|s| s.status != "done") {
        s.status = "now";
    }
    let open: Vec<ItemView> = steps
        .iter()
        .flat_map(|s| s.items.iter().filter(|x| x.status == "todo"))
        .cloned()
        .collect();
    let mut deadlines = vec![];
    if let Some(e) = pres.as_ref().and_then(|x| x.earliest.clone()) {
        deadlines.push(Deadline {
            what: "Earliest day to sign",
            date: e,
            source: r.sources[0],
        });
    }
    // The test window counts only while the test is still to do.
    let test = steps
        .iter()
        .flat_map(|s| s.items.iter())
        .find(|x| x.item.id == "test");
    if let (Some(inv), Some(t)) = (
        ps.fields.get("test_invited").and_then(|s| parse_day(s)),
        test,
    ) && t.status == "todo"
        && t.applies != Applies::Yes(false)
    {
        deadlines.push(Deadline {
            what: "Take the citizenship test by",
            date: iso(inv + chrono::Days::new(r.test_window_days as u64)),
            source: t.item.sources.get(1).copied().unwrap_or(r.sources[0]),
        });
    }
    let mut sources: Vec<Source> = vec![];
    for s in all_items(p).flat_map(|x| x.sources.iter()) {
        match sources.iter_mut().find(|x| x.url == s.url) {
            Some(x) => *x = *s, // the last one wins, as in a JS Map
            None => sources.push(*s),
        }
    }
    let facts_checked = sources
        .iter()
        .map(|s| s.checked)
        .min()
        .unwrap_or("")
        .to_string();
    Ok(View {
        id: p.id,
        title: p.title,
        summary: p.summary,
        method: ps.method.clone(),
        done: steps.iter().map(|s| s.done).sum(),
        total: steps.iter().map(|s| s.total).sum(),
        not_needed: steps.iter().map(|s| s.items.len() - s.total).sum(),
        open: open.len(),
        next: open.first().cloned(),
        needs_you: open.iter().take(3).cloned().collect(),
        steps,
        presence: pres,
        deadlines,
        fields: ps.fields.clone(),
        trips: ps
            .trips
            .iter()
            .map(|t| TripView {
                trip: t.clone(),
                absent: absent_days(t),
            })
            .collect(),
        temp_periods: ps.temp_periods.clone(),
        files: ps.files.clone(),
        facts_checked,
        sources,
    })
}

/// What an agent or a pipe may see: statuses, counts and the public checklist
/// text. No dates, notes, places, file names or field values (threat model T4).
pub fn agent_view(v: &View) -> Value {
    json!({
        "id": v.id,
        "title": v.title,
        "method": v.method,
        "done": v.done,
        "total": v.total,
        "next": v.next.as_ref().map(|n| json!({"id": n.item.id, "title": n.item.title, "owner_only": n.item.owner_only})),
        "steps": v.steps.iter().map(|s| json!({
            "id": s.id,
            "title": s.title,
            "status": s.status,
            "done": s.done,
            "total": s.total,
            "items": s.items.iter().map(|x| json!({
                "id": x.item.id,
                "title": x.item.title,
                "status": x.status,
                "applies": x.applies,
                "owner_only": x.item.owner_only,
                "files": x.files,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        // Whether you have enough days, never how many: with today's date, a
        // count gives away the PR date (threat model T4).
        "presence": v.presence.as_ref().map(|p| json!({"met": p.met, "needed": p.needed})),
        "trips": v.trips.len(),
        "fields_set": v.fields.keys().collect::<Vec<_>>(),
        "facts_checked": v.facts_checked,
        "private": "dates, notes, places and files stay on this Mac: see them with --reveal in a terminal",
    })
}

// ── facts: official sources, re-checked live ─────────────────────────────────
/// Plain text of an HTML page, for quote matching.
pub fn page_text(html: &str) -> String {
    let re_block = regex::Regex::new(r"(?is)<script\b.*?</script>|<style\b.*?</style>").unwrap();
    let re_tag = regex::Regex::new(r"<[^>]+>").unwrap();
    let re_space = regex::Regex::new(r"\s+").unwrap();
    let t = re_block.replace_all(html, " ");
    let t = re_tag.replace_all(&t, " ");
    let t = t
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&rsquo;", "'")
        .replace("&#8217;", "'");
    re_space.replace_all(&t, " ").into_owned()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SourceCheck {
    pub url: String,
    /// ok, changed, unreachable, manual, due or fresh.
    pub status: &'static str,
    pub missing: Vec<String>,
    pub checked: String,
    pub detail: String,
}

/// A fetched page: HTTP status and its text. Empty text means "not read" (a PDF).
pub type Fetch<'a> = &'a dyn Fn(&str) -> Result<(u16, String), String>;

/// The real fetcher: one plain GET of a public page through `core::http`. A
/// PDF is never read as a page: its raw bytes may hold a quote by chance.
/// Headers for the live check. No User-Agent of our own: canada.ca resets any
/// custom one (2026-11-22) and accepts curl's default.
pub fn page_headers() -> Vec<(String, String)> {
    vec![("Accept".to_string(), "text/html,*/*".to_string())]
}

pub fn fetch_page(url: &str) -> Result<(u16, String), String> {
    let r = crate::core::http::send("GET", url, &page_headers(), None, 20).map_err(|e| e.message)?;
    let pdf = url.to_ascii_lowercase().ends_with(".pdf") || r.body.starts_with(b"%PDF");
    Ok((r.status, if pdf { String::new() } else { r.text() }))
}

/// Each cited page: offline, whether its check is older than 90 days; live,
/// whether every quote is still on the page.
pub fn check_sources(p: &Process, live: bool, today: NaiveDate, fetch: Fetch) -> Vec<SourceCheck> {
    let mut by_url: Vec<(&str, Vec<Source>)> = vec![];
    for s in all_items(p)
        .flat_map(|x| x.sources.iter())
        .chain(p.rules.sources.iter())
    {
        match by_url.iter_mut().find(|(u, _)| *u == s.url) {
            Some((_, list)) => list.push(*s),
            None => by_url.push((s.url, vec![*s])),
        }
    }
    let mut out = vec![];
    for (url, list) in by_url {
        let checked = list
            .iter()
            .map(|s| s.checked)
            .min()
            .unwrap_or("")
            .to_string();
        let age = parse_day(&checked).map_or(0, |c| (today - c).num_days());
        let mut quotes: Vec<String> = vec![];
        for s in &list {
            if !quotes.iter().any(|q| q == s.quote) {
                quotes.push(s.quote.to_string());
            }
        }
        let row = |status, missing, detail| SourceCheck {
            url: url.to_string(),
            status,
            missing,
            checked: checked.clone(),
            detail,
        };
        if !live {
            let status = if age > STALE_DAYS { "due" } else { "fresh" };
            out.push(row(
                status,
                vec![],
                format!("checked {checked} ({age} days ago)"),
            ));
            continue;
        }
        match fetch(url) {
            Ok((200, text)) if text.trim().is_empty() => {
                // A PDF (or an empty page) cannot be searched here: it is not "ok".
                let detail = format!(
                    "reachable, but not read: open it and look for: {}",
                    quotes.join(" | ")
                );
                out.push(row("manual", quotes, detail));
            }
            Ok((200, text)) => {
                let body = page_text(&text).to_lowercase();
                let missing: Vec<String> = quotes
                    .iter()
                    .filter(|q| !body.contains(&q.to_lowercase()))
                    .cloned()
                    .collect();
                let detail = if missing.is_empty() {
                    format!("{} still there", plural(quotes.len() as i64, "fact"))
                } else {
                    format!("no longer says: {}", missing.join(" | "))
                };
                out.push(row(
                    if missing.is_empty() { "ok" } else { "changed" },
                    missing,
                    detail,
                ));
            }
            Ok((status, _)) => out.push(row(
                "unreachable",
                vec![],
                format!("HTTP {status}: open it in a browser and compare"),
            )),
            Err(e) => out.push(row(
                "unreachable",
                vec![],
                format!("{e}: open it in a browser and compare"),
            )),
        }
    }
    out
}

// ── doctor ───────────────────────────────────────────────────────────────────
pub type Finding = (&'static str, String);

/// Checks the paperwork folder without making or changing anything: modes,
/// links, plain files, leftovers, an old save lock and the key.
pub fn doctor(user_dir: &Path) -> Vec<Finding> {
    let mut out: Vec<Finding> = vec![];
    let root = user_dir.join("paperwork");
    let lmeta = |f: &Path| std::fs::symlink_metadata(f).ok();
    let is_link = |f: &Path| lmeta(f).is_some_and(|m| m.file_type().is_symlink());
    let is_dir = |f: &Path| lmeta(f).is_some_and(|m| m.is_dir());
    let mode = |f: &Path| -> u32 {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(f)
                .map(|m| m.permissions().mode() & 0o777)
                .unwrap_or(0)
        }
        #[cfg(not(unix))]
        {
            let _ = f;
            0o700
        }
    };
    if std::env::var_os(vault::TEST_KEY_ENV).is_some() && !vault::test_home() {
        out.push((
            "FAIL",
            format!(
                "{} is set for the real data folder: unset it",
                vault::TEST_KEY_ENV
            ),
        ));
    }
    // A file where the folder should be: every save would fail.
    let not_folder = user_dir.exists() && !user_dir.is_dir();
    if not_folder {
        out.push((
            "FAIL",
            format!("{} is not a folder (move it away)", user_dir.display()),
        ));
    }
    if !not_folder && is_link(&root) {
        out.push((
            "FAIL",
            format!(
                "the paperwork folder is a link: {} (move what it points to here, or delete it)",
                root.display()
            ),
        ));
    }
    if is_link(user_dir) && !user_dir.exists() {
        out.push((
            "FAIL",
            format!(
                "{} is a link to nothing (fix the link, or delete it)",
                user_dir.display()
            ),
        ));
    }
    if !not_folder && is_link(user_dir) && user_dir.exists() {
        out.push((
            "WARN",
            format!("the paperwork folder sits under a link: {} (it may sit anywhere, such as a synced folder)", user_dir.display()),
        ));
    }
    // Every folder and file inside, at any depth; a link is not followed.
    fn below(d: &Path, out: &mut Vec<Finding>, acc: &mut Vec<PathBuf>) {
        let names = match std::fs::read_dir(d) {
            Ok(r) => r,
            Err(e) => {
                out.push(("FAIL", format!("cannot read {}: {e}", d.display())));
                return;
            }
        };
        let mut paths: Vec<PathBuf> = names.flatten().map(|e| e.path()).collect();
        paths.sort();
        for f in paths {
            let dir = std::fs::symlink_metadata(&f).is_ok_and(|m| m.is_dir());
            acc.push(f.clone());
            if dir {
                below(&f, out, acc);
            }
        }
    }
    let real_root = !not_folder && is_dir(&root);
    let mut inside = vec![];
    if real_root {
        below(&root, &mut out, &mut inside);
    }
    let folders: Vec<PathBuf> = if real_root {
        std::iter::once(root.clone())
            .chain(inside.iter().filter(|f| is_dir(f)).cloned())
            .collect()
    } else {
        vec![]
    };
    for f in &folders {
        if *f != root && *f != root.join("files") {
            out.push((
                "FAIL",
                format!(
                    "unexpected folder in the paperwork folder: {} (move it out)",
                    f.display()
                ),
            ));
        }
        let m = mode(f);
        if m != 0o700 {
            out.push(("FAIL", format!("{} is mode {m:o}, want 700", f.display())));
        }
    }
    let lock = root.join(LOCK);
    if real_root
        && let Some(age) = age_of(&lock)
        && age > Duration::from_secs(60)
    {
        let minutes = (age.as_secs_f64() / 60.0).round() as i64;
        out.push((
            "WARN",
            format!(
                "a save lock is {} old: if no cashcast is saving, delete {}",
                plural(minutes, "minute"),
                lock.display()
            ),
        ));
    }
    let mut sealed = 0;
    for f in inside.iter().filter(|x| !is_dir(x)) {
        if *f == lock || *f == root.join(BREAKER) {
            continue;
        }
        // A file removed while we look (a save ends) is skipped.
        let Some(st) = lmeta(f) else { continue };
        if st.file_type().is_symlink() {
            out.push((
                "FAIL",
                format!("a link in the paperwork folder: {} (move what it points to here, or delete it)", f.display()),
            ));
            continue;
        }
        if !st.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // A hard link is a second name for the same file, maybe in a synced folder.
            if st.nlink() > 1 {
                out.push((
                    "FAIL",
                    format!("a hard link in the paperwork folder: {} has {} names (delete the other name)", f.display(), st.nlink()),
                ));
            }
        }
        let mut head = [0u8; 4];
        let got = match std::fs::File::open(f) {
            Ok(mut h) => std::io::Read::read(&mut h, &mut head).unwrap_or(0),
            Err(_) => continue,
        };
        let encrypted = got == 4 && &head == b"SQV1";
        let name = f.to_string_lossy();
        if encrypted && name.ends_with(".sqv") {
            sealed += 1;
        } else if encrypted {
            out.push((
                "WARN",
                format!(
                    "leftover from an interrupted save (encrypted): safe to delete {}",
                    f.display()
                ),
            ));
        } else if name.ends_with(".sqv") {
            out.push(("FAIL", format!("{} is not encrypted", f.display())));
        } else {
            out.push((
                "FAIL",
                format!("plain file in the paperwork folder: {}", f.display()),
            ));
        }
        let m = mode(f);
        if m != 0o600 {
            out.push(("FAIL", format!("{} is mode {m:o}, want 600", f.display())));
        }
    }
    let has_state = real_root && root.join("state.sqv").exists();
    match vault::key() {
        Ok(None) if has_state => out.push((
            "FAIL",
            "encrypted data exists but the Keychain key is missing".into(),
        )),
        Ok(k) => out.push((
            "ok",
            format!(
                "key: {}",
                if has_state || k.is_some() {
                    vault::where_()
                } else {
                    "made on the first save".into()
                }
            ),
        )),
        Err(e) => out.push(("FAIL", e)),
    }
    out.push((
        "ok",
        format!("{}, modes checked", plural(sealed, "encrypted file")),
    ));
    out
}
