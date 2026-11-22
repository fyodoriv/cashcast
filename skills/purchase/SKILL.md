---
name: purchase
description: Finds the best thing to buy - clothes, shoes, gear, home items, electronics, furniture, gifts - compares 3-5 real options from shops that deliver to the person in Canada, names one pick with the card to pay with for the most cash back, and ends every request with one published options page (an Artifact) built by the recipe below, then improves that recipe. Use when the user says "buy me X", "find the best X", "which X should I get", "compare these products", "order a jacket for my partner", or "what size should I get". Don't use for groceries (use grocery-cart), to pay, check out, or fill card or address forms (the person orders).
---

# Purchase: the right item at the right price, one options page per request

Moved to cashcast from sidequest's `purchase` skill and the shopping-comparison
plugin on 2026-11-22. Sizes and the people they belong to stay in sidequest
(`sidequest sizes show`); everything about price, shops and which card to pay
with lives here.

This file is the one source. The owner's claude.ai plugin skill
`shopping-comparison` holds the same text word for word, so chats outside this
repo follow the same recipe. After you change this file, paste it into that
plugin on claude.ai (Plugins > Shopping comparison > Files > SKILL.md >
Edit) and keep its `name: "shopping-comparison"` line.

## Hard rules

- Never pay, check out, sign in, or type card, address or account data. Stop at
  the options page; fill a cart only when the person asks for it in this request.
- Sizes are private. The options page may show the buyer's own size for this item
  only; never another person's sizes. Never put sizes in a repo, test, PR or message.
- Live facts only: price, stock in the size, and delivery come from the shop page
  today, with the date checked. "Unknown" or "unverified" beats a guess; never
  fill a spec or price from memory.
- Shops that deliver inside the person's country. Prefer the brand's own shop and
  big local shops with easy returns (for Canada: Best Buy, Canadian Tire, MEC,
  Sport Chek, Simons, Staples, Costco.ca, Amazon.ca).

## Steps

1. **Who and what.** For clothes and shoes, read the person's sizes:
   `sidequest sizes show --person <id> --json` (sidequest owns sizes). None saved:
   ask once. For a product that must fit something (a stand, a part, a cable),
   look up the official specs of the thing it must work with and turn the target
   into the number that matters (show the math in one line). If a deciding spec
   cannot be found, say so and ask for a photo or a check.
2. **Size for this item.** Convert to the shop's scale (US/EU/UK/cm) and check the
   brand's size chart and fit reviews ("runs narrow", "size up"). When the saved
   size is a range, say which end and why.
3. **Wants.** What the person just said plus their saved wants (sidequest
   `shopping:` lines for that person). New wants go back there with the date.
4. **Options.** Research 3-5 real products (a subagent may do the web work). For
   each: model and colour, how it meets each want, price with currency, shop,
   size in stock, delivery cost and date, review score, one downside, direct link.
   Rank by: fits the requirement (verified) > quality > looks > price. Name one
   pick; include one budget choice when it is meaningfully different.
5. **Pay with.** Run `bin/cashcast cards now` and name the card that earns the most
   on this shop's category (cap left, credit left). It needs `agent:strategy`;
   without it, give the person the command. Never apply for a card or change one.
6. **Options page (every request).** Build it by the recipe below, publish it as
   an Artifact and send the link. For clothes, also save it to the person's
   Clothes view in sidequest (`sidequest clothes add --person <name> --item <what> --title <title> --url <link> --file <page.html>`),
   where they leave feedback. Then ask in chat which option to take (or "none, because...").
7. **Learn.** Read earlier feedback first (`sidequest clothes list` for clothes).
   Record what won and what lost and why in the person's wants, then update the
   recipe below if the page missed something, and add a line to the log.
8. **After they buy.** Remind them that `bin/cashcast charges find <shop>` shows the
   charge once it posts (a double charge shows twice), and that `skills/refund`
   handles an item that never arrives.

## Options page recipe (change it every time it could be better)

- Title: "<Item> for <name>" (or the product being bought). First line: the pick
  and why, in one sentence.
- A key-facts strip: the constraint numbers that drove the decision. When there is
  a numeric requirement, a chart of each option's range against the target band.
- A wants checklist: each want with ✅/〰️/❌ per option, so the trade-off reads at a glance.
- One card per option (#1 marked as the pick): name, price with currency, shop,
  stock badge ("US 12 in stock", "check", "out"), delivery, rating, one downside,
  status chips (verified / unverified / risky), an "Open in shop" link. No shop
  photos (the Artifact CSP blocks them): name the colours instead.
- A "Not advised" card for the obvious choice that lost, with why.
- A "Your size" box for clothes: the saved size, the size to pick per option, fit notes.
- A "Pay with" line: the card from `cards now` and its rate for this shop.
- A widened want mid-request adds a second pick box, wants table and card set to
  the same page; keep the first set and its check date.
- Size stock: read it from the shop's product JSON-LD (`offers.availability` per
  `size` and `color`) or `__NEXT_DATA__` store inventory, not from the visible
  picker. Some shops block curl (403): read them in the browser.
- Footer: date checked; "Prices and stock change; the shop page decides."
- Phone first: one column under 600 px, 44 px links, light and dark.

## Recipe log

| Date | Item | What changed in the recipe |
|---|---|---|
| 2026-11-22 | (move to cashcast) | Joined the shopping-comparison ranking and key-facts strip; added the "Pay with" card from `cards now` and the after-purchase charge check |
