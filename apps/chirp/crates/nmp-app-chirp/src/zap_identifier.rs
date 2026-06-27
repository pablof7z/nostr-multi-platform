//! Chirp-owned composition action for zapping a raw NIP-05/lightning address.
//!
//! `nmp-nip57` owns the reusable zap protocol path for known pubkeys. This
//! module owns the app-specific composition that starts from a user-entered
//! identifier, resolves it through `nmp-nip05`, then chains into NIP-57's
//! existing LNURL/pay command with the resolved pubkey.

use std::sync::Arc;

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
#[path = "wire/generated/zap_identifier_generated.rs"]
mod zap_identifier_generated;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    build_record_action_failure, ActionContext, ActionModule, ActionPayload,
    ActionPayloadDecodeError, ActionRegistrar, ActionRejection, PaymentPort, ProtocolCommand,
    ProtocolCommandContext, ProtocolCommandError,
};
use nmp_nip05::parse_nip05;
use nmp_nip57::{FetchLnurlInvoiceCommand, ZapRequest};
use serde::{Deserialize, Serialize};
use zap_identifier_generated::nmp::app::chirp as fb;

/// Chirp app action namespace for raw identifier zaps.
pub const ZAP_IDENTIFIER_NAMESPACE: &str = "nmp.app.chirp.zap_identifier";

/// Wire shape for `nmp.app.chirp.zap_identifier`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ZapIdentifierInput {
    /// User-entered NIP-05/lightning address, e.g. `alice@example.com`.
    pub recipient_identifier: String,
    /// Amount in millisatoshis. Must be > 0.
    pub amount_msats: u64,
    /// Optional zapped event id (hex).
    #[serde(default)]
    pub target_event_id: Option<String>,
    /// Optional free-form zap comment.
    #[serde(default)]
    pub comment: Option<String>,
}

/// Wire schema version for `zap_identifier.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

impl ActionPayload for ZapIdentifierInput {
    const SCHEMA_ID: &'static str = ZAP_IDENTIFIER_NAMESPACE;
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let recipient_identifier = fbb.create_string(&self.recipient_identifier);
        let target_event_id = self
            .target_event_id
            .as_deref()
            .map(|s| fbb.create_string(s));
        let comment = self.comment.as_deref().map(|s| fbb.create_string(s));

        let payload = fb::ZapIdentifierPayload::create(
            &mut fbb,
            &fb::ZapIdentifierPayloadArgs {
                schema_version: SCHEMA_VERSION,
                recipient_identifier: Some(recipient_identifier),
                amount_msats: self.amount_msats,
                target_event_id,
                comment,
            },
        );
        fb::finish_zap_identifier_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::zap_identifier_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing CZID file identifier"));
        }
        let root = fb::root_as_zap_identifier_payload(bytes)
            .map_err(|e| malformed(format!("not a valid ZapIdentifierPayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(Self {
            recipient_identifier: root.recipient_identifier().to_string(),
            amount_msats: root.amount_msats(),
            target_event_id: root.target_event_id().map(str::to_string),
            comment: root.comment().map(str::to_string),
        })
    }
}

/// Chirp action module for `nmp.app.chirp.zap_identifier`.
#[derive(Default)]
pub struct ZapIdentifierAction {
    payment_port: Option<Arc<dyn PaymentPort>>,
}

impl ZapIdentifierAction {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_payment_port(payment_port: Arc<dyn PaymentPort>) -> Self {
        Self {
            payment_port: Some(payment_port),
        }
    }
}

impl ActionModule for ZapIdentifierAction {
    const NAMESPACE: &'static str = ZAP_IDENTIFIER_NAMESPACE;
    type Action = ZapIdentifierInput;

    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<ZapIdentifierInput as ActionPayload>::decode(bytes))
    }

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if parse_nip05(&action.recipient_identifier).is_none() {
            return Err(ActionRejection::Invalid(
                "zap identifier must be a valid NIP-05/lightning address".into(),
            ));
        }
        if action.amount_msats == 0 {
            return Err(ActionRejection::Invalid(
                "zap amount must be greater than 0 msats".into(),
            ));
        }
        if action
            .target_event_id
            .as_deref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(ActionRejection::Invalid(
                "zap target event id must not be empty when provided".into(),
            ));
        }
        Ok(())
    }

    fn is_async_completing() -> bool {
        true
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let Some((name, domain)) = parse_nip05(&action.recipient_identifier) else {
            return Err("zap identifier must be a valid NIP-05/lightning address".into());
        };
        send(ActorCommand::Protocol(Box::new(
            ResolveZapIdentifierCommand {
                recipient_identifier: action.recipient_identifier,
                name,
                domain,
                amount_msats: action.amount_msats,
                target_event_id: action.target_event_id,
                comment: action.comment,
                correlation_id: Some(correlation_id.to_string()),
                payment_port: self.payment_port.clone(),
            },
        )));
        Ok(())
    }
}

/// Resolve the raw identifier to a pubkey, then continue through NIP-57.
#[derive(Debug)]
pub struct ResolveZapIdentifierCommand {
    pub recipient_identifier: String,
    pub name: String,
    pub domain: String,
    pub amount_msats: u64,
    pub target_event_id: Option<String>,
    pub comment: Option<String>,
    pub correlation_id: Option<String>,
    pub payment_port: Option<Arc<dyn PaymentPort>>,
}

impl ProtocolCommand for ResolveZapIdentifierCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            recipient_identifier,
            name,
            domain,
            amount_msats,
            target_event_id,
            comment,
            correlation_id,
            payment_port,
        } = *self;

        let Some((name, domain)) = parse_nip05(&format!("{name}@{domain}")) else {
            let reason =
                format!("zap identifier `{recipient_identifier}` is not a valid NIP-05 shape");
            ctx.send(ActorCommand::ShowToast {
                message: reason.clone(),
            });
            if let Some(cid) = correlation_id {
                ctx.record_action_failure(cid, reason);
            }
            return Ok(());
        };

        if let Some(ref cid) = correlation_id {
            ctx.record_action_stage_requested(cid);
        }

        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            match nmp_nip05::resolve_nip05_pubkey_blocking(&name, &domain) {
                Ok(pubkey) => {
                    match build_nip57_fetch_command(
                        pubkey,
                        recipient_identifier,
                        amount_msats,
                        target_event_id,
                        comment,
                        correlation_id.clone(),
                        payment_port,
                    ) {
                        Ok(cmd) => {
                            let _ = worker_tx.send(ActorCommand::Protocol(Box::new(cmd)));
                        }
                        Err(reason) => {
                            report_worker_failure(worker_tx, correlation_id, reason);
                        }
                    }
                }
                Err(reason) => {
                    let message = format!("NIP-05 lookup failed for {name}@{domain}: {reason}");
                    report_worker_failure(worker_tx, correlation_id, message);
                }
            }
        });
        Ok(())
    }
}

fn build_nip57_fetch_command(
    pubkey: String,
    recipient_identifier: String,
    amount_msats: u64,
    target_event_id: Option<String>,
    comment: Option<String>,
    correlation_id: Option<String>,
    payment_port: Option<Arc<dyn PaymentPort>>,
) -> Result<FetchLnurlInvoiceCommand, String> {
    let mut builder = ZapRequest::to_pubkey(&pubkey)
        .amount_msats(amount_msats)
        .relays(Vec::new());
    if let Some(ref id) = target_event_id {
        builder = builder.zapped_event(id);
    }
    if let Some(ref comment) = comment {
        builder = builder.comment(comment);
    }
    let unsigned = builder
        .build()
        .map_err(|e| format!("build kind:9734 zap request: {e}"))?;
    Ok(FetchLnurlInvoiceCommand {
        unsigned,
        recipient_pubkey: pubkey,
        lnurl_or_address: Some(recipient_identifier),
        amount_msats,
        correlation_id,
        payment_port,
    })
}

fn report_worker_failure(
    worker_tx: nmp_core::CommandSender,
    correlation_id: Option<String>,
    reason: String,
) {
    let _ = worker_tx.send(ActorCommand::ShowToast {
        message: format!("Zap failed: {reason}"),
    });
    if let Some(cid) = correlation_id {
        let _ = worker_tx.send(build_record_action_failure(cid, reason));
    }
}

pub(crate) fn register_zap_identifier_default(app: &mut impl ActionRegistrar) {
    app.register_default_action(ZapIdentifierAction::new());
}

pub(crate) fn register_zap_identifier_with_payment_port(
    app: &mut impl ActionRegistrar,
    payment_port: Arc<dyn PaymentPort>,
) {
    app.register_action(ZapIdentifierAction::with_payment_port(payment_port))
        .expect("duplicate registration: nmp-app-chirp ZapIdentifierAction"); // doctrine-allow: D6 - startup-only duplicate wiring is a programmer error
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::actor::ActorCommand;

    #[test]
    fn payload_round_trips_raw_identifier() {
        let action = ZapIdentifierInput {
            recipient_identifier: "alice@example.com".to_string(),
            amount_msats: 21_000,
            target_event_id: None,
            comment: Some("hi".to_string()),
        };
        let decoded = ZapIdentifierInput::decode(&action.encode()).expect("payload must decode");
        assert_eq!(decoded, action);
    }

    #[test]
    fn start_accepts_valid_identifier() {
        let action = ZapIdentifierAction::new();
        let mut ctx = ActionContext::default();
        assert!(action
            .start(
                &mut ctx,
                ZapIdentifierInput {
                    recipient_identifier: "alice@example.com".to_string(),
                    amount_msats: 21_000,
                    target_event_id: None,
                    comment: Some("hi".to_string()),
                },
            )
            .is_ok());
    }

    #[test]
    fn start_rejects_malformed_identifier_and_zero_amount() {
        let action = ZapIdentifierAction::new();
        let mut ctx = ActionContext::default();
        let malformed = action
            .start(
                &mut ctx,
                ZapIdentifierInput {
                    recipient_identifier: "not-an-address".to_string(),
                    amount_msats: 21_000,
                    target_event_id: None,
                    comment: None,
                },
            )
            .unwrap_err();
        assert!(matches!(malformed, ActionRejection::Invalid(_)));

        let zero = action
            .start(
                &mut ctx,
                ZapIdentifierInput {
                    recipient_identifier: "alice@example.com".to_string(),
                    amount_msats: 0,
                    target_event_id: None,
                    comment: None,
                },
            )
            .unwrap_err();
        assert!(matches!(zero, ActionRejection::Invalid(_)));
    }

    #[test]
    fn execute_emits_resolve_command_with_raw_identifier_amount_and_comment() {
        let action = ZapIdentifierAction::new();
        let sent = std::sync::Mutex::new(Vec::new());
        action
            .execute(
                &ActionContext::default(),
                ZapIdentifierInput {
                    recipient_identifier: "alice@example.com".to_string(),
                    amount_msats: 21_000,
                    target_event_id: None,
                    comment: Some("hi".to_string()),
                },
                "cid-1",
                &|cmd| sent.lock().unwrap().push(cmd),
            )
            .expect("execute accepts");

        let commands = sent.into_inner().unwrap();
        assert_eq!(commands.len(), 1);
        let ActorCommand::Protocol(cmd) = &commands[0] else {
            panic!("expected Protocol command, got {:?}", commands[0]);
        };
        let debug = format!("{cmd:?}");
        assert!(debug.contains("ResolveZapIdentifierCommand"));
        assert!(debug.contains("recipient_identifier: \"alice@example.com\""));
        assert!(debug.contains("amount_msats: 21000"));
        assert!(debug.contains("comment: Some(\"hi\")"));
    }
}
