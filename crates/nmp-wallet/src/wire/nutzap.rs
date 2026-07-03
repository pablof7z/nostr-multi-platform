//! Typed FlatBuffers payload codecs for the `nmp.wallet.nutzap.*` family:
//! `publish_info` ([`NutzapPublishInfoAction`]), `send` ([`NutzapSendAction`]),
//! and `redeem` ([`NutzapRedeemAction`]). See `super` (`wire.rs`) module docs.

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use super::nutzap_publish_info_generated::nmp::wallet as publish_info_fb;
use super::nutzap_redeem_generated::nmp::wallet as redeem_fb;
use super::nutzap_send_generated::nmp::wallet as send_fb;
use super::{malformed, SCHEMA_VERSION};
use crate::action::{NutzapPublishInfoAction, NutzapRedeemAction, NutzapSendAction};

// --- NutzapPublishInfoAction (nmp.wallet.nutzap.publish_info) ----------------

impl ActionPayload for NutzapPublishInfoAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.nutzap.publish_info";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let payload = publish_info_fb::NutzapPublishInfoPayload::create(
            &mut fbb,
            &publish_info_fb::NutzapPublishInfoPayloadArgs {
                schema_version: SCHEMA_VERSION,
            },
        );
        publish_info_fb::finish_nutzap_publish_info_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !publish_info_fb::nutzap_publish_info_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing NWPI file identifier"));
        }
        let root = publish_info_fb::root_as_nutzap_publish_info_payload(bytes)
            .map_err(|e| malformed(format!("not a valid NutzapPublishInfoPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(NutzapPublishInfoAction {})
    }
}

// --- NutzapSendAction (nmp.wallet.nutzap.send) -------------------------------

impl ActionPayload for NutzapSendAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.nutzap.send";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let recipient_pubkey = fbb.create_string(&self.recipient_pubkey);
        let target_event_id = self.target_event_id.as_ref().map(|s| fbb.create_string(s));
        let payload = send_fb::NutzapSendPayload::create(
            &mut fbb,
            &send_fb::NutzapSendPayloadArgs {
                schema_version: SCHEMA_VERSION,
                recipient_pubkey: Some(recipient_pubkey),
                amount_sats: self.amount_sats,
                target_event_id,
            },
        );
        send_fb::finish_nutzap_send_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !send_fb::nutzap_send_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NWNS file identifier"));
        }
        let root = send_fb::root_as_nutzap_send_payload(bytes)
            .map_err(|e| malformed(format!("not a valid NutzapSendPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(NutzapSendAction {
            recipient_pubkey: root.recipient_pubkey().to_string(),
            amount_sats: root.amount_sats(),
            target_event_id: root.target_event_id().map(str::to_string),
        })
    }
}

// --- NutzapRedeemAction (nmp.wallet.nutzap.redeem) ---------------------------

impl ActionPayload for NutzapRedeemAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.nutzap.redeem";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let event_id = fbb.create_string(&self.event_id);
        let payload = redeem_fb::NutzapRedeemPayload::create(
            &mut fbb,
            &redeem_fb::NutzapRedeemPayloadArgs {
                schema_version: SCHEMA_VERSION,
                event_id: Some(event_id),
            },
        );
        redeem_fb::finish_nutzap_redeem_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !redeem_fb::nutzap_redeem_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NWNR file identifier"));
        }
        let root = redeem_fb::root_as_nutzap_redeem_payload(bytes)
            .map_err(|e| malformed(format!("not a valid NutzapRedeemPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(NutzapRedeemAction {
            event_id: root.event_id().to_string(),
        })
    }
}
