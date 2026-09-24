//! Catalog bundle model and the runtime resolver.
//!
//! Two independently testable pieces live in this crate for the resolver side:
//!
//! - [`model`] — the `catalog.json` bundle model, parsing and validation.
//! - [`resolve`] — the pure decision module the host calls. Every environment
//!   observation goes through [`Probe`], so tests never touch a filesystem.
//!
//! The builder (`catalog-build`) depends on this crate so builder output and
//! resolver input cannot drift.

pub mod model;
mod placeholders;
pub mod resolve;

pub use model::{
    Bundle, BundleError, Detect, Game, IdList, PLACEHOLDERS, Platform, SCHEMA, SaveCandidate,
    Source, Store, When,
};
pub use resolve::{
    Decision, Environment, GameRecord, Install, Probe, Reason, assign_games, resolve,
};
