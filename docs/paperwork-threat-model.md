# Paperwork: threat model

Paperwork walks the owner through hard document processes, such as a Canadian
citizenship application. It holds the most sensitive data in cashcast: travel
history, immigration dates and scans of identity documents. This page says what we
protect, from whom, how, and what we do not protect. Written before the code
(2026-11-01); a change to paperwork updates this page in the same PR.

## Short version

- Everything stays on the Mac. Nothing goes to the Cloudflare site, the shared store,
  logs, telemetry, evals, tests or git.
- The data is encrypted at rest (AES-256-GCM). The key lives in the macOS Keychain,
  never in a file.
- Paperwork has no web view and no web route: the owner reads it in a terminal
  (`--reveal`) and, for citizenship, on the private local evidence page
  (`bin/cashcast citizenship report`). The Actions view lists it as Terminal only.
- Agents get a redacted CLI view: statuses and counts, never dates, notes or files. The
  full values are for the owner's eyes: the skill forbids agents to open paperwork in any
  browser tool, or to read paperwork output in the owner's terminal.
  That is a rule, not a lock (see "What we do not protect against").
- Only the owner enters dates and trips, in a terminal. Agents and
  pipes cannot, so they cannot probe the redacted counts for the private values. In a
  terminal the owner types them at prompts, never after the command, where `ps` and the
  shell history would show them.
- The agent never signs in, never types ID numbers, never pays and never submits.

## Assets

| Id | Asset | Example | Where |
|---|---|---|---|
| A1 | Personal fields | date you became a PR, planned sign date, birth date, notes | encrypted state |
| A2 | Travel history | trips out of Canada (dates, place) | encrypted state |
| A3 | Documents | scans of passport pages, ID cards, language proof | encrypted files |
| A4 | Progress | which checklist items are done | encrypted state |
| A5 | The data key | 32 random bytes | macOS Keychain only |
| A6 | Process facts | the official rules, fees and links | public, in git |
| A7 | Application answers | every answer the IRCC form asks: names, UCI, SIN, passport numbers, addresses, trips, work history | plain YAML, mode 600 |

Data minimization: paperwork never asks for ID numbers (passport, PR card, UCI). The
checklist tracks "you have a colour copy of your passport pages", not the number.
The owner types numbers into the official form themself.

The guided application (skill `citizenship-application`, added 2026-11-07) is the owner's
own choice of a different trade-off: they give Claude every answer so Claude can guide them
through the form in Chrome. A7 holds ID numbers. Since 2026-11-13 it is **sealed** with the
paperwork key (`private/users/<name>/paperwork/citizenship/answers.sqv`, and sealed copies in `versions/`); the
owner runs `node scripts/citizenship-judge.ts seal` once in a terminal to move an old plain
`answers.yaml` in, and the judge fails while a plain draft is there. Agents read it with
`show`, which hides ID numbers, on a terminal too: an agent tool run under `script` gets a
terminal. Since 2026-11-14 only `show --reveal` shows them, with stdin and stdout on a
terminal, after the owner types "reveal" at its prompt. Not protected: an agent that drives
a pseudo-terminal can type it too (task paperwork-owner-presence asks for Touch ID). The
`put` scratch file is made with `template --out <file>` (mode 600 from its first byte), and
the `answers` judge FAILs a scratch answers file in `$TMPDIR` that others can read. Its other controls: it lives under the
ignored `private/`;
`scripts/citizenship-judge.ts` fails while others can read it (anything but mode 600) and
never prints a name, an ID number or contact details (unit and e2e canaries; it does print
dates, day counts and country names); the skill never repeats an ID number in chat,
reads pages as text rather than screenshots, deletes each screen's scratch JSON, and never
types an ID number, signs or submits. Not protected: any program of the owner's account can
run the judge, which opens the vault through the Keychain; a guided session's transcript
holds the values Claude read (never ID numbers through `show`); and `evidence.html` is a
plain file while it lives (10 minutes).

Since 2026-11-11 the draft tools also write A7 and its copies: `collect` merges a source into
it and keeps each earlier draft in `private/users/<name>/paperwork/citizenship/versions/` (files 600, folder 700, the
last 100). The collectors read scratch text files Claude writes from the owner's photos and
pages; they print key names only (canary test in `engine/tests/citizenship.rs`),
and the skill deletes each scratch file after its collect. Not protected: the copies hold the
same ID numbers as A7.

Since 2026-11-12 the evidence page also holds A7: `proof` keeps screenshots of the sources
in `private/users/<name>/paperwork/citizenship/proofs/` (files 600, folder 700), and `report` writes every answer and
those screenshots into `private/users/<name>/paperwork/citizenship/evidence.html` (mode 600, written whole). The page
loads nothing from the network (its Content-Security-Policy allows only inline styles and
`data:` images) and is never published: the owner chose a local page over a claude.ai
artifact on 2026-11-12. Since 2026-11-14 each proof next to the sealed draft is sealed with
the paperwork key (`proofs/<name>.sqv`, purpose `citizenship:proof`), and `seal` moves old
plain screenshots in. `report` opens them from the vault, and the page is removed 10 minutes
after it is written (a detached timer, and any later judge run that finds it older). Not
protected: the page is plain while it lives, and a Chrome screenshot saved for a proof also
sits in the session's tool results until deleted
(tests in `engine/tests/citizenship.rs`).

## Where the data lives

| What | Path | Mode | In git? |
|---|---|---|---|
| State (A1, A2, A4) | `private/users/<name>/paperwork/state.sqv` | file 600, folder 700 | no (`private/` is ignored) |
| Documents (A3) | `private/users/<name>/paperwork/files/<random id>.sqv` | file 600, folder 700 | no |
| Write lock | `private/users/<name>/paperwork/state.lock`, and `state.lock.break` while a stale lock is broken | file 600 | no; holds only the writer's process id |
| Key (A5) | Keychain item, service `cashcast`, account `paperwork-key` (copied once from the old `sidequest` item) | login keychain | no |
| Facts (A6) | `engine/src/paperwork/facts.rs` | normal | yes |
| Answers (A7) | `private/users/<name>/paperwork/citizenship/answers.sqv` (a worktree falls back to the main checkout's) | file 600 | yes (since 2026-11-13; `seal` moves an old `answers.yaml`) |
| Evidence page (A7) | `private/users/<name>/paperwork/citizenship/evidence.html` | file 600, removed after 10 minutes | no |
| Proofs (A7) | `private/users/<name>/paperwork/citizenship/proofs/<name>.sqv` | files 600, folder 700 | yes (since 2026-11-14; `seal` moves old plain ones) |

One state file holds every process, so the folder listing does not even say which
process the owner is on. Document file names are random; the real names are inside the
encrypted state.

## Threats and controls

| Id | Threat | Control | Test |
|---|---|---|---|
| T1 | Data lands in git (commit, PR, issue) | Data lives only in `private/` (ignored), encrypted; tests and evals use synthetic people; the pre-push privacy gate | `engine/tests/paperwork.rs` ("nothing plaintext on disk") |
| T2 | Data leaves the Mac | cashcast publishes nothing; paperwork has no web route, so `bin/cashcast web` cannot serve it | `engine/tests/paperwork.rs` |
| T3 | Data reaches logs or telemetry | Events carry only the story id and a kind (`item`, `field`, `trip`, `untrip`, `temp`, `untemp`, `file`), never values, results (met or not), ids of documents or process names. `paperwork.open` carries nothing more, and `paperwork.unlock` only whether the link worked. The `action.run` event of a paperwork action leaves out the exit code, which says whether you can apply. Telemetry (`src/core/telemetry.ts`) drops every `paperwork.*` event, so not even a count leaves the Mac; error messages never quote input | `engine/tests/paperwork.rs` ("never reaches the logs") |
| T4 | A hosted model reads the data | The engine calls no model. CLI output that is not a terminal (agents, pipes, the Actions view) is redacted: statuses, item counts and public checklist text only. For the day count it says only whether you have enough days (and the 1,095 the rule needs), never the count: with today's date, a count gives away the PR date. `--reveal` works only on a terminal. Agents and pipes cannot change dates, trips or documents or try another sign day (`set`, `trip`, `untrip`, `temp`, `untemp`, `attach`, `export-file`, `presence --sign` and `mark --add-note` exit 2 outside a terminal, and the catalog marks them terminal-only), so they cannot binary-search the answers for a private date. In the terminal the owner types dates, places, sign days and notes at prompts: `ps` shows a command's words to every account on the Mac, and the shell history keeps them, so words after the command are refused (exit 2) and the message says to delete that history line. `paperwork` always uses the real today: `--today` does not reach it | `engine/tests/paperwork.rs` ("agent view is redacted", "the agent view gives away no date", "agents and pipes cannot change dates or trips", "private values are typed at a prompt") |
| T5 | Someone on the network opens the API | The web app has no paperwork route: the Actions view lists `paperwork` and `citizenship` as Terminal only, and the web server refuses terminal-only actions | `engine/src/core/commands.rs` `TERMINAL_ONLY`, `tests/e2e/us-36-paperwork.sh` |
| T6 | A web page or another local program runs a paperwork action through the browser | No browser path exists: sidequest's one-time unlock link, browser keys and cookies were dropped in the move to cashcast (2026-11-22) | `tests/e2e/us-36-paperwork.sh` |
| T7 | Disk theft, a lost backup, a synced folder | AES-256-GCM; the key is in the login Keychain, which macOS encrypts with the login password; files 600. The doctor fails on a link anywhere in the paperwork folder, on a hard link (a file with a second name, maybe in a synced folder), and on a paperwork folder that is itself a link: what a link points to may sit in a synced folder, unchecked. It fails on a home folder (`<home>/home`) that links to nothing, where every save would fail. It warns when the paperwork folder sits under a link (`<home>` or `<home>/home`): the files are still checked, but they may sit in a synced folder | "nothing plaintext on disk", "doctor fails on a link anywhere in the paperwork folder", "doctor fails on a hard link", "doctor fails when the paperwork folder itself is a link", "doctor warns when the paperwork folder sits under a link", "doctor fails on a home folder that links to nothing, and makes nothing there" |
| T8 | Someone edits the encrypted files | GCM tag check; the additional data binds format version and purpose (state or one file id), so a swapped file fails | "a tampered or swapped file is refused" |
| T9 | The browser keeps a copy | No browser ever loads paperwork; the evidence page is a local file removed after 10 minutes | `engine/tests/citizenship.rs` |
| T10 | Plain copies left behind | `attach` reads a file and stores an encrypted copy; it prints that the original is still there and where to delete it. `export-file` writes a 600 copy only where the owner says, and refuses to overwrite | `tests/e2e/us-36-paperwork.sh` |
| T11 | The agent acts with legal effect | Signing in, typing ID numbers, paying, signing and submitting are owner-only items with exact instructions; the skill forbids them; no command does them | `evals/cases/paperwork.yaml` |
| T12 | Wrong or stale facts | Facts come only from official IRCC pages (`canada.ca`, `ircc.canada.ca`, `cic.gc.ca`), each with its URL, a short quote and the date checked. `bin/cashcast paperwork sources --live` re-reads every page and fails when a quote is gone. A page it cannot read as text (the CIT 0407 PDF) shows as MANUAL, with the quotes to look up by hand, never as ok. Checks older than 90 days show as due | "every requirement cites an official IRCC page", "sources --live never calls a PDF it cannot read ok", `paperwork sources` |
| T13 | A new dependency is compromised | No new dependency: the cipher is Node's built-in `node:crypto` (OpenSSL), behind the adapter `engine/src/paperwork/vault.rs` | none needed |
| T14 | The key leaks through the process list | The key is written to the Keychain through `security -i` on stdin, never as a command argument | `engine/tests/paperwork_vault.rs` ("never an argument") |
| T15 | A Keychain glitch replaces the key, so the data can no longer be opened | A new key is made only when the Keychain says the item does not exist (`security` exit 44). Any other read failure (a locked keychain, a cancelled prompt) stops with an error. The key is added without `-U`, so an existing item is never replaced. A save reads the key before it takes the write lock (`vault.keyFor`) and holds it for that one call (`vault.withKey`), so a Keychain prompt never holds the lock; once the state file exists, a missing key stops the save instead of making a new one | `engine/tests/paperwork_vault.rs` ("never makes a new key over the old one", "an update holds the key for one call") |
| T16 | Two saves at once (two CLI runs, or the CLI and an action) lose a change | One writer at a time: a lock file made with O_EXCL holds the writer's process id. A live lock is waited for, up to 10 seconds. A live writer's lock is never broken, however old: the Mac may sleep in the middle of a save. A lock left by a crash (a dead process, no process id after 5 seconds, or a lock from before the Mac started, since process ids start over at boot) is broken under a second lock, and only after a fresh check, so a waiter never removes the next writer's new lock. Where a sandbox hides the boot time (the agent sandbox), only the process id decides. A writer removes only its own lock. The doctor warns about a lock older than a minute and about the encrypted leftover of an interrupted save (safe to delete); a plain file, an extra folder or a folder not mode 700 fails, at any depth | `engine/tests/paperwork.rs` ("saves at the same time keep every change", "a save never breaks a live save's lock", "a lock from before the Mac started is stale", "a save removes only its own lock", "doctor checks the browser key's mode and finds a lock left behind", "doctor: a sealed leftover of an interrupted save warns", "doctor finds a plain file or an open folder at any depth") |

## Crypto

- Cipher: AES-256-GCM from the RustCrypto `aes-gcm` crate. A fresh random 12-byte nonce per write.
- Format: `SQV1` (4 bytes) + nonce (12) + tag (16) + ciphertext.
- Additional data: `sidequest-vault:v1:<purpose>`, where purpose is `state` or
  `file:<id>`. A file moved to another purpose fails to open.
- Key: 32 random bytes, hex, in the login Keychain. Made on the first save, and only
  when the Keychain says no item exists (T15).
- Adapter: `engine/src/paperwork/vault.rs` (`seal`, `open`, `readSealed`, `writeSealed`). Swapping
  the cipher or the key store touches only that file.
- Why not a library like libsodium: OpenSSL is maintained and already in Node, and a new
  package would add supply-chain risk for the most sensitive data we hold. The adapter
  keeps that choice cheap to change.

## Tests never touch the real Keychain

When `VITEST` is set, or the data folder is inside the system temp folder (every test
home is), the vault never calls the real `security` (`/usr/bin/security`). Tests pass a
synthetic key in `CASHCAST_TEST_VAULT_KEY`, which the vault refuses for any other data
folder. The doctor fails if that variable is set for the real data folder. The vault
tests put a fake `security` script first on `PATH` to test the Keychain path. No
environment switch turns the Keychain on in a test home (T4).

## What we do not protect against

- Malware or a person with a shell as the owner's macOS user. They can run
  `security find-generic-password` and read the key. Keep the Mac locked and updated.
- A coding agent the owner runs with full shell access could do the same. The skill
  forbids it and the redacted CLI view makes it unnecessary, but this is a rule, not a
  lock. There is no Keychain prompt to watch for: `security` made the item, so macOS
  lets `security` read it without asking. The terminal check alone tests only that output
  goes to a terminal, and a shell can fake one with `script`, so the terminal-only paperwork
  verbs (`--reveal`, `attach`, `export-file`, and every prompt) first ask macOS for
  Touch ID or the login password (`engine/src/paperwork/presence.rs`, LocalAuthentication through
  `/usr/bin/swift`); a "no", a cancel or a failed check exits 2 and shows or changes nothing
  (`engine/tests/paperwork.rs`, "a fake terminal with no owner present gets exit 2 and no
  private value"). A test data folder (in the temp folder) skips the prompt: it holds no real
  data, and the vault never opens the real Keychain for it. 
- An agent that drives the owner's own terminal could read paperwork output there. The skill
  forbids it; that is a rule, not a lock.
- The file paths that `attach` and `export-file` take are command words: `ps` and the
  shell history show the path, never the content. Keep private words out of file names.
- A crash that leaves a lock whose process id goes to another live program before the
  Mac restarts: the lock looks live, so saves wait 10 seconds and then say to try again.
  The doctor warns about a lock older than a minute and says how to remove it.
- - An agent that reads the redacted view every day. Its yes/no answers (enough days, a
  police certificate needed, the 18-54 language rule) each change on one real day, so
  daily polling could learn those days. Each answer changes a few times at most, and it
  never gives the trips or the count; we accept this.
- Someone who can write the folder putting back an older `state.sqv`. It opens fine
  (same key, same purpose), so progress and trips roll back. GCM proves a file is ours,
  not that it is the newest; a Time Machine restore does the same on purpose.
- Plain files outside cashcast: originals in Downloads, the official portal, email.
- Memory and swap of a running process. FileVault covers swap on disk.
- Losing the key. Without the Keychain item the encrypted data cannot be read. The
  originals (paper documents, the official portal) are the backup; paperwork is a
  guide, not the record.

## Owner-only steps

The agent never does these. The CLI shows each one with exact instructions.

1. Sign in to the official online account (IRCC). Create it yourself.
2. Type passport, PR card or other ID numbers into the official form.
3. Pay the fee on the official payment page.
4. Sign and date the application. The date you sign sets your 5-year window.
5. Submit the application.
6. Take the citizenship test and attend the ceremony.

## Moved from sidequest (2026-11-22)

The owner decided that Geese (sidequest) is for socialising and cashcast holds private
things, so Paperwork moved here (decision 0017). The vault file format and its
additional data (`sidequest-vault:v1:<purpose>`) stay the same byte for byte, so files
sealed by sidequest open unchanged (`engine/tests/paperwork_vault.rs`, "opens a sidequest
blob"). The key is copied once from the `sidequest` Keychain item to a `cashcast` item,
through `security -i` on stdin, and read back to compare. Dropped in the move: the
browser unlock (T5, T6, T9), the documents model route and the `SIDEQUEST_TODAY` switch;
`paperwork` always uses the real today. The answers file is TOML now (no YAML reader).
