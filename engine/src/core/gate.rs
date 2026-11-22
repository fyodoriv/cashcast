//! The output gate: decides whether a report may reach the reader.
//!
//! A person at a terminal always sees the report. When the reader is probably
//! an AI agent (no terminal, or an agent environment variable is set), the
//! report is shown only if the owner granted the matching consent scope.
//! Otherwise it is saved to the user's `out/` folder and the agent sees one
//! line that says where it is and how the owner can share it.

use super::paths::{self, AppError, UserHome};
use super::pyjson::Json;
use super::{consent, events};
use chrono::NaiveDate;

/// Prints `text` if allowed. Returns true when it was printed.
pub fn emit(
    user: &UserHome,
    text: &str,
    scope: &str,
    name: &str,
    today: NaiveDate,
) -> Result<bool, AppError> {
    let fields = [
        ("command", Json::Str(name.into())),
        ("scope", Json::Str(scope.into())),
    ];
    if !consent::agent_context() || consent::allowed(user, scope, today)? {
        print!(
            "{}",
            if text.ends_with('\n') {
                text.to_string()
            } else {
                format!("{text}\n")
            }
        );
        events::log(user, "report_shown", &fields);
        return Ok(true);
    }
    let out = user.out();
    std::fs::create_dir_all(&out).map_err(|e| AppError::new("oserror", e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o700));
    }
    let saved = out.join(format!("last-{name}.md"));
    crate::write_private(&saved, text).map_err(|e| AppError::new("oserror", e))?;
    events::log(user, "report_withheld", &fields);
    println!(
        "Report saved for the owner only: {}. It was not shown here because the reader looks like an AI agent \
         and consent '{scope}' is not granted.\nTo share reports with your agent, run in a terminal: \
         bin/cashcast consent grant {scope}",
        paths::rel(&saved)
    );
    Ok(false)
}
