//! The catalog: the bundle model and the resolver that turns a catalog entry
//! and an install into a save set (PLAN-CATALOG.md Sections 3.4 and 4).
//!
//! No OS APIs, database or host types live here; the machine is observed
//! only through [`Probe`].

pub mod broad;
pub mod glob;
pub mod model;
pub mod resolve;

pub use model::{Bundle, BundleError, Detect, Executables, Game, PathRule, Platform, SCHEMA, Source, Store, When};
pub use resolve::{
    Context, Decision, Filter, GameRecord, Install, KnownFolders, Outcome, Presence, Probe, SteamAccount, Target,
    assign_games, decide_builds, resolve, split_location, split_target,
};

#[cfg(test)]
mod tests;
