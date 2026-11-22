//! Canadian personal income tax: federal plus the person's province or
//! territory, as `strategy/tax.py`. Simplified on purpose: for comparing
//! choices, not for filing.

use super::py::{self, getf, list, money, obj, pct};
use super::situation::Unported;
use super::{Ctx, Data, Section, Task, rrsp};
use crate::core::pyjson::Json;
use chrono::{Datelike, NaiveDate};

pub fn progressive(income: f64, brackets: &[Json]) -> f64 {
    let (mut tax, mut lower) = (0.0, 0.0);
    for b in brackets {
        let upper = py::num(b.get("up_to"));
        let top = upper.map_or(income, |u| income.min(u));
        if top > lower {
            tax += (top - lower) * getf(b, "rate", 0.0);
        }
        match upper {
            None => break,
            Some(u) if income <= u => break,
            Some(u) => lower = u,
        }
    }
    tax
}

fn basic(income: f64, part: &Json) -> f64 {
    let brackets = list(part, "brackets");
    let first = brackets.first().map_or(0.0, |b| getf(b, "rate", 0.0));
    let credit = getf(part, "basic_personal_amount", 0.0) * first;
    (progressive(income, brackets) - credit).max(0.0)
}

#[derive(Debug)]
pub struct IncomeTax {
    pub federal: f64,
    pub provincial: f64,
    pub total: f64,
    pub average: f64,
}

pub fn income_tax(income: f64, province: &str, data: &Json) -> IncomeTax {
    let mut fed = basic(income, obj(data, "federal"));
    let prov_data = obj(obj(data, "provinces"), province);
    let prov = basic(income, prov_data);
    let surtax = py::sum(
        list(prov_data, "surtax")
            .iter()
            .map(|s| getf(s, "rate", 0.0) * (prov - getf(s, "over", 0.0)).max(0.0)),
    );
    if prov_data.get("federal_abatement").is_some() {
        fed *= 1.0 - getf(prov_data, "federal_abatement", 0.0);
    }
    let total = fed + prov + surtax;
    IncomeTax {
        federal: fed,
        provincial: prov + surtax,
        total,
        average: if income != 0.0 { total / income } else { 0.0 },
    }
}

pub fn marginal(income: f64, province: &str, data: &Json) -> f64 {
    let step = 100.0;
    (income_tax(income + step, province, data).total - income_tax(income, province, data).total)
        / step
}

pub fn rrsp_refund(income: f64, contribution: f64, province: &str, data: &Json) -> f64 {
    income_tax(income, province, data).total
        - income_tax((income - contribution).max(0.0), province, data).total
}

/// (cpp, cpp2, ei), or None without payroll data.
pub fn payroll(income: f64, province: &str, data: &Json) -> Option<(f64, f64, f64)> {
    let p = obj(data, "payroll");
    if !p.truthy() {
        return None;
    }
    let qc = province == "QC";
    let (ympe, exempt) = (
        getf(p, "cpp_ympe", 0.0),
        getf(p, "cpp_basic_exemption", 3500.0),
    );
    let rate = getf(
        p,
        if qc && p.get("qpp_rate").is_some() {
            "qpp_rate"
        } else {
            "cpp_rate"
        },
        0.0,
    );
    let cpp = (income.min(ympe) - exempt).max(0.0) * rate;
    let cpp2 =
        (income.min(getf(p, "cpp2_yampe", ympe)) - ympe).max(0.0) * getf(p, "cpp2_rate", 0.0);
    let ei = income.min(getf(p, "ei_max_insurable", 0.0))
        * getf(p, if qc { "ei_rate_qc" } else { "ei_rate" }, 0.0);
    Some((cpp, cpp2, ei))
}

#[derive(Debug)]
pub struct TaxData {
    pub income: f64,
    pub marginal: f64,
    pub tax: IncomeTax,
    pub rrsp_status: String,
    pub rrsp: Vec<(String, Json)>,
}

fn day_of(v: &Json) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&v.py_str().chars().take(10).collect::<String>(), "%Y-%m-%d").ok()
}

pub fn build(ctx: &mut Ctx) -> Result<Section, Unported> {
    let sit = &ctx.sit;
    let mut sec = Section::new("tax", "Tax: rates and moves");
    let data = sit.knowledge("tax_ca_2026")?;
    let province = sit.province();
    if !obj(obj(&data, "provinces"), &province).truthy() {
        sec.line(format!(
            "- No tax data for {province} yet (knowledge/data/tax_ca_2026.toml)."
        ));
        return Ok(sec);
    }
    let inc = obj(&sit.profile, "income");
    let income =
        getf(inc, "employment", 0.0) + getf(inc, "self_employment", 0.0) + getf(inc, "other", 0.0);
    if income <= 0.0 {
        sec.line("- Add `[income]` (gross employment and other income) to profile.toml to see your rates.");
        return Ok(sec);
    }
    let t = income_tax(income, &province, &data);
    let m = marginal(income, &province, &data);
    let limits = obj(&data, "limits");
    sec.line(format!(
        "- Estimated 2026 income tax on {}: {} (average {}, marginal **{}**).",
        money(income),
        money(t.total),
        pct(t.average),
        pct(m)
    ));
    sec.line(format!(
        "- Capital gains: half is taxable, so a $1,000 gain costs about {}.",
        money(1000.0 * getf(limits, "capital_gains_inclusion", 0.5) * m)
    ));
    if let Some((cpp, cpp2, ei)) = payroll(getf(inc, "employment", 0.0), &province, &data) {
        sec.line(format!(
            "- Payroll contributions on employment income: CPP/QPP {}, EI {}.",
            money(cpp + cpp2),
            money(ei)
        ));
    }
    let room = obj(&sit.profile, "room");
    let rrsp_room = getf(room, "rrsp", 0.0);
    if rrsp_room > 0.0 {
        let refund = rrsp_refund(income, rrsp_room, &province, &data);
        sec.line(format!(
            "- Using all RRSP room ({}) cuts tax by about **{}** ({} back on each dollar on average: below your {} top rate, because the deduction reaches lower brackets).",
            money(rrsp_room),
            money(refund),
            pct(refund / rrsp_room),
            pct(m)
        ));
        if let Some(deadline) = obj(&data, "deadlines")
            .get("rrsp_contribution_deadline_for_2026")
            .filter(|v| v.truthy())
        {
            let mut task = Task::new(
                "rrsp-deadline",
                format!(
                    "Decide your RRSP contribution for 2026 (deadline {})",
                    deadline.py_str()
                ),
            )
            .notes("The contribution itself is a transfer you make.")
            .risk("money");
            task.due = day_of(deadline);
            sec.tasks.push(task);
        }
    }
    let limit = rrsp::check(&sit.profile, sit.holdings.as_ref(), sit.today, limits);
    sec.lines.extend(limit.lines.iter().cloned());
    sec.tasks.extend(limit.tasks.iter().cloned());
    let empty = Json::Obj(Vec::new());
    let holdings = sit.holdings.as_ref().unwrap_or(&empty);
    let year = sit.today.year();
    let this_year = list(holdings, "realized_by_year")
        .iter()
        .find(|g| py::num(g.get("year")).unwrap_or(0.0) as i32 == year);
    if let Some(g) = this_year.filter(|g| g.truthy()) {
        let net = getf(g, "net_cad", 0.0);
        let inclusion = getf(limits, "capital_gains_inclusion", 0.5);
        sec.line(format!(
            "- Realized capital gains so far in {year}: {} → about {} tax.",
            money(net),
            money(net.max(0.0) * inclusion * m)
        ));
        if net > 0.0 {
            sec.line(
                "- Tax-loss harvesting: selling positions at a loss offsets gains; avoid buying the same asset 30 days \
                 before or after (superficial loss).",
            );
        }
    }
    let flags: Vec<&Json> = list(holdings, "superficial_loss_flags")
        .iter()
        .filter(|f| py::num(f.get("year")).unwrap_or(0.0) as i32 == year)
        .collect();
    if !flags.is_empty() {
        let parts: Vec<String> = flags
            .iter()
            .map(|f| {
                format!(
                    "{} ×{}",
                    f.get("asset").map_or("".into(), Json::py_str),
                    f.get("count").map_or("".into(), Json::py_str)
                )
            })
            .collect();
        sec.line(format!(
            "- Possible superficial losses this year (a loss sale with a purchase within 30 days): {}. Those losses are denied and added to the cost base instead.",
            parts.join(", ")
        ));
    }
    if inc.get("self_employment").is_some_and(Json::truthy) {
        sec.line(
            "- Self-employed: you pay both halves of CPP; if you owe more than $3,000 two years running, CRA asks \
             for quarterly instalments. Over $30,000 of revenue in four quarters means GST/HST registration.",
        );
    }
    let mut due = NaiveDate::from_ymd_opt(sit.today.year(), 4, 30).expect("valid");
    if due < sit.today {
        due = NaiveDate::from_ymd_opt(sit.today.year() + 1, 4, 30).expect("valid");
    }
    sec.tasks.push(
        Task::new(
            format!("tax-filing-{}", due.year() - 1),
            format!("File your {} tax return", due.year() - 1),
        )
        .due(due)
        .notes("Self-employed: file by June 15, but pay any balance by April 30."),
    );
    sec.data = Data::Tax(TaxData {
        income,
        marginal: m,
        tax: t,
        rrsp_status: limit.status,
        rrsp: limit.data,
    });
    Ok(sec)
}
