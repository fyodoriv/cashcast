---
name: knowledge-update
description: Checks and updates the assistant's sourced facts (tax brackets, limits, card terms, bank fees, insurance and investing rules). Use when `bin/cashcast knowledge check` reports stale facts, a review flags them, or the user says a card's terms changed. Don't use to store the user's private numbers.
---

# Knowledge update

1. Run `bin/cashcast knowledge check`. List stale files and estimates.
2. For each fact to update, open the primary source (CRA, the issuer, the
   provider), and edit `knowledge/data/<topic>.toml`: the value, `source`,
   `confidence = "verified"`, and the file's `as_of` and `review_by`.
3. Terms that apply only to this person go in their private
   `private/users/<name>/knowledge/<topic>.toml` (same schema, by `id`).
4. Run `bin/cashcast knowledge propose <file>` on the edited copy, show the user
   the added/removed/changed list, then `bin/cashcast knowledge check`; it must report no problems. Shared changes go in a PR
   (see `CONTRIBUTING.md`).
