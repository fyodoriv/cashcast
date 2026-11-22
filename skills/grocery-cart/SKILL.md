---
name: grocery-cart
description: Fills online grocery carts in the person's Chrome with the best option for each product they name, comparing at least 5 delivery stores and apps in Canada (No Frills, Real Canadian Superstore, Loblaws, Voilà, Walmart.ca, Metro, Costco, Farm Boy, Uber Eats, DoorDash, Skip) in seconds, preferring organic, reusing fresh prices from `bin/cashcast grocery`, ranking carts by the checkout total, and logging the order the person places. Use when the user says "order groceries", "fill my cart with X", "cheapest X delivered", "where are groceries cheapest", or "how much did I spend on groceries". Don't use for restaurants, gifts or clothes (use purchase), or to check out or pay.
---

# Grocery cart

cashcast keeps the prices; the agent reads the store pages and fills the carts in
the person's Chrome; the person checks out and pays. Moved from sidequest on
2026-11-22.

## Hard rules

- Never press Checkout, Place order or Pay, never accept a store's terms, and
  never type a card, an address or a password. Sign-in goes through the password
  manager flow or the person.
- Never change a saved address in a store or app. Read each checkout's
  "Deliver to" line and name any mismatch in the report.
- Never remove the person's own lines from a shared cart (Instacart keeps one
  cart per store): price only this run's lines.
- Prices are live facts: read them from the store page today. "Unknown" beats a guess.

## Where things live

- Prices, fee rules and orders: `bin/cashcast grocery …`, one SQLite file in the
  person's data folder (mode 600). Agents never open it; use the commands.
- The person's rules, quantities and memberships: `[groceries]` in their
  `profile.toml`. Read them with `bin/cashcast grocery prefs` before you pick.
  Example keys: `organic_premium = 0.75`, `fresh_days = 3`,
  `rules = ["never X", ...]`, `memberships = ["Uber One", ...]`,
  `[groceries.quantities]`, `stores_first = [...]`. Feedback from the person
  ("no chili flavour") goes there, with the date.
- The Groceries view of the web app shows the same data: spend per month, fee
  rules, and a store comparison for the items typed.

## Commands

- `bin/cashcast grocery stale --items a,b`: items with no price in `fresh_days`.
  Search only these.
- `bin/cashcast grocery save --file rows.json`: save rows from `sqRows(items)`
  ([`search.js`](search.js)). It keeps the 6 best packs per item by price per
  100 g/ml; pack sizes such as "12 x 330 mL" count, so bulk packs compare fairly.
- `bin/cashcast grocery best ITEM`: per store, the best pack (organic within the
  premium) and a substitute (the next best other product).
- `bin/cashcast grocery fee STORE --delivery N [--free-over N] [--service N] [--pct-off N --max-off N --min-order N] --note "..."`:
  the fee rule after memberships, read from the cart or checkout page, saved with the date.
- `bin/cashcast grocery compare --items a,b [--days N]`: each store's total =
  items − promo + delivery + service, full baskets first. Free delivery and a
  promo minimum count only for a full basket.
- `bin/cashcast grocery order STORE --total N [--note TEXT]`: after the person
  checks out, log the checkout total they paid. `bin/cashcast grocery orders`
  shows spend per month (needs `agent:strategy`; without it, give the person the command).
- `bin/cashcast grocery import --from OLD.sqlite`: one-time copy of an older
  price database (sidequest's). The old file stays as it is.

## Fast search (one call per store)

[`search.js`](search.js) defines `sqSearch(items)`, `sqSum(items)` and `sqRows(items)`. In Chrome:

1. Navigate one tab to the store's home page. Loblaw stores (No Frills,
   Superstore, Loblaws) show a blank bot check first: wait 5 s.
2. Run the file's code plus `await sqSum([...items])` with the JS tool. It fetches
   every item's search page in parallel (same-origin fetch, no page loads) and
   returns relevant rows (every query word in the name), cheapest per 100 g/ml
   first, organic rows marked `ORGANIC`. Each store takes 1-4 s.
3. Repeat for at least 5 stores. Search words matter: "brand flavour chips",
   not "brand".

Store data: Loblaw stores read `__NEXT_DATA__` (`productId` + `pricing`), Voilà
reads `__INITIAL_STATE__`, Walmart reads `__NEXT_DATA__` (`priceInfo`), Metro
reads `.default-product-tile` HTML. A new store gets an adapter in `search.js`.
Costco renders in the page: open `/s?keyword=<item>`, then `sqTiles("<item>")`.
Amazon uses the fetch adapter. The JS tool cuts output near 1,000 characters:
keep a long result in `window.sqOut` and read it in slices.

## Delivery apps and deals

Search the delivery apps the person has a membership for (`grocery prefs`), signed in, in their Chrome:

- Uber Eats: `/ca/search?q=<item>` lists stores at in-store prices. Read the
  rendered text (the HTML is a shell). Add with the card's "Quick Add" inside the
  store's section. Checkout shows "Taxes & Non-Gov't Fees" and may default to
  Priority delivery; Standard is cheaper. Never press Place order.
- DoorDash: `/search/store/<item>/` groups items by store. The store banner shows
  promos ("30% off, up to CA$19"). Read cards with `textContent` (innerText is
  empty in a background tab). Add with `[data-testid=add-button-label]`.
- Skip: its Grocery tab is Instacart inside Skip and first asks to accept
  Instacart's terms; never accept them. Use Skip's own stores instead
  (`/<city>/restaurants/convenience`). On a store page, paste `search.js` and run
  `await sqRows([...items])` (the page's `__NEXT_DATA__` holds the whole menu), as
  `skip:<store>` rows. `sqSkipFee()` reads the fee for `grocery fee`.
- Save each app store as `ubereats:<store>`, `doordash:<store>` or
  `skip:<store>`, and its promo with `--pct-off --max-off --min-order`.

Deals every run: keep each sale ("was" price), multi-buy and store promo in the
database, and say them in the report.

## Carts

1. Name each cart "store via app" everywhere ("Metro via Uber Eats").
2. Start with the stores that were cheapest in past orders (`grocery orders`,
   `stores_first` in prefs), and stop early if the person already picked a store.
3. Fill at least the number of stores the person asks for, one tab each. Wait for
   each add to show "in Cart" before you leave the page: a fast navigation drops
   the add. A disabled add button means out of stock: add the substitute.
4. Match lines by volume or weight, not by count ("2" can mean 2 bottles or 2
   four-packs). Read each line's pack size and quantity back from the cart and
   check its total against the person's quantities.
5. Read every cart back: lines, subtotal, minimum, delivery notes.

## Delivery check, totals, ranking

1. Sign in first, then check delivery. A signed-out cart is a guest cart at a
   pickup store ("delivery, Not available"): the wrong flow, not a store that
   cannot deliver. Drop a store only when it still cannot deliver after sign-in,
   and quote its message.
   - PC Express (No Frills, Superstore, Loblaws) signs in with PC ID. A signed-in
     cart can still sit on Pickup: press "Switch to delivery".
   - Walmart "Delivery: Not available" while signed in means no saved address:
     ask the person to add it; never type it.
   - Instacart: Costco is `/store/costco-canada/`, Farm Boy `/store/farm-boy-ca/`.
     "Save CA$X more with a Costco membership" means the membership is not
     linked: tell the person, never link it.
   - After sign-in the store loads the account's own cart: re-read every line,
     remove leftovers from past runs, and set the counts again.
2. Rank only on the checkout page's total, never the cart's estimate (PC Express
   carts show delivery as "-" and charge it at checkout). Report two totals per
   store: with promos, and without (a new-account promo can win one order and
   lose every repeat). Show "% below the priciest cart" per row, as the
   Groceries view does.
3. Include items, delivery, service, taxes, promo and the same tip on every
   store. Read the fee for the slot shown; re-read checkout after a slot change.
4. Read the earliest delivery slot ("earliest" means the first slot, not the
   cheapest one). Mark a cart under its minimum.
5. Rank by total, cheapest first. Navigate the tabs in rank order and close the
   rest (tools cannot move tabs).
6. Top 3 carts: set a named replacement on every line, the same kind first,
   never anything the person's rules forbid.

## Speed

Paste [`cart.js`](cart.js) and call one helper per line. When the person's
rules say "never X", first set `window.sqAvoid = /X/i` so replacements skip it.

- Instacart: `icAddN(/name/, n)` adds and sets the count in one call.
- PC Express replacements: `pcSet(i, [regex…])` on `/en/cartReview`, 3 lines per JS call.
- Uber Eats replacements: `ueOptions(name)`, then `uePick([regex…])` and
  `ueFinish()`. A JS call longer than 45 s times out but keeps running: wait and
  read the lines before the next one.
- DoorDash: `ddCards()[i].b.click()` adds 1.
- Uber Eats and DoorDash search pages: `await cardsReady()` instead of a fixed wait.
- PC Express: a cart button clicked off screen does nothing: `scroll_to` first.
- Walmart caps a line at its stock ("Max 1") and can lower it later: re-read
  every count on the review page.
- After a page load, paste the helpers again.

## Steps

1. `bin/cashcast grocery prefs`, then `grocery stale --items …`. Fresh prices are
   reused; search only stale or new items.
2. Fast search at 5+ stores; check each pick by eye (the filter keeps
   "Sundried tomato tuna" for "sun dried tomatoes"). Save the rows with `grocery save`.
3. `grocery best` per item and `grocery compare --items …`: fill ONE store unless
   a split saves more than the second fee.
4. Fill the carts (above), read them back, save each store's fee rule with `grocery fee`.
5. Savings: 1-3 sale items that match the person's likes. Never an item outside them.
6. Report every cart: lines, checkout total with and without promos, slot,
   "% below the priciest", and the deals. Stop at the cart: the person picks the
   store, signs in and pays.
7. After they check out, log it: `bin/cashcast grocery order STORE --total N`.
   New rules or feedback go into `[groceries]` in their `profile.toml`.
