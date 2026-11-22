# Knowledge

Public, committable reference facts the assistant reads on demand.
It holds no personal data. Your own numbers stay in `private/`.

This is education, not personal financial, tax or legal advice.
The assistant is not a licensed advisor.

Last checked: 2026-10-18

## Guides (Markdown)

| File | Topic | Main sources |
|---|---|---|
| `credit-cards.md` | Route spend to the best card, caps, rent via Chexy, 0% FX, product change, fee rebates | Issuer card pages, FCAC |
| `canada-tax.md` | Brackets, RRSP vs TFSA, FHSA, HBP, capital gains, ACB, superficial loss, crypto, T1135, GST/HST, CPP/EI, instalments | CRA (canada.ca) |
| `insurance.md` | Term life, disability, tenant/home, auto, travel medical, gap checks | FCAC, IBC, travel.gc.ca |
| `corporate-stock.md` | RSUs, ESPP, options, US-listed shares, concentration | CRA, IRS |
| `investing.md` | Low-fee core ETFs, account order, asset location, rebalancing | CRA, ETF providers |
| `crypto.md` | Registered platforms, costs, tax, safety | FCNB/CSA list, platform fee pages, CRA |
| `credit-score.md` | Utilization, payment history, account age, mortgage prep, free reports | FCAC |
| `banking.md` | Monthly fee vs minimum balance break-even, card-fee rebates | Bank package pages |

## Data (TOML, read by the assistant)

| File | Topic | Main sources |
|---|---|---|
| `data/tax_ca_2026.toml` | 2026 federal and 13 provincial/territorial brackets, BPAs, Ontario surtax, TFSA/RRSP/FHSA/HBP limits, CPP/EI/QPP, deadlines | CRA rates page, CRA T4127, CRA limits table |
| `data/tax_deductions.toml` | Common deductions, credits and benefits with line numbers and records to keep | CRA line pages |
| `data/foreign_cash.toml` | USD and EUR to CAD rates (dated; refresh monthly; USD uses the saved live Bank of Canada rate when it is newer), Norbert's gambit cost, US withholding in an RRSP, T1135 threshold, foreign interest, Wise Interest per currency, which API services hold cash or crypto | Bank of Canada Valet, CRA, broker guides, wise.com/ca/interest |
| `data/credit_cards.toml` | 24 Canadian cards: fees, FX, income, earn by category, caps, insurance | Issuer pages (reviewers where issuer pages were unreadable) |
| `data/banks.toml` | 20 chequing accounts (with Wise CAD): fees, balance waivers, card rebates, interest | Bank pages, wise.com/ca/interest |
| `data/payment_services.toml` | Chexy, Paysend, Wise, PayPal, Interac e-Transfer, Kraken and NDAX funding | Service fee pages |
| `data/merchants.toml` | Billing names per merchant (Anthropic, OpenAI, Netflix, Apple, Google, Spotify, Amazon Prime) that `charges` groups as one | Merchant help pages; descriptors are estimates |
| `data/investing.toml` | 21 core ETFs with MERs, account priority, asset location | iShares, Vanguard, BMO, Global X, CRA |
| `data/invest_evidence.toml` | Facts behind `invest-plan`: long-run return assumptions, the band around them, fees, withholding, FX, superficial loss, concentration, rebalancing, short horizons | FP Canada PAG 2026, SPIVA Canada, Morningstar, Bessembinder 2018, Vanguard, CRA, Wealthsimple |
| `data/crypto_exchanges.toml` | 9 crypto platforms: registration, fees, withdrawals, staking | FCNB registry, platform fee pages |
| `data/insurance.toml` | Insurance rules of thumb and gap checks | FCAC, IBC, card pages |
| `data/corporate_stock.toml` | Employer equity rules | CRA, IRS |
| `data/credit_score.toml` | Credit score rules | FCAC |

## How facts are versioned

Every TOML file starts with a `[meta]` table:

- `as_of`: the date the facts were checked against their sources.
- `review_by`: the date they must be checked again. After it, treat the file as stale.
- `sources`: the main pages used for the file.

Every entry that holds facts also has:

- `source`: the page that supports that entry.
- `confidence`:
  - `"verified"`: read on a primary source (CRA, issuer, provider) on the `as_of` date.
  - `"estimate"`: best value, but from a secondary source or a partly readable page. Confirm before relying on it.
  - `"heuristic"`: a rule of thumb (for example "life cover of about 10x income"). It is judgment, so no official source states it.

Rates are decimals (`0.04` = 4%). Amounts are CAD unless the note says otherwise.

## How to propose a change

1. Edit the TOML entry. Put the new value **and** a `source` URL that shows it.
2. Set `confidence` to `"verified"` only if you read the value on a primary source. Otherwise use `"estimate"`, or `"heuristic"` for a rule of thumb.
3. Update `as_of` in `[meta]` if you re-checked the whole file.
4. Run `bin/cashcast knowledge check` to validate the files.
5. Open a pull request. Say what changed and link the source.

Live facts: `bin/cashcast knowledge refresh` reads official Bank of Canada series
(decision 0015) and flags a planning fact in `invest_evidence.toml` that is more
than 1.5 points from today's value. A flag is a prompt to review the source at its
next release, not an automatic edit: hand facts change only through `knowledge propose`.

Never put personal amounts, balances, account numbers or names in this folder.

## Sources

Each file lists its own sources. The main primary sources are:

- https://www.canada.ca/en/revenue-agency/services/tax/individuals/tax-rates-brackets/current-year.html
- https://www.canada.ca/en/revenue-agency/services/tax/registered-plans-administrators/pspa/mp-rrsp-dpsp-tfsa-limits-ympe.html
- https://www.canada.ca/en/revenue-agency/services/forms-publications/payroll/t4127-payroll-deductions-formulas/t4127-jan/t4127-jan-payroll-deductions-formulas-computer-programs.html
- https://www.canada.ca/en/financial-consumer-agency/services/credit-reports-score/improve-credit-score.html
- https://fcnb.ca/en/check-licence-and-registration/registered-crypto-asset-trading-platforms
