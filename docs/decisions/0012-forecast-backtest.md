# 12. A forecast model change ships only when it lowers the backtest miss

- **Status**: accepted
- **Date**: 2026-11-01

## Context

The owner asked the forecast to learn from what went differently from the
last forecast. No earlier forecast had been saved, so there was nothing to
compare. A change that only looks better on the data it was fitted to can
make the real forecast worse.

## Decision

- Each `forecast` run saves its monthly lines in `data/forecasts/<date>.json`.
  `bin/cashcast backtest` scores the last saved forecast against the months
  imported since.
- `backtest` also forecasts from every past month with at least 12 months of
  history before it, using only that history (rolling origin, Hyndman and
  Athanasopoulos, FPP3 §5.10). It reports, per line, the mean absolute miss,
  the mean miss (sign) and the WAPE, and the misses by calendar month.
- A model change is pre-registered (metric, threshold, pivot) and ships only
  when it lowers the backtest miss on the same origins.

## Tried and rejected (2026-11-01, owner data, 18 to 24 origins)

- A spending method per category (trailing 12, 6 or 3 months, or the same
  month last year), picked by the backtest. Learned out of sample, it made
  the spending miss 30 % larger. Lumpy categories reward no short window.
- Taxes repeating the same calendar month on record when `[taxes]` is empty.
  The net cash flow miss grew 23 %: tax payments differ too much between years.

## Consequences

- The misses that remain come from flows the owner decides: large transfers,
  crypto sales, tax owing. They belong in `[taxes]` and `[extra_income]`, not
  in a history rule.
- `backtest.py` stays Python next to the model until `rust-3-forecast` ports
  `forecast/` as a whole (decision 0010).
