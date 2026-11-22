//! Cashcast engine: reads bank and app exports and produces aggregates.
//!
//! Privacy boundary: `export`, `ofx`, `ledger` and `moneywiz` are the only
//! modules that read raw rows. Description text becomes hints and hashed
//! payee keys in `text`; only `moneywiz` reads the text itself (`Txn::text`). `ledger` keeps only asset symbols, account
//! types, dates, quantities and sums. Everything printed or written to JSON
//! holds account names, category names, dates, counts and sums only. Error
//! messages name line numbers, never field values.

pub mod api;
pub mod cards;
pub mod charges;
pub mod config;
pub mod core;
pub mod detect;
pub mod digest;
pub mod docs;
pub mod export;
pub mod forecast;
pub mod inbox;
pub mod inspect;
pub mod invest_plan;
pub mod knowledge;
pub mod ledger;
pub mod live_facts;
pub mod money;
pub mod moneywiz;
pub mod moneywiz_categories;
pub mod moneywiz_categorize;
pub mod moneywiz_coverage;
pub mod moneywiz_diagnose;
pub mod moneywiz_edit;
pub mod moneywiz_invest;
pub mod moneywiz_networth;
pub mod moneywiz_ledger;
pub mod moneywiz_view;
pub mod net_total;
pub mod ofx;
pub mod paperwork;
pub mod pdf;
pub mod proof;
pub mod strategy;
pub mod summarize;
pub mod tasks;
pub mod text;
pub mod transfers;
pub mod web;

/// Writes a file readable only by its owner. The file is created with mode
/// 600, so its content is never readable by others, not even for a moment.
pub fn write_private(path: &std::path::Path, text: &str) -> Result<(), String> {
    write_private_bytes(path, text.as_bytes())
}

/// `write_private` for raw bytes, such as an API download.
pub fn write_private_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let err = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // An older file may have been created with a wider mode.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(err)?;
    }
    file.write_all(bytes).map_err(err)
}
