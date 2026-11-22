# Contributing

Strategies, connectors and facts are all welcome. Each kind has a fixed shape,
and a contract test fails when a piece is missing, so a reviewer can trust a
green gate.

## Ground rules (privacy)

- Synthetic data only. Never put real amounts, balances, account numbers,
  names, payees or employers in code, tests, fixtures, issues or PRs.
- Free text in a fixture carries a `CANARY` string. The leak tests fail if
  one reaches any output.
- Never open anyone's `private/` folder, including your own, while you work
  on code. `docs/privacy.md` has the full rules.
- Agents never move money. A contribution that automates a transfer, trade or
  payment will be rejected.

## Add a strategy section

1. Write `engine/src/strategy/<id>.rs` with `build(ctx) -> Section`. Read
   only `ctx.sit` (profile, aggregates, holdings), `ctx.results` (earlier
   sections) and the knowledge facts.
2. Put dated to-dos in `Section.tasks`, each with a `risk`
   (`read`, `settings`, `money`). They flow into the monthly review and the
   user's Reminders list.
3. Register it in `PORTED` (`engine/src/strategy/mod.rs`); its doc comment
   names its sources (repo files or `data/…`).
4. Add a test in `engine/tests/app_strategy_sections.rs` that names the id
   with synthetic inputs. `tests/repo/contributions.sh` checks that.
5. Add a row to `docs/user-stories.md` if users see something new.

Current sections: cashback, banking, credit, tax, reserve, foreign, interest, crypto, invest, invest_plan,
forecast, payments, spending, budget, alternatives, review, net_total (last: it reads the others).

To offer a move that changes the net total, set the section's `net_options` data to a
list of objects with the fields of `Opt` in `engine/src/net_total.rs`: `id`,
`title`, `effect_year` (dollars a year) or `per_dollar` with `cash_out`, and
what it needs (`cost_once`, `work_hours_week`, `new_debt`, `risk`,
`sells_home`, `tags`, `why`). The engine checks each one against the owner's
`[boundaries]` and ranks it ([decision 0014](docs/decisions/0014-net-total-north-star.md)).

## Add or change a connector

1. Copy a `connectors/<id>.toml`; keep the schema (see `connectors/README.md`).
2. Write `connectors/playbooks/<id>.md` with one `### <action-id>` per action.
3. Money actions are `via = ["human"]`; settings actions ask for confirmation.
4. A new export format is a Rust change in `engine/src/export.rs` (cash) or
   `engine/src/ledger.rs` (investments), with a canary fixture and a test.
5. A read-only API client goes in `engine/src/api.rs`, tested with
   a fake transport (`engine/tests/app_api.rs`). Never call the network in a test.

## Add or change knowledge

1. Edit `knowledge/data/<topic>.toml`. Every fact table needs `source` and
   `confidence`: `verified` from a primary source, `estimate` until confirmed,
   or `heuristic` for a rule of thumb that no official source states.
2. Bump `[meta] as_of` and `review_by`.
3. Review it before the PR: `bin/cashcast knowledge propose path/to/<topic>.toml` lists what you
   add, remove and change against the current facts and blocks unsourced or older facts.
   Then copy it into `knowledge/data/` and run `bin/cashcast knowledge check`.
4. Put personal terms (your own card's real cap, a negotiated fee) in your
   private `private/users/<you>/knowledge/<topic>.toml` instead. It overrides
   by `id` and never leaves your computer.

## Pivot and iterate

- The review flags a pivot when predictions miss twice in a row. Record what
  changed and why in `docs/learnings.md` (no private numbers).
- A rule that stops fitting most people becomes a PR here; one that stops
  fitting only you becomes a private override.

## Delivery

Branch from `main`, run `scripts/check.sh`, open a PR with `## Summary`,
`## Details` and `## Docs`. See `AGENTS.md` → Delivery.

## Report problems and ideas

**Fyodor, the author of cashcast, is glad to get every report.** If something
in cashcast did not work as expected, or could work better, please say so. A
short report from a real user is the most useful contribution there is, even
when you are not sure it is a bug.

### Your agent will offer to do it for you

If you use cashcast with an AI agent (Claude Code, Codex), the agent offers a
report when cashcast fails, surprises it, or could clearly work better
(`AGENTS.md` and the `/cashcast` command say so). It never posts on its own:

1. It writes a short draft: what ran, what was expected, what happened, the
   version (`git -C <clone> rev-parse --short HEAD`), and a suggested fix.
2. It runs `agentbrew report --title "<title>" --body-file <draft.md> --repo fyodoriv/cashcast`.
   This posts nothing. It replaces private details with placeholders (home
   path, user name, emails, tokens, keys, IP addresses) and prints a safety
   checklist.
3. It shows you the exact title and body and asks: "Post this issue to
   github.com/fyodoriv/cashcast? It is public. (yes / edit / no)".
4. Only after your yes does it post (`agentbrew report ... --yes`).
5. If the fix looks small, it asks a second question before it opens a PR
   (`agentbrew contribute`).

A run with nobody at the keyboard only saves the draft
(`~/.local/state/agentbrew/reports/`) and names it in the run report.

### cashcast-specific safety

- A report never includes anything from `private/`: no amounts, balances,
  payees, account names, file names of your exports, or rows. It describes the
  `bin/cashcast` command and its error only.
- Without agentbrew, open an issue by hand at
  https://github.com/fyodoriv/cashcast/issues/new with the same content.

### Turn the offers off

Run `agentbrew report --off`. `agentbrew report --on` turns them back on.
