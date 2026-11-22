# Stock optimizer

`bin/cashcast optimize` answers one question: with this new cash, what do I
buy, and in which account, so my portfolio moves toward my own target weights?
It does not pick stocks or predict returns. You place every trade.

## Inputs

An `[optimize]` section in your `profile.toml`, or a file passed with
`--input FILE`. The `optimize-stocks` skill (`skills/optimize-stocks/`) asks for
each value and writes the section for you.

| Key | Meaning |
|---|---|
| `[optimize.cash]` | New money per account, in CAD: `tfsa`, `rrsp`, `fhsa`, `non-registered`, … |
| `[[optimize.targets]]` | `symbol`, `weight` (all weights add up to 1), `asset_class` (`equity`, `bond`, `cash`), `listing` (`CA`, `US`), optional `price` (for whole units) and `accounts` (where it may go, in order) |
| `[[optimize.holdings]]` | Optional: `account`, `symbol`, and `value` or `units`. Without it, the holdings from your imported exports are used, valued at the target prices |
| `allow_sell` | `false` by default. `true` sells what is above target (registered accounts first) |
| `min_trade` | Skip buys smaller than this |

## How it decides

1. **How much of each symbol.** Without selling, the buys make the squared
   gap to the target weights as small as the cash allows. This is
   "water-filling": the most underweight symbol gets money first, until it is
   as underweight as the next one, and so on. With enough cash, every symbol
   reaches its target. With `allow_sell`, every symbol moves to its exact target.
2. **Which account.** Each buy goes to the first account in its list that
   still has cash:
   - bonds: RRSP, then TFSA, then non-registered (interest is taxed fully);
   - US-listed stocks: RRSP first (the US–Canada treaty waives US withholding
     tax on dividends in an RRSP, not in a TFSA);
   - Canadian-listed stocks: TFSA first.
   A target's own `accounts` list wins, for example a group RRSP that holds
   only the plan's funds. An account cashcast does not know (`group-rrsp`,
   `lira`, …) gets only the targets that name it; otherwise its cash stays,
   with a note.
3. **Whole units** when a price is given, and no buy under `min_trade`. Cash
   that rounding leaves buys one more unit at a time, of the symbol it brings
   closest to its target, while a unit still helps (its price is under twice
   the gap). Sales are whole units too. Each symbol is listed once.

The report shows the buys per account, the weights before and after, the
largest gap to target before and after, and the cash left.

## Checks

`engine/tests/app_strategy_math.rs` runs 300 random portfolios. For each one it
checks that no account spends more cash than it has, that nothing is sold
unless sales are allowed (and never more than is held), and that a target
lands only in the accounts it may use. Without prices, a minimum trade or
restricted accounts, it also checks that all the cash is placed and that the
largest gap to target never grows. With whole units, the gap can grow a
little: a small amount of cash may buy one fund but not a unit of the other.
