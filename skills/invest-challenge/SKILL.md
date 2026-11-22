---
name: invest-challenge
description: Lets the owner question, challenge or override any move from `bin/cashcast invest-plan` (the honest investment recommender). Answers only from the evidence the plan cites, says plainly when the owner's view is reasonable, changes the recommendation when their argument holds, and records an override with its reason in profile.toml after a yes. Use when the user says "challenge <move id>", "why should I sell my employer stock", "I disagree with this move", "I want to keep X", "use a different stock mix", or "skip this recommendation". Don't use to pick stocks, predict returns, time the market, or place trades.
---

# Challenge an investment move

`bin/cashcast invest-plan` ranks moves. Each move has an id (`cash-drag`,
`fee:VDY` (or `fee:VDY:tfsa` when a fund is in two accounts), `concentration:EMPL`, `location:VTI`, `harvest:XYZ`, `fill-room`,
`rebalance`, `crypto`), a gain range, a why, its risk, its cost, the evidence it cites
from `knowledge/data/invest_evidence.toml`, and "do nothing" ranked on the
same terms. The honesty rules are in `docs/decisions/0013-honest-investment-recommender.md`.

The owner decides. Your job is to make their decision well informed, not to
win the argument.

## Steps

1. **Read the move.** Run `bin/cashcast invest-plan` (it needs the
   `agent:strategy` consent; if the report is withheld, tell the owner the
   `consent grant` command and stop). Find the move by its id. Never open
   `data/`, `inbox/`, `out/` or `docs/` in the user folder.
2. **Restate the challenge** in one sentence, so the owner can correct you.
3. **Answer from the cited evidence only.** Quote the fact id, its
   confidence, and its `as_of` date. Say what the evidence does not cover.
   No forecast, no "will", no "guaranteed": every gain is a range from
   long-run planning assumptions.
4. **Judge the argument honestly.** Put it in one of three boxes and say which:
   - **It holds**: the owner has a fact or circumstance the plan lacks (a
     vest next month, money needed for a home, a cheaper broker, a pension
     that already holds bonds, a tax situation). Say so plainly, and change
     the recommendation: propose an override, a profile fact
     (`[invest] horizon_years`, `[invest.prices]` with `prices_as_of = YYYY-MM-DD`; prices older than 30 days block sell moves), or a knowledge fix.
   - **It is reasonable**: a preference the evidence cannot settle (belief in
     the company, wanting simplicity, accepting more risk). Say it is a
     reasonable choice, show the estimated cost from the plan, and offer the
     override. Do not repeat the case against it.
   - **The evidence disagrees**: a claim that a source contradicts (a stock
     "can't fall", "I can time the dip"). Say which fact disagrees, once,
     kindly, then respect the owner's call.
5. **Record an override only after a yes.** Show the exact change, ask, and
   only then edit the owner's `profile.toml` (a `settings` action):

   ```toml
   [[invest.overrides]]
   move = "concentration:EMPL"   # a move id, a kind ("fee") or a symbol; a risk move: its exact id only, with a reason
   reason = "Selling after the vest in March, when the gain is small"
   until = "2027-03-31"          # after this date the move comes back; required when the reason ends on a date
   ```

   For a different stock mix, add these two keys under the owner's
   existing `[invest]` table. Add an `[invest]` header only when the file
   has none: a second `[invest]` header breaks the whole profile.

   ```toml
   target_equity = 0.9
   target_reason = "30-year horizon, pension covers the basics"
   ```

   After the edit, run `bin/cashcast config` to confirm the profile still
   loads.

   Rerun `bin/cashcast invest-plan` and show the "Your choices" line: the
   move is skipped with its estimated cost, and no task remains. A move
   under "Blocked by a safety rule" stays there even with an override: the
   rule wins on purpose, and the move already has no task. Tell the owner
   the override is saved, and that the block lifts only when its cause does
   (for example, setting `horizon_years`).
6. **When a fact is wrong**, do not edit `knowledge/` from a chat claim. Ask
   for the source, then follow `skills/knowledge-update/SKILL.md`. The owner
   may pin a private value in their own `knowledge/invest_evidence.toml`.

## Never

- Name a single stock to buy, or say when to buy or sell.
- Move money, trade, or log in to a broker. A move is a one-time task the owner does.
- Say a gain is certain, or drop the range, the risk or the cost.
- Nag about an override: show its cost once, in "Your choices".

## Example

> **Owner:** challenge concentration:EMPL. My RSUs vest in 3 weeks; selling
> now and buying back later is silly.
>
> **You:** You want to keep EMPL until the vest. The plan rates this a risk
> move with an expected gain of about $0 (`single-stock-risk`, verified, 2018).
> Your point holds in part: shares sold right after a vest have almost no
> gain, so waiting 3 weeks costs little tax-wise. The risk stays: a 50% fall
> would cost what the plan shows. Want me to add an override with your reason,
> `move = "concentration:EMPL"`, `reason = "sell after the vest in 3 weeks"`,
> `until = "<vest date + 7 days>"`, so the move comes back after the vest?
> The plan will skip it and show its cost once. Education and arithmetic on your own data, not personal financial advice; not a licensed financial advisor.
