//! Where things live: the repo, the private root and each user's folder.
//!
//! Folder layout (all under the private root, never tracked by git):
//!
//! ```text
//! private/users/<name>/      one folder per person (mode 700)
//!     profile.toml           province, goals, wallet: the person's answers
//!     cashcast.toml          forecast answers (the original cashcast file)
//!     consent.toml           what may leave the computer, granted at a terminal
//!     inbox/ data/ out/      exports, aggregates, outputs (agents never open)
//!     docs/                  the owner's notes and records (agents never open)
//!     logs/                  content-free event and error logs
//! ```
//!
//! The original single-user layout (`private/cashcast.toml` with
//! `private/inbox`) still works: it is the user named "default".

use std::path::{Component, Path, PathBuf};

pub const LEGACY: &str = "default";

/// The repo root: `$CASHCAST_ROOT` (set by `bin/cashcast`), else three
/// folders above the engine binary (`engine/target/release/`).
pub fn root() -> PathBuf {
    if let Some(r) = std::env::var_os("CASHCAST_ROOT") {
        return resolve_path(Path::new(&r));
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe
        .ancestors()
        .nth(4)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    resolve_path(&dir)
}

/// Python's `Path.resolve()`: absolute, with symlinks of the existing part resolved.
pub fn resolve_path(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut existing = abs.clone();
    let mut rest: Vec<PathBuf> = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(&existing) {
            let mut out = real;
            for part in rest.iter().rev() {
                out = normal_join(out, part);
            }
            return out;
        }
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(PathBuf::from(name));
                existing = parent.to_path_buf();
            }
            _ => return lexical(&abs),
        }
    }
}

fn normal_join(base: PathBuf, part: &Path) -> PathBuf {
    let mut out = base;
    for c in part.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn lexical(p: &Path) -> PathBuf {
    normal_join(PathBuf::from("/"), p)
}

/// `path.relative_to(ROOT)` when the path is inside the repo, else the path.
pub fn rel(path: &Path) -> String {
    let r = root();
    path.strip_prefix(&r)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

pub fn private_root() -> PathBuf {
    match std::env::var_os("CASHCAST_PRIVATE") {
        Some(p) => resolve_path(Path::new(&p)),
        None => resolve_path(&root().join("private")),
    }
}

pub fn users_dir() -> PathBuf {
    private_root().join("users")
}

/// `^[a-z0-9][a-z0-9-]{0,31}$`
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// A user cannot be resolved, or a call is wrong. The message says how to fix it.
#[derive(Debug, Clone, PartialEq)]
pub struct AppError {
    /// The Python exception class, lowercased for the error log.
    pub kind: &'static str,
    pub message: String,
}

impl AppError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        AppError {
            kind,
            message: message.into(),
        }
    }

    /// An I/O failure as Python raised it: `PermissionError` (exit 5 with
    /// the sandbox hint) for a denied path, else `OSError` (exit 1).
    pub fn io(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            AppError::new("permissionerror", "a file")
        } else {
            AppError::new("oserror", e.to_string())
        }
    }

    /// `io` for a helper that already turned the error into text.
    pub fn io_text(e: String) -> Self {
        if e.contains("(os error 13)") || e.contains("(os error 1)") {
            AppError::new("permissionerror", "a file")
        } else {
            AppError::new("oserror", e)
        }
    }
}

#[cfg(test)]
mod io_error_tests {
    use super::AppError;
    use std::io::{Error, ErrorKind};

    #[test]
    fn a_denied_path_is_a_permission_error() {
        assert_eq!(
            AppError::io(Error::from(ErrorKind::PermissionDenied)).kind,
            "permissionerror"
        );
        assert_eq!(
            AppError::io(Error::from(ErrorKind::NotFound)).kind,
            "oserror"
        );
        let denied = Error::from_raw_os_error(13).to_string();
        assert_eq!(
            AppError::io_text(format!("cannot write x: {denied}")).kind,
            "permissionerror"
        );
        assert_eq!(
            AppError::io_text("cannot write x: disk full".into()).kind,
            "oserror"
        );
    }
}

pub fn check_name(name: &str) -> Result<(), AppError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(AppError::new(
            "usererror",
            format!(
                "user names use a-z, 0-9 and '-', up to 32 characters; got {}",
                py_repr(name)
            ),
        ))
    }
}

/// Python's `repr()` of a short string.
pub fn py_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

/// Python's `str(OSError)` for a file: `[Errno 2] No such file or directory: 'x'`.
pub fn py_os_error(e: &std::io::Error, path: &str) -> String {
    let text = e.to_string();
    let text = text.split(" (os error").next().unwrap_or(&text).to_string();
    match e.raw_os_error() {
        Some(n) => format!("[Errno {n}] {text}: {}", py_repr(path)),
        None => format!("{text}: {}", py_repr(path)),
    }
}

/// The home folder of the login `name` (`pwd.getpwnam(name).pw_dir`), as
/// Python's `expanduser("~name")` reads it; None for an unknown login.
#[cfg(unix)]
pub fn home_of(name: &str) -> Option<String> {
    use std::ffi::{CStr, CString, c_char};
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    #[repr(C)]
    struct Passwd {
        name: *const c_char,
        passwd: *const c_char,
        uid: u32,
        gid: u32,
        change: i64,
        class: *const c_char,
        gecos: *const c_char,
        dir: *const c_char,
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd")))]
    #[repr(C)]
    struct Passwd {
        name: *const c_char,
        passwd: *const c_char,
        uid: u32,
        gid: u32,
        gecos: *const c_char,
        dir: *const c_char,
    }
    unsafe extern "C" {
        fn getpwnam(name: *const c_char) -> *const Passwd;
    }
    let c = CString::new(name).ok()?;
    // SAFETY: getpwnam returns null or a pointer to a static entry, read at once.
    unsafe {
        let p = getpwnam(c.as_ptr());
        if p.is_null() || (*p).dir.is_null() {
            return None;
        }
        Some(CStr::from_ptr((*p).dir).to_string_lossy().to_string())
    }
}

#[cfg(not(unix))]
pub fn home_of(_name: &str) -> Option<String> {
    None
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserHome {
    pub name: String,
    pub home: PathBuf,
}

impl UserHome {
    pub fn new(name: &str, home: PathBuf) -> Self {
        UserHome {
            name: name.to_string(),
            home,
        }
    }
    pub fn inbox(&self) -> PathBuf {
        self.home.join("inbox")
    }
    pub fn data(&self) -> PathBuf {
        self.home.join("data")
    }
    pub fn out(&self) -> PathBuf {
        self.home.join("out")
    }
    pub fn docs(&self) -> PathBuf {
        self.home.join("docs")
    }
    pub fn logs(&self) -> PathBuf {
        self.home.join("logs")
    }
    pub fn profile(&self) -> PathBuf {
        self.home.join("profile.toml")
    }
    pub fn config(&self) -> PathBuf {
        self.home.join("cashcast.toml")
    }
    pub fn consent(&self) -> PathBuf {
        self.home.join("consent.toml")
    }
}

pub fn list_users() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(users_dir())
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| valid_name(n))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    if private_root().join("cashcast.toml").exists() {
        names.insert(0, LEGACY.to_string());
    }
    names
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Finds the user's folder. Order: $CASHCAST_HOME (an explicit folder), then
/// --user or $CASHCAST_USER, then the only user, then the original folder.
pub fn resolve(user: Option<&str>) -> Result<UserHome, AppError> {
    let user = user.filter(|u| !u.is_empty());
    if let (Some(explicit), None) = (env("CASHCAST_HOME"), user) {
        return Ok(UserHome::new(LEGACY, resolve_path(Path::new(&explicit))));
    }
    if let Some(u) = user.map(str::to_string).or_else(|| env("CASHCAST_USER")) {
        if u == LEGACY {
            return Ok(UserHome::new(LEGACY, private_root()));
        }
        check_name(&u)?;
        return Ok(UserHome::new(&u, users_dir().join(&u)));
    }
    let names = list_users();
    match names.len() {
        1 => resolve(Some(&names[0])),
        0 => Ok(UserHome::new(LEGACY, private_root())),
        _ => Err(AppError::new(
            "usererror",
            format!(
                "several users exist ({}); pass --user NAME or set CASHCAST_USER",
                names.join(", ")
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_python_pattern() {
        assert!(valid_name("alex"));
        assert!(valid_name("a-1"));
        assert!(!valid_name("-a"));
        assert!(!valid_name("Alex"));
        assert!(!valid_name(&"a".repeat(33)));
        assert_eq!(py_repr("Bad"), "'Bad'");
    }
}
