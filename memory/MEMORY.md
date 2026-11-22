# Memory index (example shape)

The live index is per person: `private/users/<name>/memory/MEMORY.md`. One line
per durable fact, newest first, each pointing to a topic file next to it:

- [Card cap reset months](cap-resets.md) — one card resets in October, one in February (2026-10-18)

Keep the index short; it loads at the start of every session. Update an entry
instead of adding a duplicate. Delete entries that turn out wrong. Amounts
and balances stay in `profile.toml`, not in memory.
