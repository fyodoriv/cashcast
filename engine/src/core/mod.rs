//! The core every command needs: paths, users, settings, consent, events,
//! the output gate and telemetry (Rust port step 1, decision 0010).

pub mod argparse;
pub mod cli;
pub mod commands;
pub mod consent;
pub mod events;
pub mod gate;
pub mod health;
pub mod http;
pub mod paths;
pub mod pyjson;
pub mod settings;
pub mod telemetry;
pub mod users;
