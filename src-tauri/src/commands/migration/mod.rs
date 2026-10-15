//! Migration-related Tauri commands.
//!
//! `run` holds the orchestration and the command; each step lives in its own module
//! (`customers`, `roles`, `users`, `access_groups`, `properties`), sharing the types,
//! environment, summary, and mapping helpers.

mod access_groups;
mod customers;
mod env;
mod mapping;
mod properties;
mod roles;
mod run;
mod summary;
mod types;
mod users;

#[cfg(test)]
mod tests;
pub use run::*;
pub use types::*;
