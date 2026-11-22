# Credit card strategy (Canada)

Get the most cash back for the least effort. Data: `data/credit_cards.toml`, `data/banks.toml`, `data/payment_services.toml`.

Last checked: 2026-11-15

## The core rule

- Route each spending category to the card with the **highest net rate** for it.
- Net rate = earn rate − any fee you pay because of that card (FX fee, service fee).
- Count the annual fee once per year. Divide it by your yearly spend on that card to see its cost per dollar.
- Two or three cards are usually enough. More cards add mistakes and missed payments.

## Caps and reset dates

- Many cards drop to a low rate after a cap. Examples:
  - Scotia Momentum Visa Infinite: 4% on the first 25,000 per year of groceries + recurring. The year follows statement periods from the account-opening month: the November statement holds the annual summary, and the next period starts a new year.
  - BMO CashBack World Elite: 5% groceries on the first 500 per month.
  - National Bank World Elite: 5x groceries + dining on the first 2,500 per month.
- Track spend per cap. When a cap is hit, move that category to the next-best card until the reset.
- Reset periods differ: monthly (statement), calendar year, or card year (from the account open date). Check `cap_reset_note` for each card.

## Category traps

- Merchant codes decide the category, not the store's name. Costco and Walmart often code as warehouse or discount stores, not groceries.
- Tangerine cards: you **pick only 2** bonus categories (3 if rewards go to a Tangerine Savings Account).
- Points cards (Scene+, MBNA, National Bank, PC Optimum) are worth less as statement credit than as travel or in-store redemptions. The TOML stores a cash value; read the card's `notes`.

## Rent by credit card (Chexy)

- Chexy charges about 1.75% on Canadian Visa and Amex cards.
- It is worth it only when your card earns more than 1.75% on it, or when it helps you hit a welcome-bonus spend.
- Scotia Momentum: from Oct 22, 2026 rent (MCC 6513) and tax (MCC 9311) are not recurring bills, so Chexy rent earns 1% there, less than the fee. TD Cash Back Visa Infinite still lists 3% on bills.
- Most issuers code it as a normal purchase. Test with one payment first. If your card treats it as cash-like, you pay cash-advance fees and interest.
- Credit Builder (rent reporting to Equifax) can help a thin credit file.

## Foreign currency

- Most Canadian cards add a **2.5%** FX fee on foreign purchases.
- Use a 0% FX card abroad and for USD online shops:
  - Wealthsimple Visa Infinite + / Privilege (2% back, 0% FX)
  - Scotiabank Passport Visa Infinite (0% FX)
  - Home Trust Preferred Visa (0% FX, but no cash back on foreign purchases)
- Brim World Elite now charges 1.5% FX (it was 0% at launch).
- At checkout, always pay in the local currency. "Pay in CAD" offers (dynamic currency conversion) add a hidden markup.
- PayPal conversion costs 3–4%. Pay in the seller's currency on a 0% FX card instead.

## Keep cards active

- Use each card at least every few months. Issuers close idle accounts.
- A small recurring charge plus autopay keeps a card alive with no effort.

## Product change instead of cancel

- To drop a fee card, ask the issuer to **product change** to a no-fee card.
- This keeps your oldest account and your credit limit. Closing can lower your credit score.

## Welcome bonuses

- Bonuses can be worth more than a year of normal earn.
- Read the spend requirement and the time window. Do not overspend to hit it.
- Many issuers limit bonuses if you held the same card recently.
- Space applications out. Avoid new cards in the 6–12 months before a mortgage.

## Fee rebates through bank packages

- Some chequing packages rebate a card's annual fee:
  - Scotiabank Ultimate Package: up to 150 per year on one card (Passport, Momentum and others).
  - TD All-Inclusive: up to 139 per year on the TD Cash Back Visa Infinite and others.
  - CIBC Smart Account tier 3 (100,000+ balances): up to 139.
  - RBC VIP: full fee on several RBC premium cards.
- These packages charge a monthly fee unless you keep a minimum balance. See `banking.md` for the break-even math.
- Wealthsimple waives its card fee with 100,000+ in assets or a 4,000+ monthly direct deposit.

## Sources

- https://www.scotiabank.com/ca/en/personal/credit-cards/visa/momentum-infinite-card.html
- https://www.scotiabank.com/ca/en/personal/credit-cards/visa/passport-infinite-card.html
- https://www.scotiabank.com/ca/en/personal/bank-accounts/chequing-accounts/ultimate-package.html
- https://www.td.com/ca/en/personal-banking/products/bank-accounts/chequing-accounts/all-inclusive-banking-plan
- https://www.wealthsimple.com/en-ca/credit-card
- https://www.tangerine.ca/en/products/spending/creditcard/money-back
- https://www.nbc.ca/personal/mastercard-credit-cards/world-elite.html
- https://brimfinancial.com/personal
- https://www.paypal.com/ca/for-you/fees
- https://milesopedia.com/en/reviews/products/chexy-earn-rewards-rent-payments/
- https://www.canada.ca/en/financial-consumer-agency/services/credit-reports-score/improve-credit-score.html
