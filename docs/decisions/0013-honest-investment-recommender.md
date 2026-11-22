# 13. The investment recommender is honest by construction

- **Status**: accepted
- **Date**: 2026-11-01

## Context

The owner asked for "the best investments recommender, but a very honest one"
that takes their investments, recommends moves for the best gains, and lets
them question and override each one. Most "best investment" tools sell
certainty: an expected return as one number, a stock pick, a timing call. The
evidence for a Canadian individual does not support that. What it does
support is small, boring and mostly certain: lower fees, the right account for
each asset, registered room, no idle cash above the reserve, no superficial
losses, and less concentration in one stock. VISION.md already rules out stock
picks and any agent moving money.

## Decision

`bin/cashcast invest-plan` (section `invest_plan`, engine `invest-plan`)
follows these rules, and its tests and evals enforce them:

1. **Evidence only.** A move exists only when facts in
   `knowledge/data/invest_evidence.toml` (or `investing.toml`) back it. Each
   move prints every fact it uses with `confidence`, `as_of`, `review_by` and
   its source, and flags a fact past its review date. A missing fact is shown
   as "unsupported", never hidden.
2. **Ranges, not forecasts.** A yearly gain is a range. Return assumptions are
   the FP Canada / Institute of Financial Planning long-run planning numbers,
   widened by a `return-band` that is marked as a judgment call. A sure gain
   (a fee cut) still shows a range, to allow for MERs that lag fee changes.
3. **Risk and cost on every move.** Each move states its risk, its known
   one-time cost (tax on a sale, conversion fees) and what is unknown.
4. **Risk moves are not gain moves.** Selling down employer stock and
   rebalancing have an expected gain of about $0; they are listed apart, so a
   risk cut never looks like free money.
5. **Do nothing is ranked.** "Do nothing" sits in the same table with a $0
   gain and its own risks (cash losing to inflation, concentration). When no
   move clears the owner's `simplicity_threshold` after costs, it ranks first
   and the plan says so.
6. **Near money stays safe.** When the money is needed within the
   `short-horizon` (a home purchase in `mortgage_in_months`, or
   `[invest] horizon_years`), cash above the reserve goes to a safe account at
   a better rate, never to stocks.
7. **The owner decides.** `[[invest.overrides]]` (a move id, a kind like
   `fee`, or a symbol) with a `reason`, and `[invest] target_equity` with a
   `target_reason`, are respected: the move gets no task and its estimated
   cost is shown once under "Your choices", with no reminders. The
   `invest-challenge` skill answers challenges from the cited evidence, says
   when the owner's argument holds, and records an override only after a yes.
   A safety rule (reserve, debt, horizon, missing or stale evidence) beats
   an override: the move stays under "Blocked" with no task.
8. **No picks, no timing, no money moves.** The plan never names a stock to
   buy (only a cheaper equivalent index fund for a fee move) and never says
   when to trade. Each move worth doing becomes a one-time `money` task.

The ranking logic lives in Rust (`engine/src/invest_plan.rs`, decision 0010);
Python only gathers aggregates into `data/invest-plan-input.json` (mode 600).

## Consequences

- The plan is modest on purpose. For a well-run portfolio it often says
  "do nothing", and that is a correct result, not a gap.
- Market values per holding come from `[invest.prices]`, the watchlist, or
  account totals recorded with `bin/cashcast api record`. Without them, fee,
  tax and loss moves say the value is at cost and the tax is unknown.
- A new move needs a fact in `invest_evidence.toml`, a Rust test, and an eval
  case when it changes what the agent may say.
- `invest-plan` has no row in `tests/parity/commands.txt` yet: its logic is
  already in Rust, and the parity list is changed only by the port tasks.
