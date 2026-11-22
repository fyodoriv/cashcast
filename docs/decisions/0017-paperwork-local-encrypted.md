# 17. Paperwork moves from sidequest: local, encrypted, Terminal only

Date: 2026-11-22 · Status: accepted

## Context

Paperwork (citizenship steps, the presence count, official sources and the guided
citizenship application) was built in sidequest (its decision 0007, 2026-11-01). On
2026-11-22 the owner decided that Geese (sidequest's hosted site) is for socialising
together, and that private personal things live in cashcast, which is local-first.
Paperwork holds the most sensitive data we keep: immigration dates, travel history and
ID numbers.

## Decision

- Paperwork lives in the cashcast engine (`engine/src/paperwork/`), per person under
  `private/users/<name>/paperwork/` (folder 700, files 600).
- Everything is sealed with AES-256-GCM. The key is in the macOS Keychain (service
  `cashcast`, account `paperwork-key`), never in a file. The file format stays the
  sidequest one, byte for byte, so moved files open unchanged; the key is copied once
  from the `sidequest` item.
- Paperwork has no web route. The Actions view lists `paperwork` and `citizenship` as
  Terminal only. Sidequest's browser unlock is not ported.
- Agents get a redacted CLI view only. Private verbs need a terminal and Touch ID.
- Nothing about paperwork is logged, sent as telemetry or published.

## Consequences

- One new dependency, the RustCrypto `aes-gcm` crate (with `getrandom`).
- The owner reads full values only in Terminal (`--reveal`) or on the local evidence page.
- Threat model: [`docs/paperwork-threat-model.md`](../paperwork-threat-model.md).
