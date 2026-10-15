//! Per-resource import handlers. Each handler:
//!  - parses one row from the CSV
//!  - resolves any name-based foreign keys against the connected source server
//!  - either creates the resource (live) or reports what would be created (dry run)
//!  - returns a RowOutcome that the caller logs and tallies in the summary.
//!
//! Handlers prefer REST and fall back to SOAP when the REST endpoint isn't
//! available (e.g. user creation is SOAP-only on N-central).
//!
//! Each resource has its own module; `context` holds the name lookups they share.

mod access_groups;
mod common;
mod context;
mod customers;
mod properties;
mod roles;
mod sites;
mod users;

#[cfg(test)]
mod tests;

pub use access_groups::import_access_group;
pub use context::ImportContext;
pub use customers::import_customer;
pub use properties::{import_device_property, import_org_property};
pub use roles::import_user_role;
pub use sites::import_site;
pub use users::import_user;
