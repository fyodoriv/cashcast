# 16. Bank PDF statements are read locally with pdftotext, and only when they add up

- **Status**: accepted
- **Date**: 2026-11-16

## Context

The owner's goal is that every MoneyWiz row since `[moneywiz] since` matches a
bank row. TD's Account Activity download reaches about 120 days back, and
Scotiabank's web filter reaches November 2024. Older months exist only as PDF
statements. On 2026-11-16 `moneywiz coverage` named TD Credit, TD debit,
Scotiabank Credit and Scotiabank Debit as the biggest history gaps.

Options to turn a PDF into rows:

- **`pdftotext -layout`** (poppler, GPL, a separate program): keeps each
  line's columns with spaces, so a row's amount sits under its header. Fast,
  local, already on this Mac (`brew install poppler`).
- **A Rust crate** (`pdf-extract`, `lopdf`): no extra program, but text comes
  out without column positions, and the build grows. Chequing signs depend on
  which column an amount sits in.
- **An LLM or OCR service**: sends statements off the computer. Refused by the
  privacy rules.

## Decision

1. `engine/src/pdf.rs` runs `pdftotext -layout -enc UTF-8 <file> -` and reads
   its stdout. Nothing is written to disk and nothing leaves the computer.
   Without pdftotext the file fails with "brew install poppler"; the doctor
   prints a note, not a failure.
2. One reader covers four layouts: TD credit card, TD chequing, Scotiabank
   Visa and Scotiabank chequing. It finds the bank by brand words on lines
   without an amount, and the kind by column words (posting date, withdrawals
   and deposits). Rows are a day-and-month date, a description and an amount;
   the year comes from the statement period. One file may hold many
   statements (a yearly download).
3. **A statement is read only when it adds up**: previous balance plus rows
   equals the new balance on a card, and each printed running balance follows
   from the last on a chequing account. One statement that does not add up
   refuses the whole file. So a misread sign or a missed row never reaches
   MoneyWiz; it shows as a failed file with the statement period.
4. The account is `Credit card …NNNN` or `Checking …NNNN` from the account
   number on the statement (the same name the QFX download gives), else the
   file name. `[moneywiz.export_aliases]` links other names.
5. PDF rows yield to downloads: a PDF row on a day that a CSV or QFX file of
   the same account covers is dropped (`inbox::trim_pdf_overlap`). Two copies
   of one statement keep one set of rows.
6. `bin/cashcast columns` prints a PDF's lines as masked cells with their
   column positions (`@0 MON 9 | @14 a a | @60 num`) and what the reader made
   of each line. Layouts are fixed from these shapes only.

## Consequences

- The layouts were written from the banks' public statement formats, before a
  real statement was in the inbox. The balance check makes a wrong guess
  safe: the file is refused, `columns` shows why, and the playbooks say to
  note those months as a known gap until the reader is fixed.
- The first real statements (2026-11-16: 30 TD card, 15 TD chequing, 14
  Scotiabank Visa, 15 Scotiabank chequing) needed these fixes, each found from
  masked shapes and pinned by a test: an amount that wraps below its row, a
  cash back box with its own "previous balance" (a card passes when one
  printed opening and closing pair adds up), a box beside the period line,
  labels printed above their values, a period taken from the "Opening balance
  on" and "Closing balance on" dates, the amount under the AMOUNT header (not a
  foreign amount in the details), and a page-margin number before a
  reference. For a refused card statement, `columns` prints every row and a
  hint naming the line numbers whose change would make it add up.
- Each inbox load runs pdftotext once per PDF (tens of milliseconds each).
- `fetch collect` moves a PDF from Downloads only when the reader recognises
  it as a TD or Scotiabank statement.
