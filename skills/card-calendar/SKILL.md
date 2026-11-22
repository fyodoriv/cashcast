---
name: card-calendar
description: Tells the person which credit card to use for each kind of spending, month by month for the next few months (3 by default), from the strategy's card routing plus the dated changes in their tasks - a cash-back cap that resets, a card product change, an annual fee or its rebate, the rent card rotation. Use when the user asks "which card should I use for what", "remind me which cards to use next months", "card calendar", or after a card changes. Don't use for the full money plan (use money-strategy), to apply for, cancel or change a card (the person does that), or to move money.
---

# Card calendar

You read the routing and the dated changes; the person uses the cards.

## 1. Before you start

`bin/cashcast consent list`: the routing needs `agent:strategy`. Missing? Give
the person `bin/cashcast consent grant agent:strategy` for their own Terminal,
then wait.

## 2. Read the routing

For "right now" (today's purchase, this week), run this first:

```bash
bin/cashcast cards now
```

It counts the cap used since each card's reset from the imported rows, and
gives the cap left, the reset date and the credit left. Lead with its answer.
For the months ahead, continue:

```bash
bin/cashcast strategy
```

Take two parts of the **Cash back: where to pay** section:

- the table: spending category → card, in order, with each card's cap;
- the rent rule (count charges per cap year, no math).

"No cards in `[wallet]`": stop and ask the person which cards they hold.

## 3. Find what changes, by month

1. `bin/cashcast tasks list --json`: keep open tasks due in the next months
   (default 3; the person may ask for more) that change a card or where to
   pay. These include a product change or downgrade, a cap reset, an annual
   fee or rebate, a rent payment switch (Chexy), a new card, or a card paydown.
2. Read those tasks' `notes` in the person's `profile.toml` for what changes
   after them (for example "After: groceries + gas → TD 3%").
3. An overdue task that changes a card (for example a downgrade not done
   yet): the routing stays as it is until the person says it is done. Say
   so, and show the change from the month after they do it.

## 4. Tell the person

One short block per month, oldest first:

| Month | Spending → card | What changes |
|---|---|---|

- Use the strategy's card names, and dates as "Tue Dec 1", never ISO.
- Name the task id behind each change, so it can be checked.
- End with the cards to keep active (one small charge), and the next date to
  look again: the first change date, or the first of next month.

## 5. Reminders

Offer a one-time reminder for each month where a card changes ("From Dec 1:
groceries on TD"). Add it to `[[tasks]]` in `profile.toml` only after a yes.
Never add a standing duty ("always use X"); it cannot be ticked done. Reminders
reach Apple Reminders after the person runs `bin/cashcast sync all` in
Terminal.

Never apply for, cancel, product-change or pay a card. Never put amounts in
git, memory or notes.
