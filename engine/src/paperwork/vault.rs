//! Encryption at rest for paperwork (ported from sidequest, decision 0016).
//!
//! AES-256-GCM with a 32-byte key in the macOS Keychain, never in a file.
//! The file format is the sidequest one, byte for byte (`SQV1`, nonce, tag,
//! body; AAD `sidequest-vault:v1:<purpose>`), so the owner's moved files open
//! unchanged. Threat model: `docs/paperwork-threat-model.md`.
//!
//! Lifecycle scope: the key lives for one call. `with_key` holds it in a
//! thread-local for one paperwork update and drops it when that returns.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAGIC: &[u8; 4] = b"SQV1";
pub const SERVICE: &str = "cashcast";
/// The sidequest Keychain item: read once and copied to `SERVICE` (the move).
pub const LEGACY_SERVICE: &str = "sidequest";
pub const ACCOUNT: &str = "paperwork-key";
pub const TEST_KEY_ENV: &str = "CASHCAST_TEST_VAULT_KEY";

fn aad(purpose: &str) -> Vec<u8> {
    format!("sidequest-vault:v1:{purpose}").into_bytes()
}

fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).expect("system random source");
    b
}

pub fn seal(key: &[u8; 32], plain: &[u8], purpose: &str) -> Vec<u8> {
    let nonce = random(12);
    let c = Aes256Gcm::new(key.into());
    let a = aad(purpose);
    let ct = c
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: &a })
        .expect("aes-gcm seal");
    // aes-gcm appends the tag; the file keeps it before the body.
    let (body, tag) = ct.split_at(ct.len() - 16);
    let mut out = Vec::with_capacity(32 + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    out
}

pub fn open(key: &[u8; 32], blob: &[u8], purpose: &str) -> Result<Vec<u8>, String> {
    if blob.len() < 32 || &blob[..4] != MAGIC {
        return Err("not a paperwork vault file".into());
    }
    let mut ct = blob[32..].to_vec();
    ct.extend_from_slice(&blob[16..32]);
    let a = aad(purpose);
    Aes256Gcm::new(key.into())
        .decrypt(Nonce::from_slice(&blob[4..16]), Payload { msg: &ct, aad: &a })
        .map_err(|_| "the vault file was changed or the key is wrong; nothing was read".into())
}

/// True when the private root is inside the system temp folder (a test home).
pub fn test_home() -> bool {
    let tmp = std::fs::canonicalize(std::env::temp_dir()).unwrap_or_default();
    let home = std::fs::canonicalize(crate::core::paths::private_root()).unwrap_or_default();
    home.starts_with(&tmp)
}

fn security_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("security"))
        .find(|p| p.exists())
}

/// Tests and test homes never reach the real Keychain: only a fake `security` first on PATH.
fn keychain_off() -> bool {
    if test_home() {
        return match security_on_path() {
            None => true,
            Some(b) => std::fs::canonicalize(&b).unwrap_or(b) == Path::new("/usr/bin/security"),
        };
    }
    !cfg!(target_os = "macos")
}

/// True when the test key opens the vault: nothing real can be read then.
pub fn test_key_in_use() -> bool {
    std::env::var_os(TEST_KEY_ENV).is_some() && test_home()
}

fn parse_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
        return None;
    }
    let mut k = [0u8; 32];
    for (i, k) in k.iter_mut().enumerate() {
        *k = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(k)
}

fn to_hex(k: &[u8]) -> String {
    k.iter().map(|b| format!("{b:02x}")).collect()
}

fn test_key() -> Result<Option<[u8; 32]>, String> {
    let Ok(hex) = std::env::var(TEST_KEY_ENV) else {
        return Ok(None);
    };
    if !test_home() {
        return Err(format!("{TEST_KEY_ENV} works only for a test data folder; unset it"));
    }
    parse_hex(&hex)
        .map(Some)
        .ok_or_else(|| format!("{TEST_KEY_ENV} must be 64 hex"))
}

/// Only exit 44 (no such item) means "no key yet". Any other failure stops
/// here, so a passing glitch never makes a new key over the old one.
fn keychain_read(service: &str) -> Result<Option<[u8; 32]>, String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", service, "-a", ACCOUNT, "-w"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "could not run security for the paperwork key".to_string())?;
    if out.status.code() == Some(44) {
        return Ok(None);
    }
    if !out.status.success() {
        return Err("could not read the paperwork key from the Keychain (is it locked?)".into());
    }
    let hex = String::from_utf8_lossy(&out.stdout).trim().to_string();
    parse_hex(&hex)
        .map(Some)
        .ok_or_else(|| "the paperwork Keychain item does not hold a key; nothing was changed".into())
}

/// No -U: never replace an existing item, whose key may still open the data.
pub fn keychain_write_command(hex: &str) -> String {
    format!("add-generic-password -s {SERVICE} -a {ACCOUNT} -l \"cashcast paperwork\" -w {hex}\n")
}

/// The key goes in on stdin (`security -i`), never as an argument.
fn keychain_write(k: &[u8; 32]) -> Result<(), String> {
    let mut child = Command::new("security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "could not run security".to_string())?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(keychain_write_command(&to_hex(k)).as_bytes())
        .map_err(|_| "could not write to security".to_string())?;
    child.wait().map_err(|_| "security did not finish".to_string())?;
    Ok(())
}

thread_local! {
    static HELD: RefCell<Option<[u8; 32]>> = const { RefCell::new(None) };
}

/// Run `f` with this key for every sealed read and write inside it.
pub fn with_key<T>(k: [u8; 32], f: impl FnOnce() -> T) -> T {
    HELD.with(|h| *h.borrow_mut() = Some(k));
    struct Drop0;
    impl Drop for Drop0 {
        fn drop(&mut self) {
            HELD.with(|h| *h.borrow_mut() = None);
        }
    }
    let _g = Drop0;
    f()
}

/// The key if one exists; None when none was made yet. A key found only in
/// the old sidequest item is copied to the cashcast item first.
pub fn key() -> Result<Option<[u8; 32]>, String> {
    if let Some(k) = HELD.with(|h| *h.borrow()) {
        return Ok(Some(k));
    }
    if let Some(k) = test_key()? {
        return Ok(Some(k));
    }
    if keychain_off() {
        return Ok(None);
    }
    if let Some(k) = keychain_read(SERVICE)? {
        return Ok(Some(k));
    }
    match keychain_read(LEGACY_SERVICE)? {
        None => Ok(None),
        Some(old) => {
            keychain_write(&old)?;
            match keychain_read(SERVICE)? {
                Some(back) if back == old => Ok(Some(old)),
                _ => Err("could not copy the paperwork key to the cashcast Keychain item".into()),
            }
        }
    }
}

/// The key, made and saved in the Keychain on first use.
pub fn ensure_key() -> Result<[u8; 32], String> {
    if let Some(k) = key()? {
        return Ok(k);
    }
    if keychain_off() {
        return Err(format!(
            "no vault key: paperwork keeps its key in the macOS Keychain (tests set {TEST_KEY_ENV})"
        ));
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&random(32));
    keychain_write(&k)?;
    keychain_read(SERVICE)?
        .ok_or_else(|| "could not save the paperwork key in the Keychain (is it unlocked?)".into())
}

/// The key a write to this file needs: never a new key over data it could not open.
pub fn key_for(path: &Path) -> Result<[u8; 32], String> {
    if !path.exists() {
        return ensure_key();
    }
    key()?.ok_or_else(|| "the paperwork key is missing from the Keychain; nothing was read".into())
}

pub fn where_() -> String {
    if std::env::var_os(TEST_KEY_ENV).is_some() {
        return format!("test key ({TEST_KEY_ENV})");
    }
    if keychain_off() {
        return "none (Keychain off for test homes)".into();
    }
    format!("macOS Keychain, service {SERVICE}, account {ACCOUNT}")
}

pub fn private_dir(path: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("cannot protect {}: {e}", path.display()))?;
    }
    Ok(path.to_path_buf())
}

/// The new file is on disk (fsync) before it replaces the old one, and the
/// folder after: a power cut leaves the old file or the new one.
pub fn write_sealed(path: &Path, plain: &[u8], purpose: &str) -> Result<(), String> {
    let folder = private_dir(path.parent().unwrap_or(Path::new(".")))?;
    let blob = seal(&ensure_key()?, plain, purpose);
    let tmp = PathBuf::from(format!("{}.{}.tmp", path.display(), to_hex(&random(4))));
    let err = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp).map_err(err)?;
        f.write_all(&blob).map_err(err)?;
        f.sync_all().map_err(err)?;
    }
    std::fs::rename(&tmp, path).map_err(err)?;
    std::fs::File::open(&folder).and_then(|d| d.sync_all()).map_err(err)?;
    Ok(())
}

/// The plain bytes, or None when the file does not exist yet.
pub fn read_sealed(path: &Path, purpose: &str) -> Result<Option<Vec<u8>>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let k = key()?.ok_or("the paperwork key is missing from the Keychain; nothing was read")?;
    let blob = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    open(&k, &blob, purpose).map(Some)
}
