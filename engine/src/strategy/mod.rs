//! Strategy sections, as `src/cashcast/strategy/`. Each one turns a
//! Situation into advice and dated tasks (decision 0010, step 4).

pub mod alternatives;
pub mod banking;
pub mod budget;
pub mod cashback;
pub mod credit;
pub mod crypto;
pub mod forecast;
pub mod foreign;
pub mod interest;
pub mod invest;
pub mod invest_plan;
pub mod net_total;
pub mod optimize;
pub mod payments;
pub mod plan;
pub mod py;
pub mod reserve;
pub mod review;
pub mod rrsp;
pub mod situation;
pub mod spending;
pub mod tax;

use crate::core::pyjson::Json;
use chrono::NaiveDate;
use situation::{Situation, Unported};

pub const DISCLAIMER: &str = "Education and arithmetic on your own data, not personal financial advice. \
This assistant is not a licensed financial advisor. Check facts marked 'estimate' before you act.";

/// A dated to-do for the owner's financial reminders list.
#[derive(Clone, Debug, Default)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub due: Option<NaiveDate>,
    pub notes: String,
    pub risk: String, // read | settings | money; money tasks are always done by the person
    pub service: String,
    pub action: String,
    /// "week", "2 weeks", "month" or "year" for a repeating task; "" when it happens once.
    pub recurs: String,
}

impl Task {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Task {
            id: id.into(),
            title: title.into(),
            risk: "read".into(),
            ..Default::default()
        }
    }
    pub fn due(mut self, d: NaiveDate) -> Self {
        self.due = Some(d);
        self
    }
    pub fn notes(mut self, n: &str) -> Self {
        self.notes = n.into();
        self
    }
    pub fn risk(mut self, r: &str) -> Self {
        self.risk = r.into();
        self
    }
    pub fn recurs(mut self, every: &str) -> Self {
        self.recurs = every.into();
        self
    }
}

/// What a section hands to later sections.
#[derive(Debug, Default)]
pub enum Data {
    #[default]
    None,
    Cashback(cashback::Plan),
    Kept(f64),
    Tax(tax::TaxData),
    Reserve(reserve::ReserveData),
    Interest(f64),
    Crypto(f64),
    InvestPlan(invest_plan::InvestPlanData),
    Forecast(forecast::ForecastData),
    Payments(payments::PaymentsData),
    Budget,
    Review(review::ReviewData),
    NetTotal(Json),
}

#[derive(Debug)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub lines: Vec<String>,
    pub tasks: Vec<Task>,
    pub data: Data,
}

impl Section {
    pub fn new(id: &str, title: &str) -> Self {
        Section {
            id: id.into(),
            title: title.into(),
            lines: Vec::new(),
            tasks: Vec::new(),
            data: Data::None,
        }
    }
    pub fn line(&mut self, l: impl Into<String>) {
        self.lines.push(l.into());
    }
    pub fn markdown(&self) -> String {
        let mut parts = vec![format!("## {}", self.title), String::new()];
        parts.extend(self.lines.iter().cloned());
        parts.push(String::new());
        parts.join("\n")
    }
}

/// One log event a section asks for: the name and its fields.
pub type Event = (String, Vec<(String, Json)>);

pub struct Ctx {
    pub sit: Situation,
    pub results: Vec<Section>,
    pub events: Vec<Event>,
}

impl Ctx {
    pub fn result(&self, id: &str) -> Option<&Section> {
        self.results.iter().find(|s| s.id == id)
    }
}

pub type Build = fn(&mut Ctx) -> Result<Section, Unported>;

/// The sections ported so far, in registry order.
pub const PORTED: &[(&str, Build)] = &[
    ("cashback", cashback::build),
    ("banking", banking::build),
    ("credit", credit::build),
    ("tax", tax::build),
    ("reserve", reserve::build),
    ("foreign", foreign::build),
    ("interest", interest::build),
    ("crypto", crypto::build),
    ("invest", invest::build),
    ("invest_plan", invest_plan::build),
    ("forecast", forecast::build),
    ("payments", payments::build),
    ("spending", spending::build),
    ("budget", budget::build),
    ("alternatives", alternatives::build),
    ("review", review::build),
    ("net_total", net_total::build),
];

/// Runs the ported sections in order and prints each one's markdown and tasks,
/// for the per-section parity check (`tests/parity/strategy-sections.sh`).
pub fn sections_main(args: &[String]) -> i32 {
    let flag = |n: &str| args.windows(2).find(|w| w[0] == n).map(|w| w[1].clone());
    let (Some(home), Some(today)) = (flag("--home"), flag("--today")) else {
        return 2;
    };
    let Ok(today) = NaiveDate::parse_from_str(&today, "%Y-%m-%d") else {
        return 2;
    };
    let home = std::path::PathBuf::from(home);
    let name = home
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let sit = match Situation::load(&home, &name, today) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e.0);
            return 64;
        }
    };
    let mut ctx = Ctx {
        sit,
        results: Vec::new(),
        events: Vec::new(),
    };
    for (_, build) in PORTED {
        match build(&mut ctx) {
            Ok(sec) => ctx.results.push(sec),
            Err(e) => {
                eprintln!("{}", e.0);
                return 64;
            }
        }
    }
    for s in &ctx.results {
        print!("{}", dump(s));
    }
    0
}

/// A section's markdown, then one line per task, as the parity check prints it.
pub fn dump(s: &Section) -> String {
    let mut out = format!("=== {}\n{}\n", s.id, s.markdown());
    for t in &s.tasks {
        out += &format!(
            "task {} | {} | {} | {} | {} | {} | {}\n",
            t.id,
            t.title,
            t.due.map_or("None".into(), |d| d.to_string()),
            t.notes,
            t.risk,
            t.service,
            t.action
        );
    }
    out
}
