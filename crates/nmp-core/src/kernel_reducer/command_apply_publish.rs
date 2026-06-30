//! Shared publish helpers for the headless command interpreter.

use super::command_apply::CommandApplyOutcome;
use super::KernelReducer;
use crate::publish::PublishTarget;
use nmp_signer_iface::UnsignedEvent;

impl KernelReducer {
    pub(super) fn begin_unsigned_publish_roundtrip(
        &mut self,
        event: UnsignedEvent,
        ownership: Option<nmp_ownership::EventOwnershipProvenance>,
        target: PublishTarget,
        correlation_id: Option<String>,
        is_group_host_pin: bool,
        label: &'static str,
    ) -> CommandApplyOutcome {
        use CommandApplyOutcome::{NeedsSign, Unsupported};

        if let Err(err) = nmp_ownership::validate_publish_ownership(
            event.kind,
            &event.tags,
            ownership,
            is_group_host_pin,
        ) {
            return Unsupported {
                reason: err.to_string(),
            };
        }
        let Some(account_pubkey) = self.active_account_pubkey() else {
            return Unsupported {
                reason: format!("no active account for {label} sign round-trip"),
            };
        };
        let created_at = if event.created_at == 0 {
            self.now_secs()
        } else {
            event.created_at
        };
        let unsigned_json = serde_json::json!({
            "pubkey": account_pubkey,
            "kind": event.kind,
            "tags": event.tags,
            "content": event.content,
            "created_at": created_at,
        })
        .to_string();
        match self.begin_sign_roundtrip_at(
            account_pubkey,
            &unsigned_json,
            crate::time::Instant::now(),
        ) {
            Ok(request) => NeedsSign {
                request,
                target,
                action_correlation_id: correlation_id,
            },
            Err(reason) => Unsupported { reason },
        }
    }
}
