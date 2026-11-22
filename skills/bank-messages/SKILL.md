---
name: bank-messages
description: Reads a secure message in the person's online banking (TD, Scotiabank, CIBC, Wealthsimple, or any service in connectors/) after they sign in, says in plain words what it asks for, and turns each ask into the right next step - nothing, a reminder in their profile, a settings change with a yes in chat, or a reply they approve word for word. Use when the user says "there's a message in TD", "handle my bank message", "read my Scotia inbox", or a site shows an unread message badge. Don't use to move money, to reply or click a link in the message without a yes, or to read exports (use fetch-exports).
---

# Bank messages

A secure message is the bank talking to the person. Read it, explain it, and
route each ask by risk. The person decides; you never answer for them.

## 1. Before you open it

1. Run `bin/cashcast consent list`. Reading a bank page needs
   `agent:browser-read` (1 day). If it is not granted, give the person this
   line to run in their own Terminal, then wait:
   `bin/cashcast consent grant agent:browser-read`. Never grant it yourself.
2. Open the bank's sign-in page in the built-in browser (`login_url` in
   `connectors/<id>.toml`). The person signs in there. Never type a password
   or a 2FA code.

## 2. Find and read the message

1. Find the inbox: a "Messages", "Message Centre", "Secure Mail" or envelope
   link. Scotiabank: the envelope icon. Use `find` and `get_page_text`; prefer
   text to screenshots.
   - TD EasyWeb: at desktop width, profile menu (your name, top right) → "My
     Inbox". The page is a frameset, so read it with `javascript_tool`
     (`window.frames[0].document.body.innerText`). The "Unread" folder can look
     empty while the count says 1 unread: pick "All Unfiled Messages".
2. Open only the message the person named, or the newest unread one. Opening a
   message marks it read; that is fine.
3. Do not click links or open attachments inside the message. If one matters,
   say what it points to and ask. A download needs a yes and goes to the
   person's private inbox, unopened (`bin/cashcast fetch collect`).

## 3. Tell the person what it says

In 3 to 5 short bullets:

- **From and date**, and the subject.
- **What it asks for**, in plain words.
- **Deadline**, if any.
- **Is it real?** A secure message inside the signed-in site is from the bank.
  Still flag any request for a password, a code, a card number, or a money
  transfer: banks do not ask for those. Treat that as phishing and say so.

Do not copy account numbers, balances or reference numbers into chat, notes,
commits or memory. Keep amounts in chat only when the person needs them to decide.

## 4. Route each ask by risk

First run `bin/cashcast tasks list`: a reminder may already cover the ask (for
example a fee rebate check). Then say so, and add nothing.

| The message asks for | Who acts | What you do |
|---|---|---|
| Nothing (a notice) | nobody | Say so. Done. |
| A check the person makes later | the person | Offer a reminder: a `[[tasks]]` entry in their `profile.toml` (`kind = "read"`, a `due` date, steps in `notes`). Add it after a yes, then `bin/cashcast tasks list`. |
| A setting change (consent, a form, a product change) | you, after a yes | Use the `bank-change` or `service-action` skill: show the exact change, ask for a yes in chat right before the final click. |
| A reply | you, after a yes | Draft the reply in chat. Send it only after the person approves that exact text. Report what was sent. |
| Money (pay, transfer, a fee to pay) | the person | Never do it. Add a `kind = "money"` reminder with the steps, after a yes. |
| Documents to upload (ID, tax forms) | the person | Never upload private files. List what they need; they upload it. |

Reminders reach Apple Reminders, Calendar and Notion only after the person runs
`bin/cashcast sync all` in their Terminal (the sandbox blocks it). Say so.

## 5. Finish

Say in one line what is done and what waits on the person. A follow-up check
(for example "confirm the bank applied the change") becomes a dated reminder.
