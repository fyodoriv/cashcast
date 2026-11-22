# Web app design

The web app (`web/`) borrows proven patterns from the best personal-finance
apps, mixes them, and fixes their known weak spots. It adds no features for
this: every element shows data the API already serves.

## What we learned from

| App | Pattern we use | Where |
|---|---|---|
| YNAB (2025 remodel) | A **Home** "command center" first: alerts, priorities, month-ahead progress. A grouped, compact sidebar. | Home view, sidebar |
| Monarch Money | A forward **cash-flow projection** of the balance. Recurring items colored by status: blue upcoming, amber off-plan. | the shared chart; blue date pills, amber when overdue |
| Copilot Money | A calm canvas with **semantic money colors**: green in, red out or below zero, blue to invest, amber pending. Large tabular numbers. | color tokens in `styles.css`, KPI tiles |
| ProjectionLab | **Milestones on the projection**: dated events marked on the chart itself. | dots on months with tasks (Plan ahead) |
| Wealthfront Path | **Live what-if**: sliders that redraw the projection at once. | the what-if control redraws the chart, with the plan before the change dashed |
| Lunch Money | **Hide empty rows**, and show what is over the limit in red. | quiet months collapse to one line; negatives in red |

## How we improve on them

- **One chart everywhere.** Home, Forecast and Plan ahead share `web/src/Chart.tsx`. It draws a line with its area to zero, turns red below zero, and adds optional bars, a dashed cash floor, a dashed baseline and task markers. Each app above draws its charts differently from view to view.
- **Keyboard and screen readers.** Every month in a chart is a focus stop, and focus shows the same tooltip as hover. The chart has an `aria-label`.
- **Honest numbers.** Money is right-aligned with tabular figures. Negatives carry a minus sign and red. A month under the cash floor (but above zero) is amber. Markdown tables right-align columns that hold only amounts or rates.
- **Phone first-class.** On a phone, a bottom tab bar holds Home, Plan ahead, Forecast and Monthly review, and More opens a sheet with every view. That replaces a scrolling strip of 15 tabs. Tables scroll inside their card, never the page.
- **Dark mode from the same tokens.** Every color is a token with a dark twin, so contrast holds in both themes.
- **No dashboard customization.** That would be a feature (Monarch has movable widgets). Home has a fixed order: alerts, four numbers, the cash ahead, then what to do.

## Hallmark system (2026-11-01)

The web app follows the Hallmark design skill at **restyle** depth: the layout above stays, the system changed.

- `web/src/tokens.css` holds the Hallmark scale (spacing, type, motion) and maps its colour names to the brand tokens in `styles.css`. `web/src/base.css` holds the Hallmark base rules.
- Spacing uses `var(--space-*)` only (a 4px scale). Type uses `var(--text-*)`; no text under 14px.
- Labels, table heads and nav titles are sentence case and untracked. No middle-dot meta strings, no arrows glued to link text, no dashes in copy.
- Every control has hover, `:active` and `:focus-visible` states, and a 44px touch target.
- Check: `node ~/.claude/skills/hallmark/scripts/sloplint.mjs web/src --genre modern-minimal --scope app` must print 0 FAIL. The stamp on line 1 of `styles.css` records the system and any waiver.

## Rules for new views

1. Start with a `page-head`: an `h2`, plus a one-line `lead` saying what the view answers.
2. Put content in `panel` cards. Use `cards`/`card` for clickable KPI tiles.
3. Use `money()`, `sign()` and `compact()` from `web/src/format.ts`, and the `num` class for amounts.
4. Use `TaskList` for any list of dated tasks, and `Chart` for anything over months.
5. Check the view at 1440px, at 390px and in dark mode before merging.

## Sources

- YNAB, "The Great YNAB Remodel": https://www.ynab.com/whats-new/the-great-ynab-remodel
- YNAB, "The new design is rad": https://www.ynab.com/the-new-design-is-rad-ynab-theme-updates
- YNAB, spaces in the mobile app: https://support.ynab.com/en_us/spaces-in-the-mobile-app-S1iIZQoqgg.md
- Monarch, recurring bills and subscriptions: https://www.monarchmoney.com/whats-new/track-recurring-bills-and-subscriptions
- Rob Berger, Monarch Money review: https://robberger.com/monarch-money-review/
- Blake Crosley, Copilot Money design guide: https://blakecrosley.com/fr/guides/design/copilot-money
- Copilot Money for web: https://help.copilot.money/en/articles/11780342-copilot-money-for-web
- ProjectionLab, milestones: https://projectionlab.com/blog/milestones
- Wealthfront, introducing Path: https://www.wealthfront.com/blog/introducing-path/
- Lunch Money, calendar: https://lunchmoney.app/features/calendar/
- Lunch Money, budget page: https://support.lunchmoney.app/guides/budgeting/step-3-navigating-the-budget-page

The sandbox could not open these pages directly, so this table rests on search
summaries of them. Copilot's colors come from a third-party teardown, not from
Copilot itself.

Last checked: 2026-10-19
