//! Bank and card setting changes the strategy asks for, and the done store
//! (`data/changes-done.json`, user-folder scope, kept until `changes undo`),
//! as `src/cashcast/changes.py`.

use super::connectors;
use crate::core::events;
use crate::core::paths::{AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::strategy::Task;
use crate::strategy::situation::Unported;
use chrono::{Duration, NaiveDate};

const TICK_HOLDS: i64 = 30;

pub struct Change {
    pub task: Task,
    pub service: String,
    pub name: String,
    pub login_url: String,
    pub steps: String,
    pub allowed: bool,
}

fn path(user: &UserHome) -> std::path::PathBuf {
    user.data().join("changes-done.json")
}

/// Task id → the day it was marked done, or {"on", "due"} for a tick from Reminders or Notion.
pub fn done(user: &UserHome) -> Result<Vec<(String, Json)>, Unported> {
    let p = path(user);
    if !p.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&p).map_err(|e| Unported(e.to_string()))?;
    match pyjson::loads(&text) {
        Some(Json::Obj(items)) => Ok(items),
        _ => Err(Unported("changes-done.json is not an object".into())),
    }
}

fn day(v: Option<&Json>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&v?.py_str(), "%Y-%m-%d").ok()
}

/// A task done by hand stays done until undo; a tick holds until 30 days after
/// the later of its due date and the tick.
pub fn is_open(
    finished: &[(String, Json)],
    task: &Task,
    today: NaiveDate,
) -> Result<bool, Unported> {
    let Some((_, entry)) = finished.iter().find(|(k, _)| *k == task.id) else {
        return Ok(true);
    };
    if !matches!(entry, Json::Obj(_)) {
        return Ok(false);
    }
    let on = day(entry.get("on")).ok_or_else(|| Unported("bad tick date".into()))?;
    let anchor = match entry.get("due").filter(|d| d.truthy()) {
        Some(d) => on.max(day(Some(d)).ok_or_else(|| Unported("bad due date".into()))?),
        None => on,
    };
    Ok(task.due.unwrap_or(today) >= anchor + Duration::days(TICK_HOLDS))
}

fn save(user: &UserHome, items: &[(String, Json)]) -> Result<(), AppError> {
    let io = AppError::io_text;
    std::fs::create_dir_all(user.data()).map_err(|e| io(e.to_string()))?;
    let text = pyjson::dumps(&Json::Obj(items.to_vec()), Some(1), true);
    crate::write_private(&path(user), &text).map_err(io)
}

pub fn pending(user: &UserHome, tasks: &[Task], today: NaiveDate) -> Result<Vec<Change>, Unported> {
    let finished = done(user)?;
    let specs = connectors::load_all()?;
    let mut out = Vec::new();
    for t in tasks {
        if t.risk != "settings" || !is_open(&finished, t, today)? {
            continue;
        }
        let spec = specs
            .iter()
            .find(|(sid, _)| !t.service.is_empty() && *sid == t.service)
            .map(|(_, s)| s);
        let action = spec.and_then(|s| {
            crate::strategy::py::list(s, "actions")
                .iter()
                .find(|a| a.get("id").map(Json::py_str).as_deref() == Some(t.action.as_str()))
                .cloned()
        });
        let (Some(spec), Some(action)) = (spec, action) else {
            out.push(Change {
                task: t.clone(),
                service: String::new(),
                name: String::new(),
                login_url: String::new(),
                steps: String::new(),
                allowed: false,
            });
            continue;
        };
        out.push(Change {
            task: t.clone(),
            service: t.service.clone(),
            name: spec.get("name").map_or(String::new(), Json::py_str),
            login_url: spec
                .get("browser")
                .and_then(|b| b.get("login_url"))
                .map_or(String::new(), Json::py_str),
            steps: connectors::playbook_section(spec, &t.action)?,
            allowed: action.get("risk").map(Json::py_str).as_deref() != Some("money"),
        });
    }
    Ok(out)
}

pub fn render(changes: &[Change], detail: Option<&str>) -> Result<String, AppError> {
    let shown: Vec<&Change> = match detail {
        Some(id) => {
            let v: Vec<&Change> = changes.iter().filter(|c| c.task.id == id).collect();
            if v.is_empty() {
                return Err(AppError::new(
                    "runtimeerror",
                    format!(
                        "no open change {}; run: bin/cashcast changes list",
                        crate::core::paths::py_repr(id)
                    ),
                ));
            }
            v
        }
        None => changes.iter().collect(),
    };
    let mut lines = vec![
        format!("# Bank and card changes ({} open)", shown.len()),
        String::new(),
    ];
    if shown.is_empty() {
        lines.push(
            "Nothing to change. Money tasks stay in your reminders: you do those yourself.".into(),
        );
    }
    for (i, c) in shown.iter().enumerate() {
        let t = &c.task;
        lines.push(format!("{}. `{}`: {}", i + 1, t.id, t.title));
        if !t.notes.is_empty() {
            lines.push(format!("   - Why: {}", t.notes));
        }
        if !c.service.is_empty() {
            lines.push(format!(
                "   - Where: {}, action `{}` (risk: settings). Sign in at {}",
                c.name, t.action, c.login_url
            ));
            lines.push(format!(
                "   - Steps: bin/cashcast action {} {}",
                c.service, t.action
            ));
        } else {
            lines.push(
                "   - Where: no connector does this one. Do it in the issuer's app, or by phone."
                    .into(),
            );
        }
        lines.push(format!(
            "   - When done: bin/cashcast changes done {}",
            t.id
        ));
        if detail.is_some() && !c.steps.is_empty() {
            lines.push(String::new());
            lines.push(c.steps.clone());
        }
    }
    lines.push(String::new());
    lines.push("An agent may do these in your browser only after `bin/cashcast consent grant actions:browser`.".into());
    lines.push(
        "You sign in. The agent shows each exact change and clicks the final button only after your yes in chat.".into(),
    );
    Ok(lines.join("\n"))
}

/// Marks tasks done that were ticked in Reminders or Notion (`sync all`): task id → its due date.
pub fn record_done(
    user: &UserHome,
    dues: &[(String, Option<String>)],
    today: NaiveDate,
) -> Result<(), AppError> {
    let mut items = done(user).map_err(|e| AppError::new("crash", e.0))?;
    for (id, due) in dues {
        let entry = Json::Obj(vec![
            ("on".into(), Json::Str(today.to_string())),
            ("due".into(), due.clone().map_or(Json::Null, Json::Str)),
        ]);
        match items.iter_mut().find(|(k, _)| k == id) {
            Some(slot) => slot.1 = entry,
            None => items.push((id.clone(), entry)),
        }
    }
    save(user, &items)?;
    events::log(
        user,
        "change_done",
        &[
            ("count", Json::Int(dues.len() as i64)),
            ("via", Json::Str("sync".into())),
        ],
    );
    Ok(())
}

pub fn mark(
    user: &UserHome,
    task_id: &str,
    today: NaiveDate,
    undo: bool,
) -> Result<String, AppError> {
    let back = |e: Unported| AppError::new("crash", e.0);
    let mut items = done(user).map_err(back)?;
    if undo {
        let Some(i) = items.iter().position(|(k, _)| k == task_id) else {
            return Ok(format!("{task_id} was not marked done."));
        };
        items.remove(i);
        save(user, &items)?;
        events::log(user, "change_reopened", &[]);
        return Ok(format!("Reopened {task_id}."));
    }
    match items.iter_mut().find(|(k, _)| k == task_id) {
        Some(slot) => slot.1 = Json::Str(today.to_string()),
        None => items.push((task_id.to_string(), Json::Str(today.to_string()))),
    }
    save(user, &items)?;
    events::log(user, "change_done", &[]);
    Ok(format!(
        "Marked {task_id} done. It leaves the list and the reminders (undo: bin/cashcast changes undo {task_id})."
    ))
}
