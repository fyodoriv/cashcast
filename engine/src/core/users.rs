//! People who use this copy of the app. Canadian residents only.
//!
//! Each person gets a private folder (mode 700) with a `profile.toml`. The
//! profile holds what the strategy needs: province, income, registered-account
//! room, goals and the cards and bank accounts they hold.

use super::paths::{self, AppError, UserHome};
use std::path::Path;

pub const PROVINCES: [(&str, &str); 13] = [
    ("AB", "Alberta"),
    ("BC", "British Columbia"),
    ("MB", "Manitoba"),
    ("NB", "New Brunswick"),
    ("NL", "Newfoundland and Labrador"),
    ("NS", "Nova Scotia"),
    ("NT", "Northwest Territories"),
    ("NU", "Nunavut"),
    ("ON", "Ontario"),
    ("PE", "Prince Edward Island"),
    ("QC", "Quebec"),
    ("SK", "Saskatchewan"),
    ("YT", "Yukon"),
];

fn err(msg: impl Into<String>) -> AppError {
    AppError::new("profileerror", msg)
}

fn codes() -> String {
    PROVINCES
        .iter()
        .map(|(c, _)| *c)
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_province(code: &str) -> bool {
    PROVINCES.iter().any(|(c, _)| *c == code)
}

/// Python's `str.upper()` for the ASCII and Latin text a province holds.
fn upper(s: &str) -> String {
    s.to_uppercase()
}

/// Rejects a profile that is not for a Canadian resident.
pub fn check_canadian(profile: &toml::Table) -> Result<(), AppError> {
    let text = |k: &str| profile.get(k).map(py_text).unwrap_or_default();
    let country = upper(&text("country"));
    if country != "CA" {
        let shown = if country.is_empty() {
            "missing".to_string()
        } else {
            country
        };
        return Err(err(format!(
            "this app supports Canadian residents only; profile country is {shown} (set country = \"CA\")"
        )));
    }
    let province = upper(&text("province"));
    if !is_province(&province) {
        let shown = if province.is_empty() {
            "nothing".to_string()
        } else {
            province
        };
        return Err(err(format!(
            "province must be one of {}; got {shown}",
            codes()
        )));
    }
    Ok(())
}

fn py_text(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Boolean(true) => "True".into(),
        toml::Value::Boolean(false) => "False".into(),
        other => other.to_string(),
    }
}

pub fn load_profile(user: &UserHome) -> Result<toml::Table, AppError> {
    if !user.profile().exists() {
        let fix = if user.name == paths::LEGACY {
            "bin/cashcast users adopt --province XX".to_string()
        } else {
            format!("bin/cashcast users add {} --province XX", user.name)
        };
        return Err(err(format!(
            "no profile for user {}. Run: {fix}",
            paths::py_repr(&user.name)
        )));
    }
    let text = std::fs::read_to_string(user.profile()).map_err(|e| err(e.to_string()))?;
    let profile: toml::Table = text
        .parse()
        .map_err(|e| AppError::new("tomldecodeerror", format!("{e}")))?;
    check_canadian(&profile)?;
    Ok(profile)
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| io_err(path, e))
}

fn io_err(path: &Path, e: std::io::Error) -> AppError {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        AppError::new("permissionerror", path.display().to_string())
    } else {
        err(format!("{}: {e}", path.display()))
    }
}

/// Creates an owner-only folder. An existing owner-only folder is left as it is.
fn private_dir(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path)
            && meta.is_dir()
            && meta.permissions().mode() & 0o777 == 0o700
        {
            return Ok(());
        }
    }
    std::fs::create_dir_all(path).map_err(|e| io_err(path, e))?;
    chmod(path, 0o700)
}

pub fn create_folders(user: &UserHome) -> Result<(), AppError> {
    let root = paths::private_root();
    private_dir(&root)?;
    if user.home != root
        && let Some(parent) = user.home.parent()
    {
        private_dir(parent)?;
    }
    for d in [
        user.home.clone(),
        user.inbox(),
        user.data(),
        user.out(),
        user.logs(),
    ] {
        private_dir(&d)?;
    }
    if !user.config().exists() {
        let example = paths::root().join("forecast").join("cashcast.example.toml");
        std::fs::copy(&example, user.config()).map_err(|e| io_err(&example, e))?;
        chmod(&user.config(), 0o600)?;
    }
    Ok(())
}

pub fn profile_text(display: &str, province: &str) -> Result<String, AppError> {
    let example = paths::root().join("profile").join("profile.example.toml");
    let text = std::fs::read_to_string(&example).map_err(|e| io_err(&example, e))?;
    Ok(text
        .replace(
            "display_name = \"Alex\"",
            &format!("display_name = \"{display}\""),
        )
        .replace("province = \"ON\"", &format!("province = \"{province}\"")))
}

fn write_private(path: &Path, text: &str) -> Result<(), AppError> {
    crate::write_private(path, text).map_err(err)
}

pub fn add(name: &str, province: &str, display: Option<&str>) -> Result<UserHome, AppError> {
    paths::check_name(name)?;
    if name == paths::LEGACY {
        return Err(AppError::new(
            "usererror",
            "'default' is the original single-user folder; use: bin/cashcast users adopt --province XX",
        ));
    }
    let province = upper(province);
    if !is_province(&province) {
        return Err(err(format!(
            "province must be one of {}; got {province}",
            codes()
        )));
    }
    let user = UserHome::new(name, paths::users_dir().join(name));
    if user.profile().exists() {
        return Err(err(format!("user {} already exists", paths::py_repr(name))));
    }
    create_folders(&user)?;
    write_private(
        &user.profile(),
        &profile_text(display.unwrap_or(name), &province)?,
    )?;
    Ok(user)
}

/// Gives the original single-user folder a profile, without moving its data.
pub fn adopt(province: &str, display: Option<&str>) -> Result<UserHome, AppError> {
    let province = upper(province);
    if !is_province(&province) {
        return Err(err(format!(
            "province must be one of {}; got {province}",
            codes()
        )));
    }
    let user = paths::resolve(Some(paths::LEGACY))?;
    create_folders(&user)?;
    if !user.profile().exists() {
        write_private(
            &user.profile(),
            &profile_text(display.unwrap_or("me"), &province)?,
        )?;
    }
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_canadian_provinces() {
        let ok: toml::Table = "country = \"ca\"\nprovince = \"on\"".parse().unwrap();
        assert!(check_canadian(&ok).is_ok());
        let us: toml::Table = "country = \"US\"\nprovince = \"NY\"".parse().unwrap();
        assert!(
            check_canadian(&us)
                .unwrap_err()
                .message
                .contains("Canadian residents only")
        );
        let bad: toml::Table = "country = \"CA\"\nprovince = \"XX\"".parse().unwrap();
        assert!(
            check_canadian(&bad)
                .unwrap_err()
                .message
                .ends_with("got XX")
        );
    }
}
