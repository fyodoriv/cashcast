---
name: balances
description: Refreshes the balances behind the money strategy for every investment, crypto and foreign-currency source in connectors/ (Wise, Kraken, PayPal, E*TRADE by API; Wealthsimple, Casa, Sun Life and any blocked API in the browser), API first and browser second, then runs `bin/cashcast strategy`. Use when the user says "update my net worth", "refresh my investments", "count my Wealthsimple / crypto / USD", or before the monthly review. Don't use for bank chequing balances and e-Transfers (use bank-balances), to read exports (use fetch-exports), or to buy, sell or move anything.
---

# Balances for the strategy

You read; the code converts to CAD and plans; the person moves every dollar.

## 1. Before you start

1. `bin/cashcast consent list`. API reads need `actions:api`. Browser reads
   need `agent:browser-read` (1 day). The strategy needs `agent:strategy`.
   If one is missing, give the person the `bin/cashcast consent grant SCOPE`
   line for their own Terminal, then wait.
2. Which sources count, and as what: `[api_services]` in
   `knowledge/data/foreign_cash.toml`. Each service is `cash`, `exchange`
   (cash plus crypto) or `investments` (account values).

## 2. API first

For each service with a balance API (`bin/cashcast connectors show SERVICE`,
`[api] read` lists `balance` or `balances`) and its token in the person's
`.env`:

```bash
bin/cashcast api balances wise
bin/cashcast api balances kraken    # also saves each coin's price in CAD or USD
bin/cashcast api balances paypal
bin/cashcast api balances etrade
```

If a call fails (no token, expired token, HTTP error), say which one failed
and use the browser for that service. Never ask for a token in chat: the
person edits `.env` themselves.

## 3. Browser second

For each service with no API, or whose API failed:

1. Open its `login_url` (`bin/cashcast connectors show SERVICE`). The person
   signs in. Never type a password or a code.
2. Follow its playbook (`connectors/playbooks/<id>.md`):
   - Wealthsimple: `read-investment-balances`
   - Casa: `check-vault-balance` (browser only; no block explorer, by the
     owner's choice)
   - Sun Life: `read-balances`
   - Wise or Kraken when the API is blocked: `check-balances`
3. Record what you read:

   ```bash
   bin/cashcast api record SERVICE --set "NAME=AMOUNT,NAME=AMOUNT" [--currency USD]
   ```

   Use plain names (TFSA, RRSP, Personal, Crypto, BTC). Never use an account
   number: a name with 4 or more digits in a row is refused. Record a USD
   account in its own call with `--currency USD`.
4. If a site will not load after two tries, skip it and say so.

## 4. Run and tell

```bash
bin/cashcast strategy
```

Tell the person, from that output only:

- the reserve's cash, now including Wise and exchange cash in CAD;
- the market value per bucket (Invest section) and crypto at market;
- any warning: stale rates or prices, a currency with no rate, the T1135
  check, or records older than 31 days.

Never write amounts into code, notes, memory or commits. They stay in the
person's private folder.
