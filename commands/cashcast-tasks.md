---
description: Go through my open cashcast finance tasks, resolve every one that is not blocked, name every blocker, then update the tasks
allowed-tools: Bash(bin/cashcast:*)
---

Resolve the person's open finance tasks from the cashcast repo. Money is never moved by an agent.

1. Run `bin/cashcast tasks resolve`. It resolves every `auto` task due this week (for example the
   monthly review: it makes the report and marks the task done), lists the `Agent checks next`, and
   prints a `BLOCKED` list.
2. Work every task under "Agent checks next" now, one by one (for example "Confirm chexy worked"):
   - Follow the line exactly. "Not before DATE": skip it until then. "Your rule": the owner's own
     proof for that task (for example, rent counts only when the rent website shows it passed); no
     other evidence closes it.
   - Find the evidence where its line says: cashcast reports (`bin/cashcast payments`, `status`,
     `review`), a read-only API (`bin/cashcast api balances SERVICE`), or fresh exports
     (`skills/fetch-exports`). Learn about data only from `bin/cashcast` output.
   - When only the service's site has the answer: the person runs
     `bin/cashcast consent grant agent:browser-read` in Terminal (1 day). Then open the site in the
     agent's browser, ask the person to sign in (never type passwords or codes), and read the page.
     Change nothing there. A statement or CSV you download goes into the person's private inbox; run
     `bin/cashcast import` and read only cashcast's output about it.
   - Record every check with `bin/cashcast tasks verdict ID pass|fail --reason "<what it showed>"`
     (add `--source browser` for a site check). The reason names what you saw, never an amount,
     balance or score; cashcast refuses one that does.
   - A pass closes the task. A fail keeps it open: report the problem with the evidence and the fix.
   - No evidence reachable (exports stale, sign-in declined): leave it open and say what is missing.
3. For each `browser` task, offer to do it now with `skills/bank-change`: the person signs in, you show
   the exact change, and click the final button only after their yes in chat. Then `changes done ID`.
4. Tell the person about every remaining blocked task explicitly, one line each: the task, why it is
   blocked (moves money, or is their decision, or no connector), and the exact next step. Never call a
   blocked task done. When the person says they did one, run `bin/cashcast changes done ID`.
5. Update the tasks everywhere: give the person `bin/cashcast sync all` to run in Terminal (the agent
   sandbox blocks Reminders and Calendar), and check the result with `bin/cashcast sync status`.

End with: resolved N (auto + checked), problems found, blocked M, and the one action needed first.
