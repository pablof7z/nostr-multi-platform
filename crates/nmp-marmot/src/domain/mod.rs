//! Marmot domain record shapes per `docs/plan/marmot-mls.md` §Step 1 and the
//! MDK→NMP mapping table in `docs/research/mdk-api.md` §6.
//!
//! Four record types live here: [`MarmotGroupRecord`], [`MarmotMessageRecord`],
//! [`MarmotKeyPackageRecord`], [`MarmotWelcomeRecord`]. Each is an
//! NMP-native projection — the actual MLS ratchet state lives in MDK/SQLite
//! (owned by [`crate::service`]), NOT in these records. This keeps `nmp-core`
//! free of MLS types (kernel-boundary exit gate).
//!
//! Marmot event kinds (mdk-api.md §4):
//! - 30443 / 443 — KeyPackage ([`MarmotKeyPackageRecord`]).
//! - 444 — Welcome rumor, wrapped in NIP-59 kind:1059 ([`MarmotWelcomeRecord`]).
//! - 445 — group message / commit / proposal ([`MarmotGroupRecord`] +
//!   [`MarmotMessageRecord`]).
//!
//! Marmot's domain records are materialised by the service after
//! `process_message` / `process_welcome` — inbound events require MLS
//! decryption before they become records, so there is no per-event
//! ingest dispatch into a Marmot decoder (unlike NIP-29's cleartext events).

pub(crate) mod records;

pub use records::{
    MarmotGroupRecord, MarmotKeyPackageRecord, MarmotMessageRecord, MarmotWelcomeRecord,
};
