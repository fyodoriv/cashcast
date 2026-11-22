---
name: fetch-exports
description: Gets fresh exports from the person's banks, cards, brokers and crypto services (TD, Scotiabank, Wealthsimple, E*TRADE, Casa, Wise, Kraken, PayPal, Sun Life, and the rest in connectors/) into their cashcast inbox, either by guiding them to export the files themselves or, on their approval, by signing in through their password manager and downloading every stale export in the browser. Use when the user says "refresh my data", "get my exports", "download my statements", "fetch everything", when `bin/cashcast fetch plan` shows stale services, or when a strategy section says its data is old. Don't use to move money, trade, pay, change settings, or read the downloaded files.
---

# Fetch exports

The person always picks how data comes in. Ask every time; never assume.

## 1. See what is stale

Run `bin/cashcast fetch plan` (add `--all` for a first setup). It lists each
service, its newest export date, and its route:

- **api**: `bin/cashcast api download SERVICE` (needs `actions:api` consent and
  a token in `.env`, which only the person adds).
- **browser**: a login URL and the playbook action that downloads.
- **human**: the person exports it (no browser route).

Nothing stale? Say so and stop.

## 2. Ask once: export yourself, or let the agent fetch

One question (the structured question tool when it exists) with the stale
services listed, and these options:

1. **Fetch them for me.** The agent signs in through the password manager and
   downloads each export. The person approves each login in the password
   manager's own prompt and enters any 2FA code there. While the agent works,
   pages it opens (balances, recent payees) reach the AI model as screenshots.
   The export files are moved unopened.
2. **I'll export them myself.** Print the steps from
   `bin/cashcast connectors show SERVICE` for each stale service. Wait, then
   continue at step 4.
3. **Only some.** The person names which services to fetch. The rest get steps.

The approval covers read-only downloads in this run only. A settings change or a
money action is never part of it (`service-action` skill).

## 3. Fetch in the browser

1. Request every login in **one** password-manager call before you navigate
   (Claude in Chrome `request_credentials`: one entry per service, keywords from
   the service name and `login_url`; Sun Life and E*TRADE stock plans may use a
   work login, so ask which one). Load `enter_verification_code` in the same
   tool batch.
2. Per service, in a fresh tab:
   - open `login_url`, then call `autofill_credential` on the first sign-in page;
   - a code prompt: focus the field and call `enter_verification_code`;
   - a CAPTCHA, a phone-app approval, or "verify it's you" by email link: ask the
     person to finish it in that tab, then continue. Never solve a CAPTCHA;
   - follow the playbook action that `fetch plan` names
     (`connectors/playbooks/<id>.md`): pick the longest period it offers and a
     CSV or OFX/QFX format;
   - do not read balances, payees or account numbers aloud or into notes. Report
     only "downloaded" or "failed at step N".
3. Two failures on one service (sign-in loop, missing button): stop that service,
   note the step, and give the person its manual steps. Go on with the rest.
4. Release the credentials when done (`release_credentials`).

Never type a password or code yourself, and never paste one into chat.

## 4. Collect and import

1. Run `bin/cashcast fetch collect` (`--minutes N` if the run took longer than an
   hour). It moves new `.csv`, `.ofx`, `.qfx` and `.xlsx` files from Downloads
   into the inbox without opening them, and prints only counts.
2. Run `bin/cashcast import`. Report per service: its format label and the
   unreadable-row count. A wrong label or unreadable rows: fix the reader with a
   synthetic fixture of the same shape (`bin/cashcast columns` shows headers and
   masked shapes), never with the real values.
3. Run `bin/cashcast fetch plan` again. Every fetched service should read "fresh".

## Notion pages

The Notion export is a `.zip` and `fetch collect` leaves it alone. Follow the
`notion-import` skill. Its private default is that the person exports the zip
themselves. If they ask the agent to click Export, open only the page's `•••`
menu, and run `bin/cashcast notion import <zip> --delete-source`.

## What the person still does

Approve each password-manager prompt, enter 2FA codes there, finish CAPTCHAs,
add API tokens to `.env`, and run `consent grant`. Batch these into one short
list at the end.
