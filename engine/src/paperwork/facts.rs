//! Canadian citizenship, grant for adults (18 and over), ported from
//! sidequest's `paperwork-citizenship.ts`. Built only from official IRCC
//! pages: every item cites its page, a short quote that `paperwork sources
//! --live` looks for, and the day it was checked. Rules and fees change:
//! re-check before relying on them.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Source {
    pub url: &'static str,
    pub title: &'static str,
    pub quote: &'static str,
    pub checked: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub how: &'static str,
    /// check, doc, form, fee, test or owner.
    pub kind: &'static str,
    /// Only the owner does it: sign in, type ID numbers, pay, sign, submit.
    pub owner_only: bool,
    /// age-18-54, online, paper, if-applicable or police.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<&'static str>,
    /// presence or police.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<&'static str>,
    pub sources: Vec<Source>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Step {
    pub id: &'static str,
    pub title: &'static str,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Rules {
    pub presence_days: i64,
    pub window_years: u32,
    pub temp_factor: f64,
    pub temp_cap: i64,
    pub police_days: i64,
    pub police_years: u32,
    pub test_window_days: i64,
    pub age_min: u32,
    pub age_max: u32,
    pub fee: i64,
    pub sources: Vec<Source>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Process {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub rules: Rules,
    pub steps: Vec<Step>,
}

pub const CHECKED: &str = "2026-11-01";
pub const CITIZENSHIP_ID: &str = "ca-citizenship-adult";

macro_rules! cit {
    ($p:literal) => {
        concat!(
            "https://www.canada.ca/en/immigration-refugees-citizenship/services/canadian-citizenship",
            $p
        )
    };
}
macro_rules! guides {
    ($p:literal) => {
        concat!(
            "https://www.canada.ca/en/immigration-refugees-citizenship/services/application/application-forms-guides",
            $p
        )
    };
}

// The pages' own (canonical) addresses: older links redirect to these.
const ELIGIBILITY: &str = cit!("/adult-minor/who.html");
const FORMS: &str = cit!("/adult-minor/how/adult-forms-documents.html");
const APPLY: &str = cit!("/adult-minor/how.html");
const CALCULATOR: &str = cit!("/adult-minor/how/physical-presence.html");
const PAPER_FORM: &str = guides!("/cit0002.html");
const CIT0407: &str = "https://www.canada.ca/content/dam/ircc/documents/pdf/english/kits/forms/cit0407/01-05-2019/cit0407e.pdf";
const FEES: &str = "https://ircc.canada.ca/english/information/fees/fees.asp";
const PAY: &str = "https://ircc.canada.ca/english/information/fees/pay.asp";
const ONLINE: &str = "https://citapply-citdemande.apps.cic.gc.ca/en/landing";
const AFTER: &str = cit!("/adult-minor/after.html");
const CEREMONY_PAGE: &str = cit!("/become-canadian-citizen/citizenship-ceremony.html");
const BRING: &str = cit!("/become-canadian-citizen/citizenship-ceremony/bring.html");
const CERT_AFTER: &str = cit!("/become-canadian-citizen/citizenship-ceremony/after.html");
const PROHIBITIONS: &str =
    cit!("/become-canadian-citizen/eligibility/situtations-prevent-citizenship.html");
const LANGUAGE_PROOF: &str = cit!("/become-canadian-citizen/eligibility/language-proof.html");
const PHOTOS: &str = guides!("/citizenship-application-photograph-specifications.html");
const CHECKLIST: &str = guides!("/cit0007.html");
const TEST_HOW: &str = cit!("/test/how-it-works.html");
const TEST_STUDY: &str = cit!("/test/study.html");

const fn src(url: &'static str, title: &'static str, quote: &'static str) -> Source {
    Source {
        url,
        title,
        quote,
        checked: CHECKED,
    }
}

const PP: &str = "Eligibility: physical presence";
const C407: &str = "CIT 0407: Physical presence calculation (paper form)";
const GUIDE: &str = "Paper application guide (CIT 0002)";
const QUESTIONS: &str = "Application questions (CIT 0002)";
const ADULT_FORMS: &str = "Adults: forms and documents";
const HOW: &str = "Apply for citizenship: adults";
const AFTER_T: &str = "After you apply";
const CERT_T: &str = "After the citizenship ceremony";

pub const PRESENCE: Source = src(ELIGIBILITY, PP, "at least 1,095 days");
const BUFFER: Source = src(ELIGIBILITY, PP, "apply with more than 1,095 days");
const WINDOW: Source = src(ELIGIBILITY, PP, "5 years before the date you sign");
const HALF_DAY: Source = src(ELIGIBILITY, PP, "counts as 0.5 days");
const CAP: Source = src(ELIGIBILITY, PP, "maximum of 365 days");
const TRAVEL_DAYS: Source = src(CIT0407, C407, "day you left and day you returned");
const UNSURE: Source = src(CIT0407, C407, "do NOT include that period");
const PERMITS: Source = src(
    PAPER_FORM,
    GUIDE,
    "these documents did not grant you temporary resident status",
);
const PROTECTED: Source = src(PAPER_FORM, GUIDE, "positive Protected Person decision");
const PR: Source = src(
    ELIGIBILITY,
    "Eligibility: permanent resident status",
    "valid permanent resident (PR) status",
);
const TAXES: Source = src(
    ELIGIBILITY,
    "Eligibility: file taxes",
    "at least 3 years during the 5 years",
);
const PROHIB: Source = src(
    PROHIBITIONS,
    "Situations that may prevent you from becoming a citizen",
    "prohibited under the Citizenship Act",
);
const LANGUAGE: Source = src(ELIGIBILITY, "Eligibility: language skills", "level 4");
const LANGUAGE_PROOF_S: Source = src(
    LANGUAGE_PROOF,
    "Proof of language skills",
    "Level 4 or higher in English or French",
);
const PASSPORTS: Source = src(
    FORMS,
    ADULT_FORMS,
    "identity pages of your passports and travel documents",
);
const IDS: Source = src(FORMS, ADULT_FORMS, "2 pieces of personal identification");
const LANG_DOCS: Source = src(FORMS, ADULT_FORMS, "CELPIP-General LS");
const POLICE: Source = src(FORMS, ADULT_FORMS, "183 days or more in a row");
pub const POLICE_COUNTRIES: Source = src(
    PAPER_FORM,
    GUIDE,
    "you were not in a single country for 183 days or more in a row",
);
const POLICE_ORIGIN: Source = src(
    FORMS,
    ADULT_FORMS,
    "if you were in your country of origin immediately before becoming a permanent resident",
);
const POLICE_VALID: Source = src(
    FORMS,
    ADULT_FORMS,
    "within 6 months of signing your application form",
);
const TRANSLATION: Source = src(FORMS, ADULT_FORMS, "certified translation");
const UNCERTIFIED: Source = src(
    FORMS,
    ADULT_FORMS,
    "You can also use an uncertified translator",
);
const AFFIDAVIT: Source = src(FORMS, ADULT_FORMS, "an affidavit (sworn statement)");
const ADDRESSES: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "Write all your addresses inside and outside of Canada during your 5-year eligibility period",
);
const ACTIVITIES: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "You must list all your work and study history",
);
const NAME_CHANGE: Source = src(FORMS, ADULT_FORMS, "If you legally changed your name");
const PHOTOS_S: Source = src(
    PHOTOS,
    "Citizenship photo specifications",
    "Citizenship photo specifications",
);
const CHECKLIST_S: Source = src(CHECKLIST, "Document checklist (CIT 0007)", "CIT 0007");
pub const FEE: Source = src(FEES, "IRCC fee list: citizenship", "653.00");
const PAY_ONLINE: Source = src(
    PAY,
    "Pay your fees online",
    "some online citizenship applications",
);
const FEE_RIGHT: Source = src(
    FEES,
    "IRCC fee list: citizenship",
    "Right of Citizenship 123.00",
);
const PAY_WHEN_READY: Source = src(APPLY, HOW, "Pay only when you");
const ONLINE_S: Source = src(APPLY, HOW, "Most applicants can now apply online");
const PAPER_ONLY: Source = src(
    APPLY,
    HOW,
    "include time spent outside Canada as a Crown servant",
);
pub const PAPER_REP: Source = src(
    APPLY,
    HOW,
    "You want your representative to complete and submit your application",
);
const SIGN_IN: Source = src(
    ONLINE,
    "Citizenship application: sign in",
    "Apply for Canadian citizenship online",
);
const PAPER_FORM_S: Source = src(PAPER_FORM, GUIDE, "CIT 0002");
const PRINTOUT: Source = src(
    PAPER_FORM,
    GUIDE,
    "You must complete and submit the printout of your Online Physical Presence Calculation",
);
const SAME_DAY: Source = src(
    PAPER_FORM,
    GUIDE,
    "application date on the physical presence calculator printout are the same",
);
const HAND_SIGN: Source = src(
    PAPER_FORM,
    GUIDE,
    "Print and provide a handwritten signature",
);
const RECEIVED: Source = src(
    PAPER_FORM,
    GUIDE,
    "dated more than 90 days before we receive it",
);
const REPRESENTATIVE: Source = src(PAPER_FORM, GUIDE, "Use of a Representative Form (IMM 5476)");
const EVENTS: Source = src(
    APPLY,
    HOW,
    "attend a required event (test, interview, hearing or ceremony)",
);
const CALCULATOR_S: Source = src(
    CALCULATOR,
    "Physical presence calculator",
    "must still use the calculator in your account",
);
const SIGN: Source = src(APPLY, HOW, "sign, date and submit your online application");
const TEST_WHO: Source = src(
    TEST_HOW,
    "How the citizenship test works",
    "between 18 and 54 years old on the day you sign your application",
);
const TEST_WINDOW: Source = src(
    TEST_HOW,
    "How the citizenship test works",
    "30 days to take this test",
);
const TEST_FORMAT: Source = src(
    TEST_STUDY,
    "Study for the test",
    "15 of the 20 questions right",
);
const TEST_CHANCES: Source = src(
    TEST_STUDY,
    "Study for the test",
    "3 chances to pass the test",
);
// The online form asks what the paper guide's questions ask (CIT 0002, questions 1 to 19).
const SERVICE_LANG: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "in which you would like to receive your service",
);
const ACCOMMODATION: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "special needs that require accommodation",
);
const UCI: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "The UCI is an eight or ten-digit number",
);
const FULL_NAME: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "Your full name as shown on your Record of Landing",
);
const OTHER_NAMES: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "We need to know all of the names you have ever used",
);
const MARITAL: Source = src(PAPER_FORM, QUESTIONS, "Choose your current marital status");
const CONTACT: Source = src(PAPER_FORM, QUESTIONS, "Home Address in Canada");
const HELPED: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "Tell us if someone helped you complete your application package",
);
const OTHER_STATUS: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "permanent resident status and/or citizenship in any other country",
);
const PASSPORTS_HELD: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "Tell us if you have held passports or travel documents during your five (5) year eligibility period",
);
const SIN: Source = src(PAPER_FORM, QUESTIONS, "social insurance number (SIN)");
const CRA_CONSENT: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "to provide details of your tax filing information",
);
const CERTIFICATE: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "either paper or electronic citizenship certificate",
);
const MP: Source = src(PAPER_FORM, QUESTIONS, "federal Member of Parliament (MP)");
const ELECTORS: Source = src(PAPER_FORM, QUESTIONS, "National Register of Electors");
const CITY: Source = src(
    PAPER_FORM,
    QUESTIONS,
    "Type the city where you are applying from",
);
// After you apply.
const NOT_AOR: Source = src(
    AFTER,
    AFTER_T,
    "is not the acknowledgement of receipt (AOR)",
);
const AOR: Source = src(
    AFTER,
    AFTER_T,
    "The AOR has your application number and unique client identifier (UCI)",
);
const RECALC: Source = src(AFTER, AFTER_T, "recalculate the physical presence");
const STATUS: Source = src(
    AFTER,
    AFTER_T,
    "You can check your application status online",
);
const KEEP_PR: Source = src(AFTER, AFTER_T, "keep your permanent resident (PR) status");
const REPLY: Source = src(AFTER, AFTER_T, "usually within 30 days");
const AWAY: Source = src(
    AFTER,
    AFTER_T,
    "plan to leave Canada for more than 2 weeks in a row",
);
const CHARGED: Source = src(AFTER, AFTER_T, "are charged with a crime");
const MOVE: Source = src(AFTER, AFTER_T, "even if you arrange for mail forwarding");
const FINGERPRINTS: Source = src(AFTER, AFTER_T, "If we ask you to submit your fingerprints");
const NO_SERVICE_CANADA: Source = src(AFTER, AFTER_T, "Do not go to a Service Canada Centre");
const QUALITY: Source = src(
    AFTER,
    AFTER_T,
    "Quality Assurance Program randomly selects applications",
);
const BRING_INVITE: Source = src(
    BRING,
    "What to bring to the ceremony",
    "your ceremony invitation",
);
const BRING_IDS: Source = src(
    BRING,
    "What to bring to the ceremony",
    "At least 1 ID must have both your photo and signature on it",
);
const PR_CARD_GONE: Source = src(
    CERT_AFTER,
    CERT_T,
    "is destroyed or collected at the ceremony",
);
const CHECK_CERT: Source = src(
    CERT_AFTER,
    CERT_T,
    "check if the personal information on the front and back of your certificate is accurate",
);
const PASSPORT_AFTER: Source = src(
    CERT_AFTER,
    CERT_T,
    "You can only apply for a passport after you get your Canadian citizenship certificate",
);
const OATH: Source = src(
    CEREMONY_PAGE,
    "Citizenship ceremony",
    "take the oath of citizenship",
);

#[allow(clippy::too_many_arguments)]
fn item(
    id: &'static str,
    kind: &'static str,
    owner_only: bool,
    when: Option<&'static str>,
    auto: Option<&'static str>,
    title: &'static str,
    how: &'static str,
    sources: &[Source],
) -> Item {
    Item {
        id,
        title,
        how,
        kind,
        owner_only,
        when,
        auto,
        sources: sources.to_vec(),
    }
}

const AGE: Option<&str> = Some("age-18-54");
const IF: Option<&str> = Some("if-applicable");
const ON: Option<&str> = Some("online");
const PAPER: Option<&str> = Some("paper");

pub fn citizenship() -> Process {
    Process {
        id: CITIZENSHIP_ID,
        title: "Canadian citizenship (adult)",
        summary: "Become a Canadian citizen as an adult permanent resident. Check you can apply, gather documents, pay, apply, then take the test and attend the ceremony.",
        rules: Rules {
            presence_days: 1095,
            window_years: 5,
            temp_factor: 0.5,
            temp_cap: 365,
            police_days: 183,
            police_years: 4,
            test_window_days: 30,
            age_min: 18,
            age_max: 54,
            fee: 653,
            sources: vec![PRESENCE, WINDOW, HALF_DAY, CAP, TRAVEL_DAYS],
        },
        steps: vec![
            Step {
                id: "eligibility",
                title: "Check you can apply",
                items: vec![
                    item(
                        "pr-status",
                        "check",
                        false,
                        None,
                        None,
                        "You have valid permanent resident status",
                        "Confirm you are a permanent resident, have no removal order, and met any conditions of your PR (such as medical checks).",
                        &[PR],
                    ),
                    item(
                        "presence",
                        "check",
                        false,
                        None,
                        Some("presence"),
                        "At least 1,095 days in Canada in the 5 years before you sign",
                        "Enter the day you became a PR and every trip outside Canada. The day you leave and the day you come back both count as days in Canada. Days in Canada before PR, as a temporary resident or protected person, count as half days, up to 365. Count only real status: not work or study permits from a refugee claim, and as a protected person only from the positive decision. Unsure about a period? Leave it out. IRCC suggests applying with more than 1,095 days.",
                        &[
                            PRESENCE,
                            WINDOW,
                            HALF_DAY,
                            CAP,
                            TRAVEL_DAYS,
                            PERMITS,
                            PROTECTED,
                            UNSURE,
                            BUFFER,
                        ],
                    ),
                    item(
                        "taxes",
                        "check",
                        false,
                        None,
                        None,
                        "Taxes filed for at least 3 of the 5 years, if you had to file",
                        "Check your filed years in your CRA account. The application asks whether you had to file and whether you did.",
                        &[TAXES, SIN, CRA_CONSENT],
                    ),
                    item(
                        "prohibitions",
                        "check",
                        false,
                        None,
                        None,
                        "No prohibition applies to you",
                        "Read the official page on situations that prevent citizenship (mostly criminal or security grounds). Time in prison, on parole or on probation does not count as days in Canada.",
                        &[PROHIB],
                    ),
                    item(
                        "language-level",
                        "check",
                        false,
                        AGE,
                        None,
                        "You speak and listen in English or French at CLB/NCLC level 4",
                        "Needed if you are 18 to 54 on the day you sign. Pick the proof you will send in step 2.",
                        &[LANGUAGE],
                    ),
                ],
            },
            Step {
                id: "documents",
                title: "Gather documents",
                items: vec![
                    item(
                        "passport-pages",
                        "doc",
                        false,
                        None,
                        None,
                        "Colour copies of your passport and travel document pages",
                        "Every passport or travel document you held in the 5 years before you sign: the identity pages and any extension stamp pages.",
                        &[PASSPORTS, PASSPORTS_HELD],
                    ),
                    item(
                        "two-ids",
                        "doc",
                        false,
                        None,
                        None,
                        "Copies of two pieces of personal ID",
                        "Each shows your name, photo and date of birth (both sides if printed on both). If one is your PR card, the other must be issued by a federal, provincial or territorial government.",
                        &[IDS],
                    ),
                    item(
                        "language-proof",
                        "doc",
                        false,
                        AGE,
                        None,
                        "One proof of language skills",
                        "Send one of these:\n• a diploma or transcript from a program taught in English or French\n• an approved test result, such as CELPIP-General, CELPIP-General LS, IELTS General Training, PTE Core, TEF or TCF\n• a LINC or CLIC certificate\n• an approved provincial program certificate",
                        &[LANG_DOCS, LANGUAGE_PROOF_S],
                    ),
                    item(
                        "police-certificate",
                        "doc",
                        false,
                        Some("police"),
                        Some("police"),
                        "Police certificate from each country where you spent 183+ days in a row",
                        "Needed for each country outside Canada where you were 183 days or more in a row in the last 4 years, since age 18. Time before you became a PR counts too. Not needed for your country of origin if you lived there right before you became a PR. A long trip through several countries, with no 183 days in a row in one of them, needs none: mark it not needed. It must be issued after your last stay there, or within 6 months of the day you sign. Ask early: it can take months.",
                        &[POLICE, POLICE_ORIGIN, POLICE_COUNTRIES, POLICE_VALID],
                    ),
                    item(
                        "translations",
                        "doc",
                        false,
                        IF,
                        None,
                        "Certified translations",
                        "For every document that is not in English or French, send the translation with the original. A certified translator stamps it. Any other translator must be fluent in both languages, must not be you or a close relative, and must add a sworn affidavit.",
                        &[TRANSLATION, UNCERTIFIED, AFFIDAVIT],
                    ),
                    item(
                        "name-change",
                        "doc",
                        false,
                        IF,
                        None,
                        "Name change documents",
                        "Only if you legally changed your name. What to send depends on where and why it changed.",
                        &[NAME_CHANGE],
                    ),
                    item(
                        "history",
                        "doc",
                        false,
                        None,
                        None,
                        "Your addresses and what you did, for the 5 years",
                        "The application asks for every address inside and outside Canada, and all work and study (language classes too), in the 5 years before you sign. Write them down before you start.",
                        &[ADDRESSES, ACTIVITIES],
                    ),
                    item(
                        "photos",
                        "doc",
                        false,
                        PAPER,
                        None,
                        "Two identical citizenship photos",
                        "Paper applications only: two printed colour photos that meet the citizenship photo specifications.",
                        &[PHOTOS_S],
                    ),
                    item(
                        "checklist-cit0007",
                        "form",
                        false,
                        PAPER,
                        None,
                        "Document checklist (CIT 0007)",
                        "Paper applications only: fill it in and put it on top of your package.",
                        &[CHECKLIST_S],
                    ),
                ],
            },
            Step {
                id: "answers",
                title: "Prepare your form answers",
                items: vec![
                    item(
                        "service-language",
                        "form",
                        false,
                        None,
                        None,
                        "Your service language and any accommodation",
                        "The form asks which language IRCC should serve you in (English or French), and whether you need an accommodation, such as wheelchair access, a sign language interpreter or study materials in large print, audio or Braille.",
                        &[SERVICE_LANG, ACCOMMODATION],
                    ),
                    item(
                        "personal-details",
                        "form",
                        false,
                        None,
                        None,
                        "Your UCI, names and marital status",
                        "Have these ready before you start the form. Your UCI (client ID) is 8 or 10 digits: on your PR card it is next to the photo; on your Confirmation of PR it is at the top right. Type your name exactly as it is on your PR card or Confirmation of PR. List every other name you have used (birth name, name before marriage, nicknames).",
                        &[UCI, FULL_NAME, OTHER_NAMES, MARITAL],
                    ),
                    item(
                        "contact",
                        "form",
                        false,
                        None,
                        None,
                        "Your home address, phone and email",
                        "The form asks for your home address in Canada (street words in full, such as Street or Avenue), a phone number with its country code, and an email address. IRCC sends its letters and emails there.",
                        &[CONTACT],
                    ),
                    item(
                        "other-status",
                        "form",
                        false,
                        None,
                        None,
                        "Status you held in other countries",
                        "The form asks whether you ever held citizenship, permanent residence or another immigration status (student, worker, refugee) in another country, such as your country of birth, and which status.",
                        &[OTHER_STATUS],
                    ),
                    item(
                        "consents",
                        "form",
                        false,
                        None,
                        None,
                        "Your certificate type and two consents",
                        "Pick a paper or an electronic citizenship certificate (an e-certificate needs an email address in the form). Then say yes or no to two consents: IRCC may give your name, address and language to your federal MP for a letter of congratulations, and may add you to the National Register of Electors (Elections Canada).",
                        &[CERTIFICATE, MP, ELECTORS],
                    ),
                ],
            },
            Step {
                id: "fee",
                title: "Pay the fee",
                items: vec![
                    item(
                        "pay-fee",
                        "fee",
                        true,
                        None,
                        None,
                        "Pay $653 (adult fee, includes the $123 right of citizenship fee)",
                        "Only you pay, and only when you are ready to submit. Pay on the official page (Pay your fees online, linked below). Check the amount on the fee list first: fees change.",
                        &[FEE, FEE_RIGHT, PAY_WHEN_READY, PAY_ONLINE],
                    ),
                    item(
                        "fee-receipt",
                        "doc",
                        false,
                        None,
                        None,
                        "Save the fee receipt",
                        "Upload the receipt in your online account (online) or print it and include it (paper).",
                        &[PAY_WHEN_READY],
                    ),
                ],
            },
            Step {
                id: "apply",
                title: "Fill in and submit",
                items: vec![
                    item(
                        "online-account",
                        "owner",
                        true,
                        ON,
                        None,
                        "Sign in to your IRCC citizenship account",
                        "Only you sign in, on the official page linked below. Never share the password. Most people apply online. You must apply on paper if you count time outside Canada as a Crown servant, or if you want a representative to fill in and submit the application for you.",
                        &[SIGN_IN, ONLINE_S, PAPER_ONLY, PAPER_REP],
                    ),
                    item(
                        "account-calculator",
                        "form",
                        true,
                        ON,
                        None,
                        "Use the physical presence calculator in your account",
                        "Online applications must use the calculator inside the account. Copy each trip from the Days in Canada card.",
                        &[CALCULATOR_S],
                    ),
                    item(
                        "fill-form",
                        "form",
                        true,
                        ON,
                        None,
                        "Fill in the online application",
                        "Only you type passport, PR card and other ID numbers into the official form. Upload the documents from step 2.",
                        &[ONLINE_S],
                    ),
                    item(
                        "fill-paper-form",
                        "form",
                        true,
                        PAPER,
                        None,
                        "Fill in the paper application (CIT 0002)",
                        "Only you type passport, PR card and other ID numbers into the form.",
                        &[PAPER_FORM_S],
                    ),
                    item(
                        "presence-printout",
                        "form",
                        false,
                        PAPER,
                        None,
                        "Print your physical presence calculation",
                        "Include the printout of the online physical presence calculator, or form CIT 0407 if you cannot use it. Its application date must be the day you sign.",
                        &[PRINTOUT, SAME_DAY],
                    ),
                    item(
                        "representative",
                        "form",
                        false,
                        IF,
                        None,
                        "Use of a Representative form (IMM 5476)",
                        "Only if a person, firm or organization acts for you. Send it with your application. Online, a representative may help, but you fill in and submit the application yourself; if they are to submit it for you, apply on paper.",
                        &[REPRESENTATIVE, PAPER_REP, HELPED],
                    ),
                    item(
                        "sign",
                        "owner",
                        true,
                        None,
                        None,
                        "Sign and date the application yourself",
                        "The date you sign sets your 5-year window. Sign only on or after your earliest sign day from the Days in Canada card. On paper, sign by hand. The form also asks the city you apply from.",
                        &[SIGN, WINDOW, HAND_SIGN, CITY],
                    ),
                    item(
                        "submit",
                        "owner",
                        true,
                        ON,
                        None,
                        "Submit the application",
                        "Only you submit. Save the confirmation it shows.",
                        &[SIGN],
                    ),
                    item(
                        "mail",
                        "owner",
                        true,
                        PAPER,
                        None,
                        "Mail the application",
                        "Only you send it, to the address in the paper guide. IRCC must receive it within 90 days of the day you sign, or it comes back.",
                        &[RECEIVED],
                    ),
                ],
            },
            Step {
                id: "after",
                title: "After you apply",
                items: vec![
                    item(
                        "aor",
                        "owner",
                        false,
                        None,
                        None,
                        "Wait for the acknowledgement of receipt (AOR)",
                        "First you get a confirmation email: it is not the AOR. IRCC then checks that your application is complete and sends the AOR, with your application number and UCI. Watch your email and spam folder for mail from an address ending in @cic.gc.ca. If the application comes back incomplete: add what is missing, recalculate the physical presence, sign and date it again, and send it again. After the AOR, check your status online.",
                        &[NOT_AOR, AOR, RECALC, STATUS],
                    ),
                    item(
                        "keep-updated",
                        "owner",
                        true,
                        None,
                        None,
                        "While you wait: keep your PR status and tell IRCC about changes",
                        "Until the oath, keep your PR status, read IRCC's letters and emails, and answer them in time (usually within 30 days). Tell IRCC right away if you move (mail forwarding is not enough), change your email or phone, plan to leave Canada for more than 2 weeks in a row, or are charged with a crime. You may travel, but you must be in Canada for the test, any interview and the ceremony.",
                        &[KEEP_PR, REPLY, MOVE, AWAY, CHARGED],
                    ),
                    item(
                        "fingerprints",
                        "owner",
                        true,
                        IF,
                        None,
                        "Give fingerprints, only if IRCC asks",
                        "If IRCC asks, pay for electronic fingerprints at a local police service, the RCMP, or an RCMP-accredited company, and bring the request letter. Do not go to a Service Canada Centre: those fingerprints are not accepted.",
                        &[FINGERPRINTS, NO_SERVICE_CANADA],
                    ),
                    item(
                        "study",
                        "test",
                        false,
                        AGE,
                        None,
                        "Study with Discover Canada",
                        "The test has 20 multiple-choice or true-or-false questions in 45 minutes; you pass with 15 of 20 right. The official study guide is Discover Canada.",
                        &[TEST_FORMAT],
                    ),
                    item(
                        "test",
                        "test",
                        true,
                        AGE,
                        None,
                        "Take the citizenship test",
                        "Needed if you are 18 to 54 on the day you sign. You get 30 days from the invitation to take it online (webcam on), and three chances to pass. Enter the invitation date to see your deadline.",
                        &[TEST_WHO, TEST_WINDOW, TEST_CHANCES],
                    ),
                    item(
                        "events",
                        "owner",
                        true,
                        IF,
                        None,
                        "Go to any interview, hearing or re-test you are invited to",
                        "IRCC may invite you to one after the test. IRCC also picks some applications at random for a closer review with an interview. If you miss a required event, IRCC can close your application.",
                        &[EVENTS, QUALITY],
                    ),
                    item(
                        "ceremony-bring",
                        "doc",
                        false,
                        None,
                        None,
                        "Get ready for the ceremony",
                        "Bring your ceremony invitation, your PR card (even if it has expired) or your Confirmation of Permanent Residence, and 2 pieces of ID. At least 1 ID must have both your photo and your signature. A virtual ceremony also needs scissors to cut up your PR card. Keep your Confirmation of PR: you may need it later for benefits.",
                        &[BRING_INVITE, BRING_IDS],
                    ),
                    item(
                        "oath",
                        "owner",
                        true,
                        None,
                        None,
                        "Attend the ceremony and take the oath",
                        "After you are approved you get an invitation. Bring what it lists.",
                        &[OATH],
                    ),
                    item(
                        "certificate-passport",
                        "owner",
                        true,
                        None,
                        None,
                        "Check your certificate, then apply for a passport",
                        "Check that the details on the front and back of your citizenship certificate are right, and do not laminate it. Your PR card is collected or destroyed at the ceremony, so you need a Canadian passport to travel and come back. You can apply for one only after you get the certificate.",
                        &[CHECK_CERT, PR_CARD_GONE, PASSPORT_AFTER],
                    ),
                ],
            },
        ],
    }
}

/// Every process paperwork knows.
pub fn processes() -> Vec<Process> {
    vec![citizenship()]
}

/// True for an official IRCC host: canada.ca, ircc.canada.ca, *.cic.gc.ca.
pub fn official(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    host == "canada.ca"
        || host == "www.canada.ca"
        || host == "ircc.canada.ca"
        || host.ends_with(".cic.gc.ca")
}
