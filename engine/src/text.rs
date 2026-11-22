//! Turns a transaction description into hints, then forgets the text.
//!
//! The description never leaves this module as text. Callers get a hashed
//! payee key (to group repeated deposits, such as paycheques), a hint about
//! what kind of money movement it is, and a spending category guess.

use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    None,
    /// A credit card payment, seen from either side.
    CardPayment,
    /// A move between the owner's own accounts.
    OwnTransfer,
    /// Money into or out of an investment account.
    Invest,
    /// Income tax paid.
    IncomeTax,
    /// A tax refund or government credit.
    TaxRefund,
    /// Cashback, rewards, interest or dividends received.
    Cashback,
    /// A payroll deposit.
    Payroll,
}

pub struct Derived {
    /// Hash of the first few words without digits; 0 when there is no text.
    pub payee_key: u64,
    pub hint: Hint,
    pub category: Option<&'static str>,
}

/// Spending categories by keyword, checked in order. Multi-word phrases that
/// share a word with a later entry come first (UBER EATS before UBER).
const CATEGORIES: &[(&str, &[&str])] = &[
    (
        "Groceries",
        &[
            "LOBLAWS",
            "NO FRILLS",
            "SOBEYS",
            "FRESHCO",
            "FOOD BASICS",
            "FARM BOY",
            "WHOLE FOODS",
            "TRADER JOE",
            "SAFEWAY",
            "KROGER",
            "COSTCO",
            "WALMART",
            "SUPERSTORE",
            "ALDI",
            "LIDL",
            "PUBLIX",
            "GROCERY",
            "GROCERIES",
            "SUPERMARKET",
            "LONGOS",
            "SAVE ON FOODS",
            "IGA",
            "MAXI",
            "PROVIGO",
            "INSTACART",
            "METRO INC",
            "WEGMANS",
            "HEB",
            "SPROUTS",
        ],
    ),
    (
        "Dining",
        &[
            "UBER EATS",
            "UBEREATS",
            "DOORDASH",
            "SKIP THE DISHES",
            "SKIPTHEDISHES",
            "GRUBHUB",
            "RESTAURANT",
            "CAFE",
            "COFFEE",
            "STARBUCKS",
            "TIM HORTONS",
            "TIMHORTONS",
            "MCDONALD",
            "MCDONALDS",
            "BURGER",
            "PIZZA",
            "SUSHI",
            "CHIPOTLE",
            "WENDYS",
            "KFC",
            "POPEYES",
            "BAR",
            "PUB",
            "BISTRO",
            "GRILL",
            "KITCHEN",
            "DINER",
            "BAKERY",
            "TAVERN",
            "RAMEN",
            "TACO",
        ],
    ),
    (
        "Transport",
        &[
            "UBER",
            "LYFT",
            "TAXI",
            "PRESTO",
            "TTC",
            "TRANSIT",
            "METROLINX",
            "GO TRANSIT",
            "VIA RAIL",
            "AMTRAK",
            "SHELL",
            "ESSO",
            "PETRO CANADA",
            "PETROCAN",
            "CHEVRON",
            "EXXON",
            "MOBIL",
            "ULTRAMAR",
            "FUEL",
            "GAS STATION",
            "PARKING",
            "IMPARK",
            "ZIPCAR",
            "407 ETR",
            "METRO CARD",
        ],
    ),
    (
        "Housing",
        &[
            "RENT",
            "LANDLORD",
            "PROPERTY MANAGEMENT",
            "MORTGAGE",
            "CONDO FEES",
            "HYDRO",
            "ENBRIDGE",
            "UTILITY",
            "UTILITIES",
            "ELECTRIC",
        ],
    ),
    (
        "Bills",
        &[
            "ROGERS",
            "BELL CANADA",
            "BELL MOBILITY",
            "TELUS",
            "FIDO",
            "KOODO",
            "FREEDOM MOBILE",
            "VERIZON",
            "AT T",
            "T MOBILE",
            "COMCAST",
            "XFINITY",
            "SPECTRUM",
            "INTERNET",
            "WIRELESS",
        ],
    ),
    (
        "Subscriptions",
        &[
            "NETFLIX",
            "SPOTIFY",
            "DISNEY",
            "HULU",
            "APPLE COM BILL",
            "ITUNES",
            "GOOGLE STORAGE",
            "GOOGLE ONE",
            "YOUTUBE",
            "AMAZON PRIME",
            "PRIME VIDEO",
            "CRAVE",
            "AUDIBLE",
            "PATREON",
            "OPENAI",
            "CHATGPT",
            "ICLOUD",
            "DROPBOX",
            "ADOBE",
        ],
    ),
    (
        "Health",
        &[
            "SHOPPERS DRUG",
            "PHARMACY",
            "REXALL",
            "CVS",
            "WALGREENS",
            "DENTAL",
            "DENTIST",
            "CLINIC",
            "MEDICAL",
            "PHYSIO",
            "MASSAGE",
            "OPTOMETR",
            "HOSPITAL",
            "GYM",
            "FITNESS",
            "GOODLIFE",
            "YMCA",
            "YOGA",
            "THERAPY",
        ],
    ),
    (
        "Shopping",
        &[
            "AMAZON",
            "AMZN",
            "IKEA",
            "BEST BUY",
            "APPLE STORE",
            "WINNERS",
            "MARSHALLS",
            "H M",
            "ZARA",
            "UNIQLO",
            "GAP",
            "OLD NAVY",
            "CANADIAN TIRE",
            "HOME DEPOT",
            "LOWES",
            "TARGET",
            "DOLLARAMA",
            "EBAY",
            "ETSY",
            "SEPHORA",
            "ALIEXPRESS",
            "TEMU",
            "SHEIN",
        ],
    ),
    (
        "Travel",
        &[
            "AIR CANADA",
            "WESTJET",
            "PORTER",
            "FLAIR",
            "AIRLINES",
            "AIRLINE",
            "AIRWAYS",
            "AIRBNB",
            "BOOKING COM",
            "EXPEDIA",
            "HOTEL",
            "HOTELS",
            "MARRIOTT",
            "HILTON",
            "HYATT",
            "VRBO",
            "HOSTEL",
        ],
    ),
    (
        "Entertainment",
        &[
            "CINEPLEX",
            "CINEMA",
            "TICKETMASTER",
            "STEAM",
            "PLAYSTATION",
            "XBOX",
            "NINTENDO",
            "THEATRE",
            "MUSEUM",
            "EVENTBRITE",
        ],
    ),
    (
        "Education",
        &["UDEMY", "COURSERA", "TUITION", "UNIVERSITY", "COLLEGE"],
    ),
    ("Pets", &["PETSMART", "PET VALU", "PETCO", "VETERINARY"]),
    (
        "Financial",
        &[
            "SERVICE CHARGE",
            "MONTHLY FEE",
            "ANNUAL FEE",
            "INTEREST CHARGE",
            "OVERDRAFT",
            "NSF",
            "FEE",
            "FEES",
            "ATM",
        ],
    ),
];

const PAYMENT_WORDS: &[&str] = &["PAYMENT", "PYMT", "PMT", "PAY", "AUTOPAY"];
const CARD_WORDS: &[&str] = &[
    "VISA",
    "MASTERCARD",
    "MC",
    "AMEX",
    "AMERICAN EXPRESS",
    "CREDIT CARD",
    "CARD",
    "THANK YOU",
    "AUTOPAY",
    "AUTO PAY",
];
const OWN_TRANSFER: &[&str] = &[
    "TRANSFER TO",
    "TRANSFER FROM",
    "TFR TO",
    "TFR FR",
    "TFR FROM",
    "XFER",
    "ONLINE TRANSFER",
    "INTERNET TRANSFER",
    "MOBILE TRANSFER",
    "ACCOUNT TRANSFER",
    "TRANSFER BETWEEN",
];
const PEOPLE_TRANSFER: &[&str] = &[
    "E TRANSFER",
    "ETRANSFER",
    "INTERAC",
    "ZELLE",
    "VENMO",
    "PAYPAL",
    "WIRE",
];
const INVEST: &[&str] = &[
    "QUESTRADE",
    "VANGUARD",
    "FIDELITY",
    "SCHWAB",
    "ROBINHOOD",
    "COINBASE",
    "KRAKEN",
    "BINANCE",
    "ETRADE",
    "E TRADE",
    "INTERACTIVE BROKERS",
    "TFSA",
    "RRSP",
    "FHSA",
    "RESP",
    "401K",
    "BROKERAGE",
    "INVESTMENT",
    "DIRECT INVESTING",
    "WEALTHFRONT",
    "BETTERMENT",
    "WEALTHSIMPLE INVEST",
];
const TAX: &[&str] = &[
    "CRA",
    "CANADA REVENUE",
    "REVENUE CANADA",
    "IRS",
    "TAX PAYMENT",
    "INCOME TAX",
    "HMRC",
    "FEDERAL TAX",
    "STATE TAX",
    "TAX REFUND",
    "TAX REF",
    "GST",
    "HST CREDIT",
    "CANADA FED",
];
const CASHBACK: &[&str] = &[
    "CASHBACK",
    "CASH BACK",
    "REWARDS",
    "REWARD",
    "INTEREST PAID",
    "INTEREST EARNED",
    "INTEREST CREDIT",
    "DIVIDEND",
];
const PAYROLL: &[&str] = &[
    "PAYROLL",
    "PAYCHECK",
    "PAYCHEQUE",
    "SALARY",
    "DIRECT DEP",
    "DIRECT DEPOSIT",
    "PAY DEP",
    "WAGES",
    "ADP",
    "CERIDIAN",
    "GUSTO",
];

/// Uppercases, keeps letters and digits, and pads with spaces so that
/// `contains(" WORD ")` matches whole words only.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(' ');
    let mut last_space = true;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_uppercase());
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    if !last_space {
        out.push(' ');
    }
    out
}

fn has(norm: &str, words: &[&str]) -> bool {
    words.iter().any(|w| norm.contains(&format!(" {w} ")))
}

pub fn derive(text: &str, cents: i64) -> Derived {
    let norm = normalize(text);
    if norm.trim().is_empty() {
        return Derived {
            payee_key: 0,
            hint: Hint::None,
            category: None,
        };
    }
    let key_words: Vec<&str> = norm
        .split_whitespace()
        .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
        .take(3)
        .collect();
    let payee_key = if key_words.is_empty() {
        0
    } else {
        let mut h = DefaultHasher::new();
        key_words.hash(&mut h);
        h.finish()
    };

    let hint = if has(&norm, PAYMENT_WORDS) && has(&norm, CARD_WORDS) {
        Hint::CardPayment
    } else if has(&norm, OWN_TRANSFER) && !has(&norm, PEOPLE_TRANSFER) {
        Hint::OwnTransfer
    } else if has(&norm, INVEST) {
        Hint::Invest
    } else if has(&norm, TAX) {
        if cents < 0 {
            Hint::IncomeTax
        } else {
            Hint::TaxRefund
        }
    } else if cents > 0 && has(&norm, CASHBACK) {
        Hint::Cashback
    } else if cents > 0 && has(&norm, PAYROLL) {
        Hint::Payroll
    } else {
        Hint::None
    };
    let category = CATEGORIES
        .iter()
        .find(|(_, words)| has(&norm, words))
        .map(|(name, _)| *name);
    Derived {
        payee_key,
        hint,
        category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_and_categories() {
        assert_eq!(
            derive("PAYMENT - THANK YOU", 50_000).hint,
            Hint::CardPayment
        );
        assert_eq!(
            derive("BANK VISA PAYMENT 1234", -50_000).hint,
            Hint::CardPayment
        );
        assert_eq!(
            derive("Online transfer to savings", -10_000).hint,
            Hint::OwnTransfer
        );
        assert_eq!(
            derive("INTERAC e-Transfer to friend", -10_000).hint,
            Hint::None
        );
        assert_eq!(derive("QUESTRADE INC", -100_000).hint, Hint::Invest);
        assert_eq!(derive("CRA TAX PAYMENT", -100_000).hint, Hint::IncomeTax);
        assert_eq!(derive("CANADA FED GST", 12_000).hint, Hint::TaxRefund);
        assert_eq!(derive("ACME CORP PAYROLL", 300_000).hint, Hint::Payroll);
        assert_eq!(derive("UBER EATS 123", -2_000).category, Some("Dining"));
        assert_eq!(derive("UBER TRIP 123", -2_000).category, Some("Transport"));
        assert_eq!(
            derive("Metroplex parking", -2_000).category,
            Some("Transport")
        );
        assert_eq!(derive("Barber shop", -2_000).category, None);
    }

    #[test]
    fn payee_key_ignores_digits_and_case() {
        let a = derive("ACME CORP PAYROLL 2026-05-08 #1234", 1).payee_key;
        let b = derive("Acme Corp Payroll 2026-05-22 #9876", 1).payee_key;
        assert_eq!(a, b);
        assert_ne!(a, derive("Other Inc payroll", 1).payee_key);
        assert_eq!(derive("", 1).payee_key, 0);
    }
}
