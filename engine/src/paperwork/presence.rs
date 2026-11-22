//! Owner presence: before a private paperwork verb, macOS asks for Touch ID
//! or the login password. A terminal alone proves nothing: any program can
//! make one with `script`.
//!
//! Lifecycle scope: the test replacement lives in a thread-local for the
//! thread that set it; `replace(None)` deletes it.

use std::cell::RefCell;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub type Ask = Box<dyn Fn(&str) -> bool>;

// LocalAuthentication, policy deviceOwnerAuthentication: Touch ID, or the
// login password when Touch ID is off or fails. Exit 0 means the owner said yes.
const SWIFT: &str = r#"import LocalAuthentication
import Foundation
let reason = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "use cashcast paperwork"
let ctx = LAContext()
var err: NSError?
guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &err) else { exit(3) }
let done = DispatchSemaphore(value: 0)
var ok = false
ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { r, _ in ok = r; done.signal() }
done.wait()
exit(ok ? 0 : 1)
"#;

/// The real check. Only /usr/bin/swift: a `swift` first on PATH could be a
/// fake that says yes. Waits up to 3 minutes.
pub fn mac_ask(reason: &str) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let Ok(mut child) = Command::new("/usr/bin/swift")
        .args(["-", reason])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(SWIFT.as_bytes()).is_err()
    {
        let _ = child.kill();
        return false;
    }
    let until = Instant::now() + Duration::from_secs(180);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

thread_local! {
    static REPLACED: RefCell<Option<Ask>> = const { RefCell::new(None) };
}

/// Tests replace the check; `None` puts the real one back.
pub fn replace(ask: Option<Ask>) {
    REPLACED.with(|r| *r.borrow_mut() = ask);
}

/// True when the owner is at this Mac. A test data folder holds no real data,
/// and the vault never opens the real Keychain for it, so no prompt is needed.
pub fn confirm(reason: &str) -> bool {
    if let Some(answer) = REPLACED.with(|r| r.borrow().as_ref().map(|f| f(reason))) {
        return answer;
    }
    if super::vault::test_home() {
        return true;
    }
    mac_ask(reason)
}
