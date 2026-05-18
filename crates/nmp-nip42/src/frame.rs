//! AUTH / OK frame shapes + parsers — re-exported from `nmp-nip42-types`.
//!
//! **T77:** these were defined here and duplicated verbatim in
//! `nmp_core::kernel::auth`. The shared, dependency-free definitions now
//! live in the `nmp-nip42-types` substrate crate; both consumers re-export
//! from it so the wire vocabulary cannot drift.

pub use nmp_nip42_types::{parse_auth_frame, parse_ok_frame, AuthChallenge, AuthOk};
