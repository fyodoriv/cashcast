//! `fetch plan` and `fetch collect`, as `src/cashcast/fetch.py`: which
//! services need a fresh export (from `data/summary.json` aggregates only),
//! and moving new downloads into the inbox unopened.

use super::connectors::{download_action, route};
use crate::core::pyjson::{self, Json};
use crate::strategy::py;
use crate::strategy::situation::Unported;
use chrono::NaiveDate;
use std::path::Path;

const EXTENSIONS: [&str; 5] = [".csv", ".ofx", ".qfx", ".xlsx", ".pdf"];
const PARTIAL: [&str; 4] = [".crdownload", ".download", ".part", ".partial"];

pub struct Need {
    pub service: String,
    pub name: String,
    pub newest: Option<String>,
    pub days_old: Option<i64>,
    /// The day the newest file was saved into the inbox, when after `newest`.
    pub downloaded: Option<String>,
    pub stale: bool,
    /// The days this service's export may age (`fetch.stale_days`, or
    /// `fetch.crypto_stale_days` for a crypto-only service).
    pub limit: i64,
    pub route: &'static str,
    pub login_url: String,
    pub action: String,
}

/// A service that holds only crypto (kinds `crypto` or `fx`: Kraken, Casa).
/// A bank that also holds crypto (Wealthsimple) is not one.
pub fn crypto_only(spec: &Json) -> bool {
    let kinds = py::list(spec, "kinds");
    !kinds.is_empty()
        && kinds
            .iter()
            .all(|k| matches!(k.py_str().as_str(), "crypto" | "fx"))
}

/// Export formats that only crypto-only services read: `moneywiz sync`
/// gives their accounts the crypto age limit.
pub fn crypto_formats(specs: &[(String, Json)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut other: Vec<String> = Vec::new();
    for (_, s) in specs {
        let crypto = crypto_only(s);
        for f in py::list(py::obj(s, "import"), "formats") {
            let f = f.py_str();
            let list = if crypto { &mut out } else { &mut other };
            if !list.contains(&f) {
                list.push(f);
            }
        }
    }
    out.retain(|f| !other.contains(f));
    out
}

/// Format label → service, for labels only one service uses.
fn owners(specs: &[(String, Json)]) -> Vec<(String, String)> {
    let mut seen: Vec<(String, Vec<String>)> = Vec::new();
    for (sid, s) in specs {
        for f in py::list(py::obj(s, "import"), "formats") {
            let f = f.py_str();
            match seen.iter_mut().find(|(k, _)| *k == f) {
                Some((_, v)) => {
                    if !v.contains(sid) {
                        v.push(sid.clone())
                    }
                }
                None => seen.push((f, vec![sid.clone()])),
            }
        }
    }
    seen.into_iter()
        .filter(|(_, v)| v.len() == 1)
        .map(|(f, v)| (f, v[0].clone()))
        .collect()
}

fn files(home: &Path) -> Result<Vec<Json>, Unported> {
    let p = home.join("data").join("summary.json");
    if !p.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&p).map_err(|e| Unported(e.to_string()))?;
    let data = pyjson::loads(&text).ok_or_else(|| Unported("bad summary.json".into()))?;
    Ok(py::list(&data, "files").to_vec())
}

pub fn unnamed(home: &Path, specs: &[(String, Json)]) -> Result<usize, Unported> {
    let own = owners(specs);
    Ok(files(home)?
        .iter()
        .filter(|f| {
            let fmt = f.get("format").map(Json::py_str);
            !own.iter().any(|(k, _)| Some(k) == fmt.as_ref()) && by_institution(f, specs).is_none()
        })
        .count())
}

/// The service of a shared-format file: its OFX `<ORG>` is in `import.ofx_orgs`,
/// or its name starts with one of `import.file_prefixes` (the bank's default download name).
fn by_institution(f: &Json, specs: &[(String, Json)]) -> Option<String> {
    let org = py::list(f, "notes")
        .iter()
        .find_map(|n| n.py_str().strip_prefix("institution: ").map(str::to_string));
    let name = f
        .get("name")
        .map_or(String::new(), Json::py_str)
        .to_lowercase();
    let lists = |s: &Json, key: &str| -> Vec<String> {
        py::list(py::obj(s, "import"), key)
            .iter()
            .map(Json::py_str)
            .collect()
    };
    specs
        .iter()
        .find(|(_, s)| {
            org.as_ref().is_some_and(|o| {
                lists(s, "ofx_orgs")
                    .iter()
                    .any(|x| x.eq_ignore_ascii_case(o))
            })
        })
        .or_else(|| {
            specs.iter().find(|(_, s)| {
                lists(s, "file_prefixes")
                    .iter()
                    .any(|p| name.starts_with(&p.to_lowercase()))
            })
        })
        .map(|(sid, _)| sid.clone())
}

/// A ledger file has no row dates in the summary; the day it was saved stands in.
fn ledger_saved(home: &Path, f: &Json) -> Option<String> {
    if !f.get("ledger").is_some_and(Json::truthy) {
        return None;
    }
    let name = f.get("name")?.py_str();
    let saved = std::fs::metadata(home.join("inbox").join(name))
        .ok()?
        .modified()
        .ok()?;
    Some(
        chrono::DateTime::<chrono::Local>::from(saved)
            .date_naive()
            .to_string(),
    )
}

pub fn plan(
    home: &Path,
    specs: &[(String, Json)],
    today: NaiveDate,
    everything: bool,
    limit: i64,
    crypto_limit: i64,
) -> Result<Vec<Need>, Unported> {
    let own = owners(specs);
    let mut newest: Vec<(String, String)> = Vec::new();
    let mut saved: Vec<(String, NaiveDate)> = Vec::new();
    for f in files(home)? {
        let fmt = f.get("format").map_or(String::new(), Json::py_str);
        let sid = own
            .iter()
            .find(|(k, _)| *k == fmt)
            .map(|(_, s)| s.clone())
            .or_else(|| by_institution(&f, specs));
        let last = f
            .get("date_max")
            .filter(|d| d.truthy())
            .map(Json::py_str)
            .or_else(|| ledger_saved(home, &f));
        let day = f
            .get("name")
            .and_then(|n| crate::inbox::saved_day(&home.join("inbox").join(n.py_str()), today));
        if let (Some(sid), Some(day)) = (&sid, day) {
            match saved.iter_mut().find(|(s, _)| s == sid) {
                Some(slot) if day > slot.1 => slot.1 = day,
                Some(_) => {}
                None => saved.push((sid.clone(), day)),
            }
        }
        if let (Some(sid), Some(last)) = (sid, last) {
            match newest.iter_mut().find(|(s, _)| *s == sid) {
                Some(slot) if last > slot.1 => slot.1 = last,
                Some(_) => {}
                None => newest.push((sid, last)),
            }
        }
    }
    let mut out = Vec::new();
    for (sid, spec) in specs {
        let last = newest
            .iter()
            .find(|(s, _)| s == sid)
            .map(|(_, d)| d.clone());
        if py::list(py::obj(spec, "import"), "formats").is_empty()
            || (last.is_none() && !everything)
        {
            continue;
        }
        let age = match &last {
            Some(d) => Some(
                (today
                    - NaiveDate::parse_from_str(d, "%Y-%m-%d")
                        .map_err(|e| Unported(e.to_string()))?)
                .num_days(),
            ),
            None => None,
        };
        let limit = if crypto_only(spec) {
            crypto_limit
        } else {
            limit
        };
        // A file downloaded lately counts as fresh even when its newest row
        // is old: the account had no activity since.
        let downloaded = saved
            .iter()
            .find(|(s, _)| s == sid)
            .map(|(_, d)| *d)
            .filter(|d| last.as_ref().is_some_and(|l| d.to_string() > *l));
        let fresh_download = downloaded.is_some_and(|d| (today - d).num_days() <= limit);
        out.push(Need {
            service: sid.clone(),
            name: spec.get("name").map_or(String::new(), Json::py_str),
            newest: last,
            days_old: age,
            downloaded: downloaded.map(|d| d.to_string()),
            stale: age.is_none_or(|a| a > limit) && !fresh_download,
            limit,
            route: route(sid, spec),
            login_url: py::obj(spec, "browser")
                .get("login_url")
                .map_or(String::new(), Json::py_str),
            action: download_action(spec),
        });
    }
    out.sort_by(|a, b| (!a.stale, &a.service).cmp(&(!b.stale, &b.service)));
    Ok(out)
}

pub fn render_plan(needs: &[Need], limit: i64, shared: usize) -> String {
    let note = if shared > 0 {
        format!(
            "{shared} inbox file(s) use a format several services share (OFX, generic CSV), so they name no service."
        )
    } else {
        String::new()
    };
    if needs.is_empty() {
        let head = if note.is_empty() {
            "No exports in the inbox yet.".to_string()
        } else {
            note
        };
        return format!("{head} Run `bin/cashcast fetch plan --all` to see every service.");
    }
    let mut lines = vec![
        format!("# Exports to refresh (stale after {limit} days)"),
        String::new(),
    ];
    for n in needs {
        let when = match (&n.newest, n.days_old) {
            (Some(d), Some(a)) => format!("newest {d} ({a} days old)"),
            _ => "no export yet".into(),
        };
        let when = match &n.downloaded {
            Some(s) => format!("{when}, downloaded {s}"),
            None => when,
        };
        let state = if n.stale { "stale" } else { "fresh" };
        let state = if n.limit == limit {
            state.to_string()
        } else {
            format!("{state} (crypto: stale after {} days)", n.limit)
        };
        let how = match n.route {
            "api" => format!("API: bin/cashcast api download {}", n.service),
            "browser" => format!("browser: {}, playbook action {}", n.login_url, n.action),
            _ => format!(
                "the person exports it (see bin/cashcast connectors show {})",
                n.service
            ),
        };
        lines.push(format!(
            "- {} ({}): {state}, {when}. {how}",
            n.service, n.name
        ));
    }
    let stale = needs.iter().filter(|n| n.stale).count();
    lines.push(String::new());
    lines.push(format!("{stale} of {} need a fresh export.", needs.len()));
    if !note.is_empty() {
        lines.push(note);
    }
    lines.push(
        "Pick one: export them yourself (steps: bin/cashcast connectors show SERVICE), \
         or approve and an agent fetches them (skill: fetch-exports)."
            .into(),
    );
    lines.join("\n")
}

/// Moves export files changed in the last `minutes` from `src` into the inbox, unopened: extension → count.
pub fn collect(inbox: &Path, src: &Path, minutes: i64) -> Result<Vec<(String, usize)>, String> {
    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs((minutes.max(0) * 60) as u64);
    std::fs::create_dir_all(inbox).map_err(|e| e.to_string())?;
    let mut moved: Vec<(String, usize)> = Vec::new();
    let mut entries: Vec<std::path::PathBuf> = match std::fs::read_dir(src) {
        Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
        Err(_) => Vec::new(),
    };
    entries.sort();
    for p in entries {
        let name = p
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().to_string());
        let ext = p.extension().map_or(String::new(), |e| {
            format!(".{}", e.to_string_lossy().to_lowercase())
        });
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        if !meta.is_file()
            || !EXTENSIONS.contains(&ext.as_str())
            || PARTIAL.iter().any(|x| name.ends_with(x))
            || meta.modified().is_ok_and(|m| m < cutoff)
        {
            continue;
        }
        // Only bank statements: other PDFs (tickets, receipts) stay put.
        // The engine reads the text locally and prints nothing of it.
        if ext == ".pdf" && !crate::pdf::sniff(&p) {
            continue;
        }
        let stem = p
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().to_string());
        let suffix = p
            .extension()
            .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
        let (mut dest, mut n) = (inbox.join(&name), 1);
        while dest.exists() {
            dest = inbox.join(format!("{stem}-{n}{suffix}"));
            n += 1;
        }
        if std::fs::rename(&p, &dest).is_err() {
            std::fs::copy(&p, &dest).map_err(|e| e.to_string())?;
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
        match moved.iter_mut().find(|(k, _)| *k == ext) {
            Some(slot) => slot.1 += 1,
            None => moved.push((ext, 1)),
        }
    }
    moved.sort();
    Ok(moved)
}

pub fn render_collect(moved: &[(String, usize)], minutes: i64) -> String {
    if moved.is_empty() {
        return format!("No new export files in Downloads from the last {minutes} minutes.");
    }
    let total: usize = moved.iter().map(|(_, n)| n).sum();
    let kinds: Vec<String> = moved.iter().map(|(e, n)| format!("{e} ×{n}")).collect();
    format!(
        "Moved {total} file(s) ({}) into your inbox, unopened. Next: bin/cashcast import",
        kinds.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_moves_exports_only_and_keeps_names_apart() {
        let root = std::env::temp_dir().join(format!("cashcast-collect-{}", std::process::id()));
        let (src, inbox) = (root.join("dl"), root.join("inbox"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&inbox).unwrap();
        for name in ["a.csv", "b.OFX", "c.csv.crdownload", "d.txt"] {
            std::fs::write(src.join(name), "canary-not-read").unwrap();
        }
        std::fs::write(inbox.join("a.csv"), "older").unwrap();
        let moved = collect(&inbox, &src, 60).unwrap();
        assert_eq!(
            moved,
            vec![(".csv".to_string(), 1), (".ofx".to_string(), 1)]
        );
        assert!(inbox.join("a-1.csv").exists() && inbox.join("b.OFX").exists());
        assert!(src.join("c.csv.crdownload").exists() && src.join("d.txt").exists());
        assert_eq!(
            render_collect(&moved, 60),
            "Moved 2 file(s) (.csv ×1, .ofx ×1) into your inbox, unopened. Next: bin/cashcast import"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
