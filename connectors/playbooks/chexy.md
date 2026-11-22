# Chexy playbook

## Before you start

- You need a Chexy account and a web browser.
- The agent never types passwords or 2FA codes. You sign in yourself, or your password manager fills them after you approve.
- The agent never pays rent or moves money.

## Export your data

Chexy has no transaction export. Rent and the Chexy fee are charged to your credit card. Import that card's bank export instead (for example the TD or Scotiabank playbook). Nothing from Chexy goes into `private/users/<you>/inbox/`.

## Actions

### change-rent-payment-card

1. Sign in from https://chexy.co. Open the Dashboard and the recurring rent payment.
2. Open the payment method and pick the new card. Check the fee for that card network on screen (fees and eligible networks changed in 2026).
3. Do it at least 5 days before rent is due. The card is charged a few days before the due date.
4. Ask the user to confirm this exact change before clicking the final button.

### check-next-charge

1. Open the Dashboard and the recurring rent payment.
2. Read the next charge date, the rent amount and the fee.
3. Report them. Do not report the landlord's details.

### check-credit-builder-status

1. Open the Credit Builder section.
2. Read whether it is active, the plan, and the last reported month.
3. Report the status.

### renew-or-cancel-membership

1. Open the membership or plan section.
2. Read the price, the renewal date, and what is lost on cancel (for example Credit Builder reporting).
3. Ask the user to confirm this exact change before clicking the final button.

### change-rent-amount-or-date

1. If Credit Builder is active, stop. Chexy says this needs its support team. The user contacts support.
2. Otherwise, open the recurring payment and edit it. Finish at least 4 business days before the due date.
3. Ask the user to confirm this exact change before clicking the final button.

## API

Chexy has no public API.

## Sources

- https://intercom.help/chexy/en/articles/10769922-pausing-reactivating-deleting-cancelling-payments
- https://intercom.help/chexyrent/en/articles/10019878-can-i-update-my-rent-amount-or-rent-due-date-with-an-active-credit-builder
- https://intercom.help/chexyrent/en/articles/10807179-paying-your-rent-with-chexy
- https://intercom.help/chexy/en/articles/11072897-payment-methods-pad-e-transfer-bill-pay-compared
- https://www.savvynewcanadians.com/chexy-review/
- https://www.neobanc.com/articles/chexy-alternatives

Last checked: 2026-10-18
