# 15. Investment advice reuses expert sources; live facts come from the Bank of Canada

- **Status**: accepted
- **Date**: 2026-11-08

## Context

The owner asked for the best investment advice for their situation, kept
current as things change, built on what experts already made rather than
reinvented. We compared the options (research on 2026-11-08):

- **Portfolio optimizers** (PyPortfolioOpt, Riskfolio-Lib): Python, which the
  engine no longer runs (decision 0010). Out of sample, mean-variance
  optimizers do not beat equal weights for a small portfolio (DeMiguel,
  Garlappi and Uppal 2009, https://doi.org/10.1093/rfs/hhm075). They would
  add false precision, not value.
- **Trackers and terminals** (Ghostfolio, Wealthfolio, Portfolio Performance,
  OpenBB): AGPL or EPL, heavy servers or keyed data, and none gives advice.
  Wealthfolio (Rust, local-first) is a design reference only; its AGPL code is
  not copied.
- **Expert frameworks** (FP Canada Projection Assumption Guidelines, SPIVA
  Canada, Canadian Couch Potato / PWL models, CRA limits): already the
  evidence behind `invest-plan` (decision 0013). They publish yearly or twice a
  year as pages and PDFs, with no API.
- **Official live data**: the Bank of Canada Valet API is free, keyless,
  read-only and published by the central bank.

## Decision

1. Advice stays on decision 0013's evidence: expert frameworks as cited facts
   with `review_by` dates. No optimizer library.
2. `bin/cashcast knowledge refresh` (`engine/src/live_facts.rs`) reads seven
   fixed Valet series (policy rate, CPI over 12 months, prime, 1- and 5-year
   GIC, 10-year Government of Canada bond, USD/CAD). It compares them with the
   planning facts they check and flags a gap over 1.5 points for review.
3. It runs on its own once a day from `cashcast` and `sync all` (the hourly
   launchd job); replies are cached per day. Setting `live.refresh` turns it
   off; `CASHCAST_LIVE=off` keeps tests offline.
4. Safety: request URLs are constants with series IDs only, GET with no body,
   no key, no user data, so no consent scope is needed (privacy exits table).
   A reply that does not parse or is outside a sane range is shown as
   unavailable and never becomes a fact. A live value never edits a hand
   fact: a flag leads to `knowledge propose` and a reviewed PR.

5. Live values feed the plan: `invest-plan` uses today's 1-year GIC rate as
   the safe rate for money needed soon and today's inflation for the cost of
   idle cash, cited as `live-gic-1y` and `live-cpi-yoy` (dropped after 90 days).
   Foreign cash converts USD at the saved live rate (`live-usd-cad`) when it is
   newer than the dated rate in `foreign_cash.toml` and at most 10 days old;
   a rate the user pinned in their own `foreign_cash.toml` always wins
   (added 2026-11-14, after the dated rate went two weeks stale).
6. `knowledge refresh` also watches the FP Canada guidelines page and says when
   the next year's edition is out. SPIVA (403 to scripts) and CRA limit pages
   (time out) stay on `review_by`.
7. Agentic investing projects (TradingAgents, ai-hedge-fund, FinRobot, FinGPT,
   OpenBB MCP, Anthropic financial-services plugins) were surveyed on
   2026-11-08. None fits index investing: their backtests are short and may
   leak future data ("The Alpha Illusion", arXiv 2605.16895). Borrowed instead:
   an advice-quality judge on synthetic profiles, a strongest objection per
   move (`objection`), deterministic risk vetoes in Rust (`veto`: reserve
   first, debt at 8%+ first, stale evidence), and a rubric judge over 12
   synthetic profiles (`engine/tests/invest_advice_eval.rs`). CSA Staff Notice 11-348 expects disclosure and a
   human decision, which the plan already has.

## Consequences

- A changed rate or inflation shows up within a day, not at the next manual
  review.
- Yearly sources (PAG, SPIVA, CRA limits) still rely on `review_by` and
  `knowledge check`; there is no safe API to fetch them.
- A new live series needs an entry in `SERIES`, a plausible range, a
  `max_age_days` and a test.
- Hardening after review round 1 (2026-11-08):
  - Ranges are plausible bands. The 1-year GIC is 0.2-10%.
  - Each series has its own age limit: 10 days for daily series, 14 for
    weekly GICs and 75 for monthly CPI.
  - A failed series keeps its last good value until that value is too old.
  - An offline run tries once a day.
  - curl is HTTPS-only and capped at 50 MB.
  - A move is blocked when a cited fact is missing, or when its `review_by`
    date is past or does not parse.
  - Risk warnings stay visible even on stale data.
  - A horizon of NaN, 0 or less counts as money needed soon.
  - A move whose numbers are not finite becomes an unknown and is not ranked.
