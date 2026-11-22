//! A tiny HTTP layer over the system `curl`, so the engine needs no TLS crate
//! (port of `api/http.py`). The URL, headers and body go to curl on stdin as
//! a config file (`-K -`), never on the command line, so tokens never show in
//! `ps`. Tests install a fake transport with `set_transport`.
//!
//! Tokens come from the environment or the user's private `.env` (mode 600,
//! never tracked). They are never logged or printed.

use std::cell::RefCell;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::core::paths::{AppError, UserHome};

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// method, url, headers, body → response.
pub type Transport =
    Box<dyn Fn(&str, &str, &[(String, String)], Option<&[u8]>) -> Result<Response, AppError>>;

thread_local! {
    static FAKE: RefCell<Option<Transport>> = const { RefCell::new(None) };
}

/// Tests install a fake transport here; None restores curl.
pub fn set_transport(t: Option<Transport>) {
    FAKE.with(|f| *f.borrow_mut() = t);
}

pub fn api_error(msg: impl Into<String>) -> AppError {
    AppError::new("apierror", msg)
}

/// A curl config string: double quotes, with `\` and `"` escaped.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The curl config for one request. The body travels in a private temp file
/// named in the config, so binary bodies need no escaping.
pub fn curl_config(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body_file: Option<&str>,
) -> String {
    let mut cfg = vec![
        format!("url = {}", quoted(url)),
        format!("request = {}", quoted(method)),
    ];
    for (k, v) in headers {
        cfg.push(format!("header = {}", quoted(&format!("{k}: {v}"))));
    }
    if let Some(path) = body_file {
        cfg.push(format!("data-binary = {}", quoted(&format!("@{path}"))));
    }
    cfg.join("\n") + "\n"
}

/// Writes the body to a new file only we can read, under a random name, and
/// never through an existing file or link (`create_new`).
fn body_file(bytes: &[u8]) -> Result<std::path::PathBuf, AppError> {
    let mut rand = [0u8; 12];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut rand))
        .map_err(|e| api_error(format!("no random source: {e}")))?;
    let path = std::env::temp_dir().join(format!("cashcast-body-{}", crate::digest::hex(&rand)));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| api_error(format!("cannot write the request body: {e}")))?;
    Ok(path)
}

/// The HTTP status (last stderr line, from `--write-out`) and curl's own error text.
fn parse_stderr(err: &str) -> (u16, String) {
    let mut status = 0;
    let mut reason = Vec::new();
    for line in err.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()) {
            status = line.parse().unwrap_or(0);
        } else {
            reason.push(line.trim_start_matches("curl: ").to_string());
        }
    }
    (status, reason.join("; "))
}

fn curl(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    timeout: u32,
) -> Result<Response, AppError> {
    let tmp = body.map(body_file).transpose()?;
    let cfg = curl_config(method, url, headers, tmp.as_ref().and_then(|p| p.to_str()));
    let out = Command::new("curl")
        .args([
            "-sS",
            // HTTPS only, also after a redirect; cap a reply at 50 MB.
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            "50M",
            "--max-time",
            &timeout.to_string(),
            "-K",
            "-",
            "--write-out",
            "%{stderr}\n%{http_code}\n",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .take()
                .expect("piped stdin")
                .write_all(cfg.as_bytes())?;
            child.wait_with_output()
        });
    if let Some(path) = tmp {
        let _ = std::fs::remove_file(path);
    }
    let out = out.map_err(|e| api_error(format!("network error: {e}")))?;
    let (status, reason) = parse_stderr(&String::from_utf8_lossy(&out.stderr));
    if status == 0 {
        return Err(api_error(format!("network error: {reason}")));
    }
    Ok(Response {
        status,
        body: out.stdout,
    })
}

/// One request with a timeout in seconds, whatever its status.
pub fn send(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    timeout: u32,
) -> Result<Response, AppError> {
    match FAKE.with(|f| f.borrow().as_ref().map(|t| t(method, url, headers, body))) {
        Some(r) => r,
        None => curl(method, url, headers, body, timeout),
    }
}

pub fn request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<Response, AppError> {
    let r = send(method, url, headers, body, 30)?;
    if r.status >= 400 {
        let base = url.split('?').next().unwrap_or(url);
        return Err(api_error(format!(
            "{method} {base} failed with HTTP {}",
            r.status
        )));
    }
    Ok(r)
}

/// Reads a token from the environment, then from the user's private .env.
pub fn secret(user: &UserHome, name: &str) -> Result<String, AppError> {
    if let Ok(v) = std::env::var(name)
        && !v.is_empty()
    {
        return Ok(v);
    }
    let env = user.home.join(".env");
    if let Ok(text) = std::fs::read_to_string(&env) {
        for line in text.lines() {
            let (key, value) = line.split_once('=').unwrap_or((line, ""));
            if key.trim() == name && !value.trim().is_empty() {
                return Ok(value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string());
            }
        }
    }
    Err(api_error(format!(
        "missing {name}: add it to {} (one line: {name}=...) or export it in your shell",
        env.display()
    )))
}

/// Saves an API download into the user's inbox, unparsed. The engine reads it.
pub fn save_download(user: &UserHome, filename: &str, body: &[u8]) -> Result<String, AppError> {
    let inbox = user.inbox();
    std::fs::create_dir_all(&inbox).map_err(|e| AppError::new("oserror", e.to_string()))?;
    crate::write_private_bytes(&inbox.join(filename), body)
        .map_err(|e| AppError::new("oserror", e))?;
    Ok(filename.to_string())
}

/// `urllib.parse.quote(s, safe="~")`: every byte but unreserved ones percent-encoded.
pub fn quote(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `urllib.parse.urlencode`: `quote_plus` of each key and value.
pub fn urlencode(pairs: &[(String, String)]) -> String {
    let plus = |s: &str| quote(s).replace("%20", "+").replace("%7E", "~");
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", plus(k), plus(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `urllib.parse.parse_qsl`: `a=b&c=d` pairs, `+` and `%XX` decoded.
pub fn parse_qsl(query: &str) -> Vec<(String, String)> {
    let unq = |s: &str| {
        let bytes = s.replace('+', " ").into_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%'
                && i + 2 < bytes.len()
                && let Ok(b) = u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"),
                    16,
                )
            {
                out.push(b);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    };
    query
        .split('&')
        .filter_map(|part| part.split_once('=').filter(|(_, v)| !v.is_empty()))
        .map(|(k, v)| (unq(k), unq(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_quotes_values_and_never_puts_the_body_inline() {
        let cfg = curl_config(
            "POST",
            "https://x.example.com/a?b=1",
            &[("Authorization".into(), "Bearer t\"k".into())],
            Some("/tmp/body"),
        );
        assert_eq!(
            cfg,
            "url = \"https://x.example.com/a?b=1\"\nrequest = \"POST\"\nheader = \"Authorization: Bearer t\\\"k\"\ndata-binary = \"@/tmp/body\"\n"
        );
    }

    #[test]
    fn quoting_matches_python_urllib() {
        assert_eq!(
            quote("Hello Ladies + Gentlemen, a signed OAuth request!"),
            "Hello%20Ladies%20%2B%20Gentlemen%2C%20a%20signed%20OAuth%20request%21"
        );
        assert_eq!(urlencode(&[("a b".into(), "c~d/e".into())]), "a+b=c~d%2Fe");
        assert_eq!(
            parse_qsl("oauth_token=a%2Bb&x=&y=c+d"),
            vec![
                ("oauth_token".to_string(), "a+b".to_string()),
                ("y".to_string(), "c d".to_string())
            ]
        );
    }

    #[test]
    fn stderr_gives_the_status_and_curls_reason() {
        assert_eq!(parse_stderr("\n200\n"), (200, String::new()));
        assert_eq!(
            parse_stderr("\n000\ncurl: (6) Could not resolve host: x.example.com\n"),
            (0, "(6) Could not resolve host: x.example.com".to_string())
        );
    }

    #[test]
    fn body_files_get_new_random_names() {
        let a = body_file(b"x").unwrap();
        let b = body_file(b"x").unwrap();
        assert_ne!(a, b);
        let _ = (std::fs::remove_file(a), std::fs::remove_file(b));
    }

    #[test]
    fn errors_name_the_status_and_drop_the_query() {
        set_transport(Some(Box::new(|_, _, _, _| {
            Ok(Response {
                status: 401,
                body: vec![],
            })
        })));
        let e = request("GET", "https://x.example.com/v1/a?token=secret", &[], None)
            .err()
            .unwrap();
        set_transport(None);
        assert_eq!(
            e.message,
            "GET https://x.example.com/v1/a failed with HTTP 401"
        );
    }
}
