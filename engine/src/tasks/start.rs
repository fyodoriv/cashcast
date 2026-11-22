//! `cashcast` with no arguments: start the web app, refresh, and say what
//! matters now, as `src/cashcast/start.py`. This starts the engine's own web
//! server (`cashcast-engine cli web`) in the background. Its process id, port
//! and link live in `private/out/web.json` (private-root scope, mode 600).

use super::changes;
use crate::core::paths::{self, AppError, UserHome};
use crate::core::pyjson::{self, Json};
use crate::core::{consent, events, gate, settings, telemetry, users};
use crate::strategy::plan;
use chrono::{Duration, NaiveDate};
use std::path::PathBuf;
use std::time::Instant;

fn real_today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn state_dir() -> Result<PathBuf, String> {
    let d = paths::private_root().join("out");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    Ok(d)
}

fn alive(pid: i64, port: i64) -> bool {
    let signal = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let Ok(port) = u16::try_from(port) else {
        return false;
    };
    signal
        && std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_secs(1),
        )
        .is_ok()
}

fn int(st: &Json, k: &str) -> Option<i64> {
    match st.get(k)? {
        Json::Int(i) => Some(*i),
        Json::Str(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `start.write_state`: the running server's pid, port and link (mode 600).
pub fn write_state(port: u16, url: &str) -> Result<(), String> {
    let p = state_dir()?.join("web.json");
    let st = Json::Obj(vec![
        ("pid".into(), Json::Int(std::process::id() as i64)),
        ("port".into(), Json::Int(port as i64)),
        ("url".into(), Json::Str(url.into())),
    ]);
    crate::write_private(&p, &pyjson::dumps(&st, None, false))
}

/// `start.clear_state`: removes the state file if this process wrote it.
pub fn clear_state() {
    let Ok(dir) = state_dir() else { return };
    let p = dir.join("web.json");
    if let Some(st) = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| pyjson::loads(&t))
        && int(&st, "pid") == Some(std::process::id() as i64)
    {
        let _ = std::fs::remove_file(p);
    }
}

/// True when `given` is the running web app's access token (from
/// `private/out/web.json`, which agents never read). The owner's web app
/// passes it as `CASHCAST_OWNER_UI` to show what only the owner may see.
pub fn owner_token_ok(given: &str) -> bool {
    let p = paths::private_root().join("out").join("web.json");
    let Some(url) = std::fs::read_to_string(p)
        .ok()
        .and_then(|t| pyjson::loads(&t))
        .and_then(|st| st.get("url").map(|u| u.py_str()))
    else {
        return false;
    };
    let Some((_, token)) = url.split_once("#t=") else {
        return false;
    };
    // Compare in constant time, as hmac.compare_digest does.
    !token.is_empty()
        && token.len() == given.len()
        && token
            .bytes()
            .zip(given.bytes())
            .fold(0u8, |d, (a, b)| d | (a ^ b))
            == 0
}

/// The running web app's state, or None (a stale file is removed).
fn running() -> Result<Option<Json>, String> {
    let p = state_dir()?.join("web.json");
    if !p.exists() {
        return Ok(None);
    }
    if let Some(st) = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| pyjson::loads(&t))
        && let (Some(pid), Some(port)) = (int(&st, "pid"), int(&st, "port"))
        && alive(pid, port)
    {
        return Ok(Some(st));
    }
    let _ = std::fs::remove_file(&p);
    Ok(None)
}

/// `start.stop`: ends the running web app's process group; false when none runs.
pub fn stop() -> Result<bool, String> {
    let Some(st) = running()? else {
        return Ok(false);
    };
    unsafe extern "C" {
        fn getpgid(pid: i32) -> i32;
        fn killpg(pgrp: i32, sig: i32) -> i32;
    }
    let pid = int(&st, "pid").unwrap_or(0) as i32;
    if pid <= 1 {
        return Ok(false);
    }
    // SAFETY: plain syscalls on a pid read from our own state file.
    let pgid = unsafe { getpgid(pid) };
    // Never signal this command's own process group.
    if pgid == unsafe { getpgid(0) } {
        return Ok(false);
    }
    if pgid < 0 || unsafe { killpg(pgid, 15) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let _ = std::fs::remove_file(state_dir()?.join("web.json"));
    Ok(true)
}

fn spawn(port: i64) -> Result<Option<Json>, String> {
    use std::os::unix::process::CommandExt;
    let dir = state_dir()?;
    let log = dir.join("web.log"); // errors only: the server never prints its link to a file
    let out = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(exe)
        .args(["cli", "web", "--port", &port.to_string()])
        .current_dir(paths::root())
        .stdout(out)
        .stderr(err)
        .stdin(std::process::Stdio::null())
        .process_group(0) // keeps running after this command ends
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    let end = Instant::now() + std::time::Duration::from_secs(20);
    while Instant::now() < end {
        if let Some(st) = std::fs::read_to_string(dir.join("web.json"))
            .ok()
            .and_then(|t| pyjson::loads(&t))
            && int(&st, "pid") == Some(child.id() as i64)
        {
            return Ok(Some(st));
        }
        if child.try_wait().ok().flatten().is_some() {
            return Ok(None); // the port was taken, most likely
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = child.kill();
    Ok(None)
}

/// (state, reused): starts the web app unless one is already running.
fn launch() -> Result<(Json, bool), String> {
    if let Some(st) = running()? {
        return Ok((st, true));
    }
    std::fs::create_dir_all(paths::private_root()).map_err(|e| e.to_string())?;
    let port = match settings::get("web.port").map_err(|e| e.message)? {
        settings::Value::Int(p) => p,
        other => other.py_str().parse().unwrap_or(0),
    };
    let st = match spawn(port)? {
        Some(st) => Some(st),
        None if port != 0 => spawn(0)?,
        None => None,
    };
    st.map(|s| (s, false))
        .ok_or_else(|| "the web app did not start; run `bin/cashcast web` to see why".to_string())
}

fn web_line(st: &Json, reused: bool) -> String {
    let verb = if reused { "already running" } else { "started" };
    let text = |k: &str| st.get(k).map_or(String::new(), Json::py_str);
    if consent::agent_context() {
        return format!(
            "Web app: {verb} on this computer, port {}. Its link holds an access token, so it prints \
             only in a terminal: run `bin/cashcast` there to open it.",
            text("port")
        );
    }
    format!("Web app: {verb}. Open {}", text("url"))
}

/// The import step: this engine run as `import --home DIR`, output kept back.
fn refresh(user: &UserHome) -> String {
    let exe = std::env::current_exe();
    match exe.and_then(|exe| {
        std::process::Command::new(exe)
            .args(["import", "--home"])
            .arg(&user.home)
            .output()
    }) {
        Ok(o) if o.status.success() => "Exports: read.".into(),
        Ok(_) => "Exports: some files could not be read. Run: bin/cashcast import".into(),
        Err(e) => format!("Exports: not read ({e}). Run: bin/cashcast doctor"),
    }
}

/// None when the start screen must go back to Python before any output.
pub fn run(user_name: Option<&str>, today: NaiveDate) -> Result<i32, AppError> {
    let real = real_today();
    let names = paths::list_users();
    let user = paths::resolve(user_name);
    let send_due = match &user {
        Ok(u) if !names.is_empty() => telemetry::start_would_send(u, today, real)?,
        _ => false,
    };
    println!("cashcast: your private money assistant. Everything below stays on this computer.\n");
    if settings::get("web.autostart")?.truthy() {
        match launch() {
            Ok((st, reused)) => println!("{}", web_line(&st, reused)),
            Err(e) => println!("Web app: not started ({e})."),
        }
    } else {
        println!(
            "Web app: autostart is off (bin/cashcast config set web.autostart true). Run: bin/cashcast web"
        );
    }
    if names.is_empty() {
        println!(
            "\nNeeded from you: create your profile. Run: bin/cashcast users add NAME --province XX"
        );
        return Ok(0);
    }
    let user = match user {
        Ok(u) => u,
        Err(e) => {
            println!(
                "\nNeeded from you: pick a person ({}). Run: bin/cashcast --user NAME   ({})",
                names.join(", "),
                e.message
            );
            return Ok(0);
        }
    };
    users::create_folders(&user)?;
    if settings::get("start.refresh")?.truthy() {
        println!("{}", refresh(&user));
    }
    if let Some(line) = crate::live_facts::auto(today) {
        println!("{line}");
    }
    if let Err(e) = users::load_profile(&user) {
        println!("\nNeeded from you: fix your profile ({}).", e.message);
        return Ok(0);
    }
    // Output has started, so a section that cannot run here is reported, not handed to Python.
    let (ctx, log) = match plan::run(&user, today) {
        Ok(r) => r,
        Err(e) => {
            println!("\nNeeded from you: fix your profile ({}).", e.0);
            return Ok(0);
        }
    };
    let back = |e: crate::strategy::situation::Unported| AppError::io_text(e.0);
    plan::finish(&user, &ctx, &log).map_err(back)?;
    let gate_day = real;
    if settings::get("start.net_total")?.truthy()
        && let Some(sec) = ctx.result("net_total")
        && let crate::strategy::Data::NetTotal(data) = &sec.data
        && let Some(headline) = data.get("headline").filter(|h| h.truthy())
    {
        // The main number, first. Its trend and options: bin/cashcast net-total.
        gate::emit(
            &user,
            &format!(
                "\n{}. Higher is better; options: bin/cashcast net-total",
                headline.py_str()
            ),
            "agent:strategy",
            "start-net-total",
            gate_day,
        )?;
    }
    let finished = changes::done(&user).map_err(back)?;
    let mut tasks = Vec::new();
    for t in plan::tasks(&ctx).map_err(back)? {
        if changes::is_open(&finished, &t, today).map_err(back)? {
            tasks.push(t);
        }
    }
    tasks.sort_by(|a, b| {
        (a.due.is_none(), a.due.unwrap_or(today), &a.id).cmp(&(
            b.due.is_none(),
            b.due.unwrap_or(today),
            &b.id,
        ))
    });
    let shown = match settings::get("start.tasks")? {
        settings::Value::Int(n) => n.max(0) as usize,
        other => other.py_str().parse().unwrap_or(0),
    };
    if shown > 0 {
        let name = ctx
            .sit
            .profile
            .get("display_name")
            .map_or(user.name.clone(), Json::py_str);
        let mut lines = vec![format!("\nComing up for {name}:")];
        lines.extend(tasks.iter().take(shown).map(|t| {
            format!(
                "- {}: {}{}",
                t.due.map_or("when you can".to_string(), |d| d.to_string()),
                t.title,
                if t.risk == "money" {
                    " (you do this)"
                } else {
                    ""
                }
            )
        }));
        gate::emit(
            &user,
            &lines.join("\n"),
            "agent:strategy",
            "start",
            gate_day,
        )?;
    }
    let now = ctx.sit.today;
    let soon = now + Duration::days(7);
    let overdue = tasks
        .iter()
        .filter(|t| t.due.is_some_and(|d| d < now))
        .count();
    let money_soon = tasks
        .iter()
        .filter(|t| t.risk == "money" && t.due.is_some_and(|d| d <= soon))
        .count();
    let open_changes = changes::pending(&user, &tasks, today).map_err(back)?.len();
    let mut needs = Vec::new();
    let inbox_empty = std::fs::read_dir(user.inbox()).map_or(true, |mut d| d.next().is_none());
    if inbox_empty && !user.data().join("summary.json").exists() {
        needs.push(format!(
            "put your bank and card exports in {} (how: bin/cashcast connectors list)",
            paths::rel(&user.inbox())
        ));
    }
    if money_soon > 0 {
        needs.push(format!(
            "{money_soon} money task(s) due within a week: you do these (bin/cashcast reminders sync)"
        ));
    }
    if open_changes > 0 {
        needs.push(format!(
            "{open_changes} bank or card setting change(s) to approve (bin/cashcast changes)"
        ));
    }
    if consent::agent_context() && !consent::allowed(&user, "agent:strategy", real)? {
        needs.push(
            "to let your agent read the plan, run in a terminal: bin/cashcast consent grant agent:strategy".into(),
        );
    }
    println!(
        "\nNeeded from you: {}",
        if needs.is_empty() {
            "nothing right now.".to_string()
        } else {
            needs.join("; ")
        }
    );
    let next = if overdue > 0 {
        "do the overdue tasks first (bin/cashcast review)"
    } else if open_changes > 0 {
        "go through the changes (bin/cashcast changes)"
    } else {
        "open the web app's Plan ahead view"
    };
    println!("Next: {next}. Every action: bin/cashcast actions");
    events::log(
        &user,
        "start",
        &[
            ("overdue", Json::Int(overdue as i64)),
            ("changes", Json::Int(open_changes as i64)),
        ],
    );
    // The one-time y/n ask: a person at a real terminal only, never an agent or CI.
    let terminal = !crate::core::consent::agent_context() && {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal()
    };
    if matches!(telemetry::ask_blocked(&user, real, terminal), Ok(None))
        && let Ok((mut tty, mut out)) = crate::core::consent::open_tty()
    {
        let _ = telemetry::ask(&user, &mut tty, &mut out, real);
    }
    if let Some(notice) = telemetry::notice_once(&user, real) {
        println!("{notice}");
    }
    // At most one send a day, only after the notice; a failure never fails the start.
    if send_due {
        telemetry::send(&user, today);
    }
    Ok(0)
}
