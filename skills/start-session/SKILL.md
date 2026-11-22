---
name: start-session
description: Starts a cashcast money session for one person. Picks the user, checks consent, shows what is due, then proposes one next step. Use when the user opens the assistant, says "start", "what's next" or "money check-in". Don't use for a one-line factual question.
---

# Start a session

1. Run `bin/cashcast users list`. If several users exist, ask which one, and
   pass `--user NAME` to every later command.
2. Run `bin/cashcast consent list`. If `agent:strategy` is not granted, say
   once what it shares and give the owner the command to run in a terminal:
   `bin/cashcast consent grant agent:strategy`. Never edit `consent.toml`.
3. Run `bin/cashcast review`. Show the next review date, the signals, and the
   three soonest dated tasks.
4. Propose one next step (for example "import last month's exports" or "do
   the review now") and why. Wait for the user's choice.
5. At the end, write one line per new durable fact (no amounts) to
   `private/users/<name>/memory/MEMORY.md`.
