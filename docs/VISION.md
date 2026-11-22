# Vision

Keep this to one screen. A PR is accepted or rejected by reading only this page.

## Problem

People in Canada with several banks, cards and brokers leave money on the
table every month. They use the wrong card for a category, miss a cap reset,
pay a bank fee, let a card report high utilization before a mortgage, or keep
too much cash idle. The tools that would catch this either need your bank
logins in someone's cloud (aggregators) or are generic spreadsheets with no
Canadian rules. This tool keeps the data on your computer and knows the
Canadian rules.

## User

A person in Canada (any province or territory) who runs a coding agent and
wants their finances optimized with five minutes a month. One copy can serve
a household: each person has their own private folder.

## Success

- **North star**: the person's **net total**, everything they have minus
  everything they owe, in CAD. Higher is better. Every section offers options
  and the strategy ranks them by their effect on the net total, inside the
  limits the person sets (`[boundaries]`: no burnout, a cash floor with a
  margin, risk, debt, keeping the home). A move that breaks a limit is never
  recommended; it shows as refused, with the reason. Measured by
  `bin/cashcast net-total` (trend over 30 and 365 days,
  `data/net-total-history.json`; [decision 0014](decisions/0014-net-total-north-star.md)).
- **Metric**: net yearly gain the strategy finds versus the person's current
  routine: cash back net of fees, fees avoided and tax saved. Pre-registered
  for the owner: **at least +$800 a year**. That is the gain the hand-made
  hand-made plan found when it moved from 3 cards to 4.
- **Target**: monthly review done in at most 5 minutes; zero private-data
  leaks (canary tests green); forecast verified to the cent.
- **Measured by**: `bin/cashcast strategy` (the cash back "Net" line and the
  alternatives total), the review's time from `review_shown` to
  `reminders_sync` in `logs/events.jsonl`, and `scripts/check.sh`.

## Non-goals

- No cloud storage, no hosted service, no bank-login aggregation.
- No agent ever moves money, trades, or pays. Those become reminders.
- No personal "buy this stock" calls and no forecasts stated as facts. The
  tool gives screens, rules and arithmetic; investment moves show a gain
  range with evidence, rank "do nothing" beside them and respect the owner's
  overrides (decision 0013). It is not a licensed advisor.
- No support outside Canada (tax, accounts and card rules are Canadian).
- No Musk-affiliated services or APIs.

## Kill criteria

Stop or archive the project when any of these holds:

- Through 2027-06-30, the strategy finds under $200 a year of net gain for the
  owner across three monthly reviews in a row. The hand-made plan is then enough.
- A private-data leak reaches git or a third party and the fix needs more
  than a test and a patch, so the privacy model is unsound.
- The owner skips the monthly review four months in a row by 2027-06-30.

## Prior art

| Tool | Overlap | Verdict (use, contribute, or build the rest) |
|---|---|---|
| Monarch, Copilot, YNAB | budgets, categories, net worth | use as **inputs** (CSV exports); they are cloud-hosted and have no Canadian card-routing or tax logic |
| MoneyWiz | local budgeting with bank sync | use as the ledger the owner keeps: read its database, add bank rows through its URL scheme (decision 0006) |
| Wealthsimple Tax and other NETFILE software | filing | use for filing; this tool only estimates rates for decisions |
| Koinly, ZenLedger | crypto cost basis | overlap with `holdings` ACB; this one is local and Canadian-only |
| Credit-card comparison sites (e.g. MoneySense, Ratehub) | card catalogs | cite as sources in `knowledge/`; they do not optimize your own wallet against your caps |
| Actual Budget (open source, local) | local budgeting | closest local tool; no Canadian strategy layer. Build the strategy layer here; reuse exports |
