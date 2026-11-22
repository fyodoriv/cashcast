# Learnings

Predicted vs observed, for changes and for the assistant's own advice. A row
is written before the result is known (the prediction), then completed.

## Changes to this repo

| Date | Change | Predicted | Observed | Keep / pivot |
|---|---|---|---|---|
| 2026-10-18 | Evolve cashcast into a multi-user financial assistant (PR `feat/financial-assistant`) | standards score 4/32 → 32/32; goal coverage 11/11; all guards green | see the PR's validation block | pending |

## The assistant's advice

The app keeps its own predicted-vs-observed record per user in
`private/users/<name>/data/journal.jsonl`: next month's spending, predicted by
each strategy run and scored by each review. Two misses over 15% in a row
flag a pivot in `bin/cashcast review`. Shared lessons (no private numbers)
go here:

| Date | Lesson | Evidence | Change made |
|---|---|---|---|
| 2026-10-18 | Count rent charges per card instead of tracking cap dollars | the owner's hand-made plan used this rule | `cashback.rent_charges` prints "card × N" |
| 2026-10-18 | A wrong cap-reset guess (Jan 1) made the plan switch cards late | the owner's plan archive | caps carry `period` and `cap_reset_note`; profile can override per card |
