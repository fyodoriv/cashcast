---
description: Start cashcast (web app, refresh) and walk through what it needs from you now
allowed-tools: Bash(bin/cashcast:*)
---

Run `bin/cashcast` from the cashcast repo. It sets up what is missing, starts the local web app in the
background (or reuses the running one), reads new exports from the inbox, and prints what is coming up,
what it needs from the person, and the next action.

Then, answer first:

1. The web app line. In an agent the link is not printed (it holds an access token): tell the person to run
   `bin/cashcast` in their own terminal to open it.
2. Each "Needed from you" item, in plain words, with the exact command. Consent is granted only by the person,
   in a terminal (`bin/cashcast consent grant SCOPE`); never grant it yourself.
3. The next action. For a first run or a full session, follow `.claude/skills/cashcast/SKILL.md` from Step 0.

The Sunday session ("weekly", "do the weekly", `/cashcast-weekly`, `/cashcast weekly`, or the scheduled Sunday run):
follow `commands/cashcast-weekly.md` (the 7 results the owner expects) and the
"Weekly session" section of `.claude/skills/cashcast/SKILL.md` from step 0 without asking: fresh exports from every
bank, today's balances, `bin/cashcast weekly --balance …` once, MoneyWiz checked on disk, agent checks, and end with
this week's moves.

Every other thing cashcast can do is in its action catalog: `bin/cashcast actions` (or `--json` for the
arguments). Run any of them as the CLI command it names; the web app's Actions view runs the same list.
Actions marked "confirm" change something outside cashcast or are hard to undo: run them only after the
person says yes. "Terminal only" actions need the person's own terminal: give them the command.

Related skills: `skills/apple-sync` (Reminders and Calendar), `skills/bank-change` (bank and card settings in
the browser), `skills/bank-messages` (read a secure message in online banking and route each ask),
`skills/optimize-stocks` (where new money goes), `skills/card-calendar` (which card for what, month by month),
`skills/invest-challenge` (question or override a move from `bin/cashcast invest-plan`),
`skills/refund` (an Amazon item that never arrived: a free replacement or a refund, never a second order). Money is never moved by an agent.
Balances and e-Transfers between banks: `skills/bank-balances` (`bin/cashcast transfers`).
Which card to use right now (caps left, rent count, credit left): `bin/cashcast cards now` (`skills/money-strategy`).
"Am I spending more than usual?", "do I spend more than I earn?", "how much interest did I pay?": `bin/cashcast spending`.
Charges the person did not expect (duplicate subscriptions, new or pricier charges, charges after a cancel):
`bin/cashcast charges`; one merchant on every account, for example "do I pay Anthropic twice?":
`bin/cashcast charges find anthropic`. Both need `agent:strategy`; without it, give the person the command.
Groceries ("order groceries", "where is oat milk cheapest delivered?", "how much did I spend on groceries?"):
follow `skills/grocery-cart` (`bin/cashcast grocery compare --items "oat milk,eggs"`, `bin/cashcast grocery orders`);
it fills carts in the person's Chrome and they check out. Buying anything else ("find me the best rain jacket",
"which monitor stand fits?"): follow `skills/purchase` (3-5 options, one options page, the card to pay with).

To change a default the person asks for, use `bin/cashcast config set KEY VALUE` (see `bin/cashcast config`).

Updates: `bin/cashcast` fast-forwards a clean checkout to the latest version first. When it prints
"Updated cashcast", re-read `AGENTS.md` and `.claude/skills/cashcast/SKILL.md` before you go on.
`bin/cashcast update` checks right away.

Problems and ideas: when cashcast fails, surprises you, or could clearly work better, offer once to report it
upstream (`CONTRIBUTING.md`, "Report problems and ideas"; the `report-upstream` skill). Run
`agentbrew report --title "<title>" --body-file <draft.md> --repo fyodoriv/cashcast` (no `--yes`), show the
person the safety checklist and the exact text, and post only after their yes. Never include anything from
`private/`: describe the command and its error only.
