# Employer stock for Canadians (RSUs, ESPP, options)

How employer equity is taxed and how to manage the risk. Data: `data/corporate_stock.toml`.
This is education, not tax advice. Cross-border cases need a professional.

Last checked: 2026-10-18

## RSUs

- RSUs are **salary at vest**. The value on the vest day, in CAD at that day's rate, goes on your T4.
- Your **ACB** for those shares = that vest-day CAD value. You already paid tax on it.
- Payroll often sells some shares to cover tax (sell-to-cover). Withholding is often too low at high incomes. Expect a balance owing.
- Selling right after vest gives little or no extra tax. Holding is the same as buying company stock with a cash bonus.

## ESPP

- The discount (market price minus your price) is a **taxable benefit** when you buy.
- ACB = market value on the purchase date (your cost + the taxed benefit).
- A 15% discount sold right away is close to a free return of about 17.6%, before tax on the benefit.

## Identical shares and ACB

- All shares of the same company and class are **identical property**. Average the ACB across RSU, ESPP and bought shares.
- Shares sold within 30 days of acquiring them as an employee can be matched to that lot instead of averaged.
- Watch the 30-day superficial-loss rule: selling at a loss while new shares vest within 30 days can deny the loss.

## Stock options

- **Public company / non-CCPC**: benefit = (market price at exercise − strike) x shares. Taxed as salary in the year you exercise.
- **CCPC** (Canadian private company, arm's length): taxed when you sell the shares, not at exercise.
- **50% deduction** usually applies, so the benefit is taxed like a capital gain. The Budget 2024 cut to one-third was tied to the cancelled capital-gains change, so 50% stands.
- **200,000 vesting limit**: since July 1, 2021, big non-CCPC employers (revenue over 500 million) can give the deduction on only 200,000 of options vesting per year.

## US-listed employer shares

- **T1135**: file when the total cost of foreign property (including US shares in a normal broker account) is over **100,000 CAD** at any time in the year. RRSP and TFSA holdings do not count.
- **US estate tax**: non-US citizens with over **60,000 USD** of US assets at death must file Form 706-NA. The Canada-US treaty usually removes most of the tax, but the filing is still needed.
- **Dividends**: 15% US withholding with a W-8BEN on file (30% without). Claim the foreign tax credit in a non-registered account.
- **Currency**: compute every buy and sell in CAD on the transaction date.

## Concentration risk

- Your job already depends on the company. Keep employer stock under about **10% of net worth**.
- Sell vested shares on a fixed schedule. A plan removes emotion and blackout-window surprises.

## Blackout windows

- Many employees can trade only in open windows, often a few weeks after earnings.
- Plan sales and tax payments around them. Ask HR about an automatic disposition plan.

## Sources

- https://www.canada.ca/en/revenue-agency/services/tax/businesses/topics/payroll/benefits-allowances/security-options.html
- https://www.canada.ca/en/revenue-agency/services/tax/individuals/topics/about-your-tax-return/tax-return/completing-a-tax-return/personal-income/line-12700-capital-gains/definitions-capital-gains.html
- https://www.canada.ca/en/revenue-agency/services/tax/international-non-residents/information-been-moved/foreign-reporting/questions-answers-about-form-t1135.html
- https://www.irs.gov/individuals/international-taxpayers/some-nonresidents-with-us-assets-must-file-estate-tax-returns
- https://www.canada.ca/en/revenue-agency/services/tax/individuals/topics/about-your-tax-return/tax-return/completing-a-tax-return/deductions-credits-expenses/line-40500-federal-foreign-tax-credit.html
- https://www.investor.gov/introduction-investing/investing-basics/save-and-invest/diversify-your-investments
