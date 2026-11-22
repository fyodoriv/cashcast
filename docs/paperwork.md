# Citizenship paperwork: how to use it

cashcast tracks your Canadian citizenship application on this Mac only. Dates, trips,
ID numbers and scans are sealed (AES-256-GCM, key in the macOS Keychain). Nothing goes
online. Why and how: [paperwork-threat-model.md](paperwork-threat-model.md).

Two commands:

- `bin/cashcast paperwork`: the steps, your presence day count, police certificates,
  documents, and the official sources.
- `bin/cashcast citizenship`: the guided application. It judges your answers and each
  screen of the IRCC online form.

Run the private parts **in your own Terminal**. An agent or a pipe gets only a
redacted view (statuses and yes/no), and cannot enter or change a date. Private
verbs ask for Touch ID or your login password first.

## 1. Once: enter your dates and trips

Type each value at the prompt, never after the command (the shell history would keep it).

```bash
bin/cashcast paperwork set ca-citizenship-adult pr_date
```

```bash
bin/cashcast paperwork set ca-citizenship-adult birth_date
```

```bash
bin/cashcast paperwork trip ca-citizenship-adult
```

Run `trip` once per trip outside Canada in the last 5 years. It asks the day you left,
the day you came back, and the place. Remove one with `untrip`. Time in Canada before
PR as a temporary resident (work or study permit): `temp`, then `untemp` to remove it.

## 2. Check whether you can apply

```bash
bin/cashcast paperwork presence ca-citizenship-adult --reveal
```

It prints your days in Canada (1,095 needed in the 5 years before you sign) and the
earliest day you can sign. Exit 0 means enough days. Try another sign day with `--sign`.

## 3. Work through the steps

```bash
bin/cashcast paperwork show ca-citizenship-adult
```

It lists 6 steps and 34 items with the next action, each cited from an official IRCC
page. Mark progress:

```bash
bin/cashcast paperwork mark ca-citizenship-adult taxes done
```

Use `todo`, `done` or `na` (not needed), and add `--add-note` for a note typed at the
prompt. Keep a scan sealed with `attach ca-citizenship-adult passport-pages <file>`, then
delete the original it names. Get a copy back with `export-file … --out <new file>`.

## 4. Fill in the application answers

```bash
bin/cashcast citizenship status
```

It shows each question, its state and where its answer came from. To fill them:

- **From your documents**: `bin/cashcast citizenship collect <kind> <file>`. Kinds are
  `pr-card`, `passport`, `id`, `marital`, `language`, `leases`, `linkedin`, `nomads` and
  `cra`. Use `collect-all <folder>` for a folder of them. It never replaces an answer you typed.
- **By hand**: `bin/cashcast citizenship template --out <file>` writes a blank TOML file
  (mode 600). Fill it, then run `bin/cashcast citizenship put <file>`. `put` seals the
  answers and deletes the file.
- **What is still missing**: `bin/cashcast citizenship questions` lists every open
  question at once.

Then judge them:

```bash
bin/cashcast citizenship answers
```

`ready: yes` (exit 0) means no gap in your 5-year history, enough days, police
certificates named, and taxes, IDs and passports in order. Each FAIL says how to fix it.
`police` names each country that needs a certificate. `versions` and `restore <name>`
bring back an older draft.

## 5. Proofs and the evidence page

Keep a photo next to an answer: `bin/cashcast citizenship proof <question> <image> --source "<where>"`.
`bin/cashcast citizenship report` writes a local evidence page with every answer and its
proof. It lives 10 minutes, is never published, and opens only on this Mac.

## 6. Apply online with Claude

Ask Claude "help me with my citizenship application". The `citizenship-application`
skill first checks that the facts are still current (`paperwork sources --live`). Then it
walks you through the IRCC form screen by screen, judging each screen
(`citizenship screen`) before you press Next. You type every ID number and press
every Submit, Pay and Sign yourself. Claude never repeats an ID number in chat.

## Keep it healthy

- `bin/cashcast paperwork doctor`: modes, links, the key, leftover files. Expect only `ok` lines.
- `bin/cashcast paperwork sources --live`: re-reads every official page. Expect OK and one
  MANUAL (the CIT 0407 PDF: look up its quotes by hand). CHANGED means a rule moved.
  Update `engine/src/paperwork/facts.rs`.
