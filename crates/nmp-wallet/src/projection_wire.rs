//! Typed FlatBuffers wire codec for the MERGED multi-backend
//! [`crate::projection::WalletProjection`] (#2915, epic #2864).
//!
//! This is the typed sidecar counterpart to the serde JSON of `WalletProjection`.
//! `nmp_wallet::register` registers it under the DISTINCT projection key
//! `"wallet.merged"` — deliberately NOT `"wallet"`, which `nmp-nip47` still owns
//! for its single-backend NWC `WalletStatus` shape
//! (`crates/nmp-nip47/src/wire/typed_fb.rs`, `NWST`). The two coexist as separate
//! typed sidecars: an NWC-only host keeps decoding the `NWST` `"wallet"` payload;
//! a host that wants the merged backend-selection + capability-union +
//! concatenated bounded rows decodes this `NWMP` `"wallet.merged"` payload. Both
//! are emitted ALONGSIDE the generic `Value` projection, never replacing it
//! (ADR-0072).
//!
//! The schema (`crates/nmp-wallet/schema/wallet_projection.fbs`) mirrors the Rust
//! structs field-for-field. `Option<...>` fields carry a `has_*` presence flag
//! plus the value so absent (`None`) round-trips distinctly from a present
//! default — the same optional-fields convention `wallet_status.fbs` uses. The
//! nested `balances`/`pending_operations`/`recent_history`/`receive_rows` vectors
//! follow the `NotificationsSnapshot`/`ModularTimelineSnapshot` vector-of-tables
//! precedent.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! there are no `unwrap`/`expect`/panicking-index operations on the decode path.
//!
//! The enum bridges and row/table encode+decode helpers live in the `enums` and
//! `rows` child modules (split out to keep each file under the 500-LOC hard cap).

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This `allow` block scopes the relaxation to the
// single generated module — no hand-written code in this file uses `unsafe`.
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
#[path = "wire/generated/wallet_projection_generated.rs"]
pub mod generated;

mod enums;
mod rows;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::wallet as fb;

use enums::{readiness_from_fb, readiness_to_fb};
use rows::{
    decode_balances, decode_capabilities, decode_history, decode_operations,
    decode_receive_rows, encode_balances, encode_capabilities, encode_history, encode_operations,
    encode_receive_rows, str_field,
};

use crate::backend::WalletBackendId;
use crate::projection::WalletProjection;

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.wallet.merged";
/// The projection key this typed sidecar registers under. Distinct from
/// `nmp-nip47`'s `"wallet"` key (see module docs).
pub const PROJECTION_KEY: &str = "wallet.merged";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NWMP";
/// Wire schema version. Bump on any breaking change to `wallet_projection.fbs`.
/// v2 (#2966 follow-up): `WalletOperation` gains `recorded_amount`/
/// `recorded_sender`/`recorded_at`; `WalletHistoryRow`/`WalletReceiveRow` gain
/// `sender`/`timestamp` — mirroring the fields #2966 added to the domain
/// structs this codec serializes.
/// v3 (#2880 follow-up, epic #2864): `WalletProjection` gains
/// `discovered_mints` — the NIP-87 web-of-trust-scoped, capability-fail-closed
/// discovered-mints view.
/// v4 (#2880 unwind): NIP-87 mint discovery moved to the standalone
/// `nmp-mint-discovery` crate (its own `"mint_discovery"` typed projection).
/// `discovered_mints` is REMOVED from `WalletProjection` — per the schema's
/// field-removal convention the wire slot is deprecated in place, not reused
/// or reordered (see `schema/wallet_projection.fbs`).
pub const SCHEMA_VERSION: u32 = 4;

// --- encode ---------------------------------------------------------------

/// Encode a [`WalletProjection`] to typed FlatBuffers bytes (with the `NWMP`
/// file identifier).
#[must_use]
pub fn encode_wallet_projection(projection: &WalletProjection) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    // All child offsets (strings, nested tables, vectors) must be created before
    // the root table is started.
    let active_backend_id = projection
        .active_backend_id
        .as_ref()
        .map(|id| fbb.create_string(id.as_str()));
    let cashu_p2pk_pubkey = projection
        .cashu_p2pk_pubkey
        .as_ref()
        .map(|value| fbb.create_string(value));

    let capabilities = encode_capabilities(&mut fbb, &projection.capabilities);
    let balances = encode_balances(&mut fbb, &projection.balances);
    let pending_operations = encode_operations(&mut fbb, &projection.pending_operations);
    let recent_history = encode_history(&mut fbb, &projection.recent_history);
    let receive_rows = encode_receive_rows(&mut fbb, &projection.receive_rows);

    let root = fb::WalletProjection::create(
        &mut fbb,
        &fb::WalletProjectionArgs {
            schema_version: SCHEMA_VERSION,
            has_active_backend_id: projection.active_backend_id.is_some(),
            active_backend_id,
            readiness: readiness_to_fb(projection.readiness),
            capabilities: Some(capabilities),
            balances: Some(balances),
            has_cashu_p2pk_pubkey: projection.cashu_p2pk_pubkey.is_some(),
            cashu_p2pk_pubkey,
            accepted_mint_count: projection.accepted_mint_count,
            accepted_relay_count: projection.accepted_relay_count,
            pending_operations: Some(pending_operations),
            recent_history: Some(recent_history),
            receive_rows: Some(receive_rows),
        },
    );
    fb::finish_wallet_projection_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_wallet_projection`])
/// back into a [`WalletProjection`]. Returns an error string on any malformed
/// input.
pub fn decode_wallet_projection(bytes: &[u8]) -> Result<WalletProjection, String> {
    if bytes.len() < 8 || !fb::wallet_projection_buffer_has_identifier(bytes) {
        return Err("missing NWMP file identifier".to_string());
    }
    let root = fb::root_as_wallet_projection(bytes)
        .map_err(|e| format!("not a valid WalletProjection buffer: {e}"))?;

    let active_backend_id = if root.has_active_backend_id() {
        Some(WalletBackendId::new(str_field(
            root.active_backend_id(),
            "WalletProjection.active_backend_id",
        )?))
    } else {
        None
    };

    let cashu_p2pk_pubkey = if root.has_cashu_p2pk_pubkey() {
        Some(str_field(
            root.cashu_p2pk_pubkey(),
            "WalletProjection.cashu_p2pk_pubkey",
        )?)
    } else {
        None
    };

    let capabilities = decode_capabilities(root.capabilities());
    let balances = decode_balances(root.balances())?;
    let pending_operations = decode_operations(root.pending_operations())?;
    let recent_history = decode_history(root.recent_history())?;
    let receive_rows = decode_receive_rows(root.receive_rows())?;

    Ok(WalletProjection {
        active_backend_id,
        readiness: readiness_from_fb(root.readiness())?,
        capabilities,
        balances,
        cashu_p2pk_pubkey,
        accepted_mint_count: root.accepted_mint_count(),
        accepted_relay_count: root.accepted_relay_count(),
        pending_operations,
        recent_history,
        receive_rows,
    })
}

#[cfg(test)]
#[path = "projection_wire_tests.rs"]
mod tests;
