---
name: refund
description: Gets a refund or a free replacement from a store in the person's browser, without paying twice. First case - an Amazon.ca order where one item (part of the order) never arrived. Use when the user says "my Amazon item didn't arrive", "get a refund", "the package is missing", "item not delivered", or "I was charged but got nothing". Don't use to buy, reorder, pay, dispute a charge with the bank, or sign in for the person.
---

# Refunds in the browser

Goal: the person gets the item (or the money back) and pays once. Never
place a new order for the missing item: that is paying twice.

## Before you start

1. Ask which store and order, if the person did not say. Ask for no card or
   account numbers.
2. Browser work shows account pages to the agent. If cashcast says so, ask
   the person to run in Terminal: `bin/cashcast consent grant actions:browser`.
3. Optional: check the charge was taken once with
   `bin/cashcast charges find amazon`. Two charges for one order: say so,
   and ask for both back in the same request.

## Amazon.ca: an item did not arrive

Tested live on 2026-11-09 (an item marked Delivered but not in the parcel;
Amazon sent a free replacement, new order total $0.00).

1. Open the order page the person gives, or
   `https://www.amazon.ca/gp/css/order-history`, in the person's Chrome. The
   person signs in and types any 2FA code. Never type passwords or codes.
2. Read the order: the item, its status ("Delivered 3 November", "Arriving"),
   and who sold it. Open **Track package**: one parcel can carry several
   orders. Tell the person which case fits:
   - **Not delivered yet**: wait until the date Amazon gives; stop.
   - **Delivered, but the item was not in the parcel** (part of the order).
   - **The whole parcel never came.**
   - **Sold by another seller**: the free resend may not exist. Ask the
     seller first; an A-to-z Guarantee claim is the next step.
3. Go to `https://www.amazon.ca/hz/contact-us/foresight/hubgateway`, pick the
   item tile, then **Says delivered, but it's not here**. Amazon first asks you
   to wait until a date. If that date has passed, click **I need more help**,
   then **Start chatting now**.
4. The chat may open in a pop-up window that the browser tools cannot see.
   If no new tab shows up, ask the person to open the chat in a tab of the
   agent's tab group, and continue there. If you see **Looks like you already
   have a chat in progress**, pick **Continue the previous chat**.
5. In the chat, the assistant offers buttons. Click them in this order:
   **Yes, that's it** → **Not received** → **Wasn't in parcel** (or
   **Parcel never arrived**) → **Replacement with free return** (or **Get
   refund**). Prefer the replacement when the person wants the item. Its
   text must say "at no extra charge".
6. For the return method, pick any **FREE** option. It is used only if the
   lost item shows up later.
7. Before **Yes, confirm my replacement** (or any typed message, or a refund
   submit), show the exact request in chat and wait for a clear yes. A yes
   given earlier for the same request counts. No yes: leave the page as it is.
8. After the click, read the result. Then check **Your Orders**: the
   replacement is a new order dated today, with **TOTAL $0.00**. A total
   above $0.00 is a second payment: say so at once and ask for a refund.
9. Add one reminder (offer the `apple-sync` skill): "Check the Amazon
   replacement arrived", dated the day Amazon gave plus 3 days. Leave the
   order number out of it.

## Never

- Order the item again, buy, pay, or add a payment method.
- Send a typed chat message without the person's yes.
- Open a chargeback or dispute with the bank: suggest it only if Amazon
  refuses, and the person does it.
- Accept new terms, or cookie banners beyond the most private choice.
- Copy order numbers, addresses or names into tasks, notes or commits.
