//! Typed FlatBuffers payload codecs for the `nmp.wallet.cashu.*` family:
//! `create` ([`CashuCreateAction`]), `recover` ([`CashuRecoverAction`]),
//! `deposit_quote` ([`CashuDepositQuoteAction`]), and `complete_deposit`
//! ([`CashuCompleteDepositAction`]). See `super` (`wire.rs`) module docs.

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use super::cashu_complete_deposit_generated::nmp::wallet as complete_deposit_fb;
use super::cashu_create_generated::nmp::wallet as create_fb;
use super::cashu_deposit_quote_generated::nmp::wallet as deposit_quote_fb;
use super::cashu_recover_generated::nmp::wallet as recover_fb;
use super::{malformed, SCHEMA_VERSION};
use crate::action::{
    CashuCompleteDepositAction, CashuCreateAction, CashuDepositQuoteAction, CashuRecoverAction,
};

// --- CashuCreateAction (nmp.wallet.cashu.create) -----------------------------

impl ActionPayload for CashuCreateAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.cashu.create";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let mint = fbb.create_string(&self.mint);
        let payload = create_fb::CashuCreatePayload::create(
            &mut fbb,
            &create_fb::CashuCreatePayloadArgs {
                schema_version: SCHEMA_VERSION,
                mint: Some(mint),
            },
        );
        create_fb::finish_cashu_create_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !create_fb::cashu_create_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NWCC file identifier"));
        }
        let root = create_fb::root_as_cashu_create_payload(bytes)
            .map_err(|e| malformed(format!("not a valid CashuCreatePayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(CashuCreateAction {
            mint: root.mint().to_string(),
        })
    }
}

// --- CashuRecoverAction (nmp.wallet.cashu.recover) ---------------------------

impl ActionPayload for CashuRecoverAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.cashu.recover";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let payload = recover_fb::CashuRecoverPayload::create(
            &mut fbb,
            &recover_fb::CashuRecoverPayloadArgs {
                schema_version: SCHEMA_VERSION,
            },
        );
        recover_fb::finish_cashu_recover_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !recover_fb::cashu_recover_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing NWCR file identifier"));
        }
        let root = recover_fb::root_as_cashu_recover_payload(bytes)
            .map_err(|e| malformed(format!("not a valid CashuRecoverPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(CashuRecoverAction {})
    }
}

// --- CashuDepositQuoteAction (nmp.wallet.cashu.deposit_quote) ----------------

impl ActionPayload for CashuDepositQuoteAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.cashu.deposit_quote";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let mint = fbb.create_string(&self.mint);
        let payload = deposit_quote_fb::CashuDepositQuotePayload::create(
            &mut fbb,
            &deposit_quote_fb::CashuDepositQuotePayloadArgs {
                schema_version: SCHEMA_VERSION,
                mint: Some(mint),
                amount_sats: self.amount_sats,
            },
        );
        deposit_quote_fb::finish_cashu_deposit_quote_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !deposit_quote_fb::cashu_deposit_quote_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing NWDQ file identifier"));
        }
        let root = deposit_quote_fb::root_as_cashu_deposit_quote_payload(bytes)
            .map_err(|e| malformed(format!("not a valid CashuDepositQuotePayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(CashuDepositQuoteAction {
            mint: root.mint().to_string(),
            amount_sats: root.amount_sats(),
        })
    }
}

// --- CashuCompleteDepositAction (nmp.wallet.cashu.complete_deposit) ----------

impl ActionPayload for CashuCompleteDepositAction {
    const SCHEMA_ID: &'static str = "nmp.wallet.cashu.complete_deposit";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let quote_id = fbb.create_string(&self.quote_id);
        let payload = complete_deposit_fb::CashuCompleteDepositPayload::create(
            &mut fbb,
            &complete_deposit_fb::CashuCompleteDepositPayloadArgs {
                schema_version: SCHEMA_VERSION,
                quote_id: Some(quote_id),
            },
        );
        complete_deposit_fb::finish_cashu_complete_deposit_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8
            || !complete_deposit_fb::cashu_complete_deposit_payload_buffer_has_identifier(bytes)
        {
            return Err(malformed("missing NWCD file identifier"));
        }
        let root =
            complete_deposit_fb::root_as_cashu_complete_deposit_payload(bytes).map_err(|e| {
                malformed(format!(
                    "not a valid CashuCompleteDepositPayload buffer: {e}"
                ))
            })?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(CashuCompleteDepositAction {
            quote_id: root.quote_id().to_string(),
        })
    }
}
