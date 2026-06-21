//! Typed FlatBuffers wire codec for [`crate::status::WalletStatus`].
//!
//! The authoritative FFI shape of the `"wallet"` projection is the serde JSON
//! of [`WalletStatus`] (registered via `register_snapshot_projection` in
//! `apps/chirp/nmp-app-chirp/src/wallet_runtime.rs`). This module adds a
//! **typed FlatBuffers** encoding of the same struct — a self-describing,
//! schema-versioned, language-neutral binary the host platforms (Swift /
//! Kotlin / TypeScript) can decode with generated accessors instead of JSON
//! reflection. It is a sidecar codec: the serde shape stays authoritative; this
//! is the typed payload carried in the `typed_projections` sidecar
//! (ADR-0037, `crates/nmp-core/schema/nmp_update.fbs`).
//!
//! The schema (`crates/nmp-nip47/schema/wallet_status.fbs`) mirrors the Rust
//! struct field-for-field. `Option<...>` fields carry a `has_*` presence flag
//! plus the value so absent (`None`) round-trips distinctly from a present
//! default — the same optional-fields convention used by `content_tree.fbs`.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! there are no `unwrap`/`expect`/panicking-index operations on the decode path.

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
#[path = "generated/wallet_status_generated.rs"]
pub mod generated;

use generated::nmp::nip_47 as fb;

use crate::status::{NwcConnectionState, WalletStatus};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.nip47.wallet";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NWST";
/// Wire schema version. Bump on any breaking change to `wallet_status.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- enum bridges ---------------------------------------------------------

fn connection_state_to_fb(state: NwcConnectionState) -> fb::NwcConnectionState {
    match state {
        NwcConnectionState::Connected => fb::NwcConnectionState::Connected,
        NwcConnectionState::Reconnecting => fb::NwcConnectionState::Reconnecting,
        NwcConnectionState::TransportLost => fb::NwcConnectionState::TransportLost,
    }
}

fn connection_state_from_fb(state: fb::NwcConnectionState) -> Result<NwcConnectionState, String> {
    match state {
        fb::NwcConnectionState::Connected => Ok(NwcConnectionState::Connected),
        fb::NwcConnectionState::Reconnecting => Ok(NwcConnectionState::Reconnecting),
        fb::NwcConnectionState::TransportLost => Ok(NwcConnectionState::TransportLost),
        other => Err(format!("unknown NwcConnectionState discriminant {}", other.0)),
    }
}

// --- encode ---------------------------------------------------------------

/// Encode a [`WalletStatus`] to typed FlatBuffers bytes (with the `NWST` file
/// identifier).
#[must_use]
pub fn encode_wallet_status(status: &WalletStatus) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    // All string offsets must be created before the table is started.
    let status_str = fbb.create_string(&status.status);
    let relay_url = fbb.create_string(&status.relay_url);
    let wallet_npub = fbb.create_string(&status.wallet_npub);
    let wallet_pubkey_hex = fbb.create_string(&status.wallet_pubkey_hex);

    let root = fb::WalletStatus::create(
        &mut fbb,
        &fb::WalletStatusArgs {
            status: Some(status_str),
            relay_url: Some(relay_url),
            wallet_npub: Some(wallet_npub),
            has_balance_msats: status.balance_msats.is_some(),
            balance_msats: status.balance_msats.unwrap_or_default(),
            has_balance_sats: status.balance_sats.is_some(),
            balance_sats: status.balance_sats.unwrap_or_default(),
            // `wallet_npub_short` vtable slot is deprecated (#1678, D7);
            // not written — shells abbreviate `wallet_npub` themselves.
            is_ready: status.is_ready,
            is_connected: status.is_connected,
            has_connection_state: status.connection_state.is_some(),
            connection_state: status
                .connection_state
                .as_ref()
                .map(|s| connection_state_to_fb(s.clone()))
                .unwrap_or(fb::NwcConnectionState::Connected),
            wallet_pubkey_hex: Some(wallet_pubkey_hex),
        },
    );
    fb::finish_wallet_status_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_wallet_status`])
/// back into a [`WalletStatus`]. Returns an error string on any malformed
/// input.
pub fn decode_wallet_status(bytes: &[u8]) -> Result<WalletStatus, String> {
    if bytes.len() < 8 || !fb::wallet_status_buffer_has_identifier(bytes) {
        return Err("missing NWST file identifier".to_string());
    }
    let root = fb::root_as_wallet_status(bytes)
        .map_err(|e| format!("not a valid WalletStatus buffer: {e}"))?;

    Ok(WalletStatus {
        status: str_field(root.status(), "WalletStatus.status")?,
        relay_url: str_field(root.relay_url(), "WalletStatus.relay_url")?,
        wallet_npub: str_field(root.wallet_npub(), "WalletStatus.wallet_npub")?,
        balance_msats: optional_u64(root.has_balance_msats(), root.balance_msats()),
        balance_sats: optional_u64(root.has_balance_sats(), root.balance_sats()),
        // `wallet_npub_short` removed (#1678, D7); deprecated vtable slot is
        // not decoded — shells abbreviate `wallet_npub` themselves.
        wallet_pubkey_hex: str_field(
            root.wallet_pubkey_hex(),
            "WalletStatus.wallet_pubkey_hex",
        )?,
        is_ready: root.is_ready(),
        is_connected: root.is_connected(),
        connection_state: if root.has_connection_state() {
            Some(connection_state_from_fb(root.connection_state())?)
        } else {
            None
        },
    })
}

/// Require a present, non-absent string field; an absent FlatBuffers string on
/// a mandatory slot is a decode error.
fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}

/// Reconstruct an `Option<u64>` from a `has_*` flag + the wire value.
fn optional_u64(present: bool, value: u64) -> Option<u64> {
    present.then_some(value)
}

#[cfg(test)]
#[path = "typed_fb_tests.rs"]
mod tests;
