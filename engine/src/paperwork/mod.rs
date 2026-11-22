//! Paperwork: citizenship steps, presence count, official sources and the
//! guided citizenship application, sealed in a local vault. Ported from
//! sidequest (decision 0017). Threat model: docs/paperwork-threat-model.md.

pub mod citizenship;
pub mod cli;
pub mod facts;
pub mod presence;
pub mod store;
pub mod vault;
