//! Typed FlatBuffers wire codecs for the ten `nmp.wallet.*` action payloads
//! this crate owns (#2920, epic #2864; `set_mints` added by #2997,
//! `cross_mint_transfer` by #3003): `select_backend`, the Cashu
//! create/recover/deposit_quote/complete_deposit/set_mints/cross_mint_transfer
//! family, and the nutzap publish_info/send/redeem family.
//!
//! These are the WRITE-direction typed payloads carried as the OPAQUE
//! `DispatchEnvelope.payload`. The registry adapter decodes them through
//! [`nmp_core::substrate::ActionPayload::decode`] — the single typed-decode
//! site — running the fail-closed `schema_version` gate BEFORE `start()`. Each
//! `ActionModule` (see `action/`) overrides `decode_payload` to delegate here,
//! which is what lets `dispatch_action_bytes_typed` reach these namespaces by
//! name (the gap #2920 reports).
//!
//! Honours D6: decode returns a data-shaped `ActionPayloadDecodeError` on any
//! malformed input; no panics on the decode path.
//!
//! Split into per-family impl files (`select_backend.rs` / `cashu.rs` /
//! `nutzap.rs`) so this module stays a thin generated-bindings registry, under
//! the file-size gate. The generated FlatBuffers bindings live here, at the
//! `wire` level, so all three family files can `use` them.

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/select_backend_generated.rs"]
pub mod select_backend_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/cashu_create_generated.rs"]
pub mod cashu_create_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/cashu_recover_generated.rs"]
pub mod cashu_recover_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/cashu_set_mints_generated.rs"]
pub mod cashu_set_mints_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/cashu_deposit_quote_generated.rs"]
pub mod cashu_deposit_quote_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/cashu_cross_mint_transfer_generated.rs"]
pub mod cashu_cross_mint_transfer_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/cashu_complete_deposit_generated.rs"]
pub mod cashu_complete_deposit_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/nutzap_publish_info_generated.rs"]
pub mod nutzap_publish_info_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/nutzap_send_generated.rs"]
pub mod nutzap_send_generated;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "wire/generated/nutzap_redeem_generated.rs"]
pub mod nutzap_redeem_generated;

/// Wire schema version for all ten wallet action payloads this crate owns.
/// Bump the relevant `.fbs` schema's own version on any breaking wire change —
/// kept as one constant today because every payload is at schema v1.
pub(crate) const SCHEMA_VERSION: u32 = 1;

pub(crate) fn malformed(
    reason: impl Into<String>,
) -> nmp_core::substrate::ActionPayloadDecodeError {
    nmp_core::substrate::ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

mod cashu;
mod nutzap;
mod select_backend;

#[cfg(test)]
#[path = "wire/action_payload_tests.rs"]
mod action_payload_tests;
