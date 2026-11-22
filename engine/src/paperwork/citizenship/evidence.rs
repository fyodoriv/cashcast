//! The citizenship evidence page (ported from sidequest
//! `citizenship-evidence.ts`): one self-contained HTML file with every answer
//! of the draft, where it came from, when, and a screenshot of the source.
//! It holds private values, so it is only written under the user's paperwork
//! folder (mode 600) and opened on this Mac; never published. Its
//! Content-Security-Policy lets it load nothing from the network. Pure.

use super::answers::{Answers, parse_day, questions};
use super::draft::{Coverage, Row};
use serde_json::Value;

/// Each source in plain words.
pub const SOURCE_LABEL: &[(&str, &str)] = &[
    ("pr-card", "PR card photo (front and back)"),
    ("passport", "Passport data page photo"),
    ("id", "Driver's licence or health card photo"),
    ("marital", "Separation agreement"),
    ("language", "Language test result"),
    ("leases", "Leases and moving mail"),
    ("linkedin", "LinkedIn data export"),
    ("nomads", "nomads.com trips list"),
    ("cra", "CRA My Account, tax returns"),
    ("derived", "Worked out from your other answers"),
    ("owner", "You wrote it"),
];
const SHOWN: usize = 10; // table rows shown before "Show N more"

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}
/// "Tue, Mar 12, 2030", with the exact day in a tooltip.
fn day(s: &str) -> String {
    match parse_day(s) {
        Some(d) => format!(
            "<time datetime=\"{0}\" title=\"{0}\">{1}</time>",
            esc(s),
            esc(&d.format("%a, %b %-d, %Y").to_string())
        ),
        None => esc(s),
    }
}
fn words(k: &str) -> String {
    k.replace('_', " ")
}
fn is_web(u: &str) -> bool {
    let l = u.to_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

fn scalar(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "<span class=\"muted\">—</span>".into(),
        Some(Value::String(s)) if s.is_empty() => "<span class=\"muted\">—</span>".into(),
        Some(Value::Bool(b)) => (if *b { "Yes" } else { "No" }).into(),
        Some(Value::String(s)) if parse_day(s).is_some() => day(s),
        Some(Value::String(s)) => esc(s),
        Some(x @ (Value::Object(_) | Value::Array(_))) => value(x),
        Some(x) => esc(&x.to_string()),
    }
}

/// Any answer: a list of rows becomes a table, an object a list of fields, the rest text.
pub fn value(v: &Value) -> String {
    match v {
        Value::Array(a) if a.is_empty() => "<p class=\"muted\">None</p>".into(),
        Value::Array(a) if a.iter().all(|x| !x.is_object() && !x.is_array()) => format!(
            "<ul class=\"plain\">{}</ul>",
            a.iter()
                .map(|x| format!("<li>{}</li>", scalar(Some(x))))
                .collect::<String>()
        ),
        Value::Array(a) => {
            let mut cols: Vec<String> = vec![];
            for x in a {
                for k in x.as_object().into_iter().flat_map(|o| o.keys()) {
                    if !cols.contains(k) {
                        cols.push(k.clone());
                    }
                }
            }
            let row = |x: &Value| {
                format!(
                    "<tr>{}</tr>",
                    cols.iter()
                        .map(|c| format!("<td>{}</td>", scalar(x.get(c))))
                        .collect::<String>()
                )
            };
            let head = format!(
                "<thead><tr>{}</tr></thead>",
                cols.iter()
                    .map(|c| format!("<th>{}</th>", esc(&words(c))))
                    .collect::<String>()
            );
            let first: String = a.iter().take(SHOWN).map(row).collect();
            let rest = &a[a.len().min(SHOWN)..];
            let more = if rest.is_empty() {
                String::new()
            } else {
                format!(
                    "<details class=\"more\"><summary>Show {} more</summary><div class=\"tw\"><table>{head}<tbody>{}</tbody></table></div></details>",
                    rest.len(),
                    rest.iter().map(row).collect::<String>()
                )
            };
            format!("<div class=\"tw\"><table>{head}<tbody>{first}</tbody></table></div>{more}")
        }
        Value::Object(o) => format!(
            "<dl>{}</dl>",
            o.iter()
                .map(|(k, x)| format!("<dt>{}</dt><dd>{}</dd>", esc(&words(k)), scalar(Some(x))))
                .collect::<String>()
        ),
        x => format!("<p>{}</p>", scalar(Some(x))),
    }
}

pub struct EvidenceInput<'a> {
    pub answers: &'a Answers,
    pub rows: &'a [Row],
    pub coverage: &'a Coverage,
    /// Countries that need a police certificate, or None when trips or the sign day are missing.
    pub police: Option<Vec<(String, String)>>,
    /// Proof file name to a data: URI; a missing one shows as "image missing".
    pub images: &'a [(String, String)],
    pub today: &'a str,
}

fn state_word(s: &str) -> &'static str {
    match s {
        "done" => "Done",
        "fix" => "Fix",
        "open" => "Open",
        _ => "Confirm",
    }
}

pub fn evidence_page(x: &EvidenceInput) -> String {
    let a = x.answers;
    let empty = serde_json::Map::new();
    let proofs = a
        .get("_proofs")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let n_proofs: usize = proofs
        .values()
        .filter_map(Value::as_array)
        .map(Vec::len)
        .sum();
    let count = |s: &str| x.rows.iter().filter(|r| r.state == s).count();
    let card = |k: &str, q: &str| -> String {
        let Some(r) = x.rows.iter().find(|r| r.id == k) else {
            return String::new();
        };
        let src = if r.source.is_empty() {
            String::new()
        } else {
            SOURCE_LABEL
                .iter()
                .find(|(s, _)| *s == r.source)
                .map_or(r.source.clone(), |(_, l)| l.to_string())
        };
        let on = a
            .get("_collected")
            .and_then(|c| c.get(k))
            .and_then(Value::as_str);
        let list: &[Value] = proofs.get(k).and_then(Value::as_array).map_or(&[], |v| v);
        let mut meta: Vec<String> = vec![];
        if !src.is_empty() {
            meta.push(format!("From: <b>{}</b>", esc(&src)));
        }
        if let Some(on) = on {
            meta.push(format!("collected {}", day(on)));
        }
        if !list.is_empty() {
            meta.push(format!(
                "{} proof{}",
                list.len(),
                if list.len() == 1 { "" } else { "s" }
            ));
        }
        let meta = meta.join(" · ");
        let f = |p: &Value, k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let shots: String = list
            .iter()
            .map(|p| {
                let (image, source, url, note) =
                    (f(p, "image"), f(p, "source"), f(p, "url"), f(p, "note"));
                let img = x
                    .images
                    .iter()
                    .find(|(n, _)| !image.is_empty() && *n == image);
                let pic = match img {
                    Some((_, uri)) => format!(
                        "<img src=\"{}\" alt=\"{}\" loading=\"lazy\">",
                        esc(uri),
                        esc(&format!("Proof for {}: {source}", words(k)))
                    ),
                    None if !image.is_empty() => {
                        "<p class=\"muted\">Image missing from the proofs folder.</p>".into()
                    }
                    None => String::new(),
                };
                // Only a web address is a link; anything else (javascript:, file:) shows as text.
                let link = if url.is_empty() {
                    String::new()
                } else if is_web(&url) {
                    format!(
                        " · <a href=\"{0}\" rel=\"noreferrer noopener\">{0}</a>",
                        esc(&url)
                    )
                } else {
                    format!(" · {}", esc(&url))
                };
                let note = if note.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", esc(&note))
                };
                format!(
                    "<figure>{pic}<figcaption>{} · taken {}{link}{note}</figcaption></figure>",
                    esc(&source),
                    day(&f(p, "taken"))
                )
            })
            .collect();
        let val = a.get(k).filter(|v| !v.is_null());
        let proof_part = if !shots.is_empty() {
            format!(
                "<details class=\"proofs\"{}><summary>Proof ({})</summary>{shots}</details>",
                if list.len() <= 2 { " open" } else { "" },
                list.len()
            )
        } else if r.kind == "fact" && val.is_some() && r.source != "derived" {
            "<p class=\"muted\">No screenshot yet.</p>".into()
        } else {
            String::new()
        };
        format!(
            "<article class=\"q s-{st}\" id=\"q-{id}\">\n<header><h3>{q}</h3><span class=\"pill\">{w}</span></header>\n{v}\n{n}\n{m}\n{proof_part}\n</article>",
            st = r.state,
            id = esc(k),
            q = esc(q),
            w = state_word(r.state),
            v = val
                .map(|v| format!("<div class=\"val\">{}</div>", value(v)))
                .unwrap_or_default(),
            n = if r.note.is_empty() {
                String::new()
            } else {
                format!("<p class=\"note\">{}</p>", esc(&r.note))
            },
            m = if meta.is_empty() {
                String::new()
            } else {
                format!("<p class=\"meta\">{meta}</p>")
            },
        )
    };
    let todo: Vec<&Row> = x
        .rows
        .iter()
        .filter(|r| r.state == "fix" || r.state == "open")
        .collect();
    let police = match &x.police {
        None => "<p>Needs your trips and the sign day first.</p>".to_string(),
        Some(p) if p.is_empty() => "<p>None needed: no stay of 183 days or more in a row in one country in the 4 years before you sign.</p>".into(),
        Some(p) => format!(
            "<ul>{}</ul>",
            p.iter()
                .map(|(c, l)| format!(
                    "<li><b>{}</b>: needed. Your last long stay there ends {}. Ask for it early: it can take weeks.</li>",
                    esc(c),
                    day(l)
                ))
                .collect::<String>()
        ),
    };
    let next = if todo.is_empty() {
        String::new()
    } else {
        format!(
            "<section><h2>Next</h2><ul>{}</ul></section>",
            todo.iter()
                .map(|r| format!(
                    "<li><a href=\"#q-{0}\">{1}</a>: {2}</li>",
                    esc(&r.id),
                    esc(&words(&r.id)),
                    esc(&r.note)
                ))
                .collect::<String>()
        )
    };
    let cards: Vec<String> = questions().iter().map(|(k, q)| card(k, q)).collect();
    let c = x.coverage;
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'">
<meta name="referrer" content="no-referrer">
<title>Citizenship evidence</title>
<style>{STYLE}</style></head>
<body><main>
<header><h1>Citizenship evidence</h1>
<p class="muted">Every answer of your draft, where it came from, and a screenshot of the source. Private: this file stays on your Mac. Made {made}.</p></header>
<section class="stats" aria-label="Summary">
<div class="stat"><b>{filled}/{facts}</b><span>facts filled ({pct}%)</span></div>
<div class="stat"><b>{fix}</b><span>to fix</span></div>
<div class="stat"><b>{open}</b><span>still open</span></div>
<div class="stat"><b>{confirm}</b><span>to confirm</span></div>
<div class="stat"><b>{n_proofs}</b><span>screenshots</span></div>
</section>
<section><h2>Police certificates</h2>{police}</section>
{next}
<section><h2>Answers</h2><div class="list">{cards}</div></section>
<p class="foot">Made by <code>bin/cashcast paperwork citizenship report</code> from your answers file. Make it again after each collect.</p>
</main></body></html>
"#,
        made = day(x.today),
        filled = c.filled,
        facts = c.facts,
        pct = c.pct,
        fix = count("fix"),
        open = count("open"),
        confirm = count("confirm"),
        cards = cards.join("\n"),
    )
}

const STYLE: &str = r#"
:root { --bg:#f6f6f2; --surface:#fff; --fg:#1c2320; --muted:#57615b; --line:#dde2dd;
  --accent:#a3231b; --ok:#1b6440; --fix:#a3231b; --open:#365687; --confirm:#7a5200; color-scheme: light; }
@media (prefers-color-scheme: dark) { :root { --bg:#121614; --surface:#1a1f1c; --fg:#e7ebe8;
  --muted:#a4aea8; --line:#2d3430; --accent:#ff8f86; --ok:#72d3a1; --fix:#ff8f86; --open:#9cbaea;
  --confirm:#efc062; color-scheme: dark; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.55 system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width: 52rem; margin: 0 auto; padding-block: 28px 64px; padding-inline: max(16px, env(safe-area-inset-left, 0px)); display: grid; gap: 24px; }
h1 { font-size: 1.9rem; line-height: 1.2; margin: 0; text-wrap: balance; }
h2 { font-size: 1.3rem; margin: 0 0 8px; }
h3 { font-size: 1.02rem; margin: 0; font-weight: 600; text-wrap: balance; }
p { margin: 0 0 8px; max-width: 70ch; }
a { color: var(--accent); overflow-wrap: anywhere; }
.muted { color: var(--muted); }
.stats { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 12px; }
.stat { background: var(--surface); border: 1px solid var(--line); border-radius: 8px; padding: 12px 14px; }
.stat b { display: block; font-size: 1.6rem; font-variant-numeric: tabular-nums; }
.stat span { color: var(--muted); font-size: .9rem; }
.q { background: var(--surface); border: 1px solid var(--line); border-left: 4px solid var(--c, var(--line)); border-radius: 8px; padding: 14px 16px; display: grid; gap: 8px; min-width: 0; }
.q > header { display: flex; gap: 12px; justify-content: space-between; align-items: start; }
.s-done { --c: var(--ok); } .s-fix { --c: var(--fix); } .s-open { --c: var(--open); } .s-confirm { --c: var(--confirm); }
.pill { color: var(--c); border: 1px solid currentColor; border-radius: 99px; padding: 0 10px; font-size: .85rem; font-weight: 600; white-space: nowrap; }
.list { display: grid; gap: 12px; }
.val { min-width: 0; }
.note { color: var(--fix); }
.s-open .note, .s-confirm .note { color: var(--muted); }
.meta { color: var(--muted); font-size: .92rem; margin: 0; }
.tw { overflow-x: auto; border: 1px solid var(--line); border-radius: 6px; }
table { border-collapse: collapse; width: 100%; font-size: .95rem; }
th, td { text-align: left; padding: 6px 10px; border-bottom: 1px solid var(--line); vertical-align: top; }
th { font-size: .82rem; text-transform: uppercase; letter-spacing: .05em; color: var(--muted); }
tr:last-child td { border-bottom: 0; }
dl { display: grid; grid-template-columns: max-content 1fr; gap: 4px 14px; margin: 0; }
dt { color: var(--muted); } dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
ul { margin: 0; padding-left: 1.2rem; } ul.plain { padding-left: 1.2rem; }
summary { cursor: pointer; min-height: 44px; display: flex; align-items: center; color: var(--accent); font-weight: 600; }
figure { margin: 8px 0 0; }
figure img { display: block; max-width: 100%; height: auto; border: 1px solid var(--line); border-radius: 6px; }
figcaption { color: var(--muted); font-size: .9rem; margin-top: 4px; }
.foot { color: var(--muted); font-size: .9rem; border-top: 1px solid var(--line); padding-top: 12px; }
"#;
