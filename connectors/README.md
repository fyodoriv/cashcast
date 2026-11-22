# Connectors

Each file here describes one service a Canadian user may bank, invest, pay or plan with:

- `<id>.toml`: what the service exports, which engine format reads it, its API, and the actions an agent may help with.
- `playbooks/<id>.md`: plain steps for the user or an agent driving a browser.

Rules that apply to every connector:

- The agent never types passwords or 2FA codes. The user signs in, or a password manager fills them with the user's approval.
- Every action that moves money (`risk = "money"`) is `via = ["human"]`. The agent creates a reminder; the user acts.
- Every settings change (`risk = "settings"`) needs the user's OK for the exact change before the final click.
- Exports reach the inbox one of two ways, and the user picks each time: they download them, or they approve and an agent fetches them in a browser (`skills/fetch-exports`; `bin/cashcast fetch plan` and `fetch collect`).
- Exports go into `private/users/<you>/inbox/`. API keys go into `private/users/<you>/.env`. Neither is ever committed.

## Services

| Service | Kinds | Import format(s) | API | Browser playbook | Money actions are human-only |
|---|---|---|---|---|---|
| [MoneyWiz](moneywiz.toml) | budgeting | `moneywiz-csv` | No | [moneywiz.md](playbooks/moneywiz.md) (desktop app) | Yes (none listed) |
| [Scotiabank](scotiabank.toml) | bank, card | `scotiabank-csv`, `scotiabank-legacy-csv`, `ofx`, `scotiabank-visa-pdf`, `scotiabank-chequing-pdf` | No | [scotiabank.md](playbooks/scotiabank.md) | Yes |
| [CIBC](cibc.toml) | bank, card | `ofx` | No | [cibc.md](playbooks/cibc.md) | Yes |
| [TD Canada Trust](td.toml) | bank, card | `td-csv`, `ofx`, `td-credit-pdf`, `td-chequing-pdf` | No | [td.md](playbooks/td.md) | Yes |
| [RBC Royal Bank](rbc.toml) | bank, card | `rbc-csv`, `ofx` | No | [rbc.md](playbooks/rbc.md) | Yes |
| [BMO Bank of Montreal](bmo.toml) | bank, card | `generic-csv`, `ofx` | No | [bmo.md](playbooks/bmo.md) | Yes |
| [Tangerine](tangerine.toml) | bank, card | `generic-csv`, `ofx` | No | [tangerine.md](playbooks/tangerine.md) | Yes |
| [Wise](wise.toml) | bank, fx, payments | `wise-statement-csv` | Yes, personal token | [wise.md](playbooks/wise.md) | Yes |
| [Wealthsimple](wealthsimple.toml) | bank, card, broker, crypto | `wealthsimple-statement-csv`, `wealthsimple-activity-csv` | No | [wealthsimple.md](playbooks/wealthsimple.md) | Yes |
| [Casa](casa.toml) | crypto | `crypto-generic-csv` | No | [casa.md](playbooks/casa.md) | Yes |
| [Kraken](kraken.toml) | crypto, fx | `kraken-ledger-csv` (fiat legs also make MoneyWiz rows) | Yes, API key | [kraken.md](playbooks/kraken.md) | Yes |
| [Chexy](chexy.toml) | rent, payments | none (charges show on the card) | No | [chexy.md](playbooks/chexy.md) | Yes (none listed) |
| [PayPal](paypal.toml) | payments | `paypal-activity-csv` | Yes, OAuth2 | [paypal.md](playbooks/paypal.md) | Yes |
| [Paysend](paysend.toml) | payments, fx | none (charges show on the card) | No | [paysend.md](playbooks/paysend.md) | Yes |
| [Google Sheets](google-sheets.toml) | sheets | `generic-csv` | Yes, OAuth2 | [google-sheets.md](playbooks/google-sheets.md) | Yes (none listed) |
| [E*TRADE](etrade.toml) | broker | `etrade-transactions-csv` (also the API's JSON pages from `api download etrade`), `etrade-stockplan-csv`, `etrade-stockplan-pdf` | Yes, OAuth1 | [etrade.md](playbooks/etrade.md) | Yes |
| [T-Bank (Tinkoff)](tinkoff.toml) | bank, card | `tinkoff-csv` (Windows-1251) | No | [tinkoff.md](playbooks/tinkoff.md) | Yes |
| [Sun Life](sunlife.toml) (group retirement) | retirement | `sunlife-transactions-csv` (layout estimated), `sunlife-statement-pdf` | No | [sunlife.md](playbooks/sunlife.md) | Yes |

MoneyWiz is optional. Without it, the bank and card downloads in the inbox are the record: `weekly` skips the MoneyWiz step (US-37).

Banks and Sun Life have a `read-balances` action (Wise: `check-balances`) for `bin/cashcast transfers` (`skills/bank-balances`).

Facts were checked on 2026-10-18. Each TOML has `[meta] review_by`, `sources` and `confidence`. `confidence = "estimate"` means at least one fact came from a third-party page or could not be confirmed; its `notes` say which.

## Add a connector

1. Copy the closest `<id>.toml`. Rename it to the new service id (lowercase, dashes).
2. Keep the schema. Fill every key, even when empty (`""` or `[]`). Do not add or rename keys.
3. Use only these `kinds`: bank, card, broker, crypto, payments, fx, rent, budgeting, notes, sheets, retirement (workplace plans).
4. Use only engine format labels in `[import] formats`. If the service has no export, leave `formats = []` and explain why in `no_export_reason`. When the format is shared (OFX), add `ofx_orgs` (the `<ORG>` its files send) and `file_prefixes` (its default download name) so `fetch plan` can credit the file to the service.
5. Add 3 to 8 `[[actions]]`. Set `risk` to `read`, `settings` or `money`. A `money` action must have `via = ["human"]` and nothing else.
6. Write `playbooks/<id>.md` with these sections: Before you start, Export your data, Actions (one subsection per action id), API, Sources, and `Last checked: <date>`.
7. If the service needs a new import format, add a synthetic fixture and a test in the engine. Use made-up rows with canary strings in the descriptions. Never use real exports.
8. Cite official help or API pages in `[meta] sources`. Set `confidence = "estimate"` if you could not confirm a fact, and say what in `notes`.
9. Run the registry test. It fails when a field is missing, a money action is not human-only, a service without formats has no `no_export_reason`, or a playbook path does not exist.

Quick local check:

```sh
bin/cashcast doctor   # the last lines check every connector spec
```
