//! The connector specs in `connectors/*.toml`, as `src/cashcast/connectors.py`
//! (only what the task commands read until step 6).

use crate::core::pyjson::Json;
use crate::strategy::py;
use crate::strategy::situation::Unported;
use std::path::PathBuf;

pub fn dir() -> PathBuf {
    crate::core::paths::root().join("connectors")
}

/// Connector id → spec, sorted by file name.
pub fn load_all() -> Result<Vec<(String, Json)>, Unported> {
    let bad = |e: std::io::Error| Unported(e.to_string());
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir())
        .map_err(bad)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for p in files {
        let sid = p
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().to_string());
        let table: toml::Table = std::fs::read_to_string(&p)
            .map_err(bad)?
            .parse()
            .map_err(|e: toml::de::Error| Unported(e.to_string()))?;
        out.push((sid, py::from_toml(&toml::Value::Table(table))));
    }
    Ok(out)
}

/// The connector for an issuer or bank name ("TD" → td), matched on its id or name.
pub fn find_by_name(specs: &[(String, Json)], name: &str) -> Option<String> {
    let want = name.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    specs
        .iter()
        .find(|(sid, s)| {
            *sid == want || s.get("name").map(|n| n.py_str().to_lowercase()) == Some(want.clone())
        })
        .map(|(sid, _)| sid.clone())
}

/// The text under `### ACTION` in the connector's playbook, up to the next `##`/`###` heading.
pub fn playbook_section(spec: &Json, action_id: &str) -> Result<String, Unported> {
    let file = spec
        .get("browser")
        .and_then(|b| b.get("playbook"))
        .map_or(String::new(), Json::py_str);
    let text = std::fs::read_to_string(dir().join(file)).map_err(|e| Unported(e.to_string()))?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let head = format!("### {action_id}");
    let Some(start) = lines
        .iter()
        .position(|l| l.trim_end_matches(['\n', '\r']).trim_end() == head && l.starts_with(&head))
    else {
        return Ok(String::new());
    };
    let body: String = lines[start + 1..]
        .iter()
        .take_while(|l| !(l.starts_with("## ") || l.starts_with("### ")))
        .copied()
        .collect();
    Ok(body.trim().to_string())
}

const RISKS: [&str; 3] = ["read", "settings", "money"];
/// Connectors whose API client downloads exports (`api/services.py` DOWNLOADS).
const API_DOWNLOADS: [&str; 4] = ["wise", "kraken", "etrade", "google-sheets"];

fn text(v: &Json, k: &str) -> String {
    v.get(k).map_or(String::new(), Json::py_str)
}

fn actions(spec: &Json) -> &[Json] {
    py::list(spec, "actions")
}

pub fn render_list(specs: &[(String, Json)]) -> String {
    let mut lines = vec![
        "| Service | Kinds | Import formats | API | Actions (read/settings/money) |".to_string(),
        "|---|---|---|---|---|".to_string(),
    ];
    for (sid, s) in specs {
        let counts: Vec<String> = RISKS
            .iter()
            .map(|r| {
                actions(s)
                    .iter()
                    .filter(|a| text(a, "risk") == *r)
                    .count()
                    .to_string()
            })
            .collect();
        let api = py::obj(s, "api");
        let api = if api.get("available").is_some_and(Json::truthy) {
            text(api, "auth")
        } else {
            "no".into()
        };
        let imp = py::obj(s, "import");
        let formats: Vec<String> = py::list(imp, "formats").iter().map(Json::py_str).collect();
        let formats = if formats.is_empty() {
            let why = text(imp, "no_export_reason");
            format!("none ({})", why.split('.').next().unwrap_or(""))
        } else {
            formats.join(", ")
        };
        let kinds: Vec<String> = py::list(s, "kinds").iter().map(Json::py_str).collect();
        lines.push(format!(
            "| {} (`{sid}`) | {} | {formats} | {api} | {} |",
            text(s, "name"),
            kinds.join(", "),
            counts.join("/")
        ));
    }
    lines.join("\n")
}

pub fn render_show(spec: &Json) -> String {
    let imp = py::obj(spec, "import");
    let mut out = vec![
        format!("# {}", text(spec, "name")),
        String::new(),
        format!(
            "Website: {}. Playbook: connectors/{}.",
            text(spec, "website"),
            text(py::obj(spec, "browser"), "playbook")
        ),
        String::new(),
    ];
    if py::list(imp, "formats").is_empty() {
        out.push("## No export".into());
        out.push(text(imp, "no_export_reason"));
    } else {
        out.push("## Export your data".into());
        out.push(text(imp, "how_to_export").trim().to_string());
        out.push(String::new());
        out.push(format!(
            "History: {}.",
            imp.get("history_limit")
                .map_or("not stated".into(), Json::py_str)
        ));
    }
    let api = py::obj(spec, "api");
    let on = api.get("available").is_some_and(Json::truthy);
    out.push(String::new());
    out.push("## API".into());
    out.push(if on {
        format!("Yes ({}): {}", text(api, "auth"), text(api, "docs"))
    } else {
        "No".into()
    });
    out.push(String::new());
    out.push("## Actions".into());
    for a in actions(spec) {
        let via: Vec<String> = py::list(a, "via").iter().map(Json::py_str).collect();
        out.push(format!(
            "- `{}` ({}, via {}): {}",
            text(a, "id"),
            text(a, "risk"),
            via.join(", "),
            text(a, "title")
        ));
    }
    out.join("\n")
}

/// `connectors.get`: the spec, or the error Python raises.
pub fn get<'a>(specs: &'a [(String, Json)], service: Option<&str>) -> Result<&'a Json, String> {
    specs
        .iter()
        .find(|(sid, _)| Some(sid.as_str()) == service)
        .map(|(_, s)| s)
        .ok_or_else(|| {
            let known: Vec<&str> = specs.iter().map(|(sid, _)| sid.as_str()).collect();
            format!(
                "unknown service {}; known: {}",
                service.map_or("None".into(), crate::core::paths::py_repr),
                known.join(", ")
            )
        })
}

/// The read action that downloads, or "".
pub fn download_action(spec: &Json) -> String {
    actions(spec)
        .iter()
        .find(|a| text(a, "risk") == "read" && text(a, "id").starts_with("download"))
        .map_or(String::new(), |a| text(a, "id"))
}

pub fn route(sid: &str, spec: &Json) -> &'static str {
    if py::obj(spec, "api")
        .get("available")
        .is_some_and(Json::truthy)
        && API_DOWNLOADS.contains(&sid)
    {
        "api"
    } else if py::obj(spec, "browser")
        .get("login_url")
        .is_some_and(Json::truthy)
        && !download_action(spec).is_empty()
    {
        "browser"
    } else {
        "human"
    }
}
