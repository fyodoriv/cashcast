# Sun Life group retirement playbook

For a workplace plan run by Sun Life Group Retirement Services: a group RRSP,
and often a TFSA, a DPSP or a pension plan next to it.

## Before you start

- You need a my Sun Life sign-in (mysunlife.ca, your plan's own link such as
  mysunlife.ca/<plan>, or the my Sun Life app).
- The agent never types passwords or 2FA codes. You sign in yourself, or your
  password manager fills them after you approve.
- The agent never moves money between funds and never withdraws.
- Menus differ by plan and by site version. The steps below name the older
  menu (my financial centre) and the newer one (Investments). Use the one you see.

## Export your data

Transaction history (the file cashcast reads):

1. Sign in at https://www.mysunlife.ca (or your plan's link).
2. Open the plan's transaction history:
   - older menus: **my financial centre > Accounts > Transaction history > Details**;
   - newer menus: **Investments > Manage plan > my plan > Check on account > View transaction history**.
3. Pick the widest date range the page allows.
4. If the page offers a download (CSV or Excel), use it. Keep an `.xlsx` as it is.
5. If there is no download: copy the table into a spreadsheet, one row per fund per
   transaction, with the columns `Date`, `Transaction type`, `Fund name`, `Units`,
   `Unit price`, `Amount`. Add `Plan` (for example `Group RRSP`, `TFSA`) if you hold
   more than the RRSP. Save it as CSV.
6. Save the file into `private/users/<you>/inbox/` with "sunlife" in its name (for example
   `sunlife-2026.csv`), and run `bin/cashcast holdings`. Without a plan column, only a file named for
   Sun Life counts as your group RRSP; any other fund file counts as non-registered (taxable).

If the holdings report says the file was not read, or shows unreadable lines, run
`bin/cashcast columns` first. It prints the header names and masked cell shapes,
never the values, so the parser can be fixed without seeing your data.

How cashcast books the rows (layout and type names are estimates):

- Your contributions and your employer's (matching) are buys of fund units at the
  dollar amount, in the RRSP unless a `Plan` column says otherwise.
- A fund transfer or switch sells units of one fund and buys units of another.
- A fee redeems units. Fund management fees are inside the unit price, so they
  never show as rows.
- Interest credited in units adds units. A withdrawal sells units.
- Registered plans have no taxable gains, so the report shows no gains for them.

Interfund transfer rows name each fund "From X" or "To X"; cashcast books the units
to fund X. The page keeps only 12 months, so funds held before that window need an
opening position: one `Transfer in (opening position)` row per fund, dated the day before
the window, with today's units from the Balances page minus the window's net units.
A fund bought only inside the window must come out at exactly 0; that checks the file.

## Actions

### download-transaction-history

First setup: pick the widest range, back to the plan's start date if the page allows it. Overlapping files are fine: the import drops duplicate rows.

1. Follow "Transaction history" above.
2. Verify: the first and last dates on screen match the range you picked.
3. Report the file name and the number of rows. Do not report amounts or the account number.

Seen on a real plan on 2026-10-25 (newer menus):

- Path: **Manage plan > my plan > View transaction history > Details**
  (`/GRS_MBR/wca/transHistoryDetails`).
- No download button. The page shows the last 12 months only, about 54 lines
  per page, with a **Next** form (`ptKey`, `direction=next`).
- Each transaction is a row with the date, the plan (`RRSP`) and the type in
  bold (`Contribution (Member)`, `Contribution (Employer)`). One line per fund
  follows, with "N units at $P" and the amount.
- An agent builds the CSV inside the page, so the values never reach the
  model. Its script walks every Next page with `fetch`, writes `Date`
  (ISO), `Plan`, `Transaction type`, `Fund name`, `Units`, `Unit price`,
  `Amount`, and saves `sunlife-transactions-<date>.csv` as a download. It
  returns only the page count, the row count, the first and last dates, and
  the count per type.
- The file lands in `~/Downloads`. Move it into the inbox with
  `mv ~/Downloads/sunlife-transactions-*.csv private/inbox/`. The agent
  sandbox blocks this move, so run it in Terminal. Then run `bin/cashcast rrsp`.
- Run it again when cashcast asks ("Download the Sun Life RRSP transaction history", when the newest
  contribution is over two months old): older rows drop off after 12 months. The next hourly sync parses it.

### download-statements

1. Open **my financial centre > Accounts > Statements** (newer menus: **Documents > Statements**).
2. Open each statement by its date and save the PDF. Statements stay online for up to
   seven years in at least one plan.
3. Save each PDF into the inbox as `sunlife-statement-<date>.pdf`. If the site opens it in a
   viewer, fetch the document link in the page and save the response (the link is a
   `javascript:openDocument('memberStatementDetails?...')` call: fetch `/GRS_MBR/wca/` plus that path).
4. cashcast reads the "Your investments" table (`sunlife-statement-pdf`): units, price and value
   per fund on the statement's last day. It refuses a file whose units times price miss a value
   or whose values miss "Total investments". `moneywiz diagnose` then prints "Statements against
   MoneyWiz units"; when every fund agrees and the plan holds no cash, `moneywiz coverage` counts
   the account's rows up to that day as covered. The transaction history covers 12 months only,
   so the statements prove the older rows.
5. A statement older than the history's first day (its opening positions) is met the same way:
   each fund's statement units move out of the opening position into a buy on the statement's
   day. The buys cost what was paid in by then (MoneyWiz's cash that day, without the
   account's buy and sell rows), split by the opening position's price per unit; the rest of
   the opening cost goes to the units left in it, so the cash stays the same. With
   `bank_wins`, `moneywiz edit --fixes` writes them (test
   `a_statement_before_the_history_moves_its_units_out_of_the_lump`; owner, 2026-11-17).

### check-rrsp-contributions

1. Open the transaction history for this calendar year. Add up the rows that are
   contributions, yours and your employer's. Statements list them apart: "My contributions"
   (member required and voluntary) and "My employer's contributions".
2. Open **my financial centre > Requests > Tax slips & RRSP info > Tax slips** for the
   official RRSP receipts. In one plan's FAQ, Sun Life mails them in early January
   (March to December contributions) and in March (contributions in the first 60 days of the year).
3. Tell the user: Sun Life counts only this plan and does not track room. For room, check
   CRA My Account or the latest Notice of Assessment.
4. With the transaction history in the inbox, `bin/cashcast rrsp` adds up this window's
   contributions (yours and your employer's; the first 60 days of a year count for the year
   before), compares them with `[room] rrsp`, and projects the window's end from the last
   three months' pace. Fund switches and transfers between RRSPs do not count.

### check-funds-and-fees

1. Open **my financial centre > Accounts > Account fees**. It lists account fees, fund
   management fees and service fees. Newer menus: **Plan overview > Account fees**.
2. For fund details and returns, open **Accounts > Investment performance** (Morningstar
   pages; click a fund name). The current split shows under **Investment mix**.
3. Report fund names, fee rates and the investment mix percentages. Do not report balances
   unless the user asks.

### change-future-investments

1. Open **my financial centre > Requests > Change investments** (newer menus: **Investments >
   Manage plan > my plan > Make a change > Change investments**).
2. Pick the option for future contributions only. Moving existing money is a separate step
   (see `rebalance-existing-money`).
3. Type the percentage for each fund; the total must be 100%. Then choose whether your
   employer's future contributions use the same funds.
4. Ask the user to confirm this exact change (each fund and its percentage) before you
   accept the terms and click **Submit**.

### change-contribution-rate

1. Check how the plan takes changes. Some plans have **Requests > Payroll contributions** on
   mysunlife.ca. Many others change the rate only through the employer's HR or payroll site.
2. If it is on mysunlife.ca: open it, enter the new rate, and ask the user to confirm this
   exact change before the final click.
3. If it is through the employer: tell the user where to change it. The user does it.
4. If the employer matches, ask the user to check the plan's match rule before they pick
   the new rate.

### rebalance-existing-money

The agent does not do this. It creates a reminder for you with the funds and percentages.
Moving money into a fund and back out within 30 days may cost a 2% short-term trading fee.
If automatic rebalancing is on, existing money is moved back to your future-contribution
mix on a schedule; turn it off first if you want a different mix for existing money.

### read-balances

1. Sign in at https://www.mysunlife.ca (the person signs in).
2. The plan overview shows each account (group RRSP, DPSP, TFSA) with its value.
3. Record the values for the strategy: `bin/cashcast api record sunlife --set "Group RRSP=…,DPSP=…,TFSA=…"`. Sun Life accounts are investments, not bank accounts: `bin/cashcast transfers` does not move money in or out of them. Do not report plan or member numbers.

## API

Sun Life has no public API for plan members. cashcast reads only the files you download.

## Sources

- https://www.sunnet.sunlife.com/content/dam/sunlife/regional/canada/documents/grs/UBC_online_roadmap_E.pdf (menus: transaction history, statements, account fees, change investments, tax slips)
- https://www.sunnet.sunlife.com/content/dam/sunlife/regional/canada/documents/grs/UBC_Change_investments_1220_E.pdf (future contributions vs existing money, employer's contributions, rebalancing)
- https://www.sunnet.sunlife.com/content/dam/sunlife/legacy/assets/canada/signin/csi/shaw/files/Shaw_RRSP_FAQ_E.pdf (receipt timing, tax slips path, payroll contributions, 2% short-term trading fee, seven years of statements)
- https://www.sunnet.sunlife.com/content/dam/sunlife/regional/canada/documents/grs/Purolator_Know%20your%20statement_E.pdf (statements path and sections)
- https://www.unbc.ca/sites/default/files/sections/human-resources/sunlifehowtoreadyourstatement.pdf (statement: your and your employer's contributions listed apart; plan names)
- https://faculty.pensions.ubc.ca/2018/08/view-your-transaction-details-online (contribution dates and amounts are in the transaction history, not the statement)
- https://faculty.pensions.ubc.ca/investments/managing/ (newer menus; no download is mentioned)
- https://www.mcgill.ca/hr/files/hr/final_-_mupp_-_sun_life_transition_guide_-_terminated_members.pdf (fund management fees in the unit price; recordkeeping fees; quarterly rebalancing)
- https://www.smu.ca/about/sun-life-details.html (the app: fund balances, recent contributions, move money, future instructions)

No source documents a CSV or Excel download. The export steps and the column layout are
estimates; `bin/cashcast columns` is the way to adjust them to a real file.

Last checked: 2026-10-25
