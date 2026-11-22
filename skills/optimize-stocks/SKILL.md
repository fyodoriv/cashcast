---
name: optimize-stocks
description: Works out where the person's new money goes so their portfolio moves toward their own target weights, across TFSA, RRSP, FHSA and non-registered accounts, with buys only by default (sales only when they allow them). Use when the user says "optimize my stocks", "where should I put this money", "rebalance", "what should I buy with my contribution", or gives target weights and cash. Don't use to pick stocks, predict returns, or place trades.
---

# Optimize stocks

`bin/cashcast optimize` takes the person's own inputs and returns a buy (or
sell) list per account. It is arithmetic on their targets, not advice and not
a stock pick. The person places every trade.

## Inputs

Ask one question at a time, with a privacy line ("this stays in your
profile.toml on this computer"):

1. Target weights per fund or stock (they add up to 1), and for each one the
   asset class (`equity`, `bond`, `cash`) and listing (`CA` or `US`). If they
   have no targets, run `bin/cashcast invest` and use its core fund as a start.
2. New cash per account (CAD). Check room first: `bin/cashcast tax`.
3. Holdings: the default reads `bin/cashcast holdings` (imported exports) and
   values them with a `price` per target. Or ask for a value per account.
4. Whether sales are allowed (`allow_sell`, default no; sales in a
   non-registered account may realize a capital gain), and a minimum trade.

Write them to the person's `profile.toml` (you may edit it):

```toml
[optimize]
min_trade = 50
allow_sell = false

[optimize.cash]
tfsa = 7000
rrsp = 3000

[[optimize.targets]]
symbol = "XEQT"
weight = 0.8
asset_class = "equity"
listing = "CA"
price = 35.20

[[optimize.targets]]
symbol = "ZAG"
weight = 0.2
asset_class = "bond"
price = 13.50
```

Optional per target: `accounts = ["rrsp"]` to limit where it may go (for
example a group RRSP that holds only the plan's funds). Optional per holding,
instead of the imported ones: `[[optimize.holdings]] account = "tfsa"`,
`symbol`, and `value` or `units`.

## Run and explain

1. `bin/cashcast optimize` (or `--input FILE`). It needs the
   `agent:strategy` consent to show you the plan; otherwise the person reads
   it in Terminal.
2. Explain: the buy list per account, why each account (bonds to the RRSP,
   US-listed stocks to the RRSP, Canadian-listed stocks to the TFSA), the gap
   to target before and after, and the cash left.
3. The person places the trades. Never log in to a broker to trade, even if asked.
