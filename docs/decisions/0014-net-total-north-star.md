# 14. The net total is the main number, and boundaries can refuse any option

- **Status**: accepted
- **Date**: 2026-11-01

## Context

The owner asked for one main metric: their net total, everything they have
minus everything they owe. Higher is better. Each section already found gains
(cash back, fees, tax, invested cash), but nothing compared them on one scale,
and nothing stopped a section from proposing a move the owner would never
make, such as more work hours or selling the home.

## Decision

- `cashcast-engine net-total` (engine/src/net_total.rs, decision 0010) sums
  one signed CAD value per source: bank accounts, card balances as debt, Wise
  in every currency, investments at market (at cost when no market value is
  recorded), crypto at the exchange price, and `[[net_total.items]]`. Python
  only gathers these aggregates, with group labels and never account names.
- Each run saves one point a day in `data/net-total-history.json`. The trend
  compares with the newest point at least 30 and 365 days old.
- Every section can offer options (`sec.data["net_options"]`); each has an
  effect a year on the net total. Before ranking, the engine checks each one
  against `[boundaries]` in `profile.toml`: work hours (no burnout), a cash
  floor with a margin (default 5 %, at most 20 %, like `[transfers] buffer`),
  risk, new debt, keeping the home, and refused tags. A refused option is
  never recommended. It still shows, with the reason and its effect, so the
  owner sees what each limit costs.
- Unknown work hours refuse any option that adds work. The safe default is
  "no".
- The net total leads the strategy document, `bin/cashcast` start output and
  the web app's home.

## Consequences

- An option ranks by its first-year effect (effect a year minus its one-time
  cost). Long-run compounding is not modelled; expected returns are the
  FP Canada projection assumptions, a planning number, not a promise.
- The investment recommender (`invest_plan`, decision 0013) offers each gain
  move as an option: the middle of its range a year, its known cost once.
  Its cash move replaces the net total's rough "invest cash above the floor".
- Money moves stay reminders; the net total never moves money.
