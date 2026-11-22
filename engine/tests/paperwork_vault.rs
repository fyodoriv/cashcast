//! Vault: AES-256-GCM files that stay byte-compatible with sidequest's.
use cashcast_engine::paperwork::vault;

const K: [u8; 32] = [7u8; 32];

#[test]
fn seal_then_open_round_trips() {
    let blob = vault::seal(&K, b"CANARY-PAPER", "state");
    assert_eq!(&blob[..4], b"SQV1");
    assert!(!blob.windows(12).any(|w| w == b"CANARY-PAPER"));
    assert_eq!(vault::open(&K, &blob, "state").unwrap(), b"CANARY-PAPER");
}

#[test]
fn wrong_purpose_or_key_or_tamper_reads_nothing() {
    let mut blob = vault::seal(&K, b"x", "state");
    assert!(vault::open(&K, &blob, "draft").is_err());
    assert!(vault::open(&[8u8; 32], &blob, "state").is_err());
    let n = blob.len() - 1;
    blob[n] ^= 1;
    assert!(vault::open(&K, &blob, "state").is_err());
    assert!(vault::open(&K, b"short", "state").is_err());
}

/// A blob sealed by sidequest's node:crypto code (key 07.., nonce 00.., purpose "state",
/// plain "hello") still opens: the owner's files move unchanged.
#[test]
fn opens_a_sidequest_blob() {
    let hex = "5351563100000000000000000000000030055369bd17aa31345935806ca5345909bdc8de94";
    let blob: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    assert_eq!(vault::open(&K, &blob, "state").unwrap(), b"hello");
}

#[test]
fn write_command_never_replaces_an_item() {
    let c = vault::keychain_write_command("ab");
    assert!(c.starts_with("add-generic-password -s cashcast -a paperwork-key"));
    assert!(!c.contains(" -U"));
}
