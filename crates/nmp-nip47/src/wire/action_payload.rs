//! Typed FlatBuffers payload codecs for the three wallet action payloads this
//! crate owns (ADR-0071 / #1756): `nmp.wallet.connect`
//! ([`WalletConnectAction`]), `nmp.wallet.disconnect`
//! ([`WalletDisconnectAction`]), and `nmp.wallet.pay_invoice`
//! ([`WalletAction`]).
//!
//! These are the WRITE-direction typed payloads carried as the OPAQUE
//! `DispatchEnvelope.payload`. The registry adapter decodes them through
//! [`ActionPayload::decode`] here — the single typed-decode site — running the
//! fail-closed `schema_version` gate BEFORE `start()`. Distinct from
//! `typed_fb.rs`, which is the READ-direction `wallet_status` snapshot sidecar.
//!
//! These close the Cut-B producer-typing gap: the three wallet modules were
//! registered `ActionModule`s with NO typed payload, reachable only through the
//! JSON doorway. Each module now overrides `decode_payload` to delegate to
//! [`ActionPayload::decode`] on its `Action` type (see `action/`), so the typed
//! byte doorway can route them.
//!
//! Honours D6: decode returns a data-shaped [`ActionPayloadDecodeError`] on any
//! malformed input; no panics on the decode path.

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
#[path = "generated/wallet_connect_generated.rs"]
pub mod wallet_connect_generated;

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
#[path = "generated/wallet_disconnect_generated.rs"]
pub mod wallet_disconnect_generated;

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
#[path = "generated/wallet_pay_invoice_generated.rs"]
pub mod wallet_pay_invoice_generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use wallet_connect_generated::nmp::nip_47 as connect_fb;
use wallet_disconnect_generated::nmp::nip_47 as disconnect_fb;
use wallet_pay_invoice_generated::nmp::nip_47 as pay_invoice_fb;

use crate::action::{WalletAction, WalletConnectAction, WalletDisconnectAction};

/// Wire schema version for all three wallet action payloads. Bump on any
/// breaking change to `wallet_connect.fbs` / `wallet_disconnect.fbs` /
/// `wallet_pay_invoice.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

// --- WalletConnectAction (nmp.wallet.connect) --------------------------------

impl ActionPayload for WalletConnectAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.connect";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let WalletConnectAction::Connect { uri } = self;
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let uri = fbb.create_string(uri);
        let payload = connect_fb::WalletConnectPayload::create(
            &mut fbb,
            &connect_fb::WalletConnectPayloadArgs {
                schema_version: SCHEMA_VERSION,
                uri: Some(uri),
            },
        );
        connect_fb::finish_wallet_connect_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !connect_fb::wallet_connect_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N47C file identifier"));
        }
        let root = connect_fb::root_as_wallet_connect_payload(bytes)
            .map_err(|e| malformed(format!("not a valid WalletConnectPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(WalletConnectAction::Connect {
            uri: root.uri().to_string(),
        })
    }
}

// --- WalletDisconnectAction (nmp.wallet.disconnect) --------------------------

impl ActionPayload for WalletDisconnectAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.disconnect";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let WalletDisconnectAction::Disconnect = self;
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let payload = disconnect_fb::WalletDisconnectPayload::create(
            &mut fbb,
            &disconnect_fb::WalletDisconnectPayloadArgs {
                schema_version: SCHEMA_VERSION,
            },
        );
        disconnect_fb::finish_wallet_disconnect_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !disconnect_fb::wallet_disconnect_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing N47D file identifier"));
        }
        let root = disconnect_fb::root_as_wallet_disconnect_payload(bytes)
            .map_err(|e| malformed(format!("not a valid WalletDisconnectPayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(WalletDisconnectAction::Disconnect)
    }
}

// --- WalletAction::PayInvoice (nmp.wallet.pay_invoice) ------------------------

impl ActionPayload for WalletAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.pay_invoice";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let WalletAction::PayInvoice {
            bolt11,
            amount_msats,
        } = self;
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let bolt11 = fbb.create_string(bolt11);
        let payload = pay_invoice_fb::WalletPayInvoicePayload::create(
            &mut fbb,
            &pay_invoice_fb::WalletPayInvoicePayloadArgs {
                schema_version: SCHEMA_VERSION,
                bolt11: Some(bolt11),
                // `Option<u64>` presence is carried by `has_amount_msats`; the
                // scalar defaults to 0 when absent (ignored on decode).
                amount_msats: amount_msats.unwrap_or(0),
                has_amount_msats: amount_msats.is_some(),
            },
        );
        pay_invoice_fb::finish_wallet_pay_invoice_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !pay_invoice_fb::wallet_pay_invoice_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing N47P file identifier"));
        }
        let root = pay_invoice_fb::root_as_wallet_pay_invoice_payload(bytes)
            .map_err(|e| malformed(format!("not a valid WalletPayInvoicePayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        // Reconstruct `Option<u64>` from the companion presence flag so that
        // `Some(0)` never collapses to `None` (and `None` never reads as
        // `Some(0)`).
        let amount_msats = if root.has_amount_msats() {
            Some(root.amount_msats())
        } else {
            None
        };
        Ok(WalletAction::PayInvoice {
            bolt11: root.bolt11().to_string(),
            amount_msats,
        })
    }
}

#[cfg(test)]
#[path = "action_payload_tests.rs"]
mod tests;
