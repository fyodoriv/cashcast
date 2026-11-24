---
name: citizenship-application
description: Guides the owner, screen by screen, through the IRCC online application for Canadian citizenship (adults) in Chrome, from one private answers file, with deterministic judges that check the answers before you start and each form screen before Next. Collects most answers from the owner's own sources over several sessions (PR card and passport photos, language result, nomads.com trips, LinkedIn, leases in mail, CRA My Account, a separation agreement) into a versioned private draft, and says which police certificates are needed. Covers getting ready (answers, documents, police certificates, the day count), the physical presence calculator, every form section, uploads, fee, signing and submitting (the owner does these), and everything after (AOR, changes, test, ceremony, passport). Use when the user says "guide me through my citizenship application", "what goes in this field", "check this screen", "am I ready to apply for citizenship", or "what do you still need from me for citizenship", "collect my citizenship info", "here are my PR card photos", "which police certificates do I need", "continue my citizenship draft", or "show me my citizenship evidence". Don't use to type ID numbers, sign in, pay, sign, attest or submit (the owner does those), for the private Paperwork tracker view (use the paperwork skill), or for other immigration forms.
---

# Citizenship application, guided

The owner opens a Claude session in this repo and the IRCC application in Chrome, then asks
you to guide them. You read each screen, say what goes in each field, and run a judge before
they move on. Every rule comes from an official page: `engine/src/paperwork/facts.rs`
(quotes, re-checked live). The judges are `bin/cashcast citizenship`
(logic: `engine/src/paperwork/citizenship/answers.rs`, tests: `engine/tests/citizenship.rs`).

## Hard rules

- **The owner acts; you read.** Never sign in, type a password, type an ID number (UCI,
  SIN, passport, PR card), upload a document, pay, tick a declaration, sign or submit. Never
  click Submit, Pay, Sign or Transmit. The owner types every field and clicks Next. If the
  owner asks you to type a plain field (an address, a date, a job title), do it only on that
  screen with their yes, and never an ID number.
- **Private data stays private.** The answers live only in `<home>/citizenship/answers.sqv`
  (`<home>` is `private/users/<name>/paperwork`), sealed with the
  paperwork key. Read them with `bin/cashcast citizenship show`: it hides ID
  numbers, on a terminal too. Never run `show --reveal`: only the owner does, in their own
  terminal, after they type "reveal". Never try to open the file another way. The draft is always sealed: `put` refuses plain drafts. Never copy a
  value into git, a PR, a task, a test, a log, memory, an artifact or a published page. Never
  repeat an ID number in chat: say where it is ("your UCI: answers file key `uci`, also next
  to the photo on your PR card").
- **Screen files are scratch.** Write each screen's JSON under `$TMPDIR`, run the judge,
  then delete it.
- **Read pages as text.** Use `get_page_text` and `read_page` (Claude in Chrome). Prefer them
  to screenshots: a screenshot of the form holds ID numbers.
- **Facts first.** Fees and rules change. At the start of each session run the facts check
  (below); on CHANGED, stop and update the facts before you advise.

## 0. Start of every session

1. Facts: `bin/cashcast paperwork sources --live` outside the agent sandbox (canada.ca
   resets connections from it). Need 0 CHANGED and 0 UNREACHABLE. MANUAL is a PDF: read it.
2. Answers: `bin/cashcast citizenship answers`. Exit 0 and "ready: yes" means go.
   - ASK lines: the owner's batch. Print them all at once with
     `bin/cashcast citizenship questions`, ask the owner for them, write them to a
     scratch YAML file under `$TMPDIR` made mode 600 (`umask 077` first), and run
     `bin/cashcast citizenship put <file>`. `put` seals the answers and removes the
     scratch file, also on an error. A section merges key by key (a file with only
     `[taxes]` `cra_consent` keeps the SIN and the years); a list is replaced whole,
     so write every row of a list. It refuses a `.sqv` file or any file in the citizenship
     folder. `collect`, `restore` and `proof` also seal an old plain draft first. Then run the judge again. Never ask
     them one at a time.
   - FAIL lines: explain each in plain words with its fix. A presence FAIL names the earliest
     sign day: the owner moves `sign_date`, or waits.
   - WARN lines: say them; the owner decides.
   - No file yet: `bin/cashcast citizenship template --out <file>` writes a blank
     one, mode 600 (the `answers` judge FAILs a scratch answers file in `$TMPDIR` that
     others can read); the
     example is `a made-up `template --out` file` (made-up person).
3. Documents ready (step "Gather documents" in the facts): colour scans of every passport's
   identity pages, two IDs, the language proof (ages 18 to 54), each police certificate the
   judge asks for, translations with affidavits, name change papers. The owner says yes or
   no to each; nothing is uploaded yet.

## 0.5 Collect the answers (several sessions; the draft)

The answers file is the draft. It grows over days: each collector merges what it finds,
keeps a dated copy first (`versions`, `restore <name>`), and never replaces what the owner
wrote. Start every session with `bin/cashcast citizenship status`: one line per
question (done, fix, open, confirm) with its source, and "facts filled: N/20". The IRCC
online form may also save progress; do not rely on it: the file is the source of truth.

For each source the owner gives: read it, write its text to a scratch file under `$TMPDIR`,
run `bin/cashcast citizenship collect <kind> <file>`, then delete the scratch file.
The collector prints which keys it added, never a value.

| Source the owner gives | How you read it | Kind |
|---|---|---|
| PR card photos (front and back) | Read the images; copy the front lines with "UCI" and "PR since", and the 3 lines of `<` letters on the back exactly | `pr-card` |
| Passport photo (data page), one file each | the "Date of issue" line and the 2 lines of `<` letters | `passport` |
| Driver's licence or health card photo | only the kind of card (no number) | `id` |
| Language test result or diploma | the file's text | `language` |
| nomads.com travel history | in the owner's Chrome, the trips list (get_page_text) or the profile JSON, or the site's CSV export ("Arrival date", "Departure date", "City", "Country"); `collect` refuses a file with no readable stay and keeps the saved trips, so then write it yourself as CSV `start,end,city,country` | `nomads` |
| LinkedIn | the owner's data export (Positions.csv, Education.csv, Email Addresses.csv, PhoneNumbers.csv; the owner downloads it), or the Experience page written in the Positions.csv columns | `linkedin` |
| Addresses | the owner's mail in Chrome (search lease, tenancy, utility, moving) and CRA My Account; write CSV `from,to,address,city,country,postal_code`, `to` = present for now | `leases` |
| Separation status | the separation agreement, divorce or marriage certificate | `marital` |
| Previous visitor visit | stays in Canada before the PR date come from nomads.com (`temp_periods`, status visitor); check the status against the visa or permit in the passport or the IRCC account and fix it in the file | `nomads` |
| Taxes | CRA My Account (the owner signs in) > Tax returns, page text | `cra` |

- The magic letters (`<<<`) carry check digits: "fails its check digits" means a letter is
  misread; zoom in and read it again.
- A folder with every scratch file: `collect-all <dir>` (file names say the kind).
- Run `pr-card` before `nomads`: time before PR needs the PR date. `police` also waits for
  it: it prints "needs sign_date and trips first" until the PR card set the sign day.
- nomads.com: click "Expand all N trips" first. Page text and script results are cut near
  1,000 characters, so build the CSV in the page from the table cells' text and read it in
  slices of about 20 rows. Read cell text only: a result that holds link attributes is blocked.
- Lease mail often has no unit number or postal code. Collect what a message confirms and put
  the missing parts in the owner's batch. Open only the kinds of mail the owner agreed to
  (never delivery receipts for an address without a yes).
- Derived on each collect: the sign day (first day with 1,095 days), the city, the contact
  address, other names (a passport in another name), the tax years you had to file,
  and `police_certificates: []` when none is needed.
- Police certificates: `bin/cashcast citizenship police` names each country with
  183+ days in a row in the last 4 years (each country of a multi-country trip counts on its
  own). Tell the owner to ask early: they take weeks to months.
- What stays with the owner: the choices (`confirm` rows: say the usual answer, they say
  yes or change it), `prohibitions_none`, the two consents, `taxes.cra_consent`, and any ID
  number a source masks (CRA shows part of the SIN). Ask them all in one batch.
- Scratch files hold ID numbers: delete each right after its collect.

### Check the trips against outside records (before the sign day is final)

The nomads.com list is the owner's own log. Do not trust its border days. Check each trip
since the PR date, because each wrong day moves the count by 1.

- **The list gives arrival days, not leave days.** An evening flight leaves Canada the day
  before the first stay starts. Read the ticket mail for the departure day.
- **A flight home due near midnight may land the next day.** The return day is the day the
  owner crossed the border. Best mail evidence: the CBSA declaration mail (ArriveCAN, sent
  minutes before the kiosk) and a ride receipt from the airport with its pickup time. A
  declaration mail sent 1 to 3 days before the flight is an advance declaration, not a crossing.
- **A gap between two stays abroad is not a visit to Canada.** The collector may turn a
  transit of 1 or 2 days into "returned, then left". Join the two trips with `put`.
- **Rate each leave day and return day** high, medium or low, and give the reason. High
  needs 2 sources that do not come from each other, one a record of that day. A return day
  also needs a record on the Canadian side. The owner's calendar is a plan, never enough alone.
  Apply the same rule to every row: a leave day needs a record in Canada on that day or the
  day before, and a row that rests on an open question is medium until the answer.
- **Read every ride and food-delivery receipt on each border day, from every provider**
  (Uber, Lyft, DoorDash and others), not only receipts with an airport charge. A ride to the
  airport proves the leave day; a ride from it proves the return day. List the provider's
  mails for the whole period first to see which providers the owner uses.
- **List every stretch of 6 days or more with no located photo**, from the photo index, and
  check each one for receipts. Name the ones with no record as weak stretches, with their cost.
- **Sweep every month since the PR date** for a trip the list does not hold: photo places,
  declaration mails, airport ride receipts, booking mails, carrier mails. Say what you searched
  and what you did not. "None found" is not "none exist": ask the owner to confirm.
- **Photos prove where a phone was, not who held it.** A library can hold photos from other
  people's phones. Name the phone model when a photo is the proof.
- **Never write "ready to submit" while a question is open.** Put each row below high, each
  airport ride with no flight, and same-day trips to the United States in a loud gaps list,
  each with one question the owner can answer in a word, and the official fix: the CBSA
  travel history request and the US I-94 travel history page (the owner types the passport
  details).

### The evidence page (the owner's one view of the draft)

After each collect, keep a screenshot of the source as proof, then rebuild the page:

1. Proof from a photo the owner gave (PR card, passport, licence, language result,
   separation papers): `bin/cashcast citizenship proof <question> <photo>
   --source "PR card, back"`. One photo can back several questions (run `proof` per question).
2. Proof from a web page (nomads.com, LinkedIn, CRA, a lease email): scroll the part that
   holds the answer into view in the owner's Chrome, take a Claude in Chrome screenshot with
   `save_to_disk: true` at scale 1, then `proof <question> <saved path> --source "<page, part>"
   --url <page URL>`. Then delete the saved copy (it is outside the sandbox: run `rm` with the
   sandbox off).
3. `bin/cashcast citizenship report` writes `private/users/<name>/paperwork/citizenship/evidence.html`
   (mode 600): every question with its answer, source, the day it was collected and its
   screenshots, the police certificates and what is next. It prints only the path and counts.
   Tell the owner to open it on this Mac within 10 minutes: then it is removed (run `report`
   again). The screenshots stay sealed in `proofs/` (`<name>.sqv`). Never read, publish, upload or send the page: it
   holds every private value.

## 1. Sign in (owner)

Open the official sign-in page (`https://citapply-citdemande.apps.cic.gc.ca/en/landing`) in a
Chrome tab. The owner signs in. Wait; do not touch the sign-in page.

## 2. The screen loop (every screen until the review page)

1. `get_page_text` and `read_page` (interactive) on the IRCC tab. Name the screen.
2. For each field, say what goes in it, from the answers file and the section guide below.
   ID numbers: say which key and document hold it, never the number.
3. The owner types and says "done".
4. `read_page` again. Write `{"screen": "<name>", "fields": [{"label": "...", "value": "..."}]}`
   with every field's label and current value to `$TMPDIR/screen.json`, then run
   `bin/cashcast citizenship screen $TMPDIR/screen.json` and delete the file.
   - Exit 0: every known field MATCHes; tell the owner to click Next (or Save).
   - MISMATCH or EMPTY: name the field; the owner fixes it; repeat from 4.
   - UNKNOWN: no rule for that label; check it by hand against the section guide.
   - Tables (other names, past addresses, trips, jobs) come back UNKNOWN on purpose: compare
     each row by hand with the same list in the answers file.
5. If a screen asks something the answers file does not hold, add the question to the
   batch, and help the owner add the answer to the file, then judge again.

## 3. Section guide (the online form asks the CIT 0002 questions)

| Section | What to enter | Traps |
|---|---|---|
| Service language, accommodation (Q1-2) | `service_language`; `accommodation` | none means none |
| Personal details (Q3-6) | `uci` (8 or 10 digits), `name` exactly as on the PR card or Confirmation of PR, `other_names`, `marital_status` | no given name: leave it empty, never "NA" or "*"; a name change since PR needs documents |
| Contact (Q7) | `contact` | street words in full (Street, not St); phone with country code; the email gets the e-certificate |
| Representative (Q8) | `representative` | yes means IMM 5476; a representative who submits for you means paper |
| Physical presence (Q9) | the calculator in the account | see step 4 below; the calculator's application date is the sign day |
| Address history (Q10) | `addresses`, newest first, current address first | covers all 5 years, no gaps, postal codes; row 1 copies the contact address |
| Work and study (Q11) | `activities` | every month of 5 years: work, school, language classes, unemployed, caregiver; "present" for ongoing |
| Taxes (Q12) | `taxes.sin`, each year's had-to-file and filed, `taxes.cra_consent` | the years fill in from the window; answer all 5 even if abroad |
| Status elsewhere (Q13) | `other_status` | your country of citizenship counts |
| Passports (Q14) | `passports` | every one held in the 5 years, numbers exactly, no spaces |
| Language (Q15) | `language_proof` | ages 18 to 54 only; CLB/NCLC 4 |
| Prohibitions (Q16) | `prohibitions_none` | the owner reads and ticks each; unsure means a lawyer first |
| Certificate (Q17) | `certificate` | electronic needs the email |
| MP, Elections Canada (Q18) | `consent_mp`, `consent_electors` | either answer is fine |
| Sign (Q19) | `city`, `sign_date` | the sign day must equal the calculator's application date |

## 4. Physical presence calculator (in the account)

The online application must use the calculator in the account. Enter `pr_date`, each
`temp_periods` entry (half days, up to 365), and each `trips` entry (left, returned,
country). The day you leave and the day you return count as days in Canada. Compare the
calculator's total with the judge's presence line (`answers` prints "N days in Canada"):
they must be the same. If they differ, find the trip that differs before you go on.

## 5. Uploads, fee, sign, submit (owner)

1. Uploads: the owner uploads each scan from step 0.3. Read the upload list on the page and
   tick off each document against the judge's findings (police certificates, language proof).
2. Fee: the owner pays on the official page when ready to submit. The amount is the
   `pay-fee` item in the facts (`bin/cashcast paperwork show citizenship`); the facts check
   at the start confirmed it. Then the owner uploads the receipt.
3. Final gate, all fresh: `paperwork sources --live` 0 CHANGED; `citizenship
   answers` exit 0 with today's date; every screen judged; every upload ticked off; today is
   the sign day in the answers and the calculator.
4. The owner signs (the date sets the 5-year window), reads the declaration and submits.
   They save the confirmation.

## 6. After submit

Walk the "After you apply" step in the facts (`paperwork show citizenship`):
- Confirmation email first; it is not the AOR. The AOR brings the application number. Watch
  spam for mail from an address ending in @cic.gc.ca. Incomplete: fix, recalculate the
  days, sign again, send again.
- While waiting: keep PR status; answer IRCC within the time it gives (usually 30 days); tell
  IRCC about a move, a new email or phone, a trip over 2 weeks, or a criminal charge.
- Fingerprints only if asked: police service, RCMP or an accredited company, never Service
  Canada.
- Test (ages 18 to 54): 30 days from the invitation, 3 tries, 15 of 20 to pass; study
  Discover Canada.
- Ceremony: invitation, PR card (even expired) or Confirmation of PR, 2 IDs (one with photo
  and signature). The PR card is collected.
- After: check the certificate; apply for a passport before any trip abroad.

Mark progress in the private tracker with `bin/cashcast paperwork mark ca-citizenship-adult
<item> done` (the paperwork skill).
